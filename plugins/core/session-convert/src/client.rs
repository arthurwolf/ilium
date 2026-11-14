//! The client side of a conversion: the context menu entry, the modal progress dialog and its keys.
//!
//! The client owns no conversion state of the server. It sends `start`, `cancel`, `resume` and `close` to the
//! server side and draws what the server side reports: `begin`, `step`, `progress`, `failed` and `finished`
//! (manual page "Client side", F-077). Only one conversion runs at a time, so one dialog model is enough.

use std::cell::RefCell;

use ilium_plugin_sdk::exports::{MenuContext, MenuEntry};
use ilium_plugin_sdk::host::{GrabSpec, HostKeys, HostTree};
use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{
    Alignment, Area, ContainerKind, Event, GrabId, Intent, MessageSpec, MessageTarget, NodeFlags,
    NodeId, NodeView, OpenViewSpec, ParagraphSpec, PluginId, PluginSide, Span, Style, Submission,
    UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{host, HostApi};
use serde_json::{json, Value};

use crate::flow::PLUGIN_ID;

/// The id of the modal panel the progress dialog lives in.
pub const PANEL: &str = "progress";
/// The command of the context menu entries.
const COMMAND: &str = "convert";
/// Width of the dialog in columns; a narrower area makes it narrower.
const DIALOG_WIDTH: usize = 48;
/// Cells of the progress bar.
const BAR_CELLS: usize = 24;
/// The reasons a menu entry is disabled, each at most 40 characters (the menu limit).
const REASON_NO_SESSION: &str = "no verified session";
const REASON_BUSY: &str = "another conversion runs";
const REASON_WORKTREE: &str = "worktree pane";

const CLAUDE_TO_CODEX_STEPS: [&str; 6] = [
    "Locate the Claude Code transcript",
    "Inspect the transcript",
    "Start a private codex app-server",
    "Import the session",
    "Verify the Codex rollout",
    "Stop the app-server",
];
const CODEX_TO_CLAUDE_STEPS: [&str; 6] = [
    "Locate the Codex rollout",
    "Parse the rollout",
    "Convert to Claude Code lines",
    "Write the Claude Code transcript",
    "Verify the transcript",
    "Register with Claude Code",
];

/// The failure the server side reported.
#[derive(Debug, Clone)]
struct Failure {
    message: String,
    carried: u64,
    left_out: u64,
    can_resume: bool,
}

/// What the dialog shows.
#[derive(Debug, Clone)]
struct Dialog {
    pane: u64,
    source: String,
    target: String,
    /// The current step, 1 based; 0 before the first step message.
    step: u32,
    percent: u32,
    message: String,
    failure: Option<Failure>,
}

#[derive(Default)]
struct ClientState {
    dialog: Option<Dialog>,
    /// The pane of the running conversion: set by a click and by `begin`, cleared by `finished`.
    running: Option<u64>,
    panel_open: bool,
    grab: Option<GrabId>,
}

thread_local! {
    static CLIENT: RefCell<ClientState> = RefCell::new(ClientState::default());
}

/// Forgets the dialog and the running conversion.
pub fn reset() {
    CLIENT.with(|cell| *cell.borrow_mut() = ClientState::default());
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

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// The agent a conversion of `signature` converts to, and the menu entry id of it.
fn counterpart(signature: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match signature {
        "claude" => Some(("codex", "to-codex", "Convert to Codex")),
        "codex" => Some(("claude", "to-claude", "Convert to Claude Code")),
        _ => None,
    }
}

fn in_worktree(api: &mut dyn HostApi, node: &NodeView) -> bool {
    let mut current = node.clone();
    for _ in 0..64 {
        if current.container == Some(ContainerKind::Worktree) {
            return true;
        }
        let Some(parent) = current.parent else {
            return false;
        };
        let Some(next) = HostTree::get(api, parent) else {
            return false;
        };
        current = next;
    }
    false
}

/// The reason a pane cannot be converted now, in the priority of the manual: no session, then a running
/// conversion, then a worktree.
fn disabled_reason(api: &mut dyn HostApi, node: &NodeView, running: bool) -> Option<&'static str> {
    if HostAgent::session(api, node.id).is_none() {
        Some(REASON_NO_SESSION)
    } else if running {
        Some(REASON_BUSY)
    } else if in_worktree(api, node) {
        Some(REASON_WORKTREE)
    } else {
        None
    }
}

/// The entry of the pane context menu: one entry for the other agent, or none.
pub fn menu_items(context: &MenuContext) -> Vec<MenuEntry> {
    let Some(pane) = context.pane.or(context.node) else {
        return Vec::new();
    };
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let Some(node) = HostTree::get(api, pane) else {
        return Vec::new();
    };
    let Some((_, id, label)) = node
        .agent
        .as_ref()
        .and_then(|agent| counterpart(&agent.signature))
    else {
        return Vec::new();
    };
    let running = CLIENT.with(|cell| cell.borrow().running.is_some());
    vec![MenuEntry {
        id: id.to_string(),
        label: label.to_string(),
        icon: Some("arrows-exchange".to_string()),
        shortcut: None,
        parent: None,
        is_submenu: false,
        separator_before: false,
        checked: None,
        disabled_reason: disabled_reason(api, &node, running).map(str::to_string),
        tooltip: None,
        danger: false,
    }]
}

/// Opens the panel and grabs Esc and Enter for it; the grab is best effort, the panel also takes the keys it
/// receives as a focused view.
fn open_dialog_panel(api: &mut dyn HostApi, out: &mut Vec<Submission>) {
    let needs_grab = CLIENT.with(|cell| {
        let mut state = cell.borrow_mut();
        if !state.panel_open {
            state.panel_open = true;
            out.push(Submission::new(Intent::OpenView(OpenViewSpec {
                view: panel(),
                focus: true,
            })));
        }
        state.grab.is_none()
    });
    if needs_grab {
        let grab = HostKeys::grab(
            api,
            GrabSpec {
                chords: vec!["escape".to_string(), "enter".to_string()],
                pane: None,
                until_release: false,
            },
        )
        .ok();
        CLIENT.with(|cell| cell.borrow_mut().grab = grab);
    }
}

/// Closes the panel, releases the keys and forgets the conversion.
fn close_dialog(api: &mut dyn HostApi, out: &mut Vec<Submission>) {
    let (grab, was_open) = CLIENT.with(|cell| {
        let mut state = cell.borrow_mut();
        let grab = state.grab.take();
        let was_open = state.panel_open;
        *state = ClientState::default();
        (grab, was_open)
    });
    if let Some(grab) = grab {
        HostKeys::release(api, grab);
    }
    if was_open {
        out.push(Submission::new(Intent::CloseView(panel())));
    }
}

fn steps_of(target: &str) -> &'static [&'static str; 6] {
    if target == "claude" {
        &CODEX_TO_CLAUDE_STEPS
    } else {
        &CLAUDE_TO_CODEX_STEPS
    }
}

fn agent_name(signature: &str) -> &'static str {
    if signature == "codex" {
        "Codex"
    } else {
        "Claude Code"
    }
}

/// A click on a menu entry: starts the conversion when the pane can be converted now.
fn on_command(
    api: &mut dyn HostApi,
    arguments_json: &str,
    context_pane: Option<NodeId>,
    context_item: Option<&str>,
    out: &mut Vec<Submission>,
) {
    let arguments: Value = serde_json::from_str(arguments_json).unwrap_or(Value::Null);
    let item = arguments
        .get("item")
        .and_then(Value::as_str)
        .or(context_item)
        .map(str::to_string);
    let pane = arguments
        .get("node")
        .and_then(Value::as_u64)
        .or(context_pane.map(|node| node.0));
    let (Some(item), Some(pane)) = (item, pane) else {
        return;
    };
    if CLIENT.with(|cell| cell.borrow().running.is_some()) {
        return;
    }
    let Some(node) = HostTree::get(api, NodeId(pane)) else {
        return;
    };
    let Some(source) = node.agent.as_ref().map(|agent| agent.signature.clone()) else {
        return;
    };
    let Some((target, id, _)) = counterpart(&source) else {
        return;
    };
    if item != id || disabled_reason(api, &node, false).is_some() {
        return;
    }
    CLIENT.with(|cell| {
        let mut state = cell.borrow_mut();
        state.running = Some(pane);
        state.dialog = Some(Dialog {
            pane,
            source: source.clone(),
            target: target.to_string(),
            step: 0,
            percent: 0,
            message: String::new(),
            failure: None,
        });
    });
    out.push(to_server("start", &json!({"pane": pane, "target": target})));
    open_dialog_panel(api, out);
}

fn json_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// A message of the server side.
fn on_server_message(
    api: &mut dyn HostApi,
    channel: &str,
    payload: &[u8],
    out: &mut Vec<Submission>,
) {
    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    match channel {
        "begin" => {
            let (Some(pane), Some(source), Some(target)) = (
                value.get("pane").and_then(Value::as_u64),
                value.get("source").and_then(Value::as_str),
                value.get("target").and_then(Value::as_str),
            ) else {
                return;
            };
            CLIENT.with(|cell| {
                let mut state = cell.borrow_mut();
                state.running = Some(pane);
                state.dialog = Some(Dialog {
                    pane,
                    source: source.to_string(),
                    target: target.to_string(),
                    step: 0,
                    percent: 0,
                    message: String::new(),
                    failure: None,
                });
            });
            open_dialog_panel(api, out);
        }
        "step" => {
            let Some(index) = value.get("index").and_then(Value::as_u64) else {
                return;
            };
            CLIENT.with(|cell| {
                if let Some(dialog) = cell.borrow_mut().dialog.as_mut() {
                    if dialog.failure.is_none() {
                        dialog.step = index.min(6) as u32;
                    }
                }
            });
        }
        "progress" => {
            let Some(percent) = value.get("percent").and_then(Value::as_f64) else {
                return;
            };
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            CLIENT.with(|cell| {
                if let Some(dialog) = cell.borrow_mut().dialog.as_mut() {
                    if dialog.failure.is_none() {
                        let clamped = percent.clamp(0.0, 100.0).floor() as u32;
                        dialog.percent = dialog.percent.max(clamped);
                        dialog.message = message;
                    }
                }
            });
        }
        "failed" => {
            let failure = Failure {
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("The conversion failed.")
                    .to_string(),
                carried: json_u64(&value, "carried"),
                left_out: json_u64(&value, "left_out"),
                can_resume: value
                    .get("can_resume")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            CLIENT.with(|cell| {
                if let Some(dialog) = cell.borrow_mut().dialog.as_mut() {
                    dialog.failure = Some(failure);
                }
            });
        }
        "finished" => {
            let pane = value.get("pane").and_then(Value::as_u64);
            let running = CLIENT.with(|cell| cell.borrow().running);
            if pane.is_none() || running.is_none() || pane == running {
                close_dialog(api, out);
            }
        }
        _ => {}
    }
}

/// Normalises the chord names of a key event: `Esc` and `escape`, `Enter` and `return` are the same keys.
fn key_name(chord: &str) -> &'static str {
    match chord.to_ascii_lowercase().as_str() {
        "esc" | "escape" => "escape",
        "enter" | "return" => "enter",
        _ => "",
    }
}

fn on_key(api: &mut dyn HostApi, chord: &str, out: &mut Vec<Submission>) {
    let Some(dialog) = CLIENT.with(|cell| cell.borrow().dialog.clone()) else {
        return;
    };
    let pane = json!({"pane": dialog.pane});
    match (key_name(chord), dialog.failure) {
        ("escape", None) => out.push(to_server("cancel", &pane)),
        ("escape", Some(_)) => {
            out.push(to_server("close", &pane));
            close_dialog(api, out);
        }
        ("enter", Some(failure)) if failure.can_resume => {
            out.push(to_server("resume", &pane));
        }
        ("enter", Some(_)) => {
            out.push(to_server("close", &pane));
            close_dialog(api, out);
        }
        _ => {}
    }
}

/// The events of the client side.
pub fn on_event(event: Event) -> Vec<Submission> {
    let mut out = Vec::new();
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    match event {
        Event::CommandInvoked(invoked) if invoked.command == COMMAND => on_command(
            api,
            &invoked.arguments_json,
            invoked.context.pane,
            invoked.context.menu_item.as_deref(),
            &mut out,
        ),
        Event::Message(message) if message.from_side == PluginSide::Server => {
            on_server_message(api, &message.channel, &message.payload, &mut out);
        }
        Event::KeyPressed(key) => on_key(api, &key.chord, &mut out),
        _ => {}
    }
    out
}

// ---- Drawing ----

/// Cuts `text` to `width` characters, ending in an ellipsis when it was longer.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut shortened: String = text.chars().take(width.saturating_sub(1)).collect();
    if width > 0 {
        shortened.push('…');
    }
    shortened
}

fn pad(text: &str, width: usize) -> String {
    let mut padded = cut(text, width);
    let missing = width.saturating_sub(padded.chars().count());
    padded.push_str(&" ".repeat(missing));
    padded
}

/// Greedy word wrap of `text` into at most `max_lines` lines of `width` characters; the last line is cut.
fn wrap(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed = if current.is_empty() { 0 } else { 1 } + word.chars().count();
        if !current.is_empty() && current.chars().count() + needed > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let mut joined = std::mem::take(last);
            joined.push('…');
            *last = cut(&joined, width);
        }
    }
    lines
}

/// `label … value`, filling the gap with the grey leader dots the settings forms use.
fn leader_row(label: &str, value: &str, width: usize) -> String {
    let used = label.chars().count() + value.chars().count() + 2;
    let dots = width.saturating_sub(used);
    if dots < 3 {
        return format!("{label} {value}");
    }
    format!("{label} {} {value}", "…".repeat(dots))
}

fn items(count: u64) -> String {
    if count == 1 {
        "1 item".to_string()
    } else {
        format!("{count} items")
    }
}

/// The text rows of the dialog, without the left margin that centres it.
fn dialog_rows(dialog: &Dialog, width: usize) -> Vec<String> {
    let inner = width.saturating_sub(4);
    let title = format!(
        "Convert {} to {}",
        agent_name(&dialog.source),
        agent_name(&dialog.target)
    );
    let title = cut(&title, width.saturating_sub(6));
    let head = format!("╭─ {title} ");
    let fill = width.saturating_sub(head.chars().count() + 1);
    let mut rows = vec![format!("{head}{}╮", "─".repeat(fill))];
    let body = |text: &str| format!("│ {} │", pad(text, inner));

    if let Some(failure) = &dialog.failure {
        rows.push(body("✕ Conversion failed"));
        for line in wrap(&failure.message, inner.saturating_sub(2), 3) {
            rows.push(body(&format!("  {line}")));
        }
        rows.push(body("  The original session is untouched."));
        if failure.carried + failure.left_out > 0 {
            rows.push(body(""));
            rows.push(body(&leader_row(
                "Carried over",
                &items(failure.carried),
                inner,
            )));
            rows.push(body(&leader_row(
                "Left out",
                &items(failure.left_out),
                inner,
            )));
        }
        rows.push(body(""));
        let buttons = if failure.can_resume {
            "[ Resume original ]   [ Close ]"
        } else {
            "[ Close ]"
        };
        rows.push(body(buttons));
    } else {
        for (index, title) in steps_of(&dialog.target).iter().enumerate() {
            let number = index as u32 + 1;
            let marker = if dialog.step == 0 || number > dialog.step {
                "○"
            } else if number == dialog.step {
                "▶"
            } else {
                "✓"
            };
            rows.push(body(&format!("{marker} {title}")));
        }
        rows.push(body(""));
        let filled = BAR_CELLS * dialog.percent.min(100) as usize / 100;
        rows.push(body(&format!(
            "{}{} {}%",
            "█".repeat(filled),
            "░".repeat(BAR_CELLS - filled),
            dialog.percent
        )));
        rows.push(body(&dialog.message));
        rows.push(body("Esc cancels"));
    }
    rows.push(format!("╰{}╯", "─".repeat(width.saturating_sub(2))));
    rows
}

fn paragraph_tree(lines: Vec<String>) -> UiTree {
    let lines = lines
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

/// The progress panel: the dialog centred in the area, or an empty panel when nothing converts.
pub fn render(view: &ViewTarget, area: Area) -> UiTree {
    if *view != panel() {
        return UiTree::default();
    }
    let columns = usize::from(area.columns);
    let width = DIALOG_WIDTH.min(columns);
    let margin = " ".repeat(columns.saturating_sub(width) / 2);
    let rows = CLIENT.with(|cell| {
        cell.borrow()
            .dialog
            .as_ref()
            .map(|dialog| dialog_rows(dialog, width))
    });
    let lines = rows
        .unwrap_or_default()
        .into_iter()
        .map(|row| format!("{margin}{row}"))
        .collect();
    paragraph_tree(lines)
}
