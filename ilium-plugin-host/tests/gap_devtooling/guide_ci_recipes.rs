//! CI recipes: dependency audit, cache keys, host-contract matrix and artifact upload (DEV-896,
//! DEV-932, DEV-933, DEV-934). Page: guide-ci-recipes.

use crate::common::*;
use crate::{
    file_sha256, manifest_requires_range_bounds, manual_page, new_rust_project,
    pack_with_warm_cache, plant_locked_dependency_with_advisory, with_code,
};

/// REQ: DEV-896
#[test]
fn publish_fails_on_vulnerable_locked_dependency_and_passes_with_allow_advisory() {
    let (_root, env, dir) = new_rust_project();
    let advisory = plant_locked_dependency_with_advisory(&env, "example-crate", "1.0.0");
    let denied = run_in_project(&env, "publish", &["--fail-severity", "high"]);
    assert_ne!(
        denied.exit_code, 0,
        "a vulnerable dependency must fail publish"
    );
    let matches = with_code(&denied, "lint.dependency-vulnerable");
    assert!(
        !matches.is_empty(),
        "each match is a lint.dependency-vulnerable diagnostic"
    );
    assert_eq!(
        matches[0]["advisory"].as_str(),
        Some(advisory.as_str()),
        "{}",
        matches[0]
    );
    assert!(
        matches[0]["fixed_version"].is_string(),
        "the record names the fixed version"
    );
    let allowed = run_in_project(
        &env,
        "publish",
        &["--fail-severity", "high", "--allow-advisory", &advisory],
    );
    assert_eq!(
        allowed.exit_code, 0,
        "--allow-advisory accepts the named advisory: {:?}",
        allowed.stdout_jsonl
    );
    let offline = run_in_project(&env, "check", &["--offline"]);
    assert!(
        !with_code(&offline, "lint.dependency-vulnerable").is_empty(),
        "--offline checks against the cached advisory database"
    );
    assert!(dir.join("manifest.json").is_file());
}

/// REQ: DEV-932
#[test]
fn ci_recipe_names_cache_paths_and_keys_and_never_caches_dist() {
    let page = manual_page("guide-ci-recipes");
    for needle in [
        "~/.cargo/registry",
        "~/.cargo/git",
        "Cargo.lock",
        "~/.bun/install/cache",
        "bun.lock",
    ] {
        assert!(page.contains(needle), "the recipe names {needle}");
    }
    for line in page
        .lines()
        .filter(|line| line.contains("dist/") && line.to_lowercase().contains("cache"))
    {
        let lowered = line.to_lowercase();
        assert!(
            lowered.contains("not") || lowered.contains("never"),
            "a line about caching dist/ must forbid it: {line}"
        );
    }
    let (_root, env, dir) = new_rust_project();
    let cold = run_in_project(&env, "pack", &[]);
    assert_eq!(
        cold.exit_code, 0,
        "cold-cache pack: {:?}",
        cold.stdout_jsonl
    );
    let cold_hash = file_sha256(&dir.join("dist").join("server.plugin.iliumpkg"));
    std::fs::remove_dir_all(dir.join("dist")).expect("remove dist before the warm build");
    let warm_hash = pack_with_warm_cache(&env);
    assert_eq!(
        cold_hash, warm_hash,
        "a pack from a cold cache is byte-identical to a warm one"
    );
}

/// REQ: DEV-933
#[test]
fn ci_recipe_runs_test_and_conformance_on_oldest_and_newest_host_contract() {
    let page = manual_page("guide-ci-recipes");
    assert!(
        page.contains("ilium plugin test --host-contract"),
        "the recipe uses --host-contract"
    );
    assert!(page.contains("conformance"), "the recipe runs conformance");
    let (_root, env, _dir) = new_rust_project();
    let refused = run_in_project(&env, "test", &["--host-contract", "0.0.1"]);
    assert_eq!(refused.exit_code, 2, "{:?}", refused.stdout_jsonl);
    assert_eq!(
        error_code(&refused).as_deref(),
        Some("usage.host-contract-unavailable"),
        "{:?}",
        refused.stdout_jsonl
    );
    let (oldest, newest) = manifest_requires_range_bounds(&env);
    for version in [oldest, newest] {
        let out = run_in_project(&env, "test", &["--host-contract", version.as_str()]);
        assert_eq!(
            out.exit_code, 0,
            "test runs on the contract {version}: {:?}",
            out.stdout_jsonl
        );
    }
}

/// REQ: DEV-934
#[test]
fn ci_recipe_uploads_required_artifacts_on_every_run_and_never_the_signing_key() {
    let page = manual_page("guide-ci-recipes");
    let upload_lines: Vec<&str> = page
        .lines()
        .filter(|line| line.to_lowercase().contains("upload"))
        .collect();
    assert!(!upload_lines.is_empty(), "the recipe has an upload step");
    for required in ["dist/build.json", "conformance", "publisher.pub"] {
        assert!(
            upload_lines.iter().any(|line| line.contains(required)),
            "the upload step names {required}"
        );
    }
    assert!(
        upload_lines
            .iter()
            .any(|line| line.to_lowercase().contains("every run")
                || line.to_lowercase().contains("always")),
        "the upload runs on success and failure"
    );
    let forbidden_lines: Vec<&str> = page
        .lines()
        .filter(|line| line.contains("publisher.key") || line.contains("dist/*.tmp"))
        .collect();
    assert!(
        !forbidden_lines.is_empty(),
        "the recipe states the excluded files"
    );
    assert!(
        page.to_lowercase().contains("must not upload")
            || page.to_lowercase().contains("never upload"),
        "the recipe forbids uploading publisher.key, dist/*.tmp and the signing environment"
    );
    assert!(
        upload_lines
            .iter()
            .all(|line| !line.contains("publisher.key") || line.to_lowercase().contains("not")),
        "no upload line uploads publisher.key"
    );
}
