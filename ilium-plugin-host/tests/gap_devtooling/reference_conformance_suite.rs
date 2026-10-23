//! Conformance report, suite versions and deterministic environment (DEV-923, DEV-924, DEV-925).
//! Page: reference-conformance-suite.

use serde_json::Value;

use crate::common::*;
use crate::{embedded_suite_version, new_rust_project, utf8};

/// REQ: DEV-923
#[test]
fn conformance_report_is_ilium_conformance_1_json_and_byte_identical_across_runs() {
    let (root, env, dir) = new_rust_project();
    let first_path = root.path().join("report-a.json");
    let second_path = root.path().join("report-b.json");
    let first = run_in_project(&env, "conformance", &["--report", utf8(&first_path)]);
    assert_eq!(first.exit_code, 0, "{:?}", first.stdout_jsonl);
    let second = run_in_project(&env, "conformance", &["--report", utf8(&second_path)]);
    assert_eq!(second.exit_code, 0, "{:?}", second.stdout_jsonl);
    let first_bytes = std::fs::read(&first_path).expect("first report");
    let second_bytes = std::fs::read(&second_path).expect("second report");
    assert_eq!(
        first_bytes, second_bytes,
        "two runs on the same inputs write the same bytes"
    );

    let report: Value = serde_json::from_slice(&first_bytes).expect("report is JSON");
    assert_eq!(report["format"], "ilium-conformance/1", "{report}");
    for key in ["suite_version", "cli_version", "cases"] {
        assert!(report.get(key).is_some(), "the report contains {key}");
    }
    assert!(
        report.to_string().contains("example.agent-tools"),
        "the plugin id is in the report"
    );
    let manifest_digest =
        sha256_hex(&std::fs::read(dir.join("manifest.json")).expect("manifest bytes"));
    assert!(
        report.to_string().contains(&manifest_digest),
        "the report carries the manifest SHA-256"
    );
}

/// REQ: DEV-924
#[test]
fn suite_version_flag_runs_the_embedded_catalogue_or_fails_with_suite_unavailable() {
    let (_root, env, _dir) = new_rust_project();
    let embedded = embedded_suite_version(&env);
    let current = run_in_project(&env, "conformance", &["--suite-version", embedded.as_str()]);
    assert_eq!(current.exit_code, 0, "{:?}", current.stdout_jsonl);
    let report = records_of(&current, "report");
    assert!(
        report
            .iter()
            .any(|row| row["suite_version"] == embedded.as_str()),
        "the run reports the requested suite version {embedded}"
    );
    let missing = run_in_project(&env, "conformance", &["--suite-version", "99.0"]);
    assert_ne!(missing.exit_code, 0, "{:?}", missing.stdout_jsonl);
    assert_eq!(
        error_code(&missing).as_deref(),
        Some("usage.suite-unavailable")
    );
}

/// REQ: DEV-925
#[test]
fn conformance_results_do_not_depend_on_home_timezone_locale_or_extra_environment() {
    let (root, base_env, _dir) = new_rust_project();
    let mut noisy_env = env(root.path());
    noisy_env
        .env
        .insert("HOME".to_owned(), "/home/another-user".to_owned());
    noisy_env
        .env
        .insert("TZ".to_owned(), "Asia/Tokyo".to_owned());
    noisy_env
        .env
        .insert("LANG".to_owned(), "fr_FR.UTF-8".to_owned());
    noisy_env
        .env
        .insert("LC_ALL".to_owned(), "fr_FR.UTF-8".to_owned());
    noisy_env
        .env
        .insert("ILIUM_TEST_EXTRA".to_owned(), "1".to_owned());
    let base_path = root.path().join("base.json");
    let noisy_path = root.path().join("noisy.json");
    let base = run_in_project(&base_env, "conformance", &["--report", utf8(&base_path)]);
    assert_eq!(base.exit_code, 0, "{:?}", base.stdout_jsonl);
    let noisy = run_in_project(&noisy_env, "conformance", &["--report", utf8(&noisy_path)]);
    assert_eq!(noisy.exit_code, 0, "{:?}", noisy.stdout_jsonl);
    assert_eq!(
        std::fs::read(&base_path).expect("base report"),
        std::fs::read(&noisy_path).expect("noisy report"),
        "the case results are the same on any machine"
    );
}
