//! reference-security-response.md: security.txt, safe harbor and reporter credit (SEC-439 .. SEC-441).
//! Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-439
#[test]
fn sec_439_the_response_page_states_the_security_txt_rule() {
    assert_page_has(
        "reference-security-response.md",
        &[
            "publish a `security.txt` (RFC 9116)",
            "`/.well-known/security.txt`",
            "no more than 12 months ahead",
            "It MUST name the same contact as `SECURITY.md`",
        ],
    );
}

/// REQ: SEC-440
#[test]
fn sec_440_the_response_page_states_the_good_faith_safe_harbor() {
    assert_page_has(
        "reference-security-response.md",
        &[
            "the project MUST NOT start or support legal action against a reporter who acts in good faith",
            "tests only their own installation",
            "stops at proof of the issue",
        ],
    );
}

/// REQ: SEC-441
#[test]
fn sec_441_the_response_page_states_reporter_credit_and_the_embargo_cap() {
    assert_page_has(
        "reference-security-response.md",
        &[
            "MUST be credited by name or handle in the advisory of [SEC-322]",
            "a reporter who asks for anonymity MUST NOT be named",
            "MUST NOT lengthen it past the 90 days of [SEC-321]",
        ],
    );
}
