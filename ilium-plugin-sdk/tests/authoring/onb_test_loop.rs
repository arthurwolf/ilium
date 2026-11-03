//! ONB requirements of the author onboarding page: the test loop of `test`, its exit codes,
//! its updates and its traps (ONB-005, ONB-015 to ONB-021). The statements are read from the
//! manual, in its requirement table and in the marked line of the onboarding page.
use crate::support::{assert_page_line, table_row};

/// REQ: ONB-005
#[test]
fn onb_005_test_without_built_component_fails_with_code_a() {
    let row = table_row("ONB-005");
    assert!(
        row.contains(
            "`test` on a project with no built component for its side MUST fail with `test.component-missing` and exit with 1."
        ),
        "{row}"
    );
}

/// REQ: ONB-005
#[test]
fn onb_005_author_runs_build_then_reruns_test_b() {
    assert_page_line(
        "ONB-005",
        &["The author MUST run `build` and rerun `test`."],
    );
}

/// REQ: ONB-015
#[test]
fn onb_015_fixture_without_expectation_passes_with_warning_a() {
    let row = table_row("ONB-015");
    assert!(
        row.contains(
            "A fixture with no expectation file MUST pass and print the warning `test.no-expectation`."
        ),
        "{row}"
    );
}

/// REQ: ONB-015
#[test]
fn onb_015_author_should_add_expectation_before_ci_b() {
    assert_page_line(
        "ONB-015",
        &["The author SHOULD add an expectation for every fixture before the first CI run"],
    );
}

/// REQ: ONB-016
#[test]
fn onb_016_test_exit_codes_zero_three_one_a() {
    let row = table_row("ONB-016");
    assert!(
        row.contains(
            "`test` MUST exit with 0 when every case passes, 3 when a case fails or traps, and 1 when a fixture cannot be read"
        ),
        "{row}"
    );
}

/// REQ: ONB-016
#[test]
fn onb_016_mismatch_prints_diff_record_b() {
    assert_page_line("ONB-016", &["A mismatch MUST print a `diff` record."]);
}

/// REQ: ONB-017
#[test]
fn onb_017_two_sided_package_requires_side_flag_a() {
    let row = table_row("ONB-017");
    assert!(
        row.contains(
            "A package with two sides MUST be given `--side` for `test`, or the command fails with `usage.missing-argument`"
        ),
        "{row}"
    );
}

/// REQ: ONB-017
#[test]
fn onb_017_one_sided_project_needs_no_flag_b() {
    assert_page_line("ONB-017", &["A one-side project needs no flag."]);
}

/// REQ: ONB-018
#[test]
fn onb_018_update_rewrites_only_differing_files_a() {
    let row = table_row("ONB-018");
    assert!(
        row.contains(
            "`test --update` MUST rewrite only the expectation and golden files that differ"
        ),
        "{row}"
    );
    assert!(
        row.contains("MUST print one `test.updated` warning for each file rewritten"),
        "{row}"
    );
}

/// REQ: ONB-018
#[test]
fn onb_018_author_reads_diff_before_committing_rewrite_b() {
    assert_page_line(
        "ONB-018",
        &["The author reads the diff in version control before committing the rewrite."],
    );
}

/// REQ: ONB-019
#[test]
fn onb_019_test_does_not_run_project_unit_tests_a() {
    let row = table_row("ONB-019");
    assert!(
        row.contains("`test` MUST NOT run the project's own unit tests."),
        "{row}"
    );
}

/// REQ: ONB-019
#[test]
fn onb_019_coverage_is_only_form_starting_unit_tests_b() {
    assert_page_line(
        "ONB-019",
        &[
            "`test --coverage` is the only form of `test` that starts them",
            "Those run with `cargo test` or `bun test` in native mode",
        ],
    );
}

/// REQ: ONB-020
#[test]
fn onb_020_exit_three_must_be_read_before_changing_code_a() {
    let row = table_row("ONB-020");
    assert!(
        row.contains(
            "MUST read the first `diff` or trap record before changing the code, and MUST NOT update the expectation file to hide a trap"
        ),
        "{row}"
    );
}

/// REQ: ONB-020
#[test]
fn onb_020_update_only_for_intended_output_change_b() {
    assert_page_line(
        "ONB-020",
        &["`test --update` is for an intended change of output only."],
    );
}

/// REQ: ONB-021
#[test]
fn onb_021_trap_position_is_file_line_and_reason_a() {
    let row = table_row("ONB-021");
    assert!(
        row.contains(
            "An author who sees a trap MUST use the `file:line` of the trap and the reason in its record."
        ),
        "{row}"
    );
}

/// REQ: ONB-021
#[test]
fn onb_021_release_trap_needs_dev_or_debug_rebuild_b() {
    assert_page_line(
        "ONB-021",
        &[
            "the author MUST rebuild with `dev` or `build --debug` before reading the position",
        ],
    );
}
