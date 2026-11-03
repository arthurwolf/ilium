//! ONB requirements of the author onboarding page: creating and checking a fresh project
//! (ONB-001 to ONB-004). The statements are read from the manual, in its requirement table and
//! in the marked line of the onboarding page.
use crate::support::{assert_page_line, table_row};

/// REQ: ONB-001
#[test]
fn onb_001_plugin_new_creates_template_files_only_a() {
    let row = table_row("ONB-001");
    assert!(
        row.contains("`plugin new` MUST create the common files listed in"),
        "{row}"
    );
    assert!(
        row.contains("and nothing else outside `<dir>`."),
        "{row}"
    );
}

/// REQ: ONB-001
#[test]
fn onb_001_plugin_new_source_files_come_from_chosen_template_b() {
    assert_page_line(
        "ONB-001",
        &[
            "the source files of the chosen template",
            "and nothing else outside `<dir>`.",
        ],
    );
}

/// REQ: ONB-002
#[test]
fn onb_002_publisher_defaults_to_os_user_name_a() {
    let row = table_row("ONB-002");
    assert!(
        row.contains(
            "The publisher of a new project MUST be the operating system user name unless `--publisher` is given"
        ),
        "{row}"
    );
}

/// REQ: ONB-002
#[test]
fn onb_002_display_name_defaults_to_last_id_label_b() {
    assert_page_line(
        "ONB-002",
        &[
            "the display name MUST be the last label of the id unless `--name` is given.",
        ],
    );
}

/// REQ: ONB-003
#[test]
fn onb_003_author_must_not_edit_dist_by_hand_a() {
    let row = table_row("ONB-003");
    assert!(
        row.contains("The author MUST NOT edit files under `dist/` by hand."),
        "{row}"
    );
    assert!(
        row.contains("`build` MUST be the only writer of `dist/<side>.wasm` and `dist/build.json`"),
        "{row}"
    );
}

/// REQ: ONB-003
#[test]
fn onb_003_test_coverage_writes_under_ilium_coverage_b() {
    assert_page_line(
        "ONB-003",
        &["`test --coverage` MUST write its output under `.ilium/coverage/`."],
    );
}

/// REQ: ONB-004
#[test]
fn onb_004_fresh_project_passes_four_commands_with_exit_zero_a() {
    let row = table_row("ONB-004");
    assert!(
        row.contains(
            "A fresh project MUST pass the four commands above with exit code 0 and no warning, in that order."
        ),
        "{row}"
    );
}

/// REQ: ONB-004
#[test]
fn onb_004_failing_fresh_project_is_reported_not_edited_b() {
    assert_page_line(
        "ONB-004",
        &[
            "the author SHOULD treat it as a template or toolchain problem and report it",
            "rather than edit the template files",
        ],
    );
}
