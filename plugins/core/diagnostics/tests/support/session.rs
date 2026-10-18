//! Helpers mounted by both the `features` and the `port` test targets: the workspace and manifest
//! paths, a session with the plugin installed, and the service calls of the report and agent debug
//! services.

use std::path::PathBuf;

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::PluginError;
use serde_json::{json, Value};

/// The caller of service calls in tests.
pub const CALLER: &str = "ilium.core.kanban";
/// The agent debug history service assumed for F-032 (API_GAPS.md item 1).
pub const AGENT_DEBUG_SERVICE: &str = "ilium.core.diagnostics/agent-debug";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/diagnostics).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A session with the plugin installed and started, plus the settings stub and a caller stub.
pub fn session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("settings");
    session.install_core("kanban");
    session
        .install(workspace_root().join("plugins/core/diagnostics"))
        .start();
    session
}

/// Calls a service of the plugin with a JSON request and parses the JSON reply.
pub fn call(
    session: &mut FakeSession,
    service: &str,
    request: Value,
) -> Result<Value, PluginError> {
    let reply = session.call_service(CALLER, service, "^0.1", &request.to_string())?;
    match reply {
        ServiceReply::Done(text) => Ok(serde_json::from_str(&text).expect("the reply is JSON")),
        ServiceReply::Deferred => panic!("the reply is still deferred after the session settled"),
    }
}

/// Calls one method of the assumed agent-debug service.
pub fn agent_debug(
    session: &mut FakeSession,
    method: &str,
    mut fields: Value,
) -> Result<Value, PluginError> {
    fields["method"] = json!(method);
    call(session, AGENT_DEBUG_SERVICE, fields)
}
