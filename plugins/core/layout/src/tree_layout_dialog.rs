//! The tree row layout editor: which of the identity, objective, activity and title slots a tree
//! row shows, and in which order, with a specimen of the result.
//!
//! The Icons settings page asks for this dialog by running the `tree-layout` command with
//! `{"layout": {"order": [...], "hidden": [...]}, "glyphs": {...}}` (the layout in use and the
//! icon glyphs of the specimen rows, each already padded to the width of its slot in the live
//! tree). This module holds the rules and the popup; `lib.rs` connects it to events and
//! intents. Every change is sent to the kernel at once as `kernel.set-tree-row-layout`, which
//! saves it and redraws the tree behind the dialog. Cancel puts the layout back that was in use
//! when the dialog opened; Done and Escape keep what is shown.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogButton,
    DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span, Style, UiNode, UiTree, ViewTarget,
    Widget,
};
use serde_json::Value;

use crate::tree_layout_rows::{EditorKey, EditorState, TreeRowElement, TreeRowLayout};

/// Local id of the command that opens the dialog.
pub const COMMAND: &str = "tree-layout";
/// Popup id of the dialog.
pub const POPUP: &str = "tree-layout";
/// Key of the slot choice.
pub const SLOT_KEY: &str = "slot";
/// The kernel command that sets the tree row layout.
pub const SET_LAYOUT_COMMAND: &str = "kernel.set-tree-row-layout";
/// Title of the dialog.
pub const TITLE: &str = "Tree layout";

/// Cells of the identity, objective and activity slots in the live tree row.
const IDENTITY_WIDTH: usize = 3;
const OBJECTIVE_WIDTH: usize = 3;
const ACTIVITY_WIDTH: usize = 2;
/// Cells of one specimen row.
const SPECIMEN_WIDTH: usize = 48;

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// The icon glyphs of the specimen rows. Each is padded by the kernel to the cells of its slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Glyphs {
    pub group: String,
    pub split: String,
    pub claude: String,
    pub codex: String,
    pub other_agent: String,
    pub goal_active: String,
    pub goal_blocked: String,
    pub task_done: String,
    pub working: String,
    pub waiting_approval: String,
    pub done: String,
}

impl Glyphs {
    fn from_value(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key)?.as_str().map(str::to_owned);
        Some(Self {
            group: text("group")?,
            split: text("split")?,
            claude: text("claude")?,
            codex: text("codex")?,
            other_agent: text("other_agent")?,
            goal_active: text("goal_active")?,
            goal_blocked: text("goal_blocked")?,
            task_done: text("task_done")?,
            working: text("working")?,
            waiting_approval: text("waiting_approval")?,
            done: text("done")?,
        })
    }
}

/// What the command asks for: the layout in use and the specimen glyphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub layout: TreeRowLayout,
    pub glyphs: Glyphs,
}

impl Request {
    /// Reads the command arguments; `None` for anything malformed, so nothing opens.
    pub fn from_arguments(arguments_json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(arguments_json).ok()?;
        let layout: TreeRowLayout = serde_json::from_value(value.get("layout")?.clone()).ok()?;
        Some(Self {
            layout: layout.normalized(),
            glyphs: Glyphs::from_value(value.get("glyphs")?)?,
        })
    }
}

/// The arguments of `kernel.set-tree-row-layout` for one layout.
pub fn set_layout_arguments(layout: &TreeRowLayout) -> String {
    serde_json::to_string(layout).unwrap_or_else(|_| "{}".to_owned())
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing for the kernel; the dialog stays open.
    Stay,
    /// The layout changed: send it to the kernel. The dialog stays open.
    Set(TreeRowLayout),
    /// Finished; what is applied stays.
    Done,
    /// Cancelled: send the layout back that was in use when the dialog opened (`None` when
    /// nothing changed since) and close.
    Cancel(Option<TreeRowLayout>),
}

/// The open dialog.
#[derive(Debug, Clone)]
pub struct Dialog {
    original: TreeRowLayout,
    layout: TreeRowLayout,
    editor: EditorState,
    glyphs: Glyphs,
}

impl Dialog {
    /// Opens the dialog on the layout in use, with the first slot selected.
    pub fn new(request: Request) -> Self {
        Self {
            original: request.layout.clone(),
            layout: request.layout,
            editor: EditorState::default(),
            glyphs: request.glyphs,
        }
    }

    /// The layout the dialog shows now.
    pub fn layout(&self) -> &TreeRowLayout {
        &self.layout
    }

    /// The index of the selected slot.
    pub fn selected(&self) -> usize {
        self.editor.selected
    }

    /// Applies one editor key and reports the new layout when it changed.
    fn edit(&mut self, key: EditorKey) -> Step {
        if self.editor.apply(&mut self.layout, key) {
            Step::Set(self.layout.clone())
        } else {
            Step::Stay
        }
    }

    /// Applies one action the kernel reported for the popup. An out-of-range choice index is
    /// ignored; the kernel closes the popup itself on Escape (a `Closed` action), which keeps
    /// the layout.
    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Done,
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == SLOT_KEY =>
            {
                if let Some(index) = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < TreeRowElement::ALL.len())
                {
                    self.editor.selected = index;
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "done" => Step::Done,
                "cancel" => {
                    Step::Cancel((self.layout != self.original).then(|| self.original.clone()))
                }
                "up" => self.edit(EditorKey::Up { shift: true }),
                "down" => self.edit(EditorKey::Down { shift: true }),
                "toggle" => self.edit(EditorKey::Space),
                "default" => self.edit(EditorKey::Preset(1)),
                "all-right" => self.edit(EditorKey::Preset(2)),
                "mixed" => self.edit(EditorKey::Preset(3)),
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The label of slot `index`: its name, the side of the title it sits on and whether it
    /// shows.
    fn slot_label(&self, index: usize) -> String {
        let order = self.layout.order();
        let Some(&element) = order.get(index) else {
            return String::new();
        };
        let after_title = order
            .iter()
            .position(|item| *item == TreeRowElement::Title)
            .is_some_and(|title| index > title);
        let (name, side, shown) = if element == TreeRowElement::Title {
            ("Title", "divider", "required")
        } else {
            (
                match element {
                    TreeRowElement::Identity => "Identity",
                    TreeRowElement::Objective => "Objective",
                    _ => "Activity",
                },
                if after_title { "right" } else { "left" },
                if self.layout.is_visible(element) {
                    "shown"
                } else {
                    "hidden"
                },
            )
        };
        format!("{name} · {side} · {shown}")
    }

    /// One specimen row: the trunk, then the slots in the layout's order within the width.
    fn specimen_row(
        &self,
        trunk: &str,
        identity: &str,
        objective: Option<&str>,
        activity: Option<&str>,
        title: &str,
    ) -> String {
        let blank_fill = self.layout.is_default();
        let objective = objective.map_or_else(
            || {
                if blank_fill {
                    " ".repeat(OBJECTIVE_WIDTH)
                } else {
                    String::new()
                }
            },
            str::to_owned,
        );
        let activity = activity.map_or_else(
            || {
                if blank_fill {
                    " ".repeat(ACTIVITY_WIDTH)
                } else {
                    String::new()
                }
            },
            str::to_owned,
        );
        let width = SPECIMEN_WIDTH.saturating_sub(trunk.chars().count());
        // The slot widths are the cells of the live tree row; an absent icon of a non-default
        // layout takes no cells at all.
        let placements = self.layout.placements(
            IDENTITY_WIDTH,
            if objective.is_empty() {
                0
            } else {
                OBJECTIVE_WIDTH
            },
            if activity.is_empty() {
                0
            } else {
                ACTIVITY_WIDTH
            },
            width,
        );
        let mut line = trunk.to_owned();
        for (element, _, size) in placements {
            match element {
                TreeRowElement::Title => {
                    let mut cut: String = title.chars().take(size).collect();
                    let missing = size.saturating_sub(cut.chars().count());
                    cut.extend(std::iter::repeat_n(' ', missing));
                    line.push_str(&cut);
                }
                TreeRowElement::Identity if size > 0 => line.push_str(identity),
                TreeRowElement::Objective if size > 0 => line.push_str(&objective),
                TreeRowElement::Activity if size > 0 => line.push_str(&activity),
                _ => {}
            }
        }
        line
    }

    /// The specimen: a small dummy tree drawn with the layout being edited.
    fn specimen(&self) -> Vec<String> {
        let glyphs = &self.glyphs;
        vec![
            self.specimen_row("", &glyphs.group, None, None, "Workspace"),
            self.specimen_row("└─ ", &glyphs.split, None, None, "Build"),
            self.specimen_row(
                "   ├─ ",
                &glyphs.claude,
                Some(&glyphs.goal_active),
                Some(&glyphs.working),
                "Working on settings",
            ),
            self.specimen_row(
                "   ├─ ",
                &glyphs.codex,
                Some(&glyphs.goal_blocked),
                Some(&glyphs.waiting_approval),
                "Waiting for approval",
            ),
            self.specimen_row(
                "   └─ ",
                &glyphs.other_agent,
                Some(&glyphs.task_done),
                Some(&glyphs.done),
                "Finished task",
            ),
        ]
    }

    /// The popup view.
    pub fn render(&self) -> UiTree {
        let mut message = vec![
            vec![span(
                "Slots above Title sit on the left, slots below it on the right.",
            )],
            vec![span("Badges follow Title; cost and actions stay fixed.")],
            vec![span("")],
            vec![span("Preview · dummy tree")],
        ];
        message.extend(self.specimen().iter().map(|row| vec![span(row)]));
        let dialog = DialogSpec {
            title: vec![span(TITLE)],
            kind: DialogKind::Plain,
            buttons: vec![
                button("done", "Done", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
                button("up", "Up", ButtonRole::Secondary, false),
                button("down", "Down", ButtonRole::Secondary, false),
                button("toggle", "Show/hide", ButtonRole::Secondary, false),
                button("default", "Default", ButtonRole::Secondary, false),
                button("all-right", "All right", ButtonRole::Secondary, false),
                button("mixed", "Mixed", ButtonRole::Secondary, false),
            ],
            width: Constraint::Length(90),
            height: Constraint::Min(0),
            message: Some(message),
        };
        let choice = ChoiceSpec {
            label: "Row slots (Up and Down move the selected slot)".to_owned(),
            options: (0..TreeRowElement::ALL.len())
                .map(|index| self.slot_label(index))
                .collect(),
            selected: u32::try_from(self.editor.selected).unwrap_or(0),
            style: ChoiceStyle::List,
            items: Vec::new(),
            source: None,
            selected_id: None,
            filter: false,
            view_modes: Vec::new(),
            disabled_marker: DisabledMarker::Reason,
            allow_custom: false,
        };
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                node(SLOT_KEY, Widget::Choice(choice)),
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

    fn glyphs() -> Glyphs {
        Glyphs {
            group: "G  ".to_owned(),
            split: "S  ".to_owned(),
            claude: "C  ".to_owned(),
            codex: "X  ".to_owned(),
            other_agent: "O  ".to_owned(),
            goal_active: "a  ".to_owned(),
            goal_blocked: "b  ".to_owned(),
            task_done: "t  ".to_owned(),
            working: "w ".to_owned(),
            waiting_approval: "p ".to_owned(),
            done: "d ".to_owned(),
        }
    }

    fn dialog() -> Dialog {
        Dialog::new(Request {
            layout: TreeRowLayout::default(),
            glyphs: glyphs(),
        })
    }

    fn press(dialog: &mut Dialog, id: &str) -> Step {
        dialog.act("dialog", id, ActionKind::Activated, None)
    }

    fn choose(dialog: &mut Dialog, index: u32) -> Step {
        dialog.act(
            SLOT_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(index)),
        )
    }

    fn arguments() -> String {
        serde_json::json!({
            "layout": {
                "order": ["title", "identity", "objective", "activity"],
                "hidden": ["activity"],
            },
            "glyphs": {
                "group": "G  ", "split": "S  ", "claude": "C  ", "codex": "X  ",
                "other_agent": "O  ", "goal_active": "a  ", "goal_blocked": "b  ",
                "task_done": "t  ", "working": "w ", "waiting_approval": "p ", "done": "d ",
            },
        })
        .to_string()
    }

    #[test]
    fn arguments_carry_the_layout_and_the_glyphs() {
        let request = Request::from_arguments(&arguments()).expect("valid arguments");
        assert_eq!(request.layout.order()[0], TreeRowElement::Title);
        assert!(!request.layout.is_visible(TreeRowElement::Activity));
        assert_eq!(request.glyphs.working, "w ");
        for bad in ["", "{}", "[]", r#"{"layout": {}}"#, r#"{"glyphs": {}}"#] {
            assert_eq!(Request::from_arguments(bad), None, "{bad}");
        }
    }

    #[test]
    fn set_layout_arguments_name_the_slots_in_snake_case() {
        let layout = TreeRowLayout::all_right();
        let value: Value = serde_json::from_str(&set_layout_arguments(&layout)).unwrap();
        assert_eq!(
            value["order"],
            serde_json::json!(["title", "identity", "objective", "activity"])
        );
        assert_eq!(value["hidden"], serde_json::json!([]));
    }

    #[test]
    fn choosing_a_slot_selects_it_and_changes_nothing() {
        let mut dialog = dialog();
        assert_eq!(choose(&mut dialog, 2), Step::Stay);
        assert_eq!(dialog.selected(), 2);
        assert_eq!(choose(&mut dialog, 9), Step::Stay);
        assert_eq!(dialog.selected(), 2);
        assert!(dialog.layout().is_default());
    }

    #[test]
    fn move_down_swaps_the_selected_slot_and_follows_it() {
        let mut dialog = dialog();
        let Step::Set(layout) = press(&mut dialog, "down") else {
            panic!("moving the first slot down changes the layout");
        };
        assert_eq!(layout.order()[0], TreeRowElement::Objective);
        assert_eq!(layout.order()[1], TreeRowElement::Identity);
        assert_eq!(dialog.selected(), 1);
        // At the top, moving up changes nothing.
        let mut top = self::dialog();
        assert_eq!(press(&mut top, "up"), Step::Stay);
    }

    #[test]
    fn title_moves_across_icons_and_cannot_be_hidden() {
        let mut dialog = dialog();
        choose(&mut dialog, 3);
        let Step::Set(layout) = press(&mut dialog, "up") else {
            panic!("moving the title up changes the layout");
        };
        assert_eq!(layout.order()[2], TreeRowElement::Title);
        assert_eq!(press(&mut dialog, "toggle"), Step::Stay);
        assert!(dialog.layout().is_visible(TreeRowElement::Title));
    }

    #[test]
    fn toggle_hides_and_shows_the_selected_icon() {
        let mut dialog = dialog();
        let Step::Set(hidden) = press(&mut dialog, "toggle") else {
            panic!("hiding the identity icon changes the layout");
        };
        assert!(!hidden.is_visible(TreeRowElement::Identity));
        let Step::Set(shown) = press(&mut dialog, "toggle") else {
            panic!("showing it again changes the layout");
        };
        assert!(shown.is_visible(TreeRowElement::Identity));
    }

    #[test]
    fn presets_replace_the_layout() {
        let mut dialog = dialog();
        assert_eq!(
            press(&mut dialog, "all-right"),
            Step::Set(TreeRowLayout::all_right())
        );
        assert_eq!(
            press(&mut dialog, "mixed"),
            Step::Set(TreeRowLayout::mixed())
        );
        assert_eq!(
            press(&mut dialog, "default"),
            Step::Set(TreeRowLayout::default())
        );
        // The default again changes nothing.
        assert_eq!(press(&mut dialog, "default"), Step::Stay);
    }

    #[test]
    fn done_and_escape_keep_the_layout_and_cancel_restores_it() {
        let mut dialog = dialog();
        press(&mut dialog, "all-right");
        assert_eq!(press(&mut dialog, "done"), Step::Done);
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Done);
        assert_eq!(
            press(&mut dialog, "cancel"),
            Step::Cancel(Some(TreeRowLayout::default()))
        );
        let mut untouched = self::dialog();
        assert_eq!(press(&mut untouched, "cancel"), Step::Cancel(None));
    }

    #[test]
    fn slot_labels_name_the_side_and_the_visibility() {
        let mut dialog = Dialog::new(Request {
            layout: TreeRowLayout {
                hidden: vec![TreeRowElement::Activity],
                ..TreeRowLayout::mixed()
            },
            glyphs: glyphs(),
        });
        let labels: Vec<_> = (0..4).map(|index| dialog.slot_label(index)).collect();
        assert_eq!(
            labels,
            vec![
                "Identity · left · shown",
                "Title · divider · required",
                "Objective · right · shown",
                "Activity · right · hidden",
            ]
        );
        press(&mut dialog, "default");
        assert_eq!(dialog.slot_label(3), "Title · divider · required");
    }

    #[test]
    fn the_specimen_follows_the_layout() {
        let mut dialog = dialog();
        let rows = dialog.specimen();
        assert_eq!(rows.len(), 5);
        assert!(
            rows[2].starts_with("   ├─ C  a  w Working on settings"),
            "{}",
            rows[2]
        );
        // Group rows keep their blank objective and activity columns in the default layout.
        assert!(rows[0].starts_with("G       Workspace"), "{}", rows[0]);
        press(&mut dialog, "all-right");
        let rows = dialog.specimen();
        assert!(
            rows[2].starts_with("   ├─ Working on settings"),
            "{}",
            rows[2]
        );
        assert!(rows[2].ends_with("C  a  w "), "{}", rows[2]);
        // Hidden icons leave the row; a non-default layout does not reserve blank columns.
        choose(&mut dialog, 1);
        press(&mut dialog, "toggle");
        let rows = dialog.specimen();
        assert!(!rows[2].contains('C'), "{}", rows[2]);
    }

    #[test]
    fn the_view_lists_every_slot_and_the_buttons() {
        let tree = dialog().render();
        let Widget::Dialog(spec) = &tree.nodes[0].widget else {
            panic!("the root is the dialog");
        };
        let ids: Vec<_> = spec
            .buttons
            .iter()
            .map(|button| button.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "done",
                "cancel",
                "up",
                "down",
                "toggle",
                "default",
                "all-right",
                "mixed"
            ]
        );
        assert_eq!(
            spec.buttons
                .iter()
                .filter(|button| button.is_cancel)
                .count(),
            1
        );
        let Widget::Choice(choice) = &tree.nodes[1].widget else {
            panic!("the first child is the slot choice");
        };
        assert_eq!(choice.options.len(), 4);
        assert_eq!(choice.selected, 0);
    }
}
