//! Install and manage plugins (user-install-and-manage.md and packages.md): USR-010 to USR-013,
//! USR-120 to USR-128, USR-140 to USR-143.

use crate::support::*;

// ---- USR-010 ----

/// REQ: USR-010
#[test]
fn usr_010_declined_review_leaves_no_start_file() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewDecline]);
    let package =
        spec("com.example.writer", "1.0.0", &["tree.read"]).start_writes_file("started.txt");
    let output = rig.install_file(&package, &["--allow-unsigned"]);
    assert_ne!(result_exit_code(&output), 0);
    assert!(!rig.data_file_exists("com.example.writer", "started.txt"));
    assert!(rig.list().iter().all(|row| row.id != "com.example.writer"));
}

/// REQ: USR-010
#[test]
fn usr_010_no_package_code_runs_while_the_dialog_is_open() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewPause]);
    let package =
        spec("com.example.writer", "1.0.0", &["tree.read"]).start_writes_file("started.txt");
    let _pending = rig.install_file_nonblocking(&package, &["--allow-unsigned"]);
    rig.advance_ms(5_000);
    assert_eq!(rig.package_instructions_run("com.example.writer"), 0);
    assert!(!rig.data_file_exists("com.example.writer", "started.txt"));
}

/// REQ: USR-010
#[test]
fn usr_010_accepted_review_lets_the_start_function_run_afterwards() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let package =
        spec("com.example.writer", "1.0.0", &["tree.read"]).start_writes_file("started.txt");
    let output = rig.install_file(&package, &["--allow-unsigned"]);
    assert_eq!(result_exit_code(&output), 0);
    rig.start_server();
    rig.advance_ms(2_000);
    assert!(rig.data_file_exists("com.example.writer", "started.txt"));
}

// ---- USR-011 ----

/// REQ: USR-011
#[test]
fn usr_011_bad_file_hash_prints_one_error_naming_the_check_and_changes_nothing() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &["tree.read"]));
    let before_list = rig.list();
    let before_files = tree_snapshot(&rig.data_dir());
    let before_settings = tree_snapshot(&rig.config_dir());
    let output = rig.install_file(
        &spec("com.example.bad", "1.0.0", &[]).corrupt_file_hash(),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(error_codes(&output).len(), 1, "exactly one error record");
    assert!(
        error_codes(&output)[0].starts_with("manifest.file-"),
        "{:?}",
        error_codes(&output)
    );
    assert_eq!(rig.list(), before_list);
    assert_eq!(tree_snapshot(&rig.data_dir()), before_files);
    assert_eq!(tree_snapshot(&rig.config_dir()), before_settings);
}

/// REQ: USR-011
#[test]
fn usr_011_failed_update_keeps_other_plugins_settings_untouched() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.keep", "density", "compact");
    let before = std::fs::read(rig.config_dir().join("config.toml")).unwrap_or_default();
    let _ = rig.install_file(
        &spec("com.example.keep2", "1.0.0", &[]).corrupt_file_hash(),
        &["--allow-unsigned", "--yes"],
    );
    let after = std::fs::read(rig.config_dir().join("config.toml")).unwrap_or_default();
    assert_eq!(before, after);
}

// ---- USR-012 ----

/// REQ: USR-012
#[test]
fn usr_012_grant_file_holds_three_granted_and_two_declined() {
    let mut rig = UserRig::new();
    let package = spec(
        "com.example.mixed",
        "1.0.0",
        &[
            "tree.read",
            "panes.read",
            "notify.toast",
            "terminal.write",
            "clipboard.read",
        ],
    );
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let output = rig.install_file(&package, &["--allow-unsigned"]);
    assert_eq!(result_exit_code(&output), 0);
    let grants = rig.grant_entries("com.example.mixed");
    let granted: Vec<_> = grants
        .iter()
        .filter(|entry| entry.state == "granted")
        .collect();
    let declined: Vec<_> = grants
        .iter()
        .filter(|entry| entry.state == "declined")
        .collect();
    assert_eq!(granted.len(), 3);
    assert_eq!(declined.len(), 2);
    assert!(declined
        .iter()
        .all(|entry| entry.grant == "terminal.write" || entry.grant == "clipboard.read"));
}

/// REQ: USR-012
#[test]
fn usr_012_dialog_lists_every_request_and_high_rows_start_unchecked() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewDecline]);
    let package = spec(
        "com.example.mixed",
        "1.0.0",
        &[
            "tree.read",
            "panes.read",
            "notify.toast",
            "terminal.write",
            "clipboard.read",
        ],
    );
    let _ = rig.install_file(&package, &["--allow-unsigned"]);
    let dialog = &rig.dialogs_shown()[0];
    assert_eq!(dialog.rows.len(), 5);
    for row in &dialog.rows {
        assert_eq!(row.checked, !row.high, "{}", row.grant);
    }
}

/// REQ: USR-012
#[test]
fn usr_012_records_exactly_the_rows_the_user_checked() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::Review(vec![
        "tree.read".into(),
        "terminal.write".into(),
    ])]);
    let package = spec(
        "com.example.mixed",
        "1.0.0",
        &["tree.read", "panes.read", "terminal.write"],
    );
    let _ = rig.install_file(&package, &["--allow-unsigned"]);
    let mut granted: Vec<String> = rig
        .grant_entries("com.example.mixed")
        .into_iter()
        .filter(|entry| entry.state == "granted")
        .map(|entry| entry.grant)
        .collect();
    granted.sort();
    assert_eq!(
        granted,
        vec!["terminal.write".to_string(), "tree.read".to_string()]
    );
}

// ---- USR-013 ----

/// REQ: USR-013
#[test]
fn usr_013_plugin_is_on_after_a_successful_install() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.on", "1.0.0", &["tree.read"]));
    let row = list_row(&mut rig, "com.example.on");
    assert_eq!(row.state, "On");
}

/// REQ: USR-013
#[test]
fn usr_013_plugin_is_off_when_the_user_chose_to_leave_it_off() {
    let mut rig = UserRig::new();
    let output = rig.install_file(
        &spec("com.example.off", "1.0.0", &[]),
        &["--allow-unsigned", "--yes", "--disabled"],
    );
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(list_row(&mut rig, "com.example.off").state, "Off");
}

// ---- USR-120 ----

/// REQ: USR-120
#[test]
fn usr_120_unknown_key_prompts_with_short_fingerprint_and_decline_installs_nothing() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.attach_client(vec![DialogAnswer::TrustDecline]);
    let output = rig.install_file(
        &spec("com.example.signed", "1.0.0", &[]).signed_with(&key),
        &["--yes"],
    );
    assert_eq!(error_codes(&output), vec!["trust.declined".to_string()]);
    let prompts = rig.trust_prompts();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].contains(&key.short_fingerprint()),
        "{}",
        prompts[0]
    );
    assert!(rig.list().iter().all(|row| row.id != "com.example.signed"));
    assert!(rig.trusted_keys().is_empty());
}

/// REQ: USR-120
#[test]
fn usr_120_short_fingerprint_is_the_first_eight_digits() {
    let key = KeyPair::generate("Example Publisher");
    let full = key.fingerprint();
    let short = key.short_fingerprint();
    assert_eq!(short, full[..8]);
}

/// REQ: USR-120
#[test]
fn usr_120_trust_list_shows_the_full_fingerprint_on_request() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    let add = rig.cli(&["trust", "add", &key.public_key_path_string(), "--yes"]);
    assert_eq!(result_exit_code(&add), 0);
    let listing = rig.cli(&["trust", "list"]);
    let text = serde_json::to_string(&listing.stdout_jsonl).unwrap();
    assert!(text.contains(&key.fingerprint()));
}

// ---- USR-121 / USR-128 ----

/// REQ: USR-121, USR-128
#[test]
fn usr_121_removing_a_key_keeps_the_plugin_installed_and_marks_it_untrusted() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    let output = rig.install_file(
        &spec("com.example.signed", "1.0.0", &["tree.read"]).signed_with(&key),
        &["--yes"],
    );
    assert_eq!(result_exit_code(&output), 0);
    let before = list_row(&mut rig, "com.example.signed");
    let removed = rig.cli(&["trust", "remove", &key.fingerprint()]);
    assert_eq!(result_exit_code(&removed), 0);
    let after = list_row(&mut rig, "com.example.signed");
    assert_eq!(after.version, before.version);
    assert_ne!(after.state, before.state, "the plugin stops loading");
    assert!(after.untrusted, "list marks the plugin untrusted");
}

/// REQ: USR-121, USR-128
#[test]
fn usr_128_untrusted_plugin_does_not_load_after_a_server_restart() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    let _ = rig.install_file(
        &spec("com.example.signed", "1.0.0", &["tree.read"]).signed_with(&key),
        &["--yes"],
    );
    let _ = rig.cli(&["trust", "remove", &key.fingerprint()]);
    rig.start_server();
    rig.restart_server();
    assert_ne!(list_row(&mut rig, "com.example.signed").state, "On");
}

/// REQ: USR-128
#[test]
fn usr_128_update_after_key_removal_asks_for_trust_again() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    let _ = rig.install_file(
        &spec("com.example.signed", "1.0.0", &[]).signed_with(&key),
        &["--yes"],
    );
    let _ = rig.cli(&["trust", "remove", &key.fingerprint()]);
    rig.attach_client(vec![DialogAnswer::TrustDecline]);
    let output = rig.install_file(
        &spec("com.example.signed", "1.1.0", &[]).signed_with(&key),
        &["--yes"],
    );
    assert_eq!(rig.trust_prompts().len(), 1);
    assert_eq!(error_codes(&output), vec!["trust.declined".to_string()]);
    assert_eq!(list_row(&mut rig, "com.example.signed").version, "1.0.0");
}

// ---- USR-122 / USR-126 ----

/// REQ: USR-122, USR-126
#[test]
fn usr_122_update_with_an_unendorsed_key_is_refused() {
    let mut rig = UserRig::new();
    let key_a = KeyPair::generate("Publisher A");
    let key_b = KeyPair::generate("Publisher B");
    rig.trust(&key_a);
    let _ = rig.install_file(
        &spec("com.example.signed", "1.0.0", &[]).signed_with(&key_a),
        &["--yes"],
    );
    rig.attach_client(vec![DialogAnswer::TrustDecline]);
    let output = rig.install_file(
        &spec("com.example.signed", "2.0.0", &[]).signed_with(&key_b),
        &["--yes"],
    );
    assert_eq!(
        error_codes(&output),
        vec!["package.publisher-key-changed".to_string()]
    );
    assert_eq!(list_row(&mut rig, "com.example.signed").version, "1.0.0");
}

/// REQ: USR-122
#[test]
fn usr_122_update_signed_by_the_same_trusted_key_needs_no_trust_prompt() {
    let mut rig = UserRig::new();
    let key_a = KeyPair::generate("Publisher A");
    rig.trust(&key_a);
    let _ = rig.install_file(
        &spec("com.example.signed", "1.0.0", &[]).signed_with(&key_a),
        &["--yes"],
    );
    let output = rig.install_file(
        &spec("com.example.signed", "1.1.0", &[]).signed_with(&key_a),
        &["--yes"],
    );
    assert_eq!(result_exit_code(&output), 0);
    assert!(rig.trust_prompts().is_empty());
}

/// REQ: USR-126
#[test]
fn usr_126_update_with_an_equal_or_lower_version_is_refused() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.up", "1.2.0", &[]));
    for version in ["1.2.0", "1.1.9"] {
        let output = rig.install_file(
            &spec("com.example.up", version, &[]),
            &["--allow-unsigned", "--yes"],
        );
        assert_ne!(result_exit_code(&output), 0, "{version}");
    }
    let output = rig.install_file(
        &spec("com.example.up", "1.2.1", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(list_row(&mut rig, "com.example.up").id, "com.example.up");
}

// ---- USR-123 / USR-173 ----

/// REQ: USR-123, USR-173
#[test]
fn usr_123_update_dialog_shows_only_the_new_grant_and_keeps_earlier_answers() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::Review(vec!["tree.read".into()])]);
    let _ = rig.install_file(
        &spec("com.example.grow", "1.0.0", &["tree.read", "panes.read"]),
        &["--allow-unsigned"],
    );
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let output = rig.install_file(
        &spec(
            "com.example.grow",
            "1.1.0",
            &["tree.read", "panes.read", "notify.toast"],
        ),
        &["--allow-unsigned"],
    );
    assert_eq!(result_exit_code(&output), 0);
    let dialogs = rig.dialogs_shown();
    let last = dialogs.last().expect("an update dialog");
    assert_eq!(last.rows.len(), 1);
    assert_eq!(last.rows[0].grant, "notify.toast");
    let entries = rig.grant_entries("com.example.grow");
    let state_of = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.grant == name)
            .map(|entry| entry.state.clone())
    };
    assert_eq!(state_of("tree.read").as_deref(), Some("granted"));
    assert_eq!(
        state_of("panes.read").as_deref(),
        Some("declined"),
        "an earlier decline is kept"
    );
}

/// REQ: USR-123, USR-173
#[test]
fn usr_123_update_without_new_grants_shows_no_permission_dialog() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let _ = rig.install_file(
        &spec("com.example.same", "1.0.0", &["tree.read"]),
        &["--allow-unsigned"],
    );
    let shown_before = rig.dialogs_shown().len();
    let _ = rig.install_file(
        &spec("com.example.same", "1.0.1", &["tree.read"]),
        &["--allow-unsigned"],
    );
    assert_eq!(rig.dialogs_shown().len(), shown_before);
}

// ---- USR-124 ----

/// REQ: USR-124
#[test]
fn usr_124_failed_activation_rolls_back_version_and_data() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.roll", "1.0.0", &["tree.read"]).start_writes_file("data.txt"),
    );
    rig.start_server();
    rig.advance_ms(1_000);
    let data_before = rig.read_data_file("com.example.roll", "data.txt");
    let output = rig.install_file(
        &spec("com.example.roll", "2.0.0", &["tree.read"]).activation_traps(),
        &["--allow-unsigned", "--yes"],
    );
    assert_ne!(result_exit_code(&output), 0);
    assert!(
        rig.notices()
            .iter()
            .any(|text| text.contains("Update failed, rolled back")),
        "{:?}",
        rig.notices()
    );
    assert_eq!(list_row(&mut rig, "com.example.roll").version, "1.0.0");
    assert_eq!(
        rig.read_data_file("com.example.roll", "data.txt"),
        data_before
    );
}

/// REQ: USR-124
#[test]
fn usr_124_rollback_leaves_no_staging_folder_behind() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.roll", "1.0.0", &[]));
    let _ = rig.install_file(
        &spec("com.example.roll", "2.0.0", &[]).activation_traps(),
        &["--allow-unsigned", "--yes"],
    );
    let staging = rig.data_dir().join("plugins/staging");
    let leftovers = std::fs::read_dir(&staging)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
}

// ---- USR-125 ----

/// REQ: USR-125
#[test]
fn usr_125_no_update_request_in_one_hour_with_two_plugins() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.one", "1.0.0", &[]));
    install_ok(&mut rig, &spec("com.example.two", "1.0.0", &[]));
    rig.start_server();
    rig.advance_ms(3_600_000);
    assert!(
        rig.network_requests().is_empty(),
        "{:?}",
        rig.network_requests()
    );
}

/// REQ: USR-125
#[test]
fn usr_125_registry_configured_still_no_automatic_check() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.one", "1.0.0", &[]));
    rig.configure_registry("https://registry.example.test/index.json");
    rig.start_server();
    rig.advance_ms(3_600_000);
    assert_eq!(rig.fetch_calls(), 0);
}

// ---- USR-127 ----

/// REQ: USR-127
#[test]
fn usr_127_script_install_with_new_key_without_yes_is_refused_and_creates_no_trust() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.set_ci(true);
    let output = rig.install_file(
        &spec("com.example.signed", "1.0.0", &[]).signed_with(&key),
        &[],
    );
    assert_ne!(result_exit_code(&output), 0);
    assert!(rig.trusted_keys().is_empty());
    assert!(rig.list().iter().all(|row| row.id != "com.example.signed"));
}

/// REQ: USR-127
#[test]
fn usr_127_trust_is_stored_per_key_in_the_user_trust_list() {
    let mut rig = UserRig::new();
    let key_a = KeyPair::generate("A");
    let key_b = KeyPair::generate("B");
    rig.trust(&key_a);
    assert_eq!(rig.trusted_keys().len(), 1);
    assert!(rig.trust_store_path().starts_with(rig.state_dir()));
    rig.trust(&key_b);
    assert_eq!(rig.trusted_keys().len(), 2);
}

// ---- USR-140 ----

/// REQ: USR-140
#[test]
fn usr_140_crash_after_the_removal_mark_is_finished_at_next_start() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &["tree.read"]));
    rig.kill_host_at(KillPoint::AfterUninstallMark);
    let _ = rig.cli(&["uninstall", "com.example.gone", "--yes"]);
    assert!(rig
        .list()
        .iter()
        .any(|row| row.id == "com.example.gone" && row.state.contains("Removing")));
    rig.start_server();
    assert!(rig.list().iter().all(|row| row.id != "com.example.gone"));
    assert!(!rig
        .data_dir()
        .join("plugins/store/com.example.gone")
        .exists());
}

/// REQ: USR-140
#[test]
fn usr_140_uninstall_order_is_mark_then_stop_then_delete() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &[]));
    rig.start_server();
    let _ = rig.cli(&["uninstall", "com.example.gone", "--yes"]);
    let steps = rig.uninstall_trace("com.example.gone");
    assert_eq!(
        steps,
        vec!["mark-removing", "stop", "delete-files", "unregister"]
    );
}

// ---- USR-141 ----

/// REQ: USR-141
#[test]
fn usr_141_uninstall_leaves_other_plugins_data_and_trusted_keys() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    install_ok(
        &mut rig,
        &spec("com.example.other", "1.0.0", &[]).start_writes_file("keep.txt"),
    );
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &[]));
    rig.start_server();
    rig.advance_ms(1_000);
    let other_before = tree_snapshot(&rig.data_dir().join("plugins/data/com.example.other"));
    let _ = rig.cli(&["uninstall", "com.example.gone", "--purge", "--yes"]);
    assert_eq!(
        tree_snapshot(&rig.data_dir().join("plugins/data/com.example.other")),
        other_before
    );
    assert_eq!(rig.trusted_keys().len(), 1);
}

/// REQ: USR-141
#[test]
fn usr_141_uninstall_of_an_unknown_id_changes_nothing() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.other", "1.0.0", &[]));
    let before = tree_snapshot(&rig.data_dir());
    let output = rig.cli(&["uninstall", "com.example.never", "--yes"]);
    assert_ne!(result_exit_code(&output), 0);
    assert_eq!(tree_snapshot(&rig.data_dir()), before);
}

// ---- USR-142 ----

/// REQ: USR-142
#[test]
fn usr_142_pane_of_a_removed_plugin_stays_with_a_placeholder_and_returns() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.panes", "1.0.0", &["panes.provide"]).provides_pane("notes"),
    );
    rig.start_server();
    let pane = rig.open_plugin_pane("com.example.panes", "notes");
    let _ = rig.cli(&["uninstall", "com.example.panes", "--yes"]);
    assert!(rig.pane_exists(pane), "the pane stays in the tree");
    assert_eq!(rig.pane_content_kind(pane), "placeholder");
    install_ok(
        &mut rig,
        &spec("com.example.panes", "1.0.0", &["panes.provide"]).provides_pane("notes"),
    );
    rig.start_server();
    assert_eq!(rig.pane_content_kind(pane), "plugin");
}

/// REQ: USR-142
#[test]
fn usr_142_placeholder_survives_a_server_restart() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.panes", "1.0.0", &["panes.provide"]).provides_pane("notes"),
    );
    rig.start_server();
    let pane = rig.open_plugin_pane("com.example.panes", "notes");
    let _ = rig.cli(&["uninstall", "com.example.panes", "--yes"]);
    rig.restart_server();
    assert_eq!(rig.pane_content_kind(pane), "placeholder");
}

// ---- USR-143 ----

/// REQ: USR-143
#[test]
fn usr_143_dependent_becomes_dormant_not_failed_and_wakes_with_the_owner() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.child", "1.0.0", &[]).contributes_to("com.example.owner", "owner.point"),
    );
    rig.start_server();
    let _ = rig.cli(&["uninstall", "com.example.owner", "--yes"]);
    let row = list_row(&mut rig, "com.example.child");
    assert_eq!(row.state, "Waiting for com.example.owner");
    assert_ne!(row.state, "Failed");
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    rig.start_server();
    assert_eq!(list_row(&mut rig, "com.example.child").state, "On");
}

/// REQ: USR-143
#[test]
fn usr_143_dormant_dependent_does_not_count_a_failure() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.child", "1.0.0", &[]).contributes_to("com.example.owner", "owner.point"),
    );
    rig.start_server();
    let _ = rig.cli(&["uninstall", "com.example.owner", "--yes"]);
    rig.advance_ms(600_000);
    assert_eq!(rig.failure_count("com.example.child"), 0);
}
