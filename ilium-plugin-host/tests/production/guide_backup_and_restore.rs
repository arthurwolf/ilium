//! Manual page `guide-backup-and-restore`: the backup cadence and job failure (HST-970), archive
//! retention and access (HST-971, HST-972), the restore drill (HST-973, HST-974), the recovery
//! targets (HST-975, HST-976) and the deletion and export rules (HST-977, HST-978).

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
    unimplemented!("ilium_plugin_host::cli::restore_timing_on_reference_machine")
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
    unimplemented!("ilium_plugin_host::cli::uninstall_purge_keeps_exports")
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
