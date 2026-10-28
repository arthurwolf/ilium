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

/// REQ: PKG-694
#[test]
fn pkg_694_halt_keeps_an_already_activated_version_running() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "A client that already runs `1.3.0` keeps running it.",
        "No client moves a host to the previous version because of a halt.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-695
#[test]
fn pkg_695_outside_cohort_client_keeps_its_current_version_silently() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "It keeps working with the version it has, and the staged version is simply not chosen for it",
        "No error, no notice. The host chooses the version it would have chosen without the staged version.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
