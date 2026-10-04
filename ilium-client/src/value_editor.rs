//! Pane-local editor choices preserve the existing buffer and viewport mutation path.
use crate::app::{App, Mode, PaneRuntime};
use crate::config::LineDisplay;
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, DialogOutcome};
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use ilium_core::NodeId;

impl App {
    pub(crate) fn begin_editor_line_display_dialog(&mut self, pane_id: NodeId) {
        if !matches!(self.mode, Mode::Normal) {
            return;
        }
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(&pane_id) else {
            return;
        };
        let target = ValueTarget::EditorLineDisplay {
            pane_id,
            path: editor.path.clone(),
            original: editor.line_display,
        };
        let options = [("Clip", "Clip"), ("Wrap", "Wrap")]
            .into_iter()
            .map(|(id, label)| ChoiceOption {
                id: id.into(),
                label: label.into(),
                disabled_reason: None,
            })
            .collect();
        let selected = format!("{:?}", editor.line_display);
        match ChoiceDialogState::new("Editor line display", options, Some(selected)) {
            Ok(dialog) => self.push_modal(Mode::ValueDialog(Box::new(
                ValueDialogHost::choice_host(target, dialog),
            ))),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub(crate) fn commit_editor_line_display_dialog(
        &mut self,
        host: &ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let ValueTarget::EditorLineDisplay {
            pane_id,
            path,
            original,
        } = &host.target
        else {
            return Err("This is not an editor line-display choice".into());
        };
        let DialogOutcome::Choose(id) = outcome else {
            return Err("Choose a line-display mode".into());
        };
        if !matches!(self.modal_stack.last(), Some(Mode::Normal)) {
            return Err("The editor parent changed; reopen its options".into());
        }
        let Some(PaneRuntime::Editor(editor)) = self.panes.get(pane_id) else {
            return Err("The editor closed; reopen its options".into());
        };
        if &editor.path != path || editor.line_display != *original {
            return Err(
                "The editor destination or line display changed; reopen its options".into(),
            );
        }
        let next = match id.as_str() {
            "Clip" => LineDisplay::Clip,
            "Wrap" => LineDisplay::Wrap,
            _ => return Err("This line-display mode is unavailable".into()),
        };
        if next != *original {
            // Reuse the existing TextArea wrap/scroll-follow and rendered-scroll clamp.
            self.execute_editor_toolbar_action(
                *pane_id,
                crate::editor_toolbar::ToolbarAction::ToggleLineDisplay,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_pane::EditorPane;
    use crate::value_dialog::ValueDialogState;

    fn opened() -> (App, Box<ValueDialogHost>, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("synthetic-editor-choice".into(), directory.path().into());
        let mut editor = EditorPane::empty();
        editor.path = Some("synthetic-notes.md".into());
        app.panes
            .insert(NodeId(7), PaneRuntime::Editor(Box::new(editor)));
        app.begin_editor_line_display_dialog(NodeId(7));
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("full editor choice dialog")
        };
        (app, host, directory)
    }

    #[test]
    fn full_catalog_changes_only_native_line_display() {
        let (mut app, host, _directory) = opened();
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("choice");
        };
        assert_eq!(dialog.options().len(), 2);
        let PaneRuntime::Editor(before) = &app.panes[&NodeId(7)] else {
            panic!("editor");
        };
        let lines = before.textarea.lines().to_vec();
        let revision = before.content_revision();
        let path = before.path.clone();
        app.commit_editor_line_display_dialog(&host, &DialogOutcome::Choose("Wrap".into()))
            .unwrap();
        let PaneRuntime::Editor(after) = &app.panes[&NodeId(7)] else {
            panic!("editor");
        };
        assert_eq!(after.line_display, LineDisplay::Wrap);
        assert_eq!(after.textarea.lines(), &lines);
        assert_eq!(after.content_revision(), revision);
        assert_eq!(after.path, path);
        assert!(app
            .commit_editor_line_display_dialog(&host, &DialogOutcome::Choose("Clip".into()))
            .is_err());
    }

    #[test]
    fn closed_retargeted_or_invalid_editor_choices_are_rejected() {
        let (mut app, host, _directory) = opened();
        assert!(app
            .commit_editor_line_display_dialog(&host, &DialogOutcome::Choose("invented".into()))
            .is_err());
        let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&NodeId(7)) else {
            panic!("editor");
        };
        editor.path = Some("different.md".into());
        assert!(app
            .commit_editor_line_display_dialog(&host, &DialogOutcome::Choose("Wrap".into()))
            .is_err());
        app.panes.remove(&NodeId(7));
        assert!(app
            .commit_editor_line_display_dialog(&host, &DialogOutcome::Choose("Wrap".into()))
            .is_err());
    }
}
