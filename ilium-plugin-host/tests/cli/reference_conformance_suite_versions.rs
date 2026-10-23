//! Conformance suite versions and case additions (DEV-103 to DEV-106).
//! Pages: reference-conformance-suite (rule text), reference-requirements-dev-tooling (register).
//! DEV-104 and DEV-105 are release-process rules that the manual states; they are checked by
//! reading the page. DEV-103 and DEV-106 are behaviour of the `conformance` run and its `summary`
//! record; they call the CLI API the rule names, which is listed in API_GAPS.md.

fn manual_page(page_file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages")
        .join(page_file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} must be readable: {error}", path.display());
    })
}

/// REQ: DEV-103
#[test]
fn dev_103_summary_suite_field_equals_the_suite_of_the_report_file_of_the_same_run() {
    unimplemented!("ilium-plugin-host::cli::conformance::summary_suite_field")
}

/// REQ: DEV-103
#[test]
fn dev_103_summary_suite_field_is_major_minor_and_missing_field_is_not_a_default() {
    unimplemented!("ilium-plugin-host::cli::conformance::summary_suite_field")
}

/// REQ: DEV-104
#[test]
fn dev_104_patch_release_of_the_suite_must_not_add_remove_or_change_a_case() {
    let page = manual_page("reference-conformance-suite.md");
    assert!(
        page.contains(
            "A PATCH release of the suite MUST NOT add a case, remove one or change one."
        ),
        "the page must state the patch-release rule for cases"
    );
    assert!(
        page.contains("Cases are added only in a MINOR release"),
        "the page must state that cases are added only in a MINOR release"
    );
}

/// REQ: DEV-104
#[test]
fn dev_104_no_text_allows_a_patch_release_to_add_cases() {
    let page = manual_page("reference-conformance-suite.md");
    assert!(
        !page.contains("PATCH release of the suite MAY"),
        "no sentence may let a PATCH release add, remove or change a case"
    );
    assert!(
        page.contains("a MINOR release MUST only add cases"),
        "a MINOR release is limited to adding cases"
    );
}

/// REQ: DEV-105
#[test]
fn dev_105_minor_release_notes_must_list_each_new_case_id_with_its_title() {
    let page = manual_page("reference-conformance-suite.md");
    assert!(
        page.contains(
            "The release notes of every MINOR release that adds cases MUST list each new case id with its title, in that same release"
        ),
        "the page must state the release-notes rule for MINOR releases that add cases"
    );
    assert!(
        page.contains("The notes are the notice to plugin authors that a build can newly fail."),
        "the page must state why the notes are required"
    );
}

/// REQ: DEV-105
#[test]
fn dev_105_release_notes_rule_is_scoped_to_minor_releases_not_every_release() {
    let page = manual_page("reference-conformance-suite.md");
    assert!(
        !page.contains("release notes of every PATCH release"),
        "the release-notes rule must not be extended to PATCH releases"
    );
    assert!(
        page.contains("(proposed, owner to confirm)"),
        "the page must keep the owner-confirmation marker on this rule"
    );
}

/// REQ: DEV-106
#[test]
fn dev_106_shared_case_ids_keep_their_verdicts_between_two_minor_versions() {
    unimplemented!("ilium-plugin-host::cli::conformance::suite_version_run")
}

/// REQ: DEV-106
#[test]
fn dev_106_a_case_added_in_a_minor_release_fails_with_fail_verdict_and_exit_code_3() {
    unimplemented!("ilium-plugin-host::cli::conformance::suite_version_run")
}
