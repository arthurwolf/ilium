//! The "New group" dialog: a name and the group the new group is created under.
//!
//! Everything here is plain data and rules; `lib.rs` connects it to events, the tree and
//! intents. The kernel opens the dialog by running the `new-group` command with the group to
//! preselect (`{"parent": id}`); the plugin reads the tree itself. Which groups are offered, how
//! they are labelled and indented, the default name and the answer for the server side are
//! decided here.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Alignment, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint,
    ContainerKind, DialogButton, DialogKind, DialogSpec, DisabledMarker, NodeClass, NodeFlags,
    NodeView, ParagraphSpec, Span, Style, TextInputSpec, UiNode, UiTree, ViewTarget, Widget,
};
use serde_json::{json, Value};

/// Popup id of the dialog.
pub const POPUP: &str = "new-group";
/// Key of the name input.
pub const NAME_KEY: &str = "name";
/// Key of the destination list.
pub const DESTINATION_KEY: &str = "destination";
/// Channel of the answer, client side to server side. The payload is JSON:
/// `{"parent": id, "name": string}`.
pub const CHANNEL: &str = "create-group";
/// Name of a group created without a typed name (the placeholder the input shows).
pub const DEFAULT_NAME: &str = "group";
/// Label of the root of the tree, which is the top level.
pub const TOP_LEVEL_LABEL: &str = "Top level";
/// Longest name the input takes, in bytes.
pub const NAME_MAX_BYTES: u32 = 255;

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// One node of the tree, reduced to what the destination rules use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFact {
    pub id: u64,
    pub parent: Option<u64>,
    /// True for a node that can directly own ordinary entries: a project or a plain group (the
    /// root of the tree is one). A split view, a worktree, a folder and a pane are not.
    pub accepts_groups: bool,
    pub title: String,
}

impl GroupFact {
    /// The fact of one node as the tree import reports it.
    pub fn from_view(view: &NodeView) -> Self {
        let accepts_groups = matches!(
            (view.class, view.container),
            (
                NodeClass::Container,
                // A host that does not report the kind of a container is read as a group.
                Some(ContainerKind::Project | ContainerKind::Group) | None
            )
        );
        Self {
            id: view.id.0,
            parent: view.parent.map(|parent| parent.0),
            accepts_groups,
            title: view.title.clone(),
        }
    }
}

/// One group the new group can be created under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub id: u64,
    /// `0` for the top level, `1` for a top-level group, and so on.
    pub depth: usize,
    pub label: String,
}

/// The groups of the tree in drawn order, from the top level down. `facts` come in the order the
/// tree draws them (a parent before its children), the first fact with no parent being the
/// root. A node under one that cannot hold groups (a split view, a worktree) is not offered, and
/// neither is anything below it.
pub fn destinations(facts: &[GroupFact]) -> Vec<Destination> {
    let mut found: Vec<Destination> = Vec::new();
    for fact in facts {
        if !fact.accepts_groups {
            continue;
        }
        let depth = match fact.parent {
            None => 0,
            Some(parent) => match found.iter().find(|known| known.id == parent) {
                Some(known) => known.depth + 1,
                None => continue,
            },
        };
        if found.iter().any(|known| known.id == fact.id) {
            continue;
        }
        let label = if depth == 0 {
            TOP_LEVEL_LABEL.to_owned()
        } else {
            format!(
                "{}{}",
                "  ".repeat(depth.saturating_sub(1)),
                if fact.title.trim().is_empty() {
                    DEFAULT_NAME
                } else {
                    fact.title.as_str()
                }
            )
        };
        found.push(Destination {
            id: fact.id,
            depth,
            label,
        });
    }
    found
}

/// The group to preselect, from the `new-group` command arguments `{"parent": id}`.
pub fn parent_argument(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("parent")?.as_u64()
}

/// What the client side asks the server side to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub parent: u64,
    /// Never blank: a blank name is `group`.
    pub name: String,
}

impl Answer {
    pub fn to_payload(&self) -> Vec<u8> {
        json!({ "parent": self.parent, "name": self.name })
            .to_string()
            .into_bytes()
    }

    /// Reads a payload; anything that is not the object above is refused. A blank name becomes
    /// the default name.
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(payload).ok()?;
        let parent = value.get("parent")?.as_u64()?;
        let name = value.get("name")?.as_str()?;
        Some(Self {
            parent,
            name: name_or_default(name),
        })
    }
}

/// A typed name, or `group` when it is blank.
pub fn name_or_default(typed: &str) -> String {
    if typed.trim().is_empty() {
        DEFAULT_NAME.to_owned()
    } else {
        typed.to_owned()
    }
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Still open.
    Stay,
    /// Finished: create this group.
    Create(Answer),
    /// Finished without creating anything.
    Cancel,
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub destinations: Vec<Destination>,
    pub selected: usize,
    pub name: String,
}

impl Dialog {
    /// Opens the dialog from the tree's facts with `preselected` highlighted. A preselected id
    /// that is not a destination (a stale id) highlights the top level, which is always there.
    pub fn from_facts(preselected: u64, facts: &[GroupFact]) -> Self {
        let destinations = destinations(facts);
        let selected = destinations
            .iter()
            .position(|destination| destination.id == preselected)
            .unwrap_or(0);
        Self {
            destinations,
            selected,
            name: String::new(),
        }
    }

    /// The answer for the highlighted destination, `None` when the tree offered none.
    pub fn answer(&self) -> Option<Answer> {
        let destination = self.destinations.get(self.selected)?;
        Some(Answer {
            parent: destination.id,
            name: name_or_default(&self.name),
        })
    }

    fn create(&self) -> Step {
        self.answer().map_or(Step::Stay, Step::Create)
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
                self.create()
            }
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == DESTINATION_KEY =>
            {
                let last = self.destinations.len().saturating_sub(1);
                self.selected = usize::try_from(index).map_or(last, |index| index.min(last));
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "create" => self.create(),
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view: the name input, then one row per destination.
    pub fn render(&self) -> UiTree {
        let dialog = DialogSpec {
            title: vec![span("New group")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("create", "Create", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(54),
            height: Constraint::Min(0),
            message: Some(vec![
                vec![span(
                    "Name the group (left blank it is called \"group\"), then choose where it goes.",
                )],
                vec![span("Create under:")],
            ]),
        };
        let name = node(
            NAME_KEY,
            Widget::TextInput(TextInputSpec {
                label: "Name".to_owned(),
                initial: self.name.clone(),
                placeholder: Some(DEFAULT_NAME.to_owned()),
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
        let destination = node(
            DESTINATION_KEY,
            Widget::Choice(ChoiceSpec {
                label: "Create under".to_owned(),
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
                    children: vec![1, 2],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                name,
                destination,
            ],
            root: 0,
        }
    }
}

/// What a view of this plugin shows when no dialog is open: one empty paragraph.
pub fn blank() -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![node(
            "blank",
            Widget::Paragraph(ParagraphSpec {
                lines: Vec::new(),
                rich: None,
                wrap: None,
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: false,
            }),
        )],
        root: 0,
    }
}

pub(crate) fn span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

pub(crate) fn button(id: &str, label: &str, role: ButtonRole, is_cancel: bool) -> DialogButton {
    DialogButton {
        id: id.to_owned(),
        label: label.to_owned(),
        role,
        is_cancel,
    }
}

pub(crate) fn node(key: &str, widget: Widget) -> UiNode {
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

    fn fact(id: u64, parent: Option<u64>, accepts_groups: bool, title: &str) -> GroupFact {
        GroupFact {
            id,
            parent,
            accepts_groups,
            title: title.to_owned(),
        }
    }

    fn sample() -> Vec<GroupFact> {
        vec![
            fact(0, None, true, "root"),
            fact(1, Some(0), true, "ilium"),
            fact(10, Some(1), true, "work"),
            fact(11, Some(10), true, "deep"),
            fact(20, Some(10), false, "Vertical split"),
            fact(21, Some(20), true, "inside a split"),
            fact(30, Some(1), false, "feature"),
            fact(40, Some(1), false, "a pane"),
        ]
    }

    #[test]
    fn destinations_walk_the_tree_and_skip_nodes_that_hold_no_group() {
        let found = destinations(&sample());
        let ids: Vec<u64> = found.iter().map(|destination| destination.id).collect();
        assert_eq!(ids, [0, 1, 10, 11]);
        let depths: Vec<usize> = found.iter().map(|destination| destination.depth).collect();
        assert_eq!(depths, [0, 1, 2, 3]);
    }

    #[test]
    fn the_top_level_is_labelled_and_deeper_groups_are_indented() {
        let labels: Vec<String> = destinations(&sample())
            .into_iter()
            .map(|destination| destination.label)
            .collect();
        assert_eq!(labels, ["Top level", "ilium", "  work", "    deep"]);
    }

    #[test]
    fn a_group_without_a_title_is_labelled_group() {
        let found = destinations(&[fact(0, None, true, ""), fact(1, Some(0), true, "  ")]);
        assert_eq!(found[1].label, "group");
    }

    #[test]
    fn a_preselected_group_is_highlighted_and_a_stale_id_falls_back_to_the_top_level() {
        assert_eq!(Dialog::from_facts(10, &sample()).selected, 2);
        assert_eq!(Dialog::from_facts(999, &sample()).selected, 0);
        assert_eq!(Dialog::from_facts(21, &sample()).selected, 0);
    }

    #[test]
    fn an_empty_tree_offers_no_destination_and_cannot_create() {
        let mut dialog = Dialog::from_facts(0, &[]);
        assert!(dialog.destinations.is_empty());
        assert_eq!(dialog.answer(), None);
        assert_eq!(
            dialog.act("", "create", ActionKind::Activated, None),
            Step::Stay
        );
    }

    #[test]
    fn typing_choosing_and_pressing_create_answers_with_the_typed_name() {
        let mut dialog = Dialog::from_facts(0, &sample());
        assert_eq!(
            dialog.act(
                NAME_KEY,
                "",
                ActionKind::Changed,
                Some(ActionValue::Text("docs".to_owned()))
            ),
            Step::Stay
        );
        assert_eq!(
            dialog.act(
                DESTINATION_KEY,
                "",
                ActionKind::Changed,
                Some(ActionValue::Index(2))
            ),
            Step::Stay
        );
        assert_eq!(
            dialog.act("", "create", ActionKind::Activated, None),
            Step::Create(Answer {
                parent: 10,
                name: "docs".to_owned()
            })
        );
    }

    #[test]
    fn enter_in_the_name_field_creates_with_the_submitted_text() {
        let mut dialog = Dialog::from_facts(1, &sample());
        assert_eq!(
            dialog.act(
                NAME_KEY,
                "",
                ActionKind::Submitted,
                Some(ActionValue::Text("notes".to_owned()))
            ),
            Step::Create(Answer {
                parent: 1,
                name: "notes".to_owned()
            })
        );
    }

    #[test]
    fn a_blank_name_creates_a_group_called_group() {
        let mut dialog = Dialog::from_facts(0, &sample());
        dialog.name = "   ".to_owned();
        assert_eq!(
            dialog.answer(),
            Some(Answer {
                parent: 0,
                name: "group".to_owned()
            })
        );
    }

    #[test]
    fn an_index_past_the_list_selects_the_last_destination() {
        let mut dialog = Dialog::from_facts(0, &sample());
        dialog.act(
            DESTINATION_KEY,
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(99)),
        );
        assert_eq!(dialog.selected, 3);
    }

    #[test]
    fn cancel_and_the_kernel_closing_the_popup_end_the_dialog() {
        let mut dialog = Dialog::from_facts(0, &sample());
        assert_eq!(
            dialog.act("", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Cancel);
    }

    #[test]
    fn an_unknown_button_or_widget_changes_nothing() {
        let mut dialog = Dialog::from_facts(0, &sample());
        let before = dialog.clone();
        assert_eq!(
            dialog.act("", "other", ActionKind::Activated, None),
            Step::Stay
        );
        assert_eq!(
            dialog.act(
                "elsewhere",
                "",
                ActionKind::Changed,
                Some(ActionValue::Text("x".to_owned()))
            ),
            Step::Stay
        );
        assert_eq!(dialog, before);
    }

    #[test]
    fn the_answer_payload_round_trips_and_malformed_payloads_are_refused() {
        let answer = Answer {
            parent: 7,
            name: "docs".to_owned(),
        };
        assert_eq!(Answer::from_payload(&answer.to_payload()), Some(answer));
        assert_eq!(Answer::from_payload(b"not json"), None);
        assert_eq!(Answer::from_payload(br#"{"parent":"7","name":"x"}"#), None);
        assert_eq!(Answer::from_payload(br#"{"parent":7}"#), None);
        assert_eq!(
            Answer::from_payload(br#"{"parent":7,"name":" "}"#),
            Some(Answer {
                parent: 7,
                name: "group".to_owned()
            })
        );
    }

    #[test]
    fn the_command_argument_names_the_group_to_preselect() {
        assert_eq!(parent_argument(r#"{"parent": 12}"#), Some(12));
        assert_eq!(parent_argument("{}"), None);
        assert_eq!(parent_argument("nope"), None);
    }

    #[test]
    fn the_rendered_dialog_has_a_name_input_a_destination_list_and_two_buttons() {
        let tree = Dialog::from_facts(10, &sample()).render();
        assert_eq!(tree.nodes.len(), 3);
        let Widget::Dialog(dialog) = &tree.nodes[0].widget else {
            panic!("the root is a dialog");
        };
        let ids: Vec<&str> = dialog
            .buttons
            .iter()
            .map(|button| button.id.as_str())
            .collect();
        assert_eq!(ids, ["create", "cancel"]);
        assert!(dialog.buttons[1].is_cancel);
        let Widget::TextInput(input) = &tree.nodes[1].widget else {
            panic!("the first child is the name input");
        };
        assert!(input.submit_on_enter);
        assert_eq!(input.placeholder.as_deref(), Some("group"));
        let Widget::Choice(choice) = &tree.nodes[2].widget else {
            panic!("the second child is the destination list");
        };
        assert_eq!(choice.options.len(), 4);
        assert_eq!(choice.selected, 2);
    }
}
