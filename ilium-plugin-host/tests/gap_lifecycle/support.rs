//! Shared helpers of the gap-lifecycle lane: rig shortcuts, manual-page reads and installed-state
//! readers. Rig calls use the `ilium_plugin_host::testing::ops` items named in API_GAPS.md; any
//! item the rig does not have yet is listed in the lane's missing_hooks report.

use std::time::Duration;

pub use ilium_plugin_api::Side;
pub use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec, ServerHandle, UpdateStage};

/// The plugin id most tests install.
pub const NOTES: &str = "com.example.notes";
/// A second, unrelated plugin id.
pub const OTHER: &str = "com.example.other";
/// An essential core plugin id (used by the core-restart rules).
pub const CORE: &str = "ilium.core.files";

/// A fresh rig with empty data, state, config and cache directories.
pub fn rig() -> OpsRig {
    OpsRig::new()
}

/// A package spec with the given id and version.
pub fn pkg(id: &str, version: &str) -> PackageSpec {
    PackageSpec::new(id, version)
}

/// Whole seconds as a `Duration`.
pub fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// Whole days as a `Duration`.
pub fn days(value: u64) -> Duration {
    Duration::from_secs(value * 86_400)
}

/// Installs a spec non-interactively and asserts exit code 0.
pub fn install_ok(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"]);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
    output
}

/// Installs a spec non-interactively without asserting the result.
pub fn install(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"])
}

/// Updates with a spec: `update <file> --yes --allow-unsigned`.
pub fn update(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["update", &path_text, "--yes", "--allow-unsigned"])
}

/// The text of one manual page (`docs/plugins-manual/pages/<name>.md`).
pub fn manual_page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the host crate sits inside the workspace")
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Asserts that a manual page states the MUST sentence a Doc check quotes.
pub fn assert_manual_says(page: &str, must_text: &str) {
    let text = manual_page(page);
    assert!(
        text.contains(must_text),
        "manual page {page} must state: {must_text}"
    );
}

/// The parsed `installed.json` of a rig's state directory.
pub fn installed_json(rig: &OpsRig) -> serde_json::Value {
    let path = rig.state_dir().join("installed.json");
    let text =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&text).expect("installed.json is JSON")
}

/// The rows of `ilium plugin list --json`.
pub fn list_rows(rig: &OpsRig) -> Vec<serde_json::Value> {
    rig.cli(&["plugin", "list", "--json"]).stdout_jsonl
}

/// The `ilium plugin list --json` row of one plugin id.
pub fn list_row(rig: &OpsRig, id: &str) -> serde_json::Value {
    list_rows(rig)
        .into_iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("plugin {id} is missing from ilium plugin list --json"))
}

/// The error codes printed by a CLI run (JSONL lines with `type == "error"`).
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "error")
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

/// The count of one log code on a server.
pub fn log_count(server: &ServerHandle, code: &str) -> usize {
    server
        .log_codes()
        .iter()
        .filter(|found| found.as_str() == code)
        .count()
}

/// A `[plugins."<id>"]` policy entry that kills `version_range`.
pub fn kill_policy(plugin_id: &str, version_range: &str) -> String {
    format!("[plugins.\"{plugin_id}\"]\nkill = \"{version_range}\"\n")
}

/// A `[plugins."<id>"]` policy entry that holds an update at a rollout stage of `percent`.
pub fn rollout_policy(plugin_id: &str, version: &str, percent: u8) -> String {
    format!(
        "[plugins.\"{plugin_id}\"]\nrollout = {{ update = \"{version}\", stage_percent = {percent} }}\n"
    )
}
