//! Shared helpers: hosts for both sides, sessions, page and manifest readers, service callers.

// This module is compiled into both the `features` and `port` test targets (the port twins mount it
// with #[path]); each target uses a different subset of the helpers, so the rest look dead per target.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_diagnostics::{DiagnosticsClient, DiagnosticsServer};
use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::{FakeHost, FakeSession};
use ilium_plugin_sdk::types::{FsPath, Intent, PluginError, Submission};
use serde_json::{json, Value};

/// The plugin id.
pub const PLUGIN_ID: &str = "ilium.core.diagnostics";
/// The report service of the manifest.
pub const REPORT_SERVICE: &str = "ilium.core.diagnostics/report";
/// The agent debug history service assumed for F-032 (API_GAPS.md item 1).
pub const AGENT_DEBUG_SERVICE: &str = "ilium.core.diagnostics/agent-debug";
/// The log folder grant of the page for user id 1000.
pub const LOG_SCOPE: &str = "path:/tmp/.ilium-1000/logs";
/// The caller of service calls in tests.
pub const CALLER: &str = "ilium.core.kanban";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/diagnostics).
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
    page("core-plugin-diagnostics.md")
}

/// The manifest file as JSON.
pub fn manifest_json() -> Value {
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

/// The cells of the first table row whose line starts with `prefix`.
pub fn table_row(text: &str, prefix: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("row {prefix:?} missing"));
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<DiagnosticsServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<DiagnosticsClient>();
    host
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

/// Builds the report with `include_log_tail`.
pub fn build_report(
    session: &mut FakeSession,
    include_log_tail: bool,
) -> Result<String, PluginError> {
    let reply = call(
        session,
        REPORT_SERVICE,
        json!({ "method": "build", "include_log_tail": include_log_tail }),
    )?;
    Ok(reply["text"]
        .as_str()
        .expect("text is a string")
        .to_string())
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

/// The intents of a submission list, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions
        .iter()
        .map(|submission| submission.intent.clone())
        .collect()
}

/// A log line in the format the page tail sample reads.
pub fn log_line(level: &str, text: &str) -> String {
    format!("2026-10-12 09:14:03 {level} {text}\n")
}

/// Writes a file into the fake log folder of the server host.
pub fn write_log(session: &mut FakeSession, name: &str, bytes: &[u8]) {
    let path = FsPath {
        scope: LOG_SCOPE.to_string(),
        path: name.to_string(),
    };
    session.server(PLUGIN_ID).fs().write(path, bytes);
}

/// Registers a section contributor and a text service that answers `body` (API_GAPS.md item 2).
pub fn provide_section(
    session: &mut FakeSession,
    contributor: &str,
    id: &str,
    title: &str,
    body: &str,
) {
    let service = format!("{contributor}/report");
    session.contribute(
        contributor,
        "ilium.core.diagnostics/report-sections",
        id,
        50,
        json!({ "title": title, "service": service }),
    );
    session.stub_service(contributor, &service, "text", Ok(body.to_string()));
}

/// Registers a section whose text service fails with `error` (API_GAPS.md item 2).
pub fn fail_section(
    session: &mut FakeSession,
    contributor: &str,
    id: &str,
    title: &str,
    error: PluginError,
) {
    let service = format!("{contributor}/report");
    session.contribute(
        contributor,
        "ilium.core.diagnostics/report-sections",
        id,
        50,
        json!({ "title": title, "service": service }),
    );
    session.stub_service(contributor, &service, "text", Err(error));
}

/// Registers a section whose text service never answers (API_GAPS.md item 2).
pub fn stall_section(session: &mut FakeSession, contributor: &str, id: &str, title: &str) {
    let service = format!("{contributor}/report");
    session.contribute(
        contributor,
        "ilium.core.diagnostics/report-sections",
        id,
        50,
        json!({ "title": title, "service": service }),
    );
    session.stall_service(contributor, &service, "text");
}

/// The text of the warnings panel on the client side, rendered at the given size.
pub fn warnings_panel(session: &mut FakeSession, columns: u16, rows: u16) -> String {
    session
        .client(PLUGIN_ID)
        .render(
            ilium_plugin_sdk::types::ViewTarget::Panel("warnings".to_string()),
            columns,
            rows,
        )
        .text()
}

/// Appends text to a log file of the fake log folder, keeping the earlier bytes (a log only grows).
pub fn append_log(session: &mut FakeSession, name: &str, more: &str) {
    let path = FsPath {
        scope: LOG_SCOPE.to_string(),
        path: name.to_string(),
    };
    let mut bytes = session
        .server(PLUGIN_ID)
        .fs()
        .read(path)
        .unwrap_or_default();
    bytes.extend_from_slice(more.as_bytes());
    write_log(session, name, &bytes);
}

/// Registers a recording observer on `log-sinks` (API_GAPS.md item 3).
pub fn observe_sinks(session: &mut FakeSession, contributor: &str) {
    session.observe_hook(contributor, "ilium.core.diagnostics/log-sinks", 50);
}

/// The chains that `contributor` received on `log-sinks`, each as the list of its payloads (API_GAPS.md item 3).
pub fn sink_chains(session: &mut FakeSession, contributor: &str) -> Vec<Vec<Value>> {
    session.hook_chains(contributor, "ilium.core.diagnostics/log-sinks")
}
