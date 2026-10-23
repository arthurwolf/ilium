//! guide-ci-recipes.md (DEV-760..766, 770..773, 775..778). Channels: generated CI files, JSONL
//! on stdout versus stderr, exit codes, package bytes.

use ilium_plugin_host::cli::CliEnv;

use crate::common::*;
use crate::lint_support::*;

fn ci_project(ci: &str) -> (tempfile::TempDir, CliEnv, std::path::PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().unwrap(),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--ci",
            ci,
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

fn github_file() -> String {
    let (_root, _env, dir) = ci_project("github");
    read_text(&dir, ".github/workflows/plugin.yml")
}

fn gitlab_file() -> String {
    let (_root, _env, dir) = ci_project("gitlab");
    read_text(&dir, ".gitlab-ci.yml")
}

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in\n{text}"))
}

/// REQ: DEV-760 WARN
#[test]
fn dev_760_github_recipe_runs_the_steps_in_the_documented_order() {
    let text = github_file();
    let order = [
        "ilium plugin doctor",
        "ilium plugin check",
        "ilium plugin lint --deny warnings",
        "ilium plugin test",
        "ilium plugin build --release",
        "ilium plugin pack",
    ];
    let positions: Vec<usize> = order.iter().map(|c| position(&text, c)).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
}

/// REQ: DEV-760 WARN
#[test]
fn dev_760_gitlab_recipe_runs_the_steps_in_the_documented_order_and_never_continues_on_error() {
    let text = gitlab_file();
    let order = [
        "ilium plugin doctor",
        "ilium plugin check",
        "ilium plugin lint --deny warnings",
        "ilium plugin test",
        "ilium plugin build --release",
        "ilium plugin pack",
    ];
    let positions: Vec<usize> = order.iter().map(|c| position(&text, c)).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    assert!(!text.contains("allow_failure: true") && !text.contains("continue-on-error"));
}

/// REQ: DEV-761
#[test]
fn dev_761_offline_commands_work_with_no_session_and_no_terminal() {
    let (_root, env, dir) = ci_project("none");
    for command in ["check", "lint", "doctor"] {
        let args: Vec<&str> = if command == "doctor" {
            vec!["doctor", "--json"]
        } else {
            vec![command, dir.to_str().unwrap(), "--json"]
        };
        let output = run(&env, &args);
        assert_ne!(output.exit_code, 6, "{command} must not need a session");
        assert!(
            error_code(&output).is_none_or(|code| !code.starts_with("session.")),
            "{command}"
        );
    }
}

/// REQ: DEV-761
#[test]
fn dev_761_pack_and_inspect_of_a_file_need_no_session() {
    let (_root, env, dir) = ci_project("none");
    write_dist(&dir, "server", &good_server_component());
    let packed = run(
        &env,
        &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"],
    );
    assert_eq!(packed.exit_code, 0, "{:?}", packed.stdout_jsonl);
    let file = records_of(&packed, "artifact")[0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(run(&env, &["inspect", &file, "--json"]).exit_code, 6);
}

/// REQ: DEV-762
#[test]
fn dev_762_json_output_is_jsonl_on_stdout_only() {
    let (_root, env, dir) = ci_project("none");
    let output = run(&env, &["lint", dir.to_str().unwrap(), "--json"]);
    assert!(!output.stdout_jsonl.is_empty());
    assert!(
        output
            .stdout_text
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()),
        "{}",
        output.stdout_text
    );
}

/// REQ: DEV-762
#[test]
fn dev_762_progress_text_goes_to_standard_error() {
    let (_root, env, dir) = ci_project("none");
    let output = run(&env, &["build", dir.to_str().unwrap(), "--json"]);
    assert!(!output.stdout_text.contains("Building"));
    // manual does not pin the progress wording, only that it is on stderr and not JSONL
    assert!(!output.stdout_jsonl.is_empty());
    assert!(
        !output.stderr.trim().is_empty(),
        "no progress text on stderr"
    );
    assert!(
        output
            .stderr
            .lines()
            .all(|l| serde_json::from_str::<serde_json::Value>(l).is_err()),
        "{}",
        output.stderr
    );
}

/// REQ: DEV-763
#[test]
fn dev_763_both_recipes_set_no_color() {
    assert!(github_file().contains("NO_COLOR: \"1\""));
    assert!(gitlab_file().contains("NO_COLOR: \"1\""));
}

/// REQ: DEV-763
#[test]
fn dev_763_output_to_a_non_terminal_has_no_escape_sequences() {
    let (_root, env, dir) = ci_project("none");
    for args in [
        vec!["lint", dir.to_str().unwrap()],
        vec!["check", dir.to_str().unwrap()],
        vec!["doctor"],
    ] {
        let output = run(&env, &args);
        assert!(
            !output.stdout_text.contains('\u{1b}') && !output.stderr.contains('\u{1b}'),
            "{args:?}"
        );
    }
}

/// REQ: DEV-764
#[test]
fn dev_764_check_exits_3_for_findings_and_lint_deny_exits_3_for_warnings() {
    let (_root, env, dir) = ci_project("none");
    assert_eq!(
        run(&env, &["check", dir.to_str().unwrap(), "--json"]).exit_code,
        3
    );
    let manifest =
        read_text(&dir, "ilium-plugin.json").replace("\"description\":", "\"description_x\":");
    write_text(&dir, "ilium-plugin.json", &manifest);
    let plain = run(&env, &["lint", dir.to_str().unwrap(), "--json"]);
    assert!(
        records_of(&plain, "lint")
            .iter()
            .any(|r| r["severity"] == "warn" && r["suppressed"] != true),
        "input must yield a warn finding: {:?}",
        plain.stdout_jsonl
    );
    assert_eq!(
        plain.exit_code, 0,
        "a warn finding is not denied without the flag"
    );
    let denied = run(
        &env,
        &[
            "lint",
            dir.to_str().unwrap(),
            "--deny",
            "warnings",
            "--json",
        ],
    );
    assert_eq!(denied.exit_code, 3, "{:?}", denied.stdout_jsonl);
}

/// REQ: DEV-764
#[test]
fn dev_764_exit_codes_are_only_from_the_documented_set() {
    let (_root, env, dir) = ci_project("none");
    // manual does not pin the exit code of a missing directory, only membership in the table
    let missing = run(&env, &["check", "/no/such/dir"]).exit_code;
    assert!(
        [1, 2, 3, 4, 5].contains(&missing),
        "missing dir -> {missing}"
    );
    // DEV-010: a usage error before any work starts exits 2
    assert_eq!(run(&env, &["nonsense"]).exit_code, 2);
    // DEV-011: a fresh project has check findings of severity error
    assert_eq!(run(&env, &["check", dir.to_str().unwrap()]).exit_code, 3);
    // DEV-011: plain lint never fails on warnings
    let lint = run(&env, &["lint", dir.to_str().unwrap()]);
    assert!(
        [0, 3].contains(&lint.exit_code),
        "lint -> {}",
        lint.exit_code
    );
}

/// REQ: DEV-765
#[test]
fn dev_765_github_third_party_actions_are_pinned_to_a_major_version_tag() {
    let text = github_file();
    let uses: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().trim_start_matches("- ").strip_prefix("uses: "))
        .collect();
    assert!(!uses.is_empty());
    for action in uses {
        let (_, version) = action.split_once('@').expect("pinned");
        assert!(
            version.starts_with('v') && version[1..].chars().all(|c| c.is_ascii_digit()),
            "{action}"
        );
    }
}

/// REQ: DEV-765
#[test]
fn dev_765_download_url_and_hash_come_from_repository_variables_not_literals() {
    let text = github_file();
    assert!(text.contains("${{ vars.ILIUM_URL }}") && text.contains("${{ vars.ILIUM_SHA256 }}"));
    assert!(!text.contains("http://") && !text.contains("https://"));
    let gitlab = gitlab_file();
    assert!(gitlab.contains("$ILIUM_URL") && gitlab.contains("$ILIUM_SHA256"));
    assert!(!gitlab.contains("https://"));
}

/// REQ: DEV-766
#[test]
fn dev_766_install_step_verifies_the_hash_before_extracting_or_running() {
    for text in [github_file(), gitlab_file()] {
        assert!(position(&text, "sha256sum --check") < position(&text, "tar -xzf"));
        assert!(position(&text, "curl") < position(&text, "sha256sum --check"));
    }
}

/// REQ: DEV-770
#[test]
fn dev_770_build_json_records_toolchain_and_input_hash() {
    let (_root, env, dir) = ci_project("none");
    build_release(&env, &dir);
    let json: serde_json::Value =
        serde_json::from_str(&read_text(&dir, "dist/build.json")).unwrap();
    assert!(
        json["toolchain"].is_object() && json["input_hash"].as_str().is_some_and(|h| h.len() == 64)
    );
}

fn build_release(env: &CliEnv, dir: &std::path::Path) -> ilium_plugin_host::cli::CliOutput {
    run(
        env,
        &["build", dir.to_str().unwrap(), "--release", "--json"],
    )
}

/// REQ: DEV-770
#[test]
fn dev_770_two_release_builds_with_the_same_toolchain_have_the_same_input_hash() {
    let (_root, env, dir) = ci_project("none");
    build_release(&env, &dir);
    let first = read_text(&dir, "dist/build.json");
    let first_json: serde_json::Value = serde_json::from_str(&first).unwrap();
    build_release(&env, &dir);
    let second: serde_json::Value =
        serde_json::from_str(&read_text(&dir, "dist/build.json")).unwrap();
    assert_eq!(first_json["input_hash"], second["input_hash"]);
}

/// REQ: DEV-771
#[test]
fn dev_771_the_project_path_does_not_change_the_built_component() {
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let (_root, env, dir) = ci_project("none");
        build_release(&env, &dir);
        hashes.push(sha256_hex(
            &std::fs::read(dir.join("dist/server.wasm")).unwrap(),
        ));
    }
    assert_eq!(hashes[0], hashes[1]);
}

/// REQ: DEV-771
#[test]
fn dev_771_package_timestamps_are_fixed_across_wall_clock_time() {
    let (_root, env, dir) = ci_project("none");
    write_dist(&dir, "server", &good_server_component());
    let first = run(
        &env,
        &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"],
    );
    let path = records_of(&first, "artifact")[0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let bytes_a = std::fs::read(&path).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    run(
        &env,
        &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"],
    );
    assert_eq!(bytes_a, std::fs::read(&path).unwrap());
}

/// REQ: DEV-772
#[test]
fn dev_772_release_build_without_a_lock_file_fails_with_lockfile_missing() {
    let (_root, env, dir) = ci_project("none");
    let _ = std::fs::remove_file(dir.join("Cargo.lock"));
    let output = build_release(&env, &dir);
    assert_eq!(
        error_code(&output).as_deref(),
        Some("toolchain.lockfile-missing")
    );
}

/// REQ: DEV-772
#[test]
fn dev_772_a_debug_build_without_a_lock_file_is_not_refused_for_it() {
    let (_root, env, dir) = ci_project("none");
    let _ = std::fs::remove_file(dir.join("Cargo.lock"));
    let output = run(&env, &["build", dir.to_str().unwrap(), "--debug", "--json"]);
    assert_ne!(
        error_code(&output).as_deref(),
        Some("toolchain.lockfile-missing")
    );
}

/// REQ: DEV-772
#[test]
fn dev_772_fresh_rust_project_ships_a_lock_file_that_is_not_ignored() {
    let (_root, _env, dir) = ci_project("none");
    assert!(tree_of(&dir).contains(&"Cargo.lock".to_owned()));
    assert!(!read_text(&dir, ".gitignore")
        .lines()
        .any(|l| l.trim() == "Cargo.lock"));
}

/// REQ: DEV-773
#[test]
fn dev_773_two_release_builds_and_packs_give_the_same_package_hash() {
    let (_root, env, dir) = ci_project("none");
    let mut hashes = Vec::new();
    for _ in 0..2 {
        build_release(&env, &dir);
        let packed = run(&env, &["pack", dir.to_str().unwrap(), "--json"]);
        let path = records_of(&packed, "artifact")[0]["path"]
            .as_str()
            .unwrap()
            .to_owned();
        hashes.push(sha256_hex(&std::fs::read(path).unwrap()));
    }
    assert_eq!(hashes[0], hashes[1]);
}

fn keyed(root: &std::path::Path, env: &CliEnv) -> (String, String) {
    let keys = root.join("keys");
    let made = run(
        env,
        &[
            "keygen",
            "--publisher-id",
            "example",
            "--publisher",
            "Example",
            "--out",
            keys.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    let secret = records_of(&made, "artifact")
        .iter()
        .find(|r| r["kind"] == "key")
        .map(|r| r["path"].as_str().unwrap().to_owned())
        .expect("key artifact");
    let file = write_package_file(
        root,
        "plain.iliumpkg",
        &unsigned_package(&good_server_component()),
    );
    (secret, file)
}

/// REQ: DEV-775
#[test]
fn dev_775_sign_reads_the_key_from_the_flag_and_prints_no_key_material() {
    let root = scratch();
    let env = env(root.path());
    let (secret, file) = keyed(root.path(), &env);
    let output = run(&env, &["sign", &file, "--key", &secret, "--json"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    let key_text = std::fs::read_to_string(&secret).unwrap();
    for line in key_text.lines().filter(|l| l.len() > 16) {
        assert!(!output.stdout_text.contains(line) && !output.stderr.contains(line));
    }
}

/// REQ: DEV-775
#[test]
fn dev_775_sign_reads_the_key_path_from_the_environment_variable() {
    let root = scratch();
    let mut env = env(root.path());
    let (secret, file) = keyed(root.path(), &env);
    env.env
        .insert("ILIUM_PLUGIN_SIGNING_KEY".to_owned(), secret);
    assert_eq!(run(&env, &["sign", &file, "--json"]).exit_code, 0);
}

/// REQ: DEV-775
#[test]
fn dev_775_sign_with_neither_flag_nor_variable_is_a_usage_error() {
    let root = scratch();
    let env = env(root.path());
    let (_secret, file) = keyed(root.path(), &env);
    let output = run(&env, &["sign", &file, "--json"]);
    assert_eq!(
        error_code(&output).as_deref(),
        Some("usage.missing-argument")
    );
}

/// REQ: DEV-775
#[test]
fn dev_775_the_generated_recipes_never_write_the_key_into_the_repository() {
    let text = github_file();
    assert!(!text.contains("BEGIN") && !text.contains("secret_key"));
    assert!(!gitlab_file().contains("BEGIN"));
}

/// REQ: DEV-776
#[test]
fn dev_776_pack_leaves_the_package_unsigned_and_needs_no_key() {
    let (_root, env, dir) = ci_project("none");
    write_dist(&dir, "server", &good_server_component());
    let packed = run(
        &env,
        &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"],
    );
    assert_eq!(packed.exit_code, 0);
    let path = records_of(&packed, "artifact")[0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let checked = check_file(&env, &path);
    assert!(has_rule(&checked, "check", "signature.absent"));
}

/// REQ: DEV-776
#[test]
fn dev_776_pack_with_a_signing_key_in_the_environment_still_does_not_sign() {
    let root = scratch();
    let mut env = env(root.path());
    let (secret, _) = keyed(root.path(), &env);
    env.env
        .insert("ILIUM_PLUGIN_SIGNING_KEY".to_owned(), secret);
    let dir = root.path().join("out");
    run(
        &env,
        &[
            "new",
            dir.to_str().unwrap(),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    write_dist(&dir, "server", &good_server_component());
    let packed = run(
        &env,
        &["pack", dir.to_str().unwrap(), "--allow-debug", "--json"],
    );
    let path = records_of(&packed, "artifact")[0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(has_rule(
        &check_file(&env, &path),
        "check",
        "signature.absent"
    ));
}

/// REQ: DEV-777 WARN
#[test]
fn dev_777_inspect_verify_of_a_signed_package_prints_the_publisher_fingerprint() {
    let root = scratch();
    let env = env(root.path());
    let (secret, file) = keyed(root.path(), &env);
    run(&env, &["sign", &file, "--key", &secret, "--json"]);
    let output = run(&env, &["inspect", &file, "--verify", "--json"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    // DEV-112: the fingerprint of a key is its `key_id`; manual does not pin the record shape
    assert!(
        output.stdout_text.contains("key_id"),
        "{}",
        output.stdout_text
    );
}

/// REQ: DEV-777 WARN
#[test]
fn dev_777_inspect_verify_of_a_tampered_package_exits_1() {
    let root = scratch();
    let env = env(root.path());
    let (secret, file) = keyed(root.path(), &env);
    run(&env, &["sign", &file, "--key", &secret, "--json"]);
    let mut bytes = std::fs::read(&file).unwrap();
    let last = bytes.len() / 2;
    bytes[last] ^= 0xff;
    std::fs::write(&file, bytes).unwrap();
    let output = run(&env, &["inspect", &file, "--verify", "--json"]);
    assert_eq!(output.exit_code, 1, "{:?}", output.stdout_jsonl);
    // DEV-801: a differing hash is reported with inspect.hash-mismatch (flipped byte may also
    // corrupt the archive; manual does not pin that case)
    assert_eq!(
        error_code(&output).as_deref(),
        Some("inspect.hash-mismatch"),
        "{:?}",
        output.stdout_jsonl
    );
}

/// REQ: DEV-778
#[test]
fn dev_778_generated_recipes_expose_no_secrets_to_pull_requests() {
    let text = github_file();
    assert!(
        !text.contains("secrets."),
        "the unsigned recipe must not read a secret"
    );
    assert!(!text.contains("pull_request_target"));
    assert!(!gitlab_file().contains("$ILIUM_PLUGIN_SIGNING_KEY"));
}

/// REQ: DEV-778
#[test]
fn dev_778_the_documented_sign_step_runs_only_on_tags() {
    let guide = std::fs::read_to_string(
        ilium_plugin_testkit::manual::pages_dir().join("guide-ci-recipes.md"),
    )
    .unwrap();
    assert!(guide.contains("if: startsWith(github.ref, 'refs/tags/v')"));
    assert!(guide.contains("rm -f \"$RUNNER_TEMP/key\""));
}
