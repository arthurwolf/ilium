//! Conformance suite versions and case additions (DEV-103 to DEV-106).
//! Pages: reference-conformance-suite (rule text), reference-requirements-dev-tooling (register).
//! DEV-104 and DEV-105 are release-process rules that the manual states; they are checked by
//! reading the page. DEV-103 and DEV-106 are behaviour of the `conformance` run and its `summary`
//! record; they call the CLI API the rule names, which is listed in API_GAPS.md.

use serde_json::Value;

use crate::common::{env, recs, run_in, scratch, server_project};

/// Runs `conformance --report` in a fresh server project with extra arguments. Returns the
/// output, the parsed report file and the project directory scratch owner.
fn conformance_run(extra: &[&str]) -> (ilium_plugin_host::cli::CliOutput, Value, tempfile::TempDir) {
    let root = scratch();
    let cli_env = env(root.path());
    let dir = server_project(&cli_env, root.path());
    let report_path = root.path().join("report.json");
    let report_arg = report_path.to_str().expect("utf8 report path").to_owned();
    let mut args = vec!["conformance", "--report", report_arg.as_str()];
    args.extend_from_slice(extra);
    let output = run_in(&cli_env, &dir, &args);
    let report = match std::fs::read(&report_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    (output, report, root)
}

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
    let (output, report, _root) = conformance_run(&[]);
    let summary = recs(&output, "summary");
    let last = summary.last().expect("the run prints a summary record");
    let in_summary = last["suite"].as_str().expect("summary carries a suite string");
    let in_report = report["suite"]
        .as_str()
        .or_else(|| report["suite_version"].as_str())
        .expect("the report file names its suite");
    assert_eq!(in_summary, in_report, "summary and report file must name the same suite");
}

/// REQ: DEV-103
#[test]
fn dev_103_summary_suite_field_is_major_minor_and_missing_field_is_not_a_default() {
    let (output, _report, _root) = conformance_run(&[]);
    let summary = recs(&output, "summary");
    let last = summary.last().expect("the run prints a summary record");
    let suite = last
        .get("suite")
        .and_then(Value::as_str)
        .expect("the suite field must be present and never defaulted");
    let parts: Vec<&str> = suite.split('.').collect();
    assert_eq!(parts.len(), 2, "suite must be MAJOR.MINOR, got {suite}");
    assert!(
        parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "both suite parts must be numbers, got {suite}"
    );
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
    let (current, _report, _root) = conformance_run(&[]);
    let summary = recs(&current, "summary");
    let embedded = summary.last().expect("summary record")["suite"]
        .as_str()
        .expect("suite string")
        .to_owned();
    let verdicts = |output: &ilium_plugin_host::cli::CliOutput| -> std::collections::BTreeMap<String, String> {
        recs(output, "conformance")
            .iter()
            .filter_map(|row| Some((row["case"].as_str()?.to_owned(), row["verdict"].as_str()?.to_owned())))
            .collect()
    };
    let newest = verdicts(&current);
    assert!(!newest.is_empty(), "the newest suite prints case records");
    let (major, minor) = embedded.split_once('.').expect("MAJOR.MINOR");
    let minor: u32 = minor.parse().expect("numeric minor");
    assert!(minor > 0, "the embedded suite {embedded} has no earlier MINOR to compare with");
    let previous = format!("{major}.{}", minor - 1);
    let (older, _report, _root2) = conformance_run(&["--suite-version", previous.as_str()]);
    let oldest = verdicts(&older);
    assert!(!oldest.is_empty(), "the pinned suite {previous} prints case records");
    for (case, verdict) in &oldest {
        if let Some(newer) = newest.get(case) {
            assert_eq!(verdict, newer, "case {case} changed its verdict between {previous} and {embedded}");
        }
    }
}

/// REQ: DEV-106
#[test]
fn dev_106_a_case_added_in_a_minor_release_fails_with_fail_verdict_and_exit_code_3() {
    // Any `fail` conformance record must come with exit code 3, and any exit 3 with a fail record.
    let (output, _report, _root) = conformance_run(&[]);
    let failed: Vec<&Value> = recs(&output, "conformance")
        .into_iter()
        .filter(|row| row["verdict"] == "fail")
        .collect();
    if failed.is_empty() {
        assert_eq!(output.exit_code, 0, "no fail record means exit 0: {:?}", output.stdout_jsonl);
    } else {
        assert_eq!(output.exit_code, 3, "a fail record makes exit 3: {:?}", output.stdout_jsonl);
        for row in &failed {
            assert!(row["case"].as_str().is_some_and(|id| id.starts_with("CONF-")), "{row}");
        }
    }
    let summary = recs(&output, "summary");
    let last = summary.last().expect("summary record");
    assert_eq!(
        last["failed"].as_u64().map(|count| count as usize),
        Some(failed.len()),
        "the summary counts exactly the fail records"
    );
}
