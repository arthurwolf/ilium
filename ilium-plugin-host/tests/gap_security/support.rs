//! Shared helpers of the gap-security lane: the ops rig shortcuts, CLI line readers and the
//! manual page reader. Mirrors the helpers of the lifecycle-ops lane so both lanes read the same way.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};

/// The plugin id most tests install.
pub const NOTES: &str = "com.example.notes";
/// A second, unrelated plugin id.
pub const OTHER: &str = "com.example.other";

/// A fresh rig with empty data, state, config and cache directories.
pub fn rig() -> OpsRig {
    OpsRig::new()
}

/// A package spec with the given id and version.
pub fn pkg(id: &str, version: &str) -> PackageSpec {
    PackageSpec::new(id, version)
}

/// A whole number of seconds as a `Duration`.
pub fn secs(count: u64) -> Duration {
    Duration::from_secs(count)
}

/// Installs a spec non-interactively: `--yes --allow-unsigned`.
pub fn install(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"])
}

/// Installs a spec and asserts exit code 0.
pub fn install_ok(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let output = install(rig, spec);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
    output
}

/// Every `code` of the `error` lines of a CLI run, in order.
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "error")
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

/// The registry entry of a plugin, or `Value::Null`.
pub fn registry_entry(rig: &OpsRig, id: &str) -> serde_json::Value {
    let registry = rig.registry_json();
    registry["plugins"]
        .as_array()
        .and_then(|plugins| plugins.iter().find(|entry| entry["id"] == id))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// The repository root (the directory that holds the workspace `Cargo.toml`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the host crate sits inside the workspace")
        .to_path_buf()
}

/// The text of one manual page (`docs/plugins-manual/pages/<name>.md`).
pub fn manual_page(name: &str) -> String {
    let path = repo_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Asserts that a manual page contains the MUST text of one requirement.
pub fn assert_manual_says(page_name: &str, must_text: &str) {
    let page = manual_page(page_name);
    assert!(
        page.contains(must_text),
        "manual page {page_name} must state: {must_text}"
    );
}
