//! Local kanban document model and its two user-owned storage adapters.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ilium_core::BoardStorage;
use ratatui_textarea::{CursorMove, Input, TextArea};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardCard {
    pub title: String,
    pub body: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumn {
    pub title: String,
    pub cards: Vec<BoardCard>,
}

/// Which immediately-persisted field owns keyboard input in the open card
/// details panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardEditorField {
    Title,
    Body,
}

/// Editable buffers for the selected card. They remain presentation state;
/// [`BoardPane`] copies their contents into the storage-neutral `BoardCard`
/// and saves through the active adapter after every modifying input event.
#[derive(Debug, Clone)]
pub struct CardDetailEditor {
    pub title: TextArea<'static>,
    pub body: TextArea<'static>,
    pub focus: CardEditorField,
}

impl CardDetailEditor {
    fn from_card(card: &BoardCard) -> Self {
        let mut title = TextArea::from([card.title.clone()]);
        title.move_cursor(CursorMove::End);
        let body_lines = if card.body.is_empty() {
            vec![String::new()]
        } else {
            card.body.lines().map(str::to_string).collect()
        };
        let body = TextArea::from(body_lines);
        Self {
            title,
            body,
            focus: CardEditorField::Title,
        }
    }

    fn title_text(&self) -> String {
        self.title.lines().join("")
    }

    fn body_text(&self) -> String {
        self.body.lines().join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct BoardPane {
    pub storage: BoardStorage,
    pub columns: Vec<BoardColumn>,
    pub selected_column: usize,
    pub selected_card: Option<usize>,
    /// First board column shown when the configured minimum width requires
    /// a horizontally scrollable viewport.
    pub column_scroll: usize,
    pub drag_source: Option<(usize, usize)>,
    /// Raw insertion line under the pointer while dragging. The renderer
    /// uses it for feedback; release converts it to a post-removal index.
    pub drag_target: Option<(usize, usize)>,
    /// Whether the selected card's complete body is visible beside the board.
    pub is_detail_panel_open: bool,
    /// Live editor for the selected card while its detail panel is open.
    pub detail_editor: Option<CardDetailEditor>,
    markdown_revision: Option<String>,
}

impl BoardPane {
    pub fn load(storage: BoardStorage) -> Result<Self, String> {
        let (columns, markdown_revision) = match &storage {
            BoardStorage::Folder { path } => (load_folder_board(path)?, None),
            BoardStorage::MarkdownFile { path } => {
                let (columns, source) = load_markdown_board(path)?;
                (columns, source)
            }
        };
        Ok(Self {
            storage,
            columns,
            selected_column: 0,
            selected_card: None,
            column_scroll: 0,
            drag_source: None,
            drag_target: None,
            is_detail_panel_open: false,
            detail_editor: None,
            markdown_revision,
        })
    }
    pub fn create(storage: BoardStorage) -> Result<Self, String> {
        let board = Self {
            storage,
            columns: vec![
                BoardColumn {
                    title: "To do".to_string(),
                    cards: Vec::new(),
                },
                BoardColumn {
                    title: "Doing".to_string(),
                    cards: Vec::new(),
                },
                BoardColumn {
                    title: "Done".to_string(),
                    cards: Vec::new(),
                },
            ],
            selected_column: 0,
            selected_card: None,
            column_scroll: 0,
            drag_source: None,
            drag_target: None,
            is_detail_panel_open: false,
            detail_editor: None,
            markdown_revision: None,
        };
        let mut board = board;
        board.save()?;
        Ok(board)
    }
    pub fn save(&mut self) -> Result<(), String> {
        match &self.storage {
            BoardStorage::Folder { path } => save_folder_board(path, &self.columns),
            BoardStorage::MarkdownFile { path } => {
                let source = serialize_markdown_board(&self.columns);
                save_markdown_board(path, &source, self.markdown_revision.as_deref())?;
                self.markdown_revision = Some(source);
                Ok(())
            }
        }
    }

    /// Persists a completed in-memory edit, restoring the exact prior board
    /// when the storage adapter rejects it (for example after an external
    /// Markdown edit). A failed save must never leave the UI showing data that
    /// was not actually committed to disk.
    fn persist_or_restore(&mut self, previous: Self) -> Result<(), String> {
        if let Err(error) = self.save() {
            *self = previous;
            return Err(error);
        }
        Ok(())
    }

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

    /// Selects a column header, making rename/delete actions unambiguously
    /// target the column even when it contains cards.
    pub fn select_column(&mut self, column_index: usize) {
        if column_index < self.columns.len() {
            self.selected_column = column_index;
            self.selected_card = None;
            self.close_card_details();
        }
    }

    /// Selects one card after validating both indices.
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

    /// Selects one card and opens its complete details in the side panel.
    pub fn open_card_details(&mut self, column_index: usize, card_index: usize) {
        self.select_card(column_index, card_index);
        if self.selected_column == column_index && self.selected_card == Some(card_index) {
            self.is_detail_panel_open = true;
            self.sync_detail_editor();
        }
    }

    /// Closes the presentation-only detail panel without changing selection.
    pub fn close_card_details(&mut self) {
        self.is_detail_panel_open = false;
        self.detail_editor = None;
    }

    /// The card currently owning the detail panel, if the selection is valid.
    pub fn selected_card(&self) -> Option<&BoardCard> {
        let selected_card = self.selected_card?;
        self.columns
            .get(self.selected_column)?
            .cards
            .get(selected_card)
    }

    /// Rebuilds the detail editor from the selected persisted card. Selection
    /// changes and checkbox clicks use this so stale buffers can never edit a
    /// different card than the one visible in the panel.
    fn sync_detail_editor(&mut self) {
        self.detail_editor = self.selected_card().map(CardDetailEditor::from_card);
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

    /// Keeps the selected column inside a fixed-count horizontal viewport
    /// after keyboard navigation or a detail-panel width change.
    pub fn ensure_selected_column_visible(&mut self, visible_column_count: usize) {
        let visible_column_count = visible_column_count.max(1);
        if self.selected_column < self.column_scroll {
            self.column_scroll = self.selected_column;
        } else if self.selected_column >= self.column_scroll + visible_column_count {
            self.column_scroll = self
                .selected_column
                .saturating_add(1)
                .saturating_sub(visible_column_count);
        }
        let max_scroll = self.columns.len().saturating_sub(visible_column_count);
        self.column_scroll = self.column_scroll.min(max_scroll);
    }

    /// Moves the horizontal viewport while keeping it within the last full
    /// page of columns. This intentionally does not change card selection.
    pub fn set_column_scroll(&mut self, column_scroll: usize, visible_column_count: usize) {
        self.column_scroll = column_scroll.min(
            self.columns
                .len()
                .saturating_sub(visible_column_count.max(1)),
        );
    }

    pub fn add_card(&mut self, title: String) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A card needs a title".to_string());
        }
        let previous = self.clone();
        let column = self
            .columns
            .get_mut(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        column.cards.push(BoardCard {
            title: title.to_string(),
            body: String::new(),
        });
        self.selected_card = Some(column.cards.len() - 1);
        self.persist_or_restore(previous)
    }
    pub fn add_column(&mut self, title: String) -> Result<(), String> {
        // A new column's title becomes a directory name under Folder storage
        // (see save_folder_board), so it must pass the same checks
        // rename_selected_column applies: rejecting separators/traversal
        // segments and duplicate names before the column ever enters
        // self.columns. Validating after the push would let an invalid or
        // duplicate title corrupt in-memory state whenever save() then
        // fails or silently merges two columns on disk.
        let title = validate_column_title(title)?;
        if self.columns.iter().any(|column| column.title == title) {
            return Err("A column with that name already exists".to_string());
        }
        let previous = self.clone();
        self.columns.push(BoardColumn {
            title,
            cards: Vec::new(),
        });
        self.selected_column = self.columns.len() - 1;
        self.selected_card = None;
        self.close_card_details();
        self.persist_or_restore(previous)
    }

    pub fn rename_selected_card(&mut self, title: String) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A card needs a title".to_string());
        }
        let selected_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let previous = self.clone();
        let card = self
            .columns
            .get_mut(self.selected_column)
            .and_then(|column| column.cards.get_mut(selected_card))
            .ok_or_else(|| "No card selected".to_string())?;
        card.title = title.to_string();
        self.persist_or_restore(previous)
    }

    pub fn rename_selected_column(&mut self, title: String) -> Result<(), String> {
        let title = validate_column_title(title)?;
        let current_title = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?
            .title
            .clone();
        if current_title == title {
            return Ok(());
        }
        if self.columns.iter().any(|column| column.title == title) {
            return Err("A column with that name already exists".to_string());
        }
        let previous = self.clone();
        if let BoardStorage::Folder { path } = &self.storage {
            let source = path.join(&current_title);
            let destination = path.join(&title);
            if destination.exists() {
                return Err(format!("{} already exists", destination.display()));
            }
            fs::rename(&source, &destination)
                .map_err(|error| format!("Could not rename {}: {error}", source.display()))?;
        }
        self.columns[self.selected_column].title = title;
        self.persist_or_restore(previous)
    }

    /// Replaces one card's semantic fields through the board's existing
    /// atomic persistence boundary. Omitted fields preserve their current
    /// value, making partial tool calls explicit and retry-safe.
    pub fn update_card(
        &mut self,
        column_index: usize,
        card_index: usize,
        title: Option<String>,
        body: Option<String>,
    ) -> Result<(), String> {
        let previous = self.clone();
        let card = self
            .columns
            .get_mut(column_index)
            .and_then(|column| column.cards.get_mut(card_index))
            .ok_or_else(|| "No matching board card".to_owned())?;
        if let Some(title) = title {
            let title = title.trim();
            if title.is_empty() {
                return Err("A card needs a title".to_owned());
            }
            card.title = title.to_owned();
        }
        if let Some(body) = body {
            card.body = body;
        }
        self.selected_column = column_index;
        self.selected_card = Some(card_index);
        if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.persist_or_restore(previous)
    }

    pub fn delete_selected_card(&mut self) -> Result<(), String> {
        let selected_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let previous = self.clone();
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
        if self.selected_card.is_none() {
            self.close_card_details();
        } else if self.is_detail_panel_open {
            self.sync_detail_editor();
        }
        self.persist_or_restore(previous)
    }

    pub fn delete_selected_column(&mut self) -> Result<(), String> {
        if self.columns.len() <= 1 {
            return Err("A board must keep at least one column".to_string());
        }
        let previous = self.clone();
        let column = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        if !column.cards.is_empty() {
            return Err("Move or remove this column's cards first".to_string());
        }
        if let BoardStorage::Folder { path } = &self.storage {
            let column_path = path.join(&column.title);
            if column_path.exists() {
                fs::remove_dir(&column_path).map_err(|error| {
                    format!(
                        "Could not remove {} (it may contain non-card files): {error}",
                        column_path.display()
                    )
                })?;
            }
        }
        self.columns.remove(self.selected_column);
        self.selected_column = self
            .selected_column
            .min(self.columns.len().saturating_sub(1));
        self.selected_card = None;
        self.close_card_details();
        self.persist_or_restore(previous)
    }

    /// Gives keyboard input to one detail field without changing the selected
    /// card or writing to storage.
    pub fn set_detail_editor_focus(&mut self, focus: CardEditorField) {
        if let Some(editor) = self.detail_editor.as_mut() {
            if editor.focus != focus && focus == CardEditorField::Body {
                editor.body.move_cursor(CursorMove::Bottom);
                editor.body.move_cursor(CursorMove::End);
            }
            editor.focus = focus;
        }
    }

    /// Moves focus between the title and body fields in either direction.
    pub fn cycle_detail_editor_focus(&mut self, _reverse: bool) {
        let Some(editor) = self.detail_editor.as_mut() else {
            return;
        };
        let next_focus = match editor.focus {
            CardEditorField::Title => CardEditorField::Body,
            CardEditorField::Body => CardEditorField::Title,
        };
        if editor.focus != next_focus && next_focus == CardEditorField::Body {
            editor.body.move_cursor(CursorMove::Bottom);
            editor.body.move_cursor(CursorMove::End);
        }
        editor.focus = next_focus;
    }

    /// Applies one editing input and commits the resulting card immediately.
    /// Storage rejection restores the edited field's prior buffer and the
    /// touched card's prior title/body -- never the whole board -- so the
    /// panel still never displays text that was not written to disk, but a
    /// keystroke's rollback snapshot stays bounded by that one card instead
    /// of growing with the size of the whole document (all columns, every
    /// other card, and the full backing Markdown source for
    /// `MarkdownFile`-backed boards).
    pub fn input_detail_editor(&mut self, input: Input) -> Result<bool, String> {
        let Some(editor) = self.detail_editor.as_mut() else {
            return Ok(false);
        };
        let focus = editor.focus;
        // Snapshot only the one field about to receive input -- not the
        // other field, not the rest of the editor, not the board -- so a
        // rejected edit can be reverted exactly. Must happen before
        // `.input()` mutates the buffer in place, since ratatui-textarea's
        // own undo history can hold more than one entry per `.input()` call
        // (e.g. typing over an active selection deletes-then-inserts), so a
        // single `.undo()` call cannot be relied on to fully revert it.
        let field_before = match focus {
            CardEditorField::Title => editor.title.clone(),
            CardEditorField::Body => editor.body.clone(),
        };
        let modified = match focus {
            CardEditorField::Title => editor.title.input(input),
            CardEditorField::Body => editor.body.input(input),
        };
        if !modified {
            return Ok(false);
        }

        let title = editor.title_text();
        if title.trim().is_empty() {
            match focus {
                CardEditorField::Title => editor.title = field_before,
                CardEditorField::Body => editor.body = field_before,
            }
            return Err("A card title cannot be empty".to_string());
        }
        let body = editor.body_text();
        let selected_column = self.selected_column;
        let selected_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let card = self
            .columns
            .get_mut(selected_column)
            .and_then(|column| column.cards.get_mut(selected_card))
            .ok_or_else(|| "No card selected".to_string())?;
        // Rollback scope for a failed save: this one card's prior title and
        // body. Nothing else on the board changes as part of this call, so
        // nothing else needs restoring.
        let card_before = card.clone();
        card.title = title;
        card.body = body;

        if let Err(error) = self.save() {
            if let Some(card) = self
                .columns
                .get_mut(selected_column)
                .and_then(|column| column.cards.get_mut(selected_card))
            {
                *card = card_before;
            }
            if let Some(editor) = self.detail_editor.as_mut() {
                match focus {
                    CardEditorField::Title => editor.title = field_before,
                    CardEditorField::Body => editor.body = field_before,
                }
            }
            return Err(error);
        }
        Ok(true)
    }

    /// Scrolls the body editor while preserving its contents and focus.
    pub fn scroll_detail_body(&mut self, rows: i16) {
        if let Some(editor) = self.detail_editor.as_mut() {
            editor.focus = CardEditorField::Body;
            editor.body.scroll((rows, 0));
        }
    }

    /// Flips one visible task checkbox in a card title and writes the single
    /// character change through the active storage adapter immediately.
    pub fn toggle_card_checkbox(
        &mut self,
        column_index: usize,
        card_index: usize,
        checkbox_index: usize,
    ) -> Result<(), String> {
        let previous = self.clone();
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
        self.persist_or_restore(previous)
    }

    pub fn move_selected_card(&mut self, direction: isize) -> Result<(), String> {
        let source_card = self
            .selected_card
            .ok_or_else(|| "No card selected".to_string())?;
        let destination = (self.selected_column as isize + direction)
            .clamp(0, self.columns.len().saturating_sub(1) as isize)
            as usize;
        if destination == self.selected_column {
            return Ok(());
        }
        let destination_card = self.columns[destination].cards.len();
        self.move_card(
            self.selected_column,
            source_card,
            destination,
            destination_card,
        )
    }

    /// Reorders the selected card within its current column.
    pub fn move_selected_card_vertically(&mut self, direction: isize) -> Result<(), String> {
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

    /// Moves one validated card to an exact index, covering keyboard movement
    /// and both same-column and cross-column drag/drop through one persistence
    /// boundary.
    pub fn move_card(
        &mut self,
        source_column: usize,
        source_card: usize,
        destination_column: usize,
        destination_card: usize,
    ) -> Result<(), String> {
        if source_column == destination_column && source_card == destination_card {
            self.select_card(source_column, source_card);
            return Ok(());
        }
        if source_column >= self.columns.len() || destination_column >= self.columns.len() {
            return Err("Board column changed before the card could be moved".to_string());
        }
        if source_card >= self.columns[source_column].cards.len() {
            return Err("Card changed before it could be moved".to_string());
        }

        let previous = self.clone();
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
        self.persist_or_restore(previous)
    }

    /// Converts a visual insertion line from the pre-removal card list into
    /// the exact destination index after the dragged source has been removed.
    pub fn move_dragged_card(
        &mut self,
        source_column: usize,
        source_card: usize,
        destination_column: usize,
        insertion_index: usize,
    ) -> Result<(), String> {
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

/// Byte positions and checked states of every literal Markdown task marker
/// in one card title. ASCII marker bytes cannot occur inside a UTF-8
/// continuation byte, so byte-window matching keeps replacement exact even
/// when arbitrary Unicode precedes the checkbox.
pub fn checkbox_occurrences(title: &str) -> Vec<(usize, bool)> {
    title
        .as_bytes()
        .windows(3)
        .enumerate()
        .filter_map(|(index, marker)| match marker {
            b"[ ]" => Some((index, false)),
            b"[x]" | b"[X]" => Some((index, true)),
            _ => None,
        })
        .collect()
}

/// Plain-text rendering of a board's columns/cards, used as one pane's
/// content extract when gathering context for the whole-tree restructure
/// LLM call (`crate::restructure`). Unlike `serialize_markdown_board`, this
/// never needs to round-trip back into a board and is never written to
/// disk -- session_naming/terminal_naming have no board-equivalent reader,
/// so this is the one genuinely new content source that feature needs.
pub fn board_text_extract(columns: &[BoardColumn]) -> String {
    let mut extract = String::new();
    for column in columns {
        extract.push_str("## ");
        extract.push_str(&column.title);
        extract.push('\n');
        for card in &column.cards {
            extract.push_str("- ");
            extract.push_str(&card.title);
            let body = card.body.trim();
            if !body.is_empty() {
                extract.push_str(": ");
                extract.push_str(body);
            }
            extract.push('\n');
        }
    }
    extract
}

fn load_folder_board(path: &Path) -> Result<Vec<BoardColumn>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut columns = Vec::new();
    for entry in read_sorted(path)? {
        if !entry.is_dir() {
            continue;
        }
        let mut cards = Vec::new();
        for card_path in read_sorted(&entry)? {
            if card_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                let source = fs::read_to_string(&card_path)
                    .map_err(|error| format!("Could not read {}: {error}", card_path.display()))?;
                let fallback_title = card_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace('-', " ");
                let (title, body) = parse_folder_card(&source, fallback_title);
                cards.push(BoardCard { title, body });
            }
        }
        columns.push(BoardColumn {
            title: entry
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            cards,
        });
    }
    Ok(columns)
}

/// Normalizes a folder-backed Markdown file into the same separate title/body
/// contract used by the single-file adapter. A leading H1 is storage syntax,
/// not duplicate body content in the detail editor.
fn parse_folder_card(source: &str, fallback_title: String) -> (String, String) {
    let mut lines = source.lines();
    let Some(first_line) = lines.next() else {
        return (fallback_title, String::new());
    };
    let Some(title) = first_line.strip_prefix("# ").map(str::trim) else {
        return (fallback_title, source.trim_end().to_string());
    };
    let mut body_lines = lines.collect::<Vec<_>>();
    if body_lines.first().is_some_and(|line| line.is_empty()) {
        body_lines.remove(0);
    }
    (
        title.to_string(),
        body_lines.join("\n").trim_end().to_string(),
    )
}
fn load_markdown_board(path: &Path) -> Result<(Vec<BoardColumn>, Option<String>), String> {
    if !path.exists() {
        return Ok((Vec::new(), None));
    }
    let source = fs::read_to_string(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    Ok((parse_markdown_board(&source), Some(source)))
}

/// Converts an existing Markdown task document into board columns without
/// rewriting it. Every ATX heading that owns content becomes a column, while
/// a leading level-one document title with no cards is ignored. Supporting
/// all unordered-list markers is important for ordinary todo files, which
/// commonly use `* [ ]` rather than the board writer's canonical `- ` form.
fn parse_markdown_board(source: &str) -> Vec<BoardColumn> {
    let mut parsed_columns: Vec<(usize, BoardColumn)> = Vec::new();
    let mut current_column: Option<(usize, BoardColumn)> = None;
    let mut is_in_fenced_code_block = false;

    for line in source.lines() {
        let trimmed_line = line.trim_start();
        if trimmed_line.starts_with("```") || trimmed_line.starts_with("~~~") {
            append_markdown_card_body_line(&mut current_column, trimmed_line);
            is_in_fenced_code_block = !is_in_fenced_code_block;
            continue;
        }
        if is_in_fenced_code_block {
            append_markdown_card_body_line(&mut current_column, trimmed_line);
            continue;
        }

        if let Some((level, title)) = markdown_heading(trimmed_line) {
            if let Some(column) = current_column.take() {
                parsed_columns.push(column);
            }
            current_column = Some((
                level,
                BoardColumn {
                    title: title.to_string(),
                    cards: Vec::new(),
                },
            ));
            continue;
        }

        let Some(title) = markdown_list_item(trimmed_line) else {
            if line.len() != trimmed_line.len() {
                append_markdown_card_body_line(&mut current_column, trimmed_line);
            } else if trimmed_line.is_empty() {
                append_markdown_card_body_line(&mut current_column, "");
            }
            continue;
        };
        let column = current_column.get_or_insert_with(|| {
            (
                0,
                BoardColumn {
                    title: "Items".to_string(),
                    cards: Vec::new(),
                },
            )
        });
        column.1.cards.push(BoardCard {
            title: title.to_string(),
            body: String::new(),
        });
    }

    if let Some(column) = current_column {
        parsed_columns.push(column);
    }
    if parsed_columns.len() > 1 && parsed_columns[0].0 == 1 && parsed_columns[0].1.cards.is_empty()
    {
        parsed_columns.remove(0);
    }
    parsed_columns
        .into_iter()
        .map(|(_, mut column)| {
            for card in &mut column.cards {
                card.body = card.body.trim_end().to_string();
            }
            column
        })
        .collect()
}

/// Appends one indented continuation line to the most recent Markdown card.
/// Top-level prose remains outside the board, while canonical two-space card
/// bodies round-trip through the detail panel.
fn append_markdown_card_body_line(current_column: &mut Option<(usize, BoardColumn)>, line: &str) {
    let Some(card) = current_column
        .as_mut()
        .and_then(|(_, column)| column.cards.last_mut())
    else {
        return;
    };
    if !card.body.is_empty() {
        card.body.push('\n');
    }
    card.body.push_str(line);
}

/// Parses a non-indented ATX heading and rejects empty marker-only headings.
fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let marker_count = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&marker_count) {
        return None;
    }
    let title = line.get(marker_count..)?.strip_prefix(' ')?.trim();
    (!title.is_empty()).then_some((marker_count, title.trim_end_matches('#').trim_end()))
}

/// Returns the visible text of an unordered Markdown list item. Indentation
/// is accepted so nested task lists still contribute cards; fenced examples
/// are filtered by the caller before reaching this parser.
fn markdown_list_item(line: &str) -> Option<&str> {
    let title = ["- ", "* ", "+ "]
        .into_iter()
        .find_map(|marker| line.strip_prefix(marker))?
        .trim();
    (!title.is_empty()).then_some(title)
}
fn save_folder_board(path: &Path, columns: &[BoardColumn]) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    for column in columns {
        // A column title is a directory name, not a generated identifier:
        // preserving it lets a manually maintained folder board round-trip
        // its visible column names. Reject separators and traversal rather
        // than silently redirecting a board write outside its root.
        validate_column_title(&column.title)?;
        let column_path = path.join(&column.title);
        fs::create_dir_all(&column_path)
            .map_err(|error| format!("Could not create {}: {error}", column_path.display()))?;
        // Stage every desired Markdown card before replacing any live file.
        // Immediate per-keystroke editing must not expose a half-rewritten
        // column if one later write fails.
        let mut staged_cards = Vec::new();
        let mut desired_paths = HashSet::new();
        for (index, card) in column.cards.iter().enumerate() {
            let card_path =
                column_path.join(format!("{:03}-{}.md", index + 1, safe_name(&card.title)));
            let body = serialize_folder_card(card);
            let temporary_path = column_path.join(format!(
                ".{}.ilium-tmp-{}-{index}",
                card_path.file_name().unwrap_or_default().to_string_lossy(),
                std::process::id()
            ));
            let _ = fs::remove_file(&temporary_path);
            let stage_result = (|| -> Result<(), String> {
                let mut file = fs::File::create(&temporary_path)
                    .map_err(|error| format!("Could not stage {}: {error}", card_path.display()))?;
                file.write_all(body.as_bytes())
                    .map_err(|error| format!("Could not stage {}: {error}", card_path.display()))?;
                file.sync_all()
                    .map_err(|error| format!("Could not sync {}: {error}", card_path.display()))
            })();
            if let Err(error) = stage_result {
                for (staged_path, _) in &staged_cards {
                    let _ = fs::remove_file(staged_path);
                }
                let _ = fs::remove_file(&temporary_path);
                return Err(error);
            }
            desired_paths.insert(card_path.clone());
            staged_cards.push((temporary_path, card_path));
        }
        for (temporary_path, card_path) in &staged_cards {
            if let Err(error) = fs::rename(temporary_path, card_path) {
                for (remaining_temporary_path, _) in &staged_cards {
                    let _ = fs::remove_file(remaining_temporary_path);
                }
                return Err(format!("Could not update {}: {error}", card_path.display()));
            }
        }
        // Only obsolete Markdown siblings are removed after every replacement
        // succeeded; attachments and nested directories remain user-owned.
        for existing_card in read_sorted(&column_path)? {
            if existing_card
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                && !desired_paths.contains(&existing_card)
            {
                fs::remove_file(&existing_card).map_err(|error| {
                    format!(
                        "Could not remove obsolete {}: {error}",
                        existing_card.display()
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn serialize_folder_card(card: &BoardCard) -> String {
    if card.body.is_empty() {
        format!("# {}\n", card.title)
    } else {
        format!("# {}\n\n{}\n", card.title, card.body.trim_end())
    }
}

fn validate_column_title(title: impl AsRef<str>) -> Result<String, String> {
    let title = title.as_ref().trim();
    if title.is_empty()
        || title == "."
        || title == ".."
        || title.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(format!("Invalid board column name: {title}"));
    }
    Ok(title.to_string())
}
fn serialize_markdown_board(columns: &[BoardColumn]) -> String {
    let mut source = String::from("# Board\n\n");
    for column in columns {
        source.push_str(&format!("## {}\n", column.title));
        for card in &column.cards {
            source.push_str(&format!("- {}\n", card.title));
            for body_line in card.body.lines() {
                source.push_str("  ");
                source.push_str(body_line);
                source.push('\n');
            }
        }
        source.push('\n');
    }
    source
}

/// Saves one Markdown board while holding an advisory exclusive lock across
/// revision comparison and replacement. Every ilium client therefore sees a
/// stale-write error instead of silently overwriting another client's newer
/// board state.
fn save_markdown_board(
    path: &Path,
    source: &str,
    expected_revision: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut current_source = String::new();
    file.read_to_string(&mut current_source)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let revision_matches = match expected_revision {
        Some(expected) => current_source == expected,
        None => current_source.is_empty(),
    };
    if !revision_matches {
        let display_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| path.as_os_str().to_string_lossy());
        return Err(format!(
            "{} changed outside this board; press r to reload before editing",
            display_name
        ));
    }
    file.set_len(0)
        .map_err(|error| format!("Could not update {}: {error}", path.display()))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("Could not update {}: {error}", path.display()))?;
    file.write_all(source.as_bytes())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("Could not sync {}: {error}", path.display()))
}
fn read_sorted(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    Ok(paths)
}
fn safe_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = sanitized
        .trim_matches('-')
        .chars()
        .take(80)
        .collect::<String>();
    if sanitized.is_empty() {
        "card".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crossterm::event::{Event, KeyCode, KeyEvent};

    use super::*;

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ilium-board-{label}-{unique}"))
    }

    fn key_input(code: KeyCode) -> Input {
        Input::from(Event::Key(KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        )))
    }

    #[test]
    fn board_text_extract_renders_columns_and_cards_with_nonempty_bodies_inline() {
        let columns = vec![BoardColumn {
            title: "Doing".to_string(),
            cards: vec![
                BoardCard {
                    title: "Fix auth bug".to_string(),
                    body: "Token expiry check uses < not <=".to_string(),
                },
                BoardCard {
                    title: "No body card".to_string(),
                    body: String::new(),
                },
            ],
        }];

        let extract = board_text_extract(&columns);

        assert_eq!(
            extract,
            "## Doing\n- Fix auth bug: Token expiry check uses < not <=\n- No body card\n"
        );
    }

    #[test]
    fn markdown_storage_round_trips_columns_and_cards() {
        let path = temporary_path("markdown").join("work.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Ship board".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].cards[0].title, "Ship board");
        assert!(path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn existing_todo_markdown_imports_heading_columns_and_common_list_markers() {
        let source =
            "# General\n\n* [ ] First task\n+ [x] Second task\n\n## Later\n\n- Third task\n";

        let columns = parse_markdown_board(source);

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].title, "General");
        assert_eq!(
            columns[0]
                .cards
                .iter()
                .map(|card| card.title.as_str())
                .collect::<Vec<_>>(),
            vec!["[ ] First task", "[x] Second task"]
        );
        assert_eq!(columns[1].title, "Later");
        assert_eq!(columns[1].cards[0].title, "Third task");
    }

    #[test]
    fn markdown_import_ignores_document_title_and_fenced_list_examples() {
        let source =
            "# Board\n\n## To do\n\n```markdown\n- not a card\n```\n- Real card\n\n## Done\n";

        let columns = parse_markdown_board(source);

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].title, "To do");
        assert_eq!(columns[0].cards[0].title, "Real card");
        assert_eq!(columns[1].title, "Done");
        assert!(columns[1].cards.is_empty());
    }

    #[test]
    fn markdown_card_details_round_trip_as_indented_continuation_lines() {
        let path = temporary_path("card-details").join("board.md");
        let source =
            "# Board\n\n## To do\n- Investigate\n  First detail line\n  \n  Final detail line\n";
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::load(storage.clone()).unwrap();

        assert_eq!(
            board.columns[0].cards[0].body,
            "First detail line\n\nFinal detail line"
        );
        board
            .rename_selected_column("In progress".to_string())
            .unwrap();

        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(
            reloaded.columns[0].cards[0].body,
            "First detail line\n\nFinal detail line"
        );
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("  Final detail line"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn folder_storage_creates_a_column_directory_and_markdown_card() {
        let path = temporary_path("folder");
        let storage = BoardStorage::Folder { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Write tests".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        let todo = reloaded
            .columns
            .iter()
            .find(|column| column.title == "To do")
            .unwrap();
        assert_eq!(todo.cards[0].title, "Write tests");
        assert!(path.join("To do").is_dir());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn detail_editor_persists_each_title_and_body_change_to_markdown() {
        let path = temporary_path("detail-edit-markdown").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage).unwrap();
        board.add_card("Task".to_string()).unwrap();
        board.open_card_details(0, 0);

        assert!(board
            .input_detail_editor(key_input(KeyCode::Char('!')))
            .unwrap());
        assert!(fs::read_to_string(&path).unwrap().contains("- Task!\n"));
        board.cycle_detail_editor_focus(false);
        assert!(board
            .input_detail_editor(key_input(KeyCode::Char('N')))
            .unwrap());

        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains("- Task!\n  N\n"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn detail_editor_uses_the_same_title_body_contract_for_folder_storage() {
        let path = temporary_path("detail-edit-folder");
        let storage = BoardStorage::Folder { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Task".to_string()).unwrap();
        board.open_card_details(0, 0);
        board
            .input_detail_editor(key_input(KeyCode::Char('!')))
            .unwrap();
        board.cycle_detail_editor_focus(false);
        board
            .input_detail_editor(key_input(KeyCode::Char('N')))
            .unwrap();

        let reloaded = BoardPane::load(storage).unwrap();
        let todo = reloaded
            .columns
            .iter()
            .find(|column| column.title == "To do")
            .unwrap();
        assert_eq!(todo.cards[0].title, "Task!");
        assert_eq!(todo.cards[0].body, "N");
        let card_path = read_sorted(&path.join("To do"))
            .unwrap()
            .into_iter()
            .find(|path| path.extension().is_some_and(|extension| extension == "md"))
            .unwrap();
        assert_eq!(fs::read_to_string(card_path).unwrap(), "# Task!\n\nN\n");
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn task_checkbox_toggle_persists_immediately_for_both_backends() {
        for (label, storage) in [
            {
                let path = temporary_path("checkbox-markdown").join("board.md");
                ("markdown", BoardStorage::MarkdownFile { path })
            },
            {
                let path = temporary_path("checkbox-folder");
                ("folder", BoardStorage::Folder { path })
            },
        ] {
            let storage_path = storage.path().to_path_buf();
            let mut board = BoardPane::create(storage.clone()).unwrap();
            board.add_card("[ ] Done".to_string()).unwrap();

            board.toggle_card_checkbox(0, 0, 0).unwrap();

            let reloaded = BoardPane::load(storage).unwrap();
            let persisted_card = reloaded
                .columns
                .iter()
                .find_map(|column| column.cards.first())
                .unwrap();
            assert_eq!(persisted_card.title, "[x] Done", "{label}");
            let cleanup_path = if storage_path.is_file() {
                storage_path.parent().unwrap().to_path_buf()
            } else {
                storage_path
            };
            let _ = fs::remove_dir_all(cleanup_path);
        }
    }

    #[test]
    fn rename_and_delete_keep_markdown_storage_in_sync() {
        let path = temporary_path("mutations").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Draft".to_string()).unwrap();
        board.rename_selected_card("Publish".to_string()).unwrap();
        board.delete_selected_card().unwrap();
        board.rename_selected_column("Ideas".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].title, "Ideas");
        assert!(reloaded.columns[0].cards.is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn populated_column_header_can_be_selected_and_renamed() {
        let path = temporary_path("column-selection").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Keep this card".to_string()).unwrap();

        board.select_column(0);
        board.rename_selected_column("Backlog".to_string()).unwrap();

        assert_eq!(board.selected_card, None);
        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].title, "Backlog");
        assert_eq!(reloaded.columns[0].cards[0].title, "Keep this card");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn cards_reorder_vertically_and_to_an_exact_cross_column_position() {
        let path = temporary_path("card-order").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        for title in ["First", "Second", "Third"] {
            board.add_card(title.to_string()).unwrap();
        }

        board.move_selected_card_vertically(-1).unwrap();
        assert_eq!(
            board.columns[0]
                .cards
                .iter()
                .map(|card| card.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Third", "Second"]
        );
        board.move_card(0, 1, 1, 0).unwrap();

        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].cards[0].title, "First");
        assert_eq!(reloaded.columns[0].cards[1].title, "Second");
        assert_eq!(reloaded.columns[1].cards[0].title, "Third");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn visual_same_column_insertion_is_converted_after_source_removal() {
        let path = temporary_path("drag-index").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage).unwrap();
        for title in ["First", "Second", "Third"] {
            board.add_card(title.to_string()).unwrap();
        }

        board.move_dragged_card(0, 0, 0, 2).unwrap();

        assert_eq!(
            board.columns[0]
                .cards
                .iter()
                .map(|card| card.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Second", "First", "Third"]
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stale_markdown_edit_is_rejected_and_in_memory_mutation_is_rolled_back() {
        let path = temporary_path("revision-conflict").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage).unwrap();
        board.add_card("Original".to_string()).unwrap();
        let columns_before_conflict = board.columns.clone();
        let external_source = "# Board\n\n## To do\n- External\n\n## Doing\n\n## Done\n";
        fs::write(&path, external_source).unwrap();

        let error = board.add_card("Stale write".to_string()).unwrap_err();

        assert!(error.contains("press r to reload"));
        assert_eq!(board.columns, columns_before_conflict);
        assert_eq!(fs::read_to_string(&path).unwrap(), external_source);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn folder_save_preserves_non_markdown_files_in_column() {
        let path = temporary_path("preserve");
        let storage = BoardStorage::Folder { path: path.clone() };
        let mut board = BoardPane::create(storage).unwrap();
        let attachment = path.join("To do").join("notes.txt");
        fs::write(&attachment, "keep me").unwrap();
        board.add_card("Task".to_string()).unwrap();
        assert_eq!(fs::read_to_string(attachment).unwrap(), "keep me");
        let _ = fs::remove_dir_all(path);
    }
}
