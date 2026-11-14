//! Deterministic confirmation policy for semantic commands.

use crate::app::App;

use super::command::{
    BoardAction, BoardCommand, ControlCommand, EditorAction, NodeTarget, SessionAction,
    TerminalAction, TerminalCommand, TerminalKey, TerminalSubmissionCommand, TerminalTypingCommand,
    TreeAction, TreeCommand,
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
            TreeAction::CreateCommandPane => {
                // Mirror the executor's own `required_nonempty` validation
                // (control/executor.rs) so the confirmation question never
                // shows a blank command that the execution step would then
                // reject outright.
                let command_line = command
                    .command_line
                    .as_deref()
                    .filter(|command_line| !command_line.trim().is_empty())
                    .ok_or_else(|| ilium_prompts::voice::VOICE_POLICY_COMMAND_LINE_IS_REQUIRED.to_owned())?;
                Some(ilium_prompts::render_value("voice/policy/run-the-shell-command-v0-in-a", &serde_json::json!({"v0": format!("{:?}", command_line)})))
            }
            TreeAction::Close => return pinned_close_plan(app, command),
            _ => None,
        },
        ControlCommand::Editor(command)
            if matches!(command.action, EditorAction::ReplaceDocument) =>
        {
            Some(ilium_prompts::voice::VOICE_POLICY_REPLACE_THE_EDITOR_S_ENTIRE_CURRENT_DOCUMENT.to_owned())
        }
        ControlCommand::TerminalSubmission(command)
            if app.voice_settings.confirm_terminal_submissions =>
        {
            return staged_terminal_submission_plan(app, command);
        }
        ControlCommand::Terminal(command) if app.voice_settings.confirm_terminal_submissions => {
            match command.action {
                TerminalAction::PressKey if matches!(command.key, Some(TerminalKey::Enter)) => {
                    Some(ilium_prompts::voice::VOICE_POLICY_PRESS_ENTER_AND_SUBMIT_WHAT_YOU_SEE.to_owned())
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
            return pinned_board_delete_plan(app, command);
        }
        ControlCommand::Session(command) => match command.action {
            SessionAction::KillSession => Some(
                ilium_prompts::voice::VOICE_POLICY_KILL_THE_ENTIRE_ILIUM_SESSION_AND_EVERY.to_owned(),
            ),
            SessionAction::RestartServer => Some(
                ilium_prompts::voice::VOICE_POLICY_RESTART_THE_DETACHED_ILIUM_SERVER_AND_TEMPORARILY
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
        cancellation_message: ilium_prompts::voice::VOICE_POLICY_CANCELLED_THE_PENDING_ACTION
            .to_owned(),
    }))
}

/// Resolves the close target once and pins it into the confirmed command's
/// target. `command.target` is frequently empty (voice-driven "close this"),
/// and an empty target re-resolves against whatever is focused/selected at
/// execution time (see `resolve_node`). Confirmation is answered on a later
/// round-trip, so focus can move in between; without pinning, confirming
/// "close the agent pane" could silently close a different pane the user
/// happened to focus while the question was pending.
fn pinned_close_plan(app: &App, command: &TreeCommand) -> Result<Option<ConfirmationPlan>, String> {
    let node_id = resolve_node(app, &command.target)?;
    let Some(question) = app.close_confirmation_message(node_id) else {
        return Ok(None);
    };

    Ok(Some(ConfirmationPlan {
        question,
        preparation: None,
        confirmed_command: ControlCommand::Tree(TreeCommand {
            target: NodeTarget {
                id: Some(node_id.0),
                name: None,
                path: None,
            },
            ..command.clone()
        }),
        cancellation_message: ilium_prompts::voice::VOICE_POLICY_CANCELLED_THE_PENDING_ACTION
            .to_owned(),
    }))
}

/// Resolves the board pane the delete is aimed at, pins it into the confirmed command's target
/// and names the doomed column or card by its position. The board's contents live in the Kanban
/// plugin, so the question cannot quote titles; the plugin validates the indices when the
/// confirmed command runs. Like `pinned_close_plan`, pinning prevents a focus change while the
/// question is pending from redirecting the delete to a different board.
fn pinned_board_delete_plan(
    app: &App,
    command: &BoardCommand,
) -> Result<Option<ConfirmationPlan>, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let is_board = app
        .tree
        .plugin_pane_kind(pane_id)
        .is_some_and(|kind| kind.kind_id() == crate::plugin_host::commands::BOARD_PANE_KIND);
    if !is_board {
        return Err(ilium_prompts::voice::VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE.to_owned());
    }

    let column_index = command
        .column
        .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?;
    let column_label = format!("number {column_index}");

    let question = match command.action {
        BoardAction::DeleteCard => {
            let card_index = command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?;
            ilium_prompts::render_value(
                "voice/policy/permanently-delete-the-card-v0-from-column",
                &serde_json::json!({"v0": format!("number {card_index}"), "v1": column_label}),
            )
        }
        BoardAction::DeleteColumn => ilium_prompts::render_value(
            "voice/policy/permanently-delete-the-column-v0-and-its",
            &serde_json::json!({"v0": column_label, "v1": "all of its"}),
        ),
        // `confirmation_plan` only routes DeleteCard/DeleteColumn here.
        _ => return Ok(None),
    };

    Ok(Some(ConfirmationPlan {
        question,
        preparation: None,
        confirmed_command: ControlCommand::Board(BoardCommand {
            target: NodeTarget {
                id: Some(pane_id.0),
                name: None,
                path: None,
            },
            ..command.clone()
        }),
        cancellation_message: ilium_prompts::voice::VOICE_POLICY_CANCELLED_THE_PENDING_ACTION
            .to_owned(),
    }))
}

fn staged_terminal_submission_plan(
    app: &App,
    command: &TerminalSubmissionCommand,
) -> Result<Option<ConfirmationPlan>, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let target = NodeTarget {
        id: Some(pane_id.0),
        name: None,
        path: None,
    };

    Ok(Some(ConfirmationPlan {
        question: ilium_prompts::voice::VOICE_POLICY_I_TYPED_IT_INTO_THE_TARGET_TERMINAL.to_owned(),
        preparation: Some(ControlCommand::TerminalTyping(TerminalTypingCommand {
            target: target.clone(),
            text: command.text.clone(),
        })),
        confirmed_command: ControlCommand::Terminal(TerminalCommand {
            action: TerminalAction::PressKey,
            target,
            text: None,
            key: Some(TerminalKey::Enter),
            lines: None,
        }),
        cancellation_message:
            ilium_prompts::voice::VOICE_POLICY_LEFT_THE_STAGED_TERMINAL_TEXT_VISIBLE_AND.to_owned(),
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
            workspace: None,
        });

        assert!(confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .is_none());
    }

    #[test]
    fn closing_with_an_empty_target_pins_the_resolved_node_for_confirmation() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree
            .add_pane(group_id, "agent", PaneContentKind::Terminal)
            .unwrap();
        // No pane is focused; only the tree selection resolves the empty
        // target below, mirroring a voice-driven "close this" invocation.
        app.select_node(group_id);

        let command = ControlCommand::Tree(TreeCommand {
            action: TreeAction::Close,
            target: NodeTarget::default(),
            parent: NodeTarget::default(),
            name: None,
            path: None,
            command_line: None,
            initial_input: None,
            orientation: None,
            members: Vec::new(),
            index: None,
            storage: None,
            provider: None,
            workspace: None,
        });

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("closing a non-empty group must be guarded");

        assert!(plan.question.contains("work"));
        // The confirmed command must carry the node resolved *now*, not the
        // empty target -- otherwise a later focus/selection change before
        // the user answers would redirect the close to a different node.
        assert!(matches!(
            plan.confirmed_command,
            ControlCommand::Tree(TreeCommand {
                action: TreeAction::Close,
                target: NodeTarget { id: Some(id), .. },
                ..
            }) if id == group_id.0
        ));
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
            Some(ControlCommand::TerminalTyping(TerminalTypingCommand {
                target: NodeTarget { id: Some(id), .. },
                ref text,
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

    #[test]
    fn board_delete_confirmation_pins_the_board_and_names_the_card() {
        let (app, board_id) = board_fixture();
        let command = board_delete_card_command(Some(0), Some(2));

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("board deletion must be guarded");

        // The question names the card by position, and the confirmed command carries the board
        // resolved *now*, so a later focus change cannot redirect the delete.
        assert!(plan.question.contains("number 2"));
        assert!(matches!(
            plan.confirmed_command,
            ControlCommand::Board(BoardCommand {
                action: BoardAction::DeleteCard,
                target: NodeTarget { id: Some(id), .. },
                ..
            }) if id == board_id.0
        ));
    }

    #[test]
    fn board_delete_confirmation_requires_a_card_before_asking() {
        let (app, _) = board_fixture();
        let command = board_delete_card_command(Some(0), None);
        assert!(confirmation_plan(&app, &command).is_err());
    }

    #[test]
    fn board_delete_confirmation_refuses_a_pane_that_is_not_a_board() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group_id, "notes", ilium_core::PaneContentKind::Editor)
            .unwrap();
        app.focus_pane(editor_id);
        assert!(confirmation_plan(&app, &board_delete_card_command(Some(0), Some(0))).is_err());
    }

    fn board_fixture() -> (App, ilium_core::NodeId) {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let board_id = app
            .tree
            .add_plugin_pane(
                group_id,
                "Board",
                ilium_core::PluginPaneKind::new(crate::plugin_host::commands::BOARD_PANE_KIND)
                    .unwrap(),
            )
            .unwrap();
        app.focus_pane(board_id);
        (app, board_id)
    }

    fn board_delete_card_command(column: Option<usize>, card: Option<usize>) -> ControlCommand {
        ControlCommand::Board(BoardCommand {
            action: BoardAction::DeleteCard,
            target: NodeTarget::default(),
            title: None,
            body: None,
            column,
            card,
            destination_column: None,
            destination_card: None,
            checkbox: None,
        })
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
        let command = ControlCommand::TerminalSubmission(TerminalSubmissionCommand {
            target: NodeTarget::default(),
            text: "/clear".to_owned(),
        });

        (app, pane_id, command)
    }
}
