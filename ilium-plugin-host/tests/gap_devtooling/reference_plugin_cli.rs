//! The `ilium plugin` command line: clean, locks, project selection, configuration, environment,
//! version, lint fixes, size budget, SDK upgrade, dependency check, platform rules, encoding,
//! interrupts, test options, bench, dev data reset, diff, SBOM and doctor bundle (DEV-900 to
//! DEV-941 as listed in the devtooling-gap inventory). Page: reference-plugin-cli.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::*;
use crate::{
    add_slow_test_fixture, bench_with_deadline_exceeded, dev_data_folders, dev_once,
    dev_watcher_poll_interval_when_native_unavailable, manual_page, new_rust_project,
    next_sdk_minor, pack_interrupted_by, package_component_limit_bytes,
    project_pinned_sdk_behind_cli, rows_with, sdk_current_version, seed_dev_data, snapshot, utf8,
    with_code, zip_entries,
};

/// REQ: DEV-900
#[test]
fn clean_removes_dist_and_build_cache_keeps_sources_keys_and_is_idempotent() {
    let (_root, env, dir) = new_rust_project();
    assert_eq!(
        run_in_project(&env, "build", &[]).exit_code,
        0,
        "build first"
    );
    write_text(&dir, "publisher.key", "key material kept by clean");
    write_text(&dir, "target/debug/marker", "build cache");
    let sources_before = snapshot(&dir.join("src"));
    let cleaned = run_in_project(&env, "clean", &[]);
    assert_eq!(cleaned.exit_code, 0, "{:?}", cleaned.stdout_jsonl);
    assert!(
        !records_of(&cleaned, "artifact").is_empty(),
        "one artifact record per removed path"
    );
    assert!(!dir.join("dist").exists(), "dist/ is deleted");
    assert!(dir.join("target").exists(), "target/ stays without --all");
    assert!(dir.join("manifest.json").is_file(), "manifest.json is kept");
    assert!(dir.join("publisher.key").is_file(), "publisher.key is kept");
    assert_eq!(snapshot(&dir.join("src")), sources_before, "src/ is kept");
    let again = run_in_project(&env, "clean", &[]);
    assert_eq!(
        again.exit_code, 0,
        "a second clean succeeds when nothing exists"
    );
    let all = run_in_project(&env, "clean", &["--all"]);
    assert_eq!(all.exit_code, 0, "{:?}", all.stdout_jsonl);
    assert!(!dir.join("target").exists(), "--all also deletes target/");
}

/// REQ: DEV-901
#[test]
fn live_lock_holder_makes_second_command_fail_with_build_locked_and_dead_lock_is_broken() {
    let (_root, env, dir) = new_rust_project();
    std::fs::create_dir_all(dir.join("dist")).expect("create dist");
    let holder_pid = std::process::id().to_string();
    write_text(&dir, "dist/.lock", &holder_pid);
    let started = Instant::now();
    let blocked = run_in_project(&env, "build", &[]);
    assert_eq!(blocked.exit_code, 1, "{:?}", blocked.stdout_jsonl);
    assert_eq!(error_code(&blocked).as_deref(), Some("build.locked"));
    let message = records_of(&blocked, "error")[0]["message"]
        .as_str()
        .expect("error message")
        .to_owned();
    assert!(
        message.contains(&holder_pid),
        "the error names the holder PID: {message}"
    );
    assert!(
        started.elapsed() >= Duration::from_secs(30),
        "the second command waits up to 30 s"
    );
    write_text(&dir, "dist/.lock", "2147483646");
    let broken = run_in_project(&env, "build", &[]);
    assert_eq!(
        broken.exit_code, 0,
        "a lock of a dead PID is broken: {:?}",
        broken.stdout_jsonl
    );
}

/// REQ: DEV-902
#[test]
fn manifest_path_selects_the_project_and_a_conflicting_directory_is_refused() {
    let (_root, env, dir) = new_rust_project();
    let manifest = dir.join("manifest.json");
    let ok = run(
        &env,
        &["build", "--manifest-path", utf8(&manifest), "--json"],
    );
    assert_eq!(ok.exit_code, 0, "{:?}", ok.stdout_jsonl);
    for artifact in records_of(&ok, "artifact") {
        let path = artifact["path"].as_str().expect("artifact path");
        assert!(
            !Path::new(path).is_absolute(),
            "paths are relative to the project: {path}"
        );
    }
    let conflicting = run(
        &env,
        &[
            "build",
            utf8(&dir),
            "--manifest-path",
            utf8(&manifest),
            "--json",
        ],
    );
    assert_eq!(conflicting.exit_code, 2, "{:?}", conflicting.stdout_jsonl);
    assert_eq!(
        error_code(&conflicting).as_deref(),
        Some("usage.conflicting-flags")
    );
}

/// REQ: DEV-903
#[test]
fn unknown_key_in_ilium_plugin_toml_fails_with_config_unknown_key() {
    let (_root, env, dir) = new_rust_project();
    write_text(
        &dir,
        "ilium-plugin.toml",
        "[build]\nprofile = \"release\"\nspeed = \"fast\"\n",
    );
    let out = run_in_project(&env, "check", &[]);
    assert_eq!(out.exit_code, 2, "{:?}", out.stdout_jsonl);
    assert_eq!(error_code(&out).as_deref(), Some("config.unknown-key"));
}

/// REQ: DEV-903
#[test]
fn command_line_flag_overrides_config_file_which_overrides_defaults() {
    let (_root, env, dir) = new_rust_project();
    write_text(
        &dir,
        "ilium-plugin.toml",
        "[build]\nprofile = \"release\"\n",
    );
    let from_file = run_in_project(&env, "build", &[]);
    assert_eq!(from_file.exit_code, 0, "{:?}", from_file.stdout_jsonl);
    assert_eq!(
        records_of(&from_file, "build")[0]["profile"],
        "release",
        "the file overrides the default"
    );
    let with_flag = run_in_project(&env, "build", &["--profile", "debug"]);
    assert_eq!(
        records_of(&with_flag, "build")[0]["profile"],
        "debug",
        "the flag overrides the file"
    );
}

/// REQ: DEV-904
#[test]
fn reference_lists_every_cli_environment_variable_with_its_override_flag() {
    let page = manual_page("reference-plugin-cli");
    for variable in [
        "ILIUM_SESSION",
        "ILIUM_PLUGIN_SIGNING_KEY",
        "NO_COLOR",
        "ILIUM_PLUGIN_OFFLINE",
    ] {
        let rows: Vec<&str> = page
            .lines()
            .filter(|line| line.contains(variable))
            .collect();
        assert!(!rows.is_empty(), "the reference lists {variable}");
        assert!(
            rows.iter().any(|line| line.contains("--")),
            "the row of {variable} names the flag that overrides it"
        );
    }
}

/// REQ: DEV-904
#[test]
fn unknown_ilium_plugin_variable_is_ignored_and_doctor_warns_env_unknown() {
    let root = scratch();
    let mut cli_env = env(root.path());
    cli_env
        .env
        .insert("ILIUM_PLUGIN_BOGUS_KNOB".to_owned(), "1".to_owned());
    let doctor = run(&cli_env, &["doctor", "--json"]);
    let warnings = with_code(&doctor, "env.unknown");
    assert!(
        !warnings.is_empty(),
        "one env.unknown warning per unknown variable"
    );
    assert_eq!(warnings[0]["severity"], "warn", "{}", warnings[0]);
    assert!(
        warnings[0].to_string().contains("ILIUM_PLUGIN_BOGUS_KNOB"),
        "the warning names it"
    );
}

/// REQ: DEV-906
#[test]
fn version_prints_one_record_with_cli_contract_wit_and_test_host_versions_without_session() {
    let root = scratch();
    let cli_env = env(root.path());
    let out = run(&cli_env, &["--version", "--json"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(
        out.stdout_jsonl.len(),
        1,
        "one version record and nothing else"
    );
    let versions = records_of(&out, "version");
    assert_eq!(versions.len(), 1, "one version record");
    for key in [
        "cli_version",
        "contract_version",
        "wit_versions",
        "test_host_version",
    ] {
        assert!(
            versions[0].get(key).is_some(),
            "the record names {key}: {}",
            versions[0]
        );
    }
}

/// REQ: DEV-908
#[test]
fn lint_fix_applies_fixable_rules_atomically_and_a_second_run_changes_nothing() {
    let (_root, env, dir) = new_rust_project();
    let fixed = lint(&env, &["--fix"]);
    for artifact in records_of(&fixed, "artifact") {
        assert!(
            artifact["path"].is_string(),
            "one artifact record per modified file: {artifact}"
        );
    }
    assert!(
        tree_of(&dir).iter().all(|path| !path.ends_with(".tmp")),
        "atomic rewrite leaves no temp file"
    );
    let before = snapshot(&dir);
    let second = lint(&env, &["--fix"]);
    assert!(
        records_of(&second, "artifact").is_empty(),
        "a second run modifies zero files"
    );
    assert_eq!(snapshot(&dir), before, "the second run changes no bytes");
}

/// REQ: DEV-908
#[test]
fn lint_fix_refuses_dirty_git_tree_with_lint_dirty_tree_unless_allow_dirty() {
    let (_root, env, dir) = new_rust_project();
    git_commit_all(&dir);
    write_text(&dir, "README.md", "uncommitted edit\n");
    let refused = lint(&env, &["--fix"]);
    assert_eq!(refused.exit_code, 1, "{:?}", refused.stdout_jsonl);
    assert_eq!(error_code(&refused).as_deref(), Some("lint.dirty-tree"));
    let allowed = lint(&env, &["--fix", "--allow-dirty"]);
    assert_ne!(
        error_code(&allowed).as_deref(),
        Some("lint.dirty-tree"),
        "--allow-dirty lifts the refusal"
    );
}

/// REQ: DEV-911
#[test]
fn component_over_2_mib_warns_and_over_package_limit_is_an_error() {
    let (_root, env, dir) = new_rust_project();
    write_dist(&dir, "server", &server_component_of_size(3 * 1024 * 1024));
    let warned = run_in_project(&env, "check", &[]);
    let budget = with_code(&warned, "size.component-over-budget");
    assert!(
        !budget.is_empty(),
        "a 3 MiB component exceeds the 2 MiB budget"
    );
    assert_eq!(budget[0]["severity"], "warn");
    let over_limit = package_component_limit_bytes() + 1;
    write_dist(&dir, "server", &server_component_of_size(over_limit));
    let errored = run_in_project(&env, "check", &[]);
    let error_budget = with_code(&errored, "size.component-over-budget");
    assert_eq!(
        error_budget[0]["severity"], "error",
        "over the format limit it is an error"
    );
    write_text(
        &dir,
        "ilium-plugin.toml",
        "[build]\nmax_component_bytes = 1024\n",
    );
    write_dist(&dir, "server", &server_component_of_size(64 * 1024));
    let lowered = run_in_project(&env, "check", &[]);
    assert!(
        !with_code(&lowered, "size.component-over-budget").is_empty(),
        "[build] max_component_bytes lowers the budget"
    );
}

/// REQ: DEV-912
#[test]
fn upgrade_sdk_refuses_an_older_target_and_a_major_jump_without_migration_note() {
    let (_root, env, dir) = new_rust_project();
    let before = snapshot(&dir);
    let older = run_in_project(&env, "upgrade-sdk", &["--to", "0.0.1", "--dry-run"]);
    assert_eq!(older.exit_code, 1, "{:?}", older.stdout_jsonl);
    assert_eq!(error_code(&older).as_deref(), Some("upgrade.no-path"));
    let major = run_in_project(&env, "upgrade-sdk", &["--to", "99.0.0", "--dry-run"]);
    assert_eq!(
        error_code(&major).as_deref(),
        Some("upgrade.no-path"),
        "a major jump needs a migration note"
    );
    assert_eq!(snapshot(&dir), before, "a dry run changes nothing");
}

/// REQ: DEV-912
#[test]
fn upgrade_sdk_is_a_no_op_when_current_and_prints_each_edit_as_diff() {
    let (_root, env, _dir) = new_rust_project();
    let current = sdk_current_version(&env);
    let noop = run_in_project(&env, "upgrade-sdk", &["--to", current.as_str()]);
    assert_eq!(noop.exit_code, 0, "{:?}", noop.stdout_jsonl);
    assert!(
        records_of(&noop, "diff").is_empty(),
        "no edit records when already current"
    );
    let newer = next_sdk_minor(&current);
    let planned = run_in_project(&env, "upgrade-sdk", &["--to", newer.as_str(), "--dry-run"]);
    assert!(
        !records_of(&planned, "diff").is_empty(),
        "each planned edit is a diff record"
    );
}

/// REQ: DEV-913
#[test]
fn check_deps_reports_sdk_versions_offline_and_warns_when_two_minors_behind() {
    let (_root, env, _dir) = new_rust_project();
    let deps = run_in_project(&env, "check", &["--deps"]);
    let rows = rows_with(&deps, "sdk_in_use");
    assert!(!rows.is_empty(), "the deps report names the SDK in use");
    assert!(
        rows[0]["sdk_newest_known"].is_string(),
        "and the newest SDK the CLI knows"
    );
    let behind_env = project_pinned_sdk_behind_cli(&env, 3);
    let stale = run_in_project(&behind_env, "check", &["--deps"]);
    let outdated = with_code(&stale, "deps.sdk-outdated");
    assert!(
        !outdated.is_empty(),
        "a project three minors behind is flagged"
    );
    assert_eq!(outdated[0]["severity"], "warn");
}

/// REQ: DEV-914
#[test]
fn offline_commands_write_owner_only_keys_lf_endings_and_forward_slash_paths() {
    let (_root, env, dir) = new_rust_project();
    let keygen = run_in_project(&env, "keygen", &[]);
    assert_eq!(keygen.exit_code, 0, "{:?}", keygen.stdout_jsonl);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("publisher.key"))
            .expect("key file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "key files are owner-only");
    }
    let generated = read_text(&dir, "src/lib.rs");
    assert!(
        !generated.contains('\r'),
        "generated files use LF line endings"
    );
    for artifact in records_of(&keygen, "artifact") {
        let path = artifact["path"].as_str().expect("artifact path");
        assert!(!path.contains('\\'), "paths use forward slashes: {path}");
    }
    assert_eq!(
        dev_watcher_poll_interval_when_native_unavailable(&env),
        500,
        "the watcher polls every 500 ms without native events"
    );
}

/// REQ: DEV-915
#[test]
fn invalid_utf8_manifest_fails_with_manifest_encoding_and_byte_offset() {
    let (_root, env, dir) = new_rust_project();
    let mut bytes = std::fs::read(dir.join("manifest.json")).expect("manifest");
    bytes.insert(10, 0xff);
    write_bytes(&dir, "manifest.json", &bytes);
    let out = run_in_project(&env, "check", &[]);
    assert_eq!(
        error_code(&out).as_deref(),
        Some("manifest.encoding"),
        "{:?}",
        out.stdout_jsonl
    );
    assert_eq!(
        records_of(&out, "error")[0]["offset"].as_u64(),
        Some(10),
        "the byte offset is named"
    );
}

/// REQ: DEV-915
#[test]
fn codes_and_jsonl_keys_are_not_localised_under_a_non_english_locale() {
    let (_root, mut env, dir) = new_rust_project();
    env.env.insert("LANG".to_owned(), "de_DE.UTF-8".to_owned());
    env.env
        .insert("LC_ALL".to_owned(), "de_DE.UTF-8".to_owned());
    let out = run_in_project(&env, "check", &[]);
    assert!(!out.stdout_jsonl.is_empty(), "the check prints records");
    for record in &out.stdout_jsonl {
        for key in ["code", "rule"] {
            if let Some(value) = record.get(key).and_then(Value::as_str) {
                assert!(value.is_ascii(), "{key} is not localised: {value}");
            }
        }
    }
    let text = run(&env, &["check", utf8(&dir)]);
    assert!(
        !text.stdout_text.is_empty(),
        "text mode prints its messages"
    );
    assert!(
        text.stdout_text.is_ascii(),
        "text mode prints English only: {}",
        text.stdout_text
    );
}

/// REQ: DEV-916
#[test]
fn interrupt_during_pack_or_sign_leaves_no_partial_output_and_exits_130() {
    let (_root, env, dir) = new_rust_project();
    assert_eq!(
        run_in_project(&env, "build", &[]).exit_code,
        0,
        "build first"
    );
    for command in ["pack", "sign"] {
        for signal in ["SIGINT", "SIGTERM"] {
            let before = snapshot(&dir.join("dist"));
            let interrupted = pack_interrupted_by(&env, command, signal);
            assert_eq!(
                interrupted.exit_code, 130,
                "{command} after {signal}: {:?}",
                interrupted.stdout_jsonl
            );
            assert!(
                !records_of(&interrupted, "summary").is_empty(),
                "{command} prints a summary"
            );
            assert!(
                !records_of(&interrupted, "result").is_empty(),
                "{command} prints a result"
            );
            let after = snapshot(&dir.join("dist"));
            assert!(
                after.keys().all(|name| !name.ends_with(".tmp")),
                "no *.tmp file remains after {command} {signal}"
            );
            assert!(
                after == before || after.keys().any(|name| name.ends_with(".iliumpkg")),
                "the target is unchanged or fully written after {command} {signal}"
            );
        }
    }
}

/// REQ: DEV-920
#[test]
fn test_timeout_above_600000_ms_is_refused_and_an_empty_filter_exits_2() {
    let (_root, env, _dir) = new_rust_project();
    let big = run_in_project(&env, "test", &["--timeout-ms", "600001"]);
    assert_eq!(big.exit_code, 2, "{:?}", big.stdout_jsonl);
    assert_eq!(error_code(&big).as_deref(), Some("usage.out-of-range"));
    let filtered = run_in_project(&env, "test", &["--filter", "no-such-fixture-*"]);
    assert_eq!(filtered.exit_code, 2, "{:?}", filtered.stdout_jsonl);
    assert_eq!(error_code(&filtered).as_deref(), Some("test.no-match"));
}

/// REQ: DEV-920
#[test]
fn case_over_its_timeout_reports_fail_with_reason_timeout() {
    let (_root, env, dir) = new_rust_project();
    add_slow_test_fixture(&dir);
    let out = run_in_project(&env, "test", &["--timeout-ms", "1000"]);
    let slow = rows_with(&out, "fixture")
        .into_iter()
        .find(|row| row["fixture"] == "slow")
        .expect("the slow fixture has a result row");
    assert_eq!(slow["status"], "fail", "{slow}");
    assert_eq!(slow["reason"], "timeout", "{slow}");
}

/// REQ: DEV-922
#[test]
fn bench_prints_p50_p99_max_per_export_labelled_synthetic_and_exits_3_over_deadline() {
    let (_root, env, _dir) = new_rust_project();
    assert_eq!(
        run_in_project(&env, "build", &[]).exit_code,
        0,
        "build first"
    );
    let out = run_in_project(&env, "bench", &[]);
    let timings = records_of(&out, "timing");
    assert!(!timings.is_empty(), "one timing record per export");
    for timing in timings {
        for key in ["p50_ms", "p99_ms", "max_ms"] {
            assert!(
                timing.get(key).is_some(),
                "the timing record has {key}: {timing}"
            );
        }
        assert_eq!(timing["synthetic"], true, "{timing}");
    }
    let over = bench_with_deadline_exceeded(&env);
    assert_eq!(over.exit_code, 3, "{:?}", over.stdout_jsonl);
}

/// REQ: DEV-931
#[test]
fn dev_reset_data_deletes_data_cache_and_buckets_before_first_install_with_warning() {
    let (_root, env, _dir) = new_rust_project();
    seed_dev_data(&env);
    let folders = dev_data_folders(&env);
    assert!(
        folders.iter().all(|folder| folder.exists()),
        "the seeded folders exist"
    );
    let kept = dev_once(&env, &[]);
    assert_eq!(kept.exit_code, 0, "{:?}", kept.stdout_jsonl);
    assert!(
        folders.iter().all(|folder| folder.exists()),
        "data survives a run without the flag"
    );
    let reset = dev_once(&env, &["--reset-data"]);
    assert_eq!(reset.exit_code, 0, "{:?}", reset.stdout_jsonl);
    let warnings = records_of(&reset, "warning");
    assert_eq!(warnings.len(), 1, "one warning record naming the folders");
    assert!(
        folders.iter().all(|folder| !folder.exists()),
        "the data, cache and bucket folders are deleted before the first install"
    );
}

/// REQ: DEV-939
#[test]
fn diff_of_identical_packages_exits_0_and_different_packages_exit_3_without_extracting() {
    let root = scratch();
    let env = env(root.path());
    let old_path = write_package_file(
        root.path(),
        "old.iliumpkg",
        &signed_package(&good_server_component()),
    );
    let same = run(
        &env,
        &["diff", old_path.as_str(), old_path.as_str(), "--json"],
    );
    assert_eq!(same.exit_code, 0, "{:?}", same.stdout_jsonl);
    let changed_component = server_component_of_size(128 * 1024);
    let new_path = write_package_file(
        root.path(),
        "new.iliumpkg",
        &signed_package(&changed_component),
    );
    let differ = run(
        &env,
        &["diff", old_path.as_str(), new_path.as_str(), "--json"],
    );
    assert_eq!(differ.exit_code, 3, "{:?}", differ.stdout_jsonl);
    assert!(
        !records_of(&differ, "diff").is_empty(),
        "diff records describe the changes"
    );
    assert_eq!(
        tree_of(root.path()),
        vec!["new.iliumpkg".to_owned(), "old.iliumpkg".to_owned()],
        "nothing is extracted to disk"
    );
}

/// REQ: DEV-940
#[test]
fn pack_sbom_writes_cyclonedx_1_5_with_locked_dependencies_reproducibly() {
    let (_root, env, dir) = new_rust_project();
    assert_eq!(
        run_in_project(&env, "build", &[]).exit_code,
        0,
        "build first"
    );
    let manifest = base_manifest();
    let name = format!(
        "{}-{}.sbom.json",
        manifest["id"].as_str().expect("manifest id"),
        manifest["version"].as_str().expect("manifest version")
    );
    let packed = run_in_project(&env, "pack", &["--sbom"]);
    assert_eq!(packed.exit_code, 0, "{:?}", packed.stdout_jsonl);
    let sbom: Value =
        serde_json::from_str(&read_text(&dir.join("dist"), &name)).expect("sbom JSON");
    assert_eq!(sbom["bomFormat"], "CycloneDX");
    assert_eq!(sbom["specVersion"], "1.5");
    let components = sbom["components"].as_array().expect("components array");
    assert!(!components.is_empty(), "every locked dependency is listed");
    for component in components {
        assert!(
            component["name"].is_string() && component["version"].is_string(),
            "{component}"
        );
        assert!(
            component["hashes"].is_array(),
            "each component carries its checksum: {component}"
        );
    }
    assert!(
        records_of(&packed, "artifact")
            .iter()
            .any(|record| record["kind"] == "sbom"),
        "an artifact record of kind sbom is printed"
    );
    let first = read_text(&dir.join("dist"), &name);
    assert_eq!(
        run_in_project(&env, "pack", &["--sbom"]).exit_code,
        0,
        "second pack"
    );
    assert_eq!(
        read_text(&dir.join("dist"), &name),
        first,
        "the SBOM is reproducible"
    );
}

/// REQ: DEV-941
#[test]
fn doctor_bundle_redacts_secrets_and_excludes_sources_and_keys() {
    let (root, mut env, dir) = new_rust_project();
    env.env
        .insert("PLUGIN_API_TOKEN".to_owned(), "token-value-123".to_owned());
    env.env
        .insert("DATABASE_SECRET".to_owned(), "secret-value-456".to_owned());
    write_text(&dir, "publisher.key", "PRIVATE-KEY-MATERIAL");
    let bundle: PathBuf = root.path().join("bundle.zip");
    let out = run_in_project(&env, "doctor", &["--bundle", utf8(&bundle)]);
    assert!(
        bundle.is_file(),
        "the bundle is written: {:?}",
        out.stdout_jsonl
    );
    let entries = zip_entries(&bundle);
    let all_text = entries
        .iter()
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !all_text.contains("token-value-123"),
        "values of *TOKEN variables are redacted"
    );
    assert!(
        !all_text.contains("secret-value-456"),
        "values of *SECRET variables are redacted"
    );
    assert!(
        !all_text.contains("PRIVATE-KEY-MATERIAL"),
        "key files are not included"
    );
    assert!(
        entries
            .iter()
            .all(|(name, _)| !name.ends_with(".rs") && !name.ends_with(".key")),
        "source and key files are not included"
    );
    assert!(
        entries.iter().any(|(name, _)| name.contains("manifest")),
        "the manifest is included"
    );
}

/// Initialises a git repository in `dir` and commits every file, so later edits are dirty.
fn git_commit_all(dir: &Path) {
    let steps: [&[&str]; 3] = [
        &["init", "-q"],
        &["add", "-A"],
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    ];
    for step in steps {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(step)
            .status()
            .expect("run git in the test project");
        assert!(status.success(), "git {step:?} succeeds");
    }
}
