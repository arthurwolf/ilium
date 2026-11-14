//! The client side of the optimizer: the command that opens the panel, the panel's text and its keys.
//!
//! The client holds only what it draws. A scan is a job of the server side: the client sends `scan` with the agent
//! to scan and the server side answers `report` (the plain-text report) or `failed` (a message).

use std::cell::RefCell;

use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{
    Alignment, Area, Event, Intent, MessageSpec, MessageTarget, NodeFlags, OpenViewSpec,
    ParagraphSpec, PluginId, PluginSide, Span, Style, Submission, UiNode, UiTree, ViewTarget,
    Widget,
};
use ilium_plugin_sdk::{host, HostApi};
use serde_json::{json, Value};

use crate::optimize::{agent_for_signature, wire_name};

/// The plugin's id, the target of the messages to the server side.
pub const PLUGIN_ID: &str = "ilium.core.compaction";
/// The id of the optimizer panel.
pub const PANEL: &str = "optimizer";
/// The command that opens the optimizer for the focused agent pane.
pub const OPTIMIZE_COMMAND: &str = "optimize";

/// What the panel shows.
#[derive(Debug, Clone, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// The panel is open and waits for the scan key.
    Ready,
    Scanning,
    Report(Vec<String>),
    Failed(String),
}

#[derive(Debug, Default)]
struct PanelState {
    agent: String,
    phase: Phase,
    scroll: usize,
}

thread_local! {
    static PANEL_STATE: RefCell<PanelState> = RefCell::new(PanelState::default());
}

fn to_server(channel: &str, payload: &Value) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: channel.to_string(),
        payload: payload.to_string().into_bytes(),
    }))
}

fn panel_target() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// Starts a scan of `agent`; a scan already running is not started twice.
fn start_scan(agent: &str, out: &mut Vec<Submission>) {
    let started = PANEL_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.phase == Phase::Scanning {
            return false;
        }
        state.agent = agent.to_string();
        state.phase = Phase::Scanning;
        state.scroll = 0;
        true
    });
    if started {
        out.push(to_server("scan", &json!({ "agent": agent })));
    }
}

/// Opens the panel for `agent`; nothing is scanned until the scan key.
fn open_panel(agent: &str, out: &mut Vec<Submission>) {
    PANEL_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.agent = agent.to_string();
        if state.phase == Phase::Idle {
            state.phase = Phase::Ready;
        }
    });
    out.push(Submission::new(Intent::OpenView(OpenViewSpec {
        view: panel_target(),
        focus: true,
    })));
}

fn on_command(pane: Option<ilium_plugin_sdk::types::NodeId>, out: &mut Vec<Submission>) {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let signature = pane
        .and_then(|pane| HostAgent::view(api, pane))
        .map(|view| view.signature)
        .unwrap_or_default();
    open_panel(wire_name(agent_for_signature(&signature)), out);
}

fn on_server_message(channel: &str, payload: &[u8]) {
    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    PANEL_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        match channel {
            "report" => {
                let text = value.get("text").and_then(Value::as_str).unwrap_or("");
                state.phase = Phase::Report(text.lines().map(str::to_string).collect());
            }
            "failed" => {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("The scan failed.");
                state.phase = Phase::Failed(message.to_string());
            }
            _ => {}
        }
    });
}

fn key_name(chord: &str) -> String {
    match chord.to_ascii_lowercase().as_str() {
        "esc" => "escape".to_string(),
        other => other.to_string(),
    }
}

fn on_key(chord: &str, out: &mut Vec<Submission>) {
    let (open, agent) = PANEL_STATE.with(|cell| {
        let state = cell.borrow();
        (state.phase != Phase::Idle, state.agent.clone())
    });
    if !open {
        return;
    }
    match key_name(chord).as_str() {
        "escape" => {
            PANEL_STATE.with(|cell| *cell.borrow_mut() = PanelState::default());
            out.push(Submission::new(Intent::CloseView(panel_target())));
        }
        "s" | "r" => start_scan(&agent, out),
        "up" => PANEL_STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.scroll = state.scroll.saturating_sub(1);
        }),
        "down" => PANEL_STATE.with(|cell| cell.borrow_mut().scroll += 1),
        "pageup" => PANEL_STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.scroll = state.scroll.saturating_sub(20);
        }),
        "pagedown" => PANEL_STATE.with(|cell| cell.borrow_mut().scroll += 20),
        _ => {}
    }
}

/// The events of the client side.
pub fn on_event(event: Event) -> Vec<Submission> {
    let mut out = Vec::new();
    match event {
        Event::CommandInvoked(invoked) if invoked.command == OPTIMIZE_COMMAND => {
            on_command(invoked.context.pane, &mut out);
        }
        Event::Message(message) if message.from_side == PluginSide::Server => {
            on_server_message(&message.channel, &message.payload);
        }
        Event::KeyPressed(key) => on_key(&key.chord, &mut out),
        _ => {}
    }
    out
}

fn panel_lines(height: usize) -> Vec<String> {
    PANEL_STATE.with(|cell| {
        let state = cell.borrow();
        match &state.phase {
            Phase::Idle => Vec::new(),
            Phase::Ready => vec![
                format!("Compaction optimizer for {}.", state.agent),
                String::new(),
                "S scans the saved transcripts and recommends an auto-compact threshold."
                    .to_string(),
                "Esc closes.".to_string(),
            ],
            Phase::Scanning => vec![format!(
                "Scanning the {} transcripts ... (Esc closes)",
                state.agent
            )],
            Phase::Failed(message) => vec![
                message.clone(),
                String::new(),
                "S scans again, Esc closes.".to_string(),
            ],
            Phase::Report(lines) => {
                let last_start = lines.len().saturating_sub(height.max(1));
                let start = state.scroll.min(last_start);
                let mut shown: Vec<String> =
                    lines.iter().skip(start).take(height).cloned().collect();
                if let Some(footer) = shown.last_mut().filter(|_| lines.len() > height) {
                    *footer = "Up/Down scroll, S scans again, Esc closes.".to_string();
                }
                shown
            }
        }
    })
}

/// The optimizer panel.
pub fn render(view: &ViewTarget, area: Area) -> UiTree {
    if *view != panel_target() {
        return UiTree::default();
    }
    let lines = panel_lines(usize::from(area.rows))
        .into_iter()
        .map(|line| {
            vec![Span {
                text: line,
                style: Style::default(),
            }]
        })
        .collect();
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: None,
            widget: Widget::Paragraph(ParagraphSpec {
                lines,
                rich: None,
                wrap: Some(false),
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: false,
            }),
            children: Vec::new(),
            flags: NodeFlags::default(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}
