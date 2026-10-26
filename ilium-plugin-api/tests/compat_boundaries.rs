use ilium_plugin_api::{
    compat::{event_deliverable, map_outcome_for, map_reason_for},
    generated::types::{DeactivationReason, Outcome},
    version::{ApiVersion, CURRENT},
};

#[test]
fn older_contracts_do_not_receive_new_events_and_receive_safe_fallbacks() {
    let older = ApiVersion {
        major: 0,
        minor: 0,
        patch: u16::MAX,
    };
    assert!(!event_deliverable("tree-changed", older));
    assert_eq!(map_outcome_for(&Outcome::OkEpoch(42), older), Outcome::Ok);
    assert_eq!(
        map_reason_for(DeactivationReason::Upgrade, older),
        Some(DeactivationReason::Shutdown)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::CrashRestart, older),
        None
    );
}

#[test]
fn patch_releases_preserve_current_case_shapes() {
    let patched = ApiVersion {
        patch: u16::MAX,
        ..CURRENT
    };
    assert!(event_deliverable("tree-changed", patched));
    assert_eq!(
        map_outcome_for(&Outcome::OkEpoch(42), patched),
        Outcome::OkEpoch(42)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::Granted, patched),
        Some(DeactivationReason::Granted)
    );
}
