//! Agent control of a board: the commands the kernel control plane (the voice agent and
//! `ilium ctl`) sends to a board pane.
//!
//! Moved from `ilium-client/src/control/executor.rs` (`execute_board`, `select_column`,
//! `select_card`). The kernel runs the `ilium.core.kanban/control` command with one JSON object
//! `{ "pane": <node id>, "action": "<name>", ... }`; the plugin applies it to the board of that
//! pane and writes the file like any other edit. Every index is zero based.

use serde_json::Value;

use crate::model::{Board, Edit};

/// The command that carries a control action: `{ "pane", "action", ... }`.
pub const COMMAND: &str = "ilium.core.kanban/control";

/// What a control command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SelectColumn,
    SelectCard,
    OpenCard,
    CloseCard,
    AddColumn,
    AddCard,
    UpdateCard,
    RenameColumn,
    MoveCard,
    ToggleCheckbox,
    DeleteCard,
    DeleteColumn,
}

impl Action {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "select_column" => Self::SelectColumn,
            "select_card" => Self::SelectCard,
            "open_card" => Self::OpenCard,
            "close_card" => Self::CloseCard,
            "add_column" => Self::AddColumn,
            "add_card" => Self::AddCard,
            "update_card" => Self::UpdateCard,
            "rename_column" => Self::RenameColumn,
            "move_card" => Self::MoveCard,
            "toggle_checkbox" => Self::ToggleCheckbox,
            "delete_card" => Self::DeleteCard,
            "delete_column" => Self::DeleteColumn,
            _ => return None,
        })
    }
}

/// One control command of an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub action: Action,
    pub title: Option<String>,
    pub body: Option<String>,
    pub column: Option<usize>,
    pub card: Option<usize>,
    pub destination_column: Option<usize>,
    pub destination_card: Option<usize>,
    pub checkbox: Option<usize>,
}

/// Reads the arguments of the control command: the pane it is for and what to do.
pub fn parse(arguments_json: &str) -> Result<(u64, Command), String> {
    let value: Value = serde_json::from_str(arguments_json)
        .map_err(|_| "The board command is not JSON".to_string())?;
    let pane = value["pane"]
        .as_u64()
        .ok_or_else(|| "The board command needs a pane".to_string())?;
    let action = value["action"]
        .as_str()
        .and_then(Action::from_name)
        .ok_or_else(|| "Unknown board action".to_string())?;
    let index = |name: &str| value[name].as_u64().map(|number| number as usize);
    let text = |name: &str| value[name].as_str().map(str::to_string);
    Ok((
        pane,
        Command {
            action,
            title: text("title"),
            body: text("body"),
            column: index("column"),
            card: index("card"),
            destination_column: index("destination_column"),
            destination_card: index("destination_card"),
            checkbox: index("checkbox"),
        },
    ))
}

fn column_of(command: &Command) -> Result<usize, String> {
    command
        .column
        .ok_or_else(|| "A column is required".to_string())
}

fn card_of(command: &Command) -> Result<usize, String> {
    command.card.ok_or_else(|| "A card is required".to_string())
}

pub fn title_of(command: &Command) -> Result<&str, String> {
    match command.title.as_deref() {
        None => Err("title is required".to_string()),
        Some(title) if title.trim().is_empty() => Err("title must not be empty".to_string()),
        Some(title) => Ok(title),
    }
}

/// Selects `column_index`, failing loudly instead of keeping the previous selection: a later
/// delete, rename or add would otherwise act on the column that was selected before.
pub fn select_column(board: &mut Board, column_index: usize) -> Result<(), String> {
    board.select_column(column_index);
    if board.selected_column == column_index {
        Ok(())
    } else {
        Err(format!("The board has no column {column_index}"))
    }
}

/// Selects `(column_index, card_index)`; fails for the same reason as [`select_column`].
pub fn select_card(
    board: &mut Board,
    column_index: usize,
    card_index: usize,
) -> Result<(), String> {
    board.select_card(column_index, card_index);
    if board.selected_column == column_index && board.selected_card == Some(card_index) {
        Ok(())
    } else {
        Err(format!(
            "The board has no card {card_index} in column {column_index}"
        ))
    }
}

/// Applies `command` to `board`. `Ok(true)` when the columns changed and the file must be written.
pub fn apply(board: &mut Board, command: &Command) -> Edit {
    match command.action {
        Action::SelectColumn => select_column(board, column_of(command)?).map(|()| false),
        Action::SelectCard => {
            select_card(board, column_of(command)?, card_of(command)?).map(|()| false)
        }
        Action::OpenCard => {
            // `open_card_details` does nothing for an index out of range; validating through
            // `select_card` first makes a bad index fail instead of reporting success.
            let (column, card) = (column_of(command)?, card_of(command)?);
            select_card(board, column, card)?;
            board.open_card_details(column, card);
            Ok(false)
        }
        Action::CloseCard => {
            board.close_card_details();
            Ok(false)
        }
        Action::AddColumn => board.add_column(title_of(command)?),
        Action::AddCard => {
            if let Some(column) = command.column {
                select_column(board, column)?;
            }
            board.add_card(title_of(command)?)
        }
        Action::UpdateCard => board.update_card(
            column_of(command)?,
            card_of(command)?,
            command.title.as_deref(),
            command.body.as_deref(),
        ),
        Action::RenameColumn => {
            select_column(board, column_of(command)?)?;
            board
                .rename_selected_column(title_of(command)?)
                .map(|renamed| renamed.is_some())
        }
        Action::MoveCard => board.move_card(
            column_of(command)?,
            card_of(command)?,
            command
                .destination_column
                .ok_or_else(|| "A destination column is required".to_string())?,
            command.destination_card.unwrap_or(usize::MAX),
        ),
        Action::ToggleCheckbox => board.toggle_card_checkbox(
            column_of(command)?,
            card_of(command)?,
            command
                .checkbox
                .ok_or_else(|| "A checkbox index is required".to_string())?,
        ),
        Action::DeleteCard => {
            select_card(board, column_of(command)?, card_of(command)?)?;
            board.delete_selected_card()
        }
        Action::DeleteColumn => {
            select_column(board, column_of(command)?)?;
            board.delete_selected_column().map(|_| true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{default_columns, Storage};

    fn board() -> Board {
        let mut board = Board::new(
            Storage::MarkdownFile {
                path: "plan.md".to_string(),
            },
            default_columns(),
        );
        board.select_column(0);
        board.add_card("first").unwrap();
        board
    }

    fn command(action: Action) -> Command {
        Command {
            action,
            title: None,
            body: None,
            column: None,
            card: None,
            destination_column: None,
            destination_card: None,
            checkbox: None,
        }
    }

    #[test]
    fn parse_reads_the_pane_the_action_and_the_indices() {
        let (pane, parsed) = parse(
            r#"{"pane": 7, "action": "move_card", "column": 0, "card": 1,
                "destination_column": 2, "destination_card": 0}"#,
        )
        .expect("a valid command");
        assert_eq!(pane, 7);
        assert_eq!(parsed.action, Action::MoveCard);
        assert_eq!(parsed.column, Some(0));
        assert_eq!(parsed.destination_column, Some(2));
        assert_eq!(parsed.destination_card, Some(0));
    }

    #[test]
    fn parse_refuses_an_unknown_action_and_a_missing_pane() {
        assert!(parse(r#"{"pane": 1, "action": "explode"}"#).is_err());
        assert!(parse(r#"{"action": "close_card"}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn add_card_selects_the_requested_column_and_reports_a_change() {
        let mut board = board();
        let mut add = command(Action::AddCard);
        add.column = Some(1);
        add.title = Some("second".to_string());
        assert_eq!(apply(&mut board, &add), Ok(true));
        assert_eq!(board.columns[1].cards[0].title, "second");
    }

    #[test]
    fn an_empty_title_is_refused() {
        let mut board = board();
        let mut add = command(Action::AddCard);
        add.title = Some("   ".to_string());
        assert!(apply(&mut board, &add).is_err());
    }

    #[test]
    fn a_bad_index_fails_instead_of_acting_on_the_previous_selection() {
        let mut board = board();
        let mut delete = command(Action::DeleteCard);
        delete.column = Some(0);
        delete.card = Some(9);
        assert!(apply(&mut board, &delete).is_err());
        assert_eq!(board.columns[0].cards.len(), 1, "nothing was deleted");
    }

    #[test]
    fn move_card_without_a_destination_index_goes_to_the_end() {
        let mut board = board();
        let mut moved = command(Action::MoveCard);
        moved.column = Some(0);
        moved.card = Some(0);
        moved.destination_column = Some(1);
        assert_eq!(apply(&mut board, &moved), Ok(true));
        assert!(board.columns[0].cards.is_empty());
        assert_eq!(board.columns[1].cards[0].title, "first");
    }

    #[test]
    fn selecting_changes_nothing_to_write() {
        let mut board = board();
        let mut select = command(Action::SelectCard);
        select.column = Some(0);
        select.card = Some(0);
        assert_eq!(apply(&mut board, &select), Ok(false));
    }
}
