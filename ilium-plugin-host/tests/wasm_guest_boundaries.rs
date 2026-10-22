#![cfg(feature = "testing")]

use std::collections::BTreeMap;
use std::sync::Arc;

use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::calls::ResolvedLimits;
use ilium_plugin_host::engine::{EngineConfig, PluginEngine};
use ilium_plugin_host::instance::GuestExports;
use ilium_plugin_host::wasm_guest::WasmGuest;

fn engine() -> Arc<PluginEngine> {
    Arc::new(
        PluginEngine::new(EngineConfig {
            epoch_tick_ms: 10,
            cache_dir: None,
            process: ProcessKind::Server,
        })
        .expect("real engine"),
    )
}

fn compile(engine: &PluginEngine, text: &str) -> ilium_plugin_host::engine::CompiledComponent {
    engine
        .compile(&ilium_plugin_host::testing::wat_component(text))
        .expect("real component")
}

#[test]
fn rejects_unlinked_imports_before_instantiation() {
    let engine = engine();
    let compiled = compile(&engine, "(component (import \"ambient\" (func)))");
    let result = WasmGuest::instantiate(engine, &compiled, ResolvedLimits(BTreeMap::new()));
    assert!(result
        .err()
        .expect("imports denied")
        .to_string()
        .contains("effective-grant"));
}

#[test]
fn refuses_a_component_from_another_engine() {
    let first = engine();
    let compiled = compile(&first, "(component)");
    assert!(WasmGuest::instantiate(engine(), &compiled, ResolvedLimits(BTreeMap::new())).is_err());
}

#[test]
fn missing_export_is_an_abi_trap_not_simulated_success() {
    let engine = engine();
    let compiled = compile(&engine, "(component)");
    let mut guest = WasmGuest::instantiate(engine, &compiled, ResolvedLimits(BTreeMap::new()))
        .expect("instantiate");
    let error = guest
        .activate(Activation::default())
        .expect_err("missing lifecycle");
    assert_eq!(error.code.as_str(), "trap.abi");
    assert_eq!(error.function, "activate");
}

// This deliberately narrow real component exercises canonical enum lowering,
// option/list lifting and persistent guest memory. Package/world validation is a
// separate gate: the fixture is not presented as a complete plugin world.
const DEACTIVATE: &str = r#"
(component
  (core module $m
    (memory (export "memory") 1)
    (data (i32.const 0) "\01\00\00\00\10\00\00\00\03\00\00\00")
    (data (i32.const 16) "\07\08\09")
    (func (export "deactivate") (param i32) (result i32)
      local.get 0 i32.const 0 i32.ne
      if unreachable end
      i32.const 0))
  (core instance $m (instantiate $m))
  (type $reason (enum "shutdown" "disabled" "upgrade" "crash-restart" "revoked" "granted"))
  (func $deactivate (param "reason" $reason) (result (option (list u8)))
    (canon lift (core func $m "deactivate") (memory $m "memory")))
  (instance $lifecycle (export "deactivation-reason" (type $reason)) (export "deactivate" (func $deactivate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle)))
"#;

#[test]
fn executes_real_canonical_deactivate_and_lifts_saved_bytes() {
    let engine = engine();
    let compiled = compile(&engine, DEACTIVATE);
    let mut guest = WasmGuest::instantiate(engine, &compiled, ResolvedLimits(BTreeMap::new()))
        .expect("instantiate");
    assert_eq!(
        guest
            .deactivate(DeactivationReason::Shutdown)
            .expect("real export"),
        Some(vec![7, 8, 9])
    );
    // The core function checks the actual enum discriminant and traps for a
    // different reason, proving this is guest execution rather than a canned reply.
    assert_eq!(
        guest
            .deactivate(DeactivationReason::Disabled)
            .expect_err("guest unreachable")
            .code
            .as_str(),
        "trap.panic"
    );
}

#[test]
fn initial_memory_is_subject_to_resolved_store_limits() {
    use ilium_plugin_api::limits::LimitKey;
    let engine = engine();
    let compiled = compile(&engine, DEACTIVATE);
    let limits = ResolvedLimits(BTreeMap::from([(LimitKey::MemoryMib, 0)]));
    assert!(WasmGuest::instantiate(engine, &compiled, limits).is_err());
}

const HANDLER_COMPONENT: &str = r#"
(component
  (core module $m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (data (i32.const 0) "\20\00\00\00\01\00\00\00")
    (data (i32.const 40) "\0d\00\00\00\00\00\00\00\2a\00\00\00\00\00\00\00")
    (func (export "events") (param i32 i32) (result i32)
      local.get 1 i32.const 1 i32.ne if unreachable end
      i32.const 0))
  (core instance $m (instantiate $m))
  (type $event (variant (case "node-removed" u64)))
  (type $intent (variant (case "close-node" u64)))
  (type $submission (record (field "intent" $intent) (field "correlation" u64) (field "dedup-key" (option string))))
  (func $events (param "batch" (list $event)) (result (list $submission))
    (canon lift (core func $m "events") (memory $m "memory") (realloc (func $m "realloc"))))
  (instance $handler (export "event" (type $event)) (export "intent" (type $intent)) (export "submission" (type $submission)) (export "on-events" (func $events)))
  (export "ilium:exports/handler@0.1.0" (instance $handler)))
"#;

#[test]
fn real_handler_lowers_generated_events_and_lifts_generated_submissions() {
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::intents::Intent;
    let engine = engine();
    let compiled = compile(&engine, HANDLER_COMPONENT);
    let mut guest = WasmGuest::instantiate(engine, &compiled, ResolvedLimits(BTreeMap::new()))
        .expect("instantiate");
    let submissions = guest
        .on_events(vec![Event::NodeRemoved(NodeId(5))])
        .expect("actual handler");
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].intent, Intent::CloseNode(NodeId(13)));
    assert_eq!(submissions[0].correlation, 42);
    assert_eq!(submissions[0].dedup_key, None);
}

const ACTIVATE_COMPONENT: &str = r#"
(component
  (core module $m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (func (export "activate") (param i32 i32 i32 i32 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
      local.get 4 i64.const 9 i64.ne if unreachable end
      i32.const 0))
  (core instance $m (instantiate $m))
  (type $activation (record
    (field "plugin" string) (field "api" string) (field "instance" u64)
    (field "side" string) (field "settings-json" string)
    (field "granted" (list string)) (field "restored" (option (list u8)))))
  (type $code (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
  (type $error (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
  (func $activate (param "context" $activation) (result (result (error $error)))
    (canon lift (core func $m "activate") (memory $m "memory") (realloc (func $m "realloc"))))
  (instance $lifecycle
    (export "activation" (type $activation)) (export "error-code" (type $code)) (export "plugin-error" (type $error))
    (export "activate" (func $activate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle)))
"#;

#[test]
fn real_activation_lowers_records_strings_lists_options_and_u64() {
    let engine = engine();
    let compiled = compile(&engine, ACTIVATE_COMPONENT);
    let mut guest = WasmGuest::instantiate(engine, &compiled, ResolvedLimits(BTreeMap::new()))
        .expect("instantiate");
    let activation = Activation {
        instance: 9,
        api: "ilium:plugin@0.1.0".into(),
        side: "server".into(),
        settings_json: "{}".into(),
        granted: vec!["tree.read".into()],
        restored: Some(vec![1, 2]),
        ..Activation::default()
    };
    assert_eq!(
        guest.activate(activation).expect("actual activation"),
        Ok(())
    );
}

#[test]
fn actual_infinite_guest_call_is_interrupted_by_resolved_deadline() {
    use ilium_plugin_api::limits::LimitKey;
    let engine = engine();
    let text = DEACTIVATE.replace(
        "local.get 0 i32.const 0 i32.ne\n      if unreachable end",
        "(loop $forever br $forever)",
    );
    let compiled = compile(&engine, &text);
    let limits = ResolvedLimits(BTreeMap::from([(LimitKey::DeactivateDeadlineMs, 1)]));
    let mut guest = WasmGuest::instantiate(engine, &compiled, limits).expect("instantiate");
    assert_eq!(
        guest
            .deactivate(DeactivationReason::Shutdown)
            .expect_err("epoch interruption")
            .code
            .as_str(),
        "trap.deadline"
    );
}

#[test]
fn actual_guest_memory_growth_past_limit_has_memory_trap_code() {
    use ilium_plugin_api::limits::LimitKey;
    let engine = engine();
    let text = DEACTIVATE.replace(
        "local.get 0 i32.const 0 i32.ne\n      if unreachable end",
        "i32.const 100 memory.grow drop",
    );
    let compiled = compile(&engine, &text);
    let limits = ResolvedLimits(BTreeMap::from([(LimitKey::MemoryMib, 1)]));
    let mut guest = WasmGuest::instantiate(engine, &compiled, limits).expect("instantiate");
    assert_eq!(
        guest
            .deactivate(DeactivationReason::Shutdown)
            .expect_err("memory quota")
            .code
            .as_str(),
        "trap.memory"
    );
}
