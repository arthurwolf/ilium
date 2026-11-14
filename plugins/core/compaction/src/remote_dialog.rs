//! The client side of remote compaction: the Compact command, the modal progress dialog and its keys.
//!
//! The client owns no state of a run. It sends `remote-start`, `remote-cancel`, `remote-resume`, `remote-close`
//! and `remote-dismiss-banner` to the server side and draws what the server side reports: `remote-begin`,
//! `remote-phase`, `remote-log`, `remote-progress`, `remote-failed` and `remote-finished`. Only one run exists at
//! a time, so one dialog model is enough.

use std::cell::RefCell;

use ilium_plugin_sdk::host::{GrabSpec, HostKeys};
use ilium_plugin_sdk::types::{
    Alignment, Area, Event, GrabId, Intent, MessageSpec, MessageTarget, NodeFlags, OpenViewSpec,
    ParagraphSpec, PluginId, PluginSide, Span, Style, Submission, UiNode, UiTree, ViewTarget,
    Widget,
};
use ilium_plugin_sdk::{host, HostApi};
use serde_json::{json, Value};

use crate::remote_flow::PLUGIN_ID;

/// The id of the modal panel the dialog lives in.
pub const PANEL: &str = "remote-compaction";
/// The command of the Compact button and of the command palette entry.
pub const COMMAND: &str = "compact";
/// Width of the dialog in columns; a narrower area makes it narrower.
const DIALOG_WIDTH: usize = 64;
/// Cells of the progress bar.
const BAR_CELLS: usize = 32;
/// How many log lines the dialog shows.
const LOG_ROWS: usize = 4;

/// The phases the server side reports, in the order of the dialog's step list.
const STEPS: [&str; 3] = [
    "Wait for the agent to finish its turn",
    "Stop the agent",
    "Summarize and rewrite the transcript",
];

/// The failure the server side reported.
#[derive(Debug, Clone)]
struct Failure {
    message: String,
    can_resume: bool,
}

/// What the dialog shows.
#[derive(Debug, Clone, Default)]
struct Dialog {
    pane: u64,
    agent: String,
    technique: String,
    banner: bool,
    /// The current step of [`STEPS`], 1 based; 0 before the first phase message.
    step: usize,
    percent: u32,
    title: String,
    log: Vec<String>,
    before: u64,
    after: u64,
    failure: Option<Failure>,
}

#[derive(Default)]
struct ClientState {
    dialog: Option<Dialog>,
    panel_open: bool,
    grab: Option<GrabId>,
}

thread_local! {
    static CLIENT: RefCell<ClientState> = RefCell::new(ClientState::default());
}

fn to_server(channel: &str, payload: &Value) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: format!("remote-{channel}"),
        payload: payload.to_string().into_bytes(),
    }))
}

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

/// Opens the panel and grabs Esc and Enter for it; the grab is best effort, the modal panel also receives the
/// keys as the focused view.
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
                chords: vec!["escape".to_string(), "enter".to_string(), "d".to_string()],
                pane: None,
                until_release: false,
            },
        )
        .ok();
        CLIENT.with(|cell| cell.borrow_mut().grab = grab);
    }
}

/// Closes the panel, releases the keys and forgets the run.
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

fn json_u64(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn json_text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn with_dialog(change: impl FnOnce(&mut Dialog)) {
    CLIENT.with(|cell| {
        if let Some(dialog) = cell.borrow_mut().dialog.as_mut() {
            change(dialog);
        }
    });
}

/// A message of the server side.
fn on_server_message(
    api: &mut dyn HostApi,
    channel: &str,
    payload: &[u8],
    out: &mut Vec<Submission>,
) {
    let Some(channel) = channel.strip_prefix("remote-") else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    match channel {
        "begin" => {
            let Some(pane) = value.get("pane").and_then(Value::as_u64) else {
                return;
            };
            CLIENT.with(|cell| {
                cell.borrow_mut().dialog = Some(Dialog {
                    pane,
                    agent: json_text(&value, "agent"),
                    technique: json_text(&value, "technique"),
                    banner: value
                        .get("banner")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    step: 1,
                    ..Dialog::default()
                });
            });
            open_dialog_panel(api, out);
        }
        "phase" => {
            let step = match value.get("phase").and_then(Value::as_str) {
                Some("stopping") => 2,
                Some("compacting") => 3,
                _ => 1,
            };
            with_dialog(|dialog| {
                if dialog.failure.is_none() {
                    dialog.step = step;
                }
            });
        }
        "log" => {
            let lines: Vec<String> = value
                .get("log")
                .and_then(Value::as_array)
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|line| line.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            with_dialog(|dialog| dialog.log = lines);
        }
        "progress" => {
            let percent = value.get("percent").and_then(Value::as_f64).unwrap_or(0.0);
            let title = json_text(&value, "title");
            let tokens = value.get("tokens").cloned().unwrap_or(Value::Null);
            let log: Option<Vec<String>> =
                value.get("log").and_then(Value::as_array).map(|lines| {
                    lines
                        .iter()
                        .filter_map(|line| line.as_str().map(str::to_string))
                        .collect()
                });
            with_dialog(|dialog| {
                if dialog.failure.is_some() {
                    return;
                }
                dialog.percent = dialog.percent.max(percent.clamp(0.0, 100.0).floor() as u32);
                if !title.is_empty() {
                    dialog.title = title;
                }
                if let Some(log) = log {
                    if !log.is_empty() {
                        dialog.log = log;
                    }
                }
                dialog.before = json_u64(&tokens, "before_context");
                dialog.after = json_u64(&tokens, "after_context");
            });
        }
        "failed" => {
            let failure = Failure {
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Remote compaction failed.")
                    .to_string(),
                can_resume: value
                    .get("can_resume")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            with_dialog(|dialog| dialog.failure = Some(failure));
        }
        "finished" => close_dialog(api, out),
        _ => {}
    }
}

/// Normalises the chord names of a key event: `Esc` and `escape`, `Enter` and `return` are the same keys.
fn key_name(chord: &str) -> &'static str {
    match chord.to_ascii_lowercase().as_str() {
        "esc" | "escape" => "escape",
        "enter" | "return" => "enter",
        "d" => "d",
        _ => "",
    }
}

fn on_key(api: &mut dyn HostApi, chord: &str, out: &mut Vec<Submission>) {
    let Some(dialog) = CLIENT.with(|cell| cell.borrow().dialog.clone()) else {
        return;
    };
    let pane = json!({ "pane": dialog.pane });
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
        ("d", _) if dialog.banner => {
            with_dialog(|dialog| dialog.banner = false);
            out.push(to_server("dismiss-banner", &pane));
        }
        _ => {}
    }
}

/// The events of the client side the dialog handles. Returns the submissions, and whether the event was its own.
pub fn on_event(event: &Event, out: &mut Vec<Submission>) {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    match event {
        Event::CommandInvoked(invoked) if invoked.command == COMMAND => {
            if let Some(pane) = invoked.context.pane {
                out.push(to_server("start", &json!({ "pane": pane.0 })));
            }
        }
        Event::Message(message) if message.from_side == PluginSide::Server => {
            on_server_message(api, &message.channel, &message.payload, out);
        }
        Event::KeyPressed(key) => on_key(api, &key.chord, out),
        _ => {}
    }
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
    if dots < 5 {
        return format!("{label} {value}");
    }
    format!("{label} {} {value}", "…".repeat(dots))
}

/// The text rows of the dialog, without the left margin that centres it.
fn dialog_rows(dialog: &Dialog, width: usize) -> Vec<String> {
    let inner = width.saturating_sub(4);
    let title = cut(
        &format!(
            "Remote compaction of {} ({})",
            dialog.agent, dialog.technique
        ),
        width.saturating_sub(6),
    );
    let head = format!("╭─ {title} ");
    let fill = width.saturating_sub(head.chars().count() + 1);
    let mut rows = vec![format!("{head}{}╮", "─".repeat(fill))];
    let body = |text: &str| format!("│ {} │", pad(text, inner));

    if let Some(failure) = &dialog.failure {
        rows.push(body("✕ Remote compaction failed"));
        for line in wrap(&failure.message, inner.saturating_sub(2), 3) {
            rows.push(body(&format!("  {line}")));
        }
        rows.push(body("  The original conversation is untouched."));
        rows.push(body(""));
        let buttons = if failure.can_resume {
            "[ Enter: resume the original session ]   [ Esc: close ]"
        } else {
            "[ Enter or Esc: close ]"
        };
        rows.push(body(buttons));
    } else {
        for (index, title) in STEPS.iter().enumerate() {
            let number = index + 1;
            let marker = if number > dialog.step {
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
        if !dialog.title.is_empty() {
            rows.push(body(&dialog.title));
        }
        if dialog.before > 0 {
            rows.push(body(&leader_row(
                "Context before",
                &format!("{} tokens", dialog.before),
                inner,
            )));
        }
        if dialog.after > 0 {
            rows.push(body(&leader_row(
                "Context after",
                &format!("{} tokens", dialog.after),
                inner,
            )));
        }
        let skip = dialog.log.len().saturating_sub(LOG_ROWS);
        for line in dialog.log.iter().skip(skip) {
            rows.push(body(&format!("  {line}")));
        }
        if dialog.banner {
            rows.push(body(""));
            for line in wrap(
                "The conversation is sent to your configured inference model to be summarized. D hides this notice.",
                inner,
                3,
            ) {
                rows.push(body(&line));
            }
        }
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

/// The progress panel: the dialog centred in the area, or an empty panel when no run exists.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> Dialog {
        Dialog {
            pane: 4,
            agent: "Claude Code".to_string(),
            technique: "Claude Code".to_string(),
            step: 3,
            percent: 50,
            title: "Summarizing chunk 1 of 2".to_string(),
            before: 120_000,
            after: 30_000,
            log: vec!["one".to_string(), "two".to_string()],
            ..Dialog::default()
        }
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::render_shows_stages_tokens_destination_and_banner
    #[test]
    fn the_running_dialog_shows_steps_progress_and_tokens() {
        let rows = dialog_rows(&dialog(), 64).join("\n");
        assert!(rows.contains("✓ Wait for the agent to finish its turn"));
        assert!(rows.contains("▶ Summarize and rewrite the transcript"));
        assert!(rows.contains("50%"));
        assert!(rows.contains("Context before"));
        assert!(rows.contains("120000 tokens"));
        assert!(rows.contains("Esc cancels"));
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::failed_dialog_offers_resume_once_the_agent_is_stopped
    #[test]
    fn the_failed_dialog_offers_resume_only_when_the_agent_was_stopped() {
        let mut failed = dialog();
        failed.failure = Some(Failure {
            message: "The model refused.".to_string(),
            can_resume: true,
        });
        let with_resume = dialog_rows(&failed, 64).join("\n");
        assert!(with_resume.contains("resume the original session"));
        assert!(with_resume.contains("The original conversation is untouched."));
        failed.failure = Some(Failure {
            message: "The agent did not pause.".to_string(),
            can_resume: false,
        });
        let without = dialog_rows(&failed, 64).join("\n");
        assert!(!without.contains("resume the original session"));
        assert!(without.contains("Enter or Esc: close"));
    }

    #[test]
    fn every_dialog_row_has_the_same_width() {
        let mut state = dialog();
        state.banner = true;
        let rows = dialog_rows(&state, 64);
        let widths: Vec<usize> = rows.iter().map(|row| row.chars().count()).collect();
        assert!(widths.iter().all(|width| *width == 64), "{widths:?}");
    }

    #[test]
    fn only_the_dialog_panel_draws() {
        let area = Area {
            columns: 80,
            rows: 24,
        };
        assert_eq!(
            render(&ViewTarget::Panel("optimizer".to_string()), area),
            UiTree::default()
        );
        assert_ne!(render(&panel(), area), UiTree::default());
    }

    #[test]
    fn chord_names_are_normalised() {
        assert_eq!(key_name("Esc"), "escape");
        assert_eq!(key_name("Return"), "enter");
        assert_eq!(key_name("D"), "d");
        assert_eq!(key_name("x"), "");
    }
}
