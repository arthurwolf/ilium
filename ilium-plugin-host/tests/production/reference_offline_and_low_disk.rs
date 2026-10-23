//! Manual page `reference-offline-and-low-disk`: the backup and restore disk-space checks and the
//! disk-full behaviour of both commands (HST-132..HST-135).

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-offline-and-low-disk.md";

/// REQ: HST-132
#[test]
fn hst_132_backup_refuses_with_backup_no_space_before_any_write() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`ilium plugin backup <file>` MUST refuse before any write with `backup.no-space` (exit 1)",
    );
    assert_page_states(&page, "plus 512 MiB (proposed, owner to confirm)");
}

/// REQ: HST-132
#[test]
fn hst_132_backup_refusal_one_byte_under_the_threshold_leaves_no_archive() {
    unimplemented!("ilium_plugin_host::cli::backup_space_precheck")
}

/// REQ: HST-133
#[test]
fn hst_133_backup_disk_full_returns_io_no_space_and_deletes_the_partial_file() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "A disk-full error during `ilium plugin backup` MUST return `io` with the message `no-space` (LIF-119), MUST delete `<file>.partial`",
    );
}

/// REQ: HST-133
#[test]
fn hst_133_backup_disk_full_never_changes_an_existing_archive() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST NOT change or delete an archive that already exists at `<file>` (HST-971 applies), and MUST exit 1",
    );
}

/// REQ: HST-134
#[test]
fn hst_134_restore_refuses_with_restore_no_space_before_any_write() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`ilium plugin restore <file>` MUST refuse before any write with `restore.no-space` (exit 1)",
    );
    assert_page_states(
        &page,
        "less than twice the uncompressed size of the entries it would write, plus 100 MiB (proposed, owner to confirm)",
    );
}

/// REQ: HST-134
#[test]
fn hst_134_restore_dry_run_does_not_run_the_space_check() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "`--dry-run` MUST NOT run this check, since it writes nothing.",
    );
}

/// REQ: HST-135
#[test]
fn hst_135_restore_disk_full_leaves_each_plugin_at_old_or_fully_restored_data() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST leave each plugin either at its pre-restore data or fully restored (HST-653)",
    );
    assert_page_states(
        &page,
        "MUST remove the temporary files the restore created under `staging/`",
    );
}

/// REQ: HST-135
#[test]
fn hst_135_restore_disk_full_lists_each_plugin_id_that_was_not_restored() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "MUST list each plugin id that was not restored.");
}
