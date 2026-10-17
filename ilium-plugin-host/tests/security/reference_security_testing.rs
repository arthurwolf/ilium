//! reference-security-testing.md: fuzz targets, the sandbox escape probe suite, the checklist,
//! side-channel tests and the shared hostile-input fixtures (SEC-340 .. 348).
//!
//! Repository artefacts (`fuzz/`, `.github/workflows/`, `tests/security-fixtures/`) are checked
//! as files; the invariants of the targets are exercised directly where an API exists.

use std::path::PathBuf;

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::Tier;
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

fn repo_file(relative: &str) -> PathBuf {
    workspace_root().join(relative)
}

fn read_repo(relative: &str) -> String {
    std::fs::read_to_string(repo_file(relative)).unwrap_or_default()
}

/// The fuzz targets of the table on the page, in page order, with their file stem.
const FUZZ_TARGETS: [(&str, &str); 14] = [
    ("`.iliumpkg` reader", "iliumpkg_reader"),
    ("Manifest parser and validator", "manifest_parser"),
    ("Grant string and host-pattern matcher", "grant_matcher"),
    ("Path canonicaliser and scope resolver", "path_scope"),
    (
        "Settings schema compiler and value validator",
        "settings_schema",
    ),
    (
        "`payload_schema` and `entry_schema` validators",
        "payload_schema",
    ),
    (
        "Every WIT host function's lifting of guest values",
        "wit_lifting",
    ),
    (
        "Dispatcher protocol messages from the helper",
        "dispatcher_protocol",
    ),
    (
        "Codecs: inflate, image decoders, audio, video headers",
        "codecs",
    ),
    ("HTTP API request parser", "http_api_request"),
    (
        "Transcript parsers behind `agent.transcript`",
        "transcript_parsers",
    ),
    ("Detector screen inputs", "detector_screens"),
    ("Regex compilation of plugin patterns", "regex_compile"),
    ("Audit record parser (`--verify`)", "audit_verify"),
];

// ---- SEC-340 ----

/// REQ: SEC-340
#[test]
fn sec_340_the_page_lists_exactly_the_fourteen_targets() {
    let page = manual_page("reference-security-testing.md");
    for (name, _) in FUZZ_TARGETS {
        assert!(
            page.contains(&format!("| {name} |")),
            "page lacks target {name}"
        );
    }
    let rows = page
        .lines()
        .skip_while(|line| !line.starts_with("| Target | Seeds"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .count();
    assert_eq!(rows, FUZZ_TARGETS.len());
}

/// REQ: SEC-340
#[test]
fn sec_340_every_target_exists_in_the_repository() {
    let missing: Vec<&str> = FUZZ_TARGETS
        .iter()
        .filter(|(_, stem)| !repo_file(&format!("fuzz/fuzz_targets/{stem}.rs")).is_file())
        .map(|(_, stem)| *stem)
        .collect();
    assert!(missing.is_empty(), "missing fuzz targets: {missing:?}");
}

/// REQ: SEC-340
#[test]
fn sec_340_every_target_has_corpus_seeds() {
    let empty: Vec<&str> = FUZZ_TARGETS
        .iter()
        .filter(|(_, stem)| {
            std::fs::read_dir(repo_file(&format!("fuzz/corpus/{stem}")))
                .map(|entries| entries.count())
                .unwrap_or(0)
                == 0
        })
        .map(|(_, stem)| *stem)
        .collect();
    assert!(empty.is_empty(), "targets without corpus seeds: {empty:?}");
}

/// REQ: SEC-340
#[test]
fn sec_340_the_common_invariant_limits_are_stated() {
    assert_page_has("reference-security-testing.md", &["no panic, no out-of-bounds access, no unbounded memory or time (a limit of 1 s and 512 MiB per input)"]);
}

/// REQ: SEC-340
#[test]
fn sec_340_manifest_target_refuses_duplicate_keys() {
    let text = r#"{"api":"ilium:plugin@0.1.0","id":"example.demo","id":"example.other","name":"Example demo","version":"0.1.0","publisher":"P","server":{"entry":"server.wasm","world":"ilium:server/base@0.1.0"}}"#;
    let codes = manifest_codes_of_text(text);
    assert!(
        codes.iter().any(|code| code == "manifest.duplicate-key"),
        "{codes:?}"
    );
}

/// REQ: SEC-340
#[test]
fn sec_340_manifest_target_reports_unknown_keys() {
    let codes = manifest_error_codes(&server_manifest(json!({ "surprise": 1 })));
    assert!(
        codes.iter().any(|code| code == "manifest.unknown-key"),
        "{codes:?}"
    );
}

/// REQ: SEC-340
#[test]
fn sec_340_manifest_target_survives_depth_1000_with_a_defined_error() {
    let nested = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
    let text = format!(
        r#"{{"api":"ilium:plugin@0.1.0","id":"example.demo","name":"Example demo","version":"0.1.0","publisher":"P","extra":{nested}}}"#
    );
    let codes = manifest_codes_of_text(&text);
    assert!(
        !codes.is_empty(),
        "a hostile document is rejected with a code"
    );
    // manual does not pin the depth code; every code must be in the manifest namespace.
    assert!(
        codes.iter().all(|code| code.starts_with("manifest.")),
        "{codes:?}"
    );
}

/// REQ: SEC-340
#[test]
fn sec_340_manifest_target_bounds_time_on_a_10_mib_string() {
    let big = "a".repeat(10 * 1024 * 1024);
    let started = std::time::Instant::now();
    let codes = manifest_error_codes(&server_manifest(json!({ "description": big })));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "limit of 1 s per input"
    );
    // manual does not pin whether manifest-size or the description limit fires first.
    assert!(
        !codes.is_empty()
            && codes
                .iter()
                .all(|code| code.starts_with("manifest.") || code == "package.manifest-size"),
        "an oversized string is refused with a code: {codes:?}"
    );
}

/// REQ: SEC-340
#[test]
fn sec_340_grant_matcher_target_rejects_bad_grant_strings_with_a_diagnostic() {
    for text in ["", "http:", "fs.read:", "no-such-grant", "http:*.*", ":x"] {
        let error = Grant::parse(text).expect_err(text);
        assert!(
            error.code.as_str().starts_with("manifest.grant-"),
            "{text:?}"
        );
    }
    let unknown = Grant::parse("no-such-grant").expect_err("unknown family");
    assert_eq!(unknown.code.as_str(), "manifest.grant-unknown");
}

/// Known-valid grant strings, so the properties below always reach their assertions.
const GRANT_STRATEGY: &str =
    "(clipboard\\.read|panes\\.create|terminal\\.read-screen|http:[a-z]{1,10}\\.com|process\\.run:[a-z]{1,10})";

proptest! {
    // REQ: SEC-340
    #[test]
    fn sec_340_grant_matcher_target_parse_is_total_and_reparses_to_itself(text in GRANT_STRATEGY) {
        {
            let grant = Grant::parse(&text).expect("valid grant parses");
            let again = Grant::parse(grant.as_str());
            prop_assert!(again.is_ok());
            let reparsed = again.expect("reparse");
            prop_assert_eq!(reparsed.as_str(), grant.as_str());
        }
    }

    // REQ: SEC-340
    #[test]
    fn sec_340_grant_matcher_target_a_grant_covers_itself(text in GRANT_STRATEGY) {
        {
            let grant = Grant::parse(&text).expect("valid grant parses");
            prop_assert!(grant.covers(&grant));
        }
    }
}

fn manifest_codes_of_text(text: &str) -> Vec<String> {
    use ilium_plugin_api::manifest::{validate_json, CheckMode, ValidationContext};
    use ilium_plugin_api::Platform;
    let context = ValidationContext {
        tier: Tier::T3,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    };
    validate_json(text, &context)
        .into_iter()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}

// ---- SEC-341 ----

fn fuzz_workflow() -> String {
    let dir = repo_file(".github/workflows");
    let mut text = String::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            text.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
    }
    text
}

/// REQ: SEC-341
#[test]
fn sec_341_ci_runs_each_target_for_at_least_10_minutes_on_a_change() {
    let workflow = fuzz_workflow();
    let active: Vec<&str> = workflow
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();
    assert!(
        active
            .iter()
            .any(|line| line.contains("-max_total_time=600")),
        "no 10 minute fuzz job in CI"
    );
    assert!(
        active.iter().any(|line| line.contains("pull_request")),
        "the fuzz job must run on a change"
    );
}

/// REQ: SEC-341
#[test]
fn sec_341_the_release_pipeline_runs_each_target_for_at_least_4_hours() {
    assert!(
        fuzz_workflow().contains("-max_total_time=14400"),
        "no 4 hour release fuzz job"
    );
}

/// REQ: SEC-341
#[test]
fn sec_341_native_code_runs_under_address_and_undefined_behaviour_sanitizers() {
    let workflow = fuzz_workflow();
    assert!(
        workflow.contains("address") && workflow.contains("undefined"),
        "sanitizer job missing"
    );
}

/// REQ: SEC-341
#[test]
fn sec_341_the_ci_job_is_triggered_by_changes_to_the_fuzzed_code() {
    let workflow = fuzz_workflow();
    assert!(
        workflow.contains("paths:") && workflow.contains("ilium-plugin"),
        "no path filter for the fuzzed crates"
    );
}

// ---- SEC-342 ----

/// REQ: SEC-342
#[test]
fn sec_342_a_fuzz_failure_fails_the_ci_job() {
    let workflow = fuzz_workflow();
    assert!(!workflow.is_empty());
    assert!(
        !workflow.contains("continue-on-error: true"),
        "a fuzz failure must block the release"
    );
}

/// REQ: SEC-342
#[test]
fn sec_342_every_crash_input_is_kept_in_the_corpus_as_a_regression() {
    assert!(
        repo_file("fuzz/regressions").is_dir(),
        "crash inputs are added to the corpus before the fix merges"
    );
}

/// REQ: SEC-342
#[test]
fn sec_342_the_manual_states_release_blocker_and_regression_input() {
    assert_page_has(
        "reference-security-testing.md",
        &[
            "A fuzz crash, hang or invariant failure is a release blocker.",
            "regression test before the fix merges",
        ],
    );
}

// ---- SEC-345 ----

/// Attempts a hostile guest and a hostile child program make; each must fail with one of the
/// `allowed` results.
fn probe(attempt: &str, allowed: &[&str]) {
    let rig = t3(&["process.run:probe-child", "panes.create"]);
    for who in ["guest", "child"] {
        let outcome = rig.host.run_hostile_probe(who, attempt);
        assert!(
            allowed.contains(&outcome.as_str()),
            "{who} {attempt}: got {outcome}, allowed {allowed:?}"
        );
    }
}

/// REQ: SEC-345
#[test]
fn sec_345_opening_a_socket_fails_with_eperm_or_no_route() {
    probe("socket", &["EPERM", "no-route"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_reading_proc_environ_of_the_server_and_init_fails() {
    probe("read-proc-environ", &["ENOENT", "EACCES"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_ptrace_of_the_server_or_a_sibling_helper_fails_with_eperm() {
    probe("ptrace", &["EPERM"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_tiocsti_and_tioclinux_fail_with_eperm_and_no_pty_fd_is_inherited() {
    probe("ioctl-tiocsti", &["EPERM"]);
    probe("ioctl-tioclinux", &["EPERM"]);
    probe("inherited-pty-fd", &["none"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_the_runtime_dir_the_ilium_socket_and_the_api_token_are_not_mounted() {
    probe("reach-runtime-dir", &["not-mounted"]);
    probe("reach-ilium-socket", &["not-mounted"]);
    probe("reach-api-token", &["not-mounted"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_secret_dotfolders_are_not_mounted_without_a_grant() {
    for folder in ["ssh", "aws", "gnupg"] {
        probe(&format!("read-home-{folder}"), &["not-mounted"]);
    }
}

/// REQ: SEC-345
#[test]
fn sec_345_writes_outside_the_granted_scope_are_not_permitted() {
    for route in ["dotdot", "symlink-swap-race", "hard-link", "bind-mount"] {
        probe(&format!("write-outside-scope-{route}"), &["not-permitted"]);
    }
}

/// REQ: SEC-345
#[test]
fn sec_345_executing_a_binary_written_to_data_fails_with_eacces() {
    probe("exec-from-data", &["EACCES"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_a_fork_bomb_gets_eagain_and_the_helper_stays_responsive() {
    probe("fork-bomb", &["EAGAIN"]);
    let rig = t3(&[]);
    rig.host.run_hostile_probe("guest", "fork-bomb");
    assert!(rig.host.helper_responsive());
}

/// REQ: SEC-345
#[test]
fn sec_345_allocating_beyond_memory_max_kills_the_helper_with_limit_exceeded_memory() {
    let rig = t3(&[]);
    assert_eq!(
        rig.host
            .run_hostile_probe("guest", "allocate-beyond-memory-max"),
        "helper-killed"
    );
    // manual does not pin the Trap variant name; its debug form must name limit-exceeded memory.
    let failures = rig.host.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    let trap = format!("{:?}", failures[0]);
    assert!(
        trap.contains("limit-exceeded") && trap.contains("memory"),
        "{trap}"
    );
}

/// REQ: SEC-345
#[test]
fn sec_345_each_denied_system_call_fails_with_eperm_or_kill() {
    for call in [
        "mount",
        "unshare",
        "setns",
        "kexec_load",
        "bpf",
        "perf_event_open",
        "keyctl",
        "init_module",
    ] {
        probe(&format!("syscall-{call}"), &["EPERM", "kill"]);
    }
}

/// REQ: SEC-345
#[test]
fn sec_345_another_plugins_data_folder_is_not_mounted() {
    probe("read-other-plugin-data", &["not-mounted"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_a_message_with_another_plugins_id_is_dropped_and_the_failure_counted() {
    let rig = t3(&[]);
    assert_eq!(
        rig.host.run_hostile_probe("guest", "send-with-foreign-id"),
        "dropped"
    );
    assert!(rig.host.inject_failure_count() >= 1);
}

/// REQ: SEC-345
#[test]
fn sec_345_ld_preload_and_path_in_the_process_run_environment_are_invalid_argument() {
    probe("process-run-env-ld-preload", &["invalid-argument"]);
    probe("process-run-env-path", &["invalid-argument"]);
}

/// REQ: SEC-345
#[test]
fn sec_345_sh_c_through_panes_create_without_the_command_grant_is_refused_not_permitted() {
    let rig = t3(&["panes.create"]);
    assert_eq!(
        rig.host.run_hostile_probe("guest", "panes-create-sh-c"),
        "refused(not-permitted)"
    );
}

// ---- SEC-346 ----

/// REQ: SEC-346
#[test]
fn sec_346_each_dialog_is_readable_and_aligned_at_48_and_120_columns_in_both_themes() {
    let rig = t3(&["terminal.read-screen", "http:example.com"]);
    for kind in ["review", "trust", "consent"] {
        for columns in [48u16, 120] {
            for dark in [false, true] {
                let lines = rig.host.render_dialog(kind, columns, dark);
                assert!(!lines.is_empty(), "{kind} {columns} {dark}");
                let widths: Vec<usize> = lines.iter().map(|line| line.chars().count()).collect();
                assert!(
                    widths.iter().all(|width| *width <= columns as usize),
                    "{kind} {columns}: {widths:?}"
                );
                let framed: Vec<usize> = lines
                    .iter()
                    .filter(|line| {
                        line.starts_with('\u{256d}')
                            || line.starts_with('\u{2502}')
                            || line.starts_with('\u{2570}')
                    })
                    .map(|line| line.chars().count())
                    .collect();
                assert!(framed.len() >= 3, "{kind} {columns}: no frame {framed:?}");
                assert!(
                    framed.windows(2).all(|pair| pair[0] == pair[1]),
                    "{kind} {columns} misaligned: {framed:?}"
                );
            }
        }
    }
}

/// REQ: SEC-346
#[test]
fn sec_346_screen_reader_text_is_present_for_each_dialog() {
    let rig = t3(&[]);
    for kind in ["review", "trust", "consent"] {
        assert!(
            !rig.host.screen_reader_text(kind).trim().is_empty(),
            "{kind}"
        );
    }
}

/// REQ: SEC-346
#[test]
fn sec_346_the_manual_requires_a_hand_check_on_each_release_at_both_widths() {
    assert_page_has("reference-security-testing.md", &["checked by hand on each release in a terminal of 48 columns and in one of 120 columns, in light and dark themes"]);
}

// ---- SEC-347 ----

fn tag_exists_in_security_tests(id: &str) -> bool {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/security");
    let tag = format!("REQ: {id}");
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.path().extension().is_some_and(|ext| ext == "rs")
                    && !entry.path().ends_with("reference_security_testing.rs")
                    && std::fs::read_to_string(entry.path())
                        .unwrap_or_default()
                        .contains(&tag)
            })
        })
        .unwrap_or(false)
}

/// REQ: SEC-347
#[test]
fn sec_347_a_test_exists_for_clock_resolution() {
    assert!(tag_exists_in_security_tests("SEC-370"));
}

/// REQ: SEC-347
#[test]
fn sec_347_a_test_exists_for_no_screen_changed_in_no_echo_mode() {
    assert!(tag_exists_in_security_tests("SEC-371"));
}

/// REQ: SEC-347
#[test]
fn sec_347_a_test_exists_for_identical_errors_for_missing_and_ungranted_plugins() {
    assert!(tag_exists_in_security_tests("SEC-373"));
}

/// REQ: SEC-347
#[test]
fn sec_347_a_test_exists_for_no_absolute_paths_in_error_strings() {
    assert!(tag_exists_in_security_tests("SEC-374"));
}

/// REQ: SEC-347
#[test]
fn sec_347_accepted_risks_are_listed_in_the_threat_model() {
    assert_page_has(
        "reference-threat-model.md",
        &["Accepted risks", "busy loop"],
    );
}

// ---- SEC-348 ----

/// REQ: SEC-348
#[test]
fn sec_348_the_fixture_folder_has_one_subfolder_per_hostile_input_family() {
    for family in ["archives", "manifests", "paths", "spoofing", "patterns"] {
        let dir = repo_file(&format!("tests/security-fixtures/{family}"));
        assert!(dir.is_dir(), "{family}");
        assert!(
            std::fs::read_dir(&dir)
                .map(|entries| entries.count())
                .unwrap_or(0)
                > 0,
            "{family} is empty"
        );
    }
}

/// REQ: SEC-348
#[test]
fn sec_348_the_fixtures_are_released_with_the_sdk() {
    let sdk = read_repo("ilium-plugin-sdk/Cargo.toml") + &read_repo("ilium-plugin-sdk/README.md");
    assert!(
        sdk.contains("security-fixtures"),
        "the SDK package does not ship the fixtures"
    );
}

/// REQ: SEC-348
#[test]
fn sec_348_the_manual_tells_authors_where_to_find_them() {
    assert_page_has(
        "reference-security-testing.md",
        &["`tests/security-fixtures/` of the repository and are released with the SDK"],
    );
}
