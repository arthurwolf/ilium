//! Shared helpers: the session with the scheduler on both sides, panes, dialog commands, readers.

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::{FakeHost, FakeSession, Screen, TreeNode};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, AgentActivityEvent, CommandContext,
    CommandInvokedEvent, CommandOrigin, Delivery, Event, Intent, NodeClass, NodeId, PluginError,
    SendInputSpec, Submission, ViewTarget,
};
use serde_json::Value;

/// The plugin id.
pub const PLUGIN: &str = "ilium.core.scheduler";
/// The service of the prompt queue (page "Services").
pub const QUEUE_SERVICE: &str = "ilium.core.scheduler/queue";
/// Assumed command ids (API_GAPS.md: the manifest lists no commands).
pub const SCHEDULE_COMMAND: &str = "ilium.core.scheduler.schedule-input";
pub const QUEUE_COMMAND: &str = "ilium.core.scheduler.queue-prompt";
/// Assumed popup view ids (API_GAPS.md).
pub const SCHEDULE_VIEW: &str = "scheduler.schedule";
pub const QUEUE_VIEW: &str = "scheduler.queue";
/// Content kinds (ilium-core PaneContentKind).
pub const TERMINAL_KIND: &str = "ilium.terminal";
pub const EDITOR_KIND: &str = "ilium.editor";

/// A session with the scheduler installed on both sides and started.
pub fn session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    session
}

/// Adds a pane of `content_kind` to both the server and the client tree; the ids agree.
pub fn add_pane(session: &mut FakeSession, name: &str, content_kind: &str) -> NodeId {
    let node = |name: &str, content_kind: &str| TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some(content_kind.to_string()),
        agent: None,
    };
    let server_id = session.server(PLUGIN).tree().add(node(name, content_kind));
    let client_id = session.client(PLUGIN).tree().add(node(name, content_kind));
    assert_eq!(
        server_id, client_id,
        "the two fake trees assign the same ids"
    );
    NodeId(server_id)
}

pub fn terminal_pane(session: &mut FakeSession, name: &str) -> NodeId {
    add_pane(session, name, TERMINAL_KIND)
}

pub fn server(session: &mut FakeSession) -> &mut FakeHost {
    session.server(PLUGIN)
}

pub fn client(session: &mut FakeSession) -> &mut FakeHost {
    session.client(PLUGIN)
}

/// One agent-activity-changed event.
pub fn activity_event(
    pane: NodeId,
    previous: AgentActivity,
    current: AgentActivity,
    revision: u64,
) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: pane,
        previous,
        current,
        revision,
    })
}

/// Delivers a finished turn (Working to Idle) on the server and returns the submissions.
pub fn finish_turn(session: &mut FakeSession, pane: NodeId, revision: u64) -> Vec<Submission> {
    server(session).deliver_one(activity_event(
        pane,
        AgentActivity::Working,
        AgentActivity::Idle,
        revision,
    ))
}

/// A call of the queue service (assumed JSON, API_GAPS.md).
pub fn call_queue(session: &mut FakeSession, request: Value) -> Result<Value, PluginError> {
    match session.call_service(PLUGIN, QUEUE_SERVICE, "*", &request.to_string())? {
        ServiceReply::Done(json) => Ok(serde_json::from_str(&json).expect("service reply is JSON")),
        ServiceReply::Deferred => panic!("the queue service answers at once"),
    }
}

/// Enqueues a prompt through the service; `mode` is once, times or forever.
pub fn enqueue(
    session: &mut FakeSession,
    pane: NodeId,
    text: &str,
    mode: &str,
    times: u32,
) -> Result<Value, PluginError> {
    call_queue(
        session,
        serde_json::json!({"method": "enqueue", "pane": pane.0, "text": text, "mode": mode, "times": times}),
    )
}

pub fn list_queue(session: &mut FakeSession, pane: NodeId) -> Vec<Value> {
    let reply = call_queue(
        session,
        serde_json::json!({"method": "list", "pane": pane.0}),
    )
    .expect("list answers");
    reply["entries"].as_array().cloned().unwrap_or_default()
}

/// Arms a scheduled input through the server service (assumed method `schedule`, API_GAPS.md).
pub fn schedule(
    session: &mut FakeSession,
    pane: NodeId,
    hours: u64,
    minutes: u64,
    seconds: u64,
    payload: &str,
    send_enter: bool,
) -> Result<Value, PluginError> {
    call_queue(
        session,
        serde_json::json!({
            "method": "schedule", "pane": pane.0, "hours": hours, "minutes": minutes,
            "seconds": seconds, "payload": payload, "send_enter": send_enter
        }),
    )
}

/// The send-input specs among the submissions, with their dedup keys.
pub fn sends(submissions: &[Submission]) -> Vec<(SendInputSpec, Option<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SendInput(spec) => Some((spec.clone(), submission.dedup_key.clone())),
            _ => None,
        })
        .collect()
}

/// The shape every plain send has (page sample 1, `plain_send`).
pub fn plain_spec(pane: NodeId, text: &str, delivery: Delivery) -> SendInputSpec {
    SendInputSpec {
        pane,
        text: text.to_string(),
        delivery,
        prompt_epoch: None,
        stages: vec![],
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: true,
    }
}

/// Invokes a menu command of the scheduler on the client with the pane as context.
pub fn invoke_menu(session: &mut FakeSession, command: &str, pane: NodeId) {
    let host = client(session);
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Menu,
        context: CommandContext {
            pane: Some(pane),
            ..Default::default()
        },
    }));
    host.settle();
}

pub fn popup(view: &str) -> ViewTarget {
    ViewTarget::Popup(view.to_string())
}

pub fn draw(session: &mut FakeSession, view: &str, columns: u16, rows: u16) -> Screen {
    client(session).render(popup(view), columns, rows)
}

pub fn press(session: &mut FakeSession, view: &str, chord: &str) {
    let host = client(session);
    host.key(popup(view), chord);
    host.settle();
}

/// Types text into the field with the given key (page sample field keys: hours, minutes, seconds, payload, prompt, times).
pub fn set_text(session: &mut FakeSession, view: &str, key: &str, text: &str) {
    let host = client(session);
    host.send_action(
        popup(view),
        key,
        ActionKind::Changed,
        ActionValue::Text(text.to_string()),
    );
    host.settle();
}

pub fn set_flag(session: &mut FakeSession, view: &str, key: &str, value: bool) {
    let host = client(session);
    host.send_action(
        popup(view),
        key,
        ActionKind::Toggled,
        ActionValue::Flag(value),
    );
    host.settle();
}

/// Lets both sides exchange their messages (bounded by the harness: 100 rounds).
pub fn pump(session: &mut FakeSession) {
    session.pump().expect("the session settles");
}
