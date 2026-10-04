//! Shared, caller-owned choice and number dialogs. This module never changes a setting.
//! Keep the prepared dialog for both painting and hit testing of one frame.

use std::collections::HashSet;

use crate::text_prompt::{self, PromptOutcome, TextPromptState};
use crate::value_control::{cell_width, clip_cells, PointerButton};
use crossterm::event::KeyCode;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// `id` identifies an option across filtering and repainting. The caller must
/// revalidate that identity and its availability against fresh domain state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceDialogState {
    pub title: String,
    pub selected_id: Option<String>,
    pub search: TextPromptState,
    pub highlighted_id: Option<String>,
    pub scroll_line: usize,
    pub notice: Option<String>,
    options: Vec<ChoiceOption>,
}

impl ChoiceDialogState {
    pub fn new(
        title: impl Into<String>,
        options: Vec<ChoiceOption>,
        selected_id: Option<String>,
    ) -> Result<Self, String> {
        validate_ids(&options)?;
        let highlighted_id = selected_id
            .as_ref()
            .filter(|id| options.iter().any(|option| &option.id == *id))
            .cloned()
            .or_else(|| options.first().map(|option| option.id.clone()));
        Ok(Self {
            title: title.into(),
            selected_id,
            search: TextPromptState::new(""),
            highlighted_id,
            scroll_line: 0,
            notice: None,
            options,
        })
    }

    pub fn options(&self) -> &[ChoiceOption] {
        &self.options
    }

    /// Refresh a live catalog by ID. An absent saved value remains visible in
    /// the status line; only the caller may decide whether to replace it.
    pub fn replace_options(&mut self, options: Vec<ChoiceOption>) -> Result<(), String> {
        validate_ids(&options)?;
        self.options = options;
        self.reconcile_highlight();
        self.notice = None;
        Ok(())
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.search.buf.to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter_map(|(index, option)| {
                (query.is_empty()
                    || option.label.to_lowercase().contains(&query)
                    || option.id.to_lowercase().contains(&query)
                    || option
                        .disabled_reason
                        .as_ref()
                        .is_some_and(|reason| reason.to_lowercase().contains(&query)))
                .then_some(index)
            })
            .collect()
    }

    fn reconcile_highlight(&mut self) {
        let filtered = self.filtered_indices();
        let still_visible = self
            .highlighted_id
            .as_ref()
            .is_some_and(|id| filtered.iter().any(|index| self.options[*index].id == *id));
        if !still_visible {
            self.highlighted_id = self
                .selected_id
                .as_ref()
                .filter(|id| filtered.iter().any(|index| self.options[*index].id == **id))
                .cloned()
                .or_else(|| {
                    filtered
                        .first()
                        .map(|index| self.options[*index].id.clone())
                });
        }
        self.scroll_line = 0;
    }

    fn move_highlight(&mut self, forward: bool, steps: usize) {
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            self.highlighted_id = None;
            return;
        }
        let current = self.highlighted_id.as_ref().and_then(|id| {
            filtered
                .iter()
                .position(|index| self.options[*index].id == *id)
        });
        let next = match current {
            Some(position) if forward => position.saturating_add(steps).min(filtered.len() - 1),
            Some(position) => position.saturating_sub(steps),
            None => 0,
        };
        self.highlighted_id = Some(self.options[filtered[next]].id.clone());
    }

    fn choose(&mut self, id: &str) -> DialogOutcome {
        match self.options.iter().find(|option| option.id == id) {
            Some(option) if option.disabled_reason.is_some() => {
                self.highlighted_id = Some(option.id.clone());
                self.notice = option.disabled_reason.clone();
                DialogOutcome::Continue
            }
            Some(option) => {
                self.highlighted_id = Some(option.id.clone());
                DialogOutcome::Choose(option.id.clone())
            }
            None => {
                self.notice = Some("That option changed; reopen the list".into());
                DialogOutcome::Continue
            }
        }
    }

    /// Keep the highlighted stable ID in view after navigation or a host catalog refresh.
    pub fn reveal_highlight(&mut self, screen: Rect) {
        let layout = dialog_layout(screen);
        let width = layout.document.width.saturating_sub(1);
        let rows = choice_rows(self, width);
        let visible = usize::from(layout.document.height);
        let maximum = rows.len().saturating_sub(visible);
        let Some(id) = self.highlighted_id.as_ref() else {
            self.scroll_line = self.scroll_line.min(maximum);
            return;
        };
        let Some(first) = rows
            .iter()
            .position(|row| row.option_id.as_deref() == Some(id))
        else {
            self.scroll_line = self.scroll_line.min(maximum);
            return;
        };
        if first < self.scroll_line {
            self.scroll_line = first;
        } else if visible > 0 && first >= self.scroll_line.saturating_add(visible) {
            self.scroll_line = first.saturating_add(1).saturating_sub(visible);
        }
        self.scroll_line = self.scroll_line.min(maximum);
    }

    pub fn scroll_by(&mut self, screen: Rect, lines: i32) {
        let layout = dialog_layout(screen);
        let row_count = choice_rows(self, layout.document.width.saturating_sub(1)).len();
        let maximum = row_count.saturating_sub(usize::from(layout.document.height));
        self.scroll_line = if lines < 0 {
            self.scroll_line
                .saturating_sub(lines.unsigned_abs() as usize)
        } else {
            self.scroll_line.saturating_add(lines as usize).min(maximum)
        };
    }
}

fn validate_ids(options: &[ChoiceOption]) -> Result<(), String> {
    let mut seen = HashSet::with_capacity(options.len());
    for option in options {
        if option.id.is_empty() {
            return Err("Choice IDs must not be empty".into());
        }
        if !seen.insert(option.id.as_str()) {
            return Err(format!("Duplicate choice ID: {}", option.id));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberDialogState {
    pub title: String,
    pub draft: TextPromptState,
    pub error: Option<String>,
}

impl NumberDialogState {
    pub fn new(title: impl Into<String>, initial: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            draft: TextPromptState::new(initial),
            error: None,
        }
    }

    /// A rejected host validation or save keeps the exact draft and cursor.
    pub fn reject(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueDialogState {
    Choice(ChoiceDialogState),
    Number(NumberDialogState),
}

/// A commit request is only an intent. The host validates a fresh choice ID
/// or the exact numeric draft before changing or persisting anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    Continue,
    Cancel,
    Choose(String),
    CommitNumber(String),
}

impl ValueDialogState {
    pub fn handle_key(&mut self, screen: Rect, code: KeyCode) -> DialogOutcome {
        match self {
            Self::Choice(choice) => {
                let page = usize::from(dialog_layout(screen).document.height).max(1);
                match code {
                    KeyCode::Esc => DialogOutcome::Cancel,
                    KeyCode::Enter => choice
                        .highlighted_id
                        .clone()
                        .map_or(DialogOutcome::Continue, |id| choice.choose(&id)),
                    KeyCode::Up | KeyCode::PageUp => {
                        choice
                            .move_highlight(false, if code == KeyCode::PageUp { page } else { 1 });
                        choice.reveal_highlight(screen);
                        DialogOutcome::Continue
                    }
                    KeyCode::Down | KeyCode::PageDown => {
                        choice
                            .move_highlight(true, if code == KeyCode::PageDown { page } else { 1 });
                        choice.reveal_highlight(screen);
                        DialogOutcome::Continue
                    }
                    KeyCode::Char(_)
                    | KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Home
                    | KeyCode::End => {
                        let previous = choice.search.buf.clone();
                        let _ = text_prompt::handle_key(&mut choice.search, code);
                        if choice.search.buf != previous {
                            choice.reconcile_highlight();
                            choice.reveal_highlight(screen);
                            choice.notice = None;
                        }
                        DialogOutcome::Continue
                    }
                    _ => DialogOutcome::Continue,
                }
            }
            Self::Number(number) => {
                let previous = number.draft.buf.clone();
                match text_prompt::handle_key(&mut number.draft, code) {
                    PromptOutcome::Commit => DialogOutcome::CommitNumber(number.draft.buf.clone()),
                    PromptOutcome::Cancel => DialogOutcome::Cancel,
                    PromptOutcome::Continue => {
                        if number.draft.buf != previous {
                            number.error = None;
                        }
                        DialogOutcome::Continue
                    }
                }
            }
        }
    }

    /// Paste inserts one complete line at the current character cursor. A
    /// multiline/control-character paste is rejected atomically, never cut.
    pub fn paste(&mut self, screen: Rect, text: &str) -> DialogOutcome {
        if text.chars().any(char::is_control) {
            match self {
                Self::Choice(choice) => choice.notice = Some("Paste one line only".into()),
                Self::Number(number) => number.error = Some("Paste one line only".into()),
            }
            return DialogOutcome::Continue;
        }
        let editor = match self {
            Self::Choice(choice) => &mut choice.search,
            Self::Number(number) => &mut number.draft,
        };
        editor.cursor = editor.cursor.min(editor.buf.chars().count());
        let byte = editor
            .buf
            .char_indices()
            .nth(editor.cursor)
            .map_or(editor.buf.len(), |(index, _)| index);
        editor.buf.insert_str(byte, text);
        editor.cursor = editor.cursor.saturating_add(text.chars().count());
        match self {
            Self::Choice(choice) => {
                choice.reconcile_highlight();
                choice.reveal_highlight(screen);
                choice.notice = None;
            }
            Self::Number(number) => number.error = None,
        }
        DialogOutcome::Continue
    }

    /// Only a left-button press activates a dialog target. The caller consumes
    /// every other pointer event while this modal owns input.
    pub fn handle_pointer(
        &mut self,
        screen: Rect,
        position: Position,
        button: PointerButton,
    ) -> DialogOutcome {
        if button != PointerButton::Left {
            return DialogOutcome::Continue;
        }
        let hit = PreparedValueDialog::new(screen, self).hit(position);
        match hit {
            DialogHit::Outside | DialogHit::Cancel => DialogOutcome::Cancel,
            DialogHit::Submit => match self {
                Self::Choice(choice) => choice
                    .highlighted_id
                    .clone()
                    .map_or(DialogOutcome::Continue, |id| choice.choose(&id)),
                Self::Number(number) => DialogOutcome::CommitNumber(number.draft.buf.clone()),
            },
            DialogHit::Choice(id) => match self {
                Self::Choice(choice) => choice.choose(&id),
                Self::Number(_) => DialogOutcome::Continue,
            },
            DialogHit::Editor => {
                match self {
                    Self::Choice(choice) => {
                        choice.search.cursor = choice.search.buf.chars().count()
                    }
                    Self::Number(number) => number.draft.cursor = number.draft.buf.chars().count(),
                }
                DialogOutcome::Continue
            }
            DialogHit::Inert => DialogOutcome::Continue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogLayout {
    pub popup: Rect,
    pub title: Rect,
    pub editor: Rect,
    pub document: Rect,
    pub status: Rect,
    pub footer: Rect,
    pub submit: Rect,
    pub cancel: Rect,
}

pub fn dialog_layout(screen: Rect) -> DialogLayout {
    let width = screen.width.saturating_sub(2).min(80);
    let height = screen.height.saturating_sub(2).min(28);
    let popup = Rect::new(
        screen.x.saturating_add((screen.width - width) / 2),
        screen.y.saturating_add((screen.height - height) / 2),
        width,
        height,
    );
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let row = |index: u16| {
        if index < inner.height {
            Rect::new(inner.x, inner.y.saturating_add(index), inner.width, 1)
        } else {
            Rect::default()
        }
    };
    let footer = if inner.height >= 4 {
        row(inner.height - 1)
    } else {
        Rect::default()
    };
    let submit_width = footer.width.min(10).min(footer.width / 2);
    let cancel_width = footer.width.saturating_sub(submit_width).min(10);
    DialogLayout {
        popup,
        title: row(0),
        editor: row(1),
        document: Rect::new(
            inner.x,
            inner.y.saturating_add(2),
            inner.width,
            inner.height.saturating_sub(4),
        ),
        status: if inner.height >= 4 {
            row(inner.height - 2)
        } else {
            Rect::default()
        },
        footer,
        submit: Rect::new(footer.x, footer.y, submit_width, footer.height),
        cancel: Rect::new(
            footer.right().saturating_sub(cancel_width),
            footer.y,
            cancel_width,
            footer.height,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogHit {
    Outside,
    Cancel,
    Submit,
    Editor,
    Choice(String),
    Inert,
}

#[derive(Debug, Clone)]
struct ChoiceLine {
    option_id: Option<String>,
    text: String,
    is_reason: bool,
    is_current: bool,
    is_highlighted: bool,
    is_disabled: bool,
}

fn choice_rows(state: &ChoiceDialogState, width: u16) -> Vec<ChoiceLine> {
    let mut rows = Vec::new();
    for index in state.filtered_indices() {
        let option = &state.options[index];
        let is_current = state.selected_id.as_deref() == Some(option.id.as_str());
        let is_highlighted = state.highlighted_id.as_deref() == Some(option.id.as_str());
        let is_disabled = option.disabled_reason.is_some();
        let marker = if is_current { "●" } else { " " };
        let suffix = if is_disabled { " [unavailable]" } else { "" };
        let header = format!("{}{suffix}", option.label);
        let header_lines = if width >= 3 {
            wrap_cells(&header, width - 2)
        } else {
            vec![clip_cells(&format!("{marker} {header}"), width)]
        };
        for (line_number, line) in header_lines.into_iter().enumerate() {
            let text = if width >= 3 {
                format!("{} {line}", if line_number == 0 { marker } else { " " })
            } else {
                line
            };
            rows.push(ChoiceLine {
                option_id: Some(option.id.clone()),
                text,
                is_reason: false,
                is_current,
                is_highlighted,
                is_disabled,
            });
        }
        if let Some(reason) = &option.disabled_reason {
            let explanation = if reason.trim().is_empty() {
                "Unavailable".to_owned()
            } else {
                format!("Unavailable: {reason}")
            };
            let indent = if width >= 3 { "  " } else { "" };
            for text in wrap_cells(&explanation, width.saturating_sub(indent.len() as u16)) {
                rows.push(ChoiceLine {
                    option_id: Some(option.id.clone()),
                    text: format!("{indent}{text}"),
                    is_reason: true,
                    is_current,
                    is_highlighted,
                    is_disabled,
                });
            }
        }
    }
    if rows.is_empty() {
        rows.push(ChoiceLine {
            option_id: None,
            text: clip_cells("No matching options", width),
            is_reason: false,
            is_current: false,
            is_highlighted: false,
            is_disabled: false,
        });
    }
    rows
}

/// Wraps every printable grapheme; no option or reason is cut to a fixed row
/// count. A terminal narrower than a wide grapheme shows an ellipsis for it.
fn wrap_cells(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let limit = usize::from(width);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for source in UnicodeSegmentation::graphemes(text, true) {
        let grapheme = if source.chars().any(char::is_control) {
            " "
        } else {
            source
        };
        let cells = UnicodeWidthStr::width(grapheme);
        if cells == 0 {
            continue;
        }
        if used + cells > limit && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if cells > limit {
            lines.push("…".into());
        } else {
            line.push_str(grapheme);
            used += cells;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[derive(Debug, Clone, Copy)]
pub struct DialogStyles {
    pub background: Style,
    pub normal: Style,
    pub highlighted: Style,
    pub current: Style,
    pub disabled: Style,
    pub error: Style,
}

impl Default for DialogStyles {
    fn default() -> Self {
        Self {
            background: Style::default(),
            normal: Style::default(),
            highlighted: Style::default().add_modifier(Modifier::REVERSED),
            current: Style::default().add_modifier(Modifier::BOLD),
            disabled: Style::default().add_modifier(Modifier::DIM),
            error: Style::default().fg(Color::Red),
        }
    }
}

/// Immutable one-frame presentation. `hit` reads exactly the rows `render`
/// paints, including wrapped disabled reasons and the same clamped scroll.
pub struct PreparedValueDialog<'a> {
    state: &'a ValueDialogState,
    layout: DialogLayout,
    rows: Vec<ChoiceLine>,
    first_line: usize,
}

impl<'a> PreparedValueDialog<'a> {
    pub fn new(screen: Rect, state: &'a ValueDialogState) -> Self {
        let layout = dialog_layout(screen);
        let (rows, first_line) = match state {
            ValueDialogState::Choice(choice) => {
                let rows = choice_rows(choice, layout.document.width.saturating_sub(1));
                let maximum = rows
                    .len()
                    .saturating_sub(usize::from(layout.document.height));
                let first_line = choice.scroll_line.min(maximum);
                (rows, first_line)
            }
            ValueDialogState::Number(_) => (Vec::new(), 0),
        };
        Self {
            state,
            layout,
            rows,
            first_line,
        }
    }

    pub fn layout(&self) -> DialogLayout {
        self.layout
    }

    pub fn hit(&self, position: Position) -> DialogHit {
        if !self.layout.popup.contains(position) {
            return DialogHit::Outside;
        }
        if self.layout.cancel.contains(position) {
            return DialogHit::Cancel;
        }
        if self.layout.submit.contains(position) {
            return DialogHit::Submit;
        }
        if self.layout.editor.contains(position) {
            return DialogHit::Editor;
        }
        if self.layout.document.contains(position)
            && matches!(self.state, ValueDialogState::Choice(_))
            && position.x < self.layout.document.right().saturating_sub(1)
        {
            let index =
                self.first_line + usize::from(position.y.saturating_sub(self.layout.document.y));
            return self
                .rows
                .get(index)
                .and_then(|row| row.option_id.clone())
                .map_or(DialogHit::Inert, DialogHit::Choice);
        }
        DialogHit::Inert
    }

    pub fn render(&self, frame: &mut Frame, styles: DialogStyles) {
        if self.layout.popup.width == 0 || self.layout.popup.height == 0 {
            return;
        }
        frame.render_widget(Clear, self.layout.popup);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .style(styles.background),
            self.layout.popup,
        );
        let title = match self.state {
            ValueDialogState::Choice(choice) => &choice.title,
            ValueDialogState::Number(number) => &number.title,
        };
        paint(
            frame,
            self.layout.title,
            &clip_cells(title, self.layout.title.width),
            styles.current,
        );
        match self.state {
            ValueDialogState::Choice(choice) => self.render_choice(frame, choice, styles),
            ValueDialogState::Number(number) => self.render_number(frame, number, styles),
        }
        let submit_label = match self.state {
            ValueDialogState::Choice(_) => "[ Select ]",
            ValueDialogState::Number(_) => "[ Apply ]",
        };
        paint(
            frame,
            self.layout.submit,
            &clip_cells(submit_label, self.layout.submit.width),
            styles.highlighted,
        );
        paint(
            frame,
            self.layout.cancel,
            &clip_cells("[ Cancel ]", self.layout.cancel.width),
            styles.normal,
        );
    }

    fn render_choice(&self, frame: &mut Frame, choice: &ChoiceDialogState, styles: DialogStyles) {
        let search = editor_text("Search: ", &choice.search, self.layout.editor.width);
        paint(frame, self.layout.editor, &search, styles.normal);
        let body_width = self.layout.document.width.saturating_sub(1);
        for (visible, row) in self
            .rows
            .iter()
            .skip(self.first_line)
            .take(usize::from(self.layout.document.height))
            .enumerate()
        {
            let area = Rect::new(
                self.layout.document.x,
                self.layout.document.y.saturating_add(visible as u16),
                body_width,
                1,
            );
            let mut style = if row.is_highlighted {
                styles.highlighted
            } else if row.is_current {
                styles.current
            } else {
                styles.normal
            };
            if row.is_disabled || row.is_reason {
                style = style.add_modifier(Modifier::DIM);
            }
            paint(frame, area, &row.text, style);
        }
        self.render_scrollbar(frame, styles.disabled);
        let current = choice.selected_id.as_ref().map_or_else(
            || "None".to_owned(),
            |id| {
                choice
                    .options
                    .iter()
                    .find(|option| &option.id == id)
                    .map_or_else(
                        || format!("{id} (unavailable)"),
                        |option| option.label.clone(),
                    )
            },
        );
        let status = choice.notice.clone().unwrap_or_else(|| {
            format!(
                "{} of {} options · Current: {}",
                choice.filtered_indices().len(),
                choice.options.len(),
                current
            )
        });
        paint(
            frame,
            self.layout.status,
            &clip_cells(&status, self.layout.status.width),
            if choice.notice.is_some() {
                styles.error
            } else {
                styles.normal
            },
        );
    }

    fn render_number(&self, frame: &mut Frame, number: &NumberDialogState, styles: DialogStyles) {
        let value = editor_text("Value: ", &number.draft, self.layout.editor.width);
        paint(frame, self.layout.editor, &value, styles.normal);
        if let Some(error) = &number.error {
            if self.layout.document.width > 0 && self.layout.document.height > 0 {
                frame.render_widget(
                    Paragraph::new(error.as_str())
                        .style(styles.error)
                        .wrap(Wrap { trim: false }),
                    self.layout.document,
                );
            }
        }
        paint(
            frame,
            self.layout.status,
            &clip_cells("Enter applies · Esc cancels", self.layout.status.width),
            styles.normal,
        );
    }

    fn render_scrollbar(&self, frame: &mut Frame, style: Style) {
        let visible = usize::from(self.layout.document.height);
        if self.rows.len() <= visible || visible == 0 || self.layout.document.width == 0 {
            return;
        }
        let maximum = self.rows.len() - visible;
        let thumb = self.first_line.saturating_mul(visible.saturating_sub(1)) / maximum;
        let x = self.layout.document.right().saturating_sub(1);
        for line in 0..visible {
            paint(
                frame,
                Rect::new(x, self.layout.document.y.saturating_add(line as u16), 1, 1),
                if line == thumb { "┃" } else { "│" },
                style,
            );
        }
    }
}

fn paint(frame: &mut Frame, area: Rect, text: &str, style: Style) {
    if area.width > 0 && area.height > 0 {
        frame.render_widget(Paragraph::new(text).style(style), area);
    }
}

/// Keep the editing caret visible without changing the saved draft. The
/// actual cursor remains a character index, matching `TextPromptState`.
fn editor_text(prefix: &str, editor: &TextPromptState, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let cursor = editor.cursor.min(editor.buf.chars().count());
    let byte = editor
        .buf
        .char_indices()
        .nth(cursor)
        .map_or(editor.buf.len(), |(index, _)| index);
    let left = format!("{prefix}{}", &editor.buf[..byte]);
    let right = &editor.buf[byte..];
    let complete = format!("{left}│{right}");
    if cell_width(&complete) <= usize::from(width) {
        return clip_cells(&complete, width);
    }
    let budget = usize::from(width).saturating_sub(1);
    let left_budget = budget / 2;
    let mut left_parts = Vec::new();
    let mut left_cells = 0;
    for grapheme in UnicodeSegmentation::graphemes(left.as_str(), true).rev() {
        let cells = UnicodeWidthStr::width(grapheme);
        if left_cells + cells > left_budget {
            break;
        }
        left_parts.push(grapheme);
        left_cells += cells;
    }
    left_parts.reverse();
    let left_visible = left_parts.concat();
    let right_visible = clip_cells(right, (budget - left_cells) as u16);
    format!(
        "{}│{right_visible}",
        clip_cells(&left_visible, left_cells as u16)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn option(id: &str, label: &str, reason: Option<&str>) -> ChoiceOption {
        ChoiceOption {
            id: id.into(),
            label: label.into(),
            disabled_reason: reason.map(str::to_owned),
        }
    }

    fn choices() -> ChoiceDialogState {
        ChoiceDialogState::new(
            "Choose palette",
            vec![
                option("a", "Amber", None),
                option(
                    "b",
                    "Blue",
                    Some("A GPU device is required for this option."),
                ),
                option("c", "Cyan", None),
            ],
            Some("a".into()),
        )
        .unwrap()
    }

    #[test]
    fn ids_are_unique_and_selection_survives_filtering_and_catalog_refresh() {
        assert!(ChoiceDialogState::new("x", vec![option("", "A", None)], None).is_err());
        assert!(ChoiceDialogState::new(
            "x",
            vec![option("a", "A", None), option("a", "B", None)],
            None
        )
        .is_err());
        let mut state = choices();
        state.search = TextPromptState::new("blue");
        state.reconcile_highlight();
        assert_eq!(state.highlighted_id.as_deref(), Some("b"));
        assert_eq!(state.selected_id.as_deref(), Some("a"));
        state
            .replace_options(vec![option("b", "Indigo", None)])
            .unwrap();
        assert_eq!(state.selected_id.as_deref(), Some("a"));
        assert_eq!(state.highlighted_id, None);
        assert_eq!(state.options()[0].id, "b");
    }

    #[test]
    fn search_includes_disabled_reasons_and_never_drops_full_options() {
        let mut state = choices();
        state.search = TextPromptState::new("GPU");
        state.reconcile_highlight();
        let rows = choice_rows(&state, 72);
        assert!(rows.iter().all(|row| row.option_id.as_deref() == Some("b")));
        assert!(rows.iter().any(|row| row.text.contains("Blue")));
        assert!(rows.iter().any(|row| row.text.contains("GPU device")));
        assert_eq!(state.options().len(), 3);
    }

    #[test]
    fn long_choice_labels_wrap_without_losing_the_option_id() {
        let state = ChoiceDialogState::new(
            "Long labels",
            vec![option(
                "stable",
                "An option label longer than this narrow dialog",
                None,
            )],
            None,
        )
        .unwrap();
        let rows = choice_rows(&state, 12);
        assert!(rows.len() > 1);
        assert!(rows
            .iter()
            .all(|row| row.option_id.as_deref() == Some("stable")));
        let recovered: String = rows
            .iter()
            .map(|row| row.text.strip_prefix("  ").unwrap_or(&row.text))
            .collect();
        assert_eq!(recovered, "An option label longer than this narrow dialog");
    }

    #[test]
    fn disabled_choice_stays_open_with_reason_and_enabled_choice_emits_id() {
        let screen = Rect::new(0, 0, 100, 40);
        let mut dialog = ValueDialogState::Choice(choices());
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Down),
            DialogOutcome::Continue
        );
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Enter),
            DialogOutcome::Continue
        );
        let ValueDialogState::Choice(state) = &dialog else {
            panic!("choice state retained");
        };
        assert_eq!(
            state.notice.as_deref(),
            Some("A GPU device is required for this option.")
        );
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Down),
            DialogOutcome::Continue
        );
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Enter),
            DialogOutcome::Choose("c".into())
        );
    }

    #[test]
    fn page_navigation_and_scrolling_reach_the_last_option() {
        let screen = Rect::new(0, 0, 30, 10);
        let options = (0..100)
            .map(|index| option(&index.to_string(), &format!("Choice {index}"), None))
            .collect();
        let mut dialog = ValueDialogState::Choice(
            ChoiceDialogState::new("Many", options, Some("0".into())).unwrap(),
        );
        for _ in 0..100 {
            dialog.handle_key(screen, KeyCode::PageDown);
        }
        let ValueDialogState::Choice(state) = &dialog else {
            panic!("choice state retained");
        };
        assert_eq!(state.highlighted_id.as_deref(), Some("99"));
        assert!(state.scroll_line > 0);
        let prepared = PreparedValueDialog::new(screen, &dialog);
        assert!(prepared.rows[prepared.first_line..]
            .iter()
            .take(usize::from(prepared.layout.document.height))
            .any(|row| row.option_id.as_deref() == Some("99")));
    }

    #[test]
    fn wrapped_reason_rows_hit_the_same_stable_choice_id() {
        let screen = Rect::new(0, 0, 42, 14);
        let dialog = ValueDialogState::Choice(choices());
        let prepared = PreparedValueDialog::new(screen, &dialog);
        let reason_index = prepared
            .rows
            .iter()
            .position(|row| row.option_id.as_deref() == Some("b") && row.is_reason)
            .unwrap();
        let y = prepared.layout.document.y + reason_index as u16;
        assert_eq!(
            prepared.hit(Position::new(prepared.layout.document.x, y)),
            DialogHit::Choice("b".into())
        );
        assert_eq!(
            prepared.hit(Position::new(prepared.layout.document.right() - 1, y)),
            DialogHit::Inert,
            "scrollbar column never selects an option"
        );
    }

    #[test]
    fn numeric_rejection_keeps_exact_draft_cursor_and_dialog() {
        let screen = Rect::new(0, 0, 80, 24);
        let mut dialog = ValueDialogState::Number(NumberDialogState::new("Rate", "17"));
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Enter),
            DialogOutcome::CommitNumber("17".into())
        );
        let ValueDialogState::Number(number) = &mut dialog else {
            panic!("number dialog retained");
        };
        number.reject("Must be at least 20");
        let cursor = number.draft.cursor;
        assert_eq!(number.draft.buf, "17");
        assert_eq!(number.error.as_deref(), Some("Must be at least 20"));
        assert_eq!(number.draft.cursor, cursor);
        dialog.handle_key(screen, KeyCode::Char('5'));
        let ValueDialogState::Number(number) = &dialog else {
            panic!("number dialog retained");
        };
        assert_eq!(number.draft.buf, "175");
        assert_eq!(number.error, None);
        assert_eq!(
            dialog.handle_key(screen, KeyCode::Esc),
            DialogOutcome::Cancel
        );
    }

    #[test]
    fn paste_is_atomic_on_multiline_input_and_preserves_exact_single_line() {
        let screen = Rect::new(0, 0, 80, 24);
        let mut dialog = ValueDialogState::Number(NumberDialogState::new("Budget", "1"));
        dialog.paste(screen, "e2");
        let ValueDialogState::Number(number) = &dialog else {
            panic!("number dialog retained");
        };
        assert_eq!(number.draft.buf, "1e2");
        dialog.paste(screen, "3\n4");
        let ValueDialogState::Number(number) = &dialog else {
            panic!("number dialog retained");
        };
        assert_eq!(number.draft.buf, "1e2");
        assert_eq!(number.error.as_deref(), Some("Paste one line only"));
    }

    #[test]
    fn rendering_and_pointer_hit_use_the_same_scrolled_document() {
        let screen = Rect::new(0, 0, 60, 9);
        let mut dialog = ValueDialogState::Choice(choices());
        let ValueDialogState::Choice(choice) = &mut dialog else {
            panic!("choice state retained");
        };
        choice.scroll_line = 1;
        let prepared = PreparedValueDialog::new(screen, &dialog);
        assert_eq!(prepared.first_line, 1);
        let mut terminal = Terminal::new(TestBackend::new(60, 9)).unwrap();
        terminal
            .draw(|frame| prepared.render(frame, DialogStyles::default()))
            .unwrap();
        let first_id = prepared.rows[prepared.first_line]
            .option_id
            .clone()
            .unwrap();
        let first_y = prepared.layout.document.y;
        assert_eq!(
            prepared.hit(Position::new(prepared.layout.document.x, first_y)),
            DialogHit::Choice(first_id)
        );
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer
                .cell((prepared.layout.document.x + 2, first_y))
                .unwrap()
                .symbol(),
            "B"
        );
        assert_eq!(
            prepared.hit(Position::new(
                prepared.layout.title.x,
                prepared.layout.title.y
            )),
            DialogHit::Inert
        );
    }

    #[test]
    fn narrow_or_empty_screens_never_create_hidden_targets() {
        let dialog = ValueDialogState::Number(NumberDialogState::new("Count", "12"));
        for screen in [Rect::new(0, 0, 0, 0), Rect::new(2, 4, 3, 3)] {
            let prepared = PreparedValueDialog::new(screen, &dialog);
            assert_eq!(prepared.layout.submit.width, 0);
            assert_eq!(prepared.layout.cancel.width, 0);
            assert_eq!(
                prepared.hit(Position::new(screen.x, screen.y)),
                DialogHit::Outside
            );
        }
    }

    #[test]
    fn outside_left_click_cancels_without_submitting() {
        let screen = Rect::new(0, 0, 80, 24);
        let mut dialog = ValueDialogState::Number(NumberDialogState::new("Count", "12"));
        assert_eq!(
            dialog.handle_pointer(screen, Position::new(0, 0), PointerButton::Right),
            DialogOutcome::Continue
        );
        assert_eq!(
            dialog.handle_pointer(screen, Position::new(0, 0), PointerButton::Left),
            DialogOutcome::Cancel
        );
    }
}
