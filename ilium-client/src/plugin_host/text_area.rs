//! Host-owned state of a `text-area` widget of a plugin view (widgets-input page, WID-026).
//!
//! A text area edits several lines. Enter inserts a line and never submits; Tab inserts a tab
//! unless `tab-moves-focus` is set; `max-bytes` counts UTF-8 bytes (0 means 1 MiB) and refuses a
//! typed commit that does not fit whole. The plugin hears `changed` after the debounce: an
//! explicit `debounce-ms` wins, otherwise 600 ms while the text is at most 10,000 bytes and 1 s
//! above, re-evaluated at each keystroke. The WIT field is a plain `u32`, so `0` stands for
//! "unset" here.
//!
//! Like `ui_state`, this module is pure: `view` draws the layout it returns and turns keys,
//! clicks, pastes and the wheel into calls on it.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ilium_plugin_api::generated::types::ui::TextAreaSpec;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::ui_state::{
    is_bidi_control, next_boundary, previous_boundary, word_start, EditOutcome, InputCell,
    DEBOUNCE_CAP_MS, PASTE_CAP_BYTES,
};

/// Largest text of an area whose `max-bytes` is 0.
pub(crate) const AREA_CAP_BYTES: usize = 1_048_576;
/// Debounce of an area whose `debounce-ms` is unset, while the text is short.
const QUIET: Duration = Duration::from_millis(600);
/// Debounce of an area whose `debounce-ms` is unset, once the text is long.
const LONG_QUIET: Duration = Duration::from_millis(1_000);
/// Above this many bytes an unset debounce waits `LONG_QUIET`.
const LONG_TEXT_BYTES: usize = 10_000;
/// Cells a tab takes on screen.
const TAB_CELLS: usize = 4;
/// Fewest and most rows a text area asks for when its container stacks it.
pub(crate) const MIN_ROWS: u16 = 3;
pub(crate) const MAX_ROWS: u16 = 12;

/// The cells the text of an area is drawn in, without the line-number gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AreaView {
    pub width: u16,
    pub rows: u16,
}

/// One screen row of the text: a byte range of one logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisualRow {
    start: usize,
    end: usize,
    line: usize,
    /// The first screen row of its logical line, which carries the line number.
    first: bool,
}

/// One drawn row of an area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AreaRow {
    /// The 1-based line number shown in the gutter, on the first row of each line.
    pub number: Option<usize>,
    pub cells: Vec<InputCell>,
}

/// An area laid out in its view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AreaLayout {
    pub rows: Vec<AreaRow>,
    /// Row and column of the caret inside the view, when it is visible.
    pub caret: Option<(u16, u16)>,
    /// Screen rows of the whole text.
    pub total_rows: usize,
    /// First screen row shown.
    pub top: usize,
}

/// The debounce of one edit: an explicit `debounce-ms` (capped at 2 s) or the length rule.
pub(crate) fn area_debounce(spec: &TextAreaSpec, text_bytes: usize) -> Duration {
    if spec.debounce_ms != 0 {
        Duration::from_millis(u64::from(spec.debounce_ms.min(DEBOUNCE_CAP_MS)))
    } else if text_bytes > LONG_TEXT_BYTES {
        LONG_QUIET
    } else {
        QUIET
    }
}

/// The rows a text area asks for when stacked: its starting lines, kept between the bounds.
pub(crate) fn wanted_rows(spec: &TextAreaSpec) -> u16 {
    let lines = u16::try_from(spec.initial.split('\n').count()).unwrap_or(u16::MAX);
    lines.clamp(MIN_ROWS, MAX_ROWS)
}

/// What a cluster shows and how many cells it takes: a tab is spaces.
fn shown(cluster: &str) -> (String, usize) {
    if cluster == "\t" {
        (" ".repeat(TAB_CELLS), TAB_CELLS)
    } else {
        (cluster.to_owned(), UnicodeWidthStr::width(cluster))
    }
}

/// Keeps line breaks and tabs, turns `\r\n` and `\r` into `\n`, and drops every other control
/// and every bidirectional override.
fn cleaned(incoming: &str) -> String {
    incoming
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|character| {
            (matches!(character, '\n' | '\t') || !character.is_control())
                && !is_bidi_control(*character)
        })
        .collect()
}

fn limit_of(spec: &TextAreaSpec) -> usize {
    match usize::try_from(spec.max_bytes) {
        Ok(0) | Err(_) => AREA_CAP_BYTES,
        Ok(bytes) => bytes.min(AREA_CAP_BYTES),
    }
}

/// The screen rows of `text` at `width` cells: one per line, or more when `wrap` breaks long
/// lines between clusters.
fn visual_rows(text: &str, width: u16, wrap: bool) -> Vec<VisualRow> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    let mut line_start = 0usize;
    for (line, piece) in text.split('\n').enumerate() {
        let mut start = line_start;
        let mut used = 0usize;
        let mut first = true;
        if wrap {
            for (offset, cluster) in piece.grapheme_indices(true) {
                let cells = shown(cluster).1;
                if used > 0 && used + cells > width {
                    rows.push(VisualRow {
                        start,
                        end: line_start + offset,
                        line,
                        first,
                    });
                    first = false;
                    start = line_start + offset;
                    used = 0;
                }
                used += cells;
            }
        }
        rows.push(VisualRow {
            start,
            end: line_start + piece.len(),
            line,
            first,
        });
        line_start += piece.len() + 1;
    }
    rows
}

/// The editable state of one `text-area`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AreaEdit {
    text: String,
    /// Byte offset of the caret, always on a cluster boundary.
    caret: usize,
    anchor: Option<usize>,
    /// First screen row shown.
    top: usize,
    /// The column Up and Down keep while they move through shorter rows.
    goal: Option<usize>,
    due: Option<Instant>,
}

impl AreaEdit {
    /// An area the user has not touched: `initial`, cut to the 1 MiB cap, caret at the end.
    pub(crate) fn seeded(initial: &str) -> AreaEdit {
        let mut text = String::new();
        for cluster in cleaned(initial).graphemes(true) {
            if text.len() + cluster.len() > AREA_CAP_BYTES {
                break;
            }
            text.push_str(cluster);
        }
        AreaEdit {
            caret: text.len(),
            text,
            ..AreaEdit::default()
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Cells of the line-number gutter: the widest number and two blanks, or none.
    pub(crate) fn gutter(&self, spec: &TextAreaSpec) -> u16 {
        if !spec.line_numbers {
            return 0;
        }
        let lines = self.text.split('\n').count();
        let digits = lines.to_string().len();
        u16::try_from(digits + 2).unwrap_or(u16::MAX)
    }

    fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .map(|anchor| (anchor.min(self.caret), anchor.max(self.caret)))
            .filter(|(start, end)| start != end)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        true
    }

    fn move_caret(&mut self, to: usize, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = to.min(self.text.len());
    }

    /// Inserts text at the caret, replacing the selection, within `max-bytes`. `whole` refuses
    /// a commit that does not fit entirely; otherwise the longest run of clusters that fits
    /// goes in.
    fn insert(&mut self, incoming: &str, spec: &TextAreaSpec, whole: bool) -> bool {
        let clean = cleaned(incoming);
        if clean.is_empty() {
            return false;
        }
        let selected = self.selection().map_or(0, |(start, end)| end - start);
        let room = limit_of(spec).saturating_sub(self.text.len() - selected);
        let mut piece = String::new();
        for cluster in clean.graphemes(true) {
            if piece.len() + cluster.len() > room {
                break;
            }
            piece.push_str(cluster);
        }
        if piece.is_empty() || (whole && piece.len() < clean.len()) {
            return false;
        }
        self.delete_selection();
        self.text.insert_str(self.caret, &piece);
        self.caret += piece.len();
        true
    }

    fn rows(&self, spec: &TextAreaSpec, view: AreaView) -> Vec<VisualRow> {
        visual_rows(&self.text, view.width, spec.wrap)
    }

    /// The screen row that holds `byte`: the last row starting at or before it.
    fn row_of(rows: &[VisualRow], byte: usize) -> usize {
        rows.iter()
            .rposition(|row| row.start <= byte)
            .unwrap_or_default()
    }

    /// Cells from the start of `row` to `byte`.
    fn column_of(&self, row: VisualRow, byte: usize) -> usize {
        self.text[row.start..byte.clamp(row.start, row.end)]
            .graphemes(true)
            .map(|cluster| shown(cluster).1)
            .sum()
    }

    /// The last caret position of a row. A row a wrap broke ends before its last cluster, so
    /// the caret stays on that row instead of jumping to the next one.
    fn row_end(&self, rows: &[VisualRow], index: usize) -> usize {
        let row = rows[index];
        let wrapped = rows
            .get(index + 1)
            .is_some_and(|next| next.line == row.line);
        if wrapped {
            previous_boundary(&self.text, row.end).max(row.start)
        } else {
            row.end
        }
    }

    /// The byte under `column` cells of row `index`.
    fn byte_at(&self, rows: &[VisualRow], index: usize, column: usize) -> usize {
        let row = rows[index];
        let mut used = 0usize;
        for (offset, cluster) in self.text[row.start..row.end].grapheme_indices(true) {
            let cells = shown(cluster).1;
            if column < used + cells.max(1) {
                return row.start + offset;
            }
            used += cells;
        }
        self.row_end(rows, index)
    }

    /// The first column shown when lines are not wrapped: far enough right to show the caret.
    fn left(&self, spec: &TextAreaSpec, rows: &[VisualRow], view: AreaView) -> usize {
        if spec.wrap {
            return 0;
        }
        let row = rows[Self::row_of(rows, self.caret)];
        let column = self.column_of(row, self.caret);
        (column + 1).saturating_sub(usize::from(view.width.max(1)))
    }

    /// Scrolls so the caret row is inside the view.
    fn reveal(&mut self, spec: &TextAreaSpec, view: AreaView) {
        let rows = self.rows(spec, view);
        let row = Self::row_of(&rows, self.caret);
        let height = usize::from(view.rows.max(1));
        if row < self.top {
            self.top = row;
        } else if row >= self.top + height {
            self.top = row + 1 - height;
        }
    }

    /// Moves the caret `delta` screen rows, keeping its column. Returns `Ignored` when it is
    /// already at the edge of the text that way, so the view can move the keyboard on.
    fn move_rows(
        &mut self,
        spec: &TextAreaSpec,
        view: AreaView,
        delta: i64,
        extend: bool,
    ) -> EditOutcome {
        let rows = self.rows(spec, view);
        let current = Self::row_of(&rows, self.caret);
        let goal = self
            .goal
            .unwrap_or_else(|| self.column_of(rows[current], self.caret));
        let last = rows.len() - 1;
        let target = i64::try_from(current)
            .unwrap_or(i64::MAX)
            .saturating_add(delta);
        let to = if target < 0 {
            if self.caret == 0 && !extend {
                return EditOutcome::Ignored;
            }
            0
        } else if usize::try_from(target).unwrap_or(usize::MAX) > last {
            if self.caret == self.text.len() && !extend {
                return EditOutcome::Ignored;
            }
            self.text.len()
        } else {
            self.byte_at(&rows, usize::try_from(target).unwrap_or(last), goal)
        };
        self.move_caret(to, extend);
        self.goal = Some(goal);
        EditOutcome::Moved
    }

    /// Applies one key. A refused commit (no room left) is taken without a change, so typing
    /// never moves the keyboard out of the area.
    pub(crate) fn apply_key(
        &mut self,
        key: KeyEvent,
        spec: &TextAreaSpec,
        view: AreaView,
        now: Instant,
    ) -> EditOutcome {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let page = i64::from(view.rows.saturating_sub(1).max(1));
        let typed = |edit: &mut AreaEdit, text: &str| {
            if edit.insert(text, spec, true) {
                EditOutcome::Edited
            } else {
                EditOutcome::Moved
            }
        };
        let outcome = match key.code {
            KeyCode::Char(character) if control => match character.to_ascii_lowercase() {
                'a' | 'e' => {
                    let rows = self.rows(spec, view);
                    let index = Self::row_of(&rows, self.caret);
                    let to = if character.eq_ignore_ascii_case(&'a') {
                        rows[index].start
                    } else {
                        self.row_end(&rows, index)
                    };
                    self.move_caret(to, shift);
                    EditOutcome::Moved
                }
                'u' => {
                    let line_start = self.text[..self.caret].rfind('\n').map_or(0, |at| at + 1);
                    if line_start == self.caret {
                        EditOutcome::Ignored
                    } else {
                        let caret = self.caret;
                        self.text.replace_range(line_start..caret, "");
                        self.caret = line_start;
                        self.anchor = None;
                        EditOutcome::Edited
                    }
                }
                'w' => {
                    if self.delete_selection() {
                        EditOutcome::Edited
                    } else {
                        let start = word_start(&self.text, self.caret);
                        if start == self.caret {
                            EditOutcome::Ignored
                        } else {
                            let caret = self.caret;
                            self.text.replace_range(start..caret, "");
                            self.caret = start;
                            EditOutcome::Edited
                        }
                    }
                }
                _ => EditOutcome::Ignored,
            },
            KeyCode::Char(_) if alt => EditOutcome::Ignored,
            KeyCode::Char(character) => {
                let mut buffer = [0u8; 4];
                typed(self, character.encode_utf8(&mut buffer))
            }
            KeyCode::Enter => typed(self, "\n"),
            KeyCode::Tab if spec.tab_moves_focus => EditOutcome::Ignored,
            KeyCode::Tab => typed(self, "\t"),
            KeyCode::Backspace => {
                if self.delete_selection() {
                    EditOutcome::Edited
                } else if self.caret > 0 {
                    let start = previous_boundary(&self.text, self.caret);
                    let caret = self.caret;
                    self.text.replace_range(start..caret, "");
                    self.caret = start;
                    EditOutcome::Edited
                } else {
                    EditOutcome::Moved
                }
            }
            KeyCode::Delete => {
                if self.delete_selection() {
                    EditOutcome::Edited
                } else if self.caret < self.text.len() {
                    let end = next_boundary(&self.text, self.caret);
                    let caret = self.caret;
                    self.text.replace_range(caret..end, "");
                    EditOutcome::Edited
                } else {
                    EditOutcome::Moved
                }
            }
            KeyCode::Left => {
                match (self.selection(), shift) {
                    (Some((start, _)), false) => {
                        self.anchor = None;
                        self.caret = start;
                    }
                    _ => {
                        let to = previous_boundary(&self.text, self.caret);
                        self.move_caret(to, shift);
                    }
                }
                EditOutcome::Moved
            }
            KeyCode::Right => {
                match (self.selection(), shift) {
                    (Some((_, end)), false) => {
                        self.anchor = None;
                        self.caret = end;
                    }
                    _ => {
                        let to = next_boundary(&self.text, self.caret);
                        self.move_caret(to, shift);
                    }
                }
                EditOutcome::Moved
            }
            KeyCode::Up => self.move_rows(spec, view, -1, shift),
            KeyCode::Down => self.move_rows(spec, view, 1, shift),
            KeyCode::PageUp => self.move_rows(spec, view, -page, shift),
            KeyCode::PageDown => self.move_rows(spec, view, page, shift),
            KeyCode::Home if control => {
                self.move_caret(0, shift);
                EditOutcome::Moved
            }
            KeyCode::End if control => {
                let end = self.text.len();
                self.move_caret(end, shift);
                EditOutcome::Moved
            }
            KeyCode::Home => {
                let rows = self.rows(spec, view);
                let start = rows[Self::row_of(&rows, self.caret)].start;
                self.move_caret(start, shift);
                EditOutcome::Moved
            }
            KeyCode::End => {
                let rows = self.rows(spec, view);
                let end = self.row_end(&rows, Self::row_of(&rows, self.caret));
                self.move_caret(end, shift);
                EditOutcome::Moved
            }
            _ => EditOutcome::Ignored,
        };
        if !matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
        ) {
            self.goal = None;
        }
        if outcome == EditOutcome::Edited {
            self.due = Some(now + area_debounce(spec, self.text.len()));
        }
        if outcome != EditOutcome::Ignored {
            self.reveal(spec, view);
        }
        outcome
    }

    /// A bracketed paste: one change, cut to 1 MiB and to the room `max-bytes` leaves.
    pub(crate) fn paste(
        &mut self,
        pasted: &str,
        spec: &TextAreaSpec,
        view: AreaView,
        now: Instant,
    ) -> bool {
        let mut end = pasted.len().min(PASTE_CAP_BYTES);
        while !pasted.is_char_boundary(end) {
            end -= 1;
        }
        let edited = self.insert(&pasted[..end], spec, false);
        if edited {
            self.goal = None;
            self.due = Some(now + area_debounce(spec, self.text.len()));
            self.reveal(spec, view);
        }
        edited
    }

    /// Puts the caret on the cluster under `column` of `row` of the view.
    pub(crate) fn place_caret(
        &mut self,
        spec: &TextAreaSpec,
        view: AreaView,
        column: u16,
        row: u16,
    ) {
        let rows = self.rows(spec, view);
        let left = self.left(spec, &rows, view);
        let index = (self.top + usize::from(row)).min(rows.len() - 1);
        let byte = self.byte_at(&rows, index, left + usize::from(column));
        self.anchor = None;
        self.goal = None;
        self.caret = byte;
    }

    /// The wheel: moves the view `delta` rows without moving the caret. Returns whether it moved.
    pub(crate) fn scroll_by(&mut self, spec: &TextAreaSpec, view: AreaView, delta: i32) -> bool {
        let total = self.rows(spec, view).len();
        let max_top = total.saturating_sub(usize::from(view.rows.max(1)));
        let current = self.top.min(max_top);
        let wanted = i64::try_from(current)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(delta))
            .clamp(0, i64::try_from(max_top).unwrap_or(i64::MAX));
        let top = usize::try_from(wanted).unwrap_or(max_top);
        let moved = top != self.top;
        self.top = top;
        moved
    }

    /// The text to send once the debounce has run out.
    pub(crate) fn take_due(&mut self, now: Instant) -> Option<String> {
        match self.due {
            Some(due) if due <= now => {
                self.due = None;
                Some(self.text.clone())
            }
            _ => None,
        }
    }

    /// The text of an edit not sent yet, whatever its debounce says.
    pub(crate) fn flush(&mut self) -> Option<String> {
        self.due.take().map(|_| self.text.clone())
    }

    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.due
    }

    /// The rows shown in `view`, from the kept first row, with the caret when it is visible.
    pub(crate) fn layout(&self, spec: &TextAreaSpec, view: AreaView) -> AreaLayout {
        let rows = self.rows(spec, view);
        let height = usize::from(view.rows);
        let top = self.top.min(rows.len().saturating_sub(height.max(1)));
        let left = self.left(spec, &rows, view);
        let width = usize::from(view.width);
        let selection = self.selection();
        let caret_row = Self::row_of(&rows, self.caret);
        let mut layout = AreaLayout {
            total_rows: rows.len(),
            top,
            ..AreaLayout::default()
        };
        for (index, row) in rows.iter().enumerate().skip(top).take(height) {
            let mut cells = Vec::new();
            let mut column = 0usize;
            for (offset, cluster) in self.text[row.start..row.end].grapheme_indices(true) {
                let (text, cells_of) = shown(cluster);
                let byte = row.start + offset;
                let start = column;
                column += cells_of;
                if start < left {
                    continue;
                }
                if column - left > width {
                    break;
                }
                cells.push(InputCell {
                    text,
                    selected: selection.is_some_and(|(from, to)| byte >= from && byte < to),
                });
            }
            if index == caret_row {
                let caret_column = self.column_of(*row, self.caret).saturating_sub(left);
                if caret_column <= width {
                    layout.caret = Some((
                        u16::try_from(index - top).unwrap_or(u16::MAX),
                        u16::try_from(caret_column.min(width.saturating_sub(1)))
                            .unwrap_or(u16::MAX),
                    ));
                }
            }
            layout.rows.push(AreaRow {
                number: row.first.then_some(row.line + 1),
                cells,
            });
        }
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: AreaView = AreaView { width: 10, rows: 3 };

    fn spec() -> TextAreaSpec {
        TextAreaSpec {
            wrap: true,
            ..TextAreaSpec::default()
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(edit: &mut AreaEdit, spec: &TextAreaSpec, text: &str, now: Instant) {
        for character in text.chars() {
            let code = if character == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(character)
            };
            edit.apply_key(press(code), spec, VIEW, now);
        }
    }

    fn row_text(row: &AreaRow) -> String {
        row.cells.iter().map(|cell| cell.text.as_str()).collect()
    }

    #[test]
    fn enter_inserts_a_line_and_never_submits() {
        let spec = spec();
        let mut edit = AreaEdit::seeded("");
        let now = Instant::now();
        typed(&mut edit, &spec, "one", now);
        assert_eq!(
            edit.apply_key(press(KeyCode::Enter), &spec, VIEW, now),
            EditOutcome::Edited
        );
        typed(&mut edit, &spec, "two", now);
        assert_eq!(edit.text(), "one\ntwo");
        let layout = edit.layout(&spec, VIEW);
        assert_eq!(layout.rows.len(), 2);
        assert_eq!(row_text(&layout.rows[1]), "two");
        assert_eq!(layout.caret, Some((1, 3)));
    }

    #[test]
    fn tab_inserts_unless_it_moves_the_focus() {
        let mut spec = spec();
        let mut edit = AreaEdit::seeded("");
        let now = Instant::now();
        assert_eq!(
            edit.apply_key(press(KeyCode::Tab), &spec, VIEW, now),
            EditOutcome::Edited
        );
        assert_eq!(edit.text(), "\t");
        assert_eq!(row_text(&edit.layout(&spec, VIEW).rows[0]), "    ");
        spec.tab_moves_focus = true;
        assert_eq!(
            edit.apply_key(press(KeyCode::Tab), &spec, VIEW, now),
            EditOutcome::Ignored
        );
        assert_eq!(edit.text(), "\t");
    }

    #[test]
    fn max_bytes_counts_bytes_and_refuses_a_commit_that_does_not_fit() {
        let spec = TextAreaSpec {
            max_bytes: 3,
            ..spec()
        };
        let mut edit = AreaEdit::seeded("");
        let now = Instant::now();
        typed(&mut edit, &spec, "é", now);
        assert_eq!(edit.text().len(), 2);
        // A second two-byte letter does not fit whole: refused, and the keyboard stays.
        assert_eq!(
            edit.apply_key(press(KeyCode::Char('é')), &spec, VIEW, now),
            EditOutcome::Moved
        );
        typed(&mut edit, &spec, "a", now);
        assert_eq!(edit.text(), "éa");
        // A paste keeps the clusters that fit.
        let mut pasted = AreaEdit::seeded("");
        assert!(pasted.paste("abcdef", &spec, VIEW, now));
        assert_eq!(pasted.text(), "abc");
    }

    #[test]
    fn a_paste_normalises_line_breaks_and_drops_controls_and_bidi_overrides() {
        let spec = spec();
        let mut edit = AreaEdit::seeded("");
        assert!(edit.paste("a\r\nb\rc\u{1b}\u{202e}d", &spec, VIEW, Instant::now()));
        assert_eq!(edit.text(), "a\nb\ncd");
    }

    #[test]
    fn the_unset_debounce_grows_with_the_text_and_an_explicit_one_wins() {
        let short = spec();
        assert_eq!(area_debounce(&short, 10_000), Duration::from_millis(600));
        assert_eq!(area_debounce(&short, 10_001), Duration::from_millis(1_000));
        let explicit = TextAreaSpec {
            debounce_ms: 200,
            ..spec()
        };
        assert_eq!(area_debounce(&explicit, 50_000), Duration::from_millis(200));

        let mut edit = AreaEdit::seeded(&"x".repeat(10_000));
        let now = Instant::now();
        typed(&mut edit, &short, "y", now);
        assert_eq!(edit.next_due(), Some(now + Duration::from_millis(1_000)));
        assert_eq!(edit.take_due(now + Duration::from_millis(999)), None);
        assert_eq!(
            edit.take_due(now + Duration::from_millis(1_000))
                .map(|text| text.len()),
            Some(10_001)
        );
        assert_eq!(edit.flush(), None);
    }

    #[test]
    fn long_lines_wrap_and_up_and_down_keep_the_column() {
        let spec = spec();
        let mut edit = AreaEdit::seeded("abcdefghijklmno\nxy");
        let layout = edit.layout(&spec, VIEW);
        assert_eq!(layout.total_rows, 3);
        assert_eq!(row_text(&layout.rows[0]), "abcdefghij");
        assert_eq!(row_text(&layout.rows[1]), "klmno");
        assert_eq!(layout.rows[0].number, Some(1));
        assert_eq!(layout.rows[1].number, None);
        assert_eq!(layout.rows[2].number, Some(2));
        let now = Instant::now();
        // From the end of "xy" (column 2) Up goes to column 2 of the wrapped row.
        assert_eq!(
            edit.apply_key(press(KeyCode::Up), &spec, VIEW, now),
            EditOutcome::Moved
        );
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((1, 2)));
        edit.apply_key(press(KeyCode::Up), &spec, VIEW, now);
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((0, 2)));
        // On the first row Up goes to the start, then lets the keyboard leave.
        assert_eq!(
            edit.apply_key(press(KeyCode::Up), &spec, VIEW, now),
            EditOutcome::Moved
        );
        assert_eq!(
            edit.apply_key(press(KeyCode::Up), &spec, VIEW, now),
            EditOutcome::Ignored
        );
        // End on a wrapped row stays on that row.
        edit.apply_key(press(KeyCode::End), &spec, VIEW, now);
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((0, 9)));
    }

    #[test]
    fn the_view_follows_the_caret_and_the_wheel_scrolls_without_moving_it() {
        let spec = spec();
        let mut edit = AreaEdit::seeded("1\n2\n3\n4\n5\n6");
        let now = Instant::now();
        // Untouched, the view starts at the top; the caret at the end is below it.
        let untouched = edit.layout(&spec, VIEW);
        assert_eq!(untouched.top, 0);
        assert_eq!(untouched.caret, None);
        // A key brings the caret row into the last row of the view.
        edit.apply_key(press(KeyCode::Left), &spec, VIEW, now);
        let layout = edit.layout(&spec, VIEW);
        assert_eq!(layout.top, 3);
        assert_eq!(row_text(&layout.rows[2]), "6");
        assert_eq!(layout.caret, Some((2, 0)));
        assert!(edit.scroll_by(&spec, VIEW, -2));
        let scrolled = edit.layout(&spec, VIEW);
        assert_eq!(scrolled.top, 1);
        assert_eq!(scrolled.caret, None);
        assert!(edit.scroll_by(&spec, VIEW, -5));
        assert_eq!(edit.layout(&spec, VIEW).top, 0);
        assert!(!edit.scroll_by(&spec, VIEW, -1));
    }

    #[test]
    fn a_click_places_the_caret_under_the_pointer() {
        let spec = spec();
        let mut edit = AreaEdit::seeded("first\nsecond");
        edit.place_caret(&spec, VIEW, 2, 1);
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((1, 2)));
        // Past the end of a short line: the end of that line.
        edit.place_caret(&spec, VIEW, 9, 0);
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((0, 5)));
        // Below the last row: the last row.
        edit.place_caret(&spec, VIEW, 0, 2);
        assert_eq!(edit.layout(&spec, VIEW).caret, Some((1, 0)));
    }

    #[test]
    fn without_wrap_a_long_line_scrolls_sideways_to_show_the_caret() {
        let spec = TextAreaSpec {
            wrap: false,
            ..spec()
        };
        let edit = AreaEdit::seeded("abcdefghijklmno");
        let layout = edit.layout(&spec, VIEW);
        assert_eq!(layout.total_rows, 1);
        assert_eq!(row_text(&layout.rows[0]), "ghijklmno");
        assert_eq!(layout.caret, Some((0, 9)));
    }

    #[test]
    fn the_gutter_fits_the_widest_line_number() {
        let numbered = TextAreaSpec {
            line_numbers: true,
            ..spec()
        };
        assert_eq!(AreaEdit::seeded("a").gutter(&numbered), 3);
        assert_eq!(AreaEdit::seeded(&"\n".repeat(9)).gutter(&numbered), 4);
        assert_eq!(AreaEdit::seeded("a").gutter(&spec()), 0);
    }
}
