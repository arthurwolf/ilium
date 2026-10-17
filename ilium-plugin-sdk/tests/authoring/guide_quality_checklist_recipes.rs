//! Checklist rules run on the fake host (guide-quality-checklist.md).

use ilium_plugin_sdk::host::InstanceState;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{DeactivationReason, ErrorCode, Outcome};

use crate::support::{tick, OptionalGrant, Versioned};

/// REQ: AUT-405, AUT-205
#[test]
fn aut_405_without_the_optional_grant_the_plugin_falls_back_and_keeps_working() {
    let mut host = FakeHost::new();
    host.activate::<OptionalGrant>();
    let first = host.deliver(vec![tick()]);
    host.settle();
    match &host.outcomes()[0] {
        Outcome::Refused(error) => assert_eq!(error.code, ErrorCode::NotPermitted),
        other => panic!("expected refused(not-permitted), got {other:?}"),
    }
    assert_eq!(first.len(), 1);
    assert!(
        host.submitted().len() >= 2,
        "the fallback submission follows the refusal"
    );
    host.deliver(vec![tick()]);
    host.settle();
    assert!(
        host.submitted().len() >= 3,
        "the plugin still handles events"
    );
}

/// REQ: AUT-405, AUT-404
#[test]
fn aut_405_with_the_grant_no_fallback_is_needed() {
    let mut host = FakeHost::new().grant("sound.play");
    host.activate::<OptionalGrant>();
    host.deliver(vec![tick()]);
    host.settle();
    assert!(host.outcomes()[0].is_ok());
    assert_eq!(host.submitted().len(), 1);
}

/// REQ: AUT-404
#[test]
fn aut_404_revoking_the_grant_at_run_time_refuses_the_next_submission() {
    let mut host = FakeHost::new().grant("sound.play");
    host.activate::<OptionalGrant>();
    host.revoke("sound.play");
    host.deliver(vec![tick()]);
    host.settle();
    assert!(
        matches!(&host.outcomes()[0], Outcome::Refused(error) if error.code == ErrorCode::NotPermitted)
    );
}

/// REQ: AUT-408
#[test]
fn aut_408_activation_reaches_the_active_state() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    assert_eq!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: AUT-408
#[test]
fn aut_408_deactivation_returns_the_saved_bytes_and_the_instance_goes_inactive() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    let saved = host.deactivate(DeactivationReason::Shutdown);
    assert_eq!(saved, Some(Versioned::default().save()));
    assert_ne!(host.lifecycle_state(), InstanceState::Active);
}

/// REQ: AUT-408
#[test]
fn aut_408_refused_not_permitted_is_a_handled_error_code() {
    let mut host = FakeHost::new();
    host.activate::<OptionalGrant>();
    host.deliver(vec![tick()]);
    host.settle();
    let refused = host
        .outcomes()
        .into_iter()
        .filter(|outcome| matches!(outcome, Outcome::Refused(_)))
        .count();
    assert_eq!(refused, 1);
    assert!(host.failures().is_empty(), "a refusal is not a failure");
}

/// REQ: AUT-409
#[test]
fn aut_409_upgrade_from_the_previous_release_snapshot_keeps_the_stored_notes() {
    let snapshot = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/authoring/fixtures/storage-v1.json"
    );
    let mut host = FakeHost::new()
        .with_data_version(1)
        .with_storage_snapshot(snapshot);
    host.activate::<Versioned>();
    let bytes = host
        .deactivate(DeactivationReason::Upgrade)
        .expect("restore bytes");
    host.restart_with(bytes);
    let state = host.snapshot();
    assert_eq!(state.instance, 2, "the restart is the second instance");
    assert_eq!(state.storage["global"]["notes"], b"notes".to_vec());
}
