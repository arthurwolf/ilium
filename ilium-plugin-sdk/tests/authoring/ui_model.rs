//! Rule statements of the ui-model page as the manual states them (doc-lint tests, part 2).
use crate::support::assert_rule;

/// REQ: AUT-507
#[test]
fn doc_aut_507_state_shown_in_text_as_well_as_colour() {
    assert_rule("AUT-507", "MUST", &["MUST show state in text as well as in colour"]);
}
