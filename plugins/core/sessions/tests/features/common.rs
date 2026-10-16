//! Shared helpers of the Sessions feature tests (manual page core-plugin-sessions.md).
//!
//! Every use of a testkit item that the skeleton lacks lives in the "gap helpers" section at the
//! end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_sessions::{SessionsClient, SessionsServer};
use ilium_plugin_sdk::primitives::{SessionRef, SignatureSpec};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, ClientId, Event, FreezePaneSpec, Intent, NodeClass, NodeId, PaneInfo,
    PaneLifecycleEvent, PaneLifecycleState, PrimitiveKind, ReplacePaneCommandSpec, Submission,
    TerminatePaneProcessSpec, UnfreezePaneSpec,
};
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.sessions";
/// Grace period of the graceful stop (page sample "Stop gracefully before a snapshot").
pub const GRACE_MS: u32 = 5_000;
/// Poll period while at least one client is attached (page sample "Detach and reattach").
pub const ATTACHED_POLL_MS: u32 = 1_000;
/// Poll period while no client is attached (page sample "Detach and reattach").
pub const DETACHED_POLL_MS: u32 = 30_000;

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-sessions`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-sessions")
}

/// A user document of the repository by file name, for example `session-recovery.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else { break };
        blocks.push(after[..end].to_string());
        rest = &after[end + 4..];
    }
    blocks
}

/// The complete manifest block of the page (the one that holds the whole manifest).
pub fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    let text = blocks.iter().find(|block| block.contains("\"name\": \"Sessions\"")).expect("complete manifest block");
    serde_json::from_str(text).expect("page manifest is JSON")
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else { break };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the table row of `text` whose first cell is `first_cell` (backticks kept).
pub fn table_row(text: &str, first_cell: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            cells.len() > 2 && cells[1] == first_cell
        })
        .unwrap_or_else(|| panic!("no table row starting with {first_cell}"));
    let mut cells: Vec<String> = line.split('|').map(|cell| cell.trim().to_string()).collect();
    cells.remove(0);
    cells.pop();
    cells
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build()
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build()
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<SessionsServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<SessionsClient>();
    host
}

/// An agent pane in the fake tree; returns its node id.
pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity: AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    })
}

/// A plain terminal pane (no agent) in the fake tree; returns its node id.
pub fn add_shell(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// `pane-created` for a pane, restored from a snapshot or not (`PaneInfo.is_restored`).
pub fn pane_created(pane: u64, is_restored: bool) -> Event {
    Event::PaneCreated(PaneInfo {
        node: NodeId(pane),
        content_kind: "ilium.terminal".to_string(),
        primitive: PrimitiveKind::Terminal,
        columns: 80,
        rows: 24,
        focused: false,
        title: format!("pane-{pane}"),
        is_restored,
    })
}

pub fn lifecycle_event(pane: u64, state: PaneLifecycleState) -> Event {
    Event::PaneLifecycleChanged(PaneLifecycleEvent { pane: NodeId(pane), state, by: None })
}

pub fn attached(client: u64) -> Event {
    Event::ClientAttached(ClientId(client))
}

pub fn detached(client: u64) -> Event {
    Event::ClientDetached(ClientId(client))
}

pub fn replace_commands(submissions: &[Submission]) -> Vec<(ReplacePaneCommandSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::ReplacePaneCommand(spec) => Some((spec.clone(), submission.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn freezes(submissions: &[Submission]) -> Vec<(FreezePaneSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::FreezePane(spec) => Some((spec.clone(), submission.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn unfreezes(submissions: &[Submission]) -> Vec<(UnfreezePaneSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::UnfreezePane(spec) => Some((spec.clone(), submission.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn terminations(submissions: &[Submission]) -> Vec<(TerminatePaneProcessSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::TerminatePaneProcess(spec) => Some((spec.clone(), submission.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

/// Resume templates of the user documentation table "Resuming agent sessions"
/// (src/docs/session-recovery.md): Claude Code, Codex, Antigravity.
pub fn claude_template() -> Vec<String> {
    vec!["claude".into(), "--resume".into(), "{session}".into()]
}

pub fn codex_template() -> Vec<String> {
    vec!["codex".into(), "resume".into(), "{session}".into()]
}

pub fn antigravity_template() -> Vec<String> {
    vec!["agy".into(), "--conversation".into(), "{session}".into()]
}

/// A signature with a resume template, as the agent-detect plugin registers it.
pub fn signature(id: &str, resume: Option<Vec<String>>) -> SignatureSpec {
    SignatureSpec {
        id: id.to_string(),
        process_names: vec![id.to_string()],
        resume,
        ..SignatureSpec::default()
    }
}

/// A verified session reference for the agent of a pane.
pub fn session_ref(session_id: &str) -> SessionRef {
    SessionRef {
        scope: "agent-store".to_string(),
        path: format!("/home/user/project/{session_id}.jsonl"),
        session_id: session_id.to_string(),
    }
}

/// Delivers `pane-created` with `is_restored` for the panes, in order, and returns the batch.
pub fn restore_panes(host: &mut FakeHost, panes: &[u64]) -> Vec<Submission> {
    let events: Vec<Event> = panes.iter().map(|pane| pane_created(*pane, true)).collect();
    let submissions = host.deliver(events);
    host.settle();
    submissions
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: every item below is missing from the skeleton testkit (see ../API_GAPS.md).
// ---------------------------------------------------------------------------------------------

/// Registers the signature with its resume template and gives the pane a verified session id.
pub fn verify_agent(host: &mut FakeHost, pane: u64, signature_spec: SignatureSpec, session_id: &str) {
    host.agent().add_signature(signature_spec);
    host.agent().set_session(pane, session_ref(session_id));
}

/// Registers the signature (with or without a resume template) and leaves the session unverified.
pub fn register_signature(host: &mut FakeHost, signature_spec: SignatureSpec) {
    host.agent().add_signature(signature_spec);
}

/// The timers the plugin holds and has not cancelled: (first delay ms, repeat ms), oldest first.
pub fn active_timers(host: &FakeHost) -> Vec<(u32, Option<u32>)> {
    host.active_timers()
}
