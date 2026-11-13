//! Manual-page helpers: mounted only by the `features` test target. The `port` target mounts
//! `common.rs` by path and never reads the manual, so these items must not live there.

use std::path::PathBuf;

/// Root of the worktree, found from the crate directory (plugins/core/fleet).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-fleet`.
pub fn page(stem: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}
