//! Rule statements of the errors-and-limits page as the manual states them (doc-lint tests).
use crate::support::assert_rule;

/// REQ: AUT-110
#[test]
fn doc_aut_110_defining_page_decides() {
    assert_rule(
        "AUT-110",
        "MUST",
        &[
            "the defining page decides",
            "the cause that the defining page gives",
        ],
    );
}

/// REQ: AUT-111
#[test]
fn doc_aut_111_limit_is_a_trap_not_a_warning() {
    assert_rule(
        "AUT-111",
        "MUST",
        &["treat a limit as a trap", "not as a warning"],
    );
}
