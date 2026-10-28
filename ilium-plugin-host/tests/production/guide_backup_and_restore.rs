//! Manual page `guide-backup-and-restore`: the backup cadence and job failure (HST-970), archive
//! retention and access (HST-971, HST-972), the restore drill (HST-973, HST-974), the recovery
//! targets (HST-975, HST-976) and the deletion and export rules (HST-977, HST-978).

use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "guide-backup-and-restore.md";

/// REQ: HST-970
#[test]
fn hst_970_the_administrator_backs_up_at_least_every_24_hours_from_a_scheduled_job() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST run `ilium plugin backup` at least once every 24 hours (proposed, owner to confirm) from a scheduled job",
    );
}

/// REQ: HST-970
#[test]
fn hst_970_the_scheduled_backup_job_fails_loudly_on_a_non_zero_exit() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "the job MUST fail loudly when the command exits non-zero.",
    );
}

/// REQ: HST-971
#[test]
fn hst_971_the_administrator_keeps_the_seven_newest_archives_on_another_disk() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The administrator MUST keep at least the 7 newest archives (proposed, owner to confirm)",
    );
    assert_page_states(
        &page,
        "MUST keep them on a different disk from the live `<data>/plugins` folder",
    );
}

/// REQ: HST-971
#[test]
fn hst_971_backup_keeps_an_older_archive_at_the_same_path_unless_forced() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST NOT delete any archive, including an older one at the same path, unless `--force` is given.",
    );
}

/// REQ: HST-972
#[test]
fn hst_972_the_archive_is_stored_with_the_access_limit_of_the_plugin_data_folder() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The administrator MUST store it with the same access limit as the plugin data folder.",
    );
}

/// REQ: HST-972
#[test]
fn hst_972_the_archive_holds_credentials_only_with_include_secrets() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "it holds plugin data and, with `--include-secrets`, credentials",
    );
}

/// REQ: HST-973
#[test]
fn hst_973_the_restore_drill_runs_monthly_and_after_every_host_version_change() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST run the restore drill at least once a month (proposed, owner to confirm) and after every change of host version.",
    );
}

/// REQ: HST-973
#[test]
fn hst_973_a_dry_run_with_a_hash_mismatch_counts_as_a_failed_drill() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A dry run that exits non-zero, or lists a hash mismatch, MUST count as a failed drill.",
    );
}

/// REQ: HST-974
#[test]
fn hst_974_the_drill_restores_into_a_scratch_data_root() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "A drill MUST restore into a scratch data root.");
}

/// REQ: HST-974
#[test]
fn hst_974_a_restore_into_a_live_root_while_a_server_runs_is_refused() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST be refused with `restore.server-running` (HST-653)",
    );
}

/// REQ: HST-975
#[test]
fn hst_975_the_recovery_point_objective_is_no_more_than_24_hours() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "the recovery point objective MUST be no more than 24 hours (proposed, owner to confirm)",
    );
}

/// REQ: HST-975
#[test]
fn hst_975_writes_after_the_newest_archive_are_lost_on_restore() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Data written after the newest archive is lost on restore.",
    );
}

/// REQ: HST-976
#[test]
fn hst_976_a_one_gib_plugin_restore_completes_within_15_minutes() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "For a single plugin's data in an archive up to 1 GiB (proposed, owner to confirm)",
    );
    assert_page_states(
        &page,
        "the restore MUST complete within 15 minutes (proposed, owner to confirm) on the reference machine",
    );
}

/// REQ: HST-976
#[test]
fn hst_976_the_fifteen_minute_restore_is_timed_on_the_reference_machine() {
    const PLUGIN: &str = "com.example.notes";
    let ops = OpsRig::new();
    let package = ops.write_package(&PackageSpec::new(PLUGIN, "1.0.0"));
    let installed = ops.cli(&["install", &package.to_string_lossy(), "--yes", "--allow-unsigned"]);
    assert_eq!(installed.exit_code, 0, "{:?}", installed.stderr);
    std::fs::create_dir_all(ops.data_dir(PLUGIN)).expect("data folder");
    // 16 MiB stands in for the 1 GiB bound; the budget is scaled down in proportion.
    std::fs::write(ops.data_dir(PLUGIN).join("blob.bin"), vec![7u8; 16 * 1024 * 1024]).expect("data");
    let archive = ops.state_dir().join("timing.ilium-data");
    let archive_text = archive.to_string_lossy().into_owned();
    assert_eq!(ops.cli(&["plugin", "export", PLUGIN, "--out", &archive_text]).exit_code, 0);
    let started = std::time::Instant::now();
    let restored = ops.cli(&["plugin", "import", PLUGIN, "--from", &archive_text, "--yes"]);
    assert_eq!(restored.exit_code, 0, "{:?}", restored.stderr);
    let budget = std::time::Duration::from_secs(15 * 60) / 64;
    assert!(started.elapsed() < budget, "restore took {:?}", started.elapsed());
}

/// REQ: HST-977
#[test]
fn hst_977_purging_plugin_data_keeps_a_whole_host_archive_or_export() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST NOT delete a whole-host archive or a single-plugin export that the administrator wrote elsewhere",
    );
}

/// REQ: HST-977
#[test]
fn hst_977_an_export_still_exists_after_the_plugin_is_purged() {
    const PLUGIN: &str = "com.example.notes";
    let ops = OpsRig::new();
    let package = ops.write_package(&PackageSpec::new(PLUGIN, "1.0.0"));
    let installed = ops.cli(&["install", &package.to_string_lossy(), "--yes", "--allow-unsigned"]);
    assert_eq!(installed.exit_code, 0, "{:?}", installed.stderr);
    std::fs::create_dir_all(ops.data_dir(PLUGIN)).expect("data folder");
    std::fs::write(ops.data_dir(PLUGIN).join("note.txt"), b"kept").expect("data");
    let export = std::env::temp_dir().join(format!("hst977-{}.ilium-data", std::process::id()));
    let export_text = export.to_string_lossy().into_owned();
    assert_eq!(ops.cli(&["plugin", "export", PLUGIN, "--out", &export_text]).exit_code, 0);
    let before = std::fs::read(&export).expect("export");
    let purged = ops.cli(&["uninstall", PLUGIN, "--purge", "--yes"]);
    assert_eq!(purged.exit_code, 0, "{:?}", purged.stderr);
    assert!(!ops.data_dir(PLUGIN).exists(), "the purge removes the plugin data");
    assert_eq!(std::fs::read(&export).expect("export survives"), before);
    std::fs::remove_file(&export).expect("remove own export");
}

/// REQ: HST-978
#[test]
fn hst_978_the_administrator_takes_an_export_or_archive_before_a_privacy_purge() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Before the administrator purges a plugin for a privacy request, the administrator MUST take an export or an archive of that plugin",
    );
}

/// REQ: HST-978
#[test]
fn hst_978_the_runbook_checks_the_export_exists_before_the_purge() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "check the export exists before the purge.");
}
