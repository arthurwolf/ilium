//! The "Editor line display" choice: clip or wrap the long lines of one editor pane.
//!
//! The editor toolbar asks for this dialog by running the `editor-line-display` command with
//! `{"pane": <id>, "line_display": "clip"|"wrap"}` (the pane and its current mode). This module
//! holds the rules and the popup; `lib.rs` connects it to events and intents. The change
//! itself is the kernel command `kernel.set-editor-option`, which toggles the editor through the
//! toolbar's own action, so the wrap, scroll clamp and rendered-Markdown rebuild of a click
//! apply too. The dialog changes presentation only, never the file.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogButton,
    DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span, Style, UiNode, UiTree, ViewTarget,
    Widget,
};
use serde_json::Value;

/// Local id of the command that opens the dialog.
pub const COMMAND: &str = "editor-line-display";
/// Popup id of the dialog.
pub const POPUP: &str = "editor-line-display";
/// Key of the choice widget.
pub const MODE_KEY: &str = "mode";
/// The kernel command that sets one option of an editor pane.
pub const SET_OPTION_COMMAND: &str = "kernel.set-editor-option";
/// Title of the dialog.
pub const TITLE: &str = "Editor line display";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// How an editor shows a line longer than its pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineDisplay {
    Clip,
    Wrap,
}

impl LineDisplay {
    /// Both modes, in the order the dialog lists them.
    pub const ALL: [Self; 2] = [Self::Clip, Self::Wrap];

    /// The label the dialog shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clip => "Clip",
            Self::Wrap => "Wrap",
        }
    }

    /// The value `kernel.set-editor-option` takes.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Wrap => "wrap",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.wire() == text)
    }

    fn index(self) -> u32 {
        match self {
            Self::Clip => 0,
            Self::Wrap => 1,
        }
    }

    fn from_index(index: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }
}

/// What the command asks for: the editor pane and the mode it shows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub pane: u64,
    pub current: LineDisplay,
}

impl Request {
    /// Reads the command arguments; `None` for anything malformed, so nothing opens.
    pub fn from_arguments(arguments_json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(arguments_json).ok()?;
        Some(Self {
            pane: value.get("pane")?.as_u64()?,
            current: LineDisplay::parse(value.get("line_display")?.as_str()?)?,
        })
    }
}

/// The arguments of `kernel.set-editor-option` for one pane and mode.
pub fn set_option_arguments(pane: u64, mode: LineDisplay) -> String {
    serde_json::json!({
        "pane": pane,
        "option": "line-display",
        "value": mode.wire(),
    })
    .to_string()
}

/// What an action did to the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Still open.
    Stay,
    /// Finished: set this mode on this pane.
    Apply { pane: u64, mode: LineDisplay },
    /// Finished without a change (cancelled, closed, or the mode did not change).
    Cancel,
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub pane: u64,
    pub current: LineDisplay,
    pub selected: LineDisplay,
}

impl Dialog {
    /// Opens the dialog on the mode the pane shows now.
    pub fn new(request: Request) -> Self {
        Self {
            pane: request.pane,
            current: request.current,
            selected: request.current,
        }
    }

    /// Applies one action the kernel reported for the popup. An out-of-range choice index is
    /// ignored, and choosing the mode the pane already has ends the dialog without a command.
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
                if key == MODE_KEY =>
            {
                if let Some(mode) = LineDisplay::from_index(index) {
                    self.selected = mode;
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "apply" if self.selected == self.current => Step::Cancel,
                "apply" => Step::Apply {
                    pane: self.pane,
                    mode: self.selected,
                },
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view.
    pub fn render(&self) -> UiTree {
        let dialog = DialogSpec {
            title: vec![span(TITLE)],
            kind: DialogKind::Plain,
            buttons: vec![
                button("apply", "Apply", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(48),
            height: Constraint::Min(0),
            message: Some(vec![vec![span("How should this editor show long lines?")]]),
        };
        let choice = ChoiceSpec {
            label: "Long lines".to_owned(),
            options: LineDisplay::ALL
                .iter()
                .map(|mode| mode.label().to_owned())
                .collect(),
            selected: self.selected.index(),
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
                node(MODE_KEY, Widget::Choice(choice)),
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

    fn request(current: LineDisplay) -> Request {
        Request { pane: 7, current }
    }

    fn choose(dialog: &mut Dialog, index: u32) -> Step {
        dialog.act(
            MODE_KEY,
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(index)),
        )
    }

    #[test]
    fn arguments_name_the_pane_and_its_current_mode() {
        assert_eq!(
            Request::from_arguments(r#"{"pane": 7, "line_display": "wrap"}"#),
            Some(request(LineDisplay::Wrap))
        );
        for bad in [
            "",
            "[]",
            r#"{"pane": 7}"#,
            r#"{"line_display": "clip"}"#,
            r#"{"pane": "7", "line_display": "clip"}"#,
            r#"{"pane": 7, "line_display": "fold"}"#,
        ] {
            assert_eq!(Request::from_arguments(bad), None, "{bad}");
        }
    }

    #[test]
    fn choosing_the_other_mode_applies_exactly_that_mode() {
        let mut dialog = Dialog::new(request(LineDisplay::Clip));
        assert_eq!(choose(&mut dialog, 1), Step::Stay);
        assert_eq!(
            dialog.act("dialog", "apply", ActionKind::Activated, None),
            Step::Apply {
                pane: 7,
                mode: LineDisplay::Wrap
            }
        );
    }

    #[test]
    fn choosing_the_current_mode_changes_nothing() {
        let mut dialog = Dialog::new(request(LineDisplay::Wrap));
        assert_eq!(
            dialog.act("dialog", "apply", ActionKind::Activated, None),
            Step::Cancel
        );
        choose(&mut dialog, 0);
        choose(&mut dialog, 1);
        assert_eq!(
            dialog.act("dialog", "apply", ActionKind::Activated, None),
            Step::Cancel
        );
    }

    #[test]
    fn an_unknown_choice_or_button_is_ignored() {
        let mut dialog = Dialog::new(request(LineDisplay::Clip));
        assert_eq!(choose(&mut dialog, 2), Step::Stay);
        assert_eq!(choose(&mut dialog, u32::MAX), Step::Stay);
        assert_eq!(dialog.selected, LineDisplay::Clip);
        assert_eq!(
            dialog.act("dialog", "invented", ActionKind::Activated, None),
            Step::Stay
        );
        assert_eq!(
            dialog.act(
                "other",
                "",
                ActionKind::Selected,
                Some(ActionValue::Index(1))
            ),
            Step::Stay
        );
    }

    #[test]
    fn closing_or_cancelling_ends_without_a_change() {
        let mut dialog = Dialog::new(request(LineDisplay::Clip));
        choose(&mut dialog, 1);
        assert_eq!(
            dialog.act("dialog", "", ActionKind::Closed, None),
            Step::Cancel
        );
        assert_eq!(
            dialog.act("dialog", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
    }

    #[test]
    fn the_kernel_command_arguments_are_the_ones_the_client_accepts() {
        let arguments: Value =
            serde_json::from_str(&set_option_arguments(9, LineDisplay::Wrap)).expect("json");
        assert_eq!(arguments["pane"], 9);
        assert_eq!(arguments["option"], "line-display");
        assert_eq!(arguments["value"], "wrap");
    }

    #[test]
    fn the_popup_lists_both_modes_and_marks_the_selected_one() {
        let mut dialog = Dialog::new(request(LineDisplay::Wrap));
        let tree = dialog.render();
        let choice = tree
            .nodes
            .iter()
            .find_map(|node| match &node.widget {
                Widget::Choice(choice) => Some(choice),
                _ => None,
            })
            .expect("a choice widget");
        assert_eq!(choice.options, ["Clip", "Wrap"]);
        assert_eq!(choice.selected, 1);
        choose(&mut dialog, 0);
        assert!(dialog
            .render()
            .nodes
            .iter()
            .any(|node| matches!(&node.widget, Widget::Choice(choice) if choice.selected == 0)));
    }
}
