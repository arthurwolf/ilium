//! Shared helpers of the lifecycle-ops lane: the rig shortcuts, JSONL readers and file-tree
//! snapshots. Everything uses the skeleton or the items named in API_GAPS.md.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::testing::ops::{
    Fault, LifeEvent, Marker, OpsRig, PackageSpec, RegistryStep, ServerHandle, UninstallStep,
    UpdateStage,
};
use serde_json::Value;

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

/// The text of one manual page (`docs/plugins-manual/pages/<name>.md`).
pub fn manual_page(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the host crate sits inside the workspace")
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
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

/// Updates with a spec: `update <file> --yes --allow-unsigned`.
pub fn update(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["update", &path_text, "--yes", "--allow-unsigned"])
}

/// Every `code` of the `error` lines, in order.
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    codes_of(output, "error")
}

/// Every `code` of the `warning` lines, in order.
pub fn warning_codes(output: &CliOutput) -> Vec<String> {
    codes_of(output, "warning")
}

fn codes_of(output: &CliOutput, line_type: &str) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == line_type)
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

/// The lines of one `type`.
pub fn lines_of<'a>(output: &'a CliOutput, line_type: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == line_type)
        .collect()
}

/// The registry entry of a plugin, or `Value::Null`.
pub fn registry_entry(rig: &OpsRig, id: &str) -> Value {
    let registry = rig.registry_json();
    registry["plugins"]
        .as_array()
        .and_then(|plugins| plugins.iter().find(|entry| entry["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The ids in `installed.json`, in file order.
pub fn registry_ids(rig: &OpsRig) -> Vec<String> {
    rig.registry_json()["plugins"]
        .as_array()
        .map(|plugins| {
            plugins
                .iter()
                .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Every file under a directory as (relative path, bytes), sorted; an absent directory is empty.
pub fn snapshot_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                found.push((relative, std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    found.sort();
    found
}

/// The names of the entries directly inside a directory, sorted; absent means empty.
pub fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The versions in `store/<id>/`, sorted.
pub fn store_versions(rig: &OpsRig, id: &str) -> Vec<String> {
    entry_names(&rig.plugins_root().join("store").join(id))
}

/// The entries of `staging/`.
pub fn staging_entries(rig: &OpsRig) -> Vec<String> {
    entry_names(&rig.plugins_root().join("staging"))
}

/// The pid of a process that has exited (so no live creator owns a staging directory).
pub fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn `true`");
    let pid = child.id();
    child.wait().expect("wait for `true`");
    pid
}

/// Milliseconds as a duration.
pub fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Seconds as a duration.
pub fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// The set of strings in a slice.
pub fn set_of(values: &[String]) -> BTreeSet<String> {
    values.iter().cloned().collect()
}

/// Position of the first element equal to `needle`; fails the test when absent.
pub fn position_of(haystack: &[String], needle: &str) -> usize {
    haystack
        .iter()
        .position(|entry| entry == needle)
        .unwrap_or_else(|| panic!("`{needle}` not in {haystack:?}"))
}

/// Every error code (first column) of the catalogue tables in the manifest-schema page.
pub fn catalogue_codes() -> BTreeSet<String> {
    let page = manual_page("reference-manifest-schema");
    let mut codes = BTreeSet::new();
    for line in page.lines() {
        let mut cells = line.split('|').map(str::trim);
        if cells.next() != Some("") {
            continue;
        }
        let Some(first) = cells.next() else { continue };
        let code = first.trim_matches('`');
        let is_code = code.contains('.')
            && !code.contains(' ')
            && code
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-' || c.is_ascii_digit());
        if is_code {
            codes.insert(code.to_owned());
        }
    }
    codes
}
