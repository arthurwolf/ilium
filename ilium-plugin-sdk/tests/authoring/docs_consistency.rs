//! Cross-page checks of codes and lint ids that the author guides cite (AUT-110 and friends).

use crate::support::page;

fn row_of(text: &str, code: &str) -> String {
    let prefix = format!("| `{code}` |");
    text.lines().find(|line| line.starts_with(&prefix)).unwrap_or_else(|| panic!("row {code} missing")).to_string()
}

/// REQ: AUT-110, AUT-319
#[test]
fn doc_aut_110_version_reused_cause_matches_the_defining_page() {
    let defining = page("reference-errors-and-limits");
    let row = row_of(&defining, "package.version-reused");
    assert!(row.contains("The version is already installed with different content"));
    for guide in ["guide-publishing-and-support", "guide-compatibility-and-upgrades"] {
        let text = page(guide);
        assert!(text.contains("`package.version-reused`"), "{guide} cites the code");
        assert!(text.contains("different content"), "{guide} gives the same cause");
    }
}

/// REQ: AUT-110, AUT-305
#[test]
fn doc_aut_110_id_publisher_code_is_defined_and_cited() {
    assert!(page("reference-errors-and-limits").contains("`manifest.id-publisher`"));
    assert!(page("guide-publishing-and-support").contains("refused with `manifest.id-publisher`"));
}

/// REQ: AUT-110, AUT-321
#[test]
fn doc_aut_110_unknown_key_code_is_defined_and_cited() {
    assert!(page("reference-errors-and-limits").contains("`manifest.unknown-key`"));
    assert!(page("reference-manifest").contains("`manifest.unknown-key`"));
}

/// REQ: AUT-110, AUT-313
#[test]
fn doc_aut_110_platform_code_is_defined_in_the_package_format() {
    assert!(page("reference-package-format").contains("`package.platform-unsupported`"));
    assert!(page("guide-publishing-and-support").contains("refused on Windows and macOS with `package.platform-unsupported`"));
}

/// REQ: AUT-311 WARN
#[test]
fn doc_aut_311_missing_readme_and_changelog_are_info_findings() {
    let rules = page("reference-check-and-lint-rules");
    assert!(row_of(&rules, "lint.readme-missing").contains("| info |"));
    assert!(row_of(&rules, "lint.changelog-missing").contains("| info |"));
}

/// REQ: AUT-303
#[test]
fn doc_aut_303_debug_info_lint_asks_for_a_release_build() {
    let rules = page("reference-check-and-lint-rules");
    let row = row_of(&rules, "lint.debug-info-in-release");
    assert!(row.contains("DWARF or a source map"));
    assert!(row.contains("Rebuild with `--release`"));
}

/// REQ: AUT-407
#[test]
fn doc_aut_407_missing_key_lint_names_interactive_widgets() {
    let rules = page("reference-check-and-lint-rules");
    let row = row_of(&rules, "lint.widget-missing-key");
    assert!(row.contains("An interactive widget has no `key`"));
    assert!(row.contains("| warn |"));
}

/// REQ: AUT-403 WARN, AUT-210
#[test]
fn doc_aut_403_unstable_feature_lint_is_a_warning() {
    let rules = page("reference-check-and-lint-rules");
    assert!(row_of(&rules, "lint.unstable-feature").contains("| warn |"));
}

/// REQ: AUT-312 WARN
#[test]
fn doc_aut_312_package_file_name_pattern_is_the_same_on_both_pages() {
    assert!(page("guide-publishing-and-support").contains("`<id>-<version>.iliumpkg`"));
    assert!(page("tutorial-09-package-and-share").contains(".iliumpkg"));
}
