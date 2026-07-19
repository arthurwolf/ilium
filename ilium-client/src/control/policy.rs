//! Deterministic confirmation policy for semantic commands.

use crate::app::App;

use super::command::{
    BoardAction, ControlCommand, EditorAction, NodeTarget, SessionAction, TerminalAction,
    TerminalCommand, TerminalKey, TreeAction,
};
use super::resolver::resolve_node;

/// A guarded command plus an optional preparation that runs before asking.
/// Terminal submission uses preparation to make the confirmation concrete:
/// the text is already visible, while the guarded command is only Enter.
pub struct ConfirmationPlan {
    pub question: String,
    pub preparation: Option<ControlCommand>,
    pub confirmed_command: ControlCommand,
    pub cancellation_message: String,
}

/// Builds confirmation behavior for one semantic command. Terminal guards are
/// user-configurable; destructive lifecycle and persistence guards remain
/// mandatory regardless of that preference.
pub fn confirmation_plan(
    app: &App,
    command: &ControlCommand,
) -> Result<Option<ConfirmationPlan>, String> {
    let question = match command {
        ControlCommand::Tree(command) => match command.action {
            TreeAction::CreateCommandPane => Some(format!(
                "Run the shell command {:?} in a new terminal pane?",
                command.command_line.as_deref().unwrap_or_default()
            )),
            TreeAction::Close => {
                let node_id = resolve_node(app, &command.target)?;
                app.close_confirmation_message(node_id)
            }
            _ => None,
        },
        ControlCommand::Editor(command)
            if matches!(command.action, EditorAction::ReplaceDocument) =>
        {
            Some("Replace the editor's entire current document?".to_owned())
        }
        ControlCommand::Terminal(command) if app.voice_settings.confirm_terminal_submissions => {
            match command.action {
                TerminalAction::Write if command.send_enter.unwrap_or(false) => {
                    return staged_terminal_submission_plan(app, command);
                }
                TerminalAction::PressKey if matches!(command.key, Some(TerminalKey::Enter)) => {
                    Some("Press Enter and submit what you see in the target terminal?".to_owned())
                }
                TerminalAction::ScheduleInput if command.send_enter.unwrap_or(true) => {
                    Some("Schedule this terminal input for automatic submission?".to_owned())
                }
                _ => None,
            }
        }
        ControlCommand::Board(command)
            if matches!(
                command.action,
                BoardAction::DeleteCard | BoardAction::DeleteColumn
            ) =>
        {
            Some("Permanently delete the selected Kanban item from its backing storage?".to_owned())
        }
        ControlCommand::Session(command) => match command.action {
            SessionAction::KillSession => Some(
                "Kill the entire ilium session and every process running in its panes?".to_owned(),
            ),
            SessionAction::RestartServer => Some(
                "Restart the detached ilium server and temporarily disconnect this client?"
                    .to_owned(),
            ),
            SessionAction::Detach | SessionAction::RestartClient => None,
        },
        ControlCommand::Search(_) => None,
        _ => None,
    };

    Ok(question.map(|question| ConfirmationPlan {
        question,
        preparation: None,
        confirmed_command: command.clone(),
        cancellation_message: "Cancelled the pending action".to_owned(),
    }))
}

fn staged_terminal_submission_plan(
    app: &App,
    command: &TerminalCommand,
) -> Result<Option<ConfirmationPlan>, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let target = NodeTarget {
        id: Some(pane_id.0),
        name: None,
        path: None,
    };
    let mut staged_command = command.clone();
    staged_command.target = target.clone();
    staged_command.send_enter = Some(false);

    Ok(Some(ConfirmationPlan {
        question: "I typed it into the target terminal without pressing Enter. Send what you see on screen?".to_owned(),
        preparation: Some(ControlCommand::Terminal(staged_command)),
        confirmed_command: ControlCommand::Terminal(TerminalCommand {
            action: TerminalAction::PressKey,
            target,
            text: None,
            key: Some(TerminalKey::Enter),
            lines: None,
            delay_seconds: None,
            send_enter: None,
            delivery: None,
            runs: None,
        }),
        cancellation_message: "Left the staged terminal text visible and unsubmitted".to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ilium_core::{PaneContentKind, ROOT_ID};

    use super::*;
    use crate::app::PaneRuntime;
    use crate::control::command::{SessionCommand, TreeCommand};
    use crate::terminal_view::TerminalView;

    #[test]
    fn killing_a_session_always_requires_confirmation() {
        let app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let command = ControlCommand::Session(SessionCommand {
            action: SessionAction::KillSession,
        });

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("kill must be guarded");

        assert!(plan.question.contains("every process"));
    }

    #[test]
    fn ordinary_terminal_creation_does_not_require_confirmation() {
        let app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let command = ControlCommand::Tree(TreeCommand {
            action: TreeAction::CreateTerminal,
            target: Default::default(),
            parent: Default::default(),
            name: None,
            path: None,
            command_line: None,
            initial_input: None,
            orientation: None,
            members: Vec::new(),
            index: None,
            storage: None,
            provider: None,
        });

        assert!(confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .is_none());
    }

    #[test]
    fn terminal_submission_confirmation_defaults_to_immediate_execution() {
        let (app, _, command) = terminal_submission_fixture();

        assert!(!app.voice_settings.confirm_terminal_submissions);
        assert!(confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .is_none());
    }

    #[test]
    fn enabled_terminal_confirmation_stages_text_and_guards_only_enter() {
        let (mut app, pane_id, command) = terminal_submission_fixture();
        app.voice_settings.confirm_terminal_submissions = true;

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("terminal submission should be guarded");

        assert!(!plan.question.contains("/clear"));
        assert!(plan.question.contains("what you see on screen"));
        assert!(matches!(
            plan.preparation,
            Some(ControlCommand::Terminal(TerminalCommand {
                action: TerminalAction::Write,
                target: NodeTarget { id: Some(id), .. },
                text: Some(ref text),
                send_enter: Some(false),
                ..
            })) if id == pane_id.0 && text == "/clear"
        ));
        assert!(matches!(
            plan.confirmed_command,
            ControlCommand::Terminal(TerminalCommand {
                action: TerminalAction::PressKey,
                target: NodeTarget { id: Some(id), .. },
                key: Some(TerminalKey::Enter),
                ..
            }) if id == pane_id.0
        ));
    }

    fn terminal_submission_fixture() -> (App, ilium_core::NodeId, ControlCommand) {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.focus_pane(pane_id);
        let command = ControlCommand::Terminal(TerminalCommand {
            action: TerminalAction::Write,
            target: NodeTarget::default(),
            text: Some("/clear".to_owned()),
            key: None,
            lines: None,
            delay_seconds: None,
            send_enter: Some(true),
            delivery: None,
            runs: None,
        });

        (app, pane_id, command)
    }
}
