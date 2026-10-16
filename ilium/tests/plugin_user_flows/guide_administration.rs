//! Administration guide (guide-administration.md): USR-070 to USR-073.

use crate::support::*;

// ---- USR-070 ----

/// REQ: USR-070
#[test]
fn usr_070_allowed_tiers_t3_on_windows_still_refuses_t3() {
    let mut rig = UserRig::new_on(Platform::Windows);
    rig.write_system_policy("version = 1\n[packages]\nallowed_tiers = [\"T3\"]\nallow_third_party = true\n");
    let output = rig.install_file(&spec("com.example.win", "1.0.0", &[]), &["--allow-unsigned", "--yes"]);
    assert_ne!(result_exit_code(&output), 0);
    assert!(error_messages(&output).iter().any(|message| message.starts_with("Cannot install")), "{:?}", error_messages(&output));
    assert!(rig.list().iter().all(|row| row.id != "com.example.win"));
}

/// REQ: USR-070
#[test]
fn usr_070_a_permissive_policy_on_macos_does_not_enable_third_party_plugins() {
    let mut rig = UserRig::new_on(Platform::MacOs);
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = true\nallowed_tiers = [\"T1\", \"T2\", \"T3\"]\n");
    let output = rig.install_file(&spec("com.example.mac", "1.0.0", &[]), &["--allow-unsigned", "--yes"]);
    assert_ne!(result_exit_code(&output), 0);
}

/// REQ: USR-070
#[test]
fn usr_070_a_policy_cannot_grant_what_the_user_or_the_manifest_did_not() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = true\n");
    install_ok(&mut rig, &spec("com.example.plain", "1.0.0", &["tree.read"]));
    assert!(rig.grant_entries("com.example.plain").iter().all(|entry| entry.state != "granted" || entry.grant == "tree.read"));
    assert_eq!(rig.call_needing_unlisted("com.example.plain", "server", "clipboard.read"), CallOutcome::Trapped { code: "host.capability-not-requested".into() });
}

// ---- USR-071 ----

/// REQ: USR-071
#[test]
fn usr_071_non_interactive_install_without_yes_fails_with_review_required() {
    let mut rig = UserRig::new();
    rig.set_ci(true);
    let output = rig.install_file(&spec("com.example.batch", "1.0.0", &["tree.read"]), &["--allow-unsigned"]);
    assert_eq!(error_codes(&output), vec!["install.review-required".to_string()]);
    assert!(rig.list().iter().all(|row| row.id != "com.example.batch"));
}

/// REQ: USR-071
#[test]
fn usr_071_yes_declines_a_high_row_that_is_not_named() {
    let mut rig = UserRig::new();
    rig.set_ci(true);
    let output = rig.install_file(&spec("com.example.batch", "1.0.0", &["tree.read", "terminal.write"]), &["--allow-unsigned", "--yes"]);
    assert_eq!(result_exit_code(&output), 0);
    let entries = rig.grant_entries("com.example.batch");
    assert!(entries.iter().any(|entry| entry.grant == "tree.read" && entry.state == "granted"));
    assert!(entries.iter().any(|entry| entry.grant == "terminal.write" && entry.state == "declined"));
}

/// REQ: USR-071
#[test]
fn usr_071_yes_with_grant_names_the_high_row_and_grants_it() {
    let mut rig = UserRig::new();
    rig.set_ci(true);
    let output = rig.install_file(&spec("com.example.batch", "1.0.0", &["terminal.write"]), &["--allow-unsigned", "--yes", "--grant", "terminal.write"]);
    assert_eq!(result_exit_code(&output), 0);
    assert!(rig.grant_entries("com.example.batch").iter().any(|entry| entry.grant == "terminal.write" && entry.state == "granted"));
}

// ---- USR-072 ----

/// REQ: USR-072
#[test]
fn usr_072_blocking_a_running_plugin_takes_effect_at_the_next_start_only() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.target", "1.0.0", &[]));
    rig.start_server();
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.target\"]\n");
    rig.advance_ms(5_000);
    assert_eq!(list_row(&mut rig, "com.example.target").state, "On", "a running plugin is not stopped by an edit");
    rig.restart_server();
    let row = list_row(&mut rig, "com.example.target");
    assert_eq!(row.state, "Blocked by policy");
    assert_eq!(row.reason.as_deref(), Some("policy.blocked"));
}

/// REQ: USR-072
#[test]
fn usr_072_removing_the_id_lets_the_plugin_run_again_at_the_next_start() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.target", "1.0.0", &[]));
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.target\"]\n");
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.target").state, "Blocked by policy");
    rig.write_system_policy("version = 1\n");
    rig.restart_server();
    assert_eq!(list_row(&mut rig, "com.example.target").state, "On");
}

/// REQ: USR-072
#[test]
fn usr_072_the_block_is_not_written_to_the_registry() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.target", "1.0.0", &[]));
    let registry = rig.data_dir().join("plugins/installed.json");
    let before = std::fs::read(&registry).expect("registry");
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.target\"]\n");
    rig.start_server();
    rig.stop_server();
    assert_eq!(std::fs::read(&registry).expect("registry"), before);
}

// ---- USR-073 ----

/// REQ: USR-073
#[test]
fn doc_usr_073_page_states_that_the_policy_only_restricts() {
    let page = manual_page("guide-administration");
    assert!(page.contains("A policy can only make Ilium stricter"));
}

/// REQ: USR-073
#[test]
fn doc_usr_073_page_states_where_the_file_lives_on_each_system() {
    let page = manual_page("guide-administration");
    assert!(page.contains("/etc/ilium/policy.toml"));
    assert!(page.contains("/Library/Application Support/Ilium/policy.toml"));
    assert!(page.contains("%ProgramData%\\Ilium\\policy.toml"));
}

/// REQ: USR-073
#[test]
fn doc_usr_073_page_states_how_to_respond_to_a_bad_plugin() {
    let page = manual_page("guide-administration").to_lowercase();
    assert!(page.contains("deny_ids"), "block by id");
    assert!(page.contains("bad plugin") || page.contains("respond"), "a response section exists");
}
