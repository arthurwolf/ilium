//! Data report, export, erase, identifiers and retention table of the page `user-privacy`
//! (USR-322 .. USR-331, USR-348).

use std::fs;
use std::time::{Duration, Instant};

use ilium_plugin_host::retention::{CRASH_RECORD_RETENTION_DAYS, LOG_RETENTION_DAYS};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::common::*;

/// REQ: USR-322
#[test]
fn usr_322_data_report_lists_every_kind_for_50_plugins_within_2_seconds_without_starting_any() {
    let rig = rig();
    for index in 0..50 {
        let id = format!("com.example.p{index}");
        install_ok(&rig, &pkg(&id, "1.0.0").grant("fs.read"));
        rig.plant_settings(&id, "theme", "dark");
        rig.plant_data(&id, 1);
        rig.plant_cache(&id);
        rig.write_storage(&id, "bucket", "key", b"value");
    }
    let started = Instant::now();
    let output = rig.cli(&["data-report"]);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "data-report took too long"
    );
    assert_eq!(output.exit_code, 0, "data-report failed: {}", output.stderr);
    for kind in [
        "settings",
        "data folder",
        "storage buckets",
        "cache",
        "logs",
        "grants",
        "consent records",
    ] {
        assert!(output.stdout_raw.contains(kind), "kind {kind} not listed");
    }
    assert!(
        output.stdout_raw.contains("newest"),
        "newest modification time missing"
    );
    assert!(
        !rig.trace()
            .iter()
            .any(|event| event.starts_with("activate")),
        "no plugin may be started by the report"
    );
}

/// REQ: USR-323
#[test]
fn usr_323_export_writes_manifest_with_sha256_per_file_and_one_folder_per_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_settings(NOTES, "theme", "dark");
    let target = rig.scratch_dir().join("export-empty");
    let output = rig.cli(&["export-data", &target.to_string_lossy()]);
    assert_eq!(output.exit_code, 0, "export failed: {}", output.stderr);

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(target.join("manifest.json")).expect("manifest"))
            .expect("manifest is JSON");
    assert_eq!(manifest["format_version"], 1);
    assert!(
        target.join(NOTES).is_dir(),
        "one subfolder per plugin expected"
    );
    for entry in manifest["files"].as_array().expect("files list") {
        let path = entry["path"].as_str().expect("path");
        let bytes = fs::read(target.join(path)).expect("exported file");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            entry["sha256"].as_str(),
            Some(digest.as_str()),
            "hash of {path}"
        );
    }
}

/// REQ: USR-323
#[test]
fn usr_323_export_into_a_non_empty_folder_is_refused() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let target = rig.scratch_dir().join("export-busy");
    fs::create_dir_all(&target).expect("target dir");
    fs::write(target.join("keep.txt"), b"x").expect("occupant");
    let output = rig.cli(&["export-data", &target.to_string_lossy()]);
    assert_ne!(output.exit_code, 0);
    assert!(
        output.stderr.contains("export.target-not-empty"),
        "{}",
        output.stderr
    );
}

/// REQ: USR-324
#[test]
fn usr_324_failed_export_removes_its_partial_folder_and_leaves_sources_unchanged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let before = rig.file_digests(&rig.data_dir(NOTES));
    rig.fail_export_after_files(1);
    let target = rig.scratch_dir().join("export-fails");
    let output = rig.cli(&["export-data", &target.to_string_lossy()]);
    assert_ne!(
        output.exit_code, 0,
        "the injected failure must fail the export"
    );
    assert!(!target.exists(), "the partial folder must be removed");
    assert_eq!(
        rig.file_digests(&rig.data_dir(NOTES)),
        before,
        "source files changed"
    );
}

/// REQ: USR-325
#[test]
fn usr_325_erase_that_cannot_delete_a_file_exits_3_and_lists_the_path() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let stuck = rig.data_dir(NOTES).join("locked.bin");
    rig.make_undeletable(&stuck);
    let output = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(
        output.exit_code, 3,
        "erase.incomplete must exit 3: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("erase.incomplete"),
        "{}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains(&stuck.to_string_lossy().into_owned()),
        "path not listed"
    );
}

/// REQ: USR-325
#[test]
fn usr_325_successful_erase_prints_remaining_zero_per_kind_and_exits_0() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let output = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    assert!(
        output
            .stdout_raw
            .lines()
            .any(|line| line.starts_with("erased ") && line.ends_with("remaining 0")),
        "expected an 'erased <kind>: ... remaining 0' line: {}",
        output.stdout_raw
    );
}

/// REQ: USR-326
#[test]
fn usr_326_erasing_a_plugin_removes_all_its_entries_or_names_each_kept_location() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    rig.plant_settings(NOTES, "theme", "dark");
    rig.plant_cache(NOTES);
    rig.plant_backup(NOTES, "pre-migration");
    rig.plant_crash_record(NOTES, 1);
    rig.make_staging("support", 4242, secs(60));
    let output = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    for location in rig.remaining_entries_for(NOTES) {
        assert!(
            output.stdout_raw.contains(&location),
            "a remaining entry is neither erased nor named: {location}"
        );
    }
    assert!(
        rig.remaining_entries_for(NOTES).len() < 5,
        "entries of the plugin were left behind"
    );
}

/// REQ: USR-327
#[test]
fn usr_327_erase_of_one_plugin_defaults_to_no_and_names_the_plugin_and_size_in_the_prompt() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let _declined = rig.cli_tty(&["erase-data", NOTES], &[""]);
    assert!(
        rig.data_dir(NOTES).exists(),
        "pressing Enter must delete nothing"
    );
    assert!(
        rig.last_prompt().contains(NOTES),
        "the prompt must name the plugin"
    );
    assert!(
        rig.last_prompt().contains("bytes"),
        "the prompt must show the total size"
    );
    let _confirmed = rig.cli_tty(&["erase-data", NOTES], &["erase"]);
    assert!(
        !rig.data_dir(NOTES).exists(),
        "typing erase must delete the data"
    );
}

/// REQ: USR-327
#[test]
fn usr_327_erase_of_all_plugins_requires_typing_the_word_erase() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let _refused = rig.cli_tty(&["erase-data"], &["no"]);
    assert!(
        rig.data_dir(NOTES).exists(),
        "any answer other than erase must delete nothing"
    );
}

/// REQ: USR-328
#[test]
fn usr_328_export_and_erase_work_for_failed_blocked_incompatible_dormant_and_uninstalled_plugins() {
    for state in [
        "failed",
        "blocked",
        "incompatible",
        "dormant",
        "uninstalled-kept-data",
    ] {
        let rig = rig();
        install_ok(&rig, &pkg(NOTES, "1.0.0"));
        rig.plant_data(NOTES, 1);
        rig.put_plugin_in_state(NOTES, state);
        let target = rig.scratch_dir().join(format!("export-{state}"));
        let exported = rig.cli(&["export-data", &target.to_string_lossy()]);
        assert_eq!(
            exported.exit_code, 0,
            "export failed for {state}: {}",
            exported.stderr
        );
        let erased = rig.cli(&["erase-data", NOTES, "--yes"]);
        assert_eq!(
            erased.exit_code, 0,
            "erase failed for {state}: {}",
            erased.stderr
        );
        assert_eq!(
            rig.plugin_code_runs(NOTES),
            0,
            "plugin code ran for state {state}"
        );
    }
}

/// REQ: USR-329
#[test]
fn usr_329_missing_privacy_table_is_warned_and_a_declared_table_is_shown_beside_grants() {
    let rig = rig();
    let without = pkg(NOTES, "1.0.0").grant("http:example.com");
    install_ok(&rig, &without);
    assert!(
        rig.last_review()
            .join("\n")
            .contains("This plugin does not describe what it collects"),
        "the warning for a missing [privacy] table is missing"
    );

    let rig_declared = crate::common::rig();
    let with = pkg(NOTES, "1.0.0").grant("http:example.com").privacy_table(
        &["page-titles"],
        30,
        "privacy@example.com",
    );
    install_ok(&rig_declared, &with);
    let review = rig_declared.last_review().join("\n");
    assert!(
        review.contains("page-titles"),
        "collects value missing: {review}"
    );
    assert!(review.contains("30"), "retention value missing: {review}");
    assert!(
        review.contains("privacy@example.com"),
        "contact value missing: {review}"
    );
}

/// REQ: USR-330
#[test]
fn usr_330_plugin_identifiers_are_random_per_plugin_per_install_and_change_on_purge() {
    let first_install = rig();
    install_ok(&first_install, &pkg(NOTES, "1.0.0"));
    install_ok(&first_install, &pkg(OTHER, "1.0.0"));
    let second_install = rig();
    install_ok(&second_install, &pkg(NOTES, "1.0.0"));

    let notes_id = first_install.plugin_instance_id(NOTES);
    let other_id = first_install.plugin_instance_id(OTHER);
    let other_install_id = second_install.plugin_instance_id(NOTES);
    assert_ne!(notes_id, other_id, "two plugins share an identifier");
    assert_ne!(
        notes_id, other_install_id,
        "two installs share an identifier"
    );

    let purged = first_install.cli(&["uninstall", NOTES, "--purge", "--yes"]);
    assert_eq!(purged.exit_code, 0, "{}", purged.stderr);
    install_ok(&first_install, &pkg(NOTES, "1.0.0"));
    assert_ne!(
        first_install.plugin_instance_id(NOTES),
        notes_id,
        "purge must change the identifier"
    );
}

/// REQ: USR-331
#[test]
fn usr_331_a_plugin_reading_another_plugins_settings_gets_not_permitted_and_one_audit_record() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.plant_settings(OTHER, "secret", "value");
    let server = rig.start_server("s1");
    let result = server.plugin_host_call(NOTES, "settings.read", &json!({ "plugin_id": OTHER }));
    assert_eq!(
        result.expect_err("cross-plugin read must be refused"),
        "not-permitted"
    );
    let denials = rig
        .audit_records("not-permitted")
        .iter()
        .filter(|record| record["event"] == "not-permitted")
        .count();
    assert_eq!(denials, 1, "exactly one audit record expected");
}

/// REQ: USR-348
#[test]
fn usr_348_privacy_table_rows_match_the_retention_constants_in_the_code() {
    let page = manual_page("user-privacy");
    let table_rows: Vec<&str> = page.lines().filter(|line| line.starts_with('|')).collect();
    let row_with = |needle: &str| -> &str {
        table_rows
            .iter()
            .copied()
            .find(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no privacy table row mentions {needle}"))
    };
    let log_row = row_with("log");
    assert!(
        log_row.contains(&format!("{LOG_RETENTION_DAYS} days")),
        "log row: {log_row}"
    );
    let crash_row = row_with("crash");
    assert!(
        crash_row.contains(&format!("{CRASH_RECORD_RETENTION_DAYS} days")),
        "crash row: {crash_row}"
    );
    let audit_row = row_with("audit");
    assert!(
        audit_row.contains("audit.keep_days"),
        "audit row: {audit_row}"
    );
    let backup_row = row_with("backup");
    assert!(backup_row.contains("newest"), "backup row: {backup_row}");
}

/// REQ: USR-348
#[test]
fn usr_348_retention_table_has_three_cells_and_a_deleting_command_in_every_row() {
    let page = manual_page("user-privacy");
    let header_index = page
        .lines()
        .position(|line| line == "| What | Default retention | Deleted by |")
        .expect("the retention table header must exist on the privacy page");
    let rows: Vec<&str> = page
        .lines()
        .skip(header_index + 2)
        .take_while(|line| line.starts_with('|'))
        .collect();
    assert!(
        rows.len() >= 10,
        "the retention table lost rows: {} found",
        rows.len()
    );
    for row in rows {
        // A row `| a | b | c |` splits into five parts: two empty edges and three cells.
        let parts: Vec<&str> = row.split('|').map(str::trim).collect();
        assert_eq!(parts.len(), 5, "a retention row must have three cells: {row}");
        assert!(
            !parts[3].is_empty(),
            "a retention row leaves its deleting column empty: {row}"
        );
    }
}

/// REQ: USR-322
#[test]
fn usr_322_data_report_changes_no_file_in_the_plugin_data_folder() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    rig.plant_data(NOTES, 1);
    let before = rig.file_digests(&rig.data_dir(NOTES));
    let output = rig.cli(&["data-report", NOTES]);
    assert_eq!(output.exit_code, 0, "data-report failed: {}", output.stderr);
    assert_eq!(
        rig.file_digests(&rig.data_dir(NOTES)),
        before,
        "the report must only read the plugin data"
    );
}

/// REQ: USR-324
#[test]
fn usr_324_successful_export_leaves_the_source_files_unchanged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let before = rig.file_digests(&rig.data_dir(NOTES));
    let target = rig.scratch_dir().join("export-ok-324");
    let output = rig.cli(&["export-data", &target.to_string_lossy()]);
    assert_eq!(output.exit_code, 0, "export failed: {}", output.stderr);
    assert_eq!(
        rig.file_digests(&rig.data_dir(NOTES)),
        before,
        "an export must not change the source files"
    );
}

/// REQ: USR-326
#[test]
fn usr_326_erasing_one_plugin_keeps_the_data_of_another_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.plant_data(NOTES, 1);
    rig.plant_data(OTHER, 1);
    let other_before = rig.file_digests(&rig.data_dir(OTHER));
    let output = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    assert_eq!(
        rig.file_digests(&rig.data_dir(OTHER)),
        other_before,
        "erasing one plugin must not touch another plugin's data"
    );
}

/// REQ: USR-328
#[test]
fn usr_328_erase_after_a_plain_uninstall_still_removes_the_data() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_data(NOTES, 1);
    let uninstalled = rig.cli(&["uninstall", NOTES, "--yes"]);
    assert_eq!(uninstalled.exit_code, 0, "{}", uninstalled.stderr);
    let erased = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(erased.exit_code, 0, "erase after uninstall: {}", erased.stderr);
    assert!(
        !rig.data_dir(NOTES).exists(),
        "erase must remove the data after a plain uninstall"
    );
}

/// REQ: USR-329
#[test]
fn usr_329_declared_privacy_table_suppresses_the_missing_table_warning() {
    let rig = crate::common::rig();
    let with = pkg(NOTES, "1.0.0").grant("http:example.com").privacy_table(
        &["page-titles"],
        30,
        "privacy@example.com",
    );
    install_ok(&rig, &with);
    let review = rig.last_review().join("\n");
    assert!(
        !review.contains("This plugin does not describe what it collects"),
        "a declared [privacy] table must not raise the missing-table warning: {review}"
    );
}

/// REQ: USR-330
#[test]
fn usr_330_erasing_data_also_changes_the_plugin_identifier() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let before = rig.plugin_instance_id(NOTES);
    let erased = rig.cli(&["erase-data", NOTES, "--yes"]);
    assert_eq!(erased.exit_code, 0, "{}", erased.stderr);
    assert_ne!(
        rig.plugin_instance_id(NOTES),
        before,
        "erase must change the identifier"
    );
}

/// REQ: USR-331
#[test]
fn usr_331_a_plugin_reading_its_own_settings_is_allowed_and_not_audited_as_denied() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_settings(NOTES, "theme", "dark");
    let server = rig.start_server("s1");
    let result = server.plugin_host_call(NOTES, "settings.read", &json!({ "plugin_id": NOTES }));
    assert!(result.is_ok(), "reading its own settings must succeed");
    let denials = rig
        .audit_records("not-permitted")
        .iter()
        .filter(|record| record["event"] == "not-permitted")
        .count();
    assert_eq!(denials, 0, "an own-settings read must not be audited as denied");
}
