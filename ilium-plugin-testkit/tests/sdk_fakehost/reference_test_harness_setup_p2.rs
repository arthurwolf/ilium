//! reference-test-harness.md "Setup": DEV-564 and DEV-565 (grants_changed and breach_deadline).

use ilium_plugin_sdk_core::host::InstanceState;
use ilium_plugin_sdk_core::types::{DeactivationReason, ErrorCode, Outcome};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::FakeHost;

use crate::common::{
    assert_fails_with, client_batch, outcome_code, sound, spy, spy_edit, spy_reset, spy_script,
    strings, with_clean_env, Spy,
};

fn host_with(grants: &[&str]) -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new().grants(grants);
    host.activate::<Spy>();
    host
}

fn pending_sound_host() -> FakeHost {
    let mut host = host_with(&["sound.play", "tree.read"]);
    spy_script(vec![vec![sound(1)]]);
    host.deliver(client_batch(1));
    assert_eq!(host.pending_requests().len(), 1);
    host
}

// ---- DEV-564 ---------------------------------------------------------------------------------

/// REQ: DEV-564
#[test]
fn dev_564_a_request_that_needs_a_removed_grant_completes_failed_revoked() {
    with_clean_env(|| {
        let mut host = pending_sound_host();
        host.grants_changed(&["tree.read"]);
        let outcomes = host.outcomes();
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], Outcome::Failed(_)));
        assert_eq!(outcome_code(&outcomes[0]), Some(ErrorCode::Revoked));
        assert!(host.pending_requests().is_empty());
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_removing_a_grant_deactivates_with_revoked_then_activates_with_the_new_list() {
    with_clean_env(|| {
        let mut host = pending_sound_host();
        host.grants_changed(&["tree.read"]);
        let log = spy();
        assert_eq!(log.deactivations, vec![DeactivationReason::Revoked]);
        assert_eq!(log.activations.len(), 2);
        assert_eq!(log.activations[1].instance, 2);
        assert_eq!(log.activations[1].granted, strings(&["tree.read"]));
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_a_list_that_only_adds_grants_deactivates_with_granted() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read", "sound.play"]);
        host.grants_changed(&["tree.read", "sound.play", "agent.read"]);
        let log = spy();
        assert_eq!(log.deactivations, vec![DeactivationReason::Granted]);
        assert_eq!(
            log.activations[1].granted,
            // DEV-420 requires sorted activation grants, including a grant-only change.
            strings(&["agent.read", "sound.play", "tree.read"])
        );
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_a_list_that_adds_and_removes_counts_as_a_revocation() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read", "sound.play"]);
        host.grants_changed(&["tree.read", "agent.read"]);
        assert_eq!(spy().deactivations, vec![DeactivationReason::Revoked]);
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_a_pending_request_that_needs_only_kept_grants_stays_pending() {
    with_clean_env(|| {
        let mut host = pending_sound_host();
        host.grants_changed(&["sound.play"]);
        assert_eq!(host.pending_requests().len(), 1);
        assert!(host.outcomes().is_empty());
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_the_list_replaces_the_whole_granted_list_in_one_step() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read", "agent.read", "sound.play"]);
        host.grants_changed(&["agent.read"]);
        let log = spy();
        assert_eq!(log.deactivations.len(), 1, "one step, one deactivate");
        assert_eq!(log.activations[1].granted, strings(&["agent.read"]));
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_no_event_is_delivered_by_the_change() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read", "sound.play"]);
        host.grants_changed(&["tree.read"]);
        assert!(spy().batches.is_empty());
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_an_intent_that_needs_the_removed_grant_is_refused_afterwards() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read", "sound.play"]);
        host.grants_changed(&["tree.read"]);
        spy_script(vec![vec![sound(5)]]);
        host.deliver(client_batch(1));
        host.settle();
        let outcomes = host.outcomes();
        assert_eq!(outcome_code(&outcomes[0]), Some(ErrorCode::NotPermitted));
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_an_unknown_grant_name_is_refused_with_grant_unknown() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        assert_fails_with(codes::GRANT_UNKNOWN, || {
            host.grants_changed(&["no.such.grant"])
        });
    });
}

/// REQ: DEV-564
#[test]
fn dev_564_an_unchanged_list_still_re_activates_with_instance_plus_one() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        host.grants_changed(&["tree.read"]);
        assert_eq!(spy().activations[1].instance, 2);
    });
}

// ---- DEV-565 ---------------------------------------------------------------------------------

/// REQ: DEV-565
#[test]
fn dev_565_a_breached_on_events_call_traps_with_trap_deadline() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        host.breach_deadline("on-events");
        host.deliver(client_batch(1));
        let failures = host.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].code, "trap.deadline");
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_the_breach_applies_to_the_next_call_only() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        host.breach_deadline("on-events");
        host.deliver(client_batch(1));
        host.advance_clock(10_000);
        host.deliver(client_batch(1));
        assert_eq!(host.failures().len(), 1, "the second call ran normally");
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_a_breached_activate_fails_the_activation_with_a_trap() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.breach_deadline("activate");
        let outcome = host.try_activate::<Spy>();
        assert!(outcome.is_err(), "the activation fails");
        assert_eq!(host.failures()[0].code, "trap.deadline");
        assert_eq!(host.failures().len(), 1);
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_a_breached_deactivate_does_not_count_as_a_failure() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        host.breach_deadline("deactivate");
        host.deactivate(DeactivationReason::Shutdown);
        assert!(host.failures().is_empty());
        assert!(!host.is_disabled());
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_a_breached_deactivate_keeps_no_restore_bytes() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        spy_edit(|state| state.restore = Some(vec![1, 2, 3]));
        host.breach_deadline("deactivate");
        assert_eq!(host.deactivate(DeactivationReason::Shutdown), None);
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_a_deactivate_without_a_breach_keeps_the_restore_bytes() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        spy_edit(|state| state.restore = Some(vec![1, 2, 3]));
        assert_eq!(
            host.deactivate(DeactivationReason::Shutdown),
            Some(vec![1, 2, 3])
        );
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_three_deactivate_breaches_do_not_disable_the_plugin() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        for _ in 0..3 {
            host.activate::<Spy>();
            host.breach_deadline("deactivate");
            host.deactivate(DeactivationReason::Shutdown);
        }
        assert!(!host.is_disabled());
        assert!(host.failures().is_empty());
        assert_ne!(host.lifecycle_state(), InstanceState::StoppedByFailure);
    });
}

/// REQ: DEV-565
#[test]
fn dev_565_three_on_events_breaches_in_a_row_disable_the_plugin() {
    with_clean_env(|| {
        let mut host = host_with(&["tree.read"]);
        for _ in 0..3 {
            host.breach_deadline("on-events");
            host.deliver(client_batch(1));
            host.advance_clock(10_000);
        }
        assert_eq!(host.failures().len(), 3);
        assert!(host.is_disabled());
    });
}
