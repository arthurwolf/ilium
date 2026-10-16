//! Plugin policy file (reference-plugin-policy.md): USR-210 to USR-215.

use crate::support::*;

// ---- USR-210 ----

/// REQ: USR-210
#[test]
fn usr_210_unreadable_policy_refuses_third_party_with_policy_invalid_and_core_runs() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n");
    rig.make_system_policy_unreadable();
    rig.start_server();
    let row = list_row(&mut rig, "com.example.t3");
    assert!(row.state.starts_with("Refused"), "{}", row.state);
    assert_eq!(row.reason.as_deref(), Some("policy.invalid"));
    assert_eq!(list_row(&mut rig, "ilium.core.tree").state, "On");
}

/// REQ: USR-210
#[test]
fn usr_210_unparsable_policy_gives_the_same_result() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("this is = = not toml [");
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.t3").reason.as_deref(), Some("policy.invalid"));
}

/// REQ: USR-210
#[test]
fn usr_210_the_policy_is_read_before_any_third_party_plugin_is_loaded() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n");
    rig.start_server();
    let order = rig.startup_trace();
    let policy = order.iter().position(|step| step == "policy-read").expect("policy-read step");
    let first_plugin = order.iter().position(|step| step.starts_with("load:com.example.")).expect("a third-party load");
    assert!(policy < first_plugin, "{order:?}");
}

/// REQ: USR-210
#[test]
fn usr_210_absent_policy_file_is_not_an_error() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.t3").state, "On");
}

// ---- USR-211 ----

/// REQ: USR-211
#[test]
fn usr_211_world_writable_policy_is_ignored_with_a_notice_and_an_audit_record() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.t3\"]\n");
    rig.set_system_policy_mode(0o666);
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.t3").state, "On", "the file is ignored");
    assert!(rig.persistent_notices().iter().any(|text| text.to_lowercase().contains("policy")), "{:?}", rig.persistent_notices());
    let violations = rig.audit_records_of("policy-violation");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["reason"], "policy-file-writable");
}

/// REQ: USR-211
#[test]
fn usr_211_mode_0644_is_accepted_and_0666_is_not() {
    let mut accepted = UserRig::new();
    install_ok(&mut accepted, &spec("com.example.t3", "1.0.0", &[]));
    accepted.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.t3\"]\n");
    accepted.set_system_policy_mode(0o644);
    accepted.start_server();
    assert_eq!(list_row(&mut accepted, "com.example.t3").state, "Blocked by policy");
    assert!(accepted.audit_records_of("policy-violation").is_empty());
}

// ---- USR-212 ----

/// REQ: USR-212
#[test]
fn usr_212_unknown_key_is_malformed_names_line_and_key_and_t1_still_runs() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n\n[packages]\nallow_thirdparty = true\n");
    rig.start_server();
    assert!(list_row(&mut rig, "com.example.t3").state.starts_with("Refused"));
    assert_eq!(list_row(&mut rig, "ilium.core.tree").state, "On");
    let notices = rig.persistent_notices().join("\n");
    assert!(notices.contains("allow_thirdparty"), "{notices}");
    assert!(notices.contains("line 4") || notices.contains(":4"), "{notices}");
}

/// REQ: USR-212
#[test]
fn usr_212_wrong_type_is_malformed_and_names_the_line_of_the_first_error() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = \"yes\"\nallowed_tiers = 5\n");
    rig.start_server();
    assert!(list_row(&mut rig, "com.example.t3").state.starts_with("Refused"));
    let notices = rig.persistent_notices().join("\n");
    assert!(notices.contains("line 3") || notices.contains(":3"), "first error only: {notices}");
}

/// REQ: USR-212
#[test]
fn usr_212_a_valid_policy_shows_no_malformed_notice() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = true\n");
    rig.start_server();
    assert!(rig.persistent_notices().is_empty());
}

// ---- USR-213 ----

/// REQ: USR-213
#[test]
fn usr_213_editing_the_policy_changes_nothing_until_restart() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t2", "1.0.0", &[]));
    rig.start_server();
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = false\n");
    rig.advance_ms(60_000);
    assert_eq!(list_row(&mut rig, "com.example.t2").state, "On");
    rig.restart_server();
    let row = list_row(&mut rig, "com.example.t2");
    assert_eq!(row.state, "Blocked by policy");
    assert_eq!(row.message.as_deref(), Some("policy.third-party-denied"));
}

/// REQ: USR-213
#[test]
fn usr_213_the_server_never_watches_the_policy_file() {
    let mut rig = UserRig::new();
    rig.start_server();
    rig.record_fs_operations();
    rig.write_system_policy("version = 1\n");
    rig.advance_ms(3_600_000);
    assert!(rig.fs_operations().iter().all(|op| !(op.kind == "read" && op.path.ends_with("policy.toml"))));
}

// ---- USR-214 ----

/// REQ: USR-214
#[test]
fn usr_214_denied_grant_is_locked_in_the_dialog_and_calls_are_not_permitted() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let output = rig.install_file(&spec("com.example.clip", "1.0.0", &["clipboard.read", "tree.read"]), &["--allow-unsigned"]);
    assert_eq!(result_exit_code(&output), 0);
    let dialog = &rig.dialogs_shown()[0];
    let row = dialog.rows.iter().find(|row| row.grant == "clipboard.read").expect("row");
    assert!(row.locked);
    assert!(!row.checked);
    assert_eq!(row.note.as_deref(), Some("Blocked by your administrator"));
    rig.start_server();
    match rig.call_needing("com.example.clip", "server", "clipboard.read") {
        CallOutcome::Refused { code, reason } => {
            assert_eq!(code, "not-permitted");
            assert_eq!(reason.as_deref(), Some("policy"));
        }
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

/// REQ: USR-214
#[test]
fn usr_214_a_grant_that_does_not_match_the_deny_list_is_unaffected() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let _ = rig.install_file(&spec("com.example.clip", "1.0.0", &["clipboard.write", "tree.read"]), &["--allow-unsigned"]);
    let dialog = &rig.dialogs_shown()[0];
    assert!(dialog.rows.iter().all(|row| !row.locked));
}

/// REQ: USR-214
#[test]
fn usr_214_cli_grant_of_a_denied_grant_is_refused() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    install_ok(&mut rig, &spec("com.example.clip", "1.0.0", &["clipboard.read"]));
    let output = rig.cli(&["grants", "com.example.clip", "--grant", "clipboard.read"]);
    assert_ne!(result_exit_code(&output), 0);
    assert!(rig.grant_entries("com.example.clip").iter().all(|entry| entry.state != "granted"));
}

// ---- USR-215 ----

/// REQ: USR-215
#[test]
fn usr_215_plugin_requesting_a_denied_grant_installs_runs_and_logs_the_refusal() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    install_ok(&mut rig, &spec("com.example.clip", "1.0.0", &["clipboard.read", "tree.read"]));
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.clip").state, "On");
    let _ = rig.call_needing("com.example.clip", "server", "clipboard.read");
    assert!(
        rig.log_lines("com.example.clip").iter().any(|line| line.contains("clipboard.read") && line.contains("policy")),
        "{:?}",
        rig.log_lines("com.example.clip")
    );
}

/// REQ: USR-215
#[test]
fn usr_215_the_denied_grant_never_disables_the_plugin_over_time() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    install_ok(&mut rig, &spec("com.example.clip", "1.0.0", &["clipboard.read"]));
    rig.start_server();
    rig.restart_server();
    rig.advance_ms(600_000);
    assert_eq!(list_row(&mut rig, "com.example.clip").state, "On");
}
