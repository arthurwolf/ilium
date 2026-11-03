//! ONB requirements of the author onboarding page: language detection, missing toolchains and
//! offline local checks (ONB-006 to ONB-008). The statements are read from the manual, in its
//! requirement table and in the marked line of the onboarding page.
use crate::support::{assert_page_line, table_row};

/// REQ: ONB-006
#[test]
fn onb_006_single_manifest_selects_its_language_a() {
    let row = table_row("ONB-006");
    assert!(
        row.contains(
            "A project with only `Cargo.toml` MUST build as Rust, and a project with only `package.json` MUST build as TypeScript."
        ),
        "{row}"
    );
}

/// REQ: ONB-006
#[test]
fn onb_006_ambiguous_or_unknown_language_fails_with_codes_b() {
    assert_page_line(
        "ONB-006",
        &[
            "A project with both files MUST fail with `toolchain.language-ambiguous`",
            "a project with neither MUST fail with `toolchain.language-unknown` ([DEV-002]).",
            "The author MUST keep one language per project.",
        ],
    );
}

/// REQ: ONB-007
#[test]
fn onb_007_missing_toolchain_stops_build_with_exit_four_a() {
    let row = table_row("ONB-007");
    assert!(
        row.contains(
            "When a toolchain is missing, `build` MUST stop with a `toolchain.*` code and exit with 4"
        ),
        "{row}"
    );
}

/// REQ: ONB-007
#[test]
fn onb_007_doctor_lists_missing_items_with_fail_status_b() {
    assert_page_line(
        "ONB-007",
        &["Running `doctor` lists every missing item with status `fail` and also exits with 4."],
    );
}

/// REQ: ONB-008
#[test]
fn onb_008_check_and_lint_run_without_network_a() {
    let row = table_row("ONB-008");
    assert!(
        row.contains("`check` and `lint` MUST run without a network connection."),
        "{row}"
    );
}

/// REQ: ONB-008
#[test]
fn onb_008_check_print_schema_runs_offline_b() {
    assert_page_line(
        "ONB-008",
        &["`check --print-schema` MUST also run offline and print the manifest schema"],
    );
}
