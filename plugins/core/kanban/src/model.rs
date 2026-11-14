//! The stateful board: columns of cards, the selection, the card detail panel and the drag state,
//! with every edit as a method that returns what changed.
//!
//! Moved from `ilium-client/src/board.rs` (`BoardPane`). Persistence is not here: an edit returns
//! `Ok(true)` when the columns changed, and the caller (`io`) writes them through the `fs` host
//! import and restores `snapshot()` when the write is refused. The detail editor buffers are
//! plain text twins of the TextArea widgets the view draws.

use crate::board::{
    checkbox_occurrences, validate_card_title, validate_column_title, BoardCard, BoardColumn,
};

/// How a board is stored on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Storage {
    /// One directory per column holding one `NNN-title.md` file per card.
    Folder { path: String },
    /// One Markdown file: headings are columns, list items are cards.
    MarkdownFile { path: String },
}

impl Storage {
    /// The project-relative path of the board.
    pub fn path(&self) -> &str {
        match self {
            Storage::Folder { path } | Storage::MarkdownFile { path } => path,
        }
    }
}

/// Which field of the open card detail panel owns keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardEditorField {
    Title,
    Body,
}

/// The text of the open detail panel; each change is written to storage at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailEditor {
    pub title: String,
    pub body: String,
    pub focus: CardEditorField,
}

impl DetailEditor {
    fn from_card(card: &BoardCard) -> Self {
        Self {
            title: card.title.clone(),
            body: card.body.clone(),
            focus: CardEditorField::Title,
        }
    }
}

/// The three columns of a new board.
pub fn default_columns() -> Vec<BoardColumn> {
    ["To do", "Doing", "Done"]
        .into_iter()
        .map(|title| BoardColumn {
            title: title.to_string(),
            cards: Vec::new(),
        })
        .collect()
}

/// The state of one board pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub storage: Storage,
    pub columns: Vec<BoardColumn>,
    pub selected_column: usize,
    pub selected_card: Option<usize>,
    /// First column shown when the minimum column width needs a horizontally scrolled viewport.
    pub column_scroll: usize,
    pub drag_source: Option<(usize, usize)>,
    /// Raw insertion line under the pointer while dragging.
    pub drag_target: Option<(usize, usize)>,
    pub is_detail_panel_open: bool,
    pub detail_editor: Option<DetailEditor>,
    /// Advances only after a semantic edit, so navigation is never reported as activity.
    pub content_revision: u64,
}

/// What an edit that can fail returns: `Ok(true)` when the columns changed.
pub type Edit = Result<bool, String>;

impl Board {
    pub fn new(storage: Storage, columns: Vec<BoardColumn>) -> Self {
        Self {
            storage,
            columns,
            selected_column: 0,
            selected_card: None,
            column_scroll: 0,
            drag_source: None,
            drag_target: None,
            is_detail_panel_open: false,
            detail_editor: None,
            content_revision: 0,
        }
    }

    /// The columns and selection to restore when a write of the next state is refused.
    pub fn snapshot(&self) -> Board {
        self.clone()
    }

    /// Replaces the columns with those read from storage; bumps the revision only when they differ.
    pub fn replace_columns(&mut self, columns: Vec<BoardColumn>) {
        if columns != self.columns {
            self.columns = columns;
            self.content_revision += 1;
            self.selected_column = self
                .selected_column
                .min(self.columns.len().saturating_sub(1));
            self.clamp_selected_card();
            self.reconcile_detail_panel();
        }
    }

    fn committed(&mut self) -> Edit {
        self.content_revision += 1;
        Ok(true)
    }

    // ---- selection

    pub fn select_next_column(&mut self) {
        if !self.columns.is_empty() {
            self.selected_column = (self.selected_column + 1) % self.columns.len();
            self.clamp_selected_card();
            self.reconcile_detail_panel();
        }
    }

    pub fn select_previous_column(&mut self) {
        if !self.columns.is_empty() {
            self.selected_column =
                (self.selected_column + self.columns.len() - 1) % self.columns.len();
            self.clamp_selected_card();
            self.reconcile_detail_panel();
        }
    }

    pub fn select_next_card(&mut self) {
        if let Some(column) = self.columns.get(self.selected_column) {
            self.selected_card = match (self.selected_card, column.cards.len()) {
                (_, 0) => None,
                (None, _) => Some(0),
                (Some(index), count) => Some((index + 1) % count),
            };
            self.reconcile_detail_panel();
        }
    }

    pub fn select_previous_card(&mut self) {
        if let Some(column) = self.columns.get(self.selected_column) {
            self.selected_card = match (self.selected_card, column.cards.len()) {
                (_, 0) => None,
                (None, count) => Some(count - 1),
                (Some(0), _) => None,
                (Some(index), _) => Some(index - 1),
            };
            self.reconcile_detail_panel();
        }
    }

    /// Selects a column header, so rename and delete target the column even when it has cards.
    pub fn select_column(&mut self, column_index: usize) {
        if column_index < self.columns.len() {
            self.selected_column = column_index;
            self.selected_card = None;
            self.close_card_details();
        }
    }

    pub fn select_card(&mut self, column_index: usize, card_index: usize) {
        if self
            .columns
            .get(column_index)
            .is_some_and(|column| card_index < column.cards.len())
        {
            self.selected_column = column_index;
            self.selected_card = Some(card_index);
            if self.is_detail_panel_open {
                self.sync_detail_editor();
            }
        }
    }

    pub fn open_card_details(&mut self, column_index: usize, card_index: usize) {
        self.select_card(column_index, card_index);
        if self.selected_column == column_index && self.selected_card == Some(card_index) {
            self.is_detail_panel_open = true;
            self.sync_detail_editor();
        }
    }

    pub fn close_card_details(&mut self) {
        self.is_detail_panel_open = false;
        self.detail_editor = None;
    }

    pub fn selected_card(&self) -> Option<&BoardCard> {
        self.columns
            .get(self.selected_column)?
            .cards
            .get(self.selected_card?)
    }

    fn sync_detail_editor(&mut self) {
        self.detail_editor = self.selected_card().map(DetailEditor::from_card);
        if self.detail_editor.is_none() {
            self.is_detail_panel_open = false;
        }
    }

    fn reconcile_detail_panel(&mut self) {
        if self.selected_card().is_none() {
            self.close_card_details();
        } else if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
    }

    fn clamp_selected_card(&mut self) {
        let Some(selected_card) = self.selected_card else {
            return;
        };
        self.selected_card = self.columns.get(self.selected_column).and_then(|column| {
            (!column.cards.is_empty()).then(|| selected_card.min(column.cards.len() - 1))
        });
    }

    /// Keeps the selected column inside a viewport of `visible_column_count` columns.
    pub fn ensure_selected_column_visible(&mut self, visible_column_count: usize) {
        let visible = visible_column_count.max(1);
        if self.selected_column < self.column_scroll {
            self.column_scroll = self.selected_column;
        } else if self.selected_column >= self.column_scroll + visible {
            self.column_scroll = (self.selected_column + 1).saturating_sub(visible);
        }
        self.column_scroll = self
            .column_scroll
            .min(self.columns.len().saturating_sub(visible));
    }

    /// Scrolls the viewport without changing the selection.
    pub fn set_column_scroll(&mut self, column_scroll: usize, visible_column_count: usize) {
        self.column_scroll = column_scroll.min(
            self.columns
                .len()
                .saturating_sub(visible_column_count.max(1)),
        );
    }

    // ---- edits

    pub fn add_card(&mut self, title: &str) -> Edit {
        let title = validate_card_title(title)?;
        let column = self
            .columns
            .get_mut(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        column.cards.push(BoardCard {
            title,
            body: String::new(),
        });
        self.selected_card = Some(column.cards.len() - 1);
        if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.committed()
    }

    pub fn add_column(&mut self, title: &str) -> Edit {
        let title = validate_column_title(title)?;
        if self.columns.iter().any(|column| column.title == title) {
            return Err("A column with that name already exists".to_string());
        }
        self.columns.push(BoardColumn {
            title,
            cards: Vec::new(),
        });
        self.selected_column = self.columns.len() - 1;
        self.selected_card = None;
        self.close_card_details();
        self.committed()
    }

    pub fn rename_selected_card(&mut self, title: &str) -> Edit {
        let title = validate_card_title(title)?;
        let selected_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let card = self
            .columns
            .get_mut(self.selected_column)
            .and_then(|column| column.cards.get_mut(selected_card))
            .ok_or_else(|| "No card selected".to_string())?;
        if card.title == title {
            return Ok(false);
        }
        card.title = title;
        if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.committed()
    }

    /// The old and new title of a column rename, so the storage can rename the directory.
    pub fn rename_selected_column(
        &mut self,
        title: &str,
    ) -> Result<Option<(String, String)>, String> {
        let title = validate_column_title(title)?;
        let current = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?
            .title
            .clone();
        if current == title {
            return Ok(None);
        }
        if self.columns.iter().any(|column| column.title == title) {
            return Err("A column with that name already exists".to_string());
        }
        self.columns[self.selected_column].title = title.clone();
        self.content_revision += 1;
        Ok(Some((current, title)))
    }

    /// Replaces one card's title and body; omitted fields keep their value.
    pub fn update_card(
        &mut self,
        column_index: usize,
        card_index: usize,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Edit {
        let card = self
            .columns
            .get_mut(column_index)
            .and_then(|column| column.cards.get_mut(card_index))
            .ok_or_else(|| "No matching board card".to_string())?;
        let mut did_change = false;
        if let Some(title) = title {
            let title = validate_card_title(title)?;
            if card.title != title {
                card.title = title;
                did_change = true;
            }
        }
        if let Some(body) = body {
            if card.body != body {
                card.body = body.to_string();
                did_change = true;
            }
        }
        if !did_change {
            return Ok(false);
        }
        self.selected_column = column_index;
        self.selected_card = Some(card_index);
        if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.committed()
    }

    pub fn delete_selected_card(&mut self) -> Edit {
        let selected_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let column = self
            .columns
            .get_mut(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        if selected_card >= column.cards.len() {
            return Err("No card selected".to_string());
        }
        column.cards.remove(selected_card);
        self.selected_card =
            (!column.cards.is_empty()).then(|| selected_card.min(column.cards.len() - 1));
        self.reconcile_detail_panel();
        self.committed()
    }

    /// Removes the selected empty column and returns its title.
    pub fn delete_selected_column(&mut self) -> Result<String, String> {
        if self.columns.len() <= 1 {
            return Err("A board must keep at least one column".to_string());
        }
        let column = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        if !column.cards.is_empty() {
            return Err("Move or remove this column's cards first".to_string());
        }
        let removed = self.columns.remove(self.selected_column).title;
        self.selected_column = self
            .selected_column
            .min(self.columns.len().saturating_sub(1));
        self.selected_card = None;
        self.close_card_details();
        self.content_revision += 1;
        Ok(removed)
    }

    // ---- the detail panel

    pub fn set_detail_editor_focus(&mut self, focus: CardEditorField) {
        if let Some(editor) = self.detail_editor.as_mut() {
            editor.focus = focus;
        }
    }

    pub fn cycle_detail_editor_focus(&mut self) {
        if let Some(editor) = self.detail_editor.as_mut() {
            editor.focus = match editor.focus {
                CardEditorField::Title => CardEditorField::Body,
                CardEditorField::Body => CardEditorField::Title,
            };
        }
    }

    /// Applies the typed text of one detail field to the card and the editor buffer. An empty or
    /// multi-line title is kept in the buffer but not written, so typing never stores an invalid
    /// card: the result is `Ok(false)` until the title is valid again.
    pub fn set_detail_text(&mut self, field: CardEditorField, text: &str) -> Edit {
        let Some(editor) = self.detail_editor.as_mut() else {
            return Ok(false);
        };
        let (Some(card_index), column_index) = (self.selected_card, self.selected_column) else {
            return Ok(false);
        };
        match field {
            CardEditorField::Title => {
                editor.title = text.to_string();
                match validate_card_title(text) {
                    Ok(valid) => {
                        let card = self
                            .columns
                            .get_mut(column_index)
                            .and_then(|column| column.cards.get_mut(card_index))
                            .ok_or_else(|| "No card selected".to_string())?;
                        if card.title == valid {
                            return Ok(false);
                        }
                        card.title = valid;
                        self.committed()
                    }
                    Err(_) => Ok(false),
                }
            }
            CardEditorField::Body => {
                editor.body = text.to_string();
                let card = self
                    .columns
                    .get_mut(column_index)
                    .and_then(|column| column.cards.get_mut(card_index))
                    .ok_or_else(|| "No card selected".to_string())?;
                if card.body == text {
                    return Ok(false);
                }
                card.body = text.to_string();
                self.committed()
            }
        }
    }

    /// Flips one task checkbox of a card title.
    pub fn toggle_card_checkbox(
        &mut self,
        column_index: usize,
        card_index: usize,
        checkbox_index: usize,
    ) -> Edit {
        let card = self
            .columns
            .get_mut(column_index)
            .and_then(|column| column.cards.get_mut(card_index))
            .ok_or_else(|| "Card changed before its checkbox could be toggled".to_string())?;
        let (byte_index, is_checked) = checkbox_occurrences(&card.title)
            .get(checkbox_index)
            .copied()
            .ok_or_else(|| "Checkbox changed before it could be toggled".to_string())?;
        card.title.replace_range(
            byte_index + 1..byte_index + 2,
            if is_checked { " " } else { "x" },
        );
        if self.is_detail_panel_open
            && self.selected_column == column_index
            && self.selected_card == Some(card_index)
        {
            self.sync_detail_editor();
        }
        self.committed()
    }

    // ---- moving cards

    /// Moves the selected card one column left (-1) or right (1), to the end of that column.
    pub fn move_selected_card(&mut self, direction: isize) -> Edit {
        let source_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let destination = (self.selected_column as isize + direction)
            .clamp(0, self.columns.len().saturating_sub(1) as isize)
            as usize;
        if destination == self.selected_column {
            return Ok(false);
        }
        let destination_card = self.columns[destination].cards.len();
        self.move_card(
            self.selected_column,
            source_card,
            destination,
            destination_card,
        )
    }

    /// Reorders the selected card within its column.
    pub fn move_selected_card_vertically(&mut self, direction: isize) -> Edit {
        let source_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let card_count = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?
            .cards
            .len();
        let destination_card = (source_card as isize + direction)
            .clamp(0, card_count.saturating_sub(1) as isize)
            as usize;
        self.move_card(
            self.selected_column,
            source_card,
            self.selected_column,
            destination_card,
        )
    }

    /// Moves one card to an exact index; keyboard moves and both kinds of drop use it.
    pub fn move_card(
        &mut self,
        source_column: usize,
        source_card: usize,
        destination_column: usize,
        destination_card: usize,
    ) -> Edit {
        if source_column == destination_column && source_card == destination_card {
            self.select_card(source_column, source_card);
            return Ok(false);
        }
        if source_column >= self.columns.len() || destination_column >= self.columns.len() {
            return Err("Board column changed before the card could be moved".to_string());
        }
        if source_card >= self.columns[source_column].cards.len() {
            return Err("Card changed before it could be moved".to_string());
        }
        let card = self.columns[source_column].cards.remove(source_card);
        let destination_card = destination_card.min(self.columns[destination_column].cards.len());
        self.columns[destination_column]
            .cards
            .insert(destination_card, card);
        self.selected_column = destination_column;
        self.selected_card = Some(destination_card);
        if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.committed()
    }

    /// Converts a visual insertion line of the pre-removal card list into the exact index after
    /// the dragged card has been removed, then moves it.
    pub fn move_dragged_card(
        &mut self,
        source_column: usize,
        source_card: usize,
        destination_column: usize,
        insertion_index: usize,
    ) -> Edit {
        let destination_card =
            if source_column == destination_column && insertion_index > source_card {
                insertion_index - 1
            } else {
                insertion_index
            };
        self.move_card(
            source_column,
            source_card,
            destination_column,
            destination_card,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut board = Board::new(
            Storage::MarkdownFile {
                path: "plan.md".to_string(),
            },
            default_columns(),
        );
        for title in ["one", "two", "three"] {
            board.add_card(title).unwrap();
        }
        board
    }

    fn titles(board: &Board, column: usize) -> Vec<String> {
        board.columns[column]
            .cards
            .iter()
            .map(|card| card.title.clone())
            .collect()
    }

    #[test]
    fn a_new_board_has_three_columns_and_adding_selects_the_new_card() {
        let board = board();
        assert_eq!(board.columns.len(), 3);
        assert_eq!(titles(&board, 0), ["one", "two", "three"]);
        assert_eq!(board.selected_card, Some(2));
        assert_eq!(board.content_revision, 3);
    }

    #[test]
    fn card_titles_must_be_one_non_empty_line() {
        let mut board = board();
        assert!(board.add_card("  ").is_err());
        assert!(board.add_card("a\nb").is_err());
        assert_eq!(board.content_revision, 3);
    }

    #[test]
    fn cards_reorder_vertically_and_to_an_exact_cross_column_position() {
        let mut board = board();
        board.select_card(0, 0);
        board.move_selected_card_vertically(1).unwrap();
        assert_eq!(titles(&board, 0), ["two", "one", "three"]);
        board.move_selected_card(1).unwrap();
        assert_eq!(titles(&board, 1), ["one"]);
        board.move_card(0, 1, 1, 0).unwrap();
        assert_eq!(titles(&board, 1), ["three", "one"]);
        assert_eq!(board.selected_column, 1);
        assert_eq!(board.selected_card, Some(0));
    }

    #[test]
    fn visual_same_column_insertion_is_converted_after_source_removal() {
        let mut board = board();
        board.move_dragged_card(0, 0, 0, 2).unwrap();
        assert_eq!(titles(&board, 0), ["two", "one", "three"]);
        board.move_dragged_card(0, 2, 0, 0).unwrap();
        assert_eq!(titles(&board, 0), ["three", "two", "one"]);
        assert_eq!(board.move_dragged_card(0, 1, 0, 1), Ok(false));
    }

    #[test]
    fn a_move_with_a_stale_index_is_refused_without_changing_the_board() {
        let mut board = board();
        let before = board.clone();
        assert!(board.move_card(0, 9, 1, 0).is_err());
        assert!(board.move_card(7, 0, 1, 0).is_err());
        assert_eq!(board, before);
    }

    #[test]
    fn populated_column_header_can_be_selected_and_renamed() {
        let mut board = board();
        board.select_column(0);
        assert_eq!(board.selected_card, None);
        let renamed = board.rename_selected_column("Backlog").unwrap();
        assert_eq!(renamed, Some(("To do".to_string(), "Backlog".to_string())));
        assert_eq!(board.columns[0].title, "Backlog");
        assert!(board.rename_selected_column("Doing").is_err());
        assert!(board.rename_selected_column("a/b").is_err());
        assert_eq!(board.rename_selected_column("Backlog"), Ok(None));
    }

    #[test]
    fn deleting_a_column_needs_it_empty_and_keeps_at_least_one() {
        let mut board = board();
        board.select_column(0);
        assert!(board.delete_selected_column().is_err());
        board.select_column(1);
        assert_eq!(board.delete_selected_column().unwrap(), "Doing");
        board.select_column(1);
        assert_eq!(board.delete_selected_column().unwrap(), "Done");
        assert_eq!(board.columns.len(), 1);
        assert!(board.delete_selected_column().is_err());
    }

    #[test]
    fn duplicate_column_names_are_refused() {
        let mut board = board();
        assert!(board.add_column("Doing").is_err());
        assert!(board.add_column("Review").unwrap());
        assert_eq!(board.selected_column, 3);
    }

    #[test]
    fn the_detail_panel_follows_the_selection_and_closes_without_a_card() {
        let mut board = board();
        board.open_card_details(0, 1);
        assert_eq!(board.detail_editor.as_ref().unwrap().title, "two");
        board.select_card(0, 2);
        assert_eq!(board.detail_editor.as_ref().unwrap().title, "three");
        board.select_column(1);
        assert!(!board.is_detail_panel_open);
        assert!(board.detail_editor.is_none());
    }

    #[test]
    fn detail_edits_change_the_card_and_an_invalid_title_is_not_written() {
        let mut board = board();
        board.open_card_details(0, 0);
        assert_eq!(
            board.set_detail_text(CardEditorField::Title, "uno"),
            Ok(true)
        );
        assert_eq!(board.set_detail_text(CardEditorField::Title, ""), Ok(false));
        assert_eq!(board.columns[0].cards[0].title, "uno");
        assert_eq!(board.detail_editor.as_ref().unwrap().title, "");
        assert_eq!(
            board.set_detail_text(CardEditorField::Body, "a\nb"),
            Ok(true)
        );
        assert_eq!(board.columns[0].cards[0].body, "a\nb");
        assert_eq!(
            board.set_detail_text(CardEditorField::Body, "a\nb"),
            Ok(false)
        );
    }

    #[test]
    fn task_checkbox_toggle_flips_the_marker_only() {
        let mut board = board();
        board
            .update_card(0, 0, Some("é [ ] ship [x] done"), None)
            .unwrap();
        board.toggle_card_checkbox(0, 0, 0).unwrap();
        assert_eq!(board.columns[0].cards[0].title, "é [x] ship [x] done");
        board.toggle_card_checkbox(0, 0, 1).unwrap();
        assert_eq!(board.columns[0].cards[0].title, "é [x] ship [ ] done");
        assert!(board.toggle_card_checkbox(0, 0, 5).is_err());
        assert!(board.toggle_card_checkbox(0, 9, 0).is_err());
    }

    #[test]
    fn update_card_keeps_omitted_fields_and_reports_no_change() {
        let mut board = board();
        assert_eq!(board.update_card(0, 0, None, Some("body")), Ok(true));
        assert_eq!(board.columns[0].cards[0].title, "one");
        assert_eq!(board.update_card(0, 0, None, Some("body")), Ok(false));
        assert!(board.update_card(0, 7, None, None).is_err());
    }

    #[test]
    fn navigation_wraps_and_the_viewport_follows_the_selected_column() {
        let mut board = board();
        board.select_column(0);
        board.select_previous_column();
        assert_eq!(board.selected_column, 2);
        board.select_next_column();
        assert_eq!(board.selected_column, 0);
        board.select_next_card();
        assert_eq!(board.selected_card, Some(0));
        board.select_previous_card();
        assert_eq!(board.selected_card, None);
        board.selected_column = 2;
        board.ensure_selected_column_visible(2);
        assert_eq!(board.column_scroll, 1);
        board.set_column_scroll(9, 2);
        assert_eq!(board.column_scroll, 1);
    }

    #[test]
    fn replacing_columns_bumps_the_revision_only_for_a_real_change() {
        let mut board = board();
        let revision = board.content_revision;
        board.replace_columns(board.columns.clone());
        assert_eq!(board.content_revision, revision);
        board.replace_columns(default_columns());
        assert_eq!(board.content_revision, revision + 1);
        assert_eq!(board.selected_card, None);
    }
}
