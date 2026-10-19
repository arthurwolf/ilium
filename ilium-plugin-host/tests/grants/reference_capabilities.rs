//! CAP-011..CAP-018 of reference-requirements-capabilities.md, host half: what a running
//! plugin gets for always-granted, declined, revoked and never-requested capabilities.
//! Imports are called through `HarnessHandle::call_import` (API_GAPS.md); intents go through
//! the recording guest.

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::intents::Intent;
use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::{ErrorCode, Side, Tier};
use ilium_plugin_host::testing::{HostHarness, ImportResult};
use serde_json::json;

use crate::support::{refusal, rig, rig_from};

const DATA_FILE: &str = "/data/note.txt";
const PROJECT_FILE: &str = "/project/note.txt";

fn read_project() -> serde_json::Value {
    json!({ "path": PROJECT_FILE })
}

fn is_not_permitted(result: &ImportResult) -> bool {
    matches!(result, ImportResult::Err(error) if error.code == ErrorCode::NotPermitted)
}

fn is_ok(result: &ImportResult) -> bool {
    matches!(result, ImportResult::Ok)
}

// ---------------------------------------------------------------- CAP-011 always granted

/// REQ: CAP-011
#[test]
fn cap_011_log_succeeds_with_an_empty_capabilities_list() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    let result = rig.host.call_import(
        "ilium:host/log.log",
        json!({ "level": "info", "message": "hello" }),
    );
    assert!(is_ok(&result), "{result:?}");
}

/// REQ: CAP-011
#[test]
fn cap_011_storage_succeeds_with_an_empty_capabilities_list() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    let put = rig.host.call_import(
        "ilium:host/storage.put",
        json!({ "bucket": "global", "key": "k", "value": [1, 2, 3] }),
    );
    assert!(is_ok(&put), "{put:?}");
    let get = rig.host.call_import(
        "ilium:host/storage.get",
        json!({ "bucket": "global", "key": "k" }),
    );
    assert!(is_ok(&get), "{get:?}");
}

/// REQ: CAP-011
#[test]
fn cap_011_clock_timers_codec_and_settings_need_no_grant() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    for (name, args) in [
        ("ilium:host/clock.now-unix-millis", json!({})),
        ("ilium:host/clock.monotonic-millis", json!({})),
        (
            "ilium:host/timers.set",
            json!({ "after-millis": 1000, "repeat-millis": null }),
        ),
        ("ilium:host/codec.sha256", json!({ "data": [1, 2, 3] })),
        ("ilium:host/settings.get-json", json!({})),
    ] {
        let result = rig.host.call_import(name, args);
        assert!(is_ok(&result), "{name}: {result:?}");
    }
}

/// REQ: CAP-011
#[test]
fn cap_011_fs_write_data_and_cache_need_no_grant() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    for path in [DATA_FILE, "/cache/note.txt"] {
        let result = rig.host.call_import(
            "ilium:host/fs.write",
            json!({ "path": path, "data": [104, 105], "mode": "create-new" }),
        );
        assert!(is_ok(&result), "{path}: {result:?}");
    }
}

/// REQ: CAP-011
#[test]
fn cap_011_fs_read_package_data_and_cache_are_never_not_permitted() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    for path in ["/package/a.txt", "/data/a.txt", "/cache/a.txt"] {
        let result = rig
            .host
            .call_import("ilium:host/fs.metadata", json!({ "path": path }));
        assert!(!is_not_permitted(&result), "{path}: {result:?}");
    }
}

/// REQ: CAP-011
#[test]
fn cap_011_fs_watch_of_always_granted_scopes_needs_no_grant() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    for path in ["/package", "/data", "/cache"] {
        let result = rig.host.call_import(
            "ilium:host/fs.watch",
            json!({ "path": path, "recursive": false }),
        );
        assert!(is_ok(&result), "{path}: {result:?}");
    }
}

/// REQ: CAP-011
#[test]
fn cap_011_fs_watch_of_project_without_a_grant_is_not_permitted_not_granted() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T3)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    let result = rig.host.call_import(
        "ilium:host/fs.watch",
        json!({ "path": "/project", "recursive": false }),
    );
    match result {
        ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(
                error.message.starts_with("not-granted: ")
                    || error.message.starts_with("declined: "),
                "{}",
                error.message
            );
        }
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

// ---------------------------------------------------------------- CAP-012 per side

/// REQ: CAP-012
#[test]
fn cap_012_server_grant_does_not_authorize_a_client_call() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Client)
            .other_side_grants(Side::Server, &["fs.read:project"])
            .declined(&["fs.read:project"]),
    );
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_not_permitted(&result), "{result:?}");
}

/// REQ: CAP-012
#[test]
fn cap_012_the_same_call_succeeds_on_the_side_that_holds_the_grant() {
    let rig = rig(Tier::T2, Side::Server, &["fs.read:project"]);
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_ok(&result), "{result:?}");
}

/// REQ: CAP-012
#[test]
fn cap_012_client_activation_lists_only_client_grants() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Client)
            .grants(&["ui.panels"])
            .other_side_grants(Side::Server, &["fs.read:project"]),
    );
    assert_eq!(rig.seen.latest_granted(), vec!["ui.panels".to_owned()]);
}

/// REQ: CAP-012
#[test]
fn cap_012_a_client_grant_does_not_authorize_the_server_side() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .other_side_grants(Side::Client, &["fs.read:project"])
            .declined(&["fs.read:project"]),
    );
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_not_permitted(&result), "{result:?}");
}

// ---------------------------------------------------------------- CAP-013 declined grant

/// REQ: CAP-013
#[test]
fn cap_013_plugin_still_loads_when_a_grant_is_declined() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    assert_eq!(rig.seen.activation_count(), 1);
    assert_eq!(rig.host.state(), InstanceState::Active);
}

/// REQ: CAP-013
#[test]
fn cap_013_call_that_needs_the_declined_grant_is_not_permitted() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    match rig
        .host
        .call_import("ilium:host/fs.read-all", read_project())
    {
        ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(error.message.starts_with("declined: "), "{}", error.message);
        }
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

/// REQ: CAP-013
#[test]
fn cap_013_granted_list_omits_the_declined_grant_and_keeps_the_others() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .grants(&["tree.read"])
            .declined(&["fs.read:project"]),
    );
    let granted = rig.seen.latest_granted();
    assert!(!granted.contains(&"fs.read:project".to_owned()));
    assert!(granted.contains(&"tree.read".to_owned()));
}

/// REQ: CAP-013
#[test]
fn cap_013_declined_intent_grant_refuses_the_submission_only() {
    let mut rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["sound.play"]),
    );
    let error = refusal(rig.submit(Intent::PlaySound("bell".to_owned())));
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(rig.host.state(), InstanceState::Active);
}

/// REQ: CAP-013
#[test]
fn cap_013_a_declined_grant_is_not_counted_as_a_plugin_failure() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    let _ = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(rig.host.failures().is_empty());
}

// ---------------------------------------------------------------- CAP-014 refusal log

/// REQ: CAP-014
#[test]
fn cap_014_refused_intent_is_logged_with_grant_and_reason() {
    let mut rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["sound.play"]),
    );
    let _ = rig.submit(Intent::PlaySound("bell".to_owned()));
    assert!(
        rig.log_messages()
            .iter()
            .any(|line| line.starts_with("refused sound.play by ") && line.ends_with(" (declined)")),
        "{:?}",
        rig.log_messages()
    );
}

/// REQ: CAP-014
#[test]
fn cap_014_refused_import_is_logged_with_a_refused_prefix() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    let _ = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    let lines = rig.log_messages();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("refused fs.read:project by ") && line.ends_with(')')),
        "{lines:?}"
    );
}

/// REQ: CAP-014
#[test]
fn cap_014_reason_is_one_word_of_the_closed_list() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    let _ = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    let closed = [
        "declined",
        "revoked",
        "not-granted",
        "scope",
        "policy",
        "offline",
        "private-address",
        "reserved",
        "downgrade",
        "not-user-initiated",
        "file-type",
        "unavailable",
        "response-size",
        "stream-size",
        "tls",
    ];
    let line = rig
        .log_messages()
        .into_iter()
        .find(|line| line.starts_with("refused "))
        .expect("a refusal line");
    let reason = line
        .rsplit_once(" (")
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .expect("reason");
    assert!(closed.contains(&reason), "{line}");
}

/// REQ: CAP-014
#[test]
fn cap_014_a_permitted_call_writes_no_refusal_line() {
    let rig = rig(Tier::T2, Side::Server, &["fs.read:project"]);
    let _ = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(rig
        .log_messages()
        .iter()
        .all(|line| !line.starts_with("refused ")));
}

// ---------------------------------------------------------------- CAP-015 run-time revoke

/// REQ: CAP-015
#[test]
fn cap_015_revoke_deactivates_with_revoked_and_reactivates_with_the_reduced_set() {
    let rig = rig(Tier::T2, Side::Server, &["fs.read:project", "tree.read"]);
    rig.host.revoke("fs.read:project");
    rig.host.pump(8);
    assert_eq!(rig.seen.reasons(), vec![DeactivationReason::Revoked]);
    assert_eq!(rig.seen.activation_count(), 2);
    assert_eq!(rig.seen.latest_granted(), vec!["tree.read".to_owned()]);
}

/// REQ: CAP-015
#[test]
fn cap_015_call_entering_after_the_revoke_is_not_permitted_with_a_revoked_message() {
    let rig = rig(Tier::T2, Side::Server, &["fs.read:project"]);
    rig.host.revoke("fs.read:project");
    rig.host.pump(8);
    match rig
        .host
        .call_import("ilium:host/fs.read-all", read_project())
    {
        ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(error.message.starts_with("revoked: "), "{}", error.message);
        }
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

/// REQ: CAP-015
#[test]
fn cap_015_in_flight_intent_that_needed_the_grant_completes_failed_revoked() {
    let mut rig = rig(Tier::T2, Side::Server, &["sound.play"]);
    rig.kernel.hold_next_request();
    let first = rig.submit(Intent::PlaySound("bell".to_owned()));
    assert!(first.is_none(), "the request is held: {first:?}");
    rig.host.revoke("sound.play");
    rig.host.pump(8);
    match rig.outcome_of(1) {
        Some(Outcome::Failed(error)) => assert_eq!(error.code, ErrorCode::Revoked),
        other => panic!("expected failed(revoked), got {other:?}"),
    }
}

/// REQ: CAP-015
#[test]
fn cap_015_a_revoke_is_not_a_plugin_failure_and_bumps_the_epoch() {
    let rig = rig(Tier::T2, Side::Server, &["tree.read", "fs.read:project"]);
    let before = rig.host.authorization_epoch();
    rig.host.revoke("tree.read");
    rig.host.pump(8);
    assert!(rig.host.authorization_epoch() > before);
    assert!(rig.host.failures().is_empty());
    assert_eq!(rig.host.state(), InstanceState::Active);
}

/// REQ: CAP-015
#[test]
fn cap_015_revoking_a_grant_the_plugin_does_not_hold_changes_nothing() {
    let rig = rig(Tier::T2, Side::Server, &["tree.read"]);
    let before = rig.host.authorization_epoch();
    rig.host.revoke("fs.read:project");
    rig.host.pump(8);
    assert_eq!(rig.host.authorization_epoch(), before);
    assert_eq!(rig.seen.activation_count(), 1);
}

/// REQ: CAP-015
#[test]
fn cap_015_a_grant_added_later_reactivates_with_reason_granted() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T2)
            .side(Side::Server)
            .declined(&["sound.play"]),
    );
    rig.host.grant_now("sound.play");
    rig.host.pump(8);
    assert_eq!(rig.seen.reasons(), vec![DeactivationReason::Granted]);
    assert!(rig.seen.latest_granted().contains(&"sound.play".to_owned()));
}

// ---------------------------------------------------------------- CAP-016 upgrade shows new grants only

/// REQ: CAP-016
#[test]
fn cap_016_upgrade_adding_one_grant_lists_exactly_one_row() {
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_host::review::update_review;
    let old = [
        Grant::parse("tree.read").expect("grant"),
        Grant::parse("fs.read:project").expect("grant"),
    ];
    let new = [
        Grant::parse("tree.read").expect("grant"),
        Grant::parse("fs.read:project").expect("grant"),
        Grant::parse("notify.toast").expect("grant"),
    ];
    let review = update_review(&old, &new);
    assert_eq!(review.new.len(), 1);
    assert_eq!(review.new[0].grant.as_str(), "notify.toast");
    assert!(review.wider.is_empty());
    assert!(review.removed.is_empty());
}

/// REQ: CAP-016
#[test]
fn cap_016_unchanged_grants_are_not_listed() {
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_host::review::update_review;
    let same = [
        Grant::parse("tree.read").expect("grant"),
        Grant::parse("fs.read:project").expect("grant"),
    ];
    let review = update_review(&same, &same);
    assert!(review.new.is_empty() && review.wider.is_empty() && review.removed.is_empty());
    assert_eq!(review.unchanged.len(), 2);
}

// ---------------------------------------------------------------- CAP-018 never requested vs declined

/// REQ: CAP-018
#[test]
fn cap_018_import_never_requested_traps() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    match rig
        .host
        .call_import("ilium:host/fs.read-all", read_project())
    {
        ImportResult::Trap(trap) => assert_eq!(trap.code.as_str(), "host.capability-not-requested"),
        other => panic!("expected a trap, got {other:?}"),
    }
}

/// REQ: CAP-018
#[test]
fn cap_018_trap_is_recorded_as_a_plugin_failure() {
    let rig = rig(Tier::T3, Side::Server, &[]);
    let _ = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    let failures = rig.host.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code.as_str(), "host.capability-not-requested");
}

/// REQ: CAP-018
#[test]
fn cap_018_import_whose_grant_was_declined_returns_not_permitted_not_a_trap() {
    let rig = rig_from(
        HostHarness::new()
            .tier(Tier::T3)
            .side(Side::Server)
            .declined(&["fs.read:project"]),
    );
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_not_permitted(&result), "{result:?}");
    assert!(rig.host.failures().is_empty());
}

/// REQ: CAP-018
#[test]
fn cap_018_import_whose_grant_was_revoked_returns_not_permitted() {
    let rig = rig(Tier::T3, Side::Server, &["fs.read:project"]);
    rig.host.revoke("fs.read:project");
    rig.host.pump(8);
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_not_permitted(&result), "{result:?}");
}

/// REQ: CAP-018
#[test]
fn cap_018_a_granted_import_is_neither_trap_nor_refusal() {
    let rig = rig(Tier::T3, Side::Server, &["fs.read:project"]);
    let result = rig
        .host
        .call_import("ilium:host/fs.read-all", read_project());
    assert!(is_ok(&result), "{result:?}");
}

// ---------------------------------------------------------------- CAP-887..891 agent-config.propose

const PROPOSE_GRANTS: [&str; 2] = ["fs.read:agent-config:claude", "agent-config.propose:claude"];
const PROPOSE_IMPORT: &str = "ilium:host/fs.propose-agent-config";

fn proposal(path: &str) -> serde_json::Value {
    json!({
        "agent": "claude",
        "title": "Ilium instructions",
        "changes": [{ "path": path, "content": [104, 105], "revision": null, "summary": "add" }],
    })
}

/// REQ: CAP-887
#[test]
fn cap_887_a_proposal_changes_no_file_before_the_answer_and_applies_all_or_none() {
    let rig = rig(Tier::T2, Side::Server, &PROPOSE_GRANTS);
    let pending = rig.host.call_import(PROPOSE_IMPORT, proposal("CLAUDE.md"));
    assert!(is_ok(&pending), "{pending:?}");
    assert!(!rig.kernel.agent_config_file_exists("claude", "CLAUDE.md"));
    let dialog = rig.kernel.open_proposal_dialog().expect("a dialog is open");
    assert!(dialog.rows.iter().all(|row| !row.checked));
    assert!(!dialog.full_diff.is_empty());
    rig.kernel.fail_next_rename();
    rig.kernel.check_row_and_apply(0);
    rig.host.pump(8);
    assert_eq!(rig.host.last_proposal_failure_code(), Some(ErrorCode::Io));
    assert!(!rig.kernel.agent_config_file_exists("claude", "CLAUDE.md"));
}

/// REQ: CAP-888
#[test]
fn cap_888_a_credential_path_is_refused_with_scope_and_opens_no_dialog() {
    let rig = rig(Tier::T2, Side::Server, &PROPOSE_GRANTS);
    let result = rig
        .host
        .call_import(PROPOSE_IMPORT, proposal(".credentials.json"));
    match result {
        ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(error.message.starts_with("scope: "), "{}", error.message);
        }
        other => panic!("expected not-permitted, got {other:?}"),
    }
    assert!(rig.kernel.open_proposal_dialog().is_none());
    let empty = json!({ "agent": "claude", "title": "t", "changes": [] });
    match rig.host.call_import(PROPOSE_IMPORT, empty) {
        ImportResult::Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument),
        other => panic!("expected invalid-argument, got {other:?}"),
    }
}

/// REQ: CAP-889
#[test]
fn cap_889_an_edited_file_completes_failed_conflict_and_stays_unchanged() {
    let rig = rig(Tier::T2, Side::Server, &PROPOSE_GRANTS);
    rig.kernel.write_agent_config_file("claude", "CLAUDE.md", b"old");
    let revision = rig.kernel.agent_config_revision("claude", "CLAUDE.md");
    let mut request = proposal("CLAUDE.md");
    request["changes"][0]["revision"] = json!(revision);
    let pending = rig.host.call_import(PROPOSE_IMPORT, request);
    assert!(is_ok(&pending), "{pending:?}");
    rig.kernel.write_agent_config_file("claude", "CLAUDE.md", b"edited");
    rig.kernel.check_row_and_apply(0);
    rig.host.pump(8);
    assert_eq!(
        rig.host.last_proposal_failure_code(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        rig.kernel.read_agent_config_file("claude", "CLAUDE.md"),
        b"edited".to_vec()
    );
}

/// REQ: CAP-890
#[test]
fn cap_890_reject_all_declines_and_is_never_stored_in_consents() {
    let rig = rig(Tier::T2, Side::Server, &PROPOSE_GRANTS);
    let pending = rig.host.call_import(PROPOSE_IMPORT, proposal("CLAUDE.md"));
    assert!(is_ok(&pending), "{pending:?}");
    match rig.host.call_import(PROPOSE_IMPORT, proposal("other.md")) {
        ImportResult::Err(error) => assert_eq!(error.code, ErrorCode::Busy),
        other => panic!("expected busy, got {other:?}"),
    }
    rig.kernel.reject_all();
    rig.host.pump(8);
    let failure = rig.host.last_proposal_failure().expect("completed failed");
    assert_eq!(failure.code, ErrorCode::NotPermitted);
    assert!(failure.message.starts_with("declined: "), "{}", failure.message);
    assert!(rig.kernel.consents_json_is_unchanged());
}

/// REQ: CAP-891
#[test]
fn cap_891_an_applied_file_leaves_a_history_entry_an_audit_record_and_the_applied_list() {
    let rig = rig(Tier::T2, Side::Server, &PROPOSE_GRANTS);
    rig.kernel.write_agent_config_file("claude", "CLAUDE.md", b"old");
    let revision = rig.kernel.agent_config_revision("claude", "CLAUDE.md");
    let mut request = proposal("CLAUDE.md");
    request["changes"][0]["revision"] = json!(revision);
    let pending = rig.host.call_import(PROPOSE_IMPORT, request);
    assert!(is_ok(&pending), "{pending:?}");
    rig.kernel.check_row_and_apply(0);
    rig.host.pump(8);
    let result = rig.host.take_proposal().expect("the proposal completed");
    assert_eq!(result.applied, vec!["CLAUDE.md".to_owned()]);
    let history = rig.kernel.plugin_data_file(&result.history);
    assert!(result.history.starts_with("agent-config-history/claude/"));
    assert!(String::from_utf8_lossy(&history).contains("old"));
    assert_eq!(rig.kernel.audit_records("write-outside-data").len(), 1);
}
