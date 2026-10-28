//! reference-test-harness.md "Modes": DEV-400 to DEV-404.

use ilium_plugin_sdk_core::host::InstanceState;
use ilium_plugin_testkit::{FakeHost, Mode};

use crate::common::{
    assert_fails_with, client_batch, panic_text, spy, spy_reset, with_clean_env, with_env, Spy,
};

/// Runs one fixed scenario through the public API and returns what a test would assert on.
fn scenario(mut host: FakeHost) -> (u64, usize, usize) {
    spy_reset();
    let handle = host.activate::<Spy>();
    let submissions = host.deliver(client_batch(3));
    (handle.instance, submissions.len(), spy().batches.len())
}

/// REQ: DEV-400
#[test]
fn dev_400_default_and_explicit_native_mode_give_the_same_results() {
    with_clean_env(|| {
        let default_host = scenario(FakeHost::new());
        let explicit_host = scenario(FakeHost::new().mode(Mode::Native));
        assert_eq!(default_host, explicit_host);
        assert_eq!(default_host, (1, 0, 1));
    });
}

/// REQ: DEV-400
#[test]
fn dev_400_the_builder_accepts_both_modes_and_reports_the_chosen_one() {
    with_clean_env(|| {
        assert_eq!(
            FakeHost::new().mode(Mode::Native).current_mode(),
            Mode::Native
        );
        assert_eq!(
            FakeHost::new().mode(Mode::Component).current_mode(),
            Mode::Component
        );
        // The native host is also usable, so the check is not satisfied by the reader alone.
        let mut host = FakeHost::new().mode(Mode::Native);
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(spy().activations.len(), 1);
    });
}

/// REQ: DEV-401
#[test]
fn dev_401_native_mode_reports_native_and_enforces_no_deadline() {
    with_clean_env(|| {
        let mut host = FakeHost::new().mode(Mode::Native);
        spy_reset();
        host.activate::<Spy>();
        for _ in 0..40 {
            host.deliver(client_batch(1));
        }
        assert_eq!(host.current_mode(), Mode::Native);
        // 40 batches in native mode never count as a failure: no deadline applies.
        assert!(host.failures().is_empty());
        assert!(host.restarts().is_empty());
    });
}

/// REQ: DEV-402
#[test]
fn dev_402_the_test_mode_variable_selects_component_mode() {
    let (mode, state) = with_env(&[("ILIUM_PLUGIN_TEST_MODE", "component")], || {
        let host = FakeHost::new();
        (host.current_mode(), host.lifecycle_state())
    });
    assert_eq!(mode, Mode::Component);
    assert_eq!(state, InstanceState::Inactive);
}

/// REQ: DEV-402
#[test]
fn dev_402_without_the_test_mode_variable_the_mode_is_native_and_the_plugin_runs() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        assert_eq!(host.current_mode(), Mode::Native);
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(spy().activations.len(), 1);
    });
}

/// REQ: DEV-403
#[test]
fn dev_403_an_unexpected_trap_fails_the_test_with_the_trap_code() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        spy_reset();
        host.activate::<Spy>();
        host.inject_trap("trap.deadline");
        assert_fails_with("trap.deadline", || {
            host.deliver(client_batch(1));
        });
    });
}

/// REQ: DEV-403
#[test]
fn dev_403_expect_trap_consumes_the_trap_and_the_test_passes() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        spy_reset();
        host.activate::<Spy>();
        host.expect_trap("trap.deadline");
        host.inject_trap("trap.deadline");
        let outcome = panic_text(|| {
            host.deliver(client_batch(1));
        });
        assert_eq!(outcome, None);
        assert_eq!(host.failures().len(), 1);
        assert_eq!(host.failures()[0].code, "trap.deadline");
    });
}

/// REQ: DEV-403
#[test]
fn dev_403_a_trap_with_a_different_code_than_expected_still_fails_the_test() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        spy_reset();
        host.activate::<Spy>();
        host.expect_trap("trap.memory");
        host.inject_trap("trap.deadline");
        assert_fails_with("trap.deadline", || {
            host.deliver(client_batch(1));
        });
    });
}

/// REQ: DEV-404
#[test]
fn dev_404_mode_is_native_under_cargo_test() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        spy_reset();
        host.activate::<Spy>();
        assert_eq!(host.current_mode(), Mode::Native);
    });
}

/// REQ: DEV-404
#[test]
fn dev_404_mode_is_component_when_the_test_mode_variable_says_component() {
    let (mode, state) = with_env(&[("ILIUM_PLUGIN_TEST_MODE", "component")], || {
        let host = FakeHost::new();
        (host.current_mode(), host.lifecycle_state())
    });
    assert_eq!(state, InstanceState::Inactive);
    assert_eq!(mode, Mode::Component);
}
