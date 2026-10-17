//! reference-test-harness.md "Activation" and "Isolation between tests": DEV-420 to DEV-429.
//!
//! DEV-422, DEV-427 and DEV-429 describe the Bun and TypeScript harness; they are listed as
//! skipped in the lane report. DEV-423 has only its native half here; the component half needs a
//! built `.wasm` fixture (API_GAPS.md).

use ilium_plugin_sdk_core::types::{Activation, DeactivationReason, ErrorCode, PluginId};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::FakeHost;

use crate::common::{
    assert_fails_with, client_batch, panic_text, sound, spy, spy_edit, spy_reset, spy_script,
    strings, temp_dir, with_clean_env, write_file, Failing, Spy, DEFAULT_CLOCK_MS,
};

fn fixture_with_settings(label: &str, body: &str) -> std::path::PathBuf {
    let dir = temp_dir(label);
    write_file(&dir, "fixture.json", body)
}

// ---- DEV-420 ---------------------------------------------------------------------------------

/// REQ: DEV-420
#[test]
fn dev_420_new_host_activation_record_has_every_documented_field() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grants(&["tree.write", "agent.read"]);
        host.activate::<Spy>();
        let expected = Activation {
            plugin: PluginId("test.plugin".to_string()),
            api: "ilium:plugin@0.1.0".to_string(),
            instance: 1,
            side: "server".to_string(),
            settings_json: "{}".to_string(),
            granted: strings(&["agent.read", "tree.write"]),
            restored: None,
        };
        assert_eq!(spy().activations, vec![expected]);
    });
}

/// REQ: DEV-420
#[test]
fn dev_420_fixture_side_and_settings_reach_the_activation() {
    with_clean_env(|| {
        let path = fixture_with_settings(
            "f420",
            r#"{ "schema": 1, "side": "client", "settings": { "mode": "fast", "limit": 3 } }"#,
        );
        spy_reset();
        let mut host = FakeHost::new().with_fixture(&path);
        host.activate::<Spy>();
        let log = spy();
        let activation = &log.activations[0];
        assert_eq!(activation.side, "client");
        let parsed: serde_json::Value =
            serde_json::from_str(&activation.settings_json).expect("settings_json is JSON");
        assert_eq!(parsed, serde_json::json!({ "mode": "fast", "limit": 3 }));
    });
}

/// REQ: DEV-420
#[test]
fn dev_420_instance_grows_by_one_per_reactivation_and_restored_is_none_until_restart_with() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.grant_now("tree.read").expect("grant");
        host.revoke("tree.read");
        let log = spy();
        let instances: Vec<u64> = log.activations.iter().map(|a| a.instance).collect();
        assert_eq!(instances, vec![1, 2, 3]);
        assert!(log.activations.iter().all(|a| a.restored.is_none()));
        host.restart_with(vec![9, 8, 7]);
        let log = spy();
        assert_eq!(log.activations[3].instance, 4);
        assert_eq!(log.activations[3].restored, Some(vec![9, 8, 7]));
    });
}

/// REQ: DEV-420
#[test]
fn dev_420_activate_returns_a_handle_with_side_and_instance() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        let handle = host.activate::<Spy>();
        assert_eq!(handle.side, "server");
        assert_eq!(handle.instance, 1);
    });
}

/// REQ: DEV-420
#[test]
fn dev_420_a_second_activate_on_one_host_fails_with_already_active() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_fails_with(codes::ALREADY_ACTIVE, || host.activate::<Spy>());
    });
}

// ---- DEV-421 ---------------------------------------------------------------------------------

/// REQ: DEV-421
#[test]
fn dev_421_activate_panics_with_the_plugin_error_code_and_message() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        let text = panic_text(|| host.activate::<Failing>()).expect("activate must panic");
        assert!(text.contains("not-permitted"), "{text}");
        assert!(text.contains("denied"), "{text}");
    });
}

/// REQ: DEV-421
#[test]
fn dev_421_try_activate_returns_the_error_instead_of_panicking() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        let error = host
            .try_activate::<Failing>()
            .expect_err("the plugin refuses");
        assert_eq!(error.code, ErrorCode::NotPermitted);
        assert_eq!(error.message, "denied");
    });
}

/// REQ: DEV-421
#[test]
fn dev_421_try_activate_returns_the_handle_when_the_plugin_succeeds() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        let handle = host.try_activate::<Spy>().expect("activation succeeds");
        assert_eq!(handle.instance, 1);
    });
}

// ---- DEV-423 (native half) -------------------------------------------------------------------

/// REQ: DEV-423
#[test]
fn dev_423_native_activation_adds_no_fake_time_when_the_plugin_makes_no_host_call() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS);
    });
}

/// REQ: DEV-423
#[test]
fn dev_423_native_mode_has_no_activation_deadline_so_a_slow_clock_jump_is_no_failure() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.advance_clock(5_000);
        host.activate::<Spy>();
        assert!(host.failures().is_empty());
        assert_eq!(host.snapshot().instance, 1);
    });
}

// ---- DEV-424 ---------------------------------------------------------------------------------

/// REQ: DEV-424
#[test]
fn dev_424_each_documented_reason_reaches_the_plugin() {
    with_clean_env(|| {
        for reason in [
            DeactivationReason::Shutdown,
            DeactivationReason::Disabled,
            DeactivationReason::Upgrade,
            DeactivationReason::Revoked,
            DeactivationReason::Granted,
        ] {
            spy_reset();
            let mut host = FakeHost::new();
            host.activate::<Spy>();
            host.deactivate(reason);
            assert_eq!(spy().deactivations, vec![reason], "reason {reason:?}");
        }
    });
}

/// REQ: DEV-424
#[test]
fn dev_424_deactivate_returns_the_restore_bytes_of_the_plugin() {
    with_clean_env(|| {
        spy_reset();
        spy_edit(|state| state.restore = Some(vec![1, 2, 3]));
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_eq!(
            host.deactivate(DeactivationReason::Shutdown),
            Some(vec![1, 2, 3])
        );
    });
}

/// REQ: DEV-424
#[test]
fn dev_424_deactivate_returns_none_when_the_plugin_returns_none() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_eq!(host.deactivate(DeactivationReason::Shutdown), None);
    });
}

/// REQ: DEV-424
#[test]
fn dev_424_the_reserved_crash_restart_reason_fails_with_reason_reserved() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_fails_with(codes::REASON_RESERVED, || {
            host.deactivate(DeactivationReason::CrashRestart)
        });
        assert!(spy().deactivations.is_empty());
    });
}

// ---- DEV-425, DEV-426, DEV-428 ---------------------------------------------------------------

/// REQ: DEV-425
#[test]
fn dev_425_the_fake_file_system_of_one_host_is_invisible_to_another() {
    with_clean_env(|| {
        let mut first = FakeHost::new();
        let mut second = FakeHost::new();
        first.fs().write("project:notes.md", b"first");
        assert_eq!(first.fs().read("project:notes.md"), Some(b"first".to_vec()));
        assert_eq!(second.fs().read("project:notes.md"), None);
    });
}

/// REQ: DEV-425
#[test]
fn dev_425_submissions_and_outcomes_of_one_host_are_invisible_to_another() {
    with_clean_env(|| {
        spy_reset();
        let mut first = FakeHost::new().grant("sound.play");
        let mut second = FakeHost::new().grant("sound.play");
        first.activate::<Spy>();
        spy_script(vec![vec![sound(1)]]);
        first.deliver(client_batch(1));
        first.settle();
        second.activate::<Spy>();
        assert_eq!(first.submitted().len(), 1);
        assert!(second.submitted().is_empty());
        assert!(second.outcomes().is_empty());
        assert!(second
            .storage()
            .dump(ilium_plugin_sdk_core::host::Bucket::Global)
            .is_empty());
    });
}

/// REQ: DEV-426
#[test]
fn dev_426_every_host_builds_its_own_plugin_value_with_default() {
    with_clean_env(|| {
        spy_reset();
        let mut first = FakeHost::new();
        let mut second = FakeHost::new();
        first.activate::<Spy>();
        second.activate::<Spy>();
        assert_eq!(spy().constructed, 2);
    });
}

/// REQ: DEV-426, DEV-491
#[test]
fn dev_426_a_restart_builds_a_new_plugin_value_so_in_memory_state_is_new() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_eq!(spy().constructed, 1);
        host.restart_with(vec![1]);
        assert_eq!(spy().constructed, 2);
    });
}

/// REQ: DEV-428
#[test]
fn dev_428_hosts_on_parallel_threads_share_no_state() {
    with_clean_env(|| {
        let handles: Vec<_> = (1..=8usize)
            .map(|count| {
                std::thread::spawn(move || {
                    spy_reset();
                    let mut host = FakeHost::new().grant("sound.play").seed(count as u64);
                    host.activate::<Spy>();
                    let replies = (0..count)
                        .map(|index| vec![sound(index as u64 + 1)])
                        .collect();
                    spy_script(replies);
                    for _ in 0..count {
                        host.deliver(client_batch(1));
                    }
                    host.settle();
                    (count, host.submitted().len())
                })
            })
            .collect();
        for handle in handles {
            let (count, submitted) = handle.join().expect("thread finished");
            assert_eq!(submitted, count);
        }
    });
}

/// REQ: DEV-428
#[test]
fn dev_428_two_hosts_on_one_thread_keep_separate_clocks() {
    with_clean_env(|| {
        let mut first = FakeHost::new();
        let second = FakeHost::new();
        first.advance_clock(10_000);
        assert_eq!(first.clock().now(), DEFAULT_CLOCK_MS + 10_000);
        assert_eq!(second.clock().now(), DEFAULT_CLOCK_MS);
    });
}
