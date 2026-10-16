//! Plugin files on disk (reference-plugin-files.md): USR-200 to USR-209.

use crate::support::*;
use std::path::PathBuf;

// ---- USR-200 ----

/// REQ: USR-200
#[test]
fn usr_200_install_writes_only_below_the_three_xdg_folders() {
    let mut rig = UserRig::new_with_xdg_isolation();
    install_ok(&mut rig, &spec("com.example.where", "1.0.0", &["fs.write:data", "fs.write:cache"]).start_writes_file("a.txt"));
    rig.start_server();
    rig.advance_ms(1_000);
    let outside = rig.files_outside_xdg_roots();
    assert!(outside.is_empty(), "plugin files outside the roots: {outside:?}");
}

/// REQ: USR-200
#[test]
fn usr_200_the_roots_come_from_the_project_name_ilium() {
    let rig = UserRig::new_with_xdg_isolation();
    assert!(rig.config_dir().ends_with("ilium"), "{}", rig.config_dir().display());
    assert!(rig.data_dir().ends_with("ilium"), "{}", rig.data_dir().display());
    assert!(rig.cache_dir().ends_with("ilium"), "{}", rig.cache_dir().display());
}

// ---- USR-201 ----

/// REQ: USR-201
#[test]
fn usr_201_clean_profile_run_matches_every_file_against_the_table() {
    let mut rig = UserRig::new_with_xdg_isolation();
    let key = KeyPair::generate("Example Publisher");
    install_ok(&mut rig, &spec("com.example.files", "1.0.0", &["tree.read"]));
    let _ = rig.cli(&["grants", "com.example.files", "--grant", "tree.read"]);
    rig.trust(&key);
    let _ = rig.cli(&["disable", "com.example.files"]);
    let _ = rig.cli(&["enable", "com.example.files"]);
    assert!(rig.data_dir().join("plugins/installed.json").exists());
    assert!(rig.data_dir().join("plugins/store/com.example.files/1.0.0/manifest.json").exists());
    assert!(rig.state_dir().join("ilium/grants.json").exists() || rig.grants_path().exists());
    assert!(rig.trust_store_path().exists());
    let _ = rig.cli(&["uninstall", "com.example.files", "--yes"]);
    assert!(!rig.data_dir().join("plugins/store/com.example.files").exists());
    assert!(rig.trust_store_path().exists(), "trust survives an uninstall");
}

/// REQ: USR-201
#[test]
fn usr_201_parent_folders_are_created_on_first_use() {
    let mut rig = UserRig::new_with_xdg_isolation();
    assert!(!rig.data_dir().join("plugins").exists());
    install_ok(&mut rig, &spec("com.example.first", "1.0.0", &[]));
    assert!(rig.data_dir().join("plugins").is_dir());
    assert!(rig.data_dir().join("plugins/store").is_dir());
}

/// REQ: USR-201
#[test]
fn usr_201_registry_lock_and_staging_live_where_the_table_says() {
    let mut rig = UserRig::new_with_xdg_isolation();
    install_ok(&mut rig, &spec("com.example.first", "1.0.0", &[]));
    assert!(rig.data_dir().join("plugins/installed.lock").exists());
    let staging = rig.data_dir().join("plugins/staging");
    let leftovers = std::fs::read_dir(&staging).map(|entries| entries.count()).unwrap_or(0);
    assert_eq!(leftovers, 0, "staging is emptied by the rename");
}

// ---- USR-202 ----

/// REQ: USR-202
#[test]
fn usr_202_kill_during_a_grant_change_leaves_old_or_new_never_a_mix() {
    for point in [KillPoint::BeforeTempWrite, KillPoint::AfterTempWriteBeforeRename, KillPoint::AfterRename] {
        let mut rig = UserRig::new();
        install_ok(&mut rig, &spec("com.example.atomic", "1.0.0", &["tree.read", "panes.read"]));
        let _ = rig.cli(&["grants", "com.example.atomic", "--grant", "tree.read"]);
        let old = std::fs::read(rig.grants_path()).expect("grants");
        rig.kill_host_at(point);
        let _ = rig.cli(&["grants", "com.example.atomic", "--grant", "panes.read"]);
        let now = std::fs::read(rig.grants_path()).expect("grants");
        let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&now);
        assert!(parsed.is_ok(), "{point:?}: the file always parses");
        let new_expected = rig.expected_grants_after("com.example.atomic", "panes.read");
        assert!(now == old || now == new_expected, "{point:?}: neither old nor new content");
    }
}

/// REQ: USR-202
#[test]
fn usr_202_writes_go_through_a_temp_sibling_and_a_rename() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.atomic", "1.0.0", &["tree.read"]));
    rig.record_fs_operations();
    let _ = rig.cli(&["grants", "com.example.atomic", "--grant", "tree.read"]);
    let operations = rig.fs_operations();
    let rename = operations.iter().position(|op| op.kind == "rename" && op.to.ends_with("grants.json")).expect("a rename over grants.json");
    let temp_write = operations.iter().position(|op| op.kind == "write" && op.path.parent() == rig.grants_path().parent() && op.path != rig.grants_path()).expect("a temp sibling write");
    let flush = operations.iter().position(|op| op.kind == "fsync" && op.path == operations[temp_write].path).expect("a flush of the temp file");
    assert!(temp_write < flush && flush < rename);
}

/// REQ: USR-202
#[test]
fn usr_202_installed_json_and_the_trust_store_are_written_the_same_way() {
    let mut rig = UserRig::new();
    rig.record_fs_operations();
    install_ok(&mut rig, &spec("com.example.atomic", "1.0.0", &[]));
    rig.trust(&KeyPair::generate("Example Publisher"));
    let operations = rig.fs_operations();
    for target in ["installed.json", "trust.json"] {
        assert!(operations.iter().any(|op| op.kind == "rename" && op.to.ends_with(target)), "{target}");
        assert!(!operations.iter().any(|op| op.kind == "write" && op.path.ends_with(target)), "{target} is never written in place");
    }
}

// ---- USR-203 ----

/// REQ: USR-203
#[test]
fn usr_203_a_project_grant_applies_in_a_second_project_and_nothing_is_in_ilium_folders() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.reader", "1.0.0", &["fs.read:project"]));
    let project_a = rig.open_project("alpha");
    let _ = rig.cli(&["grants", "com.example.reader", "--grant", "fs.read:project"]);
    rig.start_server();
    assert_eq!(rig.call_needing("com.example.reader", "server", "fs.read:project"), CallOutcome::Ok);
    let project_b = rig.open_project("beta");
    rig.switch_project(&project_b);
    assert_eq!(rig.call_needing("com.example.reader", "server", "fs.read:project"), CallOutcome::Ok);
    for project in [project_a, project_b] {
        let names: Vec<String> = tree_snapshot(&project.join(".ilium")).into_keys().collect();
        assert!(names.iter().all(|name| !name.to_lowercase().contains("grant")), "{names:?}");
    }
}

/// REQ: USR-203
#[test]
fn usr_203_the_grant_file_is_per_user_and_holds_the_plugin_id() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.reader", "1.0.0", &["tree.read"]));
    let _ = rig.cli(&["grants", "com.example.reader", "--grant", "tree.read"]);
    let text = std::fs::read_to_string(rig.grants_path()).expect("grants");
    assert!(text.contains("com.example.reader"));
    assert!(rig.grants_path().starts_with(rig.state_dir()));
}

// ---- USR-204 ----

fn log_files(rig: &UserRig, id: &str) -> Vec<PathBuf> {
    let folder = rig.data_dir().join("plugins/logs");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    found.retain(|path| path.file_name().map(|name| name.to_string_lossy().starts_with(&format!("{id}.log"))).unwrap_or(false));
    found.sort();
    found
}

/// REQ: USR-204
#[test]
fn usr_204_five_mebibytes_of_log_leave_at_most_four_files_each_within_the_limit() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.loud", "1.0.0", &[]));
    rig.start_server();
    rig.plugin_logs_bytes("com.example.loud", 5 * 1024 * 1024);
    let files = log_files(&rig, "com.example.loud");
    assert!(files.len() <= 4, "{files:?}");
    assert!(files.len() >= 2, "rotation happened");
    for file in &files {
        assert!(std::fs::metadata(file).map(|meta| meta.len()).unwrap_or(0) <= 1024 * 1024 + 1024, "{}", file.display());
    }
}

/// REQ: USR-204
#[test]
fn usr_204_log_below_one_mebibyte_does_not_rotate_and_above_it_does() {
    let mut below = UserRig::new();
    install_ok(&mut below, &spec("com.example.loud", "1.0.0", &[]));
    below.start_server();
    below.plugin_logs_bytes("com.example.loud", 1024 * 1024 - 1024);
    assert_eq!(log_files(&below, "com.example.loud").len(), 1, "n-1");
    let mut above = UserRig::new();
    install_ok(&mut above, &spec("com.example.loud", "1.0.0", &[]));
    above.start_server();
    above.plugin_logs_bytes("com.example.loud", 1024 * 1024 + 4096);
    assert_eq!(log_files(&above, "com.example.loud").len(), 2, "n+1");
}

/// REQ: USR-204
#[test]
fn usr_204_rotated_file_older_than_14_days_is_deleted_at_server_start() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.loud", "1.0.0", &[]));
    rig.start_server();
    rig.plugin_logs_bytes("com.example.loud", 5 * 1024 * 1024);
    let rotated = rig.data_dir().join("plugins/logs/com.example.loud.log.2");
    assert!(rotated.exists());
    rig.age_file(&rotated, 15);
    rig.restart_server();
    assert!(!rotated.exists());
}

/// REQ: USR-204
#[test]
fn usr_204_rotated_file_aged_13_days_is_kept() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.loud", "1.0.0", &[]));
    rig.start_server();
    rig.plugin_logs_bytes("com.example.loud", 5 * 1024 * 1024);
    let rotated = rig.data_dir().join("plugins/logs/com.example.loud.log.2");
    rig.age_file(&rotated, 13);
    rig.restart_server();
    assert!(rotated.exists());
}

// ---- USR-205 ----

/// REQ: USR-205
#[test]
fn usr_205_every_log_line_has_four_columns_with_the_timestamp_form() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.loud", "1.0.0", &[]).activation_traps());
    rig.start_server();
    rig.plugin_logs("com.example.loud", "info", "hello world");
    rig.advance_ms(7_000);
    let pattern = regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} [a-z ]{5} com\.example\.loud .+$").expect("pattern");
    let lines = rig.log_lines("com.example.loud");
    assert!(lines.len() >= 3);
    for line in lines {
        assert!(pattern.is_match(&line), "line does not match: {line:?}");
    }
}

/// REQ: USR-205
#[test]
fn usr_205_level_is_padded_to_five_characters() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.loud", "1.0.0", &[]));
    rig.start_server();
    rig.plugin_logs("com.example.loud", "warn", "careful");
    let line = rig.log_lines("com.example.loud").pop().expect("a line");
    assert_eq!(&line[20..25], "warn ");
}

// ---- USR-206 ----

/// REQ: USR-206
#[test]
fn usr_206_folders_are_0700_and_files_are_0600() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.modes", "1.0.0", &["fs.write:data"]).start_writes_file("a.txt"));
    let _ = rig.cli(&["grants", "com.example.modes", "--grant", "fs.write:data"]);
    rig.trust(&KeyPair::generate("Example Publisher"));
    rig.start_server();
    rig.advance_ms(1_000);
    let created = rig.paths_created_by_the_host();
    assert!(!created.is_empty());
    for path in created {
        let expected = if path.is_dir() { 0o700 } else { 0o600 };
        assert_eq!(file_mode(&path), expected, "{}", path.display());
    }
}

/// REQ: USR-206
#[test]
fn usr_206_a_permissive_umask_does_not_widen_the_modes() {
    let mut rig = UserRig::new();
    rig.set_umask(0o000);
    install_ok(&mut rig, &spec("com.example.modes", "1.0.0", &[]));
    assert_eq!(file_mode(&rig.data_dir().join("plugins/installed.json")), 0o600);
    assert_eq!(file_mode(&rig.data_dir().join("plugins")), 0o700);
}

// ---- USR-207 ----

/// REQ: USR-207
#[test]
fn usr_207_world_writable_grant_file_refuses_third_party_plugins_but_not_core() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &["tree.read"]));
    let _ = rig.cli(&["grants", "com.example.t3", "--grant", "tree.read"]);
    rig.set_file_mode(&rig.grants_path(), 0o666);
    rig.start_server();
    let row = list_row(&mut rig, "com.example.t3");
    assert_eq!(row.reason.as_deref(), Some("grants.unsafe-permissions"));
    assert!(row.state.starts_with("Refused"), "{}", row.state);
    assert_eq!(list_row(&mut rig, "ilium.core.tree").state, "On");
}

/// REQ: USR-207
#[test]
fn usr_207_mode_0600_and_0644_are_accepted_and_0662_is_not() {
    for (mode, refused) in [(0o600u32, false), (0o644, false), (0o662, true)] {
        let mut rig = UserRig::new();
        install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &["tree.read"]));
        rig.set_file_mode(&rig.grants_path(), mode);
        rig.start_server();
        let is_refused = list_row(&mut rig, "com.example.t3").state.starts_with("Refused");
        assert_eq!(is_refused, refused, "mode {mode:o}");
    }
}

/// REQ: USR-207
#[test]
fn usr_207_world_writable_trust_store_is_refused_too() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    install_ok(&mut rig, &spec("com.example.signed", "1.0.0", &[]).signed_with(&key));
    rig.set_file_mode(&rig.trust_store_path(), 0o666);
    rig.start_server();
    assert!(list_row(&mut rig, "com.example.signed").state.starts_with("Refused"));
}

/// REQ: USR-207
#[test]
fn usr_207_fixing_the_mode_lets_the_plugin_run_at_the_next_start() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.t3", "1.0.0", &[]));
    rig.set_file_mode(&rig.grants_path(), 0o666);
    rig.start_server();
    rig.set_file_mode(&rig.grants_path(), 0o600);
    rig.restart_server();
    assert_eq!(list_row(&mut rig, "com.example.t3").state, "On");
}

// ---- USR-208 ----

/// REQ: USR-208
#[test]
fn usr_208_default_uninstall_keeps_settings_and_removes_the_rest() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &["tree.read", "fs.write:cache"]));
    let _ = rig.cli(&["grants", "com.example.gone", "--grant", "tree.read"]);
    rig.set_plugin_setting("com.example.gone", "density", "compact");
    rig.write_cache_file("com.example.gone", "c.bin");
    rig.extensions_remember_order("com.example.gone");
    rig.set_ci(true);
    let _ = rig.cli(&["uninstall", "com.example.gone", "--yes"]);
    assert!(!rig.data_dir().join("plugins/store/com.example.gone").exists(), "package");
    assert!(rig.grant_entries("com.example.gone").is_empty(), "grants");
    assert!(!rig.cache_dir().join("plugins/com.example.gone").exists(), "cache");
    assert!(rig.extension_order_entries("com.example.gone").is_empty(), "extensions order");
    assert_eq!(rig.plugin_setting("com.example.gone", "density").as_deref(), Some("compact"), "settings are kept");
}

/// REQ: USR-208
#[test]
fn usr_208_default_uninstall_keeps_the_log_for_seven_days() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &[]));
    rig.start_server();
    rig.plugin_logs("com.example.gone", "info", "line");
    rig.set_ci(true);
    let _ = rig.cli(&["uninstall", "com.example.gone", "--yes"]);
    assert!(rig.data_dir().join("plugins/logs/com.example.gone.log").exists());
    rig.advance_ms(7 * 86_400_000 + 1_000);
    rig.restart_server();
    assert!(!rig.data_dir().join("plugins/logs/com.example.gone.log").exists());
}

// ---- USR-209 ----

fn profile_with_everything(rig: &mut UserRig) {
    install_ok(rig, &spec("com.example.gone", "1.0.0", &["tree.read", "fs.write:data", "fs.write:cache", "storage"]).start_writes_file("d.txt"));
    let _ = rig.cli(&["grants", "com.example.gone", "--grant", "tree.read"]);
    rig.set_plugin_setting("com.example.gone", "density", "compact");
    rig.start_server();
    rig.advance_ms(1_000);
    rig.plugin_logs("com.example.gone", "info", "line");
    rig.write_storage_bucket("com.example.gone");
    rig.stop_server();
    rig.set_ci(true);
}

/// REQ: USR-209
#[test]
fn usr_209_purge_removes_settings_data_buckets_and_log() {
    let mut rig = UserRig::new();
    profile_with_everything(&mut rig);
    let output = rig.cli(&["uninstall", "com.example.gone", "--purge", "--yes"]);
    assert_eq!(result_exit_code(&output), 0);
    assert_eq!(rig.plugin_setting("com.example.gone", "density"), None);
    assert!(!rig.data_dir().join("plugins/data/com.example.gone").exists());
    assert!(!rig.data_dir().join("plugins/storage/com.example.gone").exists());
    assert!(!rig.data_dir().join("plugins/logs/com.example.gone.log").exists());
}

/// REQ: USR-209
#[test]
fn usr_209_keep_data_keeps_data_and_storage_without_asking() {
    let mut rig = UserRig::new();
    profile_with_everything(&mut rig);
    rig.attach_client(vec![]);
    let output = rig.cli(&["uninstall", "com.example.gone", "--keep-data", "--yes"]);
    assert_eq!(result_exit_code(&output), 0);
    assert!(rig.data_dir().join("plugins/data/com.example.gone").exists());
    assert!(rig.data_dir().join("plugins/storage/com.example.gone").exists());
    assert!(rig.confirmations_shown().is_empty(), "no question");
}

/// REQ: USR-209
#[test]
fn usr_209_purge_and_keep_data_exclude_each_other() {
    let mut rig = UserRig::new();
    profile_with_everything(&mut rig);
    let before = tree_snapshot(&rig.data_dir());
    let output = rig.cli(&["uninstall", "com.example.gone", "--purge", "--keep-data", "--yes"]);
    assert_eq!(result_exit_code(&output), 2, "usage error");
    assert_eq!(tree_snapshot(&rig.data_dir()), before);
}
