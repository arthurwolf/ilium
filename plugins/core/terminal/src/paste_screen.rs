//! "Paste screen into another pane": the right-click entry of a terminal pane that hands the
//! visible screen of this pane to a neighbour in the same split view.
//!
//! Everything here is plain data and rules; `lib.rs` connects it to events, the tree and
//! intents. The kernel runs the `paste-screen` command with the clicked pane as `node`. The
//! client side lists the other terminal panes of the pane's split view: with one it sends the
//! transfer at once, with several it asks which one in a popup, and with none it says so. The
//! answer goes to the server side as a `paste-screen` message, which calls the same
//! `send-screen` path the `ilium.core.terminal/transfer` service uses.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogButton,
    DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span, Style, UiNode, UiTree, ViewTarget,
    Widget,
};
use serde_json::Value;

/// Name of the client command, of the server message channel and of the popup.
pub const NAME: &str = "paste-screen";
/// Key of the destination choice widget.
pub const DESTINATIONS_KEY: &str = "destinations";
/// Shown when the pane has no terminal neighbour.
pub const NO_DESTINATION: &str = "No other terminal in this split view";
/// Most destinations a split view can offer (a split view holds at most four panes).
pub const MAXIMUM_DESTINATIONS: usize = 3;

/// The popup view of the chooser.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(NAME.to_owned())
}

/// One sibling of the clicked pane, reduced to what the rules use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sibling {
    pub id: u64,
    /// The content kind as the tree reports it, `ilium.terminal` for a terminal.
    pub content_kind: Option<String>,
    pub title: String,
    /// Whether an agent runs in the pane.
    pub agent: bool,
}

/// A pane the screen can go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub pane: u64,
    pub label: String,
}

/// The terminal panes among `siblings`, except `source`, in tree order. Editors and boards are
/// never offered.
pub fn destinations(source: u64, siblings: &[Sibling]) -> Vec<Destination> {
    siblings
        .iter()
        .filter(|sibling| sibling.id != source && is_terminal(sibling.content_kind.as_deref()))
        .take(MAXIMUM_DESTINATIONS)
        .map(|sibling| Destination {
            pane: sibling.id,
            label: format!(
                "[{}] {}",
                if sibling.agent { "agent" } else { "terminal" },
                sibling.title
            ),
        })
        .collect()
}

fn is_terminal(content_kind: Option<&str>) -> bool {
    content_kind.is_some_and(|kind| kind.strip_prefix("ilium.").unwrap_or(kind) == "terminal")
}

/// The pane the command was run on, from the arguments `{"node": id}`.
pub fn node_argument(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("node")?.as_u64()
}

/// What the client asks the server side to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub source: u64,
    pub target: u64,
}

impl Answer {
    pub fn to_payload(self) -> Vec<u8> {
        serde_json::json!({ "source": self.source, "target": self.target })
            .to_string()
            .into_bytes()
    }

    /// Reads an answer; `None` for anything malformed or from a pane to itself.
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        let source = value.get("source")?.as_u64()?;
        let target = value.get("target")?.as_u64()?;
        (source != target).then_some(Self { source, target })
    }
}

/// What an action did to the chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Still open.
    Stay,
    /// Finished: send this transfer.
    Paste(Answer),
    /// Finished without sending anything.
    Cancel,
}

/// The open chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub source: u64,
    pub destinations: Vec<Destination>,
    pub selected: usize,
}

impl Dialog {
    pub fn new(source: u64, destinations: Vec<Destination>) -> Self {
        Self {
            source,
            destinations,
            selected: 0,
        }
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
                if key == DESTINATIONS_KEY =>
            {
                if let Some(position) = usize::try_from(index)
                    .ok()
                    .filter(|position| *position < self.destinations.len())
                {
                    self.selected = position;
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "cancel" => Step::Cancel,
                "paste" => match self.destinations.get(self.selected) {
                    Some(destination) => Step::Paste(Answer {
                        source: self.source,
                        target: destination.pane,
                    }),
                    None => Step::Cancel,
                },
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view of the chooser.
    pub fn render(&self) -> UiTree {
        let dialog = DialogSpec {
            title: vec![span("Paste screen into another pane")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("paste", "Paste", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(60),
            height: Constraint::Min(0),
            message: Some(vec![vec![span(
                "The visible screen is pasted without pressing Enter:",
            )]]),
        };
        let choice = node(
            DESTINATIONS_KEY,
            Widget::Choice(ChoiceSpec {
                label: "Destination".to_owned(),
                options: self
                    .destinations
                    .iter()
                    .map(|destination| destination.label.clone())
                    .collect(),
                selected: u32::try_from(self.selected).unwrap_or(0),
                style: ChoiceStyle::List,
                items: Vec::new(),
                source: None,
                selected_id: None,
                filter: false,
                view_modes: Vec::new(),
                disabled_marker: DisabledMarker::Reason,
                allow_custom: false,
            }),
        );
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                choice,
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

    fn sibling(id: u64, kind: &str, agent: bool) -> Sibling {
        Sibling {
            id,
            content_kind: Some(kind.to_owned()),
            title: format!("pane {id}"),
            agent,
        }
    }

    /// REQ: COR-004
    #[test]
    fn only_other_terminal_panes_are_offered_never_editors_or_boards() {
        let siblings = [
            sibling(1, "ilium.terminal", false),
            sibling(2, "ilium.editor", false),
            sibling(3, "ilium.terminal", true),
            sibling(4, "ilium.board", false),
        ];
        let offered = destinations(1, &siblings);
        assert_eq!(
            offered,
            vec![Destination {
                pane: 3,
                label: "[agent] pane 3".to_owned()
            }]
        );
        assert!(destinations(2, &[sibling(2, "ilium.editor", false)]).is_empty());
    }

    #[test]
    fn a_pane_never_offers_itself_and_a_missing_kind_is_not_a_terminal() {
        let mut lone = sibling(5, "ilium.terminal", false);
        assert!(destinations(5, std::slice::from_ref(&lone)).is_empty());
        lone.id = 6;
        lone.content_kind = None;
        assert!(destinations(5, &[lone]).is_empty());
    }

    #[test]
    fn the_command_arguments_name_the_pane() {
        assert_eq!(node_argument("{\"node\":12}"), Some(12));
        assert_eq!(node_argument("{}"), None);
        assert_eq!(node_argument("nonsense"), None);
    }

    #[test]
    fn an_answer_round_trips_and_refuses_a_pane_to_itself() {
        let answer = Answer {
            source: 4,
            target: 9,
        };
        assert_eq!(Answer::from_payload(&answer.to_payload()), Some(answer));
        assert_eq!(Answer::from_payload(b"{\"source\":4,\"target\":4}"), None);
        assert_eq!(Answer::from_payload(b"{\"source\":4}"), None);
        assert_eq!(Answer::from_payload(b"\xff"), None);
    }

    fn dialog() -> Dialog {
        Dialog::new(
            1,
            vec![
                Destination {
                    pane: 2,
                    label: "a".to_owned(),
                },
                Destination {
                    pane: 3,
                    label: "b".to_owned(),
                },
            ],
        )
    }

    #[test]
    fn choosing_then_pasting_sends_the_chosen_destination() {
        let mut dialog = dialog();
        let step = dialog.act(
            DESTINATIONS_KEY,
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(1)),
        );
        assert_eq!(step, Step::Stay);
        assert_eq!(
            dialog.act("", "paste", ActionKind::Activated, None),
            Step::Paste(Answer {
                source: 1,
                target: 3
            })
        );
    }

    #[test]
    fn an_index_past_the_list_is_ignored_and_cancel_and_close_end_the_dialog() {
        let mut dialog = dialog();
        dialog.act(
            DESTINATIONS_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(7)),
        );
        assert_eq!(dialog.selected, 0);
        assert_eq!(
            dialog.act("", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Cancel);
    }

    #[test]
    fn the_chooser_lists_every_destination() {
        let tree = dialog().render();
        let Widget::Choice(choice) = &tree.nodes[1].widget else {
            panic!("the second node is the destination choice");
        };
        assert_eq!(choice.options, vec!["a".to_owned(), "b".to_owned()]);
    }
}
