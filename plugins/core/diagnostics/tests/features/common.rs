//! Helpers of the `features` target: the manual page and manifest readers, the log and report
//! fixtures, and the section and sink registrations. The session and service-call helpers are
//! shared with the `port` target and come from `session_support`.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{FsPath, Intent, PluginError, Submission};
use serde_json::{json, Value};

use crate::session_support::workspace_root;
pub use crate::session_support::{agent_debug, call, manifest_path, session};

/// The plugin id.
pub const PLUGIN_ID: &str = "ilium.core.diagnostics";
/// The report service of the manifest.
pub const REPORT_SERVICE: &str = "ilium.core.diagnostics/report";
/// The log folder grant of the page for user id 1000.
pub const LOG_SCOPE: &str = "path:/tmp/.ilium-1000/logs";

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
