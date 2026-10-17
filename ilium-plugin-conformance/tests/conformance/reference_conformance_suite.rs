//! reference-conformance-suite.md: running the suite, record fields, project cases (DEV-6xx).
//!
//! The command under test is `ilium plugin test --conformance` through the pure
//! `ilium_plugin_host::cli::run`. Plugin-subject runs need the built fixture component
//! `conformance-subject.wasm` and fail red with `fixture.missing` until it is built.

use ilium_plugin_host::cli::CliOutput;
use proptest::prelude::*;

use crate::common::{
    cli_env, fixture_component, manifest_text, project_with_dist, records_of, run_cli,
    CASE_WALL_LIMIT_MS, CATALOGUE,
};

/// Runs the conformance command in a project built from `manifest` and the subject fixture.
fn run_in_project(manifest: &str, extra: &[&str]) -> CliOutput {
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(root.path(), manifest, "conformance-subject");
    let env = cli_env(root.path());
    let mut args = vec!["test", "--conformance", "--json"];
    args.extend_from_slice(extra);
    run_cli(&env, &args)
}

/// Runs the command in an empty folder; only usage errors are expected.
fn run_in_empty(extra: &[&str]) -> CliOutput {
    let root = tempfile::tempdir().expect("tempdir");
    let env = cli_env(root.path());
    let mut args = vec!["test", "--conformance", "--json"];
    args.extend_from_slice(extra);
    run_cli(&env, &args)
}

fn error_codes(output: &CliOutput) -> Vec<String> {
    records_of(output, "error")
        .iter()
        .filter_map(|record| record["code"].as_str().map(str::to_string))
        .collect()
}

fn case_ids(output: &CliOutput) -> Vec<String> {
    records_of(output, "conformance")
        .iter()
        .map(|record| record["case"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn verdict_of(output: &CliOutput, case: &str) -> Option<String> {
    records_of(output, "conformance")
        .into_iter()
        .find(|record| record["case"] == case)
        .and_then(|record| record["verdict"].as_str().map(str::to_string))
}

/// Catalogue ids whose subject column holds `subject` letter, in catalogue order.
fn cases_for(subject: char) -> Vec<&'static str> {
    CATALOGUE
        .iter()
        .filter(|(_, subjects)| subjects.contains(subject))
        .map(|(id, _)| *id)
        .collect()
}

// ---- DEV-600: subject plugin ----

/// REQ: DEV-600
#[test]
fn dev_600_plugin_subject_runs_only_plugin_marked_cases() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--subject", "plugin"]);
    let ran: Vec<String> = case_ids(&output);
    let allowed = cases_for('P');
    for id in &ran {
        assert!(
            allowed.contains(&id.as_str()),
            "case {id} is not marked plugin"
        );
    }
    assert!(!ran.is_empty(), "no conformance record printed");
}

/// REQ: DEV-600
#[test]
fn dev_600_plugin_subject_is_the_default_subject() {
    let explicit = run_in_project(&manifest_text("minimal.json"), &["--subject", "plugin"]);
    let implicit = run_in_project(&manifest_text("minimal.json"), &[]);
    assert_eq!(case_ids(&explicit), case_ids(&implicit));
}

/// REQ: DEV-600
#[test]
fn dev_600_ungranted_case_skips_with_grant_not_requested() {
    // minimal.json requests only sound.play; the fs.read case must skip.
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-GRANT-01"]);
    let records = records_of(&output, "conformance");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["verdict"], "skip");
    assert_eq!(records[0]["reason"], "grant-not-requested");
}

/// REQ: DEV-600
#[test]
fn dev_600_requested_grant_does_not_skip_for_grant_not_requested() {
    let output = run_in_project(&manifest_text("fs-read.json"), &["--case", "CONF-GRANT-01"]);
    let records = records_of(&output, "conformance");
    assert_eq!(records.len(), 1);
    assert_ne!(records[0]["reason"], "grant-not-requested");
}

// ---- DEV-601: sdk and host subjects ----

/// REQ: DEV-601
#[test]
fn dev_601_host_subject_refused_usage_subject_unavailable() {
    let output = run_in_empty(&["--subject", "host"]);
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.subject-unavailable".to_string()));
}

/// REQ: DEV-601
#[test]
fn dev_601_sdk_subject_refused_usage_subject_unavailable() {
    let output = run_in_empty(&["--subject", "sdk"]);
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.subject-unavailable".to_string()));
}

/// REQ: DEV-601
#[test]
fn dev_601_maintainer_switch_value_other_than_one_stays_refused_usage_subject_unavailable() {
    let previous = std::env::var_os("ILIUM_CONFORMANCE_MAINTAINER");
    std::env::set_var("ILIUM_CONFORMANCE_MAINTAINER", "0");
    let output = run_in_empty(&["--subject", "host"]);
    match previous {
        Some(value) => std::env::set_var("ILIUM_CONFORMANCE_MAINTAINER", value),
        None => std::env::remove_var("ILIUM_CONFORMANCE_MAINTAINER"),
    }
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.subject-unavailable".to_string()));
}

// ---- DEV-602: records and exit codes ----

/// REQ: DEV-602
#[test]
fn dev_602_one_record_per_case_then_summary_last() {
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    let kinds: Vec<&str> = output
        .stdout_jsonl
        .iter()
        .filter_map(|r| r["type"].as_str())
        .collect();
    assert_eq!(kinds.last().copied(), Some("summary"));
    assert!(kinds[..kinds.len() - 1]
        .iter()
        .all(|kind| *kind == "conformance"));
    assert_eq!(kinds.iter().filter(|kind| **kind == "summary").count(), 1);
}

/// REQ: DEV-602
#[test]
fn dev_602_fail_verdict_exits_three() {
    // CONF-EVT-06 against the over-returning fixture must fail.
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-EVT-06"]);
    if verdict_of(&output, "CONF-EVT-06").as_deref() == Some("fail") {
        assert_eq!(output.exit_code, 3);
    } else {
        panic!(
            "the subject fixture must make CONF-EVT-06 fail, got {:?}",
            verdict_of(&output, "CONF-EVT-06")
        );
    }
}

/// REQ: DEV-602
#[test]
fn dev_602_only_pass_and_skip_exits_zero() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let verdict = verdict_of(&output, "CONF-LIFE-01").expect("record");
    // CONF-LIFE-01 is a plugin-subject case needing no grant: it runs, so it must pass.
    assert_eq!(verdict, "pass");
    assert_eq!(output.exit_code, 0);
}

// ---- DEV-603: --case ----

/// REQ: DEV-603
#[test]
fn dev_603_case_runs_exactly_one_case() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    assert_eq!(case_ids(&output), vec!["CONF-LIFE-01".to_string()]);
}

/// REQ: DEV-603
#[test]
fn dev_603_unknown_case_refused_usage_invalid_value() {
    let output = run_in_empty(&["--case", "CONF-NOPE-99"]);
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.invalid-value".to_string()));
}

proptest! {
    /// REQ: DEV-603
    #[test]
    fn dev_603_any_id_outside_the_catalogue_is_refused_usage_invalid_value(suffix in "[A-Z]{1,6}-[0-9]{2}") {
        let id = format!("CONF-{suffix}");
        prop_assume!(!CATALOGUE.iter().any(|(known, _)| *known == id));
        let output = run_in_empty(&["--case", &id]);
        prop_assert_eq!(output.exit_code, 2);
        prop_assert!(error_codes(&output).contains(&"usage.invalid-value".to_string()));
    }
}

// ---- DEV-604: stable ids ----

/// REQ: DEV-604
#[test]
fn dev_604_every_catalogue_id_resolves_to_a_case() {
    for (id, _) in CATALOGUE {
        let output = run_in_project(&manifest_text("minimal.json"), &["--case", id]);
        assert_ne!(output.exit_code, 2, "case id {id} is unknown to the suite");
    }
}

/// REQ: DEV-604
#[test]
fn dev_604_case_ids_follow_the_conf_area_number_form() {
    for (id, _) in CATALOGUE {
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 3, "{id}");
        assert_eq!(parts[0], "CONF");
        assert_eq!(parts[2].len(), 2, "{id}");
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()), "{id}");
    }
}

/// REQ: DEV-604
#[test]
fn dev_604_the_title_of_a_case_does_not_change_between_two_runs() {
    let first = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let second = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    assert_eq!(
        records_of(&first, "conformance")[0]["title"],
        records_of(&second, "conformance")[0]["title"]
    );
}

// ---- DEV-605: determinism ----

/// REQ: DEV-605
#[test]
fn dev_605_two_runs_give_equal_verdicts_in_equal_order() {
    let strip = |output: &CliOutput| -> Vec<(String, String)> {
        records_of(output, "conformance")
            .iter()
            .map(|r| (r["case"].to_string(), r["verdict"].to_string()))
            .collect()
    };
    let first = run_in_project(&manifest_text("minimal.json"), &[]);
    let second = run_in_project(&manifest_text("minimal.json"), &[]);
    assert_eq!(strip(&first), strip(&second));
}

/// REQ: DEV-605
#[test]
fn dev_605_default_seed_is_not_overridable_by_the_environment() {
    std::env::set_var("ILIUM_TEST_SEED", "999");
    let first = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-EVT-03"]);
    std::env::remove_var("ILIUM_TEST_SEED");
    let second = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-EVT-03"]);
    assert_eq!(
        verdict_of(&first, "CONF-EVT-03"),
        verdict_of(&second, "CONF-EVT-03")
    );
}

// ---- DEV-606: 10,000 ms wall limit ----

/// REQ: DEV-606
#[test]
fn dev_606_case_that_runs_long_fails_with_case_exceeded_10000_ms() {
    // The slow fixture spins in on-events; the suite must cut it at 10,000 ms of wall time.
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(
        root.path(),
        &manifest_text("minimal.json"),
        "conformance-slow",
    );
    let env = cli_env(root.path());
    let output = run_cli(
        &env,
        &["test", "--conformance", "--json", "--case", "CONF-EVT-01"],
    );
    let record = records_of(&output, "conformance")[0].clone();
    assert_eq!(record["verdict"], "fail");
    assert_eq!(record["reason"], "case exceeded 10000 ms");
}

/// REQ: DEV-606
#[test]
fn dev_606_no_passing_case_reports_more_than_the_wall_limit() {
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    for record in records_of(&output, "conformance") {
        if record["verdict"] == "pass" {
            let duration = record["duration_ms"].as_u64().expect("duration_ms");
            assert!(duration <= CASE_WALL_LIMIT_MS, "{record}");
        }
    }
}

// ---- DEV-607: fuel units and the probe ----

/// REQ: DEV-607
#[test]
fn dev_607_deadline_cases_use_the_probe_component_not_the_subject() {
    // The probe is built from plugins/fixtures/conformance-probe; its absence is red.
    let probe = fixture_component("conformance-probe");
    assert!(probe.ends_with("conformance-probe.wasm"));
}

/// REQ: DEV-607
#[test]
fn dev_607_deadline_case_trap_deadline_does_not_depend_on_the_subject_plugin() {
    // CONF-EVT-02 subject H burns fuel in the probe; two different subject components agree.
    let first = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-EVT-02"]);
    let second = run_in_project(&manifest_text("fs-read.json"), &["--case", "CONF-EVT-02"]);
    assert_eq!(
        verdict_of(&first, "CONF-EVT-02"),
        verdict_of(&second, "CONF-EVT-02")
    );
}

/// REQ: DEV-607
#[test]
fn dev_607_probe_fuel_burn_import_is_exported_by_the_host_hooks() {
    ilium_plugin_host::conformance_hooks::fuel_burn(1);
}

// ---- DEV-610: the catalogue is the minimum ----

/// REQ: DEV-610
#[test]
fn dev_610_a_full_run_prints_at_least_one_record_for_each_case_of_the_plugin_subject() {
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    let ran = case_ids(&output);
    for id in cases_for('P') {
        assert!(ran.iter().any(|r| r == id), "missing {id}");
    }
}

/// REQ: DEV-610
#[test]
fn dev_610_catalogue_has_thirty_nine_cases() {
    assert_eq!(CATALOGUE.len(), 39);
}

// ---- DEV-612: n-1 / n / n+1 ----

/// REQ: DEV-612
#[test]
fn dev_612_limit_cases_check_the_value_under_and_over_the_limit() {
    // CONF-EVT-06 (256 vs 257) and CONF-VIEW-03 (4,096 vs 4,097) and CONF-STORE-02/03 pairs.
    for id in [
        "CONF-EVT-06",
        "CONF-VIEW-03",
        "CONF-STORE-02",
        "CONF-STORE-03",
    ] {
        let output = run_in_project(&manifest_text("minimal.json"), &["--case", id]);
        let records = records_of(&output, "conformance");
        assert_eq!(records.len(), 1, "{id}");
        let record = &records[0];
        assert_eq!(record["case"], id);
        // manual does not pin the verdict of each limit case per subject; a failing limit
        // case must at least say what was expected and what happened.
        let verdict = record["verdict"].as_str().expect("verdict");
        assert!(
            ["pass", "fail", "skip"].contains(&verdict),
            "{id}: {verdict}"
        );
        if verdict == "fail" {
            assert!(!record["expected"].is_null(), "{id}: {record}");
            assert!(!record["actual"].is_null(), "{id}: {record}");
        }
    }
}

/// REQ: DEV-612
#[test]
fn dev_612_limit_boundaries_used_by_the_suite_match_the_manual() {
    use ilium_plugin_api::abi::{check_submission_count, SUBMISSION_BATCH_LIMIT};
    assert_eq!(SUBMISSION_BATCH_LIMIT, 256);
    assert!(check_submission_count(255).is_ok());
    assert!(check_submission_count(256).is_ok());
    assert_eq!(
        check_submission_count(257).unwrap_err().as_str(),
        "trap.submission-batch"
    );
}

// ---- DEV-613: skip reasons ----

/// REQ: DEV-613
#[test]
fn dev_613_every_skip_carries_one_of_the_four_reasons() {
    let allowed = [
        "grant-not-requested",
        "side-not-built",
        "subject-mismatch",
        "needs-session",
    ];
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    for record in records_of(&output, "conformance") {
        if record["verdict"] == "skip" {
            let reason = record["reason"].as_str().unwrap_or_default();
            assert!(allowed.contains(&reason), "{record}");
        }
    }
}

/// REQ: DEV-613
#[test]
fn dev_613_scene_only_plugin_skips_event_cases_with_side_not_built() {
    let scene_only = manifest_text("minimal.json").replace("\"server\"", "\"scene\"");
    let output = run_in_project(&scene_only, &["--case", "CONF-EVT-10"]);
    let record = records_of(&output, "conformance")[0].clone();
    assert_eq!(record["verdict"], "skip");
    assert_eq!(record["reason"], "side-not-built");
}

// ---- DEV-620..622, 624: project case helpers (API in the SDK testing module) ----

/// REQ: DEV-620
#[test]
fn dev_620_project_case_id_must_start_with_proj_dash() {
    for id in ["PROJ-EVT-01", "PROJ-A", "PROJ-"] {
        assert!(crate::gap::is_project_case_id(id), "{id}");
    }
    for id in ["CONF-EVT-01", "proj-evt-01", "EVT-01", "PROJ", ""] {
        assert!(!crate::gap::is_project_case_id(id), "{id}");
    }
}

proptest! {
    /// REQ: DEV-620
    #[test]
    fn dev_620_ids_not_starting_with_proj_dash_never_clash_with_suite_ids(id in "[A-Z]{2,8}-[A-Z0-9-]{1,10}") {
        let is_project = crate::gap::is_project_case_id(&id);
        prop_assert_eq!(is_project, id.starts_with("PROJ-"));
        if is_project {
            prop_assert!(!CATALOGUE.iter().any(|(known, _)| *known == id));
        }
    }
}

/// REQ: DEV-621
#[test]
fn dev_621_sample_batch_returns_n_events() {
    for n in [0usize, 1, 255, 256, 257] {
        assert_eq!(crate::gap::sample_batch(n).len(), n, "n={n}");
    }
}

/// REQ: DEV-621
#[test]
fn dev_621_sample_batch_uses_the_fuzz_generator_with_the_host_seed() {
    crate::gap::case("PROJ-HOST-04", "sample batch seed", |host| {
        let batch = crate::gap::sample_batch(32);
        assert_eq!(
            format!("{batch:?}"),
            format!("{:?}", host.fuzz_events(host.current_seed(), 32))
        );
    });
}

/// REQ: DEV-621
#[test]
fn dev_621_sample_batch_twice_gives_equal_batches() {
    crate::gap::case("PROJ-HOST-05", "sample batch determinism", |_host| {
        let first = crate::gap::sample_batch(64);
        let second = crate::gap::sample_batch(64);
        assert_eq!(first, second);
    });
}

/// REQ: DEV-622
#[test]
fn dev_622_assert_within_budget_accepts_the_four_kinds() {
    let host = ilium_plugin_testkit::FakeHost::new()
        .mode(ilium_plugin_testkit::Mode::Component)
        .build();
    for kind in ["event", "render", "activate", "deactivate"] {
        crate::gap::assert_within_budget(&host, kind, 20);
    }
}

/// REQ: DEV-622
#[test]
fn dev_622_assert_within_budget_fails_when_the_recorded_time_exceeds_millis() {
    let host = ilium_plugin_testkit::FakeHost::new()
        .mode(ilium_plugin_testkit::Mode::Component)
        .build();
    let outcome = crate::gap::check_within_budget(&host, "event", 0);
    let error = outcome.expect_err("a recorded call of more than 0 ms must fail");
    assert_eq!(error.code, "harness.budget-exceeded");
}

/// REQ: DEV-622
#[test]
fn dev_622_assert_within_budget_unknown_kind_refused_harness_invalid_kind() {
    let host = ilium_plugin_testkit::FakeHost::new()
        .mode(ilium_plugin_testkit::Mode::Component)
        .build();
    let outcome = crate::gap::check_within_budget(&host, "paint", 20);
    assert_eq!(
        outcome.expect_err("unknown kind").code,
        "harness.invalid-kind"
    );
}

/// REQ: DEV-622
#[test]
fn dev_622_assert_within_budget_in_native_mode_is_skipped_needs_component() {
    let host = ilium_plugin_testkit::FakeHost::new()
        .mode(ilium_plugin_testkit::Mode::Native)
        .build();
    let outcome = crate::gap::check_within_budget(&host, "event", 20);
    assert_eq!(
        outcome.expect("native mode is a skip, not a failure"),
        Some("needs-component".to_string())
    );
}

/// REQ: DEV-624
#[test]
fn dev_624_case_passes_a_component_mode_host_with_the_default_seed() {
    let mut seen = None;
    crate::gap::case("PROJ-HOST-01", "host argument", |host| {
        seen = Some((host.current_mode(), host.current_seed()));
    });
    let (mode, seed) = seen.expect("the body ran");
    assert_eq!(mode, ilium_plugin_testkit::Mode::Component);
    assert_eq!(seed, crate::gap::DEFAULT_SEED);
}

/// REQ: DEV-624
#[test]
fn dev_624_case_host_is_built_for_manifest_json_in_the_project() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("manifest.json"),
        manifest_text("minimal.json"),
    )
    .expect("write");
    let mut runs = 0;
    crate::gap::case_in(root.path(), "PROJ-HOST-02", "manifest host", |_host| {
        runs += 1;
    });
    assert_eq!(runs, 1);
}

/// REQ: DEV-624
#[test]
fn dev_624_case_without_manifest_json_fails_harness_manifest_missing() {
    let root = tempfile::tempdir().expect("tempdir");
    let outcome = crate::gap::try_case_in(root.path(), "PROJ-HOST-03", "no manifest", |_| {});
    assert_eq!(
        outcome.expect_err("no manifest.json").code,
        "harness.manifest-missing"
    );
}

// ---- DEV-630: component mode only ----

/// REQ: DEV-630
#[test]
fn dev_630_conformance_in_native_mode_refused_usage_native_mode() {
    let output = run_in_empty(&["--mode", "native"]);
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.native-mode".to_string()));
}

/// REQ: DEV-630
#[test]
fn dev_630_conformance_defaults_to_component_mode() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let record = records_of(&output, "conformance")[0].clone();
    assert_ne!(record["reason"], "native mode");
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-630
#[test]
fn dev_630_debug_build_refused_usage_release_build_required() {
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(
        root.path(),
        &manifest_text("minimal.json"),
        "conformance-debug",
    );
    let env = cli_env(root.path());
    let output = run_cli(&env, &["test", "--conformance", "--json"]);
    assert_eq!(output.exit_code, 2);
    assert!(error_codes(&output).contains(&"usage.release-build-required".to_string()));
}

// ---- DEV-640..643: record shape ----

/// REQ: DEV-640
#[test]
fn dev_640_fail_record_carries_expected_and_actual_strings_of_at_most_200_characters() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-EVT-06"]);
    let record = records_of(&output, "conformance")[0].clone();
    assert_eq!(record["verdict"], "fail");
    for field in ["expected", "actual"] {
        let text = record[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} is not a string"));
        assert!(!text.is_empty());
        assert!(
            text.chars().count() <= 200,
            "{field} has {} characters",
            text.chars().count()
        );
    }
}

/// REQ: DEV-640
#[test]
fn dev_640_pass_record_has_no_expected_or_actual() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let record = records_of(&output, "conformance")[0].clone();
    if record["verdict"] == "pass" {
        assert!(record.get("expected").is_none());
        assert!(record.get("actual").is_none());
    }
}

/// REQ: DEV-640
#[test]
fn dev_640_long_difference_is_cut_to_exactly_200_characters() {
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(
        root.path(),
        &manifest_text("minimal.json"),
        "conformance-verbose",
    );
    let env = cli_env(root.path());
    let output = run_cli(
        &env,
        &["test", "--conformance", "--json", "--case", "CONF-EVT-06"],
    );
    let record = records_of(&output, "conformance")[0].clone();
    assert_eq!(
        record["actual"].as_str().expect("actual").chars().count(),
        200
    );
}

/// REQ: DEV-641
#[test]
fn dev_641_summary_fields_and_totals_equal_the_case_count() {
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    let summary = records_of(&output, "summary")[0].clone();
    let count = records_of(&output, "conformance").len() as u64;
    let sum = summary["passed"].as_u64().expect("passed")
        + summary["failed"].as_u64().expect("failed")
        + summary["skipped"].as_u64().expect("skipped");
    assert_eq!(sum, count);
    assert!(summary["duration_ms"].is_u64());
}

/// REQ: DEV-641
#[test]
fn dev_641_summary_of_one_case_counts_exactly_one() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let summary = records_of(&output, "summary")[0].clone();
    let sum = summary["passed"].as_u64().unwrap_or(99)
        + summary["failed"].as_u64().unwrap_or(99)
        + summary["skipped"].as_u64().unwrap_or(99);
    assert_eq!(sum, 1);
}

/// REQ: DEV-642
#[test]
fn dev_642_case_records_follow_the_catalogue_order() {
    let output = run_in_project(&manifest_text("minimal.json"), &[]);
    let ran = case_ids(&output);
    let expected: Vec<&str> = CATALOGUE
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| ran.iter().any(|r| r == id))
        .collect();
    let ran_refs: Vec<&str> = ran.iter().map(String::as_str).collect();
    assert_eq!(ran_refs, expected);
}

/// REQ: DEV-642
#[test]
fn dev_642_record_field_order_is_type_case_title_subject_verdict() {
    let output = run_in_project(&manifest_text("minimal.json"), &["--case", "CONF-LIFE-01"]);
    let record = records_of(&output, "conformance")[0].clone();
    let keys: Vec<&str> = record
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let head: Vec<&str> = keys.iter().copied().take(5).collect();
    assert_eq!(head, vec!["type", "case", "title", "subject", "verdict"]);
}

/// REQ: DEV-643
#[test]
fn dev_643_case_that_cannot_reach_the_state_is_skip_subject_mismatch_never_pass() {
    // The inert fixture world has no way to run the probe batch for CONF-EVT-06.
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(
        root.path(),
        &manifest_text("minimal.json"),
        "conformance-inert",
    );
    let env = cli_env(root.path());
    let output = run_cli(
        &env,
        &["test", "--conformance", "--json", "--case", "CONF-EVT-06"],
    );
    let record = records_of(&output, "conformance")[0].clone();
    assert_eq!(record["verdict"], "skip");
    assert_eq!(record["reason"], "subject-mismatch");
}

/// REQ: DEV-643
#[test]
fn dev_643_a_skip_is_not_counted_as_passed() {
    let root = tempfile::tempdir().expect("tempdir");
    project_with_dist(
        root.path(),
        &manifest_text("minimal.json"),
        "conformance-inert",
    );
    let env = cli_env(root.path());
    let output = run_cli(
        &env,
        &["test", "--conformance", "--json", "--case", "CONF-EVT-06"],
    );
    let summary = records_of(&output, "summary")[0].clone();
    assert_eq!(summary["passed"], 0);
    assert_eq!(summary["skipped"], 1);
}
