//! Deterministic confirmation policy for semantic commands.

use crate::app::{App, PaneRuntime};

use super::command::{
    BoardAction, BoardCommand, ControlCommand, EditorAction, NodeTarget, PromptDeliveryChoice,
    SessionAction, TerminalAction, TerminalCommand, TerminalKey, TerminalSubmissionCommand,
    TerminalTypingCommand, TreeAction, TreeCommand,
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
                    .ok_or_else(|| "command_line is required".to_owned())?;
                Some(format!(
                    "Run the shell command {command_line:?} in a new terminal pane?"
                ))
            }
            TreeAction::Close => return pinned_close_plan(app, command),
            _ => None,
        },
        ControlCommand::Editor(command)
            if matches!(command.action, EditorAction::ReplaceDocument) =>
        {
            Some("Replace the editor's entire current document?".to_owned())
        }
        ControlCommand::TerminalSubmission(command)
            if app.voice_settings.confirm_terminal_submissions =>
        {
            return staged_terminal_submission_plan(app, command);
        }
        ControlCommand::Terminal(command) if app.voice_settings.confirm_terminal_submissions => {
            match command.action {
                TerminalAction::PressKey if matches!(command.key, Some(TerminalKey::Enter)) => {
                    Some("Press Enter and submit what you see in the target terminal?".to_owned())
                }
                TerminalAction::ScheduleInput => {
                    Some("Schedule this terminal input for automatic submission?".to_owned())
                }
                // Queuing a prompt is at least as consequential as scheduling one:
                // `Forever`/`Times` delivery keeps auto-submitting into the pane
                // long after this call returns, so it must be guarded the same way.
                TerminalAction::QueuePrompt => Some(match command.delivery {
                    Some(PromptDeliveryChoice::Forever) => {
                        "Queue this prompt to be submitted automatically after every future completion, indefinitely?".to_owned()
                    }
                    Some(PromptDeliveryChoice::Times) => {
                        // Mirror the executor's own validation (see
                        // control/executor.rs's QueuePrompt/Times handling)
                        // so the confirmation question never promises a run
                        // count the execution step will then reject.
                        let runs = command.runs.filter(|runs| *runs > 0).ok_or_else(|| {
                            "runs is required and must be positive when delivery is times"
                                .to_owned()
                        })?;
                        format!(
                            "Queue this prompt to be submitted automatically for the next {runs} completions?"
                        )
                    }
                    Some(PromptDeliveryChoice::Once) | None => {
                        "Queue this prompt for automatic submission after the agent's next completion?".to_owned()
                    }
                }),
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
        cancellation_message: "Cancelled the pending action".to_owned(),
    }))
}

/// Resolves the board pane and the exact column/card the delete would remove,
/// pins the pane into the confirmed command's target, and names the doomed
/// item in the question. Mirrors the executor's own required-field and bounds
/// validation (control/executor.rs's DeleteCard/DeleteColumn handling) so the
/// confirmation question never promises a deletion the execution step would
/// reject -- and, like `pinned_close_plan`, prevents a focus change while the
/// question is pending from redirecting the delete to a different board.
fn pinned_board_delete_plan(
    app: &App,
    command: &BoardCommand,
) -> Result<Option<ConfirmationPlan>, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let Some(PaneRuntime::Board(board)) = app.panes.get(&pane_id) else {
        return Err("Target is not a board pane".to_owned());
    };

    let column_index = command.column.ok_or("column is required")?;
    let column = board
        .columns
        .get(column_index)
        .ok_or_else(|| format!("Board has no column {column_index}"))?;

    let question = match command.action {
        BoardAction::DeleteCard => {
            let card_index = command.card.ok_or("card is required")?;
            let card = column
                .cards
                .get(card_index)
                .ok_or_else(|| format!("Board has no card {card_index} in column {column_index}"))?;
            format!(
                "Permanently delete the card {:?} from column {:?} and the board's backing storage?",
                card.title, column.title
            )
        }
        BoardAction::DeleteColumn => format!(
            "Permanently delete the column {:?} and its {} card(s) from the board's backing storage?",
            column.title,
            column.cards.len()
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
        cancellation_message: "Cancelled the pending action".to_owned(),
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
        question: "I typed it into the target terminal without pressing Enter. Send what you see on screen?".to_owned(),
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
            delay_seconds: None,
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
    fn enabled_terminal_confirmation_guards_forever_queued_prompts() {
        let (mut app, pane_id, _) = terminal_submission_fixture();
        app.voice_settings.confirm_terminal_submissions = true;
        let command = ControlCommand::Terminal(TerminalCommand {
            action: TerminalAction::QueuePrompt,
            target: NodeTarget {
                id: Some(pane_id.0),
                name: None,
                path: None,
            },
            text: Some("keep going".to_owned()),
            key: None,
            lines: None,
            delay_seconds: None,
            delivery: Some(PromptDeliveryChoice::Forever),
            runs: None,
        });

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("an indefinitely-repeating queued prompt must be guarded");

        assert!(plan.question.contains("indefinitely"));
    }

    #[test]
    fn board_delete_confirmation_pins_the_board_and_names_the_card() {
        let (app, board_id) = board_fixture();
        let command = board_delete_card_command(Some(0), Some(0));

        let plan = confirmation_plan(&app, &command)
            .expect("policy should resolve")
            .expect("board deletion must be guarded");

        // The question must describe the exact card the delete would remove,
        // and the confirmed command must carry the board resolved *now* --
        // otherwise a later focus change before the user answers would
        // redirect the delete to whatever board happens to be focused then.
        assert!(plan.question.contains("Ship it"));
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
    fn board_delete_confirmation_rejects_an_out_of_range_card_before_asking() {
        let (app, _) = board_fixture();
        let command = board_delete_card_command(Some(0), Some(5));

        // Mirroring the executor's bounds validation: the policy must fail
        // loudly instead of asking a question the execution step would then
        // reject anyway.
        assert!(confirmation_plan(&app, &command).is_err());
    }

    fn board_fixture() -> (App, ilium_core::NodeId) {
        let directory = tempfile::tempdir().unwrap();
        let storage = ilium_core::BoardStorage::MarkdownFile {
            path: directory.path().join("board.md"),
        };
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let board_id = app
            .tree
            .add_board(group_id, "Board".to_owned(), storage.clone())
            .unwrap();
        let mut board = crate::board::BoardPane::create(storage).unwrap();
        board.add_card("Ship it".to_owned()).unwrap();
        app.panes
            .insert(board_id, PaneRuntime::Board(Box::new(board)));
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
