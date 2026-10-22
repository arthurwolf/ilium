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
