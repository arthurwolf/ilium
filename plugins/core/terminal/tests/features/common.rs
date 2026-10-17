//! Shared helpers: hosts for both sides, page and manifest readers, submission readers.

use std::path::PathBuf;

use ilium_plugin_core_terminal::{TerminalClient, TerminalServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Intent, Submission};

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/terminal).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Text of one manual page.
pub fn page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    page("core-plugin-terminal.md")
}

/// The manifest file as JSON.
pub fn manifest_json() -> serde_json::Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is valid JSON")
}

/// The body of the first fenced block of `language` after the heading line `heading`.
pub fn fenced_block_after(text: &str, heading: &str, language: &str) -> String {
    let start = text
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading:?} missing"));
    let rest = &text[start..];
    let fence = format!("```{language}\n");
    let open = rest
        .find(&fence)
        .unwrap_or_else(|| panic!("no {language} block after {heading:?}"));
    let body = &rest[open + fence.len()..];
    let close = body.find("\n```").expect("block is closed");
    body[..close].to_string()
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TerminalServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TerminalClient>();
    host
}

/// The intents of a submission list, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions
        .iter()
        .map(|submission| submission.intent.clone())
        .collect()
}
