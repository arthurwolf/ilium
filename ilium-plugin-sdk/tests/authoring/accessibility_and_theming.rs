//! Rule statements of the accessibility-and-theming page as the manual states them (doc-lint tests).
use crate::support::assert_rule;

/// REQ: AUT-501
#[test]
fn doc_aut_501_colour_never_alone_status_has_word_or_glyph() {
    assert_rule("AUT-501", "MUST NOT", &["MUST NOT use colour as the only carrier of meaning", "a word or a glyph beside its colour role"]);
}

/// REQ: AUT-502
#[test]
fn doc_aut_502_reduced_motion_stops_own_animating_timers() {
    assert_rule("AUT-502", "MUST", &["`reduced-motion`", "stop its own animating timers"]);
}

/// REQ: AUT-503 WARN
#[test]
fn doc_aut_503_works_in_compact_density_at_40_columns() {
    assert_rule("AUT-503", "SHOULD", &["`compact` density at 40 columns", "both icon modes"]);
}

/// REQ: AUT-504 WARN
#[test]
fn doc_aut_504_nine_colour_roles_not_rgb_or_indexed() {
    assert_rule("AUT-504", "SHOULD", &["the nine colour roles", "`rgb` and `indexed`"]);
}
