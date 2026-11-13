//! Install check order, error text, uninstall cascade, keys and N14 (security.md, lifecycle.md,
//! packages.md): USR-129, USR-130, USR-170 to USR-173.

use crate::support::*;

fn store_snapshot(rig: &UserRig) -> std::collections::BTreeMap<String, Vec<u8>> {
    tree_snapshot(&rig.data_dir().join("plugins/store"))
}

// ---- USR-170 ----

/// REQ: USR-170
#[test]
fn usr_170_each_failing_check_names_its_code_and_writes_nothing_to_the_store() {
    let cases: Vec<(&str, PackageSpec, &str)> = vec![
        (
            "archive",
            spec("com.example.c1", "1.0.0", &[]).corrupt_archive(),
            "package.zip-",
        ),
        (
            "api version",
            spec("com.example.c2", "1.0.0", &[]).api_version("9.9"),
            "manifest.api-version",
        ),
        (
            "file hash",
            spec("com.example.c3", "1.0.0", &[]).corrupt_file_hash(),
            "manifest.file-",
        ),
        (
            "grant string",
            spec("com.example.c4", "1.0.0", &["not a grant!"]),
            "manifest.grant-",
        ),
        (
            "grant tier",
            spec("com.example.c5", "1.0.0", &["plugins.manage"]),
            "manifest.grant-tier",
        ),
    ];
    for (label, package, prefix) in cases {
        let mut rig = UserRig::new();
        let output = rig.install_file(&package, &["--allow-unsigned", "--yes"]);
        let codes = error_codes(&output);
        assert_eq!(codes.len(), 1, "{label}: {codes:?}");
        assert!(codes[0].starts_with(prefix), "{label}: {codes:?}");
        assert!(
            store_snapshot(&rig).is_empty(),
            "{label}: the store must stay empty"
        );
    }
}

/// REQ: USR-170
#[test]
fn usr_170_a_package_failing_two_checks_reports_the_earlier_one() {
    let mut rig = UserRig::new();
    let both = spec("com.example.two", "1.0.0", &["plugins.manage"]).corrupt_file_hash();
    let output = rig.install_file(&both, &["--allow-unsigned", "--yes"]);
    assert_eq!(
        error_codes(&output),
        vec!["manifest.file-hash".to_string()],
        "step 1-3 (file inventory) comes before step 8-9 (grant tier)"
    );
    assert!(store_snapshot(&rig).is_empty());
}

/// REQ: USR-170
#[test]
fn usr_170_signature_is_checked_before_policy_and_policy_before_trust() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[packages]\nallow_third_party = false\n");
    let key = KeyPair::generate("Example Publisher");
    let bad_signature = spec("com.example.sig", "1.0.0", &[])
        .signed_with(&key)
        .corrupt_signature();
    let first = rig.install_file(&bad_signature, &["--yes"]);
    assert_eq!(
        error_codes(&first),
        vec!["manifest.signature-invalid".to_string()]
    );
    let unknown_key = spec("com.example.pol", "1.0.0", &[]).signed_with(&key);
    let second = rig.install_file(&unknown_key, &["--yes"]);
    assert_eq!(
        error_codes(&second),
        vec!["policy.blocked".to_string()],
        "policy answers before the trust prompt"
    );
    assert!(rig.trust_prompts().is_empty());
}

/// REQ: USR-170
#[test]
fn usr_170_the_plugins_screen_uses_the_same_order_as_the_command() {
    let mut rig = UserRig::new();
    let package = spec("com.example.both", "1.0.0", &["plugins.manage"]).corrupt_file_hash();
    let via_cli = rig.install_file(&package, &["--allow-unsigned", "--yes"]);
    let via_screen = rig.install_from_plugins_screen(&package);
    assert_eq!(error_codes(&via_cli), error_codes(&via_screen));
}

// ---- USR-171 ----

/// REQ: USR-171
#[test]
fn usr_171_hash_failure_text_begins_cannot_install_name_version() {
    let mut rig = UserRig::new();
    let output = rig.install_file(
        &spec("com.example.notes", "1.2.0", &[])
            .display_name("Notes")
            .corrupt_file_hash(),
        &["--allow-unsigned", "--yes"],
    );
    let first_line = error_messages(&output)[0]
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert_eq!(first_line, "Cannot install Notes 1.2.0.");
    let all = error_messages(&output).join("\n");
    assert!(
        all.lines().count() > 1,
        "a plain-words check name follows: {all}"
    );
}

/// REQ: USR-171
#[test]
fn usr_171_tier_failure_text_begins_the_same_way() {
    let mut rig = UserRig::new_on(Platform::Windows);
    let output = rig.install_file(
        &spec("com.example.notes", "1.2.0", &[]).display_name("Notes"),
        &["--allow-unsigned", "--yes"],
    );
    let first_line = error_messages(&output)[0]
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert_eq!(first_line, "Cannot install Notes 1.2.0.");
}

/// REQ: USR-171
#[test]
fn usr_171_a_successful_install_prints_no_cannot_install_line() {
    let mut rig = UserRig::new();
    let output = rig.install_file(
        &spec("com.example.fine", "1.0.0", &[]),
        &["--allow-unsigned", "--yes"],
    );
    assert!(error_messages(&output).is_empty());
}

// ---- USR-172 ----

/// REQ: USR-172
#[test]
fn usr_172_uninstall_applies_every_cascade_row_in_one_host_step() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.provider", "1.0.0", &["panes.provide"])
            .provides_service("greeter")
            .provides_pane("notes")
            .defines_point("provider.point"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.consumer", "1.0.0", &[])
            .consumes_service("com.example.provider", "greeter"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.contrib", "1.0.0", &[])
            .contributes_to("com.example.provider", "provider.point"),
    );
    rig.start_server();
    let pane = rig.open_plugin_pane("com.example.provider", "notes");
    rig.extensions_move_up(
        "com.example.provider",
        "provider.point",
        "com.example.contrib",
    );
    let observed = rig.uninstall_observing_steps("com.example.provider");
    assert_eq!(
        observed.registry_removal_step, observed.cascade_step,
        "one host step"
    );
    assert_eq!(
        list_row(&mut rig, "com.example.consumer").state,
        "On",
        "consumer stays active"
    );
    assert_eq!(
        rig.call_service("com.example.consumer", "com.example.provider", "greeter"),
        "not-found"
    );
    assert_eq!(
        list_row(&mut rig, "com.example.contrib").state,
        "Waiting for com.example.provider"
    );
    assert_eq!(rig.pane_content_kind(pane), "placeholder");
    assert!(
        rig.extension_order_entries("com.example.provider")
            .is_empty(),
        "order entries deleted"
    );
}

/// REQ: USR-172
#[test]
fn usr_172_contribution_to_another_plugins_point_disappears_at_once() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.contrib", "1.0.0", &[])
            .contributes_to("com.example.owner", "owner.point"),
    );
    rig.start_server();
    let _ = rig.cli(&["uninstall", "com.example.contrib", "--yes"]);
    assert!(rig
        .call_point_order("com.example.owner", "owner.point")
        .is_empty());
}

/// REQ: USR-172
#[test]
fn usr_172_a_replaced_core_plugin_turns_back_on() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReplaceYes]);
    let _ = rig.install_file(
        &spec("com.example.repl", "1.0.0", &[]).replaces(&["ilium.core.editor"]),
        &["--allow-unsigned"],
    );
    rig.start_server();
    assert_eq!(list_row(&mut rig, "ilium.core.editor").state, "Off");
    let _ = rig.cli(&["uninstall", "com.example.repl", "--yes"]);
    assert_eq!(list_row(&mut rig, "ilium.core.editor").state, "On");
}

/// REQ: USR-172
#[test]
fn usr_172_keys_menu_items_status_segments_and_toolbar_buttons_disappear() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.deco", "1.0.0", &[]).adds_decorations(),
    );
    rig.start_server();
    assert!(rig.decoration_count("com.example.deco") >= 4);
    let _ = rig.cli(&["uninstall", "com.example.deco", "--yes"]);
    assert_eq!(rig.decoration_count("com.example.deco"), 0);
}

// ---- USR-173 ----

/// REQ: USR-173
#[test]
fn usr_173_update_with_one_new_grant_shows_one_row_and_keeps_old_answers() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::Review(vec!["tree.read".into()])]);
    let _ = rig.install_file(
        &spec("com.example.up", "1.0.0", &["tree.read", "panes.read"]),
        &["--allow-unsigned"],
    );
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let _ = rig.install_file(
        &spec(
            "com.example.up",
            "1.1.0",
            &["tree.read", "panes.read", "notify.toast"],
        ),
        &["--allow-unsigned"],
    );
    let last = rig.dialogs_shown().pop().expect("dialog");
    assert_eq!(last.rows.len(), 1);
    let entries = rig.grant_entries("com.example.up");
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.state == "granted")
            .count(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.state == "declined")
            .count(),
        1
    );
}

/// REQ: USR-173
#[test]
fn usr_173_failed_update_rolls_back_and_keeps_the_answers() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let _ = rig.install_file(
        &spec("com.example.up", "1.0.0", &["tree.read"]),
        &["--allow-unsigned"],
    );
    let before = rig.grant_entries("com.example.up");
    rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
    let output = rig.install_file(
        &spec("com.example.up", "1.1.0", &["tree.read", "notify.toast"]).activation_traps(),
        &["--allow-unsigned"],
    );
    assert_ne!(result_exit_code(&output), 0);
    assert_eq!(list_row(&mut rig, "com.example.up").version, "1.0.0");
    assert_eq!(rig.grant_entries("com.example.up"), before);
}

// ---- USR-129 ----

/// REQ: USR-129
#[test]
fn usr_129_keygen_reports_the_private_key_path_with_mode_0600() {
    let mut rig = UserRig::new();
    let out_dir = rig.scratch_dir().join("keys");
    let output = rig.cli(&[
        "keygen",
        "--publisher-id",
        "example",
        "--publisher",
        "Example Publisher",
        "--out",
        &out_dir.to_string_lossy(),
    ]);
    assert_eq!(result_exit_code(&output), 0);
    let private_key = out_dir.join("publisher.key");
    assert!(private_key.exists());
    assert_eq!(file_mode(&private_key), 0o600);
    let text = serde_json::to_string(&output.stdout_jsonl).unwrap();
    assert!(
        text.contains("publisher.key"),
        "the output names the private key path"
    );
}

/// REQ: USR-129
#[test]
fn usr_129_a_built_package_never_contains_the_private_key() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    let bytes = spec("com.example.signed", "1.0.0", &[])
        .signed_with(&key)
        .build_bytes();
    let secret = key.private_key_bytes();
    assert!(!secret.is_empty());
    assert!(!bytes
        .windows(secret.len())
        .any(|window| window == secret.as_slice()));
    let _ = &mut rig;
}

/// REQ: USR-129
#[test]
fn usr_129_keygen_refuses_to_overwrite_an_existing_key() {
    let mut rig = UserRig::new();
    let out_dir = rig.scratch_dir().join("keys");
    let args = [
        "keygen",
        "--publisher-id",
        "example",
        "--publisher",
        "Example Publisher",
        "--out",
        &out_dir.to_string_lossy(),
    ]
    .map(str::to_string);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = rig.cli(&refs);
    let second = rig.cli(&refs);
    assert_eq!(error_codes(&second), vec!["trust.key-exists".to_string()]);
}

// ---- USR-130 (state and text; the footer render is in the ilium-client lane) ----

/// REQ: USR-130
#[test]
fn usr_130_n14_text_for_an_activate_trap_matches_the_template() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.cursor", "1.0.0", &[])
            .display_name("Cursor keeper")
            .activation_traps(),
    );
    rig.start_server();
    rig.advance_ms(7_000);
    let text = rig
        .n14_text("com.example.cursor")
        .expect("N14 after three failures");
    assert_eq!(
        text,
        "Plugin Cursor keeper stopped after 3 failures in 10 minutes: activate trap. Turn it on again in Settings > Plugins."
    );
}

/// REQ: USR-130
#[test]
fn usr_130_n14_names_the_view_only_for_views_render() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.view", "1.0.0", &[])
            .display_name("Viewer")
            .render_traps("sidebar"),
    );
    rig.start_server();
    rig.advance_ms(7_000);
    let text = rig.n14_text("com.example.view").expect("N14");
    assert!(text.contains("views.render trap (sidebar)"), "{text}");
    assert!(text.starts_with("Plugin Viewer stopped after 3 failures in 10 minutes: "));
}

/// REQ: USR-130
#[test]
fn usr_130_other_exports_do_not_name_a_view() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.hook", "1.0.0", &[])
            .display_name("Hooker")
            .export_traps("commands.run"),
    );
    rig.start_server();
    rig.advance_ms(7_000);
    let text = rig.n14_text("com.example.hook").expect("N14");
    assert!(text.contains("commands.run trap"), "{text}");
    assert!(!text.contains('('), "{text}");
}

/// REQ: USR-130
#[test]
fn usr_130_two_failures_show_no_notice() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.cursor", "1.0.0", &[])
            .display_name("Cursor keeper")
            .activation_traps(),
    );
    rig.start_server();
    rig.advance_ms(1_100);
    assert!(rig.n14_text("com.example.cursor").is_none(), "n-1 failures");
}
