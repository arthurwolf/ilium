//! Release-notes rules DEV-950 to DEV-954 (reference-cli-output-stability.md, "Production
//! additions: release notes"). The documentation rules read the manual page and the repository's
//! `CHANGELOG.md`. The pipeline rules describe a release pipeline that does not exist in the
//! repository yet, so their tests stay unimplemented and are listed in `API_GAPS.md`.

use crate::dev_pipeline_manual::{manual_page, repository_file};

const KEEP_A_CHANGELOG_SECTIONS: [&str; 5] = ["Added", "Changed", "Removed", "Fixed", "Security"];

/// REQ: DEV-950
#[test]
fn dev_950_release_notes_are_the_changelog_heading_with_the_release_date() {
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("`## <version> (<YYYY-MM-DD>)`"),
        "the page must give the heading form of a release entry"
    );
    assert!(
        page.contains("whose date is the release date"),
        "the heading date must be the release date"
    );
    assert!(
        page.contains("Deprecations under [DEV-863] go under `Changed`"),
        "deprecations go under Changed"
    );
}

/// REQ: DEV-950
#[test]
fn dev_950_changelog_uses_only_the_five_keep_a_changelog_sections() {
    let changelog = repository_file("CHANGELOG.md");
    let section_names: Vec<&str> = changelog
        .lines()
        .filter_map(|line| line.strip_prefix("### "))
        .map(str::trim)
        .collect();
    assert!(
        !section_names.is_empty(),
        "CHANGELOG.md must hold at least one section heading"
    );
    for section_name in &section_names {
        assert!(
            KEEP_A_CHANGELOG_SECTIONS.contains(section_name),
            "section `{section_name}` is not one of the five allowed sections"
        );
    }
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("`Added`, `Changed`, `Removed`, `Fixed` and `Security`"),
        "the page must list exactly the five allowed sections"
    );
}

/// REQ: DEV-951
#[test]
fn dev_951_the_changelog_entry_shares_the_version_commit_and_precedes_the_tag() {
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("committed in the same commit as the version change"),
        "the changelog entry is committed with the version change"
    );
    assert!(
        page.contains("the release tag MUST NOT be created before that commit exists"),
        "the release tag waits for that commit"
    );
}

/// REQ: DEV-951
#[test]
fn dev_951_release_pipeline_fails_without_artifact_when_tag_has_no_version_heading() {
    unimplemented!("ilium-plugin-host::release::pipeline::check_tag_has_changelog_heading");
}

/// REQ: DEV-952
#[test]
fn dev_952_release_notes_are_published_with_the_artifacts_never_after_them() {
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("published together with the release artifacts, never after them"),
        "the notes are published together with the artifacts"
    );
    assert!(
        page.contains("the `CHANGELOG.md` entry is the record that a check reads"),
        "the changelog entry is the record a check reads"
    );
}

/// REQ: DEV-952
#[test]
fn dev_952_publishing_notes_after_the_artifacts_is_refused_by_the_pipeline() {
    unimplemented!("ilium-plugin-host::release::pipeline::publish_notes_with_artifacts");
}

/// REQ: DEV-953
#[test]
fn dev_953_a_security_fix_is_listed_under_security_with_its_advisory_and_cve() {
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("list the fix under `Security`"),
        "a vulnerability fix is listed under Security"
    );
    assert!(
        page.contains("SEC-322") && page.contains("SEC-702"),
        "the entry names the advisory identifier and the CVE identifier"
    );
}

/// REQ: DEV-953
#[test]
fn dev_953_the_security_entry_does_not_name_the_flaw_before_the_advisory_is_public() {
    unimplemented!("ilium-plugin-host::release::pipeline::security_entry_before_advisory");
}

/// REQ: DEV-954
#[test]
fn dev_954_the_removed_section_rule_is_stated_with_the_stop_release_condition() {
    let page = manual_page("reference-cli-output-stability.md");
    assert!(
        page.contains("MUST fail when a `Removed` entry names an item that no earlier"),
        "the pipeline fails on a Removed entry without an earlier deprecation"
    );
    assert!(
        page.contains("stop release equal to the release being built"),
        "the earlier deprecation must name the release being built as its stop release"
    );
}

/// REQ: DEV-954
#[test]
fn dev_954_removed_entry_without_a_matching_deprecation_fails_the_release_pipeline() {
    unimplemented!("ilium-plugin-host::release::pipeline::removed_entry_needs_deprecation");
}
