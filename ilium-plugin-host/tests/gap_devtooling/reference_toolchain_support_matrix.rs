//! Host and toolchain support: the build matrix, unsupported hosts, end of life, minimum-toolchain
//! announcements, untested toolchains and pins (DEV-882, DEV-884, DEV-965, DEV-988, DEV-989,
//! DEV-990). Page: reference-toolchain-support-matrix.

use std::collections::BTreeSet;

use crate::common::*;
use crate::{
    announced_minimum_raises, build_pinned_component_on_second_host, end_of_life_host_env,
    file_sha256, install_toolchain_one_minor_newer_than_tested, installed_plugins_still_load,
    installed_rust_version, manual_page, matrix_component_hashes, new_rust_project,
    server_start_log_lines, toolchain_minimums, unsupported_host_env, with_code,
};

/// REQ: DEV-882
#[test]
fn same_plugin_builds_to_identical_component_bytes_on_every_matrix_host() {
    let (_root, env, _dir) = new_rust_project();
    let hashes = matrix_component_hashes(&env);
    assert_eq!(
        hashes.len(),
        6,
        "one build per Linux, macOS and Windows on x86-64 and aarch64"
    );
    let distinct: BTreeSet<&String> = hashes.values().collect();
    assert_eq!(
        distinct.len(),
        1,
        "component hashes differ by host: {hashes:?}"
    );
}

/// REQ: DEV-884
#[test]
fn build_on_unsupported_host_fails_with_host_unsupported_and_names_the_pairs() {
    let root = scratch();
    let env = unsupported_host_env(root.path(), "freebsd", "riscv64");
    let project = root.path().join("project");
    let build = run(&env, &["build", project.to_str().expect("utf8"), "--json"]);
    assert_eq!(build.exit_code, 1, "{:?}", build.stdout_jsonl);
    assert_eq!(
        error_code(&build).as_deref(),
        Some("toolchain.host-unsupported"),
        "{:?}",
        build.stdout_jsonl
    );
    let error = records_of(&build, "error")[0];
    assert_eq!(error["os"], "freebsd", "{error}");
    assert_eq!(error["arch"], "riscv64", "{error}");
    assert!(
        error["supported"]
            .as_array()
            .is_some_and(|pairs| !pairs.is_empty()),
        "the supported pairs are listed: {error}"
    );
}

/// REQ: DEV-884
#[test]
fn check_and_inspect_run_on_unsupported_host_with_host_unsupported_warning() {
    let (root, _env, _dir) = new_rust_project();
    let host_env = unsupported_host_env(root.path(), "freebsd", "riscv64");
    for command in ["check", "inspect"] {
        let out = run_in_project(&host_env, command, &[]);
        assert_eq!(
            out.exit_code, 0,
            "{command} may run on an unsupported host: {:?}",
            out.stdout_jsonl
        );
        assert!(
            !with_code(&out, "toolchain.host-unsupported").is_empty(),
            "{command} prints a warning with the same code"
        );
    }
}

/// REQ: DEV-965
#[test]
fn end_of_life_host_reports_support_field_and_writes_one_warn_line_per_start() {
    let root = scratch();
    let env = end_of_life_host_env(root.path(), "2020-01-01");
    let version = run(&env, &["version", "--json"]);
    assert_eq!(version.exit_code, 0, "{:?}", version.stdout_jsonl);
    let record = records_of(&version, "version")[0];
    assert_eq!(record["support"], "end-of-life", "{record}");
    let logs = server_start_log_lines(&env);
    let eol_warnings = logs
        .iter()
        .filter(|line| line["level"] == "warn" && line["code"] == "host.end-of-life")
        .count();
    assert_eq!(
        eol_warnings, 1,
        "one host.end-of-life warn line per server start"
    );
    assert!(
        installed_plugins_still_load(&env),
        "plugins keep loading after end of life"
    );
}

/// REQ: DEV-988
#[test]
fn raised_minimum_is_announced_one_minor_ahead_and_patch_releases_keep_minimums() {
    let page = manual_page("reference-toolchain-support-matrix");
    assert!(
        page.to_lowercase().contains("minimum"),
        "the support matrix states the minimum toolchains"
    );
    for (announced_in_minor, effective_minor) in announced_minimum_raises() {
        assert!(
            announced_in_minor < effective_minor,
            "a raise is announced in a minor release before it takes effect: {announced_in_minor} < {effective_minor}"
        );
    }
    let earlier_patch = toolchain_minimums("1.4.0");
    let later_patch = toolchain_minimums("1.4.1");
    assert_eq!(
        earlier_patch, later_patch,
        "a patch release must not raise a minimum"
    );
}

/// REQ: DEV-989
#[test]
fn doctor_reports_newer_untested_toolchain_as_warning_and_build_still_succeeds() {
    let (_root, env, dir) = new_rust_project();
    let newer = install_toolchain_one_minor_newer_than_tested(&env);
    let doctor = run(&env, &["doctor", "--json"]);
    assert_eq!(
        doctor.exit_code, 0,
        "untested is a warning, not a failure: {:?}",
        doctor.stdout_jsonl
    );
    let untested = with_code(&doctor, "toolchain.untested");
    assert!(
        !untested.is_empty(),
        "doctor reports the newer toolchain {newer} as untested"
    );
    assert_eq!(untested[0]["severity"], "warn");
    let built = run_in_project(&env, "build", &[]);
    assert_eq!(
        built.exit_code, 0,
        "the build succeeds: {:?}",
        built.stdout_jsonl
    );
    assert!(
        dir.join("dist/server.wasm").is_file(),
        "the component is written"
    );
    assert!(
        manual_page("reference-toolchain-support-matrix").contains("newest tested"),
        "the support matrix lists the newest tested version of each tool"
    );
}

/// REQ: DEV-990
#[test]
fn toolchain_pin_mismatch_fails_build_and_matching_pin_builds_reproducibly() {
    let (_root, env, dir) = new_rust_project();
    write_text(
        &dir,
        "ilium-toolchain.json",
        r#"{"rust": "0.0.1-not-installed"}"#,
    );
    let refused = run_in_project(&env, "build", &[]);
    assert_eq!(
        error_code(&refused).as_deref(),
        Some("toolchain.pin-mismatch"),
        "{:?}",
        refused.stdout_jsonl
    );
    assert_ne!(refused.exit_code, 0, "a pin mismatch must fail the build");
    let installed = installed_rust_version(&env);
    write_text(
        &dir,
        "ilium-toolchain.json",
        &format!(r#"{{"rust": "{installed}"}}"#),
    );
    let first = run_in_project(&env, "build", &[]);
    assert_eq!(first.exit_code, 0, "{:?}", first.stdout_jsonl);
    let first_hash = file_sha256(&dir.join("dist/server.wasm"));
    let second_hash = build_pinned_component_on_second_host(&env);
    assert_eq!(
        first_hash, second_hash,
        "a pinned build is reproducible across two hosts"
    );
}

/// REQ: DEV-882
#[test]
fn repeated_matrix_builds_on_one_host_give_the_same_component_hashes() {
    let (_root, env, _dir) = new_rust_project();
    let first = matrix_component_hashes(&env);
    let second = matrix_component_hashes(&env);
    assert_eq!(first, second, "a rebuild on the same host is reproducible");
    assert!(
        first.values().all(|hash| hash.len() == 64),
        "each hash is a SHA-256 hex digest: {first:?}"
    );
}

/// REQ: DEV-965
#[test]
fn host_before_its_end_of_support_date_reports_a_supported_status_and_no_end_of_life_warning() {
    let root = scratch();
    let env = end_of_life_host_env(root.path(), "2999-01-01");
    let version = run(&env, &["version", "--json"]);
    assert_eq!(version.exit_code, 0, "{:?}", version.stdout_jsonl);
    let record = records_of(&version, "version")[0];
    assert_ne!(record["support"], "end-of-life", "{record}");
    let logs = server_start_log_lines(&env);
    assert!(
        logs.iter().all(|line| line["code"] != "host.end-of-life"),
        "no end-of-life warning before the date"
    );
}

/// REQ: DEV-988
#[test]
fn toolchain_minimums_name_rust_bun_and_wasm_tools_in_every_release() {
    for release in ["1.4.0", "1.4.1"] {
        let minimums = toolchain_minimums(release);
        for tool in ["rust", "bun", "wasm-tools"] {
            assert!(
                minimums
                    .keys()
                    .any(|name| name.to_lowercase().contains(tool)),
                "{release} names a minimum for {tool}: {minimums:?}"
            );
        }
    }
}

/// REQ: DEV-989
#[test]
fn the_untested_toolchain_version_is_not_listed_as_tested_in_the_support_matrix() {
    let (_root, env, _dir) = new_rust_project();
    let untested = install_toolchain_one_minor_newer_than_tested(&env);
    let page = manual_page("reference-toolchain-support-matrix");
    assert!(
        !page.contains(&untested),
        "the newer untested version {untested} is not claimed as tested"
    );
}

/// REQ: DEV-990
#[test]
fn toolchain_pin_mismatch_writes_no_artifact_before_failing() {
    let (_root, env, dir) = new_rust_project();
    write_text(&dir, "ilium-toolchain.json", "{\"rust\": \"0.0.1\"}\n");
    let built = run_in_project(&env, "build", &[]);
    assert_ne!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    assert!(
        records_of(&built, "artifact").is_empty(),
        "a mismatched pin produces no artifact: {:?}",
        built.stdout_jsonl
    );
    assert!(!dir.join("dist").exists(), "nothing is written to dist/");
}
