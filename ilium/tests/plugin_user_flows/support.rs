//! Shared helpers of the user-admin lane (ilium side). The rig types below do not exist in the
//! skeleton yet and are listed in API_GAPS.md.


use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::lifecycle::StartMode;
pub use ilium_plugin_host::testing::user_rig::{
    CallOutcome, DialogAnswer, HookBehaviour, KeyPair, KillPoint, PackageSpec, Platform, PluginRow,
    UserRig,
};

/// The workspace root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The text of one manual page, without the `.md` suffix in `name`.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// All records of one `type` in the JSONL output.
pub fn records(output: &CliOutput, record_type: &str) -> Vec<Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record["type"] == record_type)
        .cloned()
        .collect()
}

/// The codes of all `error` records, in order.
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    records(output, "error")
        .iter()
        .filter_map(|record| record["code"].as_str().map(str::to_string))
        .collect()
}

/// The messages of all `error` records, in order.
pub fn error_messages(output: &CliOutput) -> Vec<String> {
    records(output, "error")
        .iter()
        .filter_map(|record| record["message"].as_str().map(str::to_string))
        .collect()
}

/// The `exit_code` of the final `result` record (DEV-017).
pub fn result_exit_code(output: &CliOutput) -> i64 {
    output
        .stdout_jsonl
        .last()
        .and_then(|record| record["exit_code"].as_i64())
        .unwrap_or(-1)
}

/// A plain third-party package: unsigned (T3), server side, with these grants.
pub fn spec(id: &str, version: &str, server_grants: &[&str]) -> PackageSpec {
    PackageSpec::new(id, version).grants("server", server_grants)
}

/// Installs a package without a client and asserts success; for test set-up.
pub fn install_ok(rig: &mut UserRig, package: &PackageSpec) {
    let output = rig.install_file(package, &["--allow-unsigned", "--yes"]);
    assert_eq!(
        result_exit_code(&output),
        0,
        "set-up install failed: {:?}",
        error_codes(&output)
    );
}

/// The row of one plugin in `ilium plugin list`.
pub fn list_row(rig: &mut UserRig, id: &str) -> PluginRow {
    rig.list()
        .into_iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("plugin {id} is not listed"))
}

/// Every file under `root` with its bytes, keyed by the path relative to `root`.
pub fn tree_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                found.insert(relative, bytes);
            }
        }
    }
    found
}

/// The permission bits (`0o777` mask) of a path.
pub fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0)
}

/// The 30 core plugin ids named by the table of the core plugins page.
pub fn core_plugin_ids_from_manual() -> Vec<String> {
    let page = manual_page("user-core-plugins");
    let pattern = regex::Regex::new(r"`(ilium\.core\.[a-z-]+)`").expect("pattern");
    let mut ids: Vec<String> = Vec::new();
    for capture in pattern.captures_iter(&page) {
        let id = capture[1].to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The ids of the core plugins marked "no" in the column "Can you turn it off".
pub fn core_plugins_that_cannot_be_turned_off() -> Vec<String> {
    let page = manual_page("user-core-plugins");
    let pattern = regex::Regex::new(r"`(ilium\.core\.[a-z-]+)`\)\s*\|[^|]*\|[^|]*\|\s*no\s*\|")
        .expect("pattern");
    pattern
        .captures_iter(&page)
        .map(|capture| capture[1].to_string())
        .collect()
}
