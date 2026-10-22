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
