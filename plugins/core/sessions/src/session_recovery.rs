//! The crash-recovery question: restore the stored snapshot or discard it and start fresh.
//!
//! With the recovery policy "Ask before restoring" the server holds the attach until a client
//! answers. The client kernel hands the question to this plugin by running the command
//! `session-recovery` with `{"pane_count": <n>}` (the number of panes the snapshot stores). This
//! module holds the wording and the rules of the popup; `lib.rs` connects it to events and
//! intents. The answer is the kernel command `kernel.resolve-session-recovery`, which tells the
//! server what to do. Closing the popup without a button answers nothing: the kernel asks again,
//! so a stray click or a popup that could not open never throws a snapshot away.

use ilium_plugin_sdk::types::{
    ActionKind, ButtonRole, Constraint, DialogButton, DialogKind, DialogSpec, NodeFlags, Span,
    Style, UiNode, UiTree, ViewTarget, Widget,
};
use serde_json::Value;

/// Local id of the command that opens the dialog.
pub const COMMAND: &str = "session-recovery";
/// Popup id of the dialog.
pub const POPUP: &str = "session-recovery";
/// The kernel command that answers the server.
pub const RESOLVE_COMMAND: &str = "kernel.resolve-session-recovery";
/// Title of the dialog.
pub const TITLE: &str = "Restore previous session?";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// What the command asks for: how many panes the stored snapshot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub pane_count: u64,
}

impl Request {
    /// Reads the command arguments; `None` for anything malformed, so nothing opens.
    pub fn from_arguments(arguments_json: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(arguments_json).ok()?;
        Some(Self {
            pane_count: value.get("pane_count")?.as_u64()?,
        })
    }
}

/// The arguments of `kernel.resolve-session-recovery` for one answer.
pub fn resolve_arguments(restore: bool) -> String {
    serde_json::json!({ "restore": restore }).to_string()
}

/// What an action did to the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Still open, or an event that answers nothing (the popup was closed without a button).
    Stay,
    /// The user chose: restore the snapshot (`true`) or discard it and start fresh (`false`).
    Resolve { restore: bool },
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub pane_count: u64,
}

impl Dialog {
    /// Opens the dialog for a snapshot of `request.pane_count` panes.
    pub fn new(request: Request) -> Self {
        Self {
            pane_count: request.pane_count,
        }
    }

    /// Applies one action the kernel reported for the popup. Only the two buttons answer; a
    /// close (Escape, a click outside, a popup the kernel could not show) answers nothing.
    pub fn act(&mut self, action_id: &str, kind: ActionKind) -> Step {
        match (kind, action_id) {
            (ActionKind::Activated, "restore") => Step::Resolve { restore: true },
            (ActionKind::Activated, "discard") => Step::Resolve { restore: false },
            _ => Step::Stay,
        }
    }

    /// The message line of the dialog.
    pub fn message(&self) -> String {
        format!(
            "A stored snapshot contains {} pane(s). Restore it, or discard it and start fresh?",
            self.pane_count
        )
    }

    /// The popup view.
    pub fn render(&self) -> UiTree {
        let dialog = DialogSpec {
            title: vec![span(TITLE)],
            kind: DialogKind::Plain,
            buttons: vec![
                button("discard", "Discard", ButtonRole::Danger),
                button("restore", "Restore", ButtonRole::Primary),
            ],
            width: Constraint::Length(56),
            height: Constraint::Min(0),
            message: Some(vec![vec![span(&self.message())]]),
        };
        UiTree {
            version: 1,
            nodes: vec![UiNode {
                key: Some("dialog".to_owned()),
                widget: Widget::Dialog(dialog),
                children: Vec::new(),
                flags: NodeFlags::default(),
                tooltip: None,
                tab_index: None,
                disabled_reason: None,
            }],
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

fn button(id: &str, label: &str, role: ButtonRole) -> DialogButton {
    DialogButton {
        id: id.to_owned(),
        label: label.to_owned(),
        role,
        is_cancel: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(pane_count: u64) -> Dialog {
        Dialog::new(Request { pane_count })
    }

    #[test]
    fn arguments_name_the_stored_pane_count() {
        assert_eq!(
            Request::from_arguments(r#"{"pane_count": 12}"#),
            Some(Request { pane_count: 12 })
        );
        for bad in [
            "",
            "[]",
            "{}",
            r#"{"pane_count": "3"}"#,
            r#"{"pane_count": -1}"#,
        ] {
            assert_eq!(Request::from_arguments(bad), None, "{bad}");
        }
    }

    #[test]
    fn restore_and_discard_answer_the_server() {
        let mut open = dialog(3);
        assert_eq!(
            open.act("restore", ActionKind::Activated),
            Step::Resolve { restore: true }
        );
        assert_eq!(
            open.act("discard", ActionKind::Activated),
            Step::Resolve { restore: false }
        );
    }

    #[test]
    fn closing_or_an_unknown_button_answers_nothing() {
        let mut open = dialog(3);
        assert_eq!(open.act("", ActionKind::Closed), Step::Stay);
        assert_eq!(open.act("restore", ActionKind::Closed), Step::Stay);
        assert_eq!(open.act("invented", ActionKind::Activated), Step::Stay);
        assert_eq!(open.act("restore", ActionKind::Changed), Step::Stay);
    }

    #[test]
    fn the_answer_arguments_are_the_ones_the_client_accepts() {
        let yes: Value = serde_json::from_str(&resolve_arguments(true)).expect("json");
        let no: Value = serde_json::from_str(&resolve_arguments(false)).expect("json");
        assert_eq!(yes["restore"], true);
        assert_eq!(no["restore"], false);
    }

    #[test]
    fn the_popup_keeps_the_original_wording_and_makes_restore_the_default() {
        let open = dialog(12);
        assert_eq!(
            open.message(),
            "A stored snapshot contains 12 pane(s). Restore it, or discard it and start fresh?"
        );
        let tree = open.render();
        let Widget::Dialog(spec) = &tree.nodes[0].widget else {
            panic!("the root is a dialog");
        };
        assert_eq!(spec.title[0].text, "Restore previous session?");
        let labels: Vec<_> = spec.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Discard", "Restore"]);
        let primary: Vec<_> = spec
            .buttons
            .iter()
            .filter(|b| b.role == ButtonRole::Primary)
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(primary, ["restore"]);
        assert!(spec.buttons.iter().all(|b| !b.is_cancel));
    }
}
