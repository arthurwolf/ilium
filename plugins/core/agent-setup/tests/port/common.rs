//! Shared helpers of the port twins: hosts, file access on the fake file system, service calls.

pub use crate::host_ext::*;
pub use crate::host_ext_port::*;
use std::path::PathBuf;

use ilium_plugin_core_agent_setup::AgentSetupServer;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{FsPath, PluginError};
use serde_json::{json, Value};

pub const SERVICE: &str = "ilium.core.agent-setup/install";
pub const CLAUDE_GLOBAL: &str = "claude-global";
pub const CODEX_GLOBAL: &str = "codex-global";
pub const CLAUDE_PROJECT: &str = "claude-project";
pub const CODEX_PROJECT: &str = "codex-project";

/// Target ids of the config writer facade (see API_GAPS_port.md).
pub const CLAUDE_SETTING: &str = "claude-auto-compact-window";
pub const CODEX_SETTING: &str = "codex-auto-compact-token-limit";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<AgentSetupServer>();
    host
}

/// Server host with one open project named "ilium".
pub fn project_host() -> FakeHost {
    let mut host = server_host();
    host.open_project("ilium");
    host.settle();
    host
}

fn path_of(scope: &str, path: &str) -> FsPath {
    FsPath {
        scope: scope.to_string(),
        path: path.to_string(),
    }
}

pub fn claude_file() -> FsPath {
    path_of("agent-config:claude", "CLAUDE.md")
}
pub fn codex_file() -> FsPath {
    path_of("agent-config:codex", "AGENTS.md")
}
pub fn project_claude_file() -> FsPath {
    path_of("project", "CLAUDE.md")
}
pub fn project_agents_file() -> FsPath {
    path_of("project", "AGENTS.md")
}
/// `~/.claude/settings.json` (Claude Code autoCompactWindow).
pub fn claude_settings_file() -> FsPath {
    path_of("agent-config:claude", "settings.json")
}
/// `~/.codex/config.toml` (Codex model_auto_compact_token_limit).
pub fn codex_config_file() -> FsPath {
    path_of("agent-config:codex", "config.toml")
}

pub fn put(host: &mut FakeHost, path: FsPath, text: &str) {
    host.fs().write(path, text.as_bytes());
}

pub fn get(host: &mut FakeHost, path: FsPath) -> Option<String> {
    host.fs()
        .read(path)
        .map(|bytes| String::from_utf8(bytes).expect("utf-8 file"))
}

fn service_call(host: &mut FakeHost, method: &str, payload: Value) -> Result<Value, PluginError> {
    host.call_service(SERVICE, method, &payload.to_string())
        .map(|text| serde_json::from_str(&text).expect("the reply is JSON"))
}

pub fn install(host: &mut FakeHost, block: &str, target: &str) -> Result<Value, PluginError> {
    service_call(host, "install", json!({ "block": block, "target": target }))
}

pub fn remove(host: &mut FakeHost, block: &str, target: &str) -> Result<Value, PluginError> {
    service_call(host, "remove", json!({ "block": block, "target": target }))
}

/// The status word of one block in one target (set-up, update-required, newer, detected, not-set-up).
pub fn status_of(host: &mut FakeHost, block: &str, target: &str) -> Result<String, PluginError> {
    let reply = service_call(host, "status", json!({}))?;
    let rows = reply["targets"].as_array().expect("targets array").clone();
    let row = rows
        .iter()
        .find(|row| row["block"] == block && row["target"] == target)
        .unwrap_or_else(|| panic!("no status row for {block} / {target} in {rows:?}"));
    Ok(row["status"].as_str().expect("status string").to_string())
}

pub fn status_ok(host: &mut FakeHost, block: &str, target: &str) -> String {
    status_of(host, block, target).expect("status answers")
}

/// Marker text between the opening and closing marker of a block, both included.
pub fn block_of(text: &str, feature: &str) -> String {
    let open = format!("<!-- ilium-agent-feature: {feature}");
    let close = format!("<!-- /ilium-agent-feature: {feature} -->");
    let start = text
        .find(&open)
        .unwrap_or_else(|| panic!("no {feature} block in\n{text}"));
    let end = text.find(&close).expect("closing marker") + close.len();
    text[start..end].to_string()
}

/// Calls one method of the config-writer facade (API_GAPS_port.md).
pub fn configure(host: &mut FakeHost, method: &str, payload: Value) -> Result<Value, PluginError> {
    service_call(host, &format!("configure.{method}"), payload)
}

pub fn read_current(host: &mut FakeHost, target: &str) -> Result<Value, PluginError> {
    configure(host, "read-current", json!({ "target": target }))
}

pub fn plan(host: &mut FakeHost, target: &str, value: u64) -> Result<Value, PluginError> {
    configure(host, "plan", json!({ "target": target, "value": value }))
}

pub fn apply_plan(host: &mut FakeHost, plan: &Value) -> Result<Value, PluginError> {
    configure(host, "apply", json!({ "plan": plan }))
}

/// Plans and applies in one go; panics when either step is refused.
pub fn apply_value(host: &mut FakeHost, target: &str, value: u64) -> Value {
    let planned = plan(host, target, value).expect("plan");
    apply_plan(host, &planned).expect("apply")
}

pub fn revert_record(host: &mut FakeHost, record: &Value) -> Result<Value, PluginError> {
    configure(host, "revert", json!({ "record": record }))
}

pub fn load_record(host: &mut FakeHost, target: &str) -> Option<Value> {
    let reply =
        configure(host, "load-record", json!({ "target": target })).expect("load-record answers");
    if reply["record"].is_null() {
        None
    } else {
        Some(reply["record"].clone())
    }
}

pub fn text_of(host: &mut FakeHost, path: FsPath) -> String {
    get(host, path).expect("file exists")
}
