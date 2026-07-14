//! Full-screen workspace search over client-owned editor buffers and the
//! server-replayed terminal byte histories held by [`crate::terminal_view`].
//!
//! This module owns presentation state and text matching only. `App` owns
//! the actual pane map/tree metadata and performs result activation, keeping
//! search independent from PTY ownership and IPC transport details.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_core::NodeId;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::text_prompt::TextPromptState;
use crate::theme;

/// The concrete object type behind a result. It controls the compact icon and
/// tells `App` how the selected result should be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchObjectKind {
    Agent,
    Shell,
    File,
    Board,
}

impl SearchObjectKind {
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Agent => "◈",
            Self::Shell => "▸",
            Self::File => "▤",
            Self::Board => "▦",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Agent => "AGENT",
            Self::Shell => "SHELL",
            Self::File => "FILE",
            Self::Board => "BOARD",
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::Agent => Color::Rgb(0xd9, 0x77, 0x57),
            Self::Shell => Color::Rgb(0xa9, 0xb1, 0xd6),
            Self::File => Color::Magenta,
            Self::Board => Color::Cyan,
        }
    }
}

/// A direct navigation point in a local pane buffer or a terminal's raw
/// retained output stream. Terminal offsets are exclusive byte ends so the
/// matching text has already been fed when the view rebuilds around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchLocation {
    Terminal { history_end_byte: usize },
    Editor { line: usize },
    Board,
}

/// A compact, display-ready search hit. Splitting surrounding context around
/// the match lets rendering highlight only the matched term without parsing
/// terminal escape sequences in the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub pane_id: NodeId,
    pub kind: SearchObjectKind,
    pub object_name: String,
    pub automatic_title: Option<String>,
    pub path: Option<PathBuf>,
    pub last_command: Option<String>,
    pub before: String,
    pub matched: String,
    pub after: String,
    pub location: SearchLocation,
}

/// One immutable source handed from the UI thread to the search worker.
/// Terminal histories share their retained byte buffer through `Arc`; local
/// buffers are copied only when a debounced scan is actually due.
#[derive(Debug, Clone)]
pub struct WorkspaceSearchSource {
    pub pane_id: NodeId,
    pub kind: SearchObjectKind,
    pub object_name: String,
    pub automatic_title: Option<String>,
    pub path: Option<PathBuf>,
    pub last_command: Option<String>,
    pub content: WorkspaceSearchContent,
}

/// Searchable content owned by one source snapshot.
#[derive(Debug, Clone)]
pub enum WorkspaceSearchContent {
    Terminal(Arc<Vec<u8>>),
    Text(Vec<WorkspaceSearchText>),
}

/// A local text fragment and the durable location to open once it matches.
#[derive(Debug, Clone)]
pub struct WorkspaceSearchText {
    pub text: String,
    pub location: SearchLocation,
}

/// One background scan request. The monotonically increasing revision makes
/// late worker results harmless after the user has continued typing.
#[derive(Debug, Clone)]
pub struct WorkspaceSearchRequest {
    pub revision: u64,
    pub query: String,
    pub sources: Vec<WorkspaceSearchSource>,
}

/// Full-screen search interaction state. Results are re-derived from live
/// client caches after every query edit; no stale index survives tree/editor
/// mutations or terminal replay.
pub struct SearchState {
    pub query: TextPromptState,
    pub results: Vec<SearchResult>,
    pub selected_index: usize,
    pub scroll: usize,
    query_revision: u64,
    completed_revision: Option<u64>,
    search_in_flight_revision: Option<u64>,
    last_query_edit: Option<Instant>,
}

/// A full second of silence intentionally favors fluid input over constantly
/// rescanning multi-megabyte terminal journals for intermediate prefixes.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_secs(1);

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: TextPromptState::new(""),
            results: Vec::new(),
            selected_index: 0,
            scroll: 0,
            query_revision: 0,
            completed_revision: Some(0),
            search_in_flight_revision: None,
            last_query_edit: None,
        }
    }

    pub fn replace_results(&mut self, results: Vec<SearchResult>) {
        self.results = results;
        self.selected_index = self
            .selected_index
            .min(self.results.len().saturating_sub(1));
        self.scroll = self.scroll.min(self.selected_index);
    }

    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
    }

    pub fn move_selection(&mut self, delta: i32, visible_rows: usize) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len().saturating_sub(1) as i32;
        self.selected_index = (self.selected_index as i32 + delta).clamp(0, last) as usize;
        let visible_rows = visible_rows.max(1);
        if self.selected_index < self.scroll {
            self.scroll = self.selected_index;
        } else if self.selected_index >= self.scroll + visible_rows {
            self.scroll = self.selected_index + 1 - visible_rows;
        }
    }

    /// Records an immediate input edit without doing any search work. A
    /// pending worker may finish later, but its revision will no longer be
    /// accepted after this increment.
    pub fn note_query_changed(&mut self, now: Instant) {
        self.query_revision = self.query_revision.wrapping_add(1);
        self.completed_revision = None;
        self.last_query_edit = Some(now);
        self.results.clear();
        self.selected_index = 0;
        self.scroll = 0;
    }

    /// Claims the current query once it has been quiet long enough. The
    /// caller builds the source snapshot and starts the owned worker.
    pub fn take_due_search(&mut self, now: Instant) -> Option<(u64, String)> {
        if self.query.buf.trim().is_empty()
            || self.search_in_flight_revision.is_some()
            || self.completed_revision == Some(self.query_revision)
        {
            return None;
        }
        let last_query_edit = self.last_query_edit?;
        if now.duration_since(last_query_edit) < SEARCH_DEBOUNCE {
            return None;
        }
        self.search_in_flight_revision = Some(self.query_revision);
        Some((self.query_revision, self.query.buf.trim().to_string()))
    }

    /// Makes a failed worker launch retryable on the next tick.
    pub fn cancel_in_flight_search(&mut self, revision: u64) {
        if self.search_in_flight_revision == Some(revision) {
            self.search_in_flight_revision = None;
        }
    }

    /// Installs a worker result only when it still belongs to the visible
    /// query. This is the synchronization boundary that keeps a stale scan
    /// from replacing text the user has already moved beyond.
    pub fn complete_search(&mut self, revision: u64, results: Vec<SearchResult>) -> bool {
        if self.search_in_flight_revision == Some(revision) {
            self.search_in_flight_revision = None;
        }
        if revision != self.query_revision {
            return false;
        }
        self.completed_revision = Some(revision);
        self.replace_results(results);
        true
    }

    pub fn is_waiting_for_debounce(&self) -> bool {
        !self.query.buf.trim().is_empty()
            && self.search_in_flight_revision.is_none()
            && self.completed_revision != Some(self.query_revision)
    }

    pub fn is_searching(&self) -> bool {
        self.search_in_flight_revision.is_some()
    }
}

/// Scans an immutable workspace snapshot. It is deliberately pure and owns
/// no TUI state, so `search_workers` can run it away from key handling.
pub fn search_workspace(
    request: &WorkspaceSearchRequest,
    should_cancel: impl Fn() -> bool,
) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for source in &request.sources {
        if should_cancel() || results.len() >= MAX_RESULTS {
            break;
        }
        match &source.content {
            WorkspaceSearchContent::Terminal(history) => {
                let last_command = terminal_last_command(history);
                find_terminal_results(
                    history,
                    &request.query,
                    |before, matched, after, history_end_byte| {
                        result_from_source(
                            source,
                            before,
                            matched,
                            after,
                            SearchLocation::Terminal { history_end_byte },
                            last_command.clone(),
                        )
                    },
                    &mut results,
                );
            }
            WorkspaceSearchContent::Text(entries) => {
                for entry in entries {
                    if should_cancel() || results.len() >= MAX_RESULTS {
                        break;
                    }
                    let location = entry.location;
                    let line = match location {
                        SearchLocation::Editor { line } => line,
                        SearchLocation::Terminal { .. } | SearchLocation::Board => 0,
                    };
                    find_text_results(
                        &entry.text,
                        &request.query,
                        |before, matched, after, _| {
                            result_from_source(source, before, matched, after, location, None)
                        },
                        line,
                        &mut results,
                    );
                }
            }
        }
    }
    results
}

fn result_from_source(
    source: &WorkspaceSearchSource,
    before: &str,
    matched: &str,
    after: &str,
    location: SearchLocation,
    last_command: Option<String>,
) -> SearchResult {
    SearchResult {
        pane_id: source.pane_id,
        kind: source.kind,
        object_name: source.object_name.clone(),
        automatic_title: source.automatic_title.clone(),
        path: source.path.clone(),
        last_command: last_command.or_else(|| source.last_command.clone()),
        before: before.to_string(),
        matched: matched.to_string(),
        after: after.to_string(),
        location,
    }
}

/// Finds all bounded, case-insensitive matches in a plain local document.
/// `line` is used for editors; callers may use zero for a one-line board
/// card. Returning a maximum keeps a giant generated file from starving the
/// interactive event loop while still reporting a useful exhaustive spread.
pub fn find_text_results(
    text: &str,
    query: &str,
    make_result: impl Fn(&str, &str, &str, usize) -> SearchResult,
    line: usize,
    results: &mut Vec<SearchResult>,
) {
    if query.is_empty() || results.len() >= MAX_RESULTS {
        return;
    }
    let folded_text = text.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = folded_text[start..].find(&folded_query) {
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if !text.is_char_boundary(match_start) || !text.is_char_boundary(match_end) {
            start = match_end;
            continue;
        }
        let (before, matched, after) = context_parts(text, match_start, match_end);
        results.push(make_result(before, matched, after, line));
        if results.len() >= MAX_RESULTS {
            return;
        }
        start = match_end.max(start + 1);
        if start >= folded_text.len() {
            return;
        }
    }
}

/// Searches raw terminal history after removing control sequences. The byte
/// offset table preserves a route back into the original raw PTY stream, so
/// click-to-open can recreate the visual terminal exactly around a hit.
pub fn find_terminal_results(
    history: &[u8],
    query: &str,
    make_result: impl Fn(&str, &str, &str, usize) -> SearchResult,
    results: &mut Vec<SearchResult>,
) {
    if query.is_empty() || results.len() >= MAX_RESULTS {
        return;
    }
    let searchable = strip_terminal_controls(history);
    let folded_text = searchable.text.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = folded_text[start..].find(&folded_query) {
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if !searchable.text.is_char_boundary(match_start)
            || !searchable.text.is_char_boundary(match_end)
        {
            start = match_end;
            continue;
        }
        let (before, matched, after) = context_parts(&searchable.text, match_start, match_end);
        let history_end_byte = searchable
            .raw_ends
            .get(match_end.saturating_sub(1))
            .copied()
            .unwrap_or(history.len());
        results.push(make_result(before, matched, after, history_end_byte));
        if results.len() >= MAX_RESULTS {
            return;
        }
        start = match_end.max(start + 1);
        if start >= folded_text.len() {
            return;
        }
    }
}

pub fn terminal_last_command(history: &[u8]) -> Option<String> {
    let text = strip_terminal_controls(history).text;
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        ["$ ", "❯ ", "> "]
            .iter()
            .find_map(|prompt| line.strip_prefix(prompt))
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(|command| truncate(command, 72))
    })
}

/// Draws the full-screen result browser. The intentionally border-light
/// layout gives the query room to breathe while keeping object facts and
/// evidence dense enough to scan without opening every hit.
pub fn render(frame: &mut Frame, area: Rect, state: &SearchState) {
    frame.render_widget(Clear, area);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);
    render_header(frame, vertical[0], state);
    render_summary(frame, vertical[1], state);
    render_results(frame, vertical[2], state);
    render_footer(frame, vertical[3]);
}

pub fn result_at(area: Rect, state: &SearchState, position: Position) -> Option<usize> {
    let results_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area)[2];
    if !results_area.contains(position) {
        return None;
    }
    let relative_row = usize::from(position.y.saturating_sub(results_area.y));
    let result_index = state.scroll + relative_row / RESULT_HEIGHT;
    (relative_row % RESULT_HEIGHT < RESULT_HEIGHT && result_index < state.results.len())
        .then_some(result_index)
}

pub fn visible_result_rows(area: Rect) -> usize {
    let result_height = area.height.saturating_sub(7);
    (usize::from(result_height) / RESULT_HEIGHT).max(1)
}

const RESULT_HEIGHT: usize = 5;
const MAX_RESULTS: usize = 800;

fn render_header(frame: &mut Frame, area: Rect, state: &SearchState) {
    let header = Line::from(vec![
        Span::styled(
            "⌕  Workspace Search",
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "   every agent, shell, retained terminal history, and open file",
            Style::new().add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), area);
    let input_area = Rect::new(area.x, area.y.saturating_add(1), area.width, 3);
    let block = theme::block(true);
    let inner = block.inner(input_area);
    frame.render_widget(block, input_area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let prompt = "⌕  ";
    let prompt_width = u16::try_from(prompt.width()).unwrap_or(u16::MAX);
    let available_width = inner.width.saturating_sub(prompt_width);
    let (query, cursor_offset, query_style) = if state.query.buf.is_empty() {
        (
            "Type to search everything…".to_string(),
            0,
            Style::new().fg(Color::DarkGray),
        )
    } else {
        let (visible, cursor_offset) = visible_query(&state.query, available_width);
        (visible, cursor_offset, Style::new().fg(Color::White))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                prompt,
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(query, query_style),
        ])),
        inner,
    );
    // This is the actual terminal cursor, rather than a painted glyph. It
    // remains aligned with the editable `TextPromptState` even while result
    // scans run elsewhere, which makes the field behave like a real input.
    frame.set_cursor_position(Position::new(
        inner
            .x
            .saturating_add(prompt_width)
            .saturating_add(cursor_offset)
            .min(inner.right().saturating_sub(1)),
        inner.y,
    ));
}

fn render_summary(frame: &mut Frame, area: Rect, state: &SearchState) {
    let text = if state.query.buf.is_empty() {
        "Start with a word or phrase. Search is live and case-insensitive.".to_string()
    } else if state.is_waiting_for_debounce() {
        "Waiting briefly for you to finish typing before searching…".to_string()
    } else if state.is_searching() {
        "Searching retained workspace history in the background…".to_string()
    } else if state.results.is_empty() {
        format!(
            "No matches for “{}” in the currently retained workspace.",
            state.query.buf
        )
    } else {
        format!(
            "{} matches · Enter opens the exact place · Esc returns to your workspace",
            state.results.len()
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::new().add_modifier(Modifier::DIM),
        ))),
        area,
    );
}

/// Returns the horizontally visible query fragment and the cursor's cell
/// offset within it. Reserving the final cell lets the native cursor remain
/// visible at the end of a long query instead of disappearing beyond the
/// clipped input area.
fn visible_query(state: &TextPromptState, available_width: u16) -> (String, u16) {
    if available_width == 0 {
        return (String::new(), 0);
    }
    let characters: Vec<char> = state.buf.chars().collect();
    let cursor = state.cursor.min(characters.len());
    let content_width = available_width.saturating_sub(1).max(1) as usize;
    let mut start = cursor;
    let mut used_width: usize = 0;
    while start > 0 {
        let character_width = characters[start - 1].to_string().width();
        if used_width.saturating_add(character_width) > content_width {
            break;
        }
        start -= 1;
        used_width += character_width;
    }
    let mut text = String::new();
    let mut rendered_width: usize = 0;
    for character in &characters[start..] {
        let character_width = character.to_string().width();
        if rendered_width.saturating_add(character_width) > content_width {
            break;
        }
        text.push(*character);
        rendered_width += character_width;
    }
    let cursor_offset = characters[start..cursor]
        .iter()
        .map(|character| character.to_string().width())
        .sum::<usize>();
    (text, u16::try_from(cursor_offset).unwrap_or(u16::MAX))
}

fn render_results(frame: &mut Frame, area: Rect, state: &SearchState) {
    if state.results.is_empty() {
        return;
    }
    let visible_count = (usize::from(area.height) / RESULT_HEIGHT).max(1);
    for (visible_index, result) in state
        .results
        .iter()
        .skip(state.scroll)
        .take(visible_count)
        .enumerate()
    {
        let y = area
            .y
            .saturating_add((visible_index * RESULT_HEIGHT) as u16);
        let row_area = Rect::new(area.x, y, area.width, RESULT_HEIGHT as u16);
        let selected = state.scroll + visible_index == state.selected_index;
        let selection_style = if selected {
            theme::selected_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let meta = result_metadata(result);
        let lines = vec![
            Line::from(vec![
                Span::styled("  ", selection_style),
                Span::styled(
                    format!("{} {}", result.kind.icon(), result.kind.label()),
                    Style::new()
                        .fg(result.kind.color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {}", result.object_name), selection_style),
            ]),
            Line::from(Span::styled(
                format!("       {meta}"),
                Style::new().add_modifier(Modifier::DIM),
            )),
            Line::from(vec![
                Span::raw("       "),
                Span::styled(&result.before, Style::new().fg(Color::Gray)),
                Span::styled(
                    &result.matched,
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(&result.after, Style::new().fg(Color::Gray)),
            ]),
            Line::from(""),
            Line::from(""),
        ];
        frame.render_widget(Paragraph::new(lines).style(selection_style), row_area);
    }
    if state.results.len() > visible_count {
        let mut scrollbar_state = ScrollbarState::new(state.results.len()).position(state.scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(" "))
                .style(theme::border_style(false)),
            area,
            &mut scrollbar_state,
        );
    }
}

fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "↑↓",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select   "),
            Span::styled(
                "Enter",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" jump to result   "),
            Span::styled(
                "Esc",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" close"),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}

fn result_metadata(result: &SearchResult) -> String {
    let mut facts = Vec::new();
    if let Some(title) = &result.automatic_title {
        facts.push(format!("AI title: {title}"));
    }
    if let Some(path) = &result.path {
        facts.push(path.display().to_string());
    }
    if let Some(command) = &result.last_command {
        facts.push(format!("last command: {command}"));
    }
    if facts.is_empty() {
        "Open workspace object".to_string()
    } else {
        facts.join("  ·  ")
    }
}

fn context_parts(text: &str, match_start: usize, match_end: usize) -> (&str, &str, &str) {
    let before_start = text[..match_start]
        .char_indices()
        .rev()
        .nth(72)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let after_end = text[match_end..]
        .char_indices()
        .nth(72)
        .map(|(index, _)| match_end + index)
        .unwrap_or(text.len());
    (
        &text[before_start..match_start],
        &text[match_start..match_end],
        &text[match_end..after_end],
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let end = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let suffix = if end < value.len() { "…" } else { "" };
    format!("{}{}", &value[..end], suffix)
}

struct SearchableTerminalText {
    text: String,
    raw_ends: Vec<usize>,
}

fn strip_terminal_controls(bytes: &[u8]) -> SearchableTerminalText {
    let mut text = String::with_capacity(bytes.len());
    let mut raw_ends = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index = skip_escape(bytes, index);
            continue;
        }
        let byte = bytes[index];
        if byte == b'\r' || byte == b'\n' {
            push_mapped_char(&mut text, &mut raw_ends, '\n', index + 1);
            index += 1;
            continue;
        }
        if byte < 0x20 || byte == 0x7f {
            index += 1;
            continue;
        }
        let remaining = &bytes[index..];
        let decoded = std::str::from_utf8(remaining)
            .ok()
            .and_then(|value| value.chars().next());
        if let Some(character) = decoded {
            let end = index + character.len_utf8();
            push_mapped_char(&mut text, &mut raw_ends, character, end);
            index = end;
        } else {
            index += 1;
        }
    }
    SearchableTerminalText { text, raw_ends }
}

fn skip_escape(bytes: &[u8], index: usize) -> usize {
    let Some(next) = bytes.get(index + 1).copied() else {
        return bytes.len();
    };
    if next == b'[' {
        let mut cursor = index + 2;
        while cursor < bytes.len() {
            if (0x40..=0x7e).contains(&bytes[cursor]) {
                return cursor + 1;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    if next == b']' {
        let mut cursor = index + 2;
        while cursor < bytes.len() {
            if bytes[cursor] == 0x07 {
                return cursor + 1;
            }
            if bytes[cursor] == 0x1b && bytes.get(cursor + 1) == Some(&b'\\') {
                return cursor + 2;
            }
            cursor += 1;
        }
        return bytes.len();
    }
    (index + 2).min(bytes.len())
}

fn push_mapped_char(text: &mut String, raw_ends: &mut Vec<usize>, character: char, raw_end: usize) {
    let byte_count = character.len_utf8();
    text.push(character);
    raw_ends.extend(std::iter::repeat_n(raw_end, byte_count));
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn terminal_search_ignores_ansi_and_preserves_a_raw_jump_offset() {
        let mut results = Vec::new();
        let history = b"before \x1b[31mneedle\x1b[0m after";
        find_terminal_results(
            history,
            "NEEDLE",
            |before, matched, after, history_end_byte| SearchResult {
                pane_id: NodeId(1),
                kind: SearchObjectKind::Shell,
                object_name: "shell".to_string(),
                automatic_title: None,
                path: None,
                last_command: None,
                before: before.to_string(),
                matched: matched.to_string(),
                after: after.to_string(),
                location: SearchLocation::Terminal { history_end_byte },
            },
            &mut results,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched, "needle");
        assert!(matches!(
            results[0].location,
            SearchLocation::Terminal { history_end_byte } if history_end_byte > 6
        ));
    }

    #[test]
    fn local_search_is_case_insensitive_and_keeps_the_line_target() {
        let mut results = Vec::new();
        find_text_results(
            "A Needle appears",
            "needle",
            |before, matched, after, line| SearchResult {
                pane_id: NodeId(2),
                kind: SearchObjectKind::File,
                object_name: "note.md".to_string(),
                automatic_title: None,
                path: None,
                last_command: None,
                before: before.to_string(),
                matched: matched.to_string(),
                after: after.to_string(),
                location: SearchLocation::Editor { line },
            },
            4,
            &mut results,
        );
        assert_eq!(results[0].matched, "Needle");
        assert_eq!(results[0].location, SearchLocation::Editor { line: 4 });
    }

    #[test]
    fn full_screen_render_shows_object_metadata_and_highlighted_match() {
        let mut state = SearchState::new();
        state.query = TextPromptState::new("needle");
        state.results = vec![SearchResult {
            pane_id: NodeId(3),
            kind: SearchObjectKind::Agent,
            object_name: "Investigate checkout".to_string(),
            automatic_title: Some("Investigate checkout".to_string()),
            path: None,
            last_command: Some("git status".to_string()),
            before: "before ".to_string(),
            matched: "needle".to_string(),
            after: " after".to_string(),
            location: SearchLocation::Terminal {
                history_end_byte: 42,
            },
        }];
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).expect("test terminal");

        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .expect("render workspace search");

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Workspace Search"));
        assert!(text.contains("Investigate checkout"));
        assert!(text.contains("last command: git status"));
        assert!(text.contains("needle"));
    }

    #[test]
    fn full_screen_render_shows_live_query_and_places_the_native_cursor() {
        let mut state = SearchState::new();
        state.query = TextPromptState::new("needle");
        state.query.cursor = 3;
        let mut terminal = Terminal::new(TestBackend::new(48, 18)).expect("test terminal");

        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .expect("render searchable input");

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("needle"));
        assert_eq!(terminal.get_cursor_position().expect("native cursor").y, 2);
    }

    #[test]
    fn search_state_waits_for_a_full_second_of_quiet_before_claiming_work() {
        let start = Instant::now();
        let mut state = SearchState::new();
        state.query = TextPromptState::new("needle");
        state.note_query_changed(start);

        assert!(state
            .take_due_search(start + SEARCH_DEBOUNCE - Duration::from_millis(1))
            .is_none());
        assert_eq!(
            state.take_due_search(start + SEARCH_DEBOUNCE),
            Some((1, "needle".to_string()))
        );
        assert!(state.is_searching());
    }

    #[test]
    fn stale_worker_completion_cannot_replace_a_newer_query() {
        let start = Instant::now();
        let mut state = SearchState::new();
        state.query = TextPromptState::new("first");
        state.note_query_changed(start);
        let (first_revision, _) = state
            .take_due_search(start + SEARCH_DEBOUNCE)
            .expect("first request");
        state.query = TextPromptState::new("second");
        state.note_query_changed(start + SEARCH_DEBOUNCE);

        assert!(!state.complete_search(first_revision, Vec::new()));
        assert!(state.results.is_empty());
        assert_eq!(
            state.take_due_search(start + SEARCH_DEBOUNCE + SEARCH_DEBOUNCE),
            Some((2, "second".to_string()))
        );
    }
}
