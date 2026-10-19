//! security.md: grant checks on every call (SEC-013) and no third-party code in the server
//! process (SEC-014). Refusal forms are checked per kind of function.

use ilium_plugin_api::{ErrorCode, Tier};
use ilium_plugin_host::testing::Isolation;
use serde_json::json;

use crate::support::*;

/// REQ: SEC-013
#[test]
fn sec_013_a_missing_grant_is_refused_with_reason_not_granted_not_permitted() {
    let rig = t3(&[]);
    match rig.call("ilium:host/clipboard.read", json!({})) {
        ilium_plugin_host::testing::ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(
                error.message.starts_with("not-granted: "),
                "{}",
                error.message
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// REQ: SEC-013
#[test]
fn sec_013_a_declined_grant_is_refused_with_reason_declined_not_permitted() {
    let rig = setup(&[]).declined(&["clipboard.read"]).start();
    match rig.call("ilium:host/clipboard.read", json!({})) {
        ilium_plugin_host::testing::ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(error.message.starts_with("declined: "), "{}", error.message);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// REQ: SEC-013
#[test]
fn sec_013_a_grant_revoked_between_two_calls_applies_to_the_second_call() {
    let rig = t3(&["clipboard.read"]);
    assert!(is_ok(&rig.call("ilium:host/clipboard.read", json!({}))));
    rig.host.revoke("clipboard.read");
    assert!(is_not_permitted(
        &rig.call("ilium:host/clipboard.read", json!({}))
    ));
    rig.host.grant_now("clipboard.read");
    assert!(is_ok(&rig.call("ilium:host/clipboard.read", json!({}))));
}

/// REQ: SEC-013
#[test]
fn sec_013_a_policy_forbidden_grant_is_refused_with_reason_policy_not_permitted() {
    let rig = setup(&["clipboard.read"])
        .system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n", true)
        .start();
    match rig.call("ilium:host/clipboard.read", json!({})) {
        ilium_plugin_host::testing::ImportResult::Err(error) => {
            assert!(error.message.starts_with("policy: "), "{}", error.message)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// REQ: SEC-013
#[test]
fn sec_013_the_check_applies_to_t1_t2_and_t3() {
    for tier in [Tier::T1, Tier::T2, Tier::T3] {
        let rig = rig(tier, ilium_plugin_api::Side::Server, &[]);
        assert!(
            is_not_permitted(&rig.call("ilium:host/clipboard.read", json!({}))),
            "{tier:?}"
        );
    }
}

/// REQ: SEC-013
#[test]
fn sec_013_option_and_list_returning_functions_refuse_with_none_and_empty() {
    let rig = t3(&[]);
    assert_eq!(
        rig.value("ilium:host/location.current", json!({}))
            .expect("option"),
        json!(null)
    );
    assert_eq!(
        rig.value("ilium:host/tree.children", json!({ "node": 0 }))
            .expect("list"),
        json!([])
    );
}

/// REQ: SEC-013
#[test]
fn sec_013_a_grant_never_requested_traps_with_capability_not_requested() {
    let rig = t3(&[]);
    let result = rig.call(
        "ilium:host/process.spawn",
        json!({ "program": "rg", "args": [] }),
    );
    match result {
        ilium_plugin_host::testing::ImportResult::Trap(trap) => {
            assert_eq!(trap.code.as_str(), "host.capability-not-requested")
        }
        other => panic!("expected a trap, got {other:?}"),
    }
}

/// REQ: SEC-013
#[test]
fn sec_013_the_first_refused_call_of_each_kind_writes_one_warn_line() {
    let rig = t3(&[]);
    for _ in 0..3 {
        let _ = rig.call("ilium:host/clipboard.read", json!({}));
    }
    let lines: Vec<String> = rig
        .log_messages()
        .into_iter()
        .filter(|line| line.contains("clipboard.read"))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
}

/// REQ: SEC-014
#[test]
fn sec_014_third_party_tiers_run_in_a_helper_never_in_the_server() {
    for tier in [Tier::T2, Tier::T3] {
        assert_eq!(
            rig(tier, ilium_plugin_api::Side::Server, &[])
                .host
                .isolation(),
            Isolation::HelperProcess,
            "{tier:?}"
        );
    }
}

/// REQ: SEC-014
#[test]
fn sec_014_when_the_sandbox_cannot_be_built_the_package_does_not_load() {
    let rig = setup(&[])
        .sandbox_unavailable("unprivileged user namespaces are disabled")
        .start();
    assert_eq!(
        rig.host.load_error().as_deref(),
        Some("sandbox.unavailable")
    );
    assert_eq!(rig.host.isolation_attempted(), None);
    assert!(!matches!(
        rig.host.state(),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    ));
}

/// REQ: SEC-014
#[test]
fn sec_014_the_helper_process_id_differs_from_the_server_process_id() {
    let rig = t3(&[]);
    assert_ne!(rig.host.guest_process_id(), std::process::id());
}
