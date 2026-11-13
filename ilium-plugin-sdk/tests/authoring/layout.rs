//! Rule statements of the layout page as the manual states them (doc-lint tests).
use crate::support::assert_rule;

/// REQ: AUT-505 WARN
#[test]
fn doc_aut_505_views_tested_at_40_by_12() {
    assert_rule(
        "AUT-505",
        "SHOULD",
        &["40 columns by 12 rows", "`lint.widget-missing-key`"],
    );
}

/// REQ: AUT-506 WARN
#[test]
fn doc_aut_506_prefer_fill_and_percentage_for_text() {
    assert_rule("AUT-506", "SHOULD", &["`fill` and `percentage`"]);
}
