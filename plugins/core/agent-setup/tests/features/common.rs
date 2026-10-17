//! Shared helpers: hosts for both sides, file access on the fake file system, the service calls, block text.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_agent_setup::{AgentSetupClient, AgentSetupServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{FsPath, PluginError};
use serde_json::{json, Value};

/// The service the plugin offers.
pub const SERVICE: &str = "ilium.core.agent-setup/install";

/// Opening marker of the Chatroom block.
pub const CHATROOM_OPEN: &str = "<!-- ilium-agent-feature: chatroom -->";
/// Closing marker of the Chatroom block.
pub const CHATROOM_CLOSE: &str = "<!-- /ilium-agent-feature: chatroom -->";
/// Opening marker of the current Progress block (version 13).
pub const PROGRESS_OPEN: &str = "<!-- ilium-agent-feature: progress version=13 -->";
/// Closing marker of the Progress block.
pub const PROGRESS_CLOSE: &str = "<!-- /ilium-agent-feature: progress -->";

/// Target ids (the manual does not name them, see API_GAPS.md).
pub const CLAUDE_GLOBAL: &str = "claude-global";
pub const CODEX_GLOBAL: &str = "codex-global";
pub const CLAUDE_PROJECT: &str = "claude-project";
pub const CODEX_PROJECT: &str = "codex-project";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/agent-setup).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-agent-setup.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// Any other page of the manual.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A server-side host with the manifest grants and the plugin activated; no project is open.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<AgentSetupServer>();
    host
}

/// A server-side host like `server_host` with one open project named "ilium" (its root is the `project` scope).
pub fn project_host() -> FakeHost {
    let mut host = server_host();
    host.open_project("ilium");
    host.settle();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<AgentSetupClient>();
    host
}

/// `~/.claude/CLAUDE.md` on the fake file system (scope `agent-config:claude`).
pub fn claude_file() -> FsPath {
    FsPath {
        scope: "agent-config:claude".to_string(),
        path: "CLAUDE.md".to_string(),
    }
}

/// A second file in the Claude config folder, used as a custom global path.
pub fn claude_alt_file() -> FsPath {
    FsPath {
        scope: "agent-config:claude".to_string(),
        path: "ALT.md".to_string(),
    }
}

/// `~/.codex/AGENTS.md` on the fake file system (scope `agent-config:codex`).
pub fn codex_file() -> FsPath {
    FsPath {
        scope: "agent-config:codex".to_string(),
        path: "AGENTS.md".to_string(),
    }
}

/// `CLAUDE.md` in the root of the open project.
pub fn project_claude_file() -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: "CLAUDE.md".to_string(),
    }
}

/// `AGENTS.md` in the root of the open project.
pub fn project_agents_file() -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: "AGENTS.md".to_string(),
    }
}

/// `CHATROOM.md` in the root of the open project.
pub fn chatroom_file() -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: "CHATROOM.md".to_string(),
    }
}

/// Puts a text file on the fake file system.
pub fn put(host: &mut FakeHost, path: FsPath, text: &str) {
    host.fs().write(path, text.as_bytes());
}

/// Reads a text file from the fake file system.
pub fn get(host: &mut FakeHost, path: FsPath) -> Option<String> {
    host.fs()
        .read(path)
        .map(|bytes| String::from_utf8(bytes).expect("utf-8 file"))
}

/// Calls one method of the install service.
pub fn call(
    host: &mut FakeHost,
    method: &str,
    block: &str,
    target: &str,
) -> Result<Value, PluginError> {
    let payload = json!({ "block": block, "target": target }).to_string();
    host.call_service(SERVICE, method, &payload)
        .map(|text| serde_json::from_str(&text).expect("the reply is JSON"))
}

/// Installs one block into one target.
pub fn install(host: &mut FakeHost, block: &str, target: &str) -> Result<Value, PluginError> {
    call(host, "install", block, target)
}

/// All status rows the service lists.
pub fn status_rows(host: &mut FakeHost) -> Vec<Value> {
    let text = host
        .call_service(SERVICE, "status", "{}")
        .expect("status answers");
    let reply: Value = serde_json::from_str(&text).expect("the reply is JSON");
    reply["targets"].as_array().expect("targets array").clone()
}

/// The status word of one block in one target, as the service reports it.
pub fn status_of(host: &mut FakeHost, block: &str, target: &str) -> String {
    let rows = status_rows(host);
    let row = rows
        .iter()
        .find(|row| row["block"] == block && row["target"] == target)
        .unwrap_or_else(|| panic!("no status row for {block} / {target} in {rows:?}"));
    row["status"].as_str().expect("status string").to_string()
}

/// The text between the opening and closing marker (both included) of a block.
pub fn block_of(text: &str, feature: &str) -> String {
    let open = format!("<!-- ilium-agent-feature: {feature}");
    let close = format!("<!-- /ilium-agent-feature: {feature} -->");
    let start = text
        .find(&open)
        .unwrap_or_else(|| panic!("no {feature} block in\n{text}"));
    let end = text.find(&close).expect("closing marker") + close.len();
    text[start..end].to_string()
}

/// A Progress block of an older version.
pub fn old_progress_block(version: u32) -> String {
    format!("<!-- ilium-agent-feature: progress version={version} -->\nOld wording.\n{PROGRESS_CLOSE}\n")
}

/// Text that has every signal of the Progress instruction but no markers (look-alike).
pub const PROGRESS_LOOK_ALIKE: &str =
    "Use ilium progress for any task of at least three minutes. You MUST use a monitor.\n\
Start the task in the background and verify it is alive. You MUST NOT poll.\n\
Ilium's server is the sole recurring poller. Run ilium progress check first.\n\
Wait for the positive JSONL registration acknowledgement.\n\
The probe prints job_id, status, percent.\n\
Ilium will send you a message when the task reaches done or error.\n";

/// Text that has the two signals of the Chatroom instruction but no markers.
pub const CHATROOM_LOOK_ALIKE: &str =
    "Read CHATROOM.md with ilium chat context before you start.\nUse ilium chat send for claims and handoffs.\n";

/// The rows of the Setup tab text that contain `needle`.
pub fn rows_with<'a>(text: &'a str, needle: &str) -> Vec<&'a str> {
    text.lines().filter(|line| line.contains(needle)).collect()
}

/// Compare a render with `golden/<name>.txt`. A missing golden file fails; the file holds the exact expected render.
pub fn assert_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/golden")
        .join(format!("{name}.txt"));
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("golden {path:?} unreadable: {error}"));
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "render differs from golden {name}"
    );
}
