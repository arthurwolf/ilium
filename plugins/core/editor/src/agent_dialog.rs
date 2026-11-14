//! The "Create agent from line" dialog: which agent, and the task it starts with.
//!
//! Moved from the kernel `Mode::CreateAgentFromLine` dialog (`ilium-client/src/agent_from_line.rs`,
//! `app.rs`, `keys.rs`, `mouse.rs`, `ui.rs`, `value_dialog_host.rs`). The kernel keeps the editor
//! line menu; its "Create agent from line" entry runs the command `ilium.core.editor/create-agent-from-line`
//! with the line it was opened on, and this module is everything that follows: the dialog state,
//! its rules and the view. `lib.rs` connects it to events and intents.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogButton,
    DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span, Style, TextAreaSpec, UiNode, UiTree,
    ViewTarget, Widget,
};

use crate::agent_from_line;

/// The command the kernel runs from the editor line menu.
pub const COMMAND: &str = "ilium.core.editor/create-agent-from-line";
/// Popup id of the dialog.
pub const POPUP: &str = "create-agent-from-line";
/// Key of the agent choice.
pub const AGENT_KEY: &str = "agent";
/// Key of the task text area.
pub const PROMPT_KEY: &str = "prompt";
/// Longest task the dialog accepts, in bytes.
pub const MAXIMUM_PROMPT_BYTES: usize = 64 * 1024;
/// Longest text of the clicked line the dialog keeps, in bytes.
const MAXIMUM_LINE_BYTES: usize = 64 * 1024;
/// Longest path of the file the dialog keeps, in bytes.
const MAXIMUM_PATH_BYTES: usize = 4 * 1024;
/// Shown when the task has no text.
pub const EMPTY_TASK: &str = "Agent task cannot be empty";
/// Shown when the task is longer than the limit.
pub const TASK_TOO_LONG: &str = "Agent task is longer than the limit";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// The agents the dialog offers, in the order of the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Antigravity,
}

impl Agent {
    /// Every agent, in choice order.
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Antigravity];

    /// The label shown in the choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
        }
    }

    /// The program a new pane of this agent runs.
    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
        }
    }

    fn index(self) -> u32 {
        match self {
            Self::Claude => 0,
            Self::Codex => 1,
            Self::Antigravity => 2,
        }
    }

    fn from_index(index: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }
}

/// The line the dialog was opened on, as the kernel handed it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The file the line belongs to.
    pub path: String,
    /// The line, counted from one.
    pub line_number: u32,
    /// The text of the line.
    pub text: String,
}

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// The group the pane goes into; `None` lets the kernel pick.
    pub parent: Option<u64>,
    /// The title of the pane.
    pub title: String,
    /// The program the pane runs.
    pub command: String,
    /// What the pane receives as its first input.
    pub prompt: String,
}

/// What the user decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Keep the dialog open.
    Stay,
    /// Close it and create nothing.
    Cancel,
    /// Close it and create this agent pane.
    Create(Created),
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    source: Source,
    parent: Option<u64>,
    agent: Agent,
    prompt: String,
    notice: Option<String>,
}

impl Dialog {
    /// A dialog for `source` with the first agent chosen and the line's `/goal` task ready to edit.
    pub fn new(source: Source, parent: Option<u64>) -> Self {
        let prompt = agent_from_line::default_prompt(
            &source.text,
            &source.path,
            source.line_number.saturating_sub(1),
        );
        Self {
            source,
            parent,
            agent: Agent::Claude,
            prompt,
            notice: None,
        }
    }

    /// Builds the dialog from the arguments of the command: `{ "path", "line", "text", "parent" }`
    /// where `line` counts from one. A missing path or line is refused; a missing parent lets the
    /// kernel pick the group.
    pub fn from_arguments(arguments_json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(arguments_json)
            .map_err(|_| "Create agent from line needs a JSON object".to_owned())?;
        let path = value
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty() && path.len() <= MAXIMUM_PATH_BYTES)
            .ok_or_else(|| "Create agent from line needs the path of the file".to_owned())?;
        let line_number = value
            .get("line")
            .and_then(serde_json::Value::as_u64)
            .and_then(|line| u32::try_from(line).ok())
            .filter(|line| *line >= 1)
            .ok_or_else(|| "Create agent from line needs the line number".to_owned())?;
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if text.len() > MAXIMUM_LINE_BYTES {
            return Err("The line is longer than the limit".to_owned());
        }
        let parent = value.get("parent").and_then(serde_json::Value::as_u64);
        Ok(Self::new(
            Source {
                path: path.to_owned(),
                line_number,
                text: text.to_owned(),
            },
            parent,
        ))
    }

    pub fn agent(&self) -> Agent {
        self.agent
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The agent pane this dialog would create, or why it cannot.
    pub fn answer(&self) -> Result<Created, &'static str> {
        if self.prompt.len() > MAXIMUM_PROMPT_BYTES {
            return Err(TASK_TOO_LONG);
        }
        if self.prompt.lines().all(|line| line.trim().is_empty()) {
            return Err(EMPTY_TASK);
        }
        Ok(Created {
            parent: self.parent,
            title: self.agent.command().to_owned(),
            command: self.agent.command().to_owned(),
            prompt: self.prompt.clone(),
        })
    }

    /// Applies one action the kernel reported for the popup.
    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text)))
                if key == PROMPT_KEY =>
            {
                self.prompt = text;
                self.notice = None;
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == AGENT_KEY =>
            {
                if let Some(agent) = Agent::from_index(index) {
                    self.agent = agent;
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "cancel" => Step::Cancel,
                "create" => match self.answer() {
                    Ok(created) => Step::Create(created),
                    Err(message) => {
                        self.notice = Some(message.to_owned());
                        Step::Stay
                    }
                },
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view of the dialog.
    pub fn render(&self) -> UiTree {
        let mut message = vec![format!(
            "{} line {}",
            self.source.path, self.source.line_number
        )];
        message.extend(self.notice.clone());
        let dialog = DialogSpec {
            title: vec![span("Create agent from line")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("create", "Create agent", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(88),
            height: Constraint::Min(0),
            message: Some(message.iter().map(|line| vec![span(line)]).collect()),
        };
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1, 2],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                node(
                    AGENT_KEY,
                    Widget::Choice(ChoiceSpec {
                        label: "Agent".to_owned(),
                        options: Agent::ALL
                            .iter()
                            .map(|agent| agent.label().to_owned())
                            .collect(),
                        selected: self.agent.index(),
                        style: ChoiceStyle::List,
                        items: Vec::new(),
                        source: None,
                        selected_id: None,
                        filter: false,
                        view_modes: Vec::new(),
                        disabled_marker: DisabledMarker::Reason,
                        allow_custom: false,
                    }),
                ),
                node(
                    PROMPT_KEY,
                    Widget::TextArea(TextAreaSpec {
                        initial: self.prompt.clone(),
                        placeholder: Some("Task prompt".to_owned()),
                        wrap: true,
                        line_numbers: false,
                        max_bytes: u32::try_from(MAXIMUM_PROMPT_BYTES).unwrap_or(0),
                        debounce_ms: 250,
                        tab_moves_focus: true,
                    }),
                ),
            ],
            root: 0,
        }
    }
}

fn span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

fn button(id: &str, label: &str, role: ButtonRole, is_cancel: bool) -> DialogButton {
    DialogButton {
        id: id.to_owned(),
        label: label.to_owned(),
        role,
        is_cancel,
    }
}

fn node(key: &str, widget: Widget) -> UiNode {
    UiNode {
        key: Some(key.to_owned()),
        widget,
        children: Vec::new(),
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> Dialog {
        Dialog::new(
            Source {
                path: "/work/src/main.rs".to_owned(),
                line_number: 42,
                text: "fix_this();".to_owned(),
            },
            Some(9),
        )
    }

    fn text(value: &str) -> Option<ActionValue> {
        Some(ActionValue::Text(value.to_owned()))
    }

    #[test]
    fn starts_with_the_first_agent_and_the_goal_of_the_line() {
        let dialog = dialog();
        assert_eq!(dialog.agent(), Agent::Claude);
        assert!(dialog.prompt().contains("fix_this();"));
        assert!(dialog.prompt().contains("/work/src/main.rs"));
        assert!(dialog.prompt().contains("at line 42"));
    }

    #[test]
    fn the_agents_step_through_every_provider_in_order() {
        let labels: Vec<_> = Agent::ALL.iter().map(|agent| agent.label()).collect();
        assert_eq!(labels, ["Claude", "Codex", "Antigravity"]);
        let commands: Vec<_> = Agent::ALL.iter().map(|agent| agent.command()).collect();
        assert_eq!(commands, ["claude", "codex", "agy"]);
        for agent in Agent::ALL {
            assert_eq!(Agent::from_index(agent.index()), Some(agent));
        }
        assert_eq!(Agent::from_index(3), None);
    }

    #[test]
    fn choosing_an_agent_keeps_the_authored_prompt() {
        let mut dialog = dialog();
        dialog.act(PROMPT_KEY, "", ActionKind::Changed, text("my own task"));
        dialog.act(
            AGENT_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(1)),
        );
        assert_eq!(dialog.agent(), Agent::Codex);
        assert_eq!(dialog.prompt(), "my own task");
    }

    #[test]
    fn an_unknown_choice_index_changes_nothing() {
        let mut dialog = dialog();
        dialog.act(
            AGENT_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(99)),
        );
        assert_eq!(dialog.agent(), Agent::Claude);
    }

    #[test]
    fn create_hands_over_the_agent_its_command_and_the_edited_prompt() {
        let mut dialog = dialog();
        dialog.act(
            PROMPT_KEY,
            "",
            ActionKind::Changed,
            text("do the work\nnow"),
        );
        dialog.act(
            AGENT_KEY,
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(2)),
        );
        let Step::Create(created) = dialog.act("dialog", "create", ActionKind::Activated, None)
        else {
            panic!("a valid dialog creates");
        };
        assert_eq!(created.parent, Some(9));
        assert_eq!(created.command, "agy");
        assert_eq!(created.title, "agy");
        assert_eq!(created.prompt, "do the work\nnow");
    }

    #[test]
    fn an_empty_task_is_refused_and_the_dialog_stays_open_with_its_text() {
        let mut dialog = dialog();
        dialog.act(PROMPT_KEY, "", ActionKind::Changed, text(" \n\t "));
        let step = dialog.act("dialog", "create", ActionKind::Activated, None);
        assert_eq!(step, Step::Stay);
        assert_eq!(dialog.notice(), Some(EMPTY_TASK));
        assert_eq!(dialog.prompt(), " \n\t ");
    }

    #[test]
    fn a_task_over_the_limit_is_refused() {
        let mut dialog = dialog();
        dialog.act(
            PROMPT_KEY,
            "",
            ActionKind::Changed,
            text(&"x".repeat(MAXIMUM_PROMPT_BYTES + 1)),
        );
        assert_eq!(dialog.answer(), Err(TASK_TOO_LONG));
    }

    #[test]
    fn cancel_and_close_end_the_dialog() {
        let mut dialog = dialog();
        assert_eq!(
            dialog.act("dialog", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
        assert_eq!(
            dialog.act("dialog", "", ActionKind::Closed, None),
            Step::Cancel
        );
    }

    #[test]
    fn the_command_arguments_make_a_dialog_and_a_bad_one_is_refused() {
        let made =
            Dialog::from_arguments(r#"{"path":"/a/b.rs","line":3,"text":"TODO x","parent":4}"#)
                .expect("valid arguments");
        assert_eq!(made.source().line_number, 3);
        assert_eq!(made.answer().expect("valid").parent, Some(4));
        assert!(Dialog::from_arguments("not json").is_err());
        assert!(Dialog::from_arguments(r#"{"line":3}"#).is_err());
        assert!(Dialog::from_arguments(r#"{"path":"/a","line":0}"#).is_err());
        let without_parent =
            Dialog::from_arguments(r#"{"path":"/a","line":1}"#).expect("a parent is optional");
        assert_eq!(without_parent.answer().expect("valid").parent, None);
    }

    #[test]
    fn the_view_holds_the_dialog_the_agent_choice_and_the_task() {
        let tree = dialog().render();
        assert!(matches!(tree.nodes[0].widget, Widget::Dialog(_)));
        assert_eq!(tree.nodes[0].children, vec![1, 2]);
        let keys: Vec<_> = tree.nodes[1..]
            .iter()
            .filter_map(|node| node.key.as_deref())
            .collect();
        assert_eq!(keys, [AGENT_KEY, PROMPT_KEY]);
    }
}
