//! reference-plugin-cli.md: conventions, exit codes, the JSONL envelope (DEV-001..030).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use ilium_plugin_host::cli::MemorySession;
use serde_json::{json, Value};

use crate::common::*;

const ALLOWED_FIELDS: &[(&str, &[&str])] = &[
    ("progress", &["step", "message", "current", "total"]),
    ("warning", &["code", "message", "file", "line", "column"]),
    (
        "error",
        &[
            "code", "message", "file", "line", "column", "key_path", "hint",
        ],
    ),
    ("artifact", &["kind", "path", "sha256", "bytes"]),
    (
        "check",
        &[
            "rule", "severity", "message", "file", "line", "column", "key_path",
        ],
    ),
    (
        "lint",
        &[
            "rule",
            "severity",
            "message",
            "file",
            "line",
            "column",
            "key_path",
            "fix",
            "suppressed",
        ],
    ),
    (
        "test-case",
        &[
            "name",
            "fixture",
            "side",
            "status",
            "duration_ms",
            "diff",
            "reason",
        ],
    ),
    (
        "diff",
        &["subject", "expected", "actual", "first_difference"],
    ),
    (
        "submission",
        &["index", "correlation", "intent", "dedup_key"],
    ),
    ("outcome", &["correlation", "outcome", "code"]),
    (
        "reload",
        &[
            "plugin",
            "side",
            "version",
            "restored_bytes",
            "restore_dropped",
            "build_ms",
        ],
    ),
    ("log", &["at", "plugin", "side", "level", "message"]),
    (
        "timing",
        &[
            "plugin",
            "side",
            "export",
            "calls",
            "p50_ms",
            "p99_ms",
            "max_ms",
            "deadline_ms",
        ],
    ),
    ("trace", &["at", "plugin", "side", "kind", "detail"]),
    (
        "plugin",
        &[
            "id",
            "version",
            "tier",
            "state",
            "dev",
            "sides",
            "grants_granted",
            "grants_requested",
        ],
    ),
    ("grant", &["plugin", "grant", "risk", "state"]),
    ("inspect", &["section", "data"]),
    ("doctor-check", &["item", "status", "message", "fix"]),
    ("summary", &["duration_ms", "warnings", "errors"]),
    ("result", &["ok", "exit_code"]),
];

fn assert_fields_defined(records: &[Value]) {
    for record in records {
        let record_type = record["type"].as_str().expect("type");
        let allowed = ALLOWED_FIELDS
            .iter()
            .find(|(name, _)| *name == record_type)
            .expect("known record type")
            .1;
        for key in record.as_object().expect("object").keys() {
            assert!(
                key == "type" || key == "details" || allowed.contains(&key.as_str()),
                "{record_type} has undefined field {key}"
            );
        }
    }
}

/// REQ: DEV-001
#[test]
fn dev_001_check_without_dir_uses_the_working_directory() {
    let root = scratch();
    let mut env = env(root.path());
    let dir = server_project(&env, root.path());
    let explicit = run_in(&env, &dir, &["check", "--json"]);
    env.cwd = dir;
    let implicit = run(&env, &["check", "--json"]);
    assert_eq!(types(&implicit), types(&explicit));
    assert_eq!(implicit.exit_code, explicit.exit_code);
    // DEV-011: 3 exactly when a check finding of severity error exists, otherwise 0
    let has_error_finding = recs(&implicit, "check")
        .iter()
        .any(|r| r["severity"] == "error" && r["suppressed"] != true);
    assert_eq!(
        implicit.exit_code,
        if has_error_finding { 3 } else { 0 },
        "{:?}",
        implicit.stdout_jsonl
    );
    assert_eq!(
        recs(&implicit, "result")[0]["exit_code"],
        implicit.exit_code
    );
}

/// REQ: DEV-001
#[test]
fn dev_001_build_without_dir_writes_dist_in_the_working_directory() {
    let root = scratch();
    let mut env = env(root.path());
    let dir = server_project(&env, root.path());
    env.cwd = dir.clone();
    let output = run(&env, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert!(dir.join("dist/server.wasm").is_file());
}

/// REQ: DEV-001
#[test]
fn dev_001_dir_is_the_folder_with_the_manifest_not_cwd() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert!(dir.join("dist/server.wasm").is_file());
    assert!(
        !env.cwd.join("dist").exists(),
        "cwd is not the project when <dir> is given"
    );
}

/// REQ: DEV-002
#[test]
fn dev_002_cargo_toml_alone_means_rust() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert!(
        recs(&output, "progress")
            .iter()
            .any(|r| r["step"] == "compile"),
        "rust has a compile stage"
    );
}

/// REQ: DEV-002
#[test]
fn dev_002_package_json_alone_means_typescript() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "tsdemo", "server", "typescript");
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert!(
        recs(&output, "progress")
            .iter()
            .any(|r| r["step"] == "bundle"),
        "typescript has a bundle stage"
    );
}

/// REQ: DEV-002
#[test]
fn dev_002_both_files_fail_language_ambiguous() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::write(dir.join("package.json"), "{}").expect("write");
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(
        code(&output).as_deref(),
        Some("toolchain.language-ambiguous")
    );
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-002
#[test]
fn dev_002_neither_file_fails_language_unknown() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::remove_file(dir.join("Cargo.toml")).expect("remove");
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(code(&output).as_deref(), Some("toolchain.language-unknown"));
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-003
#[test]
fn dev_003_global_flags_are_accepted_by_a_command_that_ignores_them() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let cwd = root.path().to_string_lossy().into_owned();
    let output = run_in(
        &env,
        &dir,
        &[
            "lint",
            "--quiet",
            "--no-color",
            "--json",
            "--session",
            "other",
            "--cwd",
            &cwd,
        ],
    );
    assert_ne!(output.exit_code, 2, "no usage error for global flags");
    assert!(recs(&output, "error")
        .iter()
        .all(|r| !r["code"].as_str().unwrap_or("").starts_with("usage.")));
}

/// REQ: DEV-003
#[test]
fn dev_003_session_flag_is_ignored_by_a_command_that_never_contacts_a_session() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let with = run_in(&env, &dir, &["check", "--json", "--session", "nonexistent"]);
    let without = run_in(&env, &dir, &["check", "--json"]);
    assert_eq!(with.exit_code, without.exit_code);
    assert_eq!(types(&with), types(&without));
    assert_ne!(with.exit_code, 6, "session.unreachable must not be raised");
    assert!(recs(&with, "error")
        .iter()
        .all(|r| r["code"] != "session.unreachable"));
}

/// REQ: DEV-003
#[test]
fn dev_003_help_prints_usage_and_exits_zero() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["--help"]);
    assert_eq!(output.exit_code, 0);
    let first_line = output
        .stdout_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .expect("usage line");
    assert!(first_line.contains("ilium plugin"), "{first_line}");
    for flag in [
        "--session",
        "--cwd",
        "--quiet",
        "--no-color",
        "--json",
        "--help",
    ] {
        assert!(output.stdout_text.contains(flag), "usage lists {flag}");
    }
}

/// REQ: DEV-003
#[test]
fn dev_003_each_global_flag_is_accepted_by_build() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    for flag in [
        &["--quiet"][..],
        &["--no-color"],
        &["--json"],
        &["--session", "x"],
    ] {
        let mut args = vec!["build"];
        args.extend_from_slice(flag);
        let output = run_in(&env, &dir, &args);
        assert_eq!(output.exit_code, 0, "flag {flag:?}");
    }
}

/// REQ: DEV-004
#[test]
fn dev_004_default_session_is_found_without_flags() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list", "--json"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-004
#[test]
fn dev_004_unknown_session_name_fails_session_unreachable() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list", "--json", "--session", "nope"]);
    assert_eq!(code(&output).as_deref(), Some("session.unreachable"));
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-004
#[test]
fn dev_004_no_session_at_all_fails_session_unreachable_and_exits_six() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["list", "--json"]);
    assert_eq!(code(&output).as_deref(), Some("session.unreachable"));
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-004
#[test]
fn dev_004_a_command_never_starts_a_session() {
    let root = scratch();
    let session = MemorySession::unreachable();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["enable", "acme.demo"]);
    assert_eq!(output.exit_code, 6);
    assert_eq!(session.sessions_started(), 0);
}

/// REQ: DEV-004
#[test]
fn dev_004_cwd_flag_names_the_project_of_the_session() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let other = root.path().join("elsewhere");
    std::fs::create_dir_all(&other).expect("create");
    let output = run(
        &env,
        &["list", "--json", "--cwd", other.to_str().expect("utf8")],
    );
    assert_eq!(
        output.exit_code, 6,
        "the session lives in the project of the working directory, not in `elsewhere`"
    );
}

/// REQ: DEV-005
#[test]
fn dev_005_offline_commands_need_no_session_network_or_config() {
    let root = scratch();
    let fetcher = Arc::new(CountingFetcher::default());
    let mut env = env(root.path());
    env.fetcher = fetcher.clone();
    let dir = server_project(&env, root.path());
    assert_eq!(run_in(&env, &dir, &["check", "--json"]).exit_code, 0);
    assert_eq!(run_in(&env, &dir, &["lint", "--json"]).exit_code, 0);
    assert_eq!(run_in(&env, &dir, &["build"]).exit_code, 0);
    make_key(&env, root.path(), "acme");
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0, "no network");
    assert!(
        !env.config_dir.exists(),
        "no home configuration was needed or created"
    );
}

/// REQ: DEV-005
#[test]
fn dev_005_doctor_and_inspect_of_a_file_work_offline() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    assert_eq!(
        run(
            &env,
            &["inspect", package.to_str().expect("utf8"), "--json"]
        )
        .exit_code,
        0
    );
    let doctor = run(&env, &["doctor", "--json"]);
    // DEV-811: 4 exactly when an item failed, otherwise 0
    let any_fail = recs(&doctor, "doctor-check")
        .iter()
        .any(|r| r["status"] == "fail");
    assert!(!recs(&doctor, "doctor-check").is_empty());
    assert_eq!(doctor.exit_code, if any_fail { 4 } else { 0 });
    assert_ne!(code(&doctor).as_deref(), Some("session.unreachable"));
}

/// REQ: DEV-006
#[test]
fn dev_006_unreachable_session_prints_error_summary_result() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["disable", "acme.demo"]);
    assert_eq!(types(&output), ["error", "summary", "result"]);
    assert_eq!(output.stdout_jsonl[0]["code"], "session.unreachable");
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-006
#[test]
fn dev_006_unreachable_session_is_not_retried() {
    let root = scratch();
    let session = MemorySession::unreachable();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["uninstall", "acme.demo"]);
    assert_eq!(output.exit_code, 6);
    assert_eq!(session.connect_attempts(), 1);
}

/// REQ: DEV-006
#[test]
fn dev_006_every_session_command_fails_the_same_way() {
    let root = scratch();
    let env = env(root.path());
    for args in [
        &["install", "x.iliumpkg", "--yes"][..],
        &["uninstall", "a.b"],
        &["enable", "a.b"],
        &["disable", "a.b"],
        &["grants", "a.b", "--json"],
        &["list", "--json"],
        &["logs", "a.b", "--json"],
        &["log-level", "a.b"],
    ] {
        let output = run(&env, args);
        assert_eq!(output.exit_code, 6, "{args:?}");
        assert_eq!(types(&output), ["error", "summary", "result"], "{args:?}");
    }
}

/// REQ: DEV-007
#[test]
fn dev_007_result_exit_code_equals_process_exit_code_on_success() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    for args in [&["build"][..], &["check", "--json"], &["lint", "--json"]] {
        let output = run_in(&env, &dir, args);
        assert_eq!(result(&output)["exit_code"], output.exit_code, "{args:?}");
    }
}

/// REQ: DEV-007
#[test]
fn dev_007_result_exit_code_equals_process_exit_code_on_failure() {
    let root = scratch();
    let env = env(root.path());
    let sixty_four_zeros = "0".repeat(64);
    let cases: Vec<(Vec<&str>, i32)> = vec![
        (vec!["new"], 2),
        (vec!["uninstall", "a.b"], 6),
        (vec!["trust", "remove", &sixty_four_zeros], 5),
    ];
    for (args, expected) in cases {
        let output = run(&env, &args);
        assert_eq!(output.exit_code, expected, "{args:?}");
        assert_eq!(result(&output)["exit_code"], expected, "{args:?}");
    }
}

/// REQ: DEV-007
#[test]
fn dev_007_exit_code_is_one_of_the_table() {
    let root = scratch();
    let env = env(root.path());
    // exact code per failure class: usage -> 2 (DEV-010), no session -> 6 (DEV-014)
    for (args, expected) in [(&["nonsense"][..], 2), (&["new"], 2), (&["list"], 6)] {
        assert_eq!(run(&env, args).exit_code, expected, "{args:?}");
    }
    // manual does not pin the exit code of these two in an empty environment: table membership
    let allowed = [0, 1, 2, 3, 4, 5, 6, 130];
    for args in [&["pack"][..], &["trust", "list", "--json"]] {
        assert!(allowed.contains(&run(&env, args).exit_code), "{args:?}");
    }
}

/// REQ: DEV-008
#[test]
fn dev_008_exit_zero_means_result_ok_true() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(result(&output)["ok"], true);
}

/// REQ: DEV-008
#[test]
fn dev_008_a_nonzero_exit_never_has_ok_true() {
    let root = scratch();
    let env = env(root.path());
    for args in [&["new"][..], &["pack"], &["list", "--json"]] {
        let output = run(&env, args);
        assert_ne!(output.exit_code, 0, "{args:?}");
        assert_eq!(result(&output)["ok"], false, "{args:?}");
    }
}

/// REQ: DEV-009
#[test]
fn dev_009_an_unspecific_failure_exits_one() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(code(&output).as_deref(), Some("pack.not-built"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-009
#[test]
fn dev_009_a_compile_failure_exits_one() {
    let root = scratch();
    let env = env(root.path()).with_builder(FakeBuilder::failing_at("compile"));
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-010
#[test]
fn dev_010_usage_error_exits_two_and_writes_no_file() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("fresh");
    let output = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8"),
            "--lang",
            "rust",
            "--template",
            "server",
        ],
    );
    assert_eq!(output.exit_code, 2);
    assert!(!dir.exists(), "nothing was written");
}

/// REQ: DEV-010
#[test]
fn dev_010_unknown_command_is_a_usage_error() {
    let root = scratch();
    let env = env(root.path());
    let before = tree_listing(root.path());
    let output = run(&env, &["frobnicate"]);
    assert_eq!(output.exit_code, 2);
    assert_eq!(tree_listing(root.path()), before);
}

/// REQ: DEV-010
#[test]
fn dev_010_conflicting_build_flags_exit_two_before_any_work() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--debug", "--release"]);
    assert_eq!(output.exit_code, 2);
    assert!(!dir.join("dist").exists());
}

/// REQ: DEV-011
#[test]
fn dev_011_check_error_finding_exits_three() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::write(dir.join("manifest.json"), "{ not json").expect("write");
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-011
#[test]
fn dev_011_warn_finding_under_plain_lint_returns_zero() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["description"] = json!("");
    write_json(&dir.join("manifest.json"), &manifest);
    let output = run_in(&env, &dir, &["lint", "--json"]);
    assert!(recs(&output, "lint")
        .iter()
        .any(|r| r["severity"] == "warn"));
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-011
#[test]
fn dev_011_lint_deny_warnings_with_a_finding_exits_three() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["description"] = json!("");
    write_json(&dir.join("manifest.json"), &manifest);
    let output = run_in(&env, &dir, &["lint", "--json", "--deny", "warnings"]);
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-011
#[test]
fn dev_011_lint_error_rule_exits_three_without_deny() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::write(
        dir.join("lint.toml"),
        "[[suppress]]\nrule = \"lint.description-missing\"\n",
    )
    .expect("write");
    let output = run_in(&env, &dir, &["lint", "--json"]);
    assert!(recs(&output, "lint")
        .iter()
        .any(|r| r["rule"] == "lint.suppression-reason-missing" && r["severity"] == "error"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-012
#[test]
fn dev_012_missing_toolchain_exits_four() {
    let root = scratch();
    let env = env(root.path()).with_probe(Arc::new(FakeProbe::without("rustc")));
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    // the probe lacks `rustc`: reference table row `toolchain.rustc-missing`
    assert_eq!(code(&output).as_deref(), Some("toolchain.rustc-missing"));
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-012
#[test]
fn dev_012_doctor_with_a_failing_item_exits_four() {
    let root = scratch();
    let env = env(root.path()).with_probe(Arc::new(FakeProbe::without("rustc")));
    let output = run(&env, &["doctor", "--json"]);
    assert!(recs(&output, "doctor-check")
        .iter()
        .any(|r| r["status"] == "fail"));
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-012
#[test]
fn dev_012_doctor_with_no_failing_item_does_not_exit_four() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["doctor", "--json"]);
    assert!(recs(&output, "doctor-check")
        .iter()
        .all(|r| r["status"] != "fail"));
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-013
#[test]
fn dev_013_uninstall_unknown_plugin_exits_five() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["uninstall", "acme.ghost"]);
    assert_eq!(code(&output).as_deref(), Some("plugin.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-013
#[test]
fn dev_013_trust_remove_unknown_key_exits_five() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["trust", "remove", &"a".repeat(64)]);
    assert_eq!(code(&output).as_deref(), Some("trust.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-013
#[test]
fn dev_013_inspect_unknown_id_exits_five() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["inspect", "acme.ghost", "--json"]);
    assert_eq!(code(&output).as_deref(), Some("inspect.id-unknown"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-014
#[test]
fn dev_014_session_unreachable_exits_six() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["list", "--json"]);
    assert_eq!(code(&output).as_deref(), Some("session.unreachable"));
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-014
#[test]
fn dev_014_session_not_found_also_exits_six() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let output = run(&env, &["list", "--json", "--session", "ghost"]);
    // reference table: `session.not-found` = the named session does not exist
    assert_eq!(code(&output).as_deref(), Some("session.not-found"));
    assert_eq!(output.exit_code, 6);
}

/// REQ: DEV-015
#[test]
fn dev_015_interrupt_exits_130_with_summary_and_result_not_ok() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let env = env.with_interrupt(raised_interrupt());
    let output = run_in(&env, &dir, &["build", "--watch"]);
    assert_eq!(output.exit_code, 130);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
    assert_eq!(result(&output)["ok"], false);
    assert_eq!(result(&output)["exit_code"], 130);
}

/// REQ: DEV-015
#[test]
fn dev_015_interrupt_leaves_no_half_written_file() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let env = env.with_interrupt(raised_interrupt());
    run_in(&env, &dir, &["build", "--watch"]);
    for path in tree_listing(&dir.join("dist")) {
        assert!(
            !path.ends_with(".tmp") && !path.contains(".tmp."),
            "left over {path}"
        );
    }
}

/// REQ: DEV-015
#[test]
fn dev_015_interrupt_during_pack_exits_130() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    let env = env.with_interrupt(raised_interrupt());
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(output.exit_code, 130);
    assert!(!dir.join("dist/acme.demo-0.1.0.iliumpkg").exists());
}

/// REQ: DEV-016
#[test]
fn dev_016_every_stdout_line_is_json_with_type_first() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    for line in output.stdout_raw.lines() {
        assert!(
            line.starts_with("{\"type\":"),
            "type is not the first key: {line}"
        );
        serde_json::from_str::<Value>(line).expect("line is json");
    }
    assert!(!output.stdout_raw.is_empty());
}

/// REQ: DEV-016
#[test]
fn dev_016_one_object_per_line_no_pretty_printing() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.stdout_raw.lines().count(), output.stdout_jsonl.len());
    assert!(output.stdout_raw.ends_with('\n'));
}

/// REQ: DEV-016
#[test]
fn dev_016_error_text_goes_to_standard_error_not_standard_output() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["trust", "remove", &"b".repeat(64)]);
    assert_eq!(code(&output).as_deref(), Some("trust.not-found"));
    // manual does not pin the human text; it only forbids non-JSON on stdout
    assert!(!output.stdout_raw.is_empty());
    for line in output.stdout_raw.lines() {
        serde_json::from_str::<Value>(line).expect("stdout holds only JSON lines");
    }
}

/// REQ: DEV-017
#[test]
fn dev_017_success_ends_with_summary_then_result() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
    assert_eq!(kinds.iter().filter(|k| *k == "summary").count(), 1);
    assert_eq!(kinds.iter().filter(|k| *k == "result").count(), 1);
}

/// REQ: DEV-017
#[test]
fn dev_017_failure_ends_with_summary_then_result() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["pack"]);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
    assert_eq!(recs(&output, "summary")[0]["errors"], 1);
}

/// REQ: DEV-017
#[test]
fn dev_017_usage_error_still_prints_summary_and_result() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["new"]);
    let kinds = types(&output);
    assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"]);
}

/// REQ: DEV-018
#[test]
fn dev_018_sizes_are_unsigned_integers_and_hashes_are_lowercase_hex() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    for artifact in recs(&output, "artifact") {
        assert!(artifact["bytes"].is_u64(), "{artifact}");
        assert!(
            is_sha256_hex(artifact["sha256"].as_str().expect("sha256")),
            "{artifact}"
        );
    }
    assert!(!recs(&output, "artifact").is_empty());
}

/// REQ: DEV-018
#[test]
fn dev_018_durations_are_unsigned_integer_milliseconds_with_ms_keys() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert!(recs(&output, "summary")[0]["duration_ms"].is_u64());
}

/// REQ: DEV-018
#[test]
fn dev_018_paths_use_slashes_and_are_relative_to_the_project() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    for artifact in recs(&output, "artifact") {
        let path = artifact["path"].as_str().expect("path");
        assert!(!path.contains('\\') && !path.starts_with('/'), "{path}");
    }
}

/// REQ: DEV-019
#[test]
fn dev_019_build_records_have_only_defined_fields() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    assert_fields_defined(&run_in(&env, &dir, &["build"]).stdout_jsonl);
}

/// REQ: DEV-019
#[test]
fn dev_019_check_and_lint_records_have_only_defined_fields() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["description"] = json!("");
    write_json(&dir.join("manifest.json"), &manifest);
    assert_fields_defined(&run_in(&env, &dir, &["check", "--json"]).stdout_jsonl);
    assert_fields_defined(&run_in(&env, &dir, &["lint", "--json"]).stdout_jsonl);
}

/// REQ: DEV-019
#[test]
fn dev_019_error_records_have_only_defined_fields() {
    let root = scratch();
    let env = env(root.path());
    assert_fields_defined(&run(&env, &["trust", "remove", &"c".repeat(64)]).stdout_jsonl);
    assert_fields_defined(&run(&env, &["new"]).stdout_jsonl);
}

/// REQ: DEV-020
#[test]
fn dev_020_quiet_suppresses_progress_records() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--quiet"]);
    assert!(recs(&output, "progress").is_empty());
}

/// REQ: DEV-020
#[test]
fn dev_020_quiet_keeps_every_other_record() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let loud = run_in(&env, &dir, &["build"]);
    let quiet = run_in(&env, &dir, &["build", "--quiet"]);
    let without_progress: Vec<String> = types(&loud)
        .into_iter()
        .filter(|t| t != "progress")
        .collect();
    assert_eq!(types(&quiet), without_progress);
}

/// REQ: DEV-020
#[test]
fn dev_020_quiet_keeps_errors() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["pack", "--quiet"]);
    assert_eq!(code(&output).as_deref(), Some("pack.not-built"));
}

/// REQ: DEV-021
#[test]
fn dev_021_progress_steps_are_from_the_fixed_set() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let allowed = [
        "toolchain",
        "compile",
        "bundle",
        "component",
        "api-version",
        "strip",
        "inventory",
        "pack",
        "sign",
        "check",
        "lint",
        "test",
        "install",
        "watch",
    ];
    let output = run_in(&env, &dir, &["build"]);
    assert!(!recs(&output, "progress").is_empty());
    for record in recs(&output, "progress") {
        assert!(
            allowed.contains(&record["step"].as_str().expect("step")),
            "{record}"
        );
    }
}

/// REQ: DEV-021
#[test]
fn dev_021_new_and_pack_steps_are_from_the_fixed_set() {
    let root = scratch();
    let env = env(root.path());
    let (dir, _) = packed_project(&env, root.path());
    let allowed = ["inventory", "pack", "toolchain", "check", "sign"];
    let output = run_in(&env, &dir, &["pack"]);
    for record in recs(&output, "progress") {
        assert!(
            allowed.contains(&record["step"].as_str().expect("step")),
            "{record}"
        );
    }
}

/// REQ: DEV-022
#[test]
fn dev_022_artifact_kinds_are_from_the_fixed_set() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    let allowed = [
        "component",
        "package",
        "source-map",
        "build-info",
        "key",
        "public-key",
        "signature",
        "replay",
    ];
    let mut outputs = vec![
        run_in(&env, &dir, &["build"]),
        run_in(&env, &dir, &["pack"]),
    ];
    outputs.push(run(
        &env,
        &[
            "sign",
            package.to_str().expect("utf8"),
            "--key",
            key.to_str().expect("utf8"),
        ],
    ));
    for output in outputs {
        for record in recs(&output, "artifact") {
            assert!(
                allowed.contains(&record["kind"].as_str().expect("kind")),
                "{record}"
            );
        }
    }
}

/// REQ: DEV-022
#[test]
fn dev_022_keygen_prints_key_and_public_key_artifacts() {
    let root = scratch();
    let env = env(root.path());
    let out = root.path().join("k");
    let output = run(
        &env,
        &[
            "keygen",
            "--publisher-id",
            "acme",
            "--publisher",
            "Acme",
            "--out",
            out.to_str().expect("utf8"),
        ],
    );
    assert_eq!(artifacts(&output, "key").len(), 1);
    assert_eq!(artifacts(&output, "public-key").len(), 1);
}

/// REQ: DEV-023
#[test]
fn dev_023_check_severity_is_error_or_warn() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["check", "--json"]);
    for record in recs(&output, "check") {
        assert!(
            record["severity"] == "error" || record["severity"] == "warn",
            "{record}"
        );
    }
}

/// REQ: DEV-023
#[test]
fn dev_023_lint_severity_may_also_be_info() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::remove_file(dir.join("README.md")).expect("remove readme");
    let output = run_in(&env, &dir, &["lint", "--json"]);
    let found: Vec<&Value> = recs(&output, "lint");
    assert!(found
        .iter()
        .any(|r| r["rule"] == "lint.readme-missing" && r["severity"] == "info"));
    for record in found {
        assert!(["error", "warn", "info"].contains(&record["severity"].as_str().expect("severity")));
    }
}

/// REQ: DEV-023
#[test]
fn dev_023_check_never_uses_info() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    std::fs::remove_file(dir.join("README.md")).expect("remove readme");
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .all(|r| r["severity"] != "info"));
}

/// REQ: DEV-024
#[test]
fn dev_024_test_case_status_is_pass_fail_trap_or_skipped() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["build"]);
    let output = run_in(&env, &dir, &["test"]);
    let cases = recs(&output, "test-case");
    assert!(!cases.is_empty());
    for record in cases {
        assert!(
            ["pass", "fail", "trap", "skipped"]
                .contains(&record["status"].as_str().expect("status")),
            "{record}"
        );
    }
}

/// REQ: DEV-024
#[test]
fn dev_024_doctor_check_status_is_ok_warn_or_fail() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["doctor", "--json"]);
    assert!(!recs(&output, "doctor-check").is_empty());
    for record in recs(&output, "doctor-check") {
        assert!(
            ["ok", "warn", "fail"].contains(&record["status"].as_str().expect("status")),
            "{record}"
        );
    }
}

/// REQ: DEV-025
#[test]
fn dev_025_test_case_side_is_server_client_scene_or_host() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["build"]);
    let output = run_in(&env, &dir, &["test"]);
    for record in recs(&output, "test-case") {
        assert!(
            ["server", "client", "scene", "host"].contains(&record["side"].as_str().expect("side")),
            "{record}"
        );
    }
}

/// REQ: DEV-025
#[test]
fn dev_025_host_marks_an_entry_the_host_wrote() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    session.add_log_line("acme.demo", "host", "info", "activated");
    let env = env_session(root.path(), &session);
    let output = run(&env, &["logs", "acme.demo", "--json", "--side", "host"]);
    let logs = recs(&output, "log");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["side"], "host");
}

/// REQ: DEV-026
#[test]
fn dev_026_a_jsonl_command_prints_jsonl_without_the_json_flag() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert!(output.stdout_jsonl.len() >= 2);
    assert!(output.stdout_text.is_empty() || output.stdout_text == output.stdout_raw);
}

/// REQ: DEV-026
#[test]
fn dev_026_json_flag_on_a_jsonl_command_has_no_effect() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let plain = run_in(&env, &dir, &["build"]);
    let flagged = run_in(&env, &dir, &["build", "--json"]);
    assert_eq!(types(&plain), types(&flagged));
    assert_eq!(flagged.exit_code, 0);
}

/// REQ: DEV-027
#[test]
fn dev_027_text_commands_print_text_by_default() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    for args in [&["check"][..], &["lint"]] {
        let output = run_in(&env, &dir, args);
        assert!(
            output.stdout_jsonl.is_empty(),
            "{args:?} printed JSONL by default"
        );
    }
    assert!(run(&env, &["trust", "list"]).stdout_jsonl.is_empty());
}

/// REQ: DEV-027
#[test]
fn dev_027_text_commands_end_with_summary_and_result_under_json() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    for args in [&["check", "--json"][..], &["lint", "--json"]] {
        let output = run_in(&env, &dir, args);
        let kinds = types(&output);
        assert_eq!(&kinds[kinds.len() - 2..], ["summary", "result"], "{args:?}");
    }
    let trust = run(&env, &["trust", "list", "--json"]);
    assert_eq!(
        &types(&trust)[types(&trust).len() - 2..],
        ["summary", "result"]
    );
}

/// REQ: DEV-027
#[test]
fn dev_027_session_text_commands_follow_the_same_rule() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin(
        "acme.demo",
        "0.1.0",
        "T3",
        "running",
        false,
        &[("fs.read", "low", "granted")],
    );
    let env = env_session(root.path(), &session);
    assert!(run(&env, &["list"]).stdout_jsonl.is_empty());
    assert!(run(&env, &["grants", "acme.demo"]).stdout_jsonl.is_empty());
    assert_eq!(recs(&run(&env, &["list", "--json"]), "plugin").len(), 1);
    assert_eq!(
        recs(&run(&env, &["grants", "acme.demo", "--json"]), "grant").len(),
        1
    );
}

/// REQ: DEV-028
#[test]
fn dev_028_scripts_get_a_stable_contract_from_json() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let first = run(&env, &["list", "--json"]);
    let second = run(&env, &["list", "--json"]);
    assert_eq!(first.stdout_jsonl[0]["id"], "acme.demo");
    assert_eq!(
        first.stdout_raw.lines().next(),
        second.stdout_raw.lines().next()
    );
}

/// REQ: DEV-028
#[test]
fn dev_028_text_output_is_for_people_and_is_not_json() {
    let root = scratch();
    let session = MemorySession::new();
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    let env = env_session(root.path(), &session);
    let text = run(&env, &["list"]).stdout_text;
    assert!(text.contains("acme.demo"));
    assert!(serde_json::from_str::<Value>(text.lines().next().unwrap_or("")).is_err());
}

/// REQ: DEV-029
#[test]
fn dev_029_key_flag_overrides_the_signing_key_variable() {
    let root = scratch();
    let (dir, package) = packed_project(&env(root.path()), root.path());
    let _ = dir;
    let (good, _) = make_key(&env(root.path()), root.path(), "acme");
    let bad = root.path().join("bad.key");
    std::fs::write(&bad, "garbage").expect("write");
    let env = env(root.path()).with_var("ILIUM_PLUGIN_SIGNING_KEY", bad.to_str().expect("utf8"));
    let output = run(
        &env,
        &[
            "sign",
            package.to_str().expect("utf8"),
            "--key",
            good.to_str().expect("utf8"),
        ],
    );
    assert_eq!(
        output.exit_code, 0,
        "the flag wins over the variable: {:?}",
        output.stdout_jsonl
    );
}

/// REQ: DEV-029
#[test]
fn dev_029_the_variable_is_used_when_the_flag_is_absent() {
    let root = scratch();
    let base = env(root.path());
    let (_, package) = packed_project(&base, root.path());
    let (good, _) = make_key(&base, root.path(), "acme");
    let env = base.with_var("ILIUM_PLUGIN_SIGNING_KEY", good.to_str().expect("utf8"));
    assert_eq!(
        run(&env, &["sign", package.to_str().expect("utf8")]).exit_code,
        0
    );
}

/// REQ: DEV-029
#[test]
fn dev_029_offline_flag_beats_the_offline_variable() {
    let root = scratch();
    let env = env(root.path()).with_var("ILIUM_PLUGIN_OFFLINE", "0");
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--offline"]);
    // manual does not pin that this project has an uncached dependency: when --offline fails the
    // build, it must be with build.offline-missing, and result.ok must agree with the exit code
    if let Some(found) = code(&output) {
        assert_eq!(found, "build.offline-missing");
        assert_eq!(output.exit_code, 1);
    } else {
        assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    }
}

/// REQ: DEV-030
#[test]
fn dev_030_a_failed_build_keeps_the_previous_component_intact() {
    let root = scratch();
    let base = env(root.path());
    let dir = server_project(&base, root.path());
    run_in(&base, &dir, &["build"]);
    let before = std::fs::read(dir.join("dist/server.wasm")).expect("read");
    let failing = base.with_builder(FakeBuilder::failing_at("component"));
    run_in(&failing, &dir, &["build"]);
    assert_eq!(
        std::fs::read(dir.join("dist/server.wasm")).expect("read"),
        before
    );
}

/// REQ: DEV-030
#[test]
fn dev_030_no_temporary_file_remains_after_success() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["build"]);
    assert_eq!(
        tree_listing(&dir.join("dist")),
        ["build.json", "server.wasm"]
    );
}

/// REQ: DEV-030
#[test]
fn dev_030_pack_replaces_its_output_without_leaving_a_temporary_file() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let before = tree_listing(&dir.join("dist"));
    let second = run_in(&env, &dir, &["pack"]);
    assert_eq!(second.exit_code, 0);
    assert!(package.is_file());
    assert_eq!(
        tree_listing(&dir.join("dist")),
        before,
        "the replaced output leaves exactly the same file set"
    );
    assert_eq!(artifacts(&second, "package").len(), 1);
}
