//! Shared helpers of the screens lane. The rig types below do not exist in the skeleton yet and
//! are listed in API_GAPS.md.

#![allow(dead_code)]

pub use ilium_client::plugin_host::screens_rig::{RowState, ScreensRig};

use std::path::{Path, PathBuf};

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

/// The screen text with trailing blanks trimmed from every row.
pub fn trimmed(screen: &str) -> String {
    screen
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// True when some row of `screen` contains `needle` after the box-drawing frame is stripped.
pub fn has_row_with(screen: &str, needle: &str) -> bool {
    screen.lines().any(|line| line.contains(needle))
}

/// The first row of `screen` that contains `needle`.
pub fn row_with(screen: &str, needle: &str) -> String {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?} in:\n{screen}"))
        .to_string()
}

/// The eight row words of the table in core-plugin-settings.md, in table order.
pub const ROW_WORDS: [&str; 8] = [
    "On",
    "On, slow",
    "Restarting",
    "Failed",
    "Off",
    "Waiting for",
    "Blocked by policy",
    "Refused:",
];
