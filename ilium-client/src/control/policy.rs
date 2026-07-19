//! Deterministic confirmation policy for semantic commands.

use crate::app::App;

use super::command::{
    BoardAction, ControlCommand, EditorAction, SessionAction, TerminalAction, TerminalKey,
    TreeAction,
};
use super::resolver::resolve_node;

/// Returns the exact user-facing confirmation question for a high-impact
/// command. Absence means the command may execute immediately.
pub fn confirmation_question(app: &App, command: &ControlCommand) -> Option<String> {
    match command {
        ControlCommand::Tree(command) => match command.action {
            TreeAction::CreateCommandPane => Some(format!(
                "Run the shell command {:?} in a new terminal pane?",
                command.command_line.as_deref().unwrap_or_default()
            )),
            TreeAction::Close => {
                let node_id = resolve_node(app, &command.target).ok()?;
                app.close_confirmation_message(node_id)
            }
            _ => None,
        },
        ControlCommand::Editor(command)
            if matches!(command.action, EditorAction::ReplaceDocument) =>
        {
            Some("Replace the editor's entire current document?".to_owned())
        }
        ControlCommand::Terminal(command) => match command.action {
            TerminalAction::Write if command.send_enter.unwrap_or(false) => Some(format!(
                "Send and submit {:?} to the target terminal?",
                command.text.as_deref().unwrap_or_default()
            )),
            TerminalAction::PressKey if matches!(command.key, Some(TerminalKey::Enter)) => Some(
                "Press Enter in the target terminal, submitting its current command line?"
                    .to_owned(),
            ),
            TerminalAction::ScheduleInput if command.send_enter.unwrap_or(true) => Some(format!(
                "Schedule {:?} to be submitted to the target terminal?",
                command.text.as_deref().unwrap_or_default()
            )),
            _ => None,
        },
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
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::control::command::{SessionCommand, TreeCommand};

    #[test]
    fn killing_a_session_always_requires_confirmation() {
        let app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let command = ControlCommand::Session(SessionCommand {
            action: SessionAction::KillSession,
        });

        assert!(confirmation_question(&app, &command)
            .expect("kill must be guarded")
            .contains("every process"));
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

        assert_eq!(confirmation_question(&app, &command), None);
    }
}
