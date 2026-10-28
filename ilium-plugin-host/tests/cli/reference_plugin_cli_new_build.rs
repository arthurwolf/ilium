//! reference-plugin-cli.md: `new` and `build` (DEV-040..055).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use ilium_plugin_host::cli::{DevEvent, MemorySession};

use crate::common::*;

fn new_args<'a>(dir: &'a str, id: &'a str, lang: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["new", dir, "--id", id, "--lang", lang];
    args.extend_from_slice(extra);
    args
}

/// REQ: DEV-040
#[test]
fn dev_040_missing_id_is_a_usage_error() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
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
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-040
#[test]
fn dev_040_missing_lang_is_a_usage_error() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8"),
            "--id",
            "acme.p",
            "--template",
            "server",
        ],
    );
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
}

/// REQ: DEV-040
#[test]
fn dev_040_template_or_side_is_required() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(dir.to_str().expect("utf8"), "acme.p", "rust", &[]),
    );
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
    assert!(!dir.exists());
}

/// REQ: DEV-040
#[test]
fn dev_040_side_alone_names_the_template() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--side", "client"],
        ),
    );
    assert_eq!(output.exit_code, 0);
    assert_eq!(read_json(&dir.join("manifest.json"))["id"], "acme.p");
}

/// REQ: DEV-040
#[test]
fn dev_040_template_and_side_naming_the_same_template_are_accepted() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server", "--side", "server"],
        ),
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-040
#[test]
fn dev_040_template_and_side_naming_different_templates_conflict() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server", "--side", "client"],
        ),
    );
    assert_eq!(code(&output).as_deref(), Some("usage.conflict"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-041
#[test]
fn dev_041_non_empty_dir_fails_dir_not_empty() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::write(dir.join("keep.txt"), "x").expect("write");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server"],
        ),
    );
    assert_eq!(code(&output).as_deref(), Some("new.dir-not-empty"));
    assert_eq!(tree_listing(&dir), ["keep.txt"]);
}

/// REQ: DEV-041
#[test]
fn dev_041_an_existing_empty_dir_is_fine() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    std::fs::create_dir_all(&dir).expect("create");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server"],
        ),
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-041
#[test]
fn dev_041_force_overwrites_template_files_and_keeps_others() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::write(dir.join("README.md"), "mine").expect("write");
    std::fs::write(dir.join("notes.txt"), "mine too").expect("write");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server", "--force"],
        ),
    );
    assert_eq!(output.exit_code, 0);
    assert_ne!(
        std::fs::read_to_string(dir.join("README.md")).expect("read"),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).expect("read"),
        "mine too"
    );
}

/// REQ: DEV-042
#[test]
fn dev_042_ids_that_do_not_match_the_pattern_are_invalid() {
    let root = scratch();
    let env = env(root.path());
    for bad in [
        "Acme.p", "acme", "acme..p", "acme.p_q", ".acme.p", "acme.p.", "acme. p",
    ] {
        let dir = root.path().join("p");
        let output = run(
            &env,
            &new_args(
                dir.to_str().expect("utf8"),
                bad,
                "rust",
                &["--template", "server"],
            ),
        );
        assert_eq!(code(&output).as_deref(), Some("new.id-invalid"), "{bad}");
        assert!(!dir.exists(), "{bad}");
    }
}

/// REQ: DEV-042
#[test]
fn dev_042_ids_that_match_the_pattern_are_accepted() {
    let root = scratch();
    let env = env(root.path());
    for (index, good) in ["acme.p", "a-b.c-d.e9", "x1.y2"].iter().enumerate() {
        let dir = root.path().join(format!("p{index}"));
        let output = run(
            &env,
            &new_args(
                dir.to_str().expect("utf8"),
                good,
                "rust",
                &["--template", "server"],
            ),
        );
        assert_eq!(output.exit_code, 0, "{good}");
    }
}

/// REQ: DEV-042
#[test]
fn dev_042_ilium_ids_are_reserved_without_first_party() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "ilium.badges",
            "rust",
            &["--template", "server"],
        ),
    );
    assert_eq!(code(&output).as_deref(), Some("new.id-reserved"));
}

/// REQ: DEV-042
#[test]
fn dev_042_first_party_allows_an_ilium_id() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "ilium.badges",
            "rust",
            &["--template", "core-style", "--first-party"],
        ),
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-043
#[test]
fn dev_043_unknown_template_fails_before_writing_any_file() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "banana"],
        ),
    );
    assert_eq!(code(&output).as_deref(), Some("new.template-unknown"));
    assert!(!dir.exists());
}

/// REQ: DEV-043
#[test]
fn dev_043_every_listed_template_is_known() {
    let root = scratch();
    let env = env(root.path());
    for (index, template) in [
        "server",
        "client",
        "both",
        "scene",
        "pane-kind",
        "service-provider",
        "extension",
    ]
    .iter()
    .enumerate()
    {
        let dir = root.path().join(format!("t{index}"));
        let output = run(
            &env,
            &new_args(
                dir.to_str().expect("utf8"),
                "acme.t",
                "rust",
                &["--template", template],
            ),
        );
        assert_ne!(
            code(&output).as_deref(),
            Some("new.template-unknown"),
            "{template}"
        );
    }
}

/// REQ: DEV-044
#[test]
fn dev_044_one_inventory_progress_per_written_file_with_the_relative_path() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server"],
        ),
    );
    let mut announced: Vec<String> = recs(&output, "progress")
        .iter()
        .filter(|r| r["step"] == "inventory")
        .map(|r| r["message"].as_str().expect("message").to_owned())
        .collect();
    announced.sort();
    assert_eq!(announced, tree_listing(&dir));
}

/// REQ: DEV-044
#[test]
fn dev_044_no_artifact_record() {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server"],
        ),
    );
    assert!(recs(&output, "artifact").is_empty());
}

/// REQ: DEV-044
#[test]
fn dev_044_needs_no_network_and_no_toolchain() {
    let root = scratch();
    let fetcher = Arc::new(CountingFetcher::default());
    let mut probe = FakeProbe::default();
    probe.versions.clear();
    probe.rust_target = false;
    let mut env = env(root.path()).with_probe(Arc::new(probe));
    env.fetcher = fetcher.clone();
    let dir = root.path().join("p");
    let output = run(
        &env,
        &new_args(
            dir.to_str().expect("utf8"),
            "acme.p",
            "rust",
            &["--template", "server"],
        ),
    );
    assert_eq!(output.exit_code, 0);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

/// REQ: DEV-045
#[test]
fn dev_045_fresh_rust_project_passes_every_gate_without_warnings() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = server_project(&env, root.path());
    for args in [
        &["build"][..],
        &["check", "--json"],
        &["lint", "--json", "--deny", "warnings"],
        &["test"],
    ] {
        let output = run_in(&env, &dir, args);
        assert_eq!(output.exit_code, 0, "{args:?}: {:?}", output.stdout_jsonl);
        assert_eq!(recs(&output, "summary")[0]["warnings"], 0, "{args:?}");
    }
}

/// REQ: DEV-045
#[test]
fn dev_045_fresh_typescript_project_passes_every_gate_without_warnings() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = new_project(&env, root.path(), "tsdemo", "server", "typescript");
    for args in [
        &["build"][..],
        &["check", "--json"],
        &["lint", "--json", "--deny", "warnings"],
        &["test"],
    ] {
        let output = run_in(&env, &dir, args);
        assert_eq!(output.exit_code, 0, "{args:?}: {:?}", output.stdout_jsonl);
        assert_eq!(recs(&output, "summary")[0]["warnings"], 0, "{args:?}");
    }
}

/// REQ: DEV-050
#[test]
fn dev_050_builds_every_declared_side() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "both", "both", "rust");
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        tree_listing(&dir.join("dist"))
            .into_iter()
            .filter(|f| f.ends_with(".wasm"))
            .collect::<Vec<_>>(),
        ["client.wasm", "server.wasm"]
    );
}

/// REQ: DEV-050
#[test]
fn dev_050_side_flag_builds_only_that_side() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "both", "both", "rust");
    let output = run_in(&env, &dir, &["build", "--side", "client"]);
    assert_eq!(output.exit_code, 0);
    assert!(dir.join("dist/client.wasm").is_file());
    assert!(!dir.join("dist/server.wasm").exists());
}

/// REQ: DEV-050
#[test]
fn dev_050_undeclared_side_fails_side_undeclared() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--side", "scene"]);
    assert_eq!(code(&output).as_deref(), Some("build.side-undeclared"));
    assert_eq!(output.exit_code, 1);
    assert!(!dir.join("dist/scene.wasm").exists());
}

/// REQ: DEV-051
#[test]
fn dev_051_one_component_artifact_per_side_and_one_build_info() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "both", "both", "rust");
    let output = run_in(&env, &dir, &["build"]);
    let mut paths: Vec<&str> = artifacts(&output, "component")
        .iter()
        .map(|r| r["path"].as_str().expect("path"))
        .collect();
    paths.sort();
    assert_eq!(paths, ["dist/client.wasm", "dist/server.wasm"]);
    let info = artifacts(&output, "build-info");
    assert_eq!(info.len(), 1);
    assert_eq!(info[0]["path"], "dist/build.json");
}

/// REQ: DEV-051
#[test]
fn dev_051_typescript_debug_build_prints_a_source_map_per_side() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "tsboth", "both", "typescript");
    let output = run_in(&env, &dir, &["build", "--debug"]);
    assert_eq!(artifacts(&output, "source-map").len(), 2);
}

/// REQ: DEV-051
#[test]
fn dev_051_release_build_prints_no_source_map() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "tsdemo", "server", "typescript");
    let output = run_in(&env, &dir, &["build", "--release"]);
    assert!(artifacts(&output, "source-map").is_empty());
}

/// REQ: DEV-051
#[test]
fn dev_051_rust_debug_build_prints_no_source_map_artifact() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--debug"]);
    assert!(artifacts(&output, "source-map").is_empty());
}

/// REQ: DEV-052
#[test]
fn dev_052_watch_builds_once_then_ends_only_on_interrupt_with_130() {
    let root = scratch();
    let builder = FakeBuilder::ok();
    let env = env(root.path()).with_builder(builder.clone());
    let dir = server_project(&env, root.path());
    let env = env.with_dev_script(vec![DevEvent::Interrupt { at_ms: 1000 }]);
    let output = run_in(&env, &dir, &["build", "--watch"]);
    assert_eq!(output.exit_code, 130);
    assert_eq!(builder.builds.load(Ordering::SeqCst), 1);
}

/// REQ: DEV-052
#[test]
fn dev_052_rebuilds_300_ms_after_the_last_change() {
    let root = scratch();
    let builder = FakeBuilder::ok();
    let env = env(root.path()).with_builder(builder.clone());
    let dir = server_project(&env, root.path());
    let script = vec![
        DevEvent::Change {
            at_ms: 1000,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Change {
            at_ms: 1200,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Interrupt { at_ms: 1499 },
    ];
    run_in(
        &env.clone().with_dev_script(script),
        &dir,
        &["build", "--watch"],
    );
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        1,
        "no rebuild before 300 ms after the last change (1500)"
    );
    let builder = FakeBuilder::ok();
    let script = vec![
        DevEvent::Change {
            at_ms: 1000,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Change {
            at_ms: 1200,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Interrupt { at_ms: 1501 },
    ];
    run_in(
        &env.with_builder(builder.clone()).with_dev_script(script),
        &dir,
        &["build", "--watch"],
    );
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        2,
        "one rebuild at 1500 for both changes"
    );
}

/// REQ: DEV-052
#[test]
fn dev_052_watch_never_installs_or_contacts_a_session() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let dir = server_project(&env, root.path());
    let env = env.with_dev_script(vec![DevEvent::Interrupt { at_ms: 1000 }]);
    run_in(&env, &dir, &["build", "--watch"]);
    assert_eq!(session.connect_attempts(), 0);
    assert!(session.journal().is_empty());
}

/// REQ: DEV-052
#[test]
fn dev_052_a_failed_rebuild_prints_errors_and_keeps_watching() {
    let root = scratch();
    let builder = FakeBuilder::failing_build(2);
    let env = env(root.path()).with_builder(builder.clone());
    let dir = server_project(&env, root.path());
    let script = vec![
        DevEvent::Change {
            at_ms: 1000,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Change {
            at_ms: 2000,
            path: "src/lib.rs".to_owned(),
        },
        DevEvent::Interrupt { at_ms: 3000 },
    ];
    let output = run_in(&env.with_dev_script(script), &dir, &["build", "--watch"]);
    assert!(
        !recs(&output, "error").is_empty(),
        "the failed rebuild printed its errors"
    );
    assert_eq!(
        builder.builds.load(Ordering::SeqCst),
        3,
        "the third build ran after the failure"
    );
    assert_eq!(output.exit_code, 130);
}

/// REQ: DEV-053
#[test]
fn dev_053_locked_without_a_lockfile_fails_lockfile_missing() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let _ = std::fs::remove_file(dir.join("Cargo.lock"));
    let output = run_in(&env, &dir, &["build", "--locked"]);
    assert_eq!(code(&output).as_deref(), Some("build.lockfile-missing"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-053
#[test]
fn dev_053_typescript_locked_without_bun_lock_fails_lockfile_missing() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "tsdemo", "server", "typescript");
    let _ = std::fs::remove_file(dir.join("bun.lock"));
    let output = run_in(&env, &dir, &["build", "--locked"]);
    assert_eq!(code(&output).as_deref(), Some("build.lockfile-missing"));
}

/// REQ: DEV-053
#[test]
fn dev_053_locked_fails_when_building_would_change_the_lockfile() {
    let root = scratch();
    let env = env(root.path()).with_builder(FakeBuilder::with_outdated_lock());
    let dir = server_project(&env, root.path());
    std::fs::write(dir.join("Cargo.lock"), "# lock\n").expect("write");
    let output = run_in(&env, &dir, &["build", "--locked"]);
    assert_eq!(code(&output).as_deref(), Some("build.lockfile-outdated"));
}

/// REQ: DEV-053
#[test]
fn dev_053_without_locked_a_missing_lockfile_is_fine() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let _ = std::fs::remove_file(dir.join("Cargo.lock"));
    assert_eq!(run_in(&env, &dir, &["build"]).exit_code, 0);
}

/// REQ: DEV-054
#[test]
fn dev_054_offline_with_an_uncached_dependency_fails() {
    let root = scratch();
    let env = env(root.path()).with_builder(FakeBuilder::with_uncached_dependency("serde"));
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build", "--offline"]);
    assert_eq!(code(&output).as_deref(), Some("build.offline-missing"));
}

/// REQ: DEV-054
#[test]
fn dev_054_offline_with_every_dependency_cached_builds() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    assert_eq!(run_in(&env, &dir, &["build", "--offline"]).exit_code, 0);
}

/// REQ: DEV-054
#[test]
fn dev_054_the_offline_variable_has_the_same_effect_as_the_flag() {
    let root = scratch();
    let env = env(root.path())
        .with_builder(FakeBuilder::with_uncached_dependency("serde"))
        .with_var("ILIUM_PLUGIN_OFFLINE", "1");
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(code(&output).as_deref(), Some("build.offline-missing"));
}

/// REQ: DEV-055
#[test]
fn dev_055_missing_rustc_exits_four() {
    let root = scratch();
    let env = env(root.path()).with_probe(Arc::new(FakeProbe::without("rustc")));
    let dir = server_project(&env, root.path());
    assert_eq!(run_in(&env, &dir, &["build"]).exit_code, 4);
}

/// REQ: DEV-055
#[test]
fn dev_055_missing_wasm_target_exits_four_with_a_toolchain_code() {
    let root = scratch();
    let probe = FakeProbe {
        rust_target: false,
        ..Default::default()
    };
    let env = env(root.path()).with_probe(Arc::new(probe));
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["build"]);
    assert_eq!(code(&output).as_deref(), Some("toolchain.rust-target"));
    assert_eq!(output.exit_code, 4);
}

/// REQ: DEV-055
#[test]
fn dev_055_missing_bun_exits_four_for_typescript() {
    let root = scratch();
    let env = env(root.path()).with_probe(Arc::new(FakeProbe::without("bun")));
    let dir = new_project(&env, root.path(), "tsdemo", "server", "typescript");
    assert_eq!(run_in(&env, &dir, &["build"]).exit_code, 4);
}

/// REQ: DEV-055
#[test]
fn dev_055_compile_bundle_and_component_failures_exit_one() {
    let root = scratch();
    for (stage, code_name, lang) in [
        ("compile", "build.compile-failed", "rust"),
        ("bundle", "build.bundle-failed", "typescript"),
        ("component", "build.component-failed", "rust"),
    ] {
        let env = env(root.path()).with_builder(FakeBuilder::failing_at(stage));
        let dir = new_project(&env, root.path(), &format!("p-{stage}"), "server", lang);
        let output = run_in(&env, &dir, &["build"]);
        assert_eq!(code(&output).as_deref(), Some(code_name), "{stage}");
        assert_eq!(output.exit_code, 1, "{stage}");
    }
}
