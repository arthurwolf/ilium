//! Plugin data export and import (LIF-290 to LIF-293). Red-phase skeletons.

/// REQ: LIF-290
#[test]
fn lif_290_export_archives_data_buckets_and_settings_without_grants_or_logs() {
    use crate::common::{install_ok, pkg, rig, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    std::fs::create_dir_all(ops.data_dir(NOTES)).expect("data directory");
    std::fs::write(ops.data_dir(NOTES).join("note.txt"), b"export-marker-290").expect("plugin data");
    let out_path = ops.state_dir().join("archive-290.ilium-data");
    let out_text = out_path.to_string_lossy().into_owned();
    let output = ops.cli(&["plugin", "export", NOTES, "--out", &out_text]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stderr);
    let archive = std::fs::read(&out_path).expect("export file");
    let text = String::from_utf8_lossy(&archive);
    assert!(text.contains("export-marker-290"), "the data directory must be archived");
    for excluded in ["grants.json", "trusted-keys", "audit.jsonl", "package.iliumpkg"] {
        assert!(!text.contains(excluded), "the archive must not hold {excluded}");
    }
}

/// REQ: LIF-291
#[test]
fn lif_291_export_is_a_consistent_owner_only_snapshot_and_refuses_overwrite() {
    use crate::common::{error_codes, install_ok, pkg, rig, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let out_path = ops.state_dir().join("archive-291.ilium-data");
    let out_text = out_path.to_string_lossy().into_owned();
    let first = ops.cli(&["plugin", "export", NOTES, "--out", &out_text]);
    assert_eq!(first.exit_code, 0, "{:?}", first.stderr);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out_path).expect("export metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the export must be owner-only");
    }
    let before = std::fs::read(&out_path).expect("first export");
    let second = ops.cli(&["plugin", "export", NOTES, "--out", &out_text]);
    assert!(error_codes(&second).contains(&"export.exists".to_string()), "{second:?}");
    assert_eq!(std::fs::read(&out_path).expect("export after refusal"), before, "a refused export changes nothing");
    let forced = ops.cli(&["plugin", "export", NOTES, "--out", &out_text, "--force"]);
    assert_eq!(forced.exit_code, 0, "{:?}", forced.stderr);
}

/// REQ: LIF-292
#[test]
fn lif_292_import_restores_the_same_id_and_refuses_mismatched_archives() {
    use crate::common::{error_codes, install_ok, pkg, rig, snapshot_tree, NOTES, OTHER};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    install_ok(&ops, &pkg(OTHER, "1.0.0"));
    std::fs::create_dir_all(ops.data_dir(NOTES)).expect("data directory");
    std::fs::write(ops.data_dir(NOTES).join("note.txt"), b"exported-value").expect("plugin data");
    let notes_archive = ops.state_dir().join("notes-292.ilium-data");
    let notes_text = notes_archive.to_string_lossy().into_owned();
    assert_eq!(ops.cli(&["plugin", "export", NOTES, "--out", &notes_text]).exit_code, 0);
    // Changed data comes back to the exported value after import.
    std::fs::write(ops.data_dir(NOTES).join("note.txt"), b"changed-value").expect("change data");
    let imported = ops.cli(&["plugin", "import", NOTES, "--from", &notes_text, "--yes"]);
    assert_eq!(imported.exit_code, 0, "{:?}", imported.stderr);
    assert_eq!(std::fs::read(ops.data_dir(NOTES).join("note.txt")).expect("restored"), b"exported-value");
    // An archive of another id is refused and leaves the data untouched.
    let before = snapshot_tree(&ops.data_dir(OTHER));
    let mismatch = ops.cli(&["plugin", "import", OTHER, "--from", &notes_text, "--yes"]);
    assert!(error_codes(&mismatch).contains(&"import.id-mismatch".to_string()), "{mismatch:?}");
    assert_eq!(snapshot_tree(&ops.data_dir(OTHER)), before);
    // An archive of an unknown format version is refused.
    let bumped = ops.state_dir().join("bumped-292.ilium-data");
    std::fs::write(&bumped, b"ilium-data-format-999\n").expect("write bumped archive");
    let bumped_text = bumped.to_string_lossy().into_owned();
    let refused = ops.cli(&["plugin", "import", NOTES, "--from", &bumped_text, "--yes"]);
    assert!(error_codes(&refused).contains(&"import.format-unsupported".to_string()), "{refused:?}");
    assert_eq!(std::fs::read(ops.data_dir(NOTES).join("note.txt")).expect("data kept"), b"exported-value");
}

/// REQ: LIF-293
#[test]
fn lif_293_purge_on_a_tty_hints_at_export_and_stays_silent_without_one() {
    use crate::common::{install_ok, pkg, rig, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let on_tty = ops.cli_tty(&["plugin", "uninstall", NOTES, "--purge"], &[]);
    let shown = format!("{:?} {}", on_tty.stdout_jsonl, on_tty.stderr);
    assert!(shown.contains(&format!("ilium plugin export {NOTES}")), "the TTY purge names the export command: {shown}");
    assert!(!ops.data_dir(NOTES).exists(), "the hint must not block the purge");
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let quiet = ops.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    let shown = format!("{:?} {}", quiet.stdout_jsonl, quiet.stderr);
    assert!(!shown.contains("ilium plugin export"), "no prompt without a TTY: {shown}");
}

/// REQ: LIF-290
#[test]
fn lif_290_export_changes_no_plugin_data() {
    use crate::common::{install_ok, pkg, rig, snapshot_tree, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let data_before = snapshot_tree(&ops.data_dir(NOTES));
    let out_path = ops.state_dir().join("notes-export.ilium-data");
    let out_text = out_path.to_string_lossy().into_owned();
    let output = ops.cli(&["plugin", "export", NOTES, "--out", &out_text]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stderr);
    assert_eq!(snapshot_tree(&ops.data_dir(NOTES)), data_before);
}

/// REQ: LIF-291
#[test]
fn lif_291_unknown_id_export_leaves_neither_file_nor_partial_file() {
    use crate::common::{error_codes, rig};
    let ops = rig();
    let out_path = ops.state_dir().join("missing-plugin.ilium-data");
    let out_text = out_path.to_string_lossy().into_owned();
    let output = ops.cli(&["plugin", "export", "com.example.missing", "--out", &out_text]);
    assert!(error_codes(&output).contains(&"export.unknown-plugin".to_string()));
    assert!(!out_path.exists());
    let partial_path = std::path::PathBuf::from(format!("{out_text}.partial"));
    assert!(!partial_path.exists());
}

/// REQ: LIF-292
#[test]
fn lif_292_import_of_own_export_into_the_same_id_leaves_data_identical() {
    use crate::common::{install_ok, pkg, rig, snapshot_tree, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let out_path = ops.state_dir().join("notes-roundtrip.ilium-data");
    let out_text = out_path.to_string_lossy().into_owned();
    let exported = ops.cli(&["plugin", "export", NOTES, "--out", &out_text]);
    assert_eq!(exported.exit_code, 0, "{:?}", exported.stderr);
    let data_before = snapshot_tree(&ops.data_dir(NOTES));
    let imported = ops.cli(&["plugin", "import", NOTES, "--from", &out_text, "--yes"]);
    assert_eq!(imported.exit_code, 0, "{:?}", imported.stderr);
    assert_eq!(snapshot_tree(&ops.data_dir(NOTES)), data_before);
}

/// REQ: LIF-293
#[test]
fn lif_293_purge_without_a_tty_removes_the_plugin_data_and_asks_nothing() {
    use crate::common::{install_ok, pkg, rig, NOTES};
    let ops = rig();
    install_ok(&ops, &pkg(NOTES, "1.0.0"));
    let output = ops.cli(&["plugin", "uninstall", NOTES, "--purge"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stderr);
    assert!(!ops.data_dir(NOTES).exists());
}
