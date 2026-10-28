//! Host aggregate limits doc checks: one test per requirement ID that states the rule for
//! the host-wide aggregate ceiling. Each test reads the manual page that states the rule
//! and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: HST-509
#[test]
fn hst_509_aggregate_ceiling_never_raises_session_or_bucket_limits() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "MUST NOT raise any session key",
        "or any per-side bucket",
        "MUST pass the session key checks",
        "including a retry of a waiting request",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-509
#[test]
fn hst_509_aggregate_refusal_leaves_side_inactive_without_an_error_code() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "No error code is returned to the plugin for an aggregate refusal",
        "The side stays `inactive` with the reason `host-capacity`",
        "The refusal is not a failure",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
