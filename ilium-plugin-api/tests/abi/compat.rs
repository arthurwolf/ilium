//! What a plugin of an API version receives: ABI-028 and ABI-029. In 0.1.0 every case is a
//! base case, so the rules are checked at the current version.

use ilium_plugin_api::compat::{
    case_since, event_deliverable, map_outcome_for, map_reason_for, CaseFamily,
};
use ilium_plugin_api::generated::types::{DeactivationReason, Event, Intent, Outcome};
use ilium_plugin_api::version::CURRENT;

/// REQ: ABI-028
#[test]
fn abi_028_crash_restart_is_never_delivered() {
    assert_eq!(
        map_reason_for(DeactivationReason::CrashRestart, CURRENT),
        None
    );
}

/// REQ: ABI-028
#[test]
fn abi_028_the_five_other_reasons_are_delivered_unchanged() {
    for reason in [
        DeactivationReason::Shutdown,
        DeactivationReason::Disabled,
        DeactivationReason::Upgrade,
        DeactivationReason::Revoked,
        DeactivationReason::Granted,
    ] {
        assert_eq!(map_reason_for(reason, CURRENT), Some(reason));
    }
}

/// REQ: ABI-029
#[test]
fn abi_029_ok_epoch_reaches_a_current_plugin_unchanged() {
    assert_eq!(
        map_outcome_for(&Outcome::OkEpoch(9), CURRENT),
        Outcome::OkEpoch(9)
    );
}

/// REQ: ABI-029
#[test]
fn abi_029_ok_reaches_a_current_plugin_unchanged() {
    assert_eq!(map_outcome_for(&Outcome::Ok, CURRENT), Outcome::Ok);
}

/// REQ: ABI-029
#[test]
fn abi_029_every_event_case_reaches_a_current_plugin() {
    for case in Event::CASE_NAMES {
        assert!(event_deliverable(case, CURRENT), "{case}");
    }
}

/// REQ: ABI-029
#[test]
fn abi_029_unknown_event_case_is_not_deliverable() {
    assert!(!event_deliverable("teleported", CURRENT));
}

/// REQ: ABI-029
#[test]
fn abi_029_every_0_1_0_case_dates_from_0_1_0() {
    let families: [(CaseFamily, &[&str]); 4] = [
        (CaseFamily::Event, Event::CASE_NAMES),
        (CaseFamily::Intent, Intent::CASE_NAMES),
        (CaseFamily::Outcome, Outcome::CASE_NAMES),
        (
            CaseFamily::DeactivationReason,
            DeactivationReason::CASE_NAMES,
        ),
    ];
    for (family, cases) in families {
        for case in cases {
            assert_eq!(case_since(family, case), Some(CURRENT), "{family:?} {case}");
        }
    }
}

/// REQ: ABI-029
#[test]
fn abi_029_unknown_case_has_no_version() {
    assert_eq!(case_since(CaseFamily::Outcome, "ok-later"), None);
}
