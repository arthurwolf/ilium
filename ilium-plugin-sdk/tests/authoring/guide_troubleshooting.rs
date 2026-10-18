//! Rule statements of the guide-troubleshooting page as the manual states them (doc-lint tests, part 2).
use crate::support::assert_rule;

/// REQ: AUT-801 WARN
#[test]
fn doc_aut_801_fixed_runtime_bug_becomes_test_before_next_release() {
    assert_rule(
        "AUT-801",
        "SHOULD",
        &["SHOULD turn every fixed run-time bug into a test before the next release"],
    );
}

/// REQ: AUT-802
#[test]
fn doc_aut_802_install_error_code_read_before_changing_anything() {
    assert_rule(
        "AUT-802",
        "MUST",
        &["the code in an install error", "each code names one cause"],
    );
}

/// REQ: AUT-803 WARN
#[test]
fn doc_aut_803_report_contains_version_id_manifest_100_log_lines_and_test() {
    assert_rule("AUT-803", "SHOULD", &["SHOULD contain the Ilium version, the plugin id and version, the manifest, the last 100 log lines and a test that reproduces it"]);
}

/// REQ: AUT-804
#[test]
fn doc_aut_804_diagnosis_not_dependent_on_unredacted_traces() {
    assert_rule(
        "AUT-804",
        "MUST NOT",
        &[
            "MUST NOT depend on `--unredacted` traces",
            "works only for a plugin installed with `dev`",
        ],
    );
}
