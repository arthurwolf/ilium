//! Shared helpers of the port twins: hosts for both sides, event builders and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_session_convert::{SessionConvertClient, SessionConvertServer};
use ilium_plugin_sdk::exports::{MenuContext, MenuEntry};
use ilium_plugin_sdk::primitives::SessionRef;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, CommandContext, CommandInvokedEvent, CommandOrigin, Event, FsPath, Intent, MessageEvent,
    NodeClass, NodeId, Outcome, PluginId, PluginSide, RequestRef, RequestResult, Submission, ViewTarget,
};
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.session-convert";
pub const SOURCE_SESSION: &str = "11111111-1111-4111-8111-111111111111";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.activate::<SessionConvertServer>();
    host
}

pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<SessionConvertClient>();
    host
}

pub fn add_agent_pane(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
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

/// A pane of `signature` with a verified session (client side tests).
pub fn pane_with_session(host: &mut FakeHost, signature: &str) -> u64 {
    let pane = add_agent_pane(host, signature, signature);
    host.agents().set_session(
        pane,
        Some(SessionRef {
            scope: format!("agent-sessions:{signature}"),
            path: format!("projects/p/{SOURCE_SESSION}.jsonl"),
            session_id: SOURCE_SESSION.to_string(),
        }),
    );
    pane
}

/// A server host with a pane of `signature`, its verified session and (optionally) the transcript file.
pub fn server_with_pane(signature: &str, with_transcript: bool) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = pane_with_session(&mut host, signature);
    if with_transcript {
        host.fs().write(
            FsPath { scope: format!("agent-sessions:{signature}"), path: format!("projects/p/{SOURCE_SESSION}.jsonl") },
            b"{\"type\":\"user\"}\n",
        );
    }
    (host, pane)
}

pub fn menu_context(pane: Option<u64>) -> MenuContext {
    MenuContext {
        menu: "pane".to_string(),
        node: pane.map(NodeId),
        pane: pane.map(NodeId),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    }
}

pub fn menu(host: &mut FakeHost, pane: Option<u64>) -> Vec<MenuEntry> {
    host.menu_items(menu_context(pane))
}

pub fn convert_click(item: &str, pane: u64) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: "convert".to_string(),
        arguments_json: format!(r#"{{"item":"{item}","node":{pane}}}"#),
        origin: CommandOrigin::Menu,
        context: CommandContext {
            pane: Some(NodeId(pane)),
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: Some(item.to_string()),
        },
    })
}

pub fn message_from(side: PluginSide, channel: &str, payload: &[u8]) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: side,
        from_client: None,
        channel: channel.to_string(),
        payload: payload.to_vec(),
    })
}

pub fn start_message(pane: u64, target: &str) -> Event {
    message_from(PluginSide::Client, "start", json!({"pane": pane, "target": target}).to_string().as_bytes())
}

pub fn client_message(channel: &str, pane: u64) -> Event {
    message_from(PluginSide::Client, channel, json!({"pane": pane}).to_string().as_bytes())
}

pub fn server_says(host: &mut FakeHost, channel: &str, payload: Value) {
    host.deliver_one(message_from(PluginSide::Server, channel, payload.to_string().as_bytes()));
}

pub fn panel() -> ViewTarget {
    ViewTarget::Panel("progress".to_string())
}

pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions.iter().map(|submission| submission.intent.clone()).collect()
}

pub fn frozen(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::FreezePane(spec) => Some(spec.pane.0),
            _ => None,
        })
        .collect()
}

pub fn unfrozen(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::UnfreezePane(spec) => Some(spec.pane.0),
            _ => None,
        })
        .collect()
}

pub fn terminated(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::TerminatePaneProcess(spec) => Some(spec.pane.0),
            _ => None,
        })
        .collect()
}

pub fn closed(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::CloseNode(node) => Some(node.0),
            _ => None,
        })
        .collect()
}

/// `(title, content_kind, command)` of every `create-pane` intent, in order.
pub fn created(submissions: &[Submission]) -> Vec<(String, String, Vec<String>)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::CreatePane(spec) => Some((spec.title.clone(), spec.content_kind.clone(), spec.command.clone().unwrap_or_default())),
            _ => None,
        })
        .collect()
}

/// `(pane, command, keep_scrollback)` of every `replace-pane-command` intent, in order.
pub fn replaced(submissions: &[Submission]) -> Vec<(u64, Vec<String>, bool)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::ReplacePaneCommand(spec) => Some((spec.pane.0, spec.command.clone(), spec.keep_scrollback)),
            _ => None,
        })
        .collect()
}

/// `(target side, channel, payload text)` of every `message` intent, in order.
pub fn messages(submissions: &[Submission]) -> Vec<(PluginSide, String, String)> {
    submissions
        .iter()
        .filter_map(|s| match &s.intent {
            Intent::Message(spec) => Some((spec.to.side, spec.channel.clone(), String::from_utf8_lossy(&spec.payload).to_string())),
            _ => None,
        })
        .collect()
}

pub fn toasts(submissions: &[Submission]) -> usize {
    submissions.iter().filter(|s| matches!(s.intent, Intent::Toast(_))).count()
}

/// Answers the `terminate-pane-process` submission with `outcome`, as the host would.
pub fn answer_terminate(host: &mut FakeHost, submissions: &[Submission], outcome: Outcome) -> Vec<Submission> {
    let terminate = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::TerminatePaneProcess(_)))
        .expect("terminate submitted");
    host.deliver_one(Event::RequestCompleted(RequestResult { request: RequestRef::Submission(terminate.correlation), outcome }))
}

/// Starts a conversion of the pane and completes the stop of the agent; returns the submissions of the start.
pub fn start_and_stop(host: &mut FakeHost, pane: u64, target: &str) -> Vec<Submission> {
    let submissions = host.deliver_one(start_message(pane, target));
    answer_terminate(host, &submissions, Outcome::Ok);
    submissions
}

/// The job output for `lines` converted lines claiming `kept` kept items.
pub fn job_output(lines: usize, kept: usize) -> Vec<u8> {
    let jsonl: Vec<String> = (0..lines).map(|index| json!({"type": "user", "n": index}).to_string()).collect();
    json!({"claude_jsonl": jsonl.join("\n"), "kept": kept, "dropped": 2}).to_string().into_bytes()
}

/// A Codex to Claude conversion whose job finished well; returns the host, the pane and everything submitted.
pub fn converted_codex_to_claude() -> (FakeHost, u64, Vec<Submission>) {
    let (mut host, pane) = server_with_pane("codex", true);
    start_and_stop(&mut host, pane, "claude");
    let request = host.jobs().list().first().expect("job started").request;
    host.jobs().finish(request, Ok(job_output(3, 3)));
    host.settle();
    let submissions = host.submitted();
    (host, pane, submissions)
}

pub fn failed_payload(submissions: &[Submission]) -> Option<Value> {
    messages(submissions)
        .into_iter()
        .rev()
        .find(|m| m.0 == PluginSide::Client && m.1 == "failed")
        .map(|m| serde_json::from_str(&m.2).expect("JSON payload"))
}
