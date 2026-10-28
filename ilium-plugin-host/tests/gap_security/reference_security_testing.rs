//! Security testing page (SEC-704, SEC-705, SEC-733).

use crate::support::*;

/// REQ: SEC-704
#[test]
fn sec_704_release_checklist_fails_when_an_advisory_has_no_regression_fixture() {
    assert_manual_says(
        "reference-security-testing",
        "regression fixture to the hostile-plugin suite",
    );
    let rig = rig();
    rig.cli(&[
        "security",
        "advisory",
        "publish",
        "--id",
        "ILIUM-2026-0003",
        "--severity",
        "critical",
        "--cve",
        "CVE-2026-0003",
    ]);
    let output = rig.cli(&["release", "check", "--advisories"]);
    assert_ne!(
        output.exit_code, 0,
        "the release checklist must fail while ILIUM-2026-0003 has no fixture"
    );
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "release.fixture-missing"),
        "expected release.fixture-missing, got {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-705
#[test]
fn sec_705_fuzz_targets_and_repository_corpora_exist_for_every_parser() {
    let fuzz_root = repo_root().join("ilium-plugin-host/fuzz");
    for target in [
        "manifest_parser",
        "package_archive_reader",
        "signature_metadata_parser",
        "dispatcher_channel_decoder",
    ] {
        let corpus = fuzz_root.join("corpus").join(target);
        assert!(
            corpus.is_dir(),
            "fuzz corpus for {target} must be kept in the repository at {}",
            corpus.display()
        );
        let entries = std::fs::read_dir(&corpus)
            .unwrap_or_else(|error| panic!("read {}: {error}", corpus.display()))
            .count();
        assert!(entries > 0, "the corpus for {target} must not be empty");
    }
}

/// REQ: SEC-733
#[test]
fn sec_733_hostile_suite_has_one_fixture_per_attack_class_with_jsonl_pass_fail() {
    let rig = rig();
    let output = rig.cli(&["release", "hostile-suite", "--list"]);
    assert_eq!(
        output.exit_code, 0,
        "the hostile-plugin suite must be listable: {:?}",
        output.stdout_jsonl
    );
    let names: Vec<String> = output
        .stdout_jsonl
        .iter()
        .filter_map(|line| line["name"].as_str().map(str::to_owned))
        .collect();
    for attack in [
        "fork_bomb",
        "memory_bomb",
        "fd_exhaustion",
        "infinite_loop",
        "symlink_swap_toctou",
        "path_traversal",
        "environment_leak",
        "network_probe",
        "ptrace_sibling",
        "revoked_grant_abuse",
    ] {
        assert!(
            names.iter().any(|name| name.contains(attack)),
            "hostile suite lacks a fixture for {attack}; has {names:?}"
        );
    }
    let run = rig.cli(&["release", "hostile-suite", "--run", "--format", "jsonl"]);
    assert!(
        run.stdout_jsonl
            .iter()
            .all(|line| line["result"] == "pass" || line["result"] == "fail"),
        "every fixture must emit a pass/fail JSONL line"
    );
}

/// REQ: SEC-704
#[test]
fn sec_704_release_check_passes_when_no_advisory_is_open() {
    assert_manual_says(
        "reference-security-testing",
        "the release checklist MUST fail when the advisory id has no fixture",
    );
    let rig = rig();
    let output = rig.cli(&["release", "check", "--advisories"]);
    assert_eq!(output.exit_code, 0, "no advisory means no missing fixture");
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "release.fixture-missing"),
        "no fixture-missing refusal without advisories"
    );
}

/// REQ: SEC-705
#[test]
fn sec_705_each_fuzz_target_source_file_exists_beside_its_corpus() {
    assert_manual_says(
        "reference-security-testing",
        "The project MUST fuzz the manifest parser",
    );
    let fuzz_root = repo_root().join("ilium-plugin-host/fuzz");
    for target in [
        "manifest_parser",
        "package_archive_reader",
        "signature_metadata_parser",
        "dispatcher_channel_decoder",
    ] {
        let source = fuzz_root.join("fuzz_targets").join(format!("{target}.rs"));
        assert!(
            source.is_file(),
            "a fuzz target for {target} must exist at {}",
            source.display()
        );
    }
}

/// REQ: SEC-733
#[test]
fn sec_733_hostile_run_emits_at_least_one_jsonl_line_per_listed_fixture() {
    assert_manual_says(
        "reference-security-testing",
        "each fixture MUST be run on every release candidate with a pass/fail line in JSONL",
    );
    let rig = rig();
    let listing = rig.cli(&["release", "hostile-suite", "--list"]);
    let run = rig.cli(&["release", "hostile-suite", "--run", "--format", "jsonl"]);
    assert!(
        run.stdout_jsonl.len() >= listing.stdout_jsonl.len(),
        "each listed fixture produces a result line"
    );
}
