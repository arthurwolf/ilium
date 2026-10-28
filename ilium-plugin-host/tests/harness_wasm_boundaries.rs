#![cfg(feature = "testing")]
//! Import-free component fixtures exercise the real runtime through the host harness.
//! Their narrow export types are not asserted to be a complete admitted plugin world.
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::intents::Intent;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::testing::harness::HostHarness;

// Canonical ABI fixtures adapted from wasm_guest_boundaries.rs: activate checks
// the real instance number; events checks actual list length and returns one
// close-node submission with node 13 and correlation 42 from guest memory.
const COMPONENT: &str = r#"(component

  (core module $activation_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (func (export "activate") (param i32 i32 i32 i32 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
      local.get 4 i64.const 1 i64.ne if unreachable end
      i32.const 0))
  (core instance $activation_m (instantiate $activation_m))
  (type $activation (record
    (field "plugin" string) (field "api" string) (field "instance" u64)
    (field "side" string) (field "settings-json" string)
    (field "granted" (list string)) (field "restored" (option (list u8)))))
  (type $code (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
  (type $error (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
  (func $activate (param "context" $activation) (result (result (error $error)))
    (canon lift (core func $activation_m "activate") (memory $activation_m "memory") (realloc (func $activation_m "realloc"))))
  (instance $lifecycle
    (export "activation" (type $activation)) (export "error-code" (type $code)) (export "plugin-error" (type $error))
    (export "activate" (func $activate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle))
  (core module $handler_m
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
  (core instance $handler_m (instantiate $handler_m))
  (type $event (variant (case "node-removed" u64)))
  (type $intent (variant (case "close-node" u64)))
  (type $submission (record (field "intent" $intent) (field "correlation" u64) (field "dedup-key" (option string))))
  (func $events (param "batch" (list $event)) (result (list $submission))
    (canon lift (core func $handler_m "events") (memory $handler_m "memory") (realloc (func $handler_m "realloc"))))
  (instance $handler (export "event" (type $event)) (export "intent" (type $intent)) (export "submission" (type $submission)) (export "on-events" (func $events)))
  (export "ilium:exports/handler@0.1.0" (instance $handler))
)"#;

#[test]
fn wat_component_activation_and_events_reach_real_kernel_dispatch() {
    let host = HostHarness::new()
        .grants(&["tree.read", "tree.write"])
        .with_wat(COMPONENT)
        .build()
        .expect("actual component builds");
    assert_eq!(host.state(), InstanceState::Active);
    host.deliver(vec![Event::NodeRemoved(NodeId(5))]);
    let report = host.pump(1);
    assert_eq!(report.events_delivered, 1);
    assert_eq!(report.intents_applied, 1);
    assert_eq!(report.traps, 0);
    let applied = host.kernel().applied_intents();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].1, Intent::CloseNode(NodeId(13)));
    let outcomes = host.outcomes(&PluginId("test.plugin".into()));
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].request, RequestRef::Submission(42));
    assert!(
        matches!(&outcomes[0].outcome, Outcome::Refused(error) if error.code == ErrorCode::NotFound)
    );
}

#[test]
fn wasm_bytes_use_the_same_real_component_path() {
    let bytes = wat::parse_str(COMPONENT).unwrap();
    let host = HostHarness::new().with_wasm_bytes(&bytes).build().unwrap();
    assert_eq!(host.state(), InstanceState::Active);
    assert!(host.failures().is_empty());
}

#[test]
fn activation_unreachable_is_a_counted_real_guest_trap() {
    let text = COMPONENT.replace(
        "local.get 4 i64.const 1 i64.ne if unreachable end",
        "unreachable",
    );
    assert_ne!(text, COMPONENT);
    let host = HostHarness::new().with_wat(&text).build().unwrap();
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    assert_eq!(host.failure_count(), 1);
    assert_eq!(host.failures()[0].code.as_str(), "trap.panic");
    assert!(host.kernel().applied_intents().is_empty());
}

#[test]
fn event_trap_discards_instance_without_applying_submissions() {
    let text = COMPONENT.replace(
        "local.get 1 i32.const 1 i32.ne if unreachable end",
        "unreachable",
    );
    assert_ne!(text, COMPONENT);
    let host = HostHarness::new()
        .grants(&["tree.read", "tree.write"])
        .with_wat(&text)
        .build()
        .unwrap();
    assert_eq!(host.state(), InstanceState::Active);
    host.deliver(vec![Event::NodeRemoved(NodeId(5))]);
    assert_eq!(host.pump(1).traps, 1);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    assert_eq!(host.failures()[0].code.as_str(), "trap.panic");
    assert!(host.kernel().applied_intents().is_empty());
    assert!(host.outcomes(&PluginId("test.plugin".into())).is_empty());
}

#[test]
fn imports_and_invalid_or_ambiguous_sources_fail_closed() {
    let error = HostHarness::new()
        .with_wat("(component (import \"ambient\" (func)))")
        .build()
        .unwrap_err();
    assert!(error.to_string().contains("effective-grant"));
    assert!(HostHarness::new()
        .with_wat("invalid component text")
        .build()
        .is_err());
    assert!(HostHarness::new()
        .with_wasm_bytes(b"not wasm")
        .build()
        .is_err());
    assert!(HostHarness::new()
        .with_wat(COMPONENT)
        .with_wasm_bytes(b"ignored?")
        .build()
        .unwrap_err()
        .to_string()
        .contains("exactly one"));
}

#[test]
fn missing_lifecycle_export_is_an_observed_abi_trap() {
    let host = HostHarness::new().with_wat("(component)").build().unwrap();
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    assert_eq!(host.failures()[0].code.as_str(), "trap.abi");
}
