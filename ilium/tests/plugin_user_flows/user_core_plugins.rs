//! Core plugins for users (user-core-plugins.md): USR-040 to USR-043.

use crate::support::*;

// ---- USR-040 ----

/// REQ: USR-040
#[test]
fn usr_040_list_shows_30_core_rows_with_tier_t1() {
    let mut rig = UserRig::new();
    let rows = rig.list();
    let core: Vec<_> = rows.iter().filter(|row| row.id.starts_with("ilium.core.")).collect();
    assert_eq!(core.len(), 30);
    assert!(core.iter().all(|row| row.tier == "T1"));
}

/// REQ: USR-040
#[test]
fn usr_040_uninstall_of_a_core_plugin_fails_with_not_removable() {
    let mut rig = UserRig::new();
    let output = rig.cli(&["uninstall", "ilium.core.tree", "--yes"]);
    assert_eq!(error_codes(&output), vec!["plugin.not-removable".to_string()]);
    assert_eq!(rig.list().iter().filter(|row| row.id.starts_with("ilium.core.")).count(), 30);
}

/// REQ: USR-040
#[test]
fn usr_040_every_core_id_of_the_manual_page_is_listed() {
    let mut rig = UserRig::new();
    let listed: Vec<String> = rig.list().into_iter().map(|row| row.id).collect();
    for id in core_plugin_ids_from_manual() {
        assert!(listed.contains(&id), "{id} missing from list");
    }
}

/// REQ: USR-040
#[test]
fn usr_040_no_core_plugin_can_be_uninstalled() {
    let mut rig = UserRig::new();
    for id in core_plugin_ids_from_manual() {
        let output = rig.cli(&["uninstall", &id, "--yes"]);
        assert_eq!(error_codes(&output), vec!["plugin.not-removable".to_string()], "{id}");
    }
}

// ---- USR-041 (host side: disable refusal) ----

/// REQ: USR-041
#[test]
fn usr_041_disable_of_a_plugin_marked_no_is_refused_with_a_reason() {
    let mut rig = UserRig::new();
    let protected = core_plugins_that_cannot_be_turned_off();
    assert!(protected.contains(&"ilium.core.tree".to_string()), "the table must mark ilium.core.tree as no");
    for id in protected {
        let output = rig.cli(&["disable", &id]);
        assert_ne!(result_exit_code(&output), 0, "{id}");
        assert!(!error_messages(&output).join(" ").is_empty(), "{id}: a reason is printed");
        assert_eq!(list_row(&mut rig, &id).state, "On", "{id}");
    }
}

/// REQ: USR-041
#[test]
fn usr_041_a_core_plugin_marked_yes_can_be_turned_off_and_on() {
    let mut rig = UserRig::new();
    let protected = core_plugins_that_cannot_be_turned_off();
    let free: Vec<String> = core_plugin_ids_from_manual().into_iter().filter(|id| !protected.contains(id)).collect();
    assert!(!free.is_empty());
    let id = &free[0];
    assert_eq!(result_exit_code(&rig.cli(&["disable", id])), 0);
    assert_eq!(list_row(&mut rig, id).state, "Off");
    assert_eq!(result_exit_code(&rig.cli(&["enable", id])), 0);
    assert_eq!(list_row(&mut rig, id).state, "On");
}

// ---- USR-042 ----

/// REQ: USR-042
#[test]
fn usr_042_replacing_settings_or_sessions_fails_with_replaces_not_replaceable() {
    for target in ["ilium.core.settings", "ilium.core.sessions"] {
        let mut rig = UserRig::new();
        let output = rig.install_file(&spec("com.example.repl", "1.0.0", &[]).replaces(&[target]), &["--allow-unsigned", "--yes"]);
        assert_eq!(error_codes(&output), vec!["manifest.replaces-not-replaceable".to_string()], "{target}");
        assert!(rig.list().iter().all(|row| row.id != "com.example.repl"));
    }
}

/// REQ: USR-042
#[test]
fn usr_042_replacing_a_replaceable_core_plugin_does_not_fail_with_that_code() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReplaceInstallWithout]);
    let output = rig.install_file(&spec("com.example.repl", "1.0.0", &[]).replaces(&["ilium.core.editor"]), &["--allow-unsigned"]);
    assert!(error_codes(&output).is_empty(), "{:?}", error_codes(&output));
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(rig.replace_prompts().len(), 1);
    assert!(rig.list().iter().any(|row| row.id == "com.example.repl"));
}

// ---- USR-043 ----

/// REQ: USR-043
#[test]
fn usr_043_five_replaces_entries_are_refused_and_four_are_asked() {
    let mut rig = UserRig::new();
    let five = ["ilium.core.editor", "ilium.core.kanban", "ilium.core.smart-copy", "ilium.core.agent-detect", "ilium.core.agent-toolbar"];
    let refused = rig.install_file(&spec("com.example.five", "1.0.0", &[]).replaces(&five), &["--allow-unsigned", "--yes"]);
    assert_ne!(result_exit_code(&refused), 0, "n+1");
    rig.attach_client(vec![DialogAnswer::ReplaceInstallWithout; 4]);
    let four = rig.install_file(&spec("com.example.four", "1.0.0", &[]).replaces(&five[..4]), &["--allow-unsigned"]);
    assert_eq!(result_exit_code(&four), 0, "n");
    assert_eq!(rig.replace_prompts().len(), 4);
}

/// REQ: USR-043
#[test]
fn usr_043_one_entry_prompts_and_install_without_does_not_replace() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReplaceInstallWithout]);
    let output = rig.install_file(&spec("com.example.one", "1.0.0", &[]).replaces(&["ilium.core.editor"]), &["--allow-unsigned"]);
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(rig.replace_prompts().len(), 1);
    assert!(rig.replacements().is_empty());
}

/// REQ: USR-043
#[test]
fn usr_043_yes_replaces_and_no_prompt_means_no_replacement() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::ReplaceYes]);
    let _ = rig.install_file(&spec("com.example.one", "1.0.0", &[]).replaces(&["ilium.core.editor"]), &["--allow-unsigned"]);
    assert_eq!(rig.replacements(), vec![("ilium.core.editor".to_string(), "com.example.one".to_string())]);
    let mut script_rig = UserRig::new();
    script_rig.set_ci(true);
    let _ = script_rig.install_file(&spec("com.example.two", "1.0.0", &[]).replaces(&["ilium.core.editor"]), &["--allow-unsigned", "--yes"]);
    assert!(script_rig.replacements().is_empty(), "--yes never confirms a replacement");
}
