//! The "New split view" dialog: pick an orientation, then optionally the panes that join.
//!
//! Everything here is plain data and rules; `lib.rs` connects it to events, the tree and
//! intents. The kernel opens the dialog by running the `new-split` command with the group to
//! split in; the plugin reads the tree itself. Which panes may join, how they are labelled and
//! how many fit is decided here.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, CheckItem, CheckListSpec, ChoiceSpec, ChoiceStyle,
    Constraint, DialogButton, DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span,
    SplitDirection, Style, TriState, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::types::{AgentActivity, ContainerKind, NodeClass, NodeView};
use serde_json::Value;

/// The most panes one split view holds.
pub const MAXIMUM_PANES: usize = 4;
/// Popup id of the dialog.
pub const POPUP: &str = "new-split";
/// Key of the orientation choice.
pub const ORIENTATION_KEY: &str = "orientation";
/// Key of the member check list.
pub const MEMBERS_KEY: &str = "members";
/// Shown when a fifth pane is picked.
pub const FULL_NOTICE: &str = "A split view can contain at most four panes";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// One node of the tree, reduced to what the dialog rules use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFact {
    pub id: u64,
    pub parent: Option<u64>,
    /// `project`, `group`, `split`, `worktree`, `folder` or `pane`.
    pub kind: String,
    /// `terminal`, `editor` or `board`; panes only.
    pub content: Option<String>,
    /// `agent` or `agent unavailable`; panes that run one only.
    pub agent: Option<String>,
    pub title: String,
}

/// A pane the member picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub pane: u64,
    pub label: String,
    pub selected: bool,
}

fn fact<'a>(nodes: &'a [TreeFact], id: u64) -> Option<&'a TreeFact> {
    nodes.iter().find(|node| node.id == id)
}

/// The ancestors of a node, nearest first. Stops at a missing parent and never loops.
fn ancestors<'a>(nodes: &'a [TreeFact], id: u64) -> Vec<&'a TreeFact> {
    let mut found = Vec::new();
    let mut current = fact(nodes, id).and_then(|node| node.parent);
    while let Some(parent) = current.and_then(|parent| fact(nodes, parent)) {
        if found.len() > nodes.len() {
            break;
        }
        found.push(parent);
        current = parent.parent;
    }
    found
}

/// Whether a pane may join a new split view: not already in one, and not inside a worktree.
pub fn is_eligible(nodes: &[TreeFact], pane: u64) -> bool {
    let above = ancestors(nodes, pane);
    above.first().is_some_and(|parent| parent.kind != "split")
        && above.iter().all(|node| node.kind != "worktree")
}

/// `[kind] a / b / pane`: what the pane is, then its path below the root.
pub fn label(nodes: &[TreeFact], root: u64, pane: u64) -> String {
    let Some(node) = fact(nodes, pane) else {
        return String::new();
    };
    let kind = node
        .agent
        .as_deref()
        .or(node.content.as_deref())
        .unwrap_or("pane");
    let mut path: Vec<&str> = ancestors(nodes, pane)
        .into_iter()
        .filter(|ancestor| ancestor.id != root)
        .map(|ancestor| ancestor.title.as_str())
        .collect();
    path.reverse();
    path.push(node.title.as_str());
    format!("[{kind}] {}", path.join(" / "))
}

/// The panes the picker offers, in tree order.
pub fn choices(nodes: &[TreeFact], root: u64, panes: &[u64]) -> Vec<Choice> {
    panes
        .iter()
        .copied()
        .filter(|pane| is_eligible(nodes, *pane))
        .map(|pane| Choice {
            pane,
            label: label(nodes, root, pane),
            selected: false,
        })
        .collect()
}

fn direction_name(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Vertical => "vertical",
        SplitDirection::Horizontal => "horizontal",
    }
}

/// What the dialog asks the server side to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub parent: u64,
    pub direction: SplitDirection,
    pub panes: Vec<u64>,
}

impl Answer {
    pub fn to_payload(&self) -> Vec<u8> {
        serde_json::json!({
            "parent": self.parent,
            "direction": direction_name(self.direction),
            "panes": self.panes,
        })
        .to_string()
        .into_bytes()
    }

    /// Reads an answer; `None` for anything malformed or with more than four panes.
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        let direction = match value.get("direction")?.as_str()? {
            "vertical" => SplitDirection::Vertical,
            "horizontal" => SplitDirection::Horizontal,
            _ => return None,
        };
        let panes: Vec<u64> = value
            .get("panes")?
            .as_array()?
            .iter()
            .map(Value::as_u64)
            .collect::<Option<_>>()?;
        if panes.len() > MAXIMUM_PANES {
            return None;
        }
        Some(Self {
            parent: value.get("parent")?.as_u64()?,
            direction,
            panes,
        })
    }
}

/// Which page of the dialog is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Orientation,
    Members,
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Still open (it may show something new).
    Stay,
    /// Finished: create this split view.
    Create(Answer),
    /// Finished without creating anything.
    Cancel,
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub parent: u64,
    pub direction: SplitDirection,
    pub page: Page,
    pub choices: Vec<Choice>,
    pub notice: Option<String>,
}

/// The group to split in, from the `new-split` command arguments `{"parent": id}`.
pub fn parent_argument(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("parent")?.as_u64()
}

impl TreeFact {
    /// The fact of one node as the tree import reports it.
    pub fn from_view(view: &NodeView) -> Self {
        let kind = match (view.class, view.container) {
            (NodeClass::Pane, _) => "pane",
            (NodeClass::Folder, _) => "folder",
            (NodeClass::Container, Some(ContainerKind::Project)) => "project",
            (NodeClass::Container, Some(ContainerKind::Worktree)) => "worktree",
            (
                NodeClass::Container,
                Some(ContainerKind::SplitHorizontal | ContainerKind::SplitVertical),
            ) => "split",
            (NodeClass::Container, Some(ContainerKind::Group) | None) => "group",
        };
        Self {
            id: view.id.0,
            parent: view.parent.map(|parent| parent.0),
            kind: kind.to_owned(),
            content: view
                .content_kind
                .as_deref()
                .map(|content| content.strip_prefix("ilium.").unwrap_or(content).to_owned()),
            agent: view.agent.as_ref().map(|agent| {
                match agent.activity {
                    AgentActivity::None => "agent unavailable",
                    _ => "agent",
                }
                .to_owned()
            }),
            title: view.title.clone(),
        }
    }
}

impl Dialog {
    /// Opens the dialog for a group, from the tree: `nodes` are the facts of every node and
    /// `panes` the pane ids in drawn order.
    pub fn from_facts(parent: u64, root: u64, nodes: &[TreeFact], panes: &[u64]) -> Self {
        Self {
            parent,
            direction: SplitDirection::Vertical,
            page: Page::Orientation,
            choices: choices(nodes, root, panes),
            notice: None,
        }
    }

    pub fn selected(&self) -> Vec<u64> {
        self.choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.pane)
            .collect()
    }

    fn answer(&self, panes: Vec<u64>) -> Step {
        Step::Create(Answer {
            parent: self.parent,
            direction: self.direction,
            panes,
        })
    }

    /// Checks or unchecks one pane; a fifth pane is refused with a notice.
    pub fn toggle(&mut self, pane: u64, on: bool) {
        self.notice = None;
        let full = self.selected().len() >= MAXIMUM_PANES;
        let Some(choice) = self.choices.iter_mut().find(|choice| choice.pane == pane) else {
            return;
        };
        if on && !choice.selected && full {
            self.notice = Some(FULL_NOTICE.to_owned());
            return;
        }
        choice.selected = on;
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
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == ORIENTATION_KEY =>
            {
                self.direction = if index == 0 {
                    SplitDirection::Vertical
                } else {
                    SplitDirection::Horizontal
                };
                Step::Stay
            }
            (ActionKind::Toggled, Some(ActionValue::ItemState((id, state))))
                if key == MEMBERS_KEY =>
            {
                if let Ok(pane) = id.parse::<u64>() {
                    self.toggle(pane, state == TriState::On);
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match (self.page, action_id) {
                (_, "cancel") => Step::Cancel,
                (Page::Orientation, "choose") => {
                    self.page = Page::Members;
                    Step::Stay
                }
                (Page::Orientation, "empty") => self.answer(Vec::new()),
                (Page::Members, "create") => self.answer(self.selected()),
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view of the current page.
    pub fn render(&self) -> UiTree {
        let (title, mut message, buttons, child) = match self.page {
            Page::Orientation => (
                "New split view",
                vec!["Choose how two or three panes are arranged:".to_owned()],
                vec![
                    button("choose", "Choose panes", ButtonRole::Primary, false),
                    button("empty", "Create empty", ButtonRole::Secondary, false),
                    button("cancel", "Cancel", ButtonRole::Secondary, true),
                ],
                node(
                    ORIENTATION_KEY,
                    Widget::Choice(ChoiceSpec {
                        label: "Orientation".to_owned(),
                        options: vec![
                            "Vertical \u{2014} side by side".to_owned(),
                            "Horizontal \u{2014} stacked".to_owned(),
                        ],
                        selected: u32::from(self.direction == SplitDirection::Horizontal),
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
            ),
            Page::Members => (
                "Add panes to split",
                vec![if self.choices.is_empty() {
                    "No eligible panes; all existing panes are already in split views.".to_owned()
                } else {
                    format!(
                        "Select up to four panes ({}/{MAXIMUM_PANES}). Selecting none creates an empty split.",
                        self.selected().len()
                    )
                }],
                vec![
                    button("create", "Create", ButtonRole::Primary, false),
                    button("cancel", "Cancel", ButtonRole::Secondary, true),
                ],
                node(
                    MEMBERS_KEY,
                    Widget::CheckList(CheckListSpec {
                        items: self
                            .choices
                            .iter()
                            .map(|choice| CheckItem {
                                id: choice.pane.to_string(),
                                label: vec![span(&choice.label)],
                                state: if choice.selected {
                                    TriState::On
                                } else {
                                    TriState::Off
                                },
                                disabled_reason: None,
                                children: Vec::new(),
                            })
                            .collect(),
                        all_toggle: false,
                        max_height: None,
                    }),
                ),
            ),
        };
        message.extend(self.notice.clone());
        let dialog = DialogSpec {
            title: vec![span(title)],
            kind: DialogKind::Plain,
            buttons,
            width: Constraint::Length(72),
            height: Constraint::Min(0),
            message: Some(message.iter().map(|line| vec![span(line)]).collect()),
        };
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                child,
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
