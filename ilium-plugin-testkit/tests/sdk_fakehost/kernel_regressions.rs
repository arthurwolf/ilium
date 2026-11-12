//! Focused native setup/lifecycle regressions. These do not qualify imports or component execution.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::host::{Bucket, InstanceState};
use ilium_plugin_sdk_core::types::{
    Activation, DeactivationReason, ErrorCode, Event, PluginError, Submission,
};
use ilium_plugin_sdk_core::NativeEntry;
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::{FakeHost, Mode};

use crate::common::{
    assert_fails_with, manifest_text, spy, spy_reset, strings, temp_dir, with_clean_env, with_env,
    write_file, Spy,
};

/// Rc intentionally makes this plugin non-Send: native ownership needs 'static, not Send.
#[derive(Default)]
struct OwnedProbe {
    value: u8,
    context: Option<Activation>,
    reasons: Rc<RefCell<Vec<DeactivationReason>>>,
    drops: Rc<Cell<u32>>,
    panic_on_deactivate: bool,
}

impl Drop for OwnedProbe {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

impl NativeEntry for OwnedProbe {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.value += 1;
        self.context = Some(context);
        Ok(())
    }

    fn native_deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.reasons.borrow_mut().push(reason);
        assert!(!self.panic_on_deactivate, "deactivation callback panicked");
        Some(vec![self.value])
    }

    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        panic!("a lifecycle-only change must not deliver an event")
    }
}

#[derive(Default)]
struct RejectActivation;

impl NativeEntry for RejectActivation {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Err(PluginError {
            code: ErrorCode::NotPermitted,
            message: "activation denied exactly".to_owned(),
            retry_after_ms: Some(42),
        })
    }

    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        panic!("a failed activation is dropped without deactivate")
    }

    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[derive(Default)]
struct RejectReactivation;

impl NativeEntry for RejectReactivation {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, context: Activation) -> Result<(), PluginError> {
        if context.instance > 1 {
            Err(PluginError::new(
                ErrorCode::NotPermitted,
                "new grants rejected",
            ))
        } else {
            Ok(())
        }
    }

    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

fn scene_manifest() -> String {
    let mut value: serde_json::Value = serde_json::from_str(&manifest_text(
        "example.scene",
        &["tree.read"],
        &["ui.panels"],
    ))
    .expect("fixture JSON");
    value["scene"] = serde_json::json!({
        "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": []
    });
    value["capabilities"]["scene"] = serde_json::json!(["location.observer"]);
    value.to_string()
}

#[test]
fn kernel_manifest_helper_uses_implicit_required_and_only_optional_exports() {
    let text = manifest_text("example.fixture", &["tree.read"], &["ui.panels"]);
    let manifest = ilium_plugin_api::manifest::parse_manifest(&text).expect("canonical fixture");
    assert!(manifest.server.expect("server").exports.is_empty());
    assert_eq!(
        manifest.client.expect("client").exports,
        strings(&["views"])
    );
    let invalid = text.replacen("\"exports\": []", "\"exports\": [\"handler\"]", 1);
    let errors = ilium_plugin_api::manifest::parse_manifest(&invalid)
        .expect_err("handler is required, not optional");
    assert!(errors
        .iter()
        .any(|error| error.code.as_str() == "manifest.schema"
            && error.pointer == "/server/exports/0"));
}

#[test]
fn kernel_manifest_unreadable_path_has_a_specific_harness_diagnostic() {
    with_clean_env(|| {
        let path = temp_dir("kernel-unreadable").join("absent.json");
        let error = FakeHost::for_manifest(&path).expect_err("missing input");
        assert_eq!(error.code, codes::MANIFEST_READ);
        assert!(error.message.contains("absent.json"));
    });
}

#[test]
fn kernel_manifest_bad_json_and_non_utf8_are_json_invalid() {
    with_clean_env(|| {
        let dir = temp_dir("kernel-json");
        let path = write_file(&dir, "bad.json", "{\"api\":");
        assert_eq!(
            FakeHost::for_manifest(path).expect_err("bad JSON").code,
            "manifest.json-invalid"
        );
        let path = dir.join("bytes.json");
        std::fs::write(&path, [0xff, 0x00]).expect("invalid UTF-8 fixture");
        assert_eq!(
            FakeHost::for_manifest(path).expect_err("bad encoding").code,
            "manifest.json-invalid"
        );
    });
}

#[test]
fn kernel_manifest_duplicate_key_keeps_canonical_code_and_pointer() {
    with_clean_env(|| {
        let text = manifest_text("example.original", &[], &[]).replacen(
            "\"id\": \"example.original\",",
            "\"id\": \"example.original\", \"id\": \"example.duplicate\",",
            1,
        );
        let path = write_file(&temp_dir("kernel-duplicate"), "manifest.json", &text);
        let error = FakeHost::for_manifest(path).expect_err("duplicate source key");
        assert_eq!(error.code, "manifest.duplicate-key");
        // The canonical parser locates a duplicate at its containing object.
        let findings = ilium_plugin_api::manifest::parse_manifest(&text)
            .expect_err("canonical duplicate diagnostic");
        let finding = &findings[0];
        assert!(error.message.contains(&format!(
            "{} {}: {}",
            finding.code, finding.pointer, finding.message
        )));
    });
}

#[test]
fn kernel_manifest_schema_error_preserves_path_and_does_not_accept_serde_only_input() {
    with_clean_env(|| {
        let mut value: serde_json::Value =
            serde_json::from_str(&manifest_text("example.invalid", &[], &[])).expect("fixture");
        value["server"]["exports"] = serde_json::json!(["handler"]);
        let path = write_file(
            &temp_dir("kernel-schema"),
            "manifest.json",
            &value.to_string(),
        );
        let error = FakeHost::for_manifest(path).expect_err("required exports are not optional");
        assert_eq!(error.code, "manifest.schema");
        assert!(error.message.contains("/server/exports/0"));
    });
}

#[test]
fn kernel_manifest_duplicate_grants_are_rejected_by_schema() {
    with_clean_env(|| {
        let text = manifest_text("example.duplicate-grant", &["tree.read", "tree.read"], &[]);
        let path = write_file(&temp_dir("kernel-duplicate-grant"), "manifest.json", &text);
        let error = FakeHost::for_manifest(path).expect_err("schema uniqueItems");
        assert_eq!(error.code, "manifest.schema");
        assert!(error.message.contains("/capabilities/server"));
    });
}

#[test]
fn kernel_scene_side_uses_only_its_declared_grants() {
    with_clean_env(|| {
        let path = write_file(
            &temp_dir("kernel-scene"),
            "manifest.json",
            &scene_manifest(),
        );
        let mut host = FakeHost::for_manifest(path)
            .expect("manifest")
            .side("scene");
        host.activate::<OwnedProbe>();
        let context = host
            .instance_mut::<OwnedProbe>()
            .context
            .as_ref()
            .expect("real activation");
        assert_eq!(context.plugin.0, "example.scene");
        assert_eq!(context.side, "scene");
        assert_eq!(context.granted, strings(&["location.observer"]));
        assert_eq!(
            host.grant_now("tree.read")
                .expect_err("different side")
                .code,
            codes::GRANT_NOT_DECLARED
        );
    });
}

#[test]
fn kernel_scene_only_manifest_selects_scene_and_absent_sides_are_refused() {
    with_clean_env(|| {
        let mut value: serde_json::Value =
            serde_json::from_str(&scene_manifest()).expect("fixture");
        let object = value.as_object_mut().expect("object");
        object.remove("server");
        object.remove("client");
        let capabilities = object
            .get_mut("capabilities")
            .expect("capabilities")
            .as_object_mut()
            .expect("object");
        capabilities.remove("server");
        capabilities.remove("client");
        let text = value.to_string();
        let path = write_file(&temp_dir("kernel-scene-only"), "manifest.json", &text);
        assert_fails_with(codes::CONFIG_INVALID, || {
            FakeHost::for_manifest(&path)
                .expect("manifest")
                .side("client")
        });
        let mut host = FakeHost::for_manifest(path).expect("scene manifest");
        assert_eq!(host.activate::<OwnedProbe>().side, "scene");
    });
}

#[test]
fn kernel_duplicate_builder_grants_form_a_sorted_set() {
    with_clean_env(|| {
        let mut host =
            FakeHost::new()
                .grant("tree.read")
                .grants(&["sound.play", "tree.read", "agent.read"]);
        host.activate::<OwnedProbe>();
        assert_eq!(
            host.instance_mut::<OwnedProbe>()
                .context
                .as_ref()
                .expect("context")
                .granted,
            strings(&["agent.read", "sound.play", "tree.read"])
        );
    });
}

#[test]
fn kernel_build_closes_each_builder_but_still_allows_activation() {
    with_clean_env(|| {
        let builders: [fn(FakeHost) -> FakeHost; 8] = [
            |host| host.grant("tree.read"),
            |host| host.grants(&["tree.read"]),
            |host| host.tier("T2"),
            |host| host.side("client"),
            |host| host.platform("windows"),
            |host| host.seed(2),
            FakeHost::build,
            |host| host.mode(Mode::Native),
        ];
        for builder in builders {
            assert_fails_with(codes::SETUP_CLOSED, || builder(FakeHost::new().build()));
        }
        let mut host = FakeHost::new().build();
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.activate::<OwnedProbe>().instance, 1);
    });
}

#[test]
fn kernel_invalid_setup_names_are_refused() {
    with_clean_env(|| {
        assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().tier("t1"));
        assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().side("worker"));
        assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().platform("darwin"));
    });
}

#[test]
fn kernel_invalid_environment_seed_is_rejected_and_explicit_seed_repairs_it() {
    for value in ["", "-1", "+1", " 2", "two", "18446744073709551616"] {
        with_env(&[("ILIUM_PLUGIN_TEST_SEED", value)], || {
            assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().current_seed());
            assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().build());
            let mut host = FakeHost::new().seed(7);
            assert_eq!(host.current_seed(), 7);
            assert_eq!(host.activate::<OwnedProbe>().instance, 1);
        });
    }
}

#[test]
fn kernel_invalid_environment_mode_is_rejected_and_explicit_mode_repairs_it() {
    for value in ["", "Native", "wasm", " component"] {
        with_env(&[("ILIUM_PLUGIN_TEST_MODE", value)], || {
            assert_fails_with(codes::CONFIG_INVALID, || FakeHost::new().current_mode());
            assert_fails_with(codes::CONFIG_INVALID, || {
                FakeHost::new().activate::<OwnedProbe>()
            });
            let mut host = FakeHost::new().mode(Mode::Native);
            assert_eq!(host.activate::<OwnedProbe>().instance, 1);
        });
    }
}

#[test]
fn kernel_component_selection_never_falls_back_to_a_native_callback() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().mode(Mode::Component);
        assert_eq!(host.current_mode(), Mode::Component);
        assert_eq!(
            host.try_activate::<Spy>()
                .expect_err("no component backend")
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(spy().constructed, 0);
        assert!(spy().activations.is_empty());
        assert_eq!(host.snapshot().instance, 0);
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert!(host.failures().is_empty());
        assert_fails_with(codes::SETUP_CLOSED, || host.mode(Mode::Native));
    });
}

#[test]
fn kernel_mutated_owned_value_is_the_value_deactivated_and_dropped() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.activate::<OwnedProbe>();
        let probe = host.instance_mut::<OwnedProbe>();
        let reasons = probe.reasons.clone();
        let drops = probe.drops.clone();
        probe.value = 73;
        assert_eq!(
            host.deactivate(DeactivationReason::Shutdown),
            Some(vec![73])
        );
        assert_eq!(*reasons.borrow(), vec![DeactivationReason::Shutdown]);
        assert_eq!(drops.get(), 1);
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_fails_with(codes::NOT_ACTIVE, || {
            host.instance_mut::<OwnedProbe>();
        });
        assert_eq!(host.activate::<OwnedProbe>().instance, 2);
        assert_eq!(host.instance_mut::<OwnedProbe>().value, 1);
        assert_eq!(host.restored(), None);
    });
}

#[test]
fn kernel_grant_change_deactivates_live_value_and_constructs_a_fresh_one() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grant("tree.read");
        host.activate::<OwnedProbe>();
        let probe = host.instance_mut::<OwnedProbe>();
        let reasons = probe.reasons.clone();
        let drops = probe.drops.clone();
        probe.value = 99;
        host.grant_now("sound.play").expect("approved grant");
        assert_eq!(*reasons.borrow(), vec![DeactivationReason::Granted]);
        assert_eq!(drops.get(), 1);
        let probe = host.instance_mut::<OwnedProbe>();
        assert_eq!(probe.value, 1);
        let context = probe.context.as_ref().expect("fresh activation");
        assert_eq!(context.instance, 2);
        assert_eq!(context.granted, strings(&["sound.play", "tree.read"]));
        assert_eq!(context.restored, None);
        let reasons = probe.reasons.clone();
        host.revoke("tree.read");
        assert_eq!(*reasons.borrow(), vec![DeactivationReason::Revoked]);
        assert_eq!(host.snapshot().instance, 3);
        assert_eq!(host.instance_mut::<OwnedProbe>().value, 1);
    });
}

#[test]
fn kernel_mixed_invalid_runtime_list_is_atomic_for_grants_instance_and_plugin() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grant("tree.read");
        host.activate::<OwnedProbe>();
        host.instance_mut::<OwnedProbe>().value = 87;
        assert_fails_with(codes::GRANT_UNKNOWN, || {
            host.grants_changed(&["sound.play", "not.a.real.grant"])
        });
        assert_eq!(host.snapshot().instance, 1);
        let probe = host.instance_mut::<OwnedProbe>();
        assert_eq!(probe.value, 87);
        assert!(probe.reasons.borrow().is_empty());
        assert_eq!(
            probe.context.as_ref().expect("original context").granted,
            strings(&["tree.read"])
        );
        host.grants_changed(&["tree.read"]);
        let probe = host.instance_mut::<OwnedProbe>();
        assert_eq!(probe.context.as_ref().expect("new context").instance, 2);
        assert_eq!(
            probe.context.as_ref().expect("new context").granted,
            strings(&["tree.read"])
        );
    });
}

#[test]
fn kernel_mixed_undeclared_runtime_list_is_atomic() {
    with_clean_env(|| {
        let path = write_file(
            &temp_dir("kernel-undeclared"),
            "manifest.json",
            &manifest_text("example.declared", &["tree.read"], &[]),
        );
        let mut host = FakeHost::for_manifest(path).expect("manifest");
        host.activate::<OwnedProbe>();
        host.instance_mut::<OwnedProbe>().value = 57;
        assert_fails_with(codes::GRANT_NOT_DECLARED, || {
            host.grants_changed(&["tree.read", "sound.play"])
        });
        assert_eq!(host.snapshot().instance, 1);
        assert_eq!(host.instance_mut::<OwnedProbe>().value, 57);
        assert_eq!(
            host.grant_now("sound.play").expect_err("not declared").code,
            codes::GRANT_NOT_DECLARED
        );
        assert_eq!(host.snapshot().instance, 1);
    });
}

#[test]
fn kernel_grants_changed_uses_sorted_activation_order_even_for_only_adds() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grants(&["tree.read", "sound.play"]);
        host.activate::<OwnedProbe>();
        let reasons = host.instance_mut::<OwnedProbe>().reasons.clone();
        host.grants_changed(&["tree.read", "sound.play", "agent.read"]);
        assert_eq!(*reasons.borrow(), vec![DeactivationReason::Granted]);
        assert_eq!(
            host.instance_mut::<OwnedProbe>()
                .context
                .as_ref()
                .expect("context")
                .granted,
            strings(&["agent.read", "sound.play", "tree.read"])
        );
    });
}

#[test]
fn kernel_restart_supplies_only_explicit_bytes_and_retains_the_factory() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.activate::<OwnedProbe>();
        let probe = host.instance_mut::<OwnedProbe>();
        let reasons = probe.reasons.clone();
        let drops = probe.drops.clone();
        probe.value = 99;
        host.restart_with(vec![4, 5, 6]);
        assert_eq!(*reasons.borrow(), vec![DeactivationReason::Upgrade]);
        assert_eq!(drops.get(), 1);
        assert_eq!(host.instance_mut::<OwnedProbe>().value, 1);
        assert_eq!(host.restored(), Some(vec![4, 5, 6]));
        assert_eq!(host.snapshot().instance, 2);
        host.grant_now("tree.read").expect("grant");
        assert_eq!(host.restored(), None);
    });
}

#[test]
fn kernel_activation_errors_preserve_all_plugin_error_fields_and_count_attempts() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        let error = host.try_activate::<RejectActivation>().expect_err("reject");
        assert_eq!(error.code, ErrorCode::NotPermitted);
        assert_eq!(error.message, "activation denied exactly");
        assert_eq!(error.retry_after_ms, Some(42));
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.failures().len(), 1);
        assert_eq!(host.failures()[0].code, "not-permitted");
        assert_eq!(host.activate::<OwnedProbe>().instance, 2);
    });
}

#[test]
fn kernel_grant_reactivation_failure_commits_grants_and_leaves_no_active_value() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.activate::<RejectReactivation>();
        let error = host
            .grant_now("tree.read")
            .expect_err("new callback refused");
        assert_eq!(error.code, codes::ACTIVATION_FAILED);
        assert!(error.message.contains("not-permitted: new grants rejected"));
        assert_eq!(host.snapshot().instance, 2);
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.failures().len(), 1);
        assert_eq!(host.activate::<OwnedProbe>().instance, 3);
        assert_eq!(
            host.instance_mut::<OwnedProbe>()
                .context
                .as_ref()
                .expect("context")
                .granted,
            strings(&["tree.read"])
        );
    });
}

#[test]
fn kernel_reserved_reason_and_wrong_type_preserve_the_active_value() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.activate::<OwnedProbe>();
        host.instance_mut::<OwnedProbe>().value = 88;
        assert_fails_with(codes::REASON_RESERVED, || {
            host.deactivate(DeactivationReason::CrashRestart)
        });
        assert_fails_with(codes::INSTANCE_TYPE, || {
            host.instance_mut::<Spy>();
        });
        assert_eq!(host.instance_mut::<OwnedProbe>().value, 88);
        assert_eq!(host.lifecycle_state(), InstanceState::Active);
    });
}

#[test]
fn kernel_hosts_isolate_live_values_storage_snapshots_and_clock() {
    with_clean_env(|| {
        let mut first = FakeHost::new();
        let mut second = FakeHost::new();
        first.activate::<OwnedProbe>();
        second.activate::<OwnedProbe>();
        first.instance_mut::<OwnedProbe>().value = 11;
        second.instance_mut::<OwnedProbe>().value = 22;
        first.storage_mut().put(Bucket::Global, "token", &[1, 2]);
        let snapshot = first.snapshot();
        first.storage_mut().put(Bucket::Global, "token", &[3]);
        first.advance_clock(50);
        assert_eq!(first.clock().now(), 1_893_456_000_050);
        assert_eq!(second.clock().now(), 1_893_456_000_000);
        assert_eq!(second.storage().get(Bucket::Global, "token"), None);
        assert_eq!(snapshot.storage["global"]["token"], vec![1, 2]);
        assert_eq!(
            first.deactivate(DeactivationReason::Shutdown),
            Some(vec![11])
        );
        assert_eq!(
            second.deactivate(DeactivationReason::Shutdown),
            Some(vec![22])
        );
        assert_eq!(first.storage().get(Bucket::Global, "token"), Some(vec![3]));
    });
}

#[test]
fn kernel_parallel_hosts_construct_independent_non_send_plugin_values() {
    with_clean_env(|| {
        let threads: Vec<_> = [13, 29]
            .into_iter()
            .map(|value| {
                std::thread::spawn(move || {
                    let mut host = FakeHost::new();
                    host.activate::<OwnedProbe>();
                    host.instance_mut::<OwnedProbe>().value = value;
                    host.deactivate(DeactivationReason::Shutdown)
                })
            })
            .collect();
        let values: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("host thread"))
            .collect();
        assert_eq!(values, vec![Some(vec![13]), Some(vec![29])]);
    });
}

#[test]
fn kernel_clock_rejects_backwards_and_overflow_without_changing_time() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        assert_fails_with(codes::CLOCK_BACKWARDS, || host.set_clock(0));
        assert_eq!(host.clock().now(), 1_893_456_000_000);
        host.set_clock(u64::MAX);
        assert_fails_with(codes::CLOCK_ARGUMENT, || host.advance_clock(1));
        assert_eq!(host.clock().now(), u64::MAX);
    });
}

#[test]
fn kernel_failed_activation_closes_the_builder() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.try_activate::<RejectActivation>().expect_err("reject");
        assert_fails_with(codes::SETUP_CLOSED, || host.seed(7));
    });
}

#[test]
fn kernel_panicking_deactivation_drops_the_owned_value_and_clears_active_state() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        host.activate::<OwnedProbe>();
        let probe = host.instance_mut::<OwnedProbe>();
        let drops = probe.drops.clone();
        probe.panic_on_deactivate = true;
        assert_fails_with("deactivation callback panicked", || {
            host.deactivate(DeactivationReason::Shutdown)
        });
        assert_eq!(drops.get(), 1);
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.activate::<OwnedProbe>().instance, 2);
    });
}

#[test]
fn kernel_runtime_grant_calls_configure_an_inactive_built_host_without_activation() {
    with_clean_env(|| {
        let mut host = FakeHost::new().build();
        host.grant_now("tree.read")
            .expect("grant before activation");
        host.grants_changed(&["sound.play", "agent.read"]);
        host.revoke("sound.play");
        assert_eq!(host.snapshot().instance, 0);
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        host.activate::<OwnedProbe>();
        assert_eq!(
            host.instance_mut::<OwnedProbe>()
                .context
                .as_ref()
                .expect("context")
                .granted,
            strings(&["agent.read"])
        );
    });
}

#[test]
fn kernel_revocation_reactivation_failure_uses_the_harness_diagnostic() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grant("tree.read");
        host.activate::<RejectReactivation>();
        assert_fails_with(codes::ACTIVATION_FAILED, || host.revoke("tree.read"));
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.snapshot().instance, 2);
        host.activate::<OwnedProbe>();
        assert!(host
            .instance_mut::<OwnedProbe>()
            .context
            .as_ref()
            .expect("context")
            .granted
            .is_empty());
    });
}

#[test]
fn kernel_grant_replacement_reactivation_failure_uses_the_harness_diagnostic() {
    with_clean_env(|| {
        let mut host = FakeHost::new().grant("tree.read");
        host.activate::<RejectReactivation>();
        assert_fails_with(codes::ACTIVATION_FAILED, || {
            host.grants_changed(&["sound.play"])
        });
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert_eq!(host.snapshot().instance, 2);
        host.activate::<OwnedProbe>();
        assert_eq!(
            host.instance_mut::<OwnedProbe>()
                .context
                .as_ref()
                .expect("context")
                .granted,
            strings(&["sound.play"])
        );
    });
}
