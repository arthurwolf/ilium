//! reference-plugin-cli.md "The build pipeline" (DEV-300..307). Channels: build JSONL, `dist/`
//! contents, `dist/build.json`. The build runs through the host's build backend (API_GAPS.md).

use ilium_plugin_host::cli::{CliEnv, CliOutput};
use serde_json::Value;

use crate::common::*;

const STEPS: [&str; 6] = [
    "toolchain",
    "compile",
    "bundle",
    "component",
    "api-version",
    "strip",
];

fn new_project(lang: &str, template: &str) -> (tempfile::TempDir, CliEnv, std::path::PathBuf) {
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
            lang,
            "--template",
            template,
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

fn build(env: &CliEnv, dir: &std::path::Path, extra: &[&str]) -> CliOutput {
    let mut args = vec!["build", dir.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    run(env, &args)
}

fn steps_of(output: &CliOutput) -> Vec<String> {
    records_of(output, "progress")
        .iter()
        .filter_map(|r| r["step"].as_str().map(str::to_owned))
        .collect()
}

fn build_json(dir: &std::path::Path) -> Value {
    serde_json::from_str(&read_text(dir, "dist/build.json")).expect("build.json")
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// REQ: DEV-300
#[test]
fn dev_300_rust_build_prints_a_progress_record_per_stage_in_order() {
    let (_root, env, dir) = new_project("rust", "server");
    let steps = steps_of(&build(&env, &dir, &[]));
    let positions: Vec<usize> = STEPS
        .iter()
        .filter_map(|s| steps.iter().position(|x| x == s))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{steps:?}"
    );
    assert_eq!(steps.first().map(String::as_str), Some("toolchain"));
    assert_eq!(steps.last().map(String::as_str), Some("strip"));
}

/// REQ: DEV-300
#[test]
fn dev_300_typescript_build_uses_bundle_not_compile() {
    let (_root, env, dir) = new_project("typescript", "server");
    let steps = steps_of(&build(&env, &dir, &[]));
    assert!(
        steps.contains(&"bundle".to_owned()) && !steps.contains(&"compile".to_owned()),
        "{steps:?}"
    );
}

/// REQ: DEV-300
#[test]
fn dev_300_a_failing_stage_stops_the_later_stages() {
    let (_root, env, dir) = new_project("rust", "server");
    write_text(&dir, "src/lib.rs", "this is not rust");
    let output = build(&env, &dir, &[]);
    assert_eq!(error_code(&output).as_deref(), Some("build.compile-failed"));
    let steps = steps_of(&output);
    assert!(
        !steps.contains(&"component".to_owned()) && !steps.contains(&"strip".to_owned()),
        "{steps:?}"
    );
}

/// REQ: DEV-301
#[test]
fn dev_301_one_component_per_declared_side_with_fixed_names() {
    let (_root, env, dir) = new_project("rust", "both");
    build(&env, &dir, &[]);
    let dist: Vec<String> = tree_of(&dir)
        .into_iter()
        .filter(|p| p.starts_with("dist/") && p.ends_with(".wasm"))
        .collect();
    assert_eq!(
        dist,
        vec!["dist/client.wasm".to_owned(), "dist/server.wasm".to_owned()]
    );
}

/// REQ: DEV-301
#[test]
fn dev_301_a_scene_project_builds_only_scene_wasm_matching_the_manifest_entry() {
    let (_root, env, dir) = new_project("rust", "scene");
    build(&env, &dir, &[]);
    assert!(tree_of(&dir).contains(&"dist/scene.wasm".to_owned()));
    assert!(!tree_of(&dir).contains(&"dist/server.wasm".to_owned()));
}

/// REQ: DEV-302
#[test]
fn dev_302_api_version_section_is_three_big_endian_u16() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    let bytes = std::fs::read(dir.join("dist/server.wasm")).unwrap();
    let payload = api_payload([0, 1, 0]);
    assert_eq!(payload.len(), 6);
    assert!(bytes_contain(&bytes, b"ilium:api-version"));
    assert!(
        bytes_contain(&bytes, &payload),
        "section payload 0.1.0 as big-endian u16"
    );
}

/// REQ: DEV-302
#[test]
fn dev_302_a_component_world_that_differs_from_the_manifest_fails_with_world_mismatch() {
    let (_root, env, dir) = new_project("rust", "server");
    let mut manifest: Value = serde_json::from_str(&read_text(&dir, "ilium-plugin.json")).unwrap();
    manifest["server"]["world"] = Value::String("ilium:server/other@0.1.0".to_owned());
    write_text(
        &dir,
        "ilium-plugin.json",
        &serde_json::to_string_pretty(&manifest).unwrap(),
    );
    let output = build(&env, &dir, &[]);
    assert_eq!(error_code(&output).as_deref(), Some("build.world-mismatch"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-303
#[test]
fn dev_303_release_build_has_no_dwarf_sections_and_keeps_export_names() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &["--release"]);
    let bytes = std::fs::read(dir.join("dist/server.wasm")).unwrap();
    assert!(!bytes_contain(&bytes, b".debug_info"));
    assert!(bytes_contain(&bytes, b"handle_batch"));
}

/// REQ: DEV-303
#[test]
fn dev_303_release_typescript_build_writes_no_source_map() {
    let (_root, env, dir) = new_project("typescript", "server");
    build(&env, &dir, &["--release"]);
    assert!(!tree_of(&dir).contains(&"dist/server.wasm.map".to_owned()));
}

/// REQ: DEV-303
#[test]
fn dev_303_debug_rust_build_keeps_dwarf() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &["--debug"]);
    assert!(bytes_contain(
        &std::fs::read(dir.join("dist/server.wasm")).unwrap(),
        b".debug_info"
    ));
}

/// REQ: DEV-303
#[test]
fn dev_303_debug_typescript_build_writes_a_source_map() {
    let (_root, env, dir) = new_project("typescript", "server");
    build(&env, &dir, &["--debug"]);
    assert!(tree_of(&dir).contains(&"dist/server.wasm.map".to_owned()));
}

/// REQ: DEV-304
#[test]
fn dev_304_build_json_has_the_documented_keys() {
    let (_root, env, dir) = new_project("rust", "both");
    build(&env, &dir, &[]);
    let json = build_json(&dir);
    for key in [
        "profile",
        "api",
        "input_hash",
        "sides",
        "toolchain",
        "cached",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }
    let sides = json["sides"].as_array().expect("sides");
    assert_eq!(sides.len(), 2);
    for side in sides {
        assert!(side["name"].is_string() && side["bytes"].is_u64() && side["sha256"].is_string());
    }
}

/// REQ: DEV-304
#[test]
fn dev_304_side_bytes_and_sha256_describe_the_written_component() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    let bytes = std::fs::read(dir.join("dist/server.wasm")).unwrap();
    let side = &build_json(&dir)["sides"][0];
    assert_eq!(side["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(side["sha256"].as_str(), Some(sha256_hex(&bytes).as_str()));
}

/// REQ: DEV-304
#[test]
fn dev_304_toolchain_names_rustc_for_rust_and_bun_for_typescript() {
    let (_r1, env1, dir1) = new_project("rust", "server");
    build(&env1, &dir1, &[]);
    assert!(build_json(&dir1)["toolchain"].to_string().contains("rustc"));
    let (_r2, env2, dir2) = new_project("typescript", "server");
    build(&env2, &dir2, &[]);
    assert!(build_json(&dir2)["toolchain"].to_string().contains("bun"));
}

/// REQ: DEV-305
#[test]
fn dev_305_unchanged_input_hash_reuses_the_cache_and_sets_cached_true() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    let first = build_json(&dir);
    assert_eq!(first["cached"], false);
    build(&env, &dir, &[]);
    let second = build_json(&dir);
    assert_eq!(second["cached"], true);
    assert_eq!(second["input_hash"], first["input_hash"]);
}

/// REQ: DEV-305
#[test]
fn dev_305_changing_a_source_file_changes_the_hash_and_rebuilds() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    let before = build_json(&dir)["input_hash"].clone();
    let source = read_text(&dir, "src/lib.rs");
    write_text(&dir, "src/lib.rs", &format!("{source}\n// changed\n"));
    build(&env, &dir, &[]);
    let after = build_json(&dir);
    assert_ne!(after["input_hash"], before);
    assert_eq!(after["cached"], false);
}

/// REQ: DEV-305
#[test]
fn dev_305_manifest_edit_also_changes_the_hash() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    let before = build_json(&dir)["input_hash"].clone();
    let text = read_text(&dir, "ilium-plugin.json")
        .replace("\"version\": \"0.1.0\"", "\"version\": \"0.1.1\"");
    write_text(&dir, "ilium-plugin.json", &text);
    build(&env, &dir, &[]);
    assert_ne!(build_json(&dir)["input_hash"], before);
}

/// REQ: DEV-306
#[test]
fn dev_306_two_locked_builds_of_the_same_sources_are_byte_identical() {
    let (_root, env, dir) = new_project("rust", "both");
    build(&env, &dir, &["--locked"]);
    let first: Vec<Vec<u8>> = ["dist/server.wasm", "dist/client.wasm"]
        .iter()
        .map(|p| std::fs::read(dir.join(p)).unwrap())
        .collect();
    std::fs::remove_dir_all(dir.join("dist")).unwrap();
    std::fs::remove_dir_all(dir.join(".ilium")).unwrap();
    build(&env, &dir, &["--locked"]);
    let second: Vec<Vec<u8>> = ["dist/server.wasm", "dist/client.wasm"]
        .iter()
        .map(|p| std::fs::read(dir.join(p)).unwrap())
        .collect();
    assert_eq!(first, second);
}

/// REQ: DEV-306
#[test]
fn dev_306_typescript_locked_builds_are_byte_identical() {
    let (_root, env, dir) = new_project("typescript", "server");
    build(&env, &dir, &["--locked"]);
    let first = std::fs::read(dir.join("dist/server.wasm")).unwrap();
    std::fs::remove_dir_all(dir.join(".ilium")).unwrap();
    build(&env, &dir, &["--locked"]);
    assert_eq!(first, std::fs::read(dir.join("dist/server.wasm")).unwrap());
}

/// REQ: DEV-307
#[test]
fn dev_307_build_cache_lives_under_dot_ilium_cache() {
    let (_root, env, dir) = new_project("rust", "server");
    build(&env, &dir, &[]);
    assert!(
        tree_of(&dir).iter().any(|p| p.starts_with(".ilium/cache/")),
        "{:?}",
        tree_of(&dir)
    );
}

/// REQ: DEV-307
#[test]
fn dev_307_no_cache_file_is_written_outside_the_allowed_places() {
    let (_root, env, dir) = new_project("typescript", "server");
    let before = tree_of(&dir);
    build(&env, &dir, &[]);
    let added: Vec<String> = tree_of(&dir)
        .into_iter()
        .filter(|p| !before.contains(p))
        .collect();
    assert!(
        added.iter().all(|p| p.starts_with("dist/")
            || p.starts_with(".ilium/")
            || p.starts_with("node_modules/")
            || p.starts_with("target/")),
        "{added:?}"
    );
}

/// REQ: DEV-307
#[test]
fn dev_307_gitignore_lists_the_four_build_folders() {
    let (_root, _env, dir) = new_project("rust", "server");
    let text = read_text(&dir, ".gitignore");
    for line in ["dist/", ".ilium/", "target/", "node_modules/"] {
        assert!(text.lines().any(|l| l.trim() == line), "{line}");
    }
}
