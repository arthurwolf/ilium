//! Manual page `reference-install-update-uninstall`: the migration backup size limit and the audited
//! `--no-backup` opt-out (HST-677).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-677
#[test]
fn hst_677_oversized_backup_stops_update_before_migration_unless_no_backup_is_audited() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_config_value("migration_backup_mib", "1");
    rig.fill_plugin_data_mib(NOTES, 4);
    let update = rig.cli(&["plugin", "update", NOTES]);
    assert_eq!(update.exit_code, 1, "update stops before migration");
    assert!(
        update.stderr.contains("migration.backup-too-large"),
        "code migration.backup-too-large"
    );
    assert!(
        update.stderr.contains("migration_backup_mib"),
        "names the key and the size"
    );
    assert_eq!(rig.migrate_calls(NOTES).len(), 0, "migrate did not run");
    let forced = rig.cli(&["plugin", "update", NOTES, "--no-backup"]);
    assert_eq!(forced.exit_code, 0, "--no-backup proceeds");
    assert!(
        rig.audit_entries_with_flag("no-backup").len() == 1,
        "--no-backup recorded in the audit log"
    );
    assert!(
        rig.backup_verified_by_byte_and_file_count(NOTES),
        "backup verified before migrate runs"
    );
}

/// REQ: HST-677
#[test]
fn hst_677_a_backup_within_the_limit_updates_without_the_no_backup_flag_or_audit_entry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_config_value("migration_backup_mib", "64");
    rig.fill_plugin_data_mib(NOTES, 1);
    let update = rig.cli(&["plugin", "update", NOTES]);
    assert_eq!(
        update.exit_code, 0,
        "data under migration_backup_mib updates without --no-backup"
    );
    assert_eq!(
        rig.audit_entries_with_flag("no-backup").len(),
        0,
        "no no-backup audit entry when the backup was taken"
    );
}
