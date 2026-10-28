//! reference-audit-log.md and guide-enterprise-deployment.md: core dumps, crash reports, audit
//! forwarding, audit export and the remote sink failure rule (SEC-445 .. SEC-450).
//! Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-445
#[test]
fn sec_445_the_audit_log_page_states_that_core_dumps_are_disabled() {
    assert_page_has(
        "reference-audit-log.md",
        &[
            "The server and every plugin helper MUST disable core dumps for themselves",
            "`RLIMIT_CORE` 0 and the non-dumpable flag on Linux",
        ],
    );
}

/// REQ: SEC-446
#[test]
fn sec_446_the_audit_log_page_states_the_crash_report_content_and_redaction_rule() {
    assert_page_has(
        "reference-audit-log.md",
        &[
            "MUST contain only the plugin id, the error code, the host version and a stack of host frames",
            "It MUST NOT contain memory, argument values, secret values, granted data or file contents",
            "a redaction test MUST run over every report before it is stored",
        ],
    );
}

/// REQ: SEC-447
#[test]
fn sec_447_the_audit_log_page_states_the_crash_report_storage_and_expiry_rule() {
    assert_page_has(
        "reference-audit-log.md",
        &[
            "Crash reports MUST be stored with mode 0600 in a 0700 folder beside the audit directory",
            "MUST be deleted after 14 days or when the plugin is uninstalled",
            "MUST NOT be sent anywhere unless the user runs the explicit export command",
            "`crash-report` audit record",
        ],
    );
}

/// REQ: SEC-448
#[test]
fn sec_448_the_enterprise_guide_states_that_forwarded_audit_messages_carry_seq_and_chain_hash() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["every forwarded message MUST carry the record's `seq` and its chain hash"],
    );
}

/// REQ: SEC-449
#[test]
fn sec_449_the_enterprise_guide_states_the_audit_export_anchor_and_offline_verify_rule() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &[
            "`ilium plugin audit --export <file>` MUST write the records plus a final `anchor` line",
            "`ilium plugin audit --verify <file>` MUST verify an exported file offline against that anchor",
        ],
    );
}

/// REQ: SEC-450
#[test]
fn sec_450_the_enterprise_guide_states_the_remote_sink_failure_rule() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &[
            "MUST apply the disk-full rule of [SEC-225]",
            "MUST NOT drop records silently or buffer more than 10,000 records in memory",
        ],
    );
}

/// REQ: SEC-445
#[test]
fn sec_445_the_kernel_host_requirements_repeat_the_core_dump_default() {
    assert_page_has(
        "reference-requirements-kernel-host.md",
        &["A core dump MAY be enabled only by an administrator"],
    );
}

/// REQ: SEC-446
#[test]
fn sec_446_the_client_crash_record_holds_only_the_plugin_id_and_error_code() {
    assert_page_has(
        "reference-persistence-and-restart.md",
        &["The client crash record MUST contain only the plugin id, error code"],
    );
}

/// REQ: SEC-447
#[test]
fn sec_447_the_crash_report_folder_is_0700_and_each_file_0600() {
    assert_page_has(
        "reference-audit-log.md",
        &["The folder has mode 0700 and each file mode 0600 ([SEC-447])."],
    );
}

/// REQ: SEC-448
#[test]
fn sec_448_forwarded_seq_and_chain_hash_let_a_collector_detect_gaps() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["so a collector can detect a missing or reordered record without reading the local file"],
    );
}

/// REQ: SEC-449
#[test]
fn sec_449_the_exported_audit_file_is_verified_offline_against_its_anchor() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["MUST verify an exported file offline against that anchor"],
    );
}

/// REQ: SEC-450
#[test]
fn sec_450_a_failing_remote_sink_caps_the_memory_buffer_at_10000_records() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["MUST NOT drop records silently or buffer more than 10,000 records in memory"],
    );
}
