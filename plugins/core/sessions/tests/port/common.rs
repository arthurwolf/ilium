//! Shared helpers of the port twins. Items missing from the skeleton testkit are used only in the
//! "gap helpers" section (see ../API_GAPS.md, part ports).
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_sessions::SessionsServer;
use ilium_plugin_sdk::primitives::{SessionRef, SignatureSpec};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, Event, FreezePaneSpec, Intent, NodeClass, NodeId, PaneInfo,
    PaneLifecycleEvent, PaneLifecycleState, PrimitiveKind, ReplacePaneCommandSpec, Submission,
    UnfreezePaneSpec,
};

pub const SESSION_ID: &str = "verified-session";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn activated_server() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SessionsServer>();
    host
}

fn add_pane(host: &mut FakeHost, name: &str, agent: Option<AgentView>) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent,
    })
}

pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
    add_pane(
        host,
        name,
        Some(AgentView {
            signature: signature.to_string(),
            activity: AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    )
}

pub fn add_shell(host: &mut FakeHost, name: &str) -> u64 {
    add_pane(host, name, None)
}

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
    Event::PaneLifecycleChanged(PaneLifecycleEvent {
        pane: NodeId(pane),
        state,
        by: None,
    })
}

/// Delivers `pane-created` with `is_restored` for the panes and lets the host settle.
pub fn restore_panes(host: &mut FakeHost, panes: &[u64]) -> Vec<Submission> {
    let events: Vec<Event> = panes.iter().map(|pane| pane_created(*pane, true)).collect();
    let submissions = host.deliver(events);
    host.settle();
    submissions
}

pub fn replace_commands(
    submissions: &[Submission],
) -> Vec<(ReplacePaneCommandSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::ReplacePaneCommand(spec) => Some((spec.clone(), s.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn freezes(submissions: &[Submission]) -> Vec<(FreezePaneSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::FreezePane(spec) => Some((spec.clone(), s.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn unfreezes(submissions: &[Submission]) -> Vec<(UnfreezePaneSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::UnfreezePane(spec) => Some((spec.clone(), s.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

pub fn claude_template() -> Vec<String> {
    vec!["claude".into(), "--resume".into(), "{session}".into()]
}

pub fn codex_template() -> Vec<String> {
    vec!["codex".into(), "resume".into(), "{session}".into()]
}

pub fn signature(id: &str, resume: Option<Vec<String>>) -> SignatureSpec {
    SignatureSpec {
        id: id.to_string(),
        process_names: vec![id.to_string()],
        resume,
        ..SignatureSpec::default()
    }
}

pub fn session_ref(session_id: &str) -> SessionRef {
    SessionRef {
        scope: "agent-store".to_string(),
        path: format!("/home/user/project/{session_id}.jsonl"),
        session_id: session_id.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: items missing from the skeleton testkit (see ../API_GAPS.md, part ports).
// ---------------------------------------------------------------------------------------------

pub fn verify_agent(
    host: &mut FakeHost,
    pane: u64,
    signature_spec: SignatureSpec,
    session_id: &str,
) {
    host.agents().add_signature(signature_spec);
    host.agents().set_session(pane, session_ref(session_id));
}

pub fn register_signature(host: &mut FakeHost, signature_spec: SignatureSpec) {
    host.agents().add_signature(signature_spec);
}
