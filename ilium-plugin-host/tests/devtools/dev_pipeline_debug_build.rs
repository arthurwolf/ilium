//! Debug builds, the debugger endpoint and the reproducible-build checks (DEV-876, DEV-877,
//! DEV-892, DEV-893). The build and endpoint tests need the real toolchain, so they stay red until
//! the build backend runs; they assert the outputs the manual requires.

use std::path::PathBuf;

use ilium_plugin_host::cli::CliEnv;
use serde_json::Value;

use crate::common::*;

/// A Rust server project made by `ilium plugin new`, with its environment and project folder.
fn new_rust_project() -> (tempfile::TempDir, CliEnv, PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8 path"),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

/// True when `needle` occurs in `haystack` as a contiguous byte sequence.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// REQ: DEV-876
#[test]
fn debug_build_keeps_dwarf_and_writes_debug_record_with_input_hash() {
    let (_root, env, dir) = new_rust_project();
    let built = run_in_project(&env, "build", &[]);
    assert_eq!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    let component = std::fs::read(dir.join("dist/server.wasm")).expect("debug component");
    assert!(
        contains_bytes(&component, b".debug_info"),
        "a debug build keeps DWARF sections"
    );
    let record_name = tree_of(&dir.join("dist"))
        .into_iter()
        .find(|path| path.ends_with(".debug.json"))
        .expect("dist/<name>.debug.json is written");
    let record: Value =
        serde_json::from_str(&read_text(&dir.join("dist"), &record_name)).expect("debug json");
    assert!(
        record.get("input_hash").is_some(),
        "record names input_hash"
    );
}

/// REQ: DEV-876
#[test]
fn pack_refuses_debug_component_with_package_debug_build() {
    let (_root, env, _dir) = new_rust_project();
    let built = run_in_project(&env, "build", &[]);
    assert_eq!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    let packed = run_in_project(&env, "pack", &[]);
    assert_eq!(error_code(&packed).as_deref(), Some("package.debug-build"));
    assert_ne!(
        packed.exit_code, 0,
        "pack must not accept a debug component"
    );
}

/// REQ: DEV-877
#[test]
fn debug_session_prints_loopback_debug_endpoint_artifact() {
    let (_root, env, _dir) = new_rust_project();
    let started = run_in_project(&env, "debug", &[]);
    assert_eq!(started.exit_code, 0, "{:?}", started.stdout_jsonl);
    let endpoint = records_of(&started, "artifact")
        .into_iter()
        .find(|record| record.get("kind").and_then(Value::as_str) == Some("debug-endpoint"))
        .expect("an artifact record of kind debug-endpoint");
    let text = endpoint.to_string();
    assert!(
        text.contains("127.0.0.1") || text.contains("[::1]"),
        "the endpoint is on a loopback address: {text}"
    );
}

/// REQ: DEV-892
#[test]
fn verify_reproducible_exits_zero_on_clean_fixture() {
    let (_root, env, _dir) = new_rust_project();
    let checked = run_in_project(&env, "verify-reproducible", &[]);
    assert_eq!(checked.exit_code, 0, "{:?}", checked.stdout_jsonl);
    assert!(
        error_code(&checked).is_none(),
        "no error record on identical builds"
    );
}

/// REQ: DEV-893
#[test]
fn verify_reproducible_refuses_missing_lockfile_before_any_build() {
    let (_root, env, dir) = new_rust_project();
    std::fs::remove_file(dir.join("Cargo.lock")).expect("remove the lockfile");
    let output = run_in_project(&env, "verify-reproducible", &[]);
    assert_eq!(error_code(&output).as_deref(), Some("build.unpinned-input"));
    assert!(!dir.join("target").exists(), "no build folder is created");
    assert!(!dir.join("dist").exists(), "no component is built");
}

/// REQ: DEV-877
#[test]
fn debug_endpoint_artifact_never_names_the_unspecified_address() {
    let (_root, env, _dir) = new_rust_project();
    let started = run_in_project(&env, "debug", &[]);
    let endpoint = records_of(&started, "artifact")
        .into_iter()
        .find(|record| record.get("kind").and_then(Value::as_str) == Some("debug-endpoint"))
        .expect("an artifact record of kind debug-endpoint");
    assert!(
        !endpoint.to_string().contains("0.0.0.0"),
        "the endpoint is not bound to all interfaces"
    );
}

/// REQ: DEV-892
#[test]
fn verify_reproducible_twice_on_the_clean_fixture_gives_the_same_exit_code() {
    let (_root, env, _dir) = new_rust_project();
    let first = run_in_project(&env, "verify-reproducible", &[]);
    let second = run_in_project(&env, "verify-reproducible", &[]);
    assert_eq!(first.exit_code, second.exit_code);
}

/// REQ: DEV-893
#[test]
fn verify_reproducible_does_not_refuse_a_project_with_its_lockfile() {
    let (_root, env, _dir) = new_rust_project();
    let output = run_in_project(&env, "verify-reproducible", &[]);
    assert_ne!(error_code(&output).as_deref(), Some("build.unpinned-input"));
}
