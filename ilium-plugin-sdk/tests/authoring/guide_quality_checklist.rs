//! Rule statements of the guide-quality-checklist page as the manual states them (doc-lint tests).
use crate::support::{assert_page_line, assert_rule, table_row};

/// REQ: AUT-401
#[test]
fn doc_aut_401_release_has_no_check_or_lint_error() {
    assert_rule(
        "AUT-401",
        "MUST",
        &["no `check` error and no unsuppressed `lint` error"],
    );
}

/// REQ: AUT-402 WARN
#[test]
fn doc_aut_402_release_passes_deny_warnings_with_reasoned_suppressions() {
    assert_rule(
        "AUT-402",
        "SHOULD",
        &[
            "`ilium plugin lint --deny warnings`",
            "carry a reason that a reviewer can read",
        ],
    );
}

/// REQ: AUT-403 WARN
#[test]
fn doc_aut_403_release_lists_no_unstable_feature() {
    assert_rule(
        "AUT-403",
        "MUST NOT",
        &["MUST NOT list an `unstable` feature"],
    );
}

/// REQ: AUT-404
#[test]
fn doc_aut_404_smallest_grant_set_explained_in_readme() {
    assert_rule(
        "AUT-404",
        "MUST",
        &["explained in the README with the feature that needs it"],
    );
}

/// REQ: AUT-405
#[test]
fn doc_aut_405_optional_grant_handles_not_permitted() {
    assert_rule(
        "AUT-405",
        "MUST",
        &[
            "`not-permitted` for every call of an optional grant",
            "reduced feature set",
        ],
    );
}

/// REQ: AUT-406 WARN
#[test]
fn doc_aut_406_measured_use_below_80_percent_of_limit() {
    assert_rule("AUT-406", "SHOULD", &["below 80 percent of its limit"]);
}

/// REQ: AUT-407
#[test]
fn doc_aut_407_interactive_widget_has_stable_key() {
    assert_rule(
        "AUT-407",
        "MUST",
        &["every interactive widget a stable `key`"],
    );
}

/// REQ: AUT-408
#[test]
fn doc_aut_408_tests_for_activation_deactivation_and_each_error_code() {
    assert_rule(
        "AUT-408",
        "MUST",
        &["at least one test for activation, one for deactivation and one for each error code"],
    );
}

/// REQ: AUT-409
#[test]
fn doc_aut_409_layout_change_tested_with_previous_storage_snapshot() {
    assert_rule("AUT-409", "MUST", &["storage snapshot of that release"]);
}

/// REQ: AUT-410
#[test]
fn doc_aut_410_colour_not_the_only_carrier_of_meaning_quality() {
    assert_rule(
        "AUT-410",
        "MUST NOT",
        &["MUST NOT use colour as the only carrier of meaning"],
    );
}

/// REQ: AUT-401
#[test]
fn doc_aut_401_release_check_row_names_the_verifying_commands() {
    assert!(table_row("AUT-401")
        .contains("Run `ilium plugin check` or `lint` and read the finding"));
}

/// REQ: AUT-402
#[test]
fn doc_aut_402_suppression_check_row_names_the_verifying_commands() {
    assert!(table_row("AUT-402")
        .contains("Run `ilium plugin check` or `lint` and read the finding"));
}

/// REQ: AUT-406
#[test]
fn doc_aut_406_measured_use_is_taken_on_the_documented_fixture_set() {
    assert_page_line(
        "AUT-406",
        &["on the fixture data set named in the README"],
    );
}
