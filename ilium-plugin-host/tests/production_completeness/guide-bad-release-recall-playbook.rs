/// REQ: PDU-001
#[test]
fn pdu_001_yank_within_one_hour_of_confirmation() {
    // Observable: Read the incident record of a Level 1 defect: the yank timestamp is at most 60 minutes after the recorded confirmation timestamp.
    panic!(
        "PDU-001 not implemented: The publisher MUST run `ilium plugin yank <id>@<version> --reason <text>` within 1 hour of confirming a Level 1 or Level 2 defect."
    );
}

/// REQ: PDU-002
#[test]
fn pdu_002_yank_reason_prefix_security_or_defect() {
    // Observable: Run `ilium plugin yank example.sync@1.2.0 --reason "bad build"`: the exit code is non-zero and the output contains `yank.reason-prefix`; with `--reason "defect: bad build"` it exits 0.
    panic!(
        "PDU-002 not implemented: A yank reason MUST begin with `security:` for a security defect and with `defect:` otherwise, and the registry MUST refuse any other prefix with `yank.reason-prefix`."
    );
}

/// REQ: PDU-003
#[test]
fn pdu_003_hotfix_version_higher_than_yanked() {
    // Observable: Publish 1.1.9 after yanking 1.2.0: the publish exits non-zero with `package.downgrade`; publishing 1.2.1 exits 0.
    panic!(
        "PDU-003 not implemented: The hotfix MUST carry a higher version number than the yanked release, because a lower one is refused with `package.downgrade`."
    );
}

/// REQ: PDU-004
#[test]
fn pdu_004_hotfix_data_version_requires_migration() {
    // Observable: Run `ilium plugin check` on a hotfix that raises `data_version` without a migration from the previous value: the output contains `lint.hotfix-data-version` as an error.
    panic!(
        "PDU-004 not implemented: A hotfix MUST NOT raise `data_version` unless it ships a migration from the yanked version's data, because rollback is otherwise refused with `migration.data-newer`."
    );
}

/// REQ: PDU-005
#[test]
fn pdu_005_previous_good_version_kept_downloadable() {
    // Observable: After the yank, run `ilium plugin rollback example.sync` on a machine with the bad version: the exit code is 0 and `ilium plugin list` shows the previous good version.
    panic!(
        "PDU-005 not implemented: The publisher MUST keep the previous good version downloadable so that `ilium plugin rollback <id>` has a target."
    );
}

/// REQ: PDU-006
#[test]
fn pdu_006_incident_record_lists_mirrors_and_bundles() {
    // Observable: Read the incident record template section `Mirrors and bundles`: every listed entry has a `refreshed` or `outstanding` mark and a date.
    panic!(
        "PDU-006 not implemented: The incident record MUST list every offline mirror and bundle the publisher knows of, because a yank does not reach a bundle already copied, and each MUST be marked refreshed or outstanding."
    );
}

/// REQ: PDU-007
#[test]
fn pdu_007_post_incident_record_within_five_working_days() {
    // Observable: Validate a post-incident record with `ilium plugin incident check <file>`: it exits non-zero naming any missing one of `detected_at`, `yanked_at`, `hotfix_at` or `installs_affected`.
    panic!(
        "PDU-007 not implemented: The post-incident record MUST be written within 5 working days and MUST name the detection time, yank time, hotfix time and the number of installs affected."
    );
}
