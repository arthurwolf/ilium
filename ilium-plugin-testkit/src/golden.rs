//! Golden and snapshot files (reference-test-harness.md "Snapshot and golden files").

/// Environment variable that switches golden files to update mode.
pub const UPDATE_GOLDEN_ENV: &str = "ILIUM_PLUGIN_UPDATE_GOLDEN";

/// Compares `actual` to the golden file after converting CRLF to LF; fails with a unified diff of at most 40 lines.
pub fn assert_golden(_path: &str, _actual: &str) {
    unimplemented!("DEV-500")
}

/// Compares the value, as pretty JSON with sorted keys and a trailing newline, to `tests/snapshots/<name>.json`.
pub fn assert_snapshot(_name: &str, _value: &impl serde::Serialize) {
    unimplemented!("DEV-504")
}
