//! Release-rollout gap doc checks: one test per requirement ID for halting a rollout and
//! for file installs outside a rollout cohort. Each test reads the manual page that states
//! the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: PKG-694
#[test]
fn pkg_694_halt_changes_only_signed_metadata_and_keeps_blob() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "MUST change only the signed metadata",
        "The package blob of the version MUST stay in place",
        "a halt MUST NOT delete it",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-695
#[test]
fn pkg_695_rollout_never_limits_file_install() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "A rollout MUST NOT limit a file install",
        "because the user chooses the file directly",
        "A client outside the cohort MUST NOT show an error for the staged version",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
