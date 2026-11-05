/// REQ: PDU-008
#[test]
fn pdu_008_contain_first_by_deny_or_blocklist_before_investigation() {
    // Observable: Read the incident record: the `Actions taken` line for the deny or blocklist entry carries a time earlier than the first investigation entry.
    panic!(
        "PDU-008 not implemented: The operator MUST contain first, by adding the plugin id to `plugins.deny_ids` or the version to the file named by `packages.blocklist_file`, before any investigation."
    );
}

/// REQ: PDU-009
#[test]
fn pdu_009_containment_recorded_as_actions_taken_line_with_time_and_admin() {
    // Observable: Validate the incident record with `ilium plugin incident check <file>`: it fails with `incident.action-unrecorded` when the blocklist change has no `Actions taken` line with a time and an administrator.
    panic!(
        "PDU-009 not implemented: Containment MUST be recorded as an `Actions taken` line with time and administrator, as the incident runbook requires."
    );
}

/// REQ: PDU-010
#[test]
fn pdu_010_identify_affected_machines_from_central_audit_records() {
    // Observable: Query the collector for `install` and `update` records naming the bad version: the result lists every machine id that installed it, and the incident record's machine list equals that result.
    panic!(
        "PDU-010 not implemented: The operator MUST identify affected machines from the `install`, `update` and `update.rolled-back` audit records collected centrally, not by asking users."
    );
}

/// REQ: PDU-011
#[test]
fn pdu_011_recovery_by_lockfile_uses_plugin_sync_locked() {
    // Observable: Pin the good version in the fleet lockfile and run `ilium plugin sync --locked` on a machine with the bad version: the exit code is 0 and `ilium plugin list` shows the good version.
    panic!(
        "PDU-011 not implemented: Recovery by lockfile MUST use `ilium plugin sync --locked`, which downgrades to the locked version without `--allow-downgrade` ([USR-048])."
    );
}

/// REQ: PDU-012
#[test]
fn pdu_012_newer_data_recovers_by_rollback_restore_backup_after_dry_run() {
    // Observable: On a machine whose data_version was raised by the bad release, run the dry run and then the rollback with `--restore-backup`: the dry run exits 0 listing the archive members and the rollback exits 0 with the old version running.
    panic!(
        "PDU-012 not implemented: A machine whose plugin data is newer than the old version MUST recover with `ilium plugin rollback <id> --restore-backup`, and the operator MUST first verify the archive with `ilium plugin restore <file> --dry-run`."
    );
}

/// REQ: PDU-013
#[test]
fn pdu_013_incident_not_closed_until_every_machine_recovered_or_denied() {
    // Observable: Run `ilium plugin incident check <file>` with one in-scope machine still reporting the bad version: it exits non-zero with `incident.machine-outstanding` and the machine id.
    panic!(
        "PDU-013 not implemented: The incident MUST NOT be closed until every machine in scope reports the fixed version in `ilium plugin list` or reports the plugin as denied."
    );
}

/// REQ: PDU-014
#[test]
fn pdu_014_level1_ack_in_time_and_bundle_only_after_ops_006_check() {
    // Observable: Attach a support bundle without the OPS-006 check recorded: `ilium plugin incident check <file>` exits non-zero with `incident.bundle-unchecked`.
    panic!(
        "PDU-014 not implemented: Level 1 incidents MUST be acknowledged within the severity table's time, and the operator MUST attach a support bundle built with `ilium plugin support-bundle <id>` only after the [OPS-006] attach check passes."
    );
}
