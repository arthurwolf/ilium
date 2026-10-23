//! Security response page (SEC-700, SEC-702, SEC-703, SEC-729).

use crate::support::*;

/// REQ: SEC-700
#[test]
fn sec_700_advisory_is_exported_as_osv_json_with_every_required_key() {
    let rig = rig();
    let output = rig.cli(&[
        "security",
        "advisory",
        "export",
        "--format",
        "osv",
        "--id",
        "ILIUM-2026-0001",
    ]);
    assert_eq!(
        output.exit_code, 0,
        "advisory export failed: {:?}",
        output.stdout_jsonl
    );
    let advisory = output
        .stdout_jsonl
        .first()
        .expect("the export prints one OSV document");
    for key in [
        "id",
        "aliases",
        "affected",
        "severity",
        "fixed",
        "published",
        "credits",
    ] {
        assert!(!advisory[key].is_null(), "OSV advisory lacks `{key}`");
    }
    let vector = advisory["severity"][0]["score"]
        .as_str()
        .expect("severity carries a CVSS v4 vector string");
    assert!(
        vector.starts_with("CVSS:4.0/"),
        "severity must be a CVSS v4 vector, got {vector}"
    );
}

/// REQ: SEC-702
#[test]
fn sec_702_high_severity_advisory_without_a_cve_id_is_refused_at_publish() {
    let rig = rig();
    let output = rig.cli(&[
        "security",
        "advisory",
        "publish",
        "--id",
        "ILIUM-2026-0002",
        "--severity",
        "high",
    ]);
    assert_ne!(
        output.exit_code, 0,
        "publishing a High advisory without a CVE id must fail"
    );
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "advisory.cve-missing"),
        "expected advisory.cve-missing, got {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-703
#[test]
fn sec_703_unacknowledged_third_party_advisory_quarantines_after_14_days_and_publishes_after_90() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&[
        "security",
        "report",
        NOTES,
        "--version",
        "1.0.0",
        "--private",
    ]);
    rig.advance(secs(14 * 86_400));
    let entry = registry_entry(&rig, NOTES);
    assert_eq!(
        entry["quarantine"], "emergency-feed",
        "no acknowledgement in 14 days must quarantine the version via EXT-057"
    );
    rig.advance(secs(76 * 86_400));
    let advisory = rig.cli(&["security", "advisory", "list", "--plugin", NOTES]);
    assert!(
        advisory
            .stdout_jsonl
            .iter()
            .any(|line| line["state"] == "published"),
        "the advisory must be published 90 days after first contact"
    );
}

/// REQ: SEC-729
#[test]
fn sec_729_response_page_commits_to_emergency_disable_status_page_and_postmortem() {
    let page = manual_page("reference-security-response");
    for must in ["4 hours", "status page", "post-mortem", "30 days"] {
        assert!(
            page.contains(must),
            "the security response page must commit to `{must}`"
        );
    }
    let rig = rig();
    let output = rig.cli(&[
        "security",
        "emergency-disable",
        "--advisory",
        "ILIUM-2026-0001",
        "--dry-run",
    ]);
    assert_eq!(
        output.exit_code, 0,
        "emergency-disable must have a dry-run route: {:?}",
        output.stdout_jsonl
    );
}
