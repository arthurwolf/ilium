//! The "Rename" dialog: one name input for the node the user asked to rename.
//!
//! Everything here is plain data and rules; `lib.rs` connects it to events, the tree and
//! intents. The kernel runs the `rename` command with the selected node (`{"node": id}`) from
//! the `,` key, the double-click on a tree row, the row action and the context menu. The plugin
//! reads the node, decides whether it can be renamed, shows the popup with the current name and
//! sends the typed name to its server side, which submits the `rename-node` intent.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, Constraint, ContainerKind, DialogKind, DialogSpec,
    NodeView, TextInputSpec, ToastSpec, Tone, UiNode, UiTree, ViewTarget, Widget,
};
use serde_json::{json, Value};

use crate::new_group::{blank, button, node, span};

/// Popup id of the dialog.
pub const POPUP: &str = "rename";
/// Key of the name input.
pub const NAME_KEY: &str = "name";
/// Channel of the answer, client side to server side. The payload is JSON:
/// `{"node": id, "title": string}`.
pub const CHANNEL: &str = "rename-node";
/// Longest name the input takes, in bytes.
pub const NAME_MAX_BYTES: u32 = 255;
/// What the user is told when the selected row is a worktree row.
pub const WORKTREE_REFUSAL: &str = "The worktree row uses its branch name";
/// Id of the toast that carries the refusal, so a repeated refusal replaces the toast.
pub const REFUSAL_TOAST_ID: &str = "ilium.core.tree/rename-refused";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// The node to rename, from the `rename` command arguments `{"node": id}`.
pub fn node_argument(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("node")?.as_u64()
}

/// One node of the tree, reduced to what the rename rules use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFact {
    pub id: u64,
    pub title: String,
    /// A worktree row shows the branch name and is renamed through the branch.
    pub is_worktree: bool,
}

impl NodeFact {
    /// The fact of one node as the tree import reports it.
    pub fn from_view(view: &NodeView) -> Self {
        Self {
            id: view.id.0,
            title: view.title.clone(),
            is_worktree: view.container == Some(ContainerKind::Worktree),
        }
    }
}

/// Why a node cannot be renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The row is not a node of the tree (a resource row, a foreign invocation, a stale id): the
    /// command does nothing and says nothing.
    NotANode,
    /// The row is a worktree row.
    Worktree,
}

impl Refusal {
    /// What the user is told, or `None` when the refusal is silent.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::NotANode => None,
            Self::Worktree => Some(WORKTREE_REFUSAL),
        }
    }

    /// The toast that carries the message, `None` for a silent refusal.
    pub fn toast(self) -> Option<ToastSpec> {
        let title = self.message()?;
        Some(ToastSpec {
            id: Some(REFUSAL_TOAST_ID.to_owned()),
            tone: Tone::Info,
            title: title.to_owned(),
            body: None,
            actions: Vec::new(),
            duration_ms: None,
        })
    }
}

/// What the client side asks the server side to rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub node: u64,
    /// The typed name as it is: the server decides what an empty or unchanged name means.
    pub title: String,
}

impl Answer {
    pub fn to_payload(&self) -> Vec<u8> {
        json!({ "node": self.node, "title": self.title })
            .to_string()
            .into_bytes()
    }

    /// Reads a payload; anything that is not the object above is refused.
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        Some(Self {
            node: value.get("node")?.as_u64()?,
            title: value.get("title")?.as_str()?.to_owned(),
        })
    }
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Still open.
    Stay,
    /// Finished: rename the node.
    Rename(Answer),
    /// Finished without renaming anything.
    Cancel,
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub node: u64,
    pub name: String,
}

impl Dialog {
    /// Opens the dialog for a node, or says why it cannot be renamed. A missing node (`None`) is
    /// a row that is not part of the tree.
    pub fn open(fact: Option<&NodeFact>) -> Result<Self, Refusal> {
        let fact = fact.ok_or(Refusal::NotANode)?;
        if fact.is_worktree {
            return Err(Refusal::Worktree);
        }
        Ok(Self {
            node: fact.id,
            name: fact.title.clone(),
        })
    }

    fn rename(&self) -> Step {
        Step::Rename(Answer {
            node: self.node,
            title: self.name.clone(),
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
            (ActionKind::Changed, Some(ActionValue::Text(text))) if key == NAME_KEY => {
                self.name = text;
                Step::Stay
            }
            (ActionKind::Submitted, Some(ActionValue::Text(text))) if key == NAME_KEY => {
                self.name = text;
                self.rename()
            }
            (ActionKind::Activated, _) => match action_id {
                "rename" => self.rename(),
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view: the name input with the current name.
    pub fn render(&self) -> UiTree {
        let dialog = DialogSpec {
            title: vec![span("Rename")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("rename", "Rename", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(54),
            height: Constraint::Min(0),
            message: None,
        };
        let name = node(
            NAME_KEY,
            Widget::TextInput(TextInputSpec {
                label: "Name".to_owned(),
                initial: self.name.clone(),
                placeholder: None,
                secret: false,
                reveal_last: 0,
                reveal: false,
                max_length: NAME_MAX_BYTES,
                submit_on_enter: true,
                debounce_ms: 0,
                validation: None,
                suggestions: None,
            }),
        );
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                name,
            ],
            root: 0,
        }
    }
}

/// What a view of this plugin shows when no rename dialog is open.
pub fn closed() -> UiTree {
    blank()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::types::{NodeClass, NodeId};

    fn fact(id: u64, title: &str, is_worktree: bool) -> NodeFact {
        NodeFact {
            id,
            title: title.to_owned(),
            is_worktree,
        }
    }

    fn view_of(container: Option<ContainerKind>) -> NodeView {
        NodeView {
            id: NodeId(7),
            parent: Some(NodeId(1)),
            class: NodeClass::Container,
            content_kind: None,
            title: "feature".to_owned(),
            cwd: None,
            agent: None,
            locked: false,
            revision: 0,
            container,
        }
    }

    #[test]
    fn the_node_comes_from_the_command_arguments() {
        assert_eq!(node_argument(r#"{"node": 12}"#), Some(12));
        assert_eq!(node_argument(r#"{"node": "12"}"#), None);
        assert_eq!(node_argument("{}"), None);
        assert_eq!(node_argument("not json"), None);
        assert_eq!(node_argument(""), None);
    }

    #[test]
    fn a_worktree_container_is_read_as_a_worktree_row() {
        assert!(NodeFact::from_view(&view_of(Some(ContainerKind::Worktree))).is_worktree);
        assert!(!NodeFact::from_view(&view_of(Some(ContainerKind::Group))).is_worktree);
        assert!(!NodeFact::from_view(&view_of(None)).is_worktree);
    }

    #[test]
    fn the_dialog_opens_with_the_current_name() {
        let dialog = Dialog::open(Some(&fact(3, "build", false))).expect("a plain node opens");
        assert_eq!(dialog.node, 3);
        assert_eq!(dialog.name, "build");
    }

    #[test]
    fn a_worktree_row_is_refused_with_its_message() {
        let refusal = Dialog::open(Some(&fact(3, "feature", true))).expect_err("refused");
        assert_eq!(refusal, Refusal::Worktree);
        assert_eq!(
            refusal.message(),
            Some("The worktree row uses its branch name")
        );
        let toast = refusal.toast().expect("a worktree refusal is shown");
        assert_eq!(toast.title, WORKTREE_REFUSAL);
        assert!(toast.title.chars().count() <= 60);
    }

    #[test]
    fn a_row_that_is_not_a_node_is_refused_silently() {
        let refusal = Dialog::open(None).expect_err("refused");
        assert_eq!(refusal, Refusal::NotANode);
        assert_eq!(refusal.message(), None);
        assert!(refusal.toast().is_none());
    }

    #[test]
    fn enter_in_the_name_field_renames_with_the_typed_name() {
        let mut dialog = Dialog::open(Some(&fact(3, "build", false))).expect("opens");
        let step = dialog.act(
            NAME_KEY,
            "",
            ActionKind::Submitted,
            Some(ActionValue::Text("release".to_owned())),
        );
        assert_eq!(
            step,
            Step::Rename(Answer {
                node: 3,
                title: "release".to_owned()
            })
        );
    }

    #[test]
    fn typing_only_updates_the_name_and_the_button_sends_it() {
        let mut dialog = Dialog::open(Some(&fact(3, "build", false))).expect("opens");
        let typed = dialog.act(
            NAME_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Text("bu".to_owned())),
        );
        assert_eq!(typed, Step::Stay);
        let step = dialog.act("", "rename", ActionKind::Activated, None);
        assert_eq!(
            step,
            Step::Rename(Answer {
                node: 3,
                title: "bu".to_owned()
            })
        );
    }

    #[test]
    fn a_blank_name_is_sent_as_typed() {
        let mut dialog = Dialog::open(Some(&fact(3, "build", false))).expect("opens");
        dialog.act(
            NAME_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Text(String::new())),
        );
        let step = dialog.act("", "rename", ActionKind::Activated, None);
        assert_eq!(
            step,
            Step::Rename(Answer {
                node: 3,
                title: String::new()
            })
        );
    }

    #[test]
    fn cancel_closing_and_unknown_actions() {
        let mut dialog = Dialog::open(Some(&fact(3, "build", false))).expect("opens");
        assert_eq!(
            dialog.act("", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Cancel);
        assert_eq!(
            dialog.act("", "other", ActionKind::Activated, None),
            Step::Stay
        );
        assert_eq!(
            dialog.act(
                "other",
                "",
                ActionKind::Changed,
                Some(ActionValue::Text("x".to_owned()))
            ),
            Step::Stay
        );
        assert_eq!(dialog.name, "build");
    }

    #[test]
    fn the_answer_round_trips_and_malformed_payloads_are_refused() {
        let answer = Answer {
            node: 9,
            title: "café 世界".to_owned(),
        };
        assert_eq!(Answer::from_payload(&answer.to_payload()), Some(answer));
        assert_eq!(Answer::from_payload(b""), None);
        assert_eq!(Answer::from_payload(b"[]"), None);
        assert_eq!(Answer::from_payload(br#"{"node": 1}"#), None);
        assert_eq!(
            Answer::from_payload(br#"{"node": "1", "title": "x"}"#),
            None
        );
        assert_eq!(Answer::from_payload(br#"{"node": 1, "title": 5}"#), None);
    }

    #[test]
    fn the_view_is_a_dialog_with_one_prefilled_name_input() {
        let dialog = Dialog::open(Some(&fact(3, "build", false))).expect("opens");
        let tree = dialog.render();
        assert_eq!(tree.root, 0);
        assert_eq!(tree.nodes[0].children, vec![1]);
        match &tree.nodes[1].widget {
            Widget::TextInput(input) => {
                assert_eq!(input.initial, "build");
                assert!(input.submit_on_enter);
                assert_eq!(input.max_length, NAME_MAX_BYTES);
            }
            other => panic!("expected a text input, got {other:?}"),
        }
        assert_eq!(tree.nodes[1].key.as_deref(), Some(NAME_KEY));
    }
}
