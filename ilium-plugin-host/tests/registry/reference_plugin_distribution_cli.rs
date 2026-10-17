//! reference-plugin-distribution-cli.md: exit codes, install, update/rollback/pin/channel,
//! search/info/verify, registry/mirror/bundle, publish/yank/deprecate and the trust and signing
//! commands (PKG-423..PKG-445).

use crate::common::*;
use ilium_plugin_host::cli::CliOutput;
use proptest::prelude::*;

const NOTES: &str = "com.acme.notes";

fn key() -> String {
    publisher_key_id()
}

fn install_args<'a>(source: &'a str, key_id: &'a str) -> Vec<&'a str> {
    vec!["install", source, "--yes", "--trust", key_id]
}

fn file_install(sandbox: &Sandbox, package: &[u8], extra: &[&str]) -> CliOutput {
    let path = sandbox.project_file("candidate.iliumpkg", package);
    let key_id = key();
    let mut args = vec![
        "install",
        path.to_str().unwrap(),
        "--yes",
        "--trust",
        key_id.as_str(),
    ];
    args.extend_from_slice(extra);
    sandbox.run(&args)
}

fn write_policy(sandbox: &Sandbox, text: &str) {
    std::fs::write(sandbox.config_dir().join("policy.toml"), text).unwrap();
}

// ---- Exit codes -------------------------------------------------------------------------

/// REQ: PKG-423
#[test]
fn pkg_423_success_exits_0_and_the_result_record_says_so() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search"]);
    assert_ok(&output);
}

/// REQ: PKG-423
#[test]
fn pkg_423_each_failure_class_carries_its_exit_code_in_the_result_record() {
    let sandbox = Sandbox::standard();
    let cases: Vec<(Vec<&str>, i32)> = vec![
        (vec!["install"], 2),
        (vec!["info", "com.acme.nothing"], 5),
        (vec!["update", "com.acme.nothing"], 5),
        (vec!["rollback", NOTES], 5),
    ];
    for (args, exit) in cases {
        let output = sandbox.run(&args);
        assert_eq!(output.exit_code, exit, "{args:?}: {output:?}");
        assert_eq!(
            result_record(&output).and_then(|record| record["exit_code"].as_i64()),
            Some(i64::from(exit)),
            "{args:?}"
        );
    }
}

/// REQ: PKG-423
#[test]
fn pkg_423_network_failure_exits_8_with_a_matching_result_record() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    assert_failed(&sandbox.run(&["search"]), "registry.unreachable", 8);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// REQ: PKG-423
    #[test]
    fn pkg_423_exit_code_always_equals_the_result_exit_code_property(pick in 0usize..9) {
        let sandbox = Sandbox::standard();
        let commands: [&[&str]; 9] = [
            &["search"], &["search", "--limit", "0"], &["info", "nope.x"], &["install"],
            &["registry", "list"], &["rollback", "nope.x"], &["pin", "nope.x@1.0.0"],
            &["channel", "nope.x", "beta"], &["update", "--all"],
        ];
        let output = sandbox.run(commands[pick]);
        let result = result_record(&output);
        prop_assert!(result.is_some(), "the last record must be result");
        prop_assert_eq!(result.unwrap()["exit_code"].as_i64(), Some(i64::from(output.exit_code)));
    }
}

/// REQ: PKG-424
#[test]
fn pkg_424_a_policy_refusal_exits_7() {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        "version = 1\n[packages]\nallowed_sources = [\"/srv/managed/\"]\n",
    );
    let output = file_install(&sandbox, &signed_package("com.acme.local", "1.0.0"), &[]);
    assert_failed(&output, "policy.source-denied", 7);
}

/// REQ: PKG-424
#[test]
fn pkg_424_unreachable_and_expired_registries_exit_8() {
    let sandbox = Sandbox::standard();
    let expired = sandbox.run_at(now_plus(3 * 86_400), &["search"]);
    assert_failed(&expired, "registry.metadata-expired", 8);
    sandbox.net.set_down(true);
    assert_failed(&sandbox.run(&["search"]), "registry.unreachable", 8);
}

/// REQ: PKG-424
#[test]
fn pkg_424_a_failed_download_exits_8() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    sandbox
        .net
        .remove_file(&format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0")));
    let k = key();
    let output = sandbox.run(&install_args(NOTES, &k));
    assert_failed(&output, "install.download-failed", 8);
}

/// REQ: PKG-424
#[test]
fn pkg_424_install_needs_input_and_other_package_failures_exit_1() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("broken.iliumpkg", b"not a package");
    let output = sandbox.run(&["install", path.to_str().unwrap(), "--yes"]);
    assert_eq!(output.exit_code, 1, "{output:?}");
    let first = first_error(&output).unwrap_or_default();
    assert!(
        first.starts_with("package.") || first.starts_with("manifest."),
        "{first}"
    );
}

/// REQ: PKG-424
#[test]
fn pkg_424_not_found_codes_exit_5() {
    let sandbox = Sandbox::standard();
    assert_failed(
        &sandbox.run(&["info", "com.acme.nothing"]),
        "plugin.not-found",
        5,
    );
    assert_failed(
        &sandbox.run(&["update", "com.acme.notes"]),
        "install.not-installed",
        5,
    );
}

// ---- PKG-425 -----------------------------------------------------------------------------

/// REQ: PKG-425
#[test]
fn pkg_425_usage_errors_are_found_before_any_file_or_network_access() {
    let sandbox = Sandbox::standard();
    sandbox.net.clear_log();
    let before = sandbox.snapshot();
    for args in [
        vec!["install"],
        vec!["install", NOTES, "--channel", "nightly"],
        vec!["search", "--limit", "0"],
        vec!["search", "--category", "games"],
        vec!["yank", "com.acme.notes@1.0.0"],
        vec!["channel", NOTES, "dev"],
        vec!["registry", "add"],
        vec!["pin"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(output.exit_code, 2, "{args:?}: {output:?}");
    }
    assert!(sandbox.net.log().is_empty(), "{:?}", sandbox.net.log());
    assert_eq!(sandbox.snapshot(), before, "no file may change");
}

/// REQ: PKG-425
#[test]
fn pkg_425_a_valid_command_is_not_a_usage_error() {
    let sandbox = Sandbox::standard();
    assert_ne!(sandbox.run(&["search", "--limit", "1"]).exit_code, 2);
}

// ---- install -----------------------------------------------------------------------------

/// REQ: PKG-426
#[test]
fn pkg_426_an_existing_file_path_is_classified_before_a_registry_id() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(NOTES, &signed_package("com.acme.fromfile", "1.0.0"));
    let k = key();
    let output = sandbox.run(&["install", path.to_str().unwrap(), "--yes", "--trust", &k]);
    assert_ok(&output);
    assert_eq!(
        sandbox.installed_version("com.acme.fromfile").as_deref(),
        Some("1.0.0")
    );
    assert!(sandbox.installed_version(NOTES).is_none());
}

/// REQ: PKG-426
#[test]
fn pkg_426_an_https_url_is_downloaded() {
    let sandbox = Sandbox::standard();
    let url = "https://downloads.example.org/notes.iliumpkg";
    sandbox
        .net
        .put_file(url, signed_package("com.acme.fromurl", "1.0.0"));
    let k = key();
    let output = sandbox.run(&["install", url, "--yes", "--trust", &k]);
    assert_ok(&output);
    assert_eq!(sandbox.net.requests_to("notes.iliumpkg").len(), 1);
}

/// REQ: PKG-426
#[test]
fn pkg_426_another_scheme_fails_with_source_scheme() {
    let sandbox = Sandbox::standard();
    sandbox.net.clear_log();
    for url in [
        "http://downloads.example.org/a.iliumpkg",
        "ftp://downloads.example.org/a.iliumpkg",
        "git://example.org/a",
        "file:///tmp/a.iliumpkg",
    ] {
        let output = sandbox.run(&["install", url, "--yes"]);
        assert_failed(&output, "install.source-scheme", 1);
    }
    assert!(sandbox.net.log().is_empty());
}

/// REQ: PKG-426
#[test]
fn pkg_426_an_id_with_from_bundle_never_uses_the_network() {
    let sandbox = Sandbox::standard();
    let dir = sandbox.dir.path().join("bundle");
    std::fs::create_dir_all(&dir).unwrap();
    assert_ok(&sandbox.run(&["export-bundle", dir.to_str().unwrap(), NOTES]));
    sandbox.net.clear_log();
    sandbox.net.set_down(true);
    let k = key();
    assert_ok(&sandbox.run(&[
        "install",
        NOTES,
        "--from-bundle",
        dir.to_str().unwrap(),
        "--yes",
        "--trust",
        &k,
    ]));
    assert!(sandbox.net.log().is_empty(), "{:?}", sandbox.net.log());
}

/// REQ: PKG-427
#[test]
fn pkg_427_dry_run_changes_no_file_and_gives_the_exit_code_of_the_real_install() {
    for (package, expect_ok) in [
        (signed_package("com.acme.dry", "1.0.0"), true),
        (b"broken".to_vec(), false),
    ] {
        let sandbox = Sandbox::standard();
        let path = sandbox.project_file("dry.iliumpkg", &package);
        let k = key();
        let before = sandbox.snapshot_of("data");
        let dry = sandbox.run(&[
            "install",
            path.to_str().unwrap(),
            "--yes",
            "--trust",
            &k,
            "--dry-run",
        ]);
        assert_eq!(
            sandbox.snapshot_of("data"),
            before,
            "dry run must not touch the plugin root"
        );
        let real = sandbox.run(&["install", path.to_str().unwrap(), "--yes", "--trust", &k]);
        assert_eq!(dry.exit_code, real.exit_code, "{dry:?} {real:?}");
        assert_eq!(dry.exit_code == 0, expect_ok);
    }
}

/// REQ: PKG-427
#[test]
fn pkg_427_dry_run_prints_the_review_as_records() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("dry.iliumpkg", &signed_package("com.acme.dry", "1.0.0"));
    let k = key();
    let output = sandbox.run(&[
        "install",
        path.to_str().unwrap(),
        "--yes",
        "--trust",
        &k,
        "--dry-run",
    ]);
    assert!(
        output.stdout_jsonl.len() >= 2,
        "a review before the result: {output:?}"
    );
    assert!(
        !records(&output, "plugin").is_empty() || !records(&output, "summary").is_empty(),
        "{output:?}"
    );
}

fn write_lock(sandbox: &Sandbox, entries: serde_json::Value) -> std::path::PathBuf {
    let text = serde_json::to_string_pretty(
        &serde_json::json!({ "format": "ilium-lock/1", "plugins": entries }),
    )
    .unwrap();
    sandbox.project_file("ilium-plugins.lock", text.as_bytes())
}

/// REQ: PKG-428
#[test]
fn pkg_428_locked_installs_exactly_the_locked_version_without_resolution() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let blob = builder.blob_path(NOTES, "1.0.0");
    let package = &builder.plugins[0].versions[0].package;
    let lock = write_lock(
        &sandbox,
        serde_json::json!([{ "id": NOTES, "version": "1.0.0", "sha256": sha256_hex(package), "source": format!("{BASE}{blob}") }]),
    );
    sandbox.net.clear_log();
    let k = key();
    let output = sandbox.run(&[
        "install",
        "--locked",
        lock.to_str().unwrap(),
        "--yes",
        "--trust",
        &k,
    ]);
    assert_ok(&output);
    assert_eq!(
        sandbox.installed_version(NOTES).as_deref(),
        Some("1.0.0"),
        "not the newest 1.1.0"
    );
    assert!(
        sandbox
            .net
            .log()
            .iter()
            .all(|url| !url.ends_with("index.json") && !url.ends_with("timestamp.json")),
        "no registry resolution: {:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-428
#[test]
fn pkg_428_a_hash_that_differs_from_the_lock_fails_with_lock_mismatch() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let blob = builder.blob_path(NOTES, "1.0.0");
    let lock = write_lock(
        &sandbox,
        serde_json::json!([{ "id": NOTES, "version": "1.0.0", "sha256": "0".repeat(64), "source": format!("{BASE}{blob}") }]),
    );
    let k = key();
    let output = sandbox.run(&[
        "install",
        "--locked",
        lock.to_str().unwrap(),
        "--yes",
        "--trust",
        &k,
    ]);
    assert_failed(&output, "dependency.lock-mismatch", 1);
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-428
#[test]
fn pkg_428_a_version_that_differs_from_the_lock_fails_with_lock_mismatch() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let package = &builder.plugins[0].versions[0].package;
    // The lock says 1.1.0 but names the 1.0.0 blob.
    let lock = write_lock(
        &sandbox,
        serde_json::json!([{ "id": NOTES, "version": "1.1.0", "sha256": sha256_hex(package), "source": format!("{BASE}{}", builder.blob_path(NOTES, "1.0.0")) }]),
    );
    let k = key();
    assert_code(
        &sandbox.run(&[
            "install",
            "--locked",
            lock.to_str().unwrap(),
            "--yes",
            "--trust",
            &k,
        ]),
        "dependency.lock-mismatch",
    );
}

/// REQ: PKG-429
#[test]
fn pkg_429_yes_does_not_trust_a_new_key() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(
        "new-key.iliumpkg",
        &signed_package("com.acme.fresh", "1.0.0"),
    );
    let output = sandbox.run(&["install", path.to_str().unwrap(), "--yes"]);
    assert_failed(&output, "trust.key-unknown", 1);
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-429
#[test]
fn pkg_429_trust_with_the_exact_key_id_installs() {
    let sandbox = Sandbox::standard();
    assert_ok(&file_install(
        &sandbox,
        &signed_package("com.acme.fresh", "1.0.0"),
        &[],
    ));
    assert_eq!(
        sandbox.installed_version("com.acme.fresh").as_deref(),
        Some("1.0.0")
    );
}

/// REQ: PKG-429
#[test]
fn pkg_429_trust_with_another_key_id_does_not_trust_the_signer() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("other.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let other = key_id_of(KEY_FOREIGN);
    let output = sandbox.run(&[
        "install",
        path.to_str().unwrap(),
        "--yes",
        "--trust",
        &other,
    ]);
    assert_code(&output, "trust.key-unknown");
}

/// REQ: PKG-430
#[test]
fn pkg_430_without_a_client_and_without_yes_install_fails_with_needs_input() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("ask.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run(&["install", path.to_str().unwrap()]);
    assert_failed(&output, "install.needs-input", 1);
    assert!(sandbox.installed().is_empty());
}

// ---- update, rollback, pin, channel ------------------------------------------------------

/// REQ: PKG-431
#[test]
fn pkg_431_update_of_an_id_that_is_not_installed_exits_5() {
    let sandbox = Sandbox::standard();
    assert_failed(&sandbox.run(&["update", NOTES]), "install.not-installed", 5);
}

/// REQ: PKG-431
#[test]
fn pkg_431_update_when_already_newest_prints_a_plugin_record_and_exits_0() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.1.0", BASE, "stable", false);
    let output = sandbox.run(&["update", NOTES]);
    assert_ok(&output);
    assert_eq!(plugin_ids(&output), vec![NOTES.to_owned()]);
}

/// REQ: PKG-431
#[test]
fn pkg_431_update_installs_the_newest_allowed_version() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let k = key();
    assert_ok(&sandbox.run(&["update", NOTES, "--yes", "--trust", &k]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-432
#[test]
fn pkg_432_update_all_continues_after_a_failure_and_exits_1() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[1].versions = vec![VersionSpec::stable("com.acme.git-tools", "2.0.0")]; // not a real package
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed("com.acme.git-tools", "1.0.0", BASE, "stable", false);
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let k = key();
    let output = sandbox.run(&["update", "--all", "--yes", "--trust", &k]);
    assert_eq!(output.exit_code, 1, "{output:?}");
    assert!(!error_codes(&output).is_empty());
    assert_eq!(
        sandbox.installed_version(NOTES).as_deref(),
        Some("1.1.0"),
        "the healthy plugin is still updated"
    );
    assert_eq!(
        sandbox.installed_version("com.acme.git-tools").as_deref(),
        Some("1.0.0")
    );
}

/// REQ: PKG-432
#[test]
fn pkg_432_update_all_without_a_failure_exits_0_and_prints_one_result_per_plugin() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    sandbox.seed_installed("com.acme.git-tools", "2.0.0", BASE, "stable", false);
    let k = key();
    let output = sandbox.run(&["update", "--all", "--yes", "--trust", &k]);
    assert_ok(&output);
    let mut ids = plugin_ids(&output);
    ids.sort();
    assert_eq!(ids, vec!["com.acme.git-tools".to_owned(), NOTES.to_owned()]);
}

/// REQ: PKG-432
#[test]
fn pkg_432_update_all_skips_pinned_plugins() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", true);
    let k = key();
    assert_ok(&sandbox.run(&["update", "--all", "--yes", "--trust", &k]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: PKG-433
#[test]
fn pkg_433_rollback_without_a_previous_version_exits_5() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.1.0", BASE, "stable", false);
    assert_failed(&sandbox.run(&["rollback", NOTES]), "install.no-previous", 5);
}

/// REQ: PKG-433
#[test]
fn pkg_433_rollback_after_an_update_restores_the_previous_version() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let k = key();
    assert_ok(&sandbox.run(&["update", NOTES, "--yes", "--trust", &k]));
    assert_ok(&sandbox.run(&["rollback", NOTES]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: PKG-434
#[test]
fn pkg_434_pin_changes_only_installed_json_and_is_idempotent() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let before = sandbox.snapshot();
    assert_ok(&sandbox.run(&["pin", "com.acme.notes@1.0.0"]));
    let once = sandbox.snapshot();
    assert_ok(&sandbox.run(&["pin", "com.acme.notes@1.0.0"]));
    assert_eq!(sandbox.snapshot(), once, "pin twice is the same as once");
    let changed: Vec<&String> = once
        .keys()
        .filter(|path| before.get(*path) != once.get(*path))
        .collect();
    assert!(
        changed.iter().all(|path| path.ends_with("installed.json")),
        "{changed:?}"
    );
    assert!(sandbox
        .installed()
        .iter()
        .any(|record| record.id == NOTES && record.pinned));
}

/// REQ: PKG-434
#[test]
fn pkg_434_unpin_clears_the_pin() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", true);
    assert_ok(&sandbox.run(&["unpin", NOTES]));
    assert!(sandbox
        .installed()
        .iter()
        .any(|record| record.id == NOTES && !record.pinned));
}

/// REQ: PKG-434
#[test]
fn pkg_434_channel_changes_only_installed_json_and_is_idempotent() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let before = sandbox.snapshot();
    assert_ok(&sandbox.run(&["channel", NOTES, "beta"]));
    let once = sandbox.snapshot();
    assert_ok(&sandbox.run(&["channel", NOTES, "beta"]));
    assert_eq!(sandbox.snapshot(), once);
    let changed: Vec<&String> = once
        .keys()
        .filter(|path| before.get(*path) != once.get(*path))
        .collect();
    assert!(
        changed.iter().all(|path| path.ends_with("installed.json")),
        "{changed:?}"
    );
    assert!(sandbox
        .installed()
        .iter()
        .any(|record| record.channel == "beta"));
}

/// REQ: PKG-434
#[test]
fn pkg_434_channel_dev_is_refused_invalid_value() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let output = sandbox.run(&["channel", NOTES, "dev"]);
    assert_failed(&output, "usage.invalid-value", 2);
    assert!(sandbox
        .installed()
        .iter()
        .all(|record| record.channel == "stable"));
}

// ---- search, info, verify ----------------------------------------------------------------

/// REQ: PKG-435
#[test]
fn pkg_435_search_with_no_registry_fails_with_none_configured_and_no_network() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["search", "notes"]);
    assert_failed(&output, "registry.none-configured", 1);
    assert!(sandbox.net.log().is_empty());
}

/// REQ: PKG-435
#[test]
fn pkg_435_a_bundle_as_the_only_source_is_searched_without_the_network() {
    let builder = RegistryBuilder::standard();
    let origin = Sandbox::with_registry(&builder);
    let dir = origin.dir.path().join("bundle");
    std::fs::create_dir_all(&dir).unwrap();
    assert_ok(&origin.run(&["export-bundle", dir.to_str().unwrap(), NOTES]));
    let reader = Sandbox::new();
    let url = format!("file://{}/", dir.display());
    assert_ok(&reader.pin(&url, &builder.pinned_key_id()));
    let output = reader.run(&["search"]);
    assert_ok(&output);
    assert_eq!(plugin_ids(&output), vec![NOTES.to_owned()]);
    assert!(reader.net.log().is_empty(), "{:?}", reader.net.log());
}

/// REQ: PKG-436
#[test]
fn pkg_436_verify_changes_no_file_and_exits_0_when_stages_1_to_7_pass() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("check.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let before = sandbox.snapshot();
    let output = sandbox.run(&["verify", path.to_str().unwrap()]);
    assert_ok(&output);
    assert_eq!(sandbox.snapshot(), before);
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-436
#[test]
fn pkg_436_verify_of_a_broken_package_exits_1_and_changes_nothing() {
    let sandbox = Sandbox::standard();
    let mut package = signed_package("com.acme.fresh", "1.0.0");
    let last = package.len() / 2;
    package[last] ^= 0xFF;
    let path = sandbox.project_file("check.iliumpkg", &package);
    let before = sandbox.snapshot();
    let output = sandbox.run(&["verify", path.to_str().unwrap()]);
    assert_eq!(output.exit_code, 1, "{output:?}");
    assert_eq!(sandbox.snapshot(), before);
}

/// REQ: PKG-436
#[test]
fn pkg_436_verify_of_an_unknown_key_warns_and_still_exits_0() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("check.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run(&["verify", path.to_str().unwrap()]);
    assert_ok(&output);
    assert!(
        warning_codes(&output)
            .iter()
            .any(|code| code == "trust.key-unknown"),
        "{output:?}"
    );
}

/// REQ: PKG-437
#[test]
fn pkg_437_info_of_an_unknown_id_exits_5() {
    let sandbox = Sandbox::standard();
    assert_failed(
        &sandbox.run(&["info", "com.acme.nothing"]),
        "plugin.not-found",
        5,
    );
}

/// REQ: PKG-437
#[test]
fn pkg_437_registry_fields_appear_in_the_plugin_record() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].deprecated = Some(("Use notes2".to_owned(), "com.acme.notes2".to_owned()));
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("broken");
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert_ok(&output);
    let record = records(&output, "plugin")
        .into_iter()
        .next()
        .expect("a plugin record");
    for field in ["yanked", "deprecated", "successor"] {
        assert!(record.get(field).is_some(), "{field} missing in {record}");
    }
    assert!(record.to_string().contains("com.acme.notes2"));
}

/// REQ: PKG-437
#[test]
fn pkg_437_info_of_an_installed_plugin_works_without_a_registry() {
    let sandbox = Sandbox::new();
    sandbox.seed_installed(NOTES, "1.0.0", "file", "stable", false);
    let output = sandbox.run(&["info", NOTES]);
    assert_ok(&output);
    assert!(sandbox.net.log().is_empty());
}

// ---- registry, mirror, bundle ------------------------------------------------------------

/// REQ: PKG-438
#[test]
fn pkg_438_registry_add_without_a_root_key_fails() {
    let sandbox = Sandbox::new();
    sandbox
        .net
        .serve(BASE, &RegistryBuilder::standard().build());
    let output = sandbox.run(&["registry", "add", BASE]);
    assert_code(&output, "registry.root-key-missing");
    assert!(sandbox
        .run(&["registry", "list"])
        .stdout_jsonl
        .iter()
        .all(|record| !record.to_string().contains(BASE)));
}

/// REQ: PKG-438
#[test]
fn pkg_438_registry_add_with_a_non_https_url_fails_with_invalid_value() {
    let sandbox = Sandbox::new();
    for url in [
        "http://plugins.example.org/",
        "ftp://plugins.example.org/",
        "plugins.example.org",
    ] {
        let output = sandbox.run(&["registry", "add", url, "--root-key", &key_id_of(KEY_A)]);
        assert_failed(&output, "usage.invalid-value", 2);
    }
}

/// REQ: PKG-438
#[test]
fn pkg_438_registry_add_accepts_a_file_url_for_bundles_and_mirrors() {
    let sandbox = Sandbox::new();
    let builder = RegistryBuilder::standard();
    let folder = sandbox.dir.path().join("mirror");
    write_tree(&folder, &builder.build());
    let url = format!("file://{}/", folder.display());
    assert_ok(&sandbox.pin(&url, &builder.pinned_key_id()));
}

/// REQ: PKG-438
#[test]
fn pkg_438_registry_add_and_remove_round_trip() {
    let sandbox = Sandbox::new();
    let builder = RegistryBuilder::standard();
    sandbox.net.serve(BASE, &builder.build());
    assert_ok(&sandbox.pin(BASE, &builder.pinned_key_id()));
    assert!(output_contains(&sandbox.run(&["registry", "list"]), BASE));
    assert_ok(&sandbox.run(&["registry", "remove", BASE]));
    assert!(!output_contains(&sandbox.run(&["registry", "list"]), BASE));
}

/// REQ: PKG-439
#[test]
fn pkg_439_mirror_sync_does_not_change_a_signed_byte_and_leaves_no_temporary_file() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sandbox.dir.path().join("mirror");
    std::fs::create_dir_all(&dir).unwrap();
    assert_ok(&sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]));
    let copied = read_tree(&dir);
    let source = builder.build();
    assert!(!copied.is_empty());
    for (path, bytes) in &copied {
        assert_eq!(
            source.get(path),
            Some(bytes),
            "{path} differs from the registry"
        );
    }
    assert!(
        copied
            .keys()
            .all(|path| !path.ends_with(".tmp") && !path.ends_with(".part")),
        "{:?}",
        copied.keys().collect::<Vec<_>>()
    );
}

/// REQ: PKG-439
#[test]
fn pkg_439_a_document_with_a_bad_signature_is_not_written() {
    let mut builder = RegistryBuilder::standard();
    builder
        .overrides
        .insert(format!("plugin:{NOTES}"), vec![KEY_FOREIGN]);
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sandbox.dir.path().join("mirror");
    std::fs::create_dir_all(&dir).unwrap();
    let output = sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]);
    assert_code(&output, "registry.signature-invalid");
    assert!(!read_tree(&dir).contains_key(&format!("plugins/{NOTES}.json")));
}

/// REQ: PKG-440
#[test]
fn pkg_440_export_bundle_without_with_deps_lists_the_missing_dependency() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions =
        vec![VersionSpec::signed(NOTES, "1.0.0").depends_on("com.acme.git-tools", "^2")];
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sandbox.dir.path().join("bundle");
    std::fs::create_dir_all(&dir).unwrap();
    let output = sandbox.run(&["export-bundle", dir.to_str().unwrap(), NOTES]);
    assert_code(&output, "dependency.unsatisfied");
    assert!(
        error_message(&output, "dependency.unsatisfied").contains("com.acme.git-tools"),
        "{output:?}"
    );
}

/// REQ: PKG-440
#[test]
fn pkg_440_with_deps_adds_the_dependency_to_the_bundle() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions =
        vec![VersionSpec::signed(NOTES, "1.0.0").depends_on("com.acme.git-tools", "^2")];
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sandbox.dir.path().join("bundle");
    std::fs::create_dir_all(&dir).unwrap();
    assert_ok(&sandbox.run(&["export-bundle", dir.to_str().unwrap(), NOTES, "--with-deps"]));
    assert!(read_tree(&dir).contains_key(&builder.blob_path("com.acme.git-tools", "2.0.0")));
}

/// REQ: PKG-440
#[test]
fn pkg_440_listing_the_dependency_explicitly_satisfies_the_set() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions =
        vec![VersionSpec::signed(NOTES, "1.0.0").depends_on("com.acme.git-tools", "^2")];
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sandbox.dir.path().join("bundle");
    std::fs::create_dir_all(&dir).unwrap();
    assert_ok(&sandbox.run(&[
        "export-bundle",
        dir.to_str().unwrap(),
        NOTES,
        "com.acme.git-tools",
    ]));
}

// ---- publish, yank, deprecate ------------------------------------------------------------

fn publish_with_token(sandbox: &Sandbox, package: &[u8]) -> CliOutput {
    let path = sandbox.project_file("up.iliumpkg", package);
    sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "tok-secret-456")],
        &["publish", path.to_str().unwrap(), "--registry", BASE],
    )
}

/// REQ: PKG-441
#[test]
fn pkg_441_publish_of_an_unsigned_package_fails_with_publish_unsigned() {
    let sandbox = Sandbox::standard();
    let output = publish_with_token(&sandbox, &unsigned_package("com.acme.fresh", "1.0.0"));
    assert_failed(&output, "publish.unsigned", 1);
    assert!(sandbox
        .net
        .requests_to("/publish/com.acme.fresh/1.0.0")
        .is_empty());
}

/// REQ: PKG-441
#[test]
fn pkg_441_publish_of_an_existing_version_fails_with_version_exists() {
    let sandbox = Sandbox::standard();
    let output = publish_with_token(&sandbox, &signed_package(NOTES, "1.0.0"));
    assert_failed(&output, "registry.version-exists", 1);
}

/// REQ: PKG-441
#[test]
fn pkg_441_publish_never_prints_the_credential() {
    let sandbox = Sandbox::standard();
    for package in [
        unsigned_package("com.acme.fresh", "1.0.0"),
        signed_package(NOTES, "1.0.0"),
        signed_package("com.acme.fresh", "1.0.0"),
    ] {
        let output = publish_with_token(&sandbox, &package);
        assert!(!output_contains(&output, "tok-secret-456"), "{output:?}");
    }
}

/// REQ: PKG-441
#[test]
fn pkg_441_publish_runs_verify_first_so_a_broken_package_never_reaches_the_server() {
    let sandbox = Sandbox::standard();
    let output = publish_with_token(&sandbox, b"garbage");
    assert_ne!(output.exit_code, 0);
    assert!(
        sandbox
            .net
            .log()
            .iter()
            .all(|url| !url.contains("/publish/")),
        "{:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-442
#[test]
fn pkg_442_the_token_is_not_accepted_as_a_flag_value() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("up.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run(&[
        "publish",
        path.to_str().unwrap(),
        "--registry",
        BASE,
        "--token",
        "tok-flag-789",
    ]);
    assert_eq!(output.exit_code, 2, "{output:?}");
    assert!(
        !output_contains(&output, "tok-flag-789"),
        "the secret must not be echoed: {output:?}"
    );
    assert!(sandbox.net.log().is_empty());
}

/// REQ: PKG-442
#[test]
fn pkg_442_token_file_supplies_the_credential() {
    let sandbox = Sandbox::standard();
    let token = sandbox.project_file("token.txt", b"tok-file-321\n");
    let path = sandbox.project_file("up.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "")],
        &[
            "publish",
            path.to_str().unwrap(),
            "--registry",
            BASE,
            "--token-file",
            token.to_str().unwrap(),
        ],
    );
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "registry.auth"),
        "{output:?}"
    );
    assert!(!output_contains(&output, "tok-file-321"));
}

/// REQ: PKG-442
#[test]
fn pkg_442_missing_credential_fails_with_registry_auth() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("up.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "")],
        &["publish", path.to_str().unwrap(), "--registry", BASE],
    );
    assert_code(&output, "registry.auth");
}

/// REQ: PKG-443
#[test]
fn pkg_443_yank_and_deprecate_never_ask_for_a_blob_deletion() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.net.clear_log();
    let _ = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "t")],
        &[
            "yank",
            "com.acme.notes@1.0.0",
            "--reason",
            "x",
            "--registry",
            BASE,
        ],
    );
    let _ = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "t")],
        &[
            "deprecate",
            NOTES,
            "--message",
            "old",
            "--successor",
            "com.acme.git-tools",
            "--registry",
            BASE,
        ],
    );
    assert!(
        sandbox.net.log().iter().all(|url| !url.contains("/blobs/")),
        "{:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-443
#[test]
fn pkg_443_deprecate_needs_a_message() {
    let sandbox = Sandbox::standard();
    assert_failed(
        &sandbox.run(&["deprecate", NOTES, "--registry", BASE]),
        "usage.missing-argument",
        2,
    );
}

// ---- trust and signing -------------------------------------------------------------------

fn keygen(sandbox: &Sandbox, name: &str) -> (std::path::PathBuf, CliOutput) {
    let dir = sandbox.dir.path().join(name);
    let output = sandbox.run(&[
        "keygen",
        "--publisher-id",
        "example",
        "--publisher",
        "Example Labs",
        "--out",
        dir.to_str().unwrap(),
    ]);
    (dir, output)
}

/// REQ: PKG-444
#[test]
fn pkg_444_keygen_writes_the_file_formats_of_the_signing_page() {
    let sandbox = Sandbox::new();
    let (dir, output) = keygen(&sandbox, "keys");
    assert_ok(&output);
    let secret = std::fs::read_to_string(dir.join("publisher.key")).unwrap();
    assert!(
        secret.starts_with("-----BEGIN PRIVATE KEY-----"),
        "PKCS#8 PEM expected"
    );
    let public: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("publisher.pub")).unwrap()).unwrap();
    assert_eq!(public["format"], "ilium-pub/1");
    assert_eq!(public["algorithm"], "ed25519");
    assert_eq!(public["publisher_id"], "example");
    let raw = {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(public["public_key"].as_str().unwrap())
            .unwrap()
    };
    assert_eq!(raw.len(), 32);
    assert_eq!(public["key_id"].as_str().unwrap(), sha256_hex(&raw));
}

/// REQ: PKG-444
#[test]
fn pkg_444_keygen_refuses_to_overwrite_with_key_exists() {
    let sandbox = Sandbox::new();
    assert_ok(&keygen(&sandbox, "keys").1);
    let (_, again) = keygen(&sandbox, "keys");
    assert_failed(&again, "trust.key-exists", 1);
}

/// REQ: PKG-444
#[test]
fn pkg_444_trust_add_refuses_a_public_key_file_with_another_key_id() {
    let sandbox = Sandbox::new();
    let (dir, output) = keygen(&sandbox, "keys");
    assert_ok(&output);
    let path = dir.join("publisher.pub");
    let mut public: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    public["key_id"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(&path, serde_json::to_vec_pretty(&public).unwrap()).unwrap();
    assert_failed(
        &sandbox.run(&["trust", "add", path.to_str().unwrap(), "--yes"]),
        "trust.key-malformed",
        1,
    );
}

/// REQ: PKG-444
#[test]
fn doc_pkg_444_the_distribution_page_shows_no_other_key_format() {
    let text = page_text("reference-plugin-distribution-cli.md");
    for wrong in [
        "BEGIN OPENSSH PRIVATE KEY",
        "ssh-ed25519",
        "\"format\": \"ilium-key",
        "BEGIN EC PRIVATE KEY",
    ] {
        assert!(!text.contains(wrong), "{wrong}");
    }
    assert!(text.contains("Signing and trust"));
}

/// REQ: PKG-445
#[test]
fn pkg_445_trust_failures_exit_1() {
    let sandbox = Sandbox::new();
    let (dir, output) = keygen(&sandbox, "keys");
    assert_ok(&output);
    let (_, again) = keygen(&sandbox, "keys");
    assert_eq!(again.exit_code, 1);
    let bad = sandbox.project_file("bad.pub", b"{ \"format\": \"ilium-pub/9\" }");
    let malformed = sandbox.run(&["trust", "add", bad.to_str().unwrap(), "--yes"]);
    assert_eq!(malformed.exit_code, 1, "{malformed:?}");
    let _ = dir;
}

/// REQ: PKG-445
#[test]
fn pkg_445_trust_not_found_exits_5() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["trust", "remove", &"a".repeat(64)]);
    assert_failed(&output, "trust.not-found", 5);
    let export = sandbox.run(&["trust", "export", &"b".repeat(64)]);
    assert_failed(&export, "trust.not-found", 5);
}

/// REQ: PKG-445
#[test]
fn pkg_445_trust_remove_of_a_key_in_use_exits_1() {
    let sandbox = Sandbox::standard();
    assert_ok(&file_install(
        &sandbox,
        &signed_package("com.acme.fresh", "1.0.0"),
        &[],
    ));
    let output = sandbox.run(&["trust", "remove", &key()]);
    assert_failed(&output, "trust.in-use", 1);
}

/// REQ: PKG-445
#[test]
fn doc_pkg_445_a_declined_prompt_exits_7() {
    let text = page_text("reference-plugin-distribution-cli.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-445]"))
        .expect("row");
    assert!(
        row.contains("`trust.not-found` (5)") && row.contains("a declined prompt (7)"),
        "{row}"
    );
}
