//! Helpers of the Worktrees feature tests only (manual page core-plugin-worktrees.md): manual pages,
//! user docs, golden files, server-side hosts and constants. Helpers both suites use live in
//! ../shared/common.rs, which this module re-exports.
//!
//! Every use of a testkit or plugin item that the skeleton lacks lives in the "gap helpers" section
//! at the end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
pub use crate::shared_common::*;

use std::path::PathBuf;

use ilium_plugin_core_worktrees::WorktreesServer;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

pub const BEFORE_CREATE: &str = "ilium.core.worktrees/before-create";
pub const BEFORE_REMOVE: &str = "ilium.core.worktrees/before-remove";
pub const POST_CREATE_STEPS: &str = "ilium.core.worktrees/post-create-steps";

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-worktrees`.
pub fn page(stem: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-worktrees")
}

/// A user document of the repository by file name, for example `worktrees.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

pub fn golden_path(name: &str) -> String {
    format!(
        "{}/tests/features/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Table rows (cells trimmed) of the first table after the line starting with `anchor`.
pub fn table_after(text: &str, anchor: &str) -> Vec<Vec<String>> {
    let mut lines = text.lines().skip_while(|line| !line.starts_with(anchor));
    lines.next().expect("anchor present");
    lines
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// The bodies of the ```json fences of a text, in order.
pub fn json_fences(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```json\n") {
        let body = &rest[start + "```json\n".len()..];
        let end = body.find("```").expect("fence end");
        found.push(body[..end].to_string());
        rest = &body[end + 3..];
    }
    found
}

// ---- gap helpers (see ../API_GAPS.md) -----------------------------------------------------

/// A server-side host with the manifest grants, not yet activated.
pub fn server_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build()
}

/// A server-side host with the plugin activated; every git run succeeds with empty output.
pub fn server_host() -> FakeHost {
    let mut host = server_host_unactivated();
    host.process().default_exit(ok_exit());
    host.activate::<WorktreesServer>();
    host
}

/// A server-side host whose project root is `root`, not yet activated.
pub fn server_host_unactivated_at(root: &str) -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .with_project_root(PathBuf::from(root))
        .build()
}
