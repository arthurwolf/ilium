//! The client side of the attention queue: the host-facing glue around [`State`].
//!
//! It scans the agent panes, keeps the queue current, sends the `analyse` job to the server side,
//! draws the panel and the overview, reports the count in the chrome and takes the user to the
//! next agent that needs them.

use std::cell::RefCell;

use ilium_plugin_sdk::host::{
    Bucket, GrabSpec, HostClock, HostKeys, HostPanes, HostSettings, HostStorage, HostTimers,
    HostTree,
};
use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{
    Alignment, Area, Event, GrabId, Intent, InvokeCommandSpec, MessageSpec, MessageTarget,
    NodeFlags, NodeId, OpenViewSpec, ParagraphSpec, PluginId, PluginSide, RichSpan,
    SetStatusSegmentSpec, Span, SpanTarget, Style, Submission, TimerId, ToastSpec, Tone, UiNode,
    UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{host, HostApi};
use serde_json::{json, Value};

use crate::instant::LivePane;
use crate::queue::PersistedQueue;
use crate::state::State;
use crate::view;

/// The plugin's id, the target of the messages to the server side.
pub const PLUGIN_ID: &str = "ilium.core.attention";
pub const PANEL: &str = "queue";
pub const OVERVIEW: &str = "overview";
pub const NEXT_COMMAND: &str = "ilium.core.attention/next";
pub const PANEL_COMMAND: &str = "ilium.core.attention/panel";
pub const OVERVIEW_COMMAND: &str = "ilium.core.attention/overview";
/// The keys the panel and the overview take while one of them is open.
const GRABBED_KEYS: [&str; 8] = ["escape", "enter", "up", "down", "left", "right", "s", "r"];
const STATUS_SEGMENT: &str = "attention";
const ACTIVITY_DETAIL_SIGNAL: &str = "activity-detail";
const FOCUS_NODE_COMMAND: &str = "kernel.focus-node";
const STORAGE_KEY: &str = "queue";
const TICK_MILLIS: u32 = 1_000;
const ARRIVAL_TOAST: &str = "attention-arrival";
const BRIEFING_TOAST: &str = "attention-briefing";
const NOTICE_TOAST: &str = "attention-notice";
const ARRIVAL_TOAST_MILLIS: u32 = 6_000;
const BRIEFING_TOAST_MILLIS: u32 = 12_000;
const NOTICE_TOAST_MILLIS: u32 = 3_000;

pub struct Client {
    pub state: State,
    timer: Option<TimerId>,
    grab: Option<GrabId>,
    panel_open: bool,
    overview_open: bool,
    /// Columns of the overview grid at the last render, so the arrow keys move by whole rows.
    columns: usize,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            state: State::new(),
            timer: None,
            grab: None,
            panel_open: false,
            overview_open: false,
            columns: 1,
        }
    }
}

fn panel_target() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

fn overview_target() -> ViewTarget {
    ViewTarget::Panel(OVERVIEW.to_string())
}

fn text(content: &str) -> Vec<RichSpan> {
    vec![RichSpan {
        text: content.to_string(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }]
}

fn toast(id: &str, tone: Tone, title: &str, body: Option<&str>, millis: u32) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some(id.to_string()),
        tone,
        title: view::clip(title, 60),
        body: body.map(|body| text(&view::clip(body, 200))),
        actions: Vec::new(),
        duration_ms: Some(millis),
    }))
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

fn focus_node(node: NodeId) -> Submission {
    Submission::new(Intent::InvokeCommand(InvokeCommandSpec {
        command: FOCUS_NODE_COMMAND.to_string(),
        arguments_json: json!({ "node": node.0 }).to_string(),
    }))
}

fn key_name(chord: &str) -> String {
    match chord.to_ascii_lowercase().as_str() {
        "esc" => "escape".to_string(),
        "return" => "enter".to_string(),
        other => other.to_string(),
    }
}

/// "claude" -> "Claude": the agent's name as the user knows it.
fn agent_label(signature: &str) -> String {
    let mut characters = signature.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => "The agent".to_string(),
    }
}

/// Every agent pane the host knows, as the instant layer sees it.
fn scan_panes(api: &mut dyn HostApi) -> Vec<LivePane> {
    let agents = HostAgent::agents(api);
    agents
        .into_iter()
        .map(|agent| {
            let node = HostTree::get(api, agent.pane);
            LivePane {
                pane_id: agent.pane,
                project_id: node.as_ref().and_then(|view| view.parent),
                title: node.map(|view| view.title).unwrap_or_default(),
                agent_label: agent_label(&agent.view.signature),
                activity: agent.view.activity,
                goal: agent.view.goal,
                waiting_background: false,
            }
        })
        .collect()
}

impl Client {
    /// Reads the settings and the stored queue and starts the one-second tick.
    pub fn activate(&mut self) {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        self.state.apply_settings(&HostSettings::get_json(api));
        self.state.focused = HostPanes::focused(api);
        if let Some(stored) = HostStorage::get(api, Bucket::Session, STORAGE_KEY.to_string())
            .and_then(|bytes| serde_json::from_slice::<PersistedQueue>(&bytes).ok())
        {
            self.state.set_pending_restore(stored);
        }
        self.timer = HostTimers::set(api, TICK_MILLIS, Some(TICK_MILLIS)).ok();
    }

    fn now(api: &mut dyn HostApi) -> u64 {
        HostClock::now_unix_millis(api)
    }

    /// One pass: scan, reconcile, announce, ask the model, save.
    fn cycle(&mut self, api: &mut dyn HostApi, forced: bool, out: &mut Vec<Submission>) {
        let now = Self::now(api);
        let panes = self.state.with_background(scan_panes(api));
        let cell = RefCell::new(&mut *api);
        let exists = |node: NodeId| HostTree::get(&mut **cell.borrow_mut(), node).is_some();
        let reconciled = self.state.reconcile(panes, &exists, now);
        let api = cell.into_inner();
        if let Some(item) = reconciled.arrival {
            out.push(toast(
                ARRIVAL_TOAST,
                Tone::Info,
                &format!("{}: {}", item.kind.label(), item.title),
                Some(&item.reason),
                ARRIVAL_TOAST_MILLIS,
            ));
        }
        if let Some(request) = self.state.analysis_request(now, forced) {
            out.push(to_server("analyse", &request));
        }
        if let Some(snapshot) = self.state.save_due(now) {
            if let Ok(bytes) = serde_json::to_vec(&snapshot) {
                let _ = HostStorage::put(api, Bucket::Session, STORAGE_KEY.to_string(), bytes);
            }
        }
        self.update_status(now, out);
    }

    /// Sends the chrome text when it changed: the count and the most urgent kind, or nothing.
    fn update_status(&mut self, now: u64, out: &mut Vec<Submission>) {
        let status = if self.state.settings.enabled() {
            view::status_text(&self.state.queue.ordered_for_display(now))
        } else {
            None
        };
        if status == self.state.shown_status {
            return;
        }
        match &status {
            Some(content) => out.push(Submission::new(Intent::SetStatusSegment(
                SetStatusSegmentSpec {
                    segment: STATUS_SEGMENT.to_string(),
                    content: text(content),
                    tooltip: None,
                    tone: Some(Tone::Warning),
                },
            ))),
            None => out.push(Submission::new(Intent::ClearStatusSegment(
                STATUS_SEGMENT.to_string(),
            ))),
        }
        self.state.shown_status = status;
    }

    fn off_notice() -> Submission {
        toast(
            NOTICE_TOAST,
            Tone::Muted,
            "The attention queue is off",
            Some("Turn it on in Settings, Attention."),
            NOTICE_TOAST_MILLIS,
        )
    }

    /// Takes the user to `pane`: records the visit, focuses it and shows the briefing.
    fn jump(&mut self, pane: NodeId, now: u64, out: &mut Vec<Submission>) {
        let briefing = if self.state.settings.briefing_enabled() {
            self.state.queue.item(pane).and_then(view::briefing_toast)
        } else {
            None
        };
        self.state.visit(pane, now);
        out.push(focus_node(pane));
        if let Some((title, body)) = briefing {
            out.push(toast(
                BRIEFING_TOAST,
                Tone::Info,
                &title,
                Some(&body),
                BRIEFING_TOAST_MILLIS,
            ));
        }
    }

    /// `next`: the agent that most needs the user, right now, from what is known. A press also
    /// refreshes the model's view in the background.
    fn press(&mut self, api: &mut dyn HostApi, out: &mut Vec<Submission>) {
        if !self.state.settings.enabled() {
            out.push(Self::off_notice());
            return;
        }
        // Fresh evidence first: the press must never jump to a stale item.
        self.cycle(api, self.state.settings.press_sorts, out);
        let now = Self::now(api);
        match self.state.target(now) {
            Some(pane) => self.jump(pane, now, out),
            None => out.push(toast(
                NOTICE_TOAST,
                Tone::Success,
                "All clear",
                Some("Nothing needs you right now."),
                NOTICE_TOAST_MILLIS,
            )),
        }
        self.update_status(now, out);
    }

    fn open(&mut self, view: ViewTarget, out: &mut Vec<Submission>) {
        out.push(Submission::new(Intent::OpenView(OpenViewSpec {
            view,
            focus: true,
        })));
    }

    fn on_command(&mut self, command: &str, api: &mut dyn HostApi, out: &mut Vec<Submission>) {
        match command {
            NEXT_COMMAND => self.press(api, out),
            PANEL_COMMAND => {
                if self.panel_open {
                    self.panel_open = false;
                    out.push(Submission::new(Intent::CloseView(panel_target())));
                } else if self.state.settings.enabled() {
                    self.panel_open = true;
                    self.open(panel_target(), out);
                } else {
                    out.push(Self::off_notice());
                }
            }
            OVERVIEW_COMMAND => {
                if self.state.settings.enabled() {
                    self.overview_open = true;
                    self.open(overview_target(), out);
                } else {
                    out.push(Self::off_notice());
                }
            }
            _ => {}
        }
    }

    /// Holds the keys while a view is open and gives them back when none is.
    fn sync_grab(&mut self, api: &mut dyn HostApi) {
        let open = self.panel_open || self.overview_open;
        match (open, self.grab) {
            (true, None) => {
                self.grab = HostKeys::grab(
                    api,
                    GrabSpec {
                        chords: GRABBED_KEYS.iter().map(|key| (*key).to_string()).collect(),
                        pane: None,
                        until_release: false,
                    },
                )
                .ok();
            }
            (false, Some(grab)) => {
                HostKeys::release(api, grab);
                self.grab = None;
            }
            _ => {}
        }
    }

    fn close_views(&mut self, out: &mut Vec<Submission>) {
        if std::mem::take(&mut self.panel_open) {
            out.push(Submission::new(Intent::CloseView(panel_target())));
        }
        if std::mem::take(&mut self.overview_open) {
            out.push(Submission::new(Intent::CloseView(overview_target())));
        }
    }

    fn on_key(&mut self, chord: &str, api: &mut dyn HostApi, out: &mut Vec<Submission>) {
        if !(self.panel_open || self.overview_open) {
            return;
        }
        let now = Self::now(api);
        let step = if self.overview_open {
            self.columns.max(1) as isize
        } else {
            1
        };
        match key_name(chord).as_str() {
            "escape" => self.close_views(out),
            "up" => self.state.move_selection(-step, now),
            "down" => self.state.move_selection(step, now),
            "left" if self.overview_open => self.state.move_selection(-1, now),
            "right" if self.overview_open => self.state.move_selection(1, now),
            "enter" => {
                if let Some(pane) = self.state.selected_pane(now) {
                    self.close_views(out);
                    self.jump(pane, now, out);
                }
            }
            "s" => {
                if let Some(pane) = self.state.selected_pane(now) {
                    self.state.snooze(pane, now);
                    self.update_status(now, out);
                }
            }
            "r" => self.cycle(api, true, out),
            _ => {}
        }
    }

    fn on_server_message(&mut self, channel: &str, payload: &[u8], api: &mut dyn HostApi) {
        let now = Self::now(api);
        match channel {
            "reply" => {
                if let Ok(value) = serde_json::from_slice::<Value>(payload) {
                    self.state.apply_reply(&value, now);
                }
            }
            "failed" => self.state.refresh_failed(now),
            _ => {}
        }
    }

    fn on_attention_changed(
        &mut self,
        node: NodeId,
        reason: Option<String>,
        by: Option<PluginId>,
        api: &mut dyn HostApi,
    ) {
        if !self.state.settings.enabled() {
            return;
        }
        let now = Self::now(api);
        let found = HostTree::get(api, node);
        let title = found
            .as_ref()
            .map(|view| view.title.clone())
            .unwrap_or_default();
        let project = found.and_then(|view| view.parent);
        let plugin = by.map(|id| id.0).unwrap_or_else(|| "plugin".to_string());
        self.state
            .plugin_mark(node, project, title, &plugin, reason, now);
    }

    /// The events of the client side.
    pub fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let mut out = Vec::new();
        let mut dirty = false;
        for event in batch {
            match event {
                Event::TimerFired(id) if Some(id) == self.timer => dirty = true,
                Event::AgentActivityChanged(_)
                | Event::AgentGoalChanged(_)
                | Event::AgentIdentified(_)
                | Event::PaneCreated(_)
                | Event::PaneExited(_)
                | Event::NodeRemoved(_)
                | Event::TreeChanged(_) => dirty = true,
                Event::SignalChanged(signal) if signal.signal == ACTIVITY_DETAIL_SIGNAL => {
                    self.state.observe_detail(signal.pane, &signal.value);
                    dirty = true;
                }
                Event::PaneFocused(pane) => {
                    self.state.focused = pane;
                    dirty = true;
                }
                Event::SettingsChanged(_) => {
                    self.state.apply_settings(&HostSettings::get_json(api));
                    dirty = true;
                }
                Event::AttentionChanged(change) => {
                    let reason = change.marked.then(|| change.reason.unwrap_or_default());
                    self.on_attention_changed(change.node, reason, change.by, api);
                    dirty = true;
                }
                Event::Message(message) if message.from_side == PluginSide::Server => {
                    self.on_server_message(&message.channel, &message.payload, api);
                    dirty = true;
                }
                Event::CommandInvoked(invoked) => {
                    self.on_command(&invoked.command, api, &mut out);
                }
                Event::KeyPressed(key) => self.on_key(&key.chord, api, &mut out),
                Event::ViewVisibility(visibility) => {
                    if visibility.view == panel_target() {
                        self.panel_open = visibility.visible;
                    } else if visibility.view == overview_target() {
                        self.overview_open = visibility.visible;
                    }
                }
                _ => {}
            }
        }
        if dirty {
            self.cycle(api, false, &mut out);
        }
        self.sync_grab(api);
        out
    }

    /// The panel or the overview, drawn as plain text.
    pub fn render(&mut self, target: &ViewTarget, area: Area) -> UiTree {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let now = Self::now(api);
        let width = usize::from(area.columns);
        let rows = usize::from(area.rows);
        let items = self.state.queue.ordered_for_display(now);
        let lines = if *target == panel_target() {
            view::panel_lines(&items, self.state.selected, width, rows, now)
        } else if *target == overview_target() {
            self.columns = view::overview_columns(width);
            view::overview_lines(&items, self.state.selected, width, rows, now)
        } else {
            return UiTree::default();
        };
        paragraph(lines)
    }
}

fn paragraph(lines: Vec<String>) -> UiTree {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_names_are_capitalised_and_an_unknown_one_has_a_fallback() {
        assert_eq!(agent_label("claude"), "Claude");
        assert_eq!(agent_label("codex"), "Codex");
        assert_eq!(agent_label(""), "The agent");
    }

    #[test]
    fn key_names_normalise_the_escape_and_enter_aliases() {
        assert_eq!(key_name("Esc"), "escape");
        assert_eq!(key_name("Return"), "enter");
        assert_eq!(key_name("S"), "s");
    }

    #[test]
    fn the_panel_and_the_overview_are_two_distinct_views_of_this_plugin() {
        assert_ne!(panel_target(), overview_target());
        assert_eq!(panel_target(), ViewTarget::Panel("queue".to_string()));
    }

    #[test]
    fn the_server_message_names_this_plugin_and_the_server_side() {
        let submission = to_server("analyse", &json!({"generation": 1}));
        let Intent::Message(spec) = submission.intent else {
            panic!("expected a message intent");
        };
        assert_eq!(spec.to.plugin.0, PLUGIN_ID);
        assert_eq!(spec.to.side, PluginSide::Server);
        assert_eq!(spec.channel, "analyse");
    }

    #[test]
    fn toasts_respect_the_host_limits() {
        let long = "x".repeat(400);
        let submission = toast("id", Tone::Info, &long, Some(&long), 5_000);
        let Intent::Toast(spec) = submission.intent else {
            panic!("expected a toast");
        };
        assert!(spec.title.chars().count() <= 60);
        let body: usize = spec
            .body
            .unwrap_or_default()
            .iter()
            .map(|span| span.text.chars().count())
            .sum();
        assert!(body <= 200);
    }
}
