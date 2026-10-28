//! Manual page `reference-plugin-files`: the restored trust store (HST-959), the backup archive
//! contents (HST-960), cross-bucket consistency (HST-961), archive modes and location (HST-962) and
//! the data-version check on restore (HST-963).

use ilium_plugin_api::Tier;
use ilium_plugin_host::trust::restored_store_without_mac;

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-plugin-files.md";

/// REQ: HST-959
#[test]
fn hst_959_a_restored_trust_json_is_not_trusted_on_its_own() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A restored `trust.json` MUST NOT be trusted on its own.",
    );
    assert_page_states(
        &page,
        "the host MUST keep the restored file as `trust.json.tampered-<YYYYMMDD-HHmmss>`",
    );
}

/// REQ: HST-959
#[test]
fn hst_959_without_a_verifiable_mac_t2_and_t3_plugins_are_refused_as_store_tampered() {
    for tier in [Tier::T2, Tier::T3] {
        let outcome = restored_store_without_mac(tier, "20300102-030405");
        assert_eq!(outcome.refused_code, Some("trust.store-tampered"));
        assert_eq!(outcome.kept_as, "trust.json.tampered-20300102-030405");
        assert!(outcome.message.contains("ilium plugin trust add"));
    }
    assert_eq!(restored_store_without_mac(Tier::T1, "20300102-030405").refused_code, None);
}

/// REQ: HST-960
#[test]
fn hst_960_the_archive_holds_no_cache_logs_staging_or_secrets_without_flag() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "It MUST NOT hold cache, logs, staging, compile cache, the pre-migration copies under `backups/`, or secrets unless `--include-secrets` is given.",
    );
}

/// REQ: HST-960
#[test]
fn hst_960_backup_json_lists_every_entry_with_its_size_and_sha256() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`backup.json` MUST list every entry path with its size and SHA-256.",
    );
}

/// REQ: HST-961
#[test]
fn hst_961_backup_json_records_the_utc_lock_time_of_each_bucket() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`backup.json` MUST record, for each bucket entry, the UTC time at which its lock was taken",
    );
}

/// REQ: HST-961
#[test]
fn hst_961_a_multi_bucket_archive_prints_the_not_cross_bucket_consistent_warning() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST print the warning `backup.not-cross-bucket-consistent`",
    );
    assert_page_states(
        &page,
        "A plugin that keeps an invariant across buckets MUST keep that invariant inside one bucket",
    );
}

/// REQ: HST-962
#[test]
fn hst_962_the_partial_file_and_archive_are_mode_0600_and_a_new_folder_is_0700() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST create the `.partial` file and the final archive with mode `0600`",
    );
    assert_page_states(
        &page,
        "MUST create the folder beside them with mode `0700` when it creates the folder",
    );
}

/// REQ: HST-962
#[test]
fn hst_962_backup_refuses_the_live_root_and_says_the_archive_is_not_encrypted() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST refuse to write the archive inside `<data>/plugins/` with `backup.inside-live-root`",
    );
    assert_page_states(
        &page,
        "The host does not encrypt archives in this version, and the command output MUST say so and name the archive path.",
    );
}

/// REQ: HST-963
#[test]
fn hst_963_a_newer_data_stamp_is_refused_with_migration_data_newer_exit_1() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A stamp higher than that version MUST be refused with `migration.data-newer` (exit 1)",
    );
}

/// REQ: HST-963
#[test]
fn hst_963_an_uninstalled_plugin_without_a_package_is_refused_and_named() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A plugin that is not installed and has no package in the archive MUST be refused with `restore.plugin-not-installed`",
    );
    assert_page_states(&page, "the message MUST name the plugin id");
}
