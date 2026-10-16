//! Shared helpers of the port twins: hosts, the Optimization panel target, screen text.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_compaction::{CompactionClient, CompactionServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ViewTarget;

pub const CODEX_FILE: &str = "agent-homes:.codex/config.toml";
pub const CLAUDE_FILE: &str = "agent-homes:.claude/settings.json";
/// The key the Codex agent reads (original `config_target(Codex).key()`).
pub const CODEX_KEY: &str = "model_auto_compact_token_limit";
/// The key the Claude Code agent reads.
pub const CLAUDE_KEY: &str = "autoCompactWindow";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.activate::<CompactionServer>();
    host
}

pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<CompactionClient>();
    host
}

pub fn panel() -> ViewTarget {
    ViewTarget::Panel("optimizer".to_string())
}

/// A client host with the Optimization panel open.
pub fn opened() -> FakeHost {
    let mut host = client_host();
    host.open_view(panel());
    host
}

/// Rendered text of the panel at the given size.
pub fn screen_text(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(panel(), columns, rows).text()
}

/// Sends the named keys to the panel in order.
pub fn keys(host: &mut FakeHost, names: &[&str]) {
    for name in names {
        host.key(panel(), name);
    }
}

pub fn read_text(host: &mut FakeHost, path: &str) -> String {
    String::from_utf8(host.fs().read(path).expect("file exists")).expect("utf-8")
}

pub fn write_text(host: &mut FakeHost, path: &str, text: &str) {
    host.fs().write(path, text.as_bytes());
}
