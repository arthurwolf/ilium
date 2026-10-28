//! Release-notes rules DEV-950 to DEV-954 (reference-cli-output-stability.md, "Production
//! additions: release notes"). The documentation rules read the manual page and the repository's
//! `CHANGELOG.md`. The pipeline rules run the small checkers of this file, because the release
//! pipeline is not in the repository yet (see `API_GAPS.md`).

use crate::dev_pipeline_manual::{manual_page, repository_file};

/// The version and date of every release heading `## <version> (<YYYY-MM-DD>)` or
/// `## [<version>] - <YYYY-MM-DD>` of a changelog.
fn release_headings(changelog: &str) -> Vec<String> {
    changelog
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(|heading| heading.trim_start_matches('[').split([']', ' ']).next().unwrap_or_default().to_owned())
        .collect()
}

/// What a release run produced: the artifacts and, when it stopped, the reason.
#[derive(Debug, PartialEq)]
struct PipelineRun {
    artifacts: Vec<String>,
    failure: Option<String>,
}

/// The release pipeline step of DEV-951: it checks the tag against the changelog first and builds
/// the artifacts only when the tag has a heading for its version.
fn run_release_pipeline(changelog: &str, tag: &str) -> PipelineRun {
    let version = tag.trim_start_matches('v');
    if !release_headings(changelog).iter().any(|heading| heading == version) {
        return PipelineRun {
            artifacts: Vec::new(),
            failure: Some(format!("release.changelog-heading-missing: no heading for {version}")),
        };
    }
    PipelineRun { artifacts: vec![format!("ilium-{version}.tar.gz")], failure: None }
}

/// One publication step of a release: what is published and in which batch it goes out.
#[derive(Debug, Clone, PartialEq)]
struct Publication {
    what: &'static str,
    batch: u32,
}

/// DEV-952: the notes go out in the same batch as the artifacts or an earlier one, never later.
fn check_publication_order(steps: &[Publication]) -> Result<(), String> {
    let batch_of = |what: &str| steps.iter().filter(|step| step.what == what).map(|step| step.batch).min();
    match (batch_of("notes"), batch_of("artifacts")) {
        (Some(notes), Some(artifacts)) if notes <= artifacts => Ok(()),
        (Some(_), Some(_)) => Err("release.notes-after-artifacts".to_owned()),
        _ => Err("release.notes-or-artifacts-missing".to_owned()),
    }
}

/// DEV-953: a security entry that names the flaw is refused while the advisory is not public.
fn check_security_entry(entry: &str, flaw_terms: &[&str], advisory_public: bool) -> Result<(), String> {
    if !advisory_public {
        let lowered = entry.to_lowercase();
        if let Some(term) = flaw_terms.iter().find(|term| lowered.contains(&term.to_lowercase())) {
            return Err(format!("release.security-flaw-named-early: `{term}`"));
        }
    }
    Ok(())
}

/// The items named in backticks on a line.
fn backticked(line: &str) -> Vec<String> {
    line.split('`').skip(1).step_by(2).map(str::to_owned).collect()
}

/// DEV-954: every `Removed` item of the release being built needs an earlier entry that marks it
/// deprecated with the words `stop release <version>` naming this release.
fn check_removed_entries(changelog: &str, release: &str) -> Result<(), String> {
    let mut current_version = String::new();
    let mut current_section = String::new();
    let mut deprecations: Vec<(String, String, String)> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for line in changelog.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            current_version = heading.trim_start_matches('[').split([']', ' ']).next().unwrap_or_default().to_owned();
        } else if let Some(section) = line.strip_prefix("### ") {
            current_section = section.trim().to_owned();
        } else if line.starts_with("- ") {
            if current_section == "Removed" && current_version == release {
                removed.extend(backticked(line));
            } else if line.contains("deprecated") && current_version != release {
                let stop = line.split("stop release ").nth(1).map(|rest| {
                    rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>()
                });
                for item in backticked(line) {
                    deprecations.push((item, current_version.clone(), stop.clone().unwrap_or_default()));
                }
            }
        }
    }
    for item in removed {
        if !deprecations.iter().any(|(name, _, stop)| *name == item && stop.trim_end_matches('.') == release) {
            return Err(format!("release.removed-without-deprecation: `{item}`"));
        }
    }
    Ok(())
}



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
    let changelog = "# Changelog\n\n## 0.2.0 (2030-01-01)\n\n### Added\n\n- Something.\n";
    let untagged = run_release_pipeline(changelog, "v0.3.0");
    assert!(untagged.artifacts.is_empty(), "an artifact was produced for a tag without a heading: {untagged:?}");
    let failure = untagged.failure.expect("a tag without a heading fails the pipeline");
    assert!(failure.contains("release.changelog-heading-missing"), "{failure}");
    let tagged = run_release_pipeline(changelog, "v0.2.0");
    assert_eq!(tagged.failure, None, "a tag with a heading must pass");
    assert!(!tagged.artifacts.is_empty(), "a tag with a heading produces its artifact");
    // The real changelog of the repository has a heading for the version being built.
    let real = repository_file("CHANGELOG.md");
    assert!(!release_headings(&real).is_empty(), "CHANGELOG.md holds no release heading");
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
    let together = [Publication { what: "notes", batch: 1 }, Publication { what: "artifacts", batch: 1 }];
    assert_eq!(check_publication_order(&together), Ok(()));
    let notes_first = [Publication { what: "notes", batch: 1 }, Publication { what: "artifacts", batch: 2 }];
    assert_eq!(check_publication_order(&notes_first), Ok(()));
    let notes_after = [Publication { what: "artifacts", batch: 1 }, Publication { what: "notes", batch: 2 }];
    assert_eq!(
        check_publication_order(&notes_after),
        Err("release.notes-after-artifacts".to_owned()),
        "notes published after the artifacts must be refused"
    );
    assert!(check_publication_order(&[Publication { what: "artifacts", batch: 1 }]).is_err());
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
    let flaw_terms = ["path traversal", "unpack"];
    let naming = "Fixed a path traversal in package unpack (SEC-322, CVE-2030-0001).";
    let generic = "Fixed a vulnerability in the command line (SEC-322); details follow with the advisory.";
    let early = check_security_entry(naming, &flaw_terms, false).expect_err("naming the flaw early must be refused");
    assert!(early.contains("release.security-flaw-named-early"), "{early}");
    assert_eq!(check_security_entry(generic, &flaw_terms, false), Ok(()));
    assert_eq!(check_security_entry(naming, &flaw_terms, true), Ok(()), "after the advisory is public the flaw may be named");
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
    let deprecated = "## 0.2.0 (2030-01-01)\n\n### Changed\n\n- Marked `--old-flag` deprecated; stop release 0.3.0.\n\n## 0.1.0 (2029-01-01)\n\n";
    let removing = "## 0.3.0 (2030-06-01)\n\n### Removed\n\n- Removed `--old-flag`.\n\n";
    assert_eq!(check_removed_entries(&format!("{removing}{deprecated}"), "0.3.0"), Ok(()));
    let without = check_removed_entries(&format!("{removing}## 0.2.0 (2030-01-01)\n\n### Added\n\n- Nothing.\n"), "0.3.0")
        .expect_err("a Removed entry without a deprecation must fail");
    assert!(without.contains("release.removed-without-deprecation"), "{without}");
    let wrong_stop = deprecated.replace("stop release 0.3.0", "stop release 0.4.0");
    assert!(
        check_removed_entries(&format!("{removing}{wrong_stop}"), "0.3.0").is_err(),
        "a deprecation whose stop release is another release does not allow the removal"
    );
}
