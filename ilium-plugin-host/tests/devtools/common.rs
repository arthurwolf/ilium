//! Shared helpers of the devtools tests (lane dev-pipeline): environment builder, project builder,
//! component builder, record selectors, SHA-256.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_host::cli::{self, CliEnv, CliOutput};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::testing::wat_component;
use serde_json::{json, Value};

/// A fetcher that always fails: author tooling must work offline.
pub struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// The fixed "now" of the tests (2030-01-01 12:00:00 UTC).
pub fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// A scratch directory the test owns.
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("create scratch dir")
}

/// An environment rooted in `root`; the working directory is `root/project` and exists.
pub fn env(root: &Path) -> CliEnv {
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("create project dir");
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        project,
        fixed_now(),
        true,
        Arc::new(NoNetwork),
    )
}

/// The project directory of an environment.
pub fn project_dir(env: &CliEnv) -> PathBuf {
    env.cwd.clone()
}

/// Runs `ilium plugin <args...>`.
pub fn run(env: &CliEnv, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    cli::run(&owned, env)
}

/// Runs `ilium plugin <command> <project dir> --json <extra...>`.
pub fn run_in_project(env: &CliEnv, command: &str, extra: &[&str]) -> CliOutput {
    let dir = project_dir(env);
    let dir_text = dir.to_str().expect("utf8 path").to_owned();
    let mut args: Vec<&str> = vec![command, dir_text.as_str(), "--json"];
    args.extend_from_slice(extra);
    run(env, &args)
}

/// The records of one `type` in output order.
pub fn records_of<'a>(output: &'a CliOutput, record_type: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get("type").and_then(Value::as_str) == Some(record_type))
        .collect()
}

/// The `type` of every record in order.
pub fn record_types(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .map(|record| record.get("type").and_then(Value::as_str).unwrap_or("").to_owned())
        .collect()
}

/// The `code` of the first `error` record.
pub fn error_code(output: &CliOutput) -> Option<String> {
    records_of(output, "error")
        .first()
        .and_then(|record| record.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The `rule` of every record of `record_type` (`check` or `lint`) in output order.
pub fn rules_of(output: &CliOutput, record_type: &str) -> Vec<String> {
    records_of(output, record_type)
        .iter()
        .filter_map(|record| record.get("rule").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// The first record of `record_type` whose `rule` equals `rule`.
pub fn finding<'a>(output: &'a CliOutput, record_type: &str, rule: &str) -> Option<&'a Value> {
    records_of(output, record_type)
        .into_iter()
        .find(|record| record.get("rule").and_then(Value::as_str) == Some(rule))
}

/// True when a record of `record_type` carries `rule`.
pub fn has_rule(output: &CliOutput, record_type: &str, rule: &str) -> bool {
    finding(output, record_type, rule).is_some()
}

/// The `severity` of the first finding with `rule`.
pub fn severity_of(output: &CliOutput, record_type: &str, rule: &str) -> Option<String> {
    finding(output, record_type, rule)
        .and_then(|record| record.get("severity"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Writes a text file below `dir`, creating folders.
pub fn write_text(dir: &Path, relative: &str, text: &str) {
    write_bytes(dir, relative, text.as_bytes());
}

/// Writes a binary file below `dir`, creating folders.
pub fn write_bytes(dir: &Path, relative: &str, bytes: &[u8]) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create folders");
    std::fs::write(path, bytes).expect("write file");
}

/// Reads a text file below `dir`.
pub fn read_text(dir: &Path, relative: &str) -> String {
    std::fs::read_to_string(dir.join(relative)).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

/// Every file below `dir` as sorted relative paths with `/` separators.
pub fn tree_of(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, current: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(current) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).expect("below base");
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The manifest of the `server` template (reference-project-templates.md) as a value.
pub fn base_manifest() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.agent-tools",
        "name": "Agent tools",
        "version": "0.1.0",
        "publisher": "example",
        "description": "Marks agents that start working.",
        "license": "MIT",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
        "capabilities": { "server": ["tree.read", "tree.decorate"] },
        "contributes": {
            "decoration_slots": [{ "id": "work", "placement": "badge", "priority": 50 }],
            "help": [{ "id": "overview", "file": "help/overview.md", "title": "Agent tools" }]
        }
    })
}

/// Writes `manifest.json` into `dir`.
pub fn write_manifest(dir: &Path, manifest: &Value) {
    write_text(dir, "manifest.json", &serde_json::to_string_pretty(manifest).expect("json"));
}

/// The README of a fresh project: grants explained, build and test commands.
pub fn good_readme() -> String {
    "# Agent tools\n\nMarks agent panes that are working with a badge in the tree.\n\n## Grants\n\n- `tree.read`: reads agent activity.\n- `tree.decorate`: sets the badge.\n\n## Build and test\n\n    ilium plugin check\n    ilium plugin lint --deny warnings\n    ilium plugin test\n    ilium plugin build\n".to_owned()
}

/// A changelog that follows Keep a Changelog.
pub fn good_changelog() -> String {
    "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- First version.\n".to_owned()
}

/// Writes README, CHANGELOG, LICENSE and the help topic of the base manifest.
pub fn write_docs(dir: &Path) {
    write_text(dir, "README.md", &good_readme());
    write_text(dir, "CHANGELOG.md", &good_changelog());
    write_text(dir, "LICENSE", "MIT License\n");
    write_text(dir, "help/overview.md", "# Agent tools\n\nOne sentence that says what the plugin adds.\n\nThe plugin adds the work badge.\n");
}

/// A project that should raise no lint finding except skipped-rule hints: base manifest plus docs.
pub fn fresh_project(dir: &Path) {
    write_manifest(dir, &base_manifest());
    write_docs(dir);
}

/// Mutates the base manifest with `edit`, writes it with the docs and returns the value.
pub fn project_with(dir: &Path, edit: impl FnOnce(&mut Value)) -> Value {
    let mut manifest = base_manifest();
    edit(&mut manifest);
    write_manifest(dir, &manifest);
    write_docs(dir);
    manifest
}

/// The three big-endian `u16` values of the `ilium:api-version` section.
pub fn api_payload(version: [u16; 3]) -> Vec<u8> {
    version.iter().flat_map(|part| part.to_be_bytes()).collect()
}

fn push_leb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends a custom section (id 0) to a component binary.
pub fn with_custom_section(mut bytes: Vec<u8>, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    push_leb(&mut body, name.len() as u64);
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(payload);
    bytes.push(0);
    push_leb(&mut bytes, body.len() as u64);
    bytes.extend_from_slice(&body);
    bytes
}

/// Name of the custom section that tests use for the world embedded in a component.
pub const WORLD_SECTION: &str = "ilium:world";

/// Component text of a server component that imports the given functions and exports the
/// required `lifecycle` and `handler` interfaces. `imports` is `(interface, function)` pairs.
pub fn server_component_text(imports: &[(&str, &str)]) -> String {
    let mut text = String::from("(component\n");
    let mut interfaces: Vec<&str> = imports.iter().map(|(interface, _)| *interface).collect();
    interfaces.sort_unstable();
    interfaces.dedup();
    for interface in interfaces {
        text.push_str(&format!("  (import \"{interface}\" (instance"));
        for (owner, function) in imports {
            if *owner == interface {
                text.push_str(&format!(" (export \"{function}\" (func))"));
            }
        }
        text.push_str("))\n");
    }
    text.push_str("  (instance $l) (instance $h)\n");
    text.push_str("  (export \"ilium:exports/lifecycle@0.1.0\" (instance $l))\n");
    text.push_str("  (export \"ilium:exports/handler@0.1.0\" (instance $h)))\n");
    text
}

/// A server component with embedded world and api version sections.
pub fn server_component(imports: &[(&str, &str)], world: &str, api: [u16; 3]) -> Vec<u8> {
    let bytes = wat_component(&server_component_text(imports));
    let bytes = with_custom_section(bytes, "ilium:api-version", &api_payload(api));
    with_custom_section(bytes, WORLD_SECTION, world.as_bytes())
}

/// The component of a clean server plugin: right world, api 0.1.0, no imports.
pub fn good_server_component() -> Vec<u8> {
    server_component(&[], "ilium:server/base@0.1.0", [0, 1, 0])
}

/// A good server component padded with a custom section so that the whole file has exactly `size` bytes.
pub fn server_component_of_size(size: usize) -> Vec<u8> {
    let base = good_server_component();
    let mut pad = size.saturating_sub(base.len() + 16);
    for _ in 0..4 {
        let candidate = with_custom_section(base.clone(), "pad", &vec![0u8; pad]);
        let delta = size as i64 - candidate.len() as i64;
        if delta == 0 {
            return candidate;
        }
        pad = (pad as i64 + delta).max(0) as usize;
    }
    with_custom_section(base, "pad", &vec![0u8; pad])
}

/// Writes `dist/<side>.wasm`.
pub fn write_dist(dir: &Path, side: &str, bytes: &[u8]) {
    write_bytes(dir, &format!("dist/{side}.wasm"), bytes);
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// SHA-256 of `data` as 64 lowercase hexadecimal characters.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks(64) {
        let mut w = [0u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([block[index * 4], block[index * 4 + 1], block[index * 4 + 2], block[index * 4 + 3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
            w[index] = w[index - 16].wrapping_add(s0).wrapping_add(w[index - 7]).wrapping_add(s1);
        }
        let mut v = state;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let choice = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(choice).wrapping_add(SHA256_K[index]).wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let majority = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(majority);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(v[index]);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// The packed manifest of the base manifest: the files list is added (`length` and `sha256`).
pub fn packed_manifest(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut manifest = base_manifest();
    let listing: Vec<Value> = files
        .iter()
        .map(|(path, bytes)| json!({ "path": path, "length": bytes.len(), "sha256": sha256_hex(bytes) }))
        .collect();
    manifest["files"] = Value::Array(listing);
    serde_json::to_vec_pretty(&manifest).expect("json")
}

/// An unsigned `.iliumpkg` of the base manifest holding `dist/server.wasm`.
pub fn unsigned_package(component: &[u8]) -> Vec<u8> {
    ilium_plugin_host::package::write_package(&[
        ("manifest.json".to_owned(), packed_manifest(&[("dist/server.wasm", component)])),
        ("dist/server.wasm".to_owned(), component.to_vec()),
    ])
    .expect("package")
}

/// The same package signed with a throwaway Ed25519 seed.
pub fn signed_package(component: &[u8]) -> Vec<u8> {
    ilium_plugin_host::signing::sign(
        &unsigned_package(component),
        &ilium_plugin_host::signing::SigningKey(vec![7u8; 32]),
        ilium_plugin_host::signing::SignedAt("2030-01-01 12:00:00".to_owned()),
    )
    .expect("signed package")
}

/// Writes a package to `dir/<name>` and returns the path text.
pub fn write_package_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
    write_bytes(dir, name, bytes);
    dir.join(name).to_str().expect("utf8").to_owned()
}

/// Runs `check <file> --json`.
pub fn check_file(env: &CliEnv, file: &str) -> CliOutput {
    run(env, &["check", file, "--json"])
}

/// Loads a built guest fixture component `plugins/fixtures/<name>/dist/<side>.wasm`. A fixture that
/// the toolchain has not built yet fails the test with `fixture.missing` (red until it is built).
pub fn fixture_component(name: &str, side: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("plugins")
        .join("fixtures")
        .join(name)
        .join("dist")
        .join(format!("{side}.wasm"));
    std::fs::read(&path).unwrap_or_else(|_| panic!("fixture.missing: {}", path.display()))
}

/// Writes a sparse file of exactly `size` bytes.
pub fn write_sparse(dir: &Path, relative: &str, size: u64) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create folders");
    let file = std::fs::File::create(path).expect("create sparse file");
    file.set_len(size).expect("set length");
}

/// Writes a project that lints clean of everything except skipped-rule hints, with a built
/// server component, and returns the environment.
pub fn lint_env_with(edit: impl FnOnce(&mut Value)) -> (tempfile::TempDir, CliEnv) {
    let root = scratch();
    let env = env(root.path());
    project_with(&project_dir(&env), edit);
    (root, env)
}

/// Runs `lint <project> --json` with extra arguments.
pub fn lint(env: &CliEnv, extra: &[&str]) -> CliOutput {
    run_in_project(env, "lint", extra)
}

/// `Some(true)` when the lint record with `rule` is marked suppressed.
pub fn is_suppressed(output: &CliOutput, rule: &str) -> Option<bool> {
    finding(output, "lint", rule).and_then(|record| record["suppressed"].as_bool())
}
