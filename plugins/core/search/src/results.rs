//! Result rows, grouping and the debounced query state of the workspace search panel.
//! Pure: times are caller-supplied monotonic milliseconds (the clock import), never read here.

use crate::matching::SearchResult;

/// A full second of silence favours fluid typing over rescanning multi-megabyte terminal
/// journals for every intermediate prefix.
pub const SEARCH_DEBOUNCE_MILLIS: u64 = 1000;
/// Terminal rows one result takes: header, metadata, match, divider, spacer.
pub const RESULT_HEIGHT: usize = 5;
/// Most continuations queued while a query is still being prepared.
pub const MAX_PENDING_CONTINUATIONS: usize = 32;

/// What to do with a result once its source finished preparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationAction {
    Select(usize),
    Open(Option<usize>),
    Move(i32),
}

/// A queued action tied to the query revision it was issued under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Continuation {
    pub revision: u64,
    pub action: ContinuationAction,
}

/// The query, its results and the revision bookkeeping that keeps a stale scan from
/// replacing what the user has already typed past.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected_index: usize,
    pub scroll: usize,
    query_revision: u64,
    completed_revision: Option<u64>,
    in_flight_revision: Option<u64>,
    last_query_edit_millis: Option<u64>,
    pending: Vec<Continuation>,
    pub control_error: Option<String>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    /// An empty query with revision 0 already completed.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            scroll: 0,
            query_revision: 0,
            completed_revision: Some(0),
            in_flight_revision: None,
            last_query_edit_millis: None,
            pending: Vec::new(),
            control_error: None,
        }
    }

    /// The revision of the current query text.
    pub fn revision(&self) -> u64 {
        self.query_revision
    }

    /// The pending continuations, oldest first.
    pub fn pending(&self) -> &[Continuation] {
        &self.pending
    }

    /// Replaces the results and keeps the selection and scroll inside them.
    pub fn replace_results(&mut self, results: Vec<SearchResult>) {
        self.results = results;
        self.selected_index = self
            .selected_index
            .min(self.results.len().saturating_sub(1));
        self.scroll = self.scroll.min(self.selected_index);
    }

    fn advance_revision(&mut self) {
        self.query_revision = self.query_revision.checked_add(1).unwrap_or(0);
    }

    /// The highlighted result.
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
    }

    /// Adopts results computed synchronously for the current query text: bumps the revision so
    /// a still-running worker for an earlier query is rejected, and resets the selection.
    pub fn adopt_synchronous_results(&mut self, results: Vec<SearchResult>) {
        self.advance_revision();
        self.completed_revision = Some(self.query_revision);
        self.last_query_edit_millis = None;
        self.selected_index = 0;
        self.scroll = 0;
        self.replace_results(results);
    }

    /// Moves the selection by `delta` rows, clamped, and scrolls to keep it visible.
    pub fn move_selection(&mut self, delta: i32, visible_rows: usize) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len().saturating_sub(1) as i64;
        self.selected_index =
            (self.selected_index as i64 + i64::from(delta)).clamp(0, last) as usize;
        let visible_rows = visible_rows.max(1);
        if self.selected_index < self.scroll {
            self.scroll = self.selected_index;
        } else if self.selected_index >= self.scroll.saturating_add(visible_rows) {
            self.scroll = self.selected_index + 1 - visible_rows;
        }
    }

    /// Records an input edit without doing search work. Queued continuations of the previous
    /// query are cancelled and the cancellation is reported in `control_error`.
    pub fn note_query_changed(&mut self, now_millis: u64) {
        if !self.pending.is_empty() {
            self.pending.clear();
            self.control_error =
                Some("Previous search continuations cancelled by a newer query".into());
        }
        self.advance_revision();
        self.completed_revision = None;
        self.last_query_edit_millis = Some(now_millis);
        self.results = Vec::new();
        self.selected_index = 0;
        self.scroll = 0;
    }

    /// Claims the current query once it has been quiet for [`SEARCH_DEBOUNCE_MILLIS`].
    pub fn take_due_search(&mut self, now_millis: u64) -> Option<(u64, String)> {
        if self.query.trim().is_empty()
            || self.in_flight_revision.is_some()
            || self.completed_revision == Some(self.query_revision)
        {
            return None;
        }
        let last_edit = self.last_query_edit_millis?;
        if now_millis.saturating_sub(last_edit) < SEARCH_DEBOUNCE_MILLIS {
            return None;
        }
        self.in_flight_revision = Some(self.query_revision);
        Some((self.query_revision, self.query.trim().to_string()))
    }

    /// Makes a failed worker launch retryable on the next tick.
    pub fn cancel_in_flight_search(&mut self, revision: u64) {
        if self.in_flight_revision == Some(revision) {
            self.in_flight_revision = None;
        }
    }

    /// Installs a scan result only when it still belongs to the visible query.
    pub fn complete_search(&mut self, revision: u64, results: Vec<SearchResult>) -> bool {
        if self.in_flight_revision == Some(revision) {
            self.in_flight_revision = None;
        }
        if revision != self.query_revision {
            return false;
        }
        self.completed_revision = Some(revision);
        self.replace_results(results);
        true
    }

    /// True while a non-empty query waits for its quiet period or its scan.
    pub fn is_waiting_for_debounce(&self) -> bool {
        !self.query.trim().is_empty()
            && self.in_flight_revision.is_none()
            && self.completed_revision != Some(self.query_revision)
    }

    /// True while the current query has no completed scan.
    pub fn is_preparing(&self) -> bool {
        !self.query.trim().is_empty() && self.completed_revision != Some(self.query_revision)
    }

    /// True while a scan for some revision runs.
    pub fn is_searching(&self) -> bool {
        self.in_flight_revision.is_some()
    }

    /// Queues an action until the current query finished preparing.
    pub fn queue_control(&mut self, action: ContinuationAction) -> Result<(), String> {
        if self.pending.len() >= MAX_PENDING_CONTINUATIONS {
            return Err(
                "Search continuation queue is full; retry the original command after preparation"
                    .into(),
            );
        }
        self.pending.push(Continuation {
            revision: self.query_revision,
            action,
        });
        Ok(())
    }

    /// The instant (monotonic milliseconds) at which a quiet query becomes due, so a host can
    /// sleep until then instead of waking on every tick.
    pub fn next_due_search_at(&self) -> Option<u64> {
        if self.is_waiting_for_debounce() {
            self.last_query_edit_millis
                .map(|edited_at| edited_at.saturating_add(SEARCH_DEBOUNCE_MILLIS))
        } else {
            None
        }
    }
}

/// The metadata line of a result: AI title, path and last command, or a generic line.
pub fn result_metadata(result: &SearchResult) -> String {
    let mut facts = Vec::new();
    if let Some(title) = &result.automatic_title {
        facts.push(format!("AI title: {title}"));
    }
    if let Some(path) = &result.path {
        facts.push(path.clone());
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

/// How many whole results fit a list `height` rows tall (at least one).
pub fn visible_result_rows(height: u16) -> usize {
    (usize::from(height) / RESULT_HEIGHT).max(1)
}

/// The width left for result text: one column goes to a scrollbar when results overflow.
pub fn result_content_width(width: u16, height: u16, result_count: usize) -> u16 {
    let gutter = u16::from(width >= 2 && result_count > visible_result_rows(height));
    width.saturating_sub(gutter)
}

/// Which result a click on `row` (relative to the list's first row) of a list `height` rows
/// tall selects. Leftover rows below the last whole result select nothing.
pub fn result_at_row(row: usize, height: u16, scroll: usize, result_count: usize) -> Option<usize> {
    let visible = visible_result_rows(height);
    if row >= visible * RESULT_HEIGHT {
        return None;
    }
    let index = scroll + row / RESULT_HEIGHT;
    (index < result_count).then_some(index)
}

/// The display rows of one result: header, metadata and the match line split three ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    pub header: String,
    pub metadata: String,
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// Builds the row shown for `result` (the header carries the kind label and the name).
pub fn result_row(result: &SearchResult) -> ResultRow {
    ResultRow {
        header: format!("{}  {}", result.kind.label(), result.object_name),
        metadata: result_metadata(result),
        before: result.before.clone(),
        matched: result.matched.clone(),
        after: result.after.clone(),
    }
}

/// Groups result indexes by object kind, kinds in first-seen order.
pub fn group_by_kind(results: &[SearchResult]) -> Vec<(crate::matching::ObjectKind, Vec<usize>)> {
    let mut groups: Vec<(crate::matching::ObjectKind, Vec<usize>)> = Vec::new();
    for (index, result) in results.iter().enumerate() {
        match groups.iter_mut().find(|(kind, _)| *kind == result.kind) {
            Some((_, indexes)) => indexes.push(index),
            None => groups.push((result.kind, vec![index])),
        }
    }
    groups
}
