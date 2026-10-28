//! Core plugin `ilium.core.agent-detect` (manual page core-plugin-agent-detect.md).
//!
//! Server side: the `detector` export (`identify` from the process tree, `classify` from a screen
//! snapshot) and the activity, goal and signal decorations. Client side: the Agents panel with its
//! Normal and Attention display modes. Plugin structs stay unit structs (tests build them as
//! literals), so per-instance state lives in thread-local cells.

use std::cell::RefCell;
use std::collections::BTreeMap;

use ilium_plugin_sdk::generated::exports::detector::Classification;
use ilium_plugin_sdk::generated::exports::detector::DetectorProcess;
use ilium_plugin_sdk::generated::exports::detector::DetectorScreen;
use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::{
    DecorationKey, DecorationVisibility, Intent, SetDecorationSpec, Submission,
};
use ilium_plugin_sdk::generated::types::layout::{Alignment, Area};
use ilium_plugin_sdk::generated::types::tree::AgentActivity;
use ilium_plugin_sdk::generated::types::ui::{
    NodeFlags, ParagraphSpec, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{NodeId, Span, Style};
use ilium_plugin_sdk::{ilium_plugin, Detector, Handler, Lifecycle, Views};

/// The ten braille frames of the working spinner.
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Frame time of the spinner in milliseconds (the host refuses less than 90).
const SPINNER_FRAME_MS: u16 = 100;
/// Slot ids declared in the manifest.
const ACTIVITY_SLOT: &str = "activity";
const GOAL_SLOT: &str = "goal";

/// One agent signature of the registry: id returned by `identify`, and the lowercase substrings that match.
struct SignatureEntry {
    id: &'static str,
    needles: &'static [&'static str],
}

/// Registry in match priority order. A new agent CLI is a new entry here, not a new branch.
const SIGNATURES: &[SignatureEntry] = &[
    SignatureEntry { id: "claude", needles: &["claude"] },
    SignatureEntry { id: "codex", needles: &["codex"] },
    SignatureEntry { id: "antigravity", needles: &["antigravity", "antimatter", "agy"] },
    SignatureEntry { id: "opencode", needles: &["opencode"] },
    SignatureEntry { id: "aider", needles: &["aider"] },
    SignatureEntry { id: "example-agent", needles: &["example-agent", "example"] },
];

fn file_stem_lowercase(argument: &str) -> String {
    let base = argument.rsplit('/').next().unwrap_or(argument);
    base.to_lowercase()
}

/// The signature id for one process: its name first, then the first two argv entries (a node launcher).
fn signature_of_process(process: &DetectorProcess) -> Option<&'static str> {
    let mut candidates = vec![process.name.to_lowercase()];
    candidates.extend(process.argv.iter().take(2).map(|argument| file_stem_lowercase(argument)));
    for candidate in &candidates {
        for entry in SIGNATURES {
            if entry.needles.iter().any(|needle| candidate.contains(needle)) {
                return Some(entry.id);
            }
        }
    }
    None
}

fn screen_text(screen: &DetectorScreen) -> Vec<&str> {
    screen.rows.iter().map(String::as_str).collect()
}

fn is_interrupt_marker(line: &str) -> bool {
    line.contains("esc to interrupt")
}

/// True for an elapsed-time token such as `48s`, `3m`, `1h` or `2m` directly after a digit.
fn has_elapsed_token(line: &str) -> bool {
    let characters: Vec<char> = line.chars().collect();
    characters.windows(2).enumerate().any(|(index, pair)| {
        pair[0].is_ascii_digit()
            && matches!(pair[1], 's' | 'm' | 'h')
            && characters
                .get(index + 2)
                .is_none_or(|next| !next.is_alphanumeric())
    })
}

/// Claude and generic live status: an ellipsis and an elapsed time on one line.
fn is_live_status_line(line: &str) -> bool {
    line.contains('…') && has_elapsed_token(line) && !line.contains("Goal not yet met")
}

/// Codex present-tense status: `Working (18m 26s ...`.
fn is_codex_live_status_line(line: &str) -> bool {
    let trimmed = line.trim_start_matches(|c: char| !c.is_alphanumeric()).trim_start();
    trimmed.starts_with("Working (") && has_elapsed_token(trimmed)
}

fn is_approval_line(line: &str) -> bool {
    const MARKERS: [&str; 12] = [
        "Allow this command?",
        "Do you want to",
        "Would you like to",
        "Do you trust",
        "(y/n)",
        "[y/n]",
        "Yes, proceed",
        "Enter to select",
        "Esc to cancel",
        "Press enter to confirm",
        "Allow Codex to",
        "needs your approval",
    ];
    MARKERS.iter().any(|marker| line.contains(marker))
}

/// Classifies the activity: the newest of working and approval evidence wins; approval wins a tie.
fn classify_activity(signature: &str, rows: &[&str]) -> AgentActivity {
    let is_codex = signature == "codex";
    let mut last_approval: Option<usize> = None;
    let mut last_working: Option<usize> = None;
    for (index, row) in rows.iter().enumerate() {
        if is_approval_line(row) {
            last_approval = Some(index);
        }
        let working = is_interrupt_marker(row)
            || if is_codex {
                is_codex_live_status_line(row)
            } else {
                is_live_status_line(row)
            };
        if working {
            last_working = Some(index);
        }
    }
    match (last_approval, last_working) {
        (Some(approval), Some(working)) if approval >= working => AgentActivity::AwaitingApproval,
        (Some(_), None) => AgentActivity::AwaitingApproval,
        (_, Some(_)) => AgentActivity::Working,
        (None, None) => AgentActivity::Idle,
    }
}

/// Claude shows its goal on a shared indicator row that ends with `◎ /goal <state>`.
fn claude_goal(rows: &[&str]) -> Option<String> {
    rows.iter().rev().take(8).find_map(|row| {
        let trimmed = row.trim_end();
        let at = trimmed.rfind("◎ /goal")?;
        let text = trimmed[at + "◎ ".len()..].trim();
        Some(text.to_string())
    })
}

/// Codex shows its goal in the footer after the status segments: `Pursuing goal (5m)`, `Goal paused (...)`.
fn codex_goal(rows: &[&str]) -> Option<String> {
    const TOKENS: [&str; 3] = ["Pursuing goal (", "Goal paused (", "Goal stalled ("];
    rows.iter().rev().take(6).find_map(|row| {
        for token in TOKENS {
            if let Some(at) = row.find(token) {
                // Prose that merely starts with the words is not a footer.
                if row[..at].trim().is_empty() {
                    continue;
                }
                let rest = &row[at..];
                let end = rest.find(')').map(|close| close + 1).unwrap_or(rest.len());
                return Some(rest[..end].to_string());
            }
        }
        None
    })
}

fn plain_text(content: &str) -> Vec<Span> {
    vec![Span { text: content.to_string(), style: Style::default() }]
}

fn activity_submission(node: NodeId, current: AgentActivity) -> Option<Submission> {
    let key = format!("activity-{}", node.0);
    let intent = match current {
        AgentActivity::None => Intent::ClearDecoration(DecorationKey {
            node,
            slot: ACTIVITY_SLOT.to_string(),
        }),
        AgentActivity::Working => Intent::SetDecoration(decoration_spec(
            node,
            ACTIVITY_SLOT,
            SPINNER_FRAMES[0],
            SPINNER_FRAMES.iter().map(|frame| plain_text(frame)).collect(),
        )),
        AgentActivity::AwaitingApproval => {
            Intent::SetDecoration(decoration_spec(node, ACTIVITY_SLOT, "?", Vec::new()))
        }
        AgentActivity::Idle => {
            Intent::SetDecoration(decoration_spec(node, ACTIVITY_SLOT, "●", Vec::new()))
        }
        AgentActivity::Done => {
            Intent::SetDecoration(decoration_spec(node, ACTIVITY_SLOT, "✓", Vec::new()))
        }
        AgentActivity::Menu => {
            Intent::SetDecoration(decoration_spec(node, ACTIVITY_SLOT, "≡", Vec::new()))
        }
    };
    Some(Submission::keyed(intent, key))
}

fn decoration_spec(
    node: NodeId,
    slot: &str,
    icon: &str,
    frames: Vec<Vec<Span>>,
) -> SetDecorationSpec {
    let frame_ms = if frames.is_empty() { 0 } else { SPINNER_FRAME_MS };
    SetDecorationSpec {
        node,
        slot: slot.to_string(),
        content: plain_text(icon),
        icon: Some(icon.to_string()),
        sort_key: None,
        persist: false,
        spans: None,
        tooltip: None,
        frames,
        frame_ms,
        trailing: false,
        visibility: DecorationVisibility::Always,
        tone: None,
    }
}

fn goal_submission(node: NodeId, goal: Option<String>) -> Submission {
    let key = format!("goal-{}", node.0);
    match goal {
        Some(text) => {
            let mut spec = decoration_spec(node, GOAL_SLOT, "", Vec::new());
            spec.icon = None;
            spec.content = plain_text(&format!("goal {text}"));
            spec.trailing = true;
            Submission::keyed(Intent::SetDecoration(spec), key)
        }
        None => Submission::keyed(
            Intent::ClearDecoration(DecorationKey { node, slot: GOAL_SLOT.to_string() }),
            key,
        ),
    }
}

fn signal_submission(node: NodeId, signal: &str, value: &str) -> Submission {
    let mut spec = decoration_spec(node, ACTIVITY_SLOT, "", Vec::new());
    spec.icon = None;
    spec.content = plain_text(&format!("{signal}: {value}"));
    Submission::new(Intent::SetDecoration(spec))
}

/// The server side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
pub struct AgentDetectServer;

impl Lifecycle for AgentDetectServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AgentDetectServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::AgentActivityChanged(change) => {
                    submissions.extend(activity_submission(change.node, change.current));
                }
                Event::AgentGoalChanged(change) => {
                    submissions.push(goal_submission(change.node, change.goal));
                }
                Event::AgentIdentified(identified) => {
                    ilium_plugin_sdk::log::log(
                        Level::Info,
                        &format!("node {} runs {}", identified.node.0, identified.signature),
                    );
                }
                Event::SignalChanged(signal) => {
                    submissions.push(signal_submission(signal.pane, &signal.signal, &signal.value));
                }
                _ => {}
            }
        }
        submissions
    }
}

impl Detector for AgentDetectServer {
    fn identify(&mut self, processes: Vec<DetectorProcess>) -> Option<String> {
        processes
            .iter()
            .find_map(signature_of_process)
            .map(str::to_string)
    }

    fn classify(&mut self, signature: String, screen: DetectorScreen) -> Classification {
        let rows = screen_text(&screen);
        let goal = match signature.as_str() {
            "claude" => claude_goal(&rows),
            "codex" => codex_goal(&rows),
            _ => None,
        };
        Classification {
            activity: classify_activity(&signature, &rows),
            objective: None,
            goal,
        }
    }
}

/// What the panel remembers about one agent pane.
#[derive(Debug, Clone)]
struct PanelAgent {
    title: String,
    signature: String,
    activity: AgentActivity,
    goal: Option<String>,
}

#[derive(Default)]
struct PanelState {
    agents: BTreeMap<u64, PanelAgent>,
    attention_mode: bool,
}

thread_local! {
    static PANEL: RefCell<PanelState> = RefCell::new(PanelState::default());
}

fn mode_is_attention(name: Option<&str>) -> bool {
    name == Some("attention")
}

fn agent_entry(state: &mut PanelState, node: NodeId) -> &mut PanelAgent {
    state.agents.entry(node.0).or_insert_with(|| PanelAgent {
        title: format!("node {}", node.0),
        signature: String::new(),
        activity: AgentActivity::None,
        goal: None,
    })
}

/// The client side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct AgentDetectClient;

impl Lifecycle for AgentDetectClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let settings: serde_json_free::Settings = serde_json_free::Settings::from(&context.settings_json);
        PANEL.with(|cell| {
            let mut state = cell.borrow_mut();
            state.agents.clear();
            state.attention_mode = settings.attention;
        });
        Ok(())
    }
}

/// Minimal reader for the one setting the client needs, so the crate needs no JSON dependency.
mod serde_json_free {
    pub struct Settings {
        pub attention: bool,
    }

    impl Settings {
        pub fn from(settings_json: &str) -> Settings {
            let attention = settings_json
                .find("agent_monitoring_mode")
                .map(|at| settings_json[at..].chars().take(48).collect::<String>())
                .is_some_and(|window| window.contains("\"attention\""));
            Settings { attention }
        }
    }
}

impl Handler for AgentDetectClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        PANEL.with(|cell| {
            let mut state = cell.borrow_mut();
            for event in batch {
                match event {
                    Event::DisplayModeChanged(mode) => {
                        state.attention_mode = mode_is_attention(mode.as_deref());
                    }
                    Event::AgentIdentified(identified) => {
                        let entry = agent_entry(&mut state, identified.node);
                        entry.signature = identified.signature;
                    }
                    Event::AgentActivityChanged(change) => {
                        agent_entry(&mut state, change.node).activity = change.current;
                    }
                    Event::AgentGoalChanged(change) => {
                        agent_entry(&mut state, change.node).goal = change.goal;
                    }
                    Event::PaneTitleChanged(change) => {
                        agent_entry(&mut state, change.node).title = change.title;
                    }
                    Event::NodeRemoved(node) | Event::PaneExited(ilium_plugin_sdk::types::PaneExitedEvent { node, .. }) => {
                        state.agents.remove(&node.0);
                    }
                    _ => {}
                }
            }
        });
        Vec::new()
    }
}

impl Views for AgentDetectClient {
    fn render(&mut self, _view: ViewTarget, area: Area) -> UiTree {
        let lines = PANEL.with(|cell| panel_lines(&cell.borrow(), usize::from(area.columns)));
        paragraph_tree(lines)
    }
}

fn activity_label(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::None => "-",
        AgentActivity::Working => "working",
        AgentActivity::Idle => "idle",
        AgentActivity::AwaitingApproval => "approval",
        AgentActivity::Done => "done",
        AgentActivity::Menu => "menu",
    }
}

fn needs_attention(activity: AgentActivity) -> bool {
    matches!(activity, AgentActivity::AwaitingApproval | AgentActivity::Done)
}

fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// Agent, State and Goal columns; attention mode keeps only the agents that need the user.
fn panel_lines(state: &PanelState, columns: usize) -> Vec<String> {
    let width = columns.max(20);
    let mut lines = vec![format!("{:<14} {:<9} {}", "Agent", "State", "Goal")];
    for agent in state.agents.values() {
        if state.attention_mode && !needs_attention(agent.activity) {
            continue;
        }
        let mut state_text = activity_label(agent.activity).to_string();
        if agent.activity == AgentActivity::Working {
            state_text = format!("{} {}", SPINNER_FRAMES[0], state_text);
        }
        if agent.activity == AgentActivity::AwaitingApproval {
            state_text = "? approval".to_string();
        }
        let goal = agent.goal.as_deref().unwrap_or("");
        let line = format!("{:<14} {:<9} {}", clip(&agent.title, 14), state_text, goal);
        lines.push(clip(&line, width));
    }
    if lines.len() == 1 {
        lines.push("No agents".to_string());
    }
    lines
}

fn paragraph_tree(lines: Vec<String>) -> UiTree {
    let spec = ParagraphSpec {
        lines: lines.iter().map(|line| plain_text(line)).collect(),
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    };
    UiTree {
        version: 1,
        nodes: vec![UiNode {
            key: None,
            widget: Widget::Paragraph(spec),
            children: Vec::new(),
            flags: NodeFlags(0),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_sdk::generated::types::tree::AgentActivity;

    fn process_named(name: &str) -> DetectorProcess {
        DetectorProcess {
            name: name.to_string(),
            argv: vec![name.to_string()],
        }
    }

    fn screen_with_rows(rows: &[&str]) -> DetectorScreen {
        DetectorScreen {
            rows: rows.iter().map(|row| row.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn identify_returns_none_for_an_empty_process_list() {
        let mut server = AgentDetectServer;
        assert_eq!(server.identify(Vec::new()), None);
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn identify_recognises_a_codex_process_in_the_tree() {
        let mut server = AgentDetectServer;
        let identified = server.identify(vec![process_named("bash"), process_named("codex")]);
        assert!(identified.is_some());
    }

    /// Manual: failure case: no agent in the tree
    #[test]
    fn identify_returns_none_when_only_a_plain_shell_runs() {
        let mut server = AgentDetectServer;
        assert_eq!(
            server.identify(vec![process_named("bash"), process_named("vim")]),
            None
        );
    }

    /// Manual: approval prompt maps to `awaiting-approval`
    #[test]
    fn classify_reports_awaiting_approval_when_the_approval_question_is_shown() {
        let mut server = AgentDetectServer;
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["Allow this command?", "> yes"]),
        );
        assert_eq!(classification.activity, AgentActivity::AwaitingApproval);
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn classify_reports_working_while_the_interrupt_hint_is_shown() {
        let mut server = AgentDetectServer;
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["Thinking about the parser", "esc to interrupt"]),
        );
        assert_eq!(classification.activity, AgentActivity::Working);
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn classify_reports_idle_when_no_marker_is_shown() {
        let mut server = AgentDetectServer;
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["> ready for the next task"]),
        );
        assert_eq!(classification.activity, AgentActivity::Idle);
    }

    /// Manual: an approval question takes precedence over the working hint
    #[test]
    fn classify_prefers_awaiting_approval_over_working_when_both_are_shown() {
        let mut server = AgentDetectServer;
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["esc to interrupt", "Allow this command?"]),
        );
        assert_eq!(classification.activity, AgentActivity::AwaitingApproval);
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn classify_leaves_objective_and_goal_unset_for_a_working_screen() {
        let mut server = AgentDetectServer;
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["esc to interrupt"]),
        );
        assert_eq!(classification.objective, None);
        assert_eq!(classification.goal, None);
    }

    /// Manual: empty-input case
    #[test]
    fn classify_an_empty_screen_is_idle() {
        let mut server = AgentDetectServer;
        let classification =
            server.classify("example-agent".to_string(), DetectorScreen::default());
        assert_eq!(classification.activity, AgentActivity::Idle);
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn activity_enum_case_names_match_the_manual_list() {
        assert_eq!(
            AgentActivity::CASE_NAMES,
            &[
                "none",
                "working",
                "idle",
                "awaiting-approval",
                "done",
                "menu"
            ]
        );
    }
}
