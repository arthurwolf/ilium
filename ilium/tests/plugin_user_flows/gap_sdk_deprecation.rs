//! Doc checks for the SDK deprecation manual page: one test per requirement ID that
//! sets the successor rule, the removal rule and the host's silence for SDK deprecations.
//! Each test reads the manual page that states the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: API-085
#[test]
fn api_085_successor_shipped_in_marking_release() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "MUST already ship its successor",
        "so that an author can move",
        "say so in its note instead",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-086
#[test]
fn api_086_removal_only_in_named_minor() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "MUST be the minor named in its mark",
        "MUST NOT be a patch release",
        "MUST NOT change the signature or behaviour",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-087
#[test]
fn api_087_host_writes_no_sdk_deprecation_record() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "MUST NOT write a `deprecated` record",
        "a log line or a refusal",
        "for an SDK deprecation",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
