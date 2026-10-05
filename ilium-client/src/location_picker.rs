//! Modal world-map picker for the shared observer or an independent OSM map.
//!
//! Three ways to choose a place, all ending in the same candidate that
//! Enter (or the "Use location" button) confirms:
//! * an address, geocoded on the client's owned finite I/O bank;
//! * a direct "lat, lon" entry, parsed locally (no network);
//! * a Braille world map with a crosshair (arrows, Shift for big steps, or a
//!   mouse click).
//!
//! Render and input share [`layout`], so hit-testing can never drift from
//! what is drawn.

#[cfg(test)]
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::{path::PathBuf, sync::Arc};

use crossterm::event::{KeyCode, KeyModifiers};
use ilium_ambient::{worldmap, AddressProvider, AddressSearchSettings, GeoLocation};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Retained,
    SkipReason, StorageAdmission,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    widgets::{Clear, Paragraph},
    Frame,
};

use crate::modal::{
    centered_fixed_rect, dialog_action_layout, inset_rect, render_dialog_actions, DialogAction,
    DialogActionLayout, DialogActions,
};
use crate::text_prompt::{self, PromptOutcome, TextPromptState};
use crate::theme;

/// Rows of the results list.
const RESULT_ROWS: u16 = 4;
/// Cells the Shift modifier moves the crosshair per key press.
const BIG_STEP_CELLS: i32 = 5;
const MAX_SEARCH_QUERY_BYTES: usize = 64 * 1024;
const MIB: usize = 1024 * 1024;
/// Covers the 256 KiB HTTP body, provider parsing and bounded query capture.
/// This is a cooperative peak declaration, not an allocator or RSS limit.
const SEARCH_COST: JobCost = JobCost {
    input_bytes: 16 * MIB,
    result_bytes: 4 * MIB,
};
const SEARCH_RESULT_STORAGE_BYTES: usize = SEARCH_COST.result_bytes;

/// A blocking address lookup, run only on an already-admitted finite I/O worker.
pub type Searcher = Arc<dyn Fn(&str) -> Result<Vec<GeoLocation>, String> + Send + Sync>;
type SearchResult = Result<Vec<GeoLocation>, String>;

struct SearchOutput {
    results: SearchResult,
    // Last: the actual allocation is destroyed before its process-root debit.
    storage: Arc<StorageAdmission>,
}
struct SearchTask {
    query: Arc<str>,
    searcher: Searcher,
    // Reserved before copying the query or publishing the job.
    storage: Arc<StorageAdmission>,
}
impl Job for SearchTask {
    type Output = SearchOutput;
    type Error = String;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("Address search cancelled".into());
        }
        let results = (self.searcher)(&self.query);
        if context.stop_requested() {
            return Err("Address search cancelled".into());
        }
        let bytes = match &results {
            Ok(results) => retained_result_bytes(results, results.capacity()),
            Err(message) => std::mem::size_of::<SearchOutput>().checked_add(message.capacity()),
        }
        .ok_or("Address search result size overflow")?;
        if bytes > context.cost().result_bytes {
            return Err("Address search result exceeds admitted storage".into());
        }
        Ok(SearchOutput {
            results,
            storage: self.storage,
        })
    }
}

/// Include the installed vector and one selected-label copy, which may outlive
/// the result list as the map candidate. No extra heap copy inherits this debit.
fn retained_result_bytes(results: &[GeoLocation], capacity: usize) -> Option<usize> {
    let vector = capacity.checked_mul(std::mem::size_of::<GeoLocation>())?;
    let labels = results.iter().try_fold(0usize, |total, location| {
        total.checked_add(location.label.capacity())
    })?;
    let candidate = results
        .iter()
        .map(|location| location.label.capacity())
        .max()
        .unwrap_or(0);
    std::mem::size_of::<SearchOutput>()
        .checked_add(vector)?
        .checked_add(labels)?
        .checked_add(candidate)
}

fn search_refusal(reason: RejectReason) -> String {
    match reason {
        RejectReason::Closed => "Address search stopped during shutdown".into(),
        RejectReason::Busy
        | RejectReason::QueueFull
        | RejectReason::JobLimit
        | RejectReason::InputBytes
        | RejectReason::ResultBytes
        | RejectReason::WorkerBytes => "Address workers are busy; try again shortly".into(),
        _ => format!("Address search resources unavailable: {reason:?}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerTarget {
    SharedObserver,
    OpenStreetMap {
        project_path: PathBuf,
        search_provider: AddressProvider,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerFocus {
    Input,
    Results,
    Map,
}

/// What a key press asks the surrounding mode handler to do.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerOutcome {
    Continue,
    Cancel,
    // The App admits destination storage before copying the candidate.
    Confirm,
}

enum SearchCompletion {
    Bank(Receipt<SearchTask>),
    #[cfg(test)]
    TestChannel(Receiver<SearchResult>),
}

enum CompletedSearch {
    Bank(Retained<JobOutcome<SearchTask>>),
    #[cfg(test)]
    TestChannel(SearchResult),
    Lost,
}

/// One pending search. The existing bank and supervisor retain a blocked
/// callback after the picker closes; dropping this UI receipt requests stop.
struct SearchJob {
    query: Arc<str>,
    input_revision: u64,
    // Last: query allocation dies before a receipt can release its job debit.
    completion: SearchCompletion,
}
impl Drop for SearchJob {
    fn drop(&mut self) {
        match &self.completion {
            SearchCompletion::Bank(receipt) => receipt.cancel(),
            #[cfg(test)]
            SearchCompletion::TestChannel(_) => {}
        }
    }
}

pub struct LocationPickerState {
    pub input: TextPromptState,
    pub results: Vec<GeoLocation>,
    pub selected_result: usize,
    /// The place Enter would confirm; also where the crosshair is drawn.
    pub candidate: GeoLocation,
    pub focus: PickerFocus,
    pub status: Option<String>,
    search: Option<SearchJob>,
    searcher: Searcher,
    search_client: Option<Client>,
    target: PickerTarget,
    input_revision: u64,
    pending_save: Option<Arc<()>>,
    // Last: results and selected candidate are destroyed before this debit.
    results_storage: Option<Arc<StorageAdmission>>,
    candidate_storage: Option<Arc<StorageAdmission>>,
    // Provider errors can contain the complete query; keep their source debit
    // after the finite job retires, just like successful result labels.
    status_storage: Option<Arc<StorageAdmission>>,
}

impl LocationPickerState {
    pub fn new(current: GeoLocation, search_client: Option<Client>) -> Self {
        Self::with_searcher_and_client(
            current,
            Arc::new(|query: &str| ilium_ambient::geocode::search(query)),
            search_client,
        )
    }

    /// Test searchers still execute on a real, shared client bank.
    #[cfg(test)]
    pub fn with_searcher(current: GeoLocation, searcher: Searcher) -> Self {
        Self::with_searcher_and_client(current, searcher, Some(crate::execution::test_client()))
    }

    fn with_searcher_and_client(
        current: GeoLocation,
        searcher: Searcher,
        search_client: Option<Client>,
    ) -> Self {
        Self {
            input: TextPromptState::new(""),
            results: Vec::new(),
            selected_result: 0,
            candidate: current.normalized(),
            focus: PickerFocus::Input,
            status: None,
            search: None,
            searcher,
            search_client,
            target: PickerTarget::SharedObserver,
            input_revision: 0,
            pending_save: None,
            results_storage: None,
            candidate_storage: None,
            status_storage: None,
        }
    }

    pub fn for_openstreetmap(
        current: GeoLocation,
        project_path: PathBuf,
        settings: &AddressSearchSettings,
        search_client: Option<Client>,
    ) -> Self {
        let search_provider = settings.provider;
        // Settings can be loaded from disk. Bound this picker snapshot before
        // allocating its owned copy; invalid endpoints remain editable and
        // direct-coordinate selection still works.
        let searcher: Searcher =
            if settings.photon_endpoint.len() > 2048 || settings.nominatim_endpoint.len() > 2048 {
                Arc::new(|_| Err("Search endpoint exceeds 2048 bytes".into()))
            } else {
                let settings = settings.clone();
                Arc::new(move |query| {
                    ilium_ambient::openstreetmap_address_search::search(&settings, query)
                })
            };
        let mut picker = Self::with_searcher_and_client(current, searcher, search_client);
        picker.target = PickerTarget::OpenStreetMap {
            project_path,
            search_provider,
        };
        picker
    }

    pub fn target(&self) -> &PickerTarget {
        &self.target
    }

    pub(crate) fn is_saving(&self) -> bool {
        self.pending_save.is_some()
    }

    pub(crate) fn begin_save(&mut self, token: Arc<()>) {
        self.pending_save = Some(token);
        self.status = Some("Saving map location…".into());
    }

    pub(crate) fn matches_save(&self, token: &Arc<()>) -> bool {
        self.pending_save
            .as_ref()
            .is_some_and(|pending| Arc::ptr_eq(pending, token))
    }

    pub(crate) fn fail_save(&mut self, message: String) {
        self.pending_save = None;
        self.status = Some(message);
    }

    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    /// Test-only transport seam; forcing ownership tests use a real bank.
    #[cfg(test)]
    pub fn adopt_search_channel(&mut self, receiver: Receiver<SearchResult>) {
        self.search = Some(SearchJob {
            query: Arc::from(self.input.buf.trim()),
            input_revision: self.input_revision,
            completion: SearchCompletion::TestChannel(receiver),
        });
        self.status = Some("Searching\u{2026}".to_owned());
    }

    fn start_search(&mut self) {
        if self.search.is_some() {
            self.status = Some("A search is still running; wait before submitting again".into());
            return;
        }
        let Some(client) = &self.search_client else {
            self.status = Some("Address search is unavailable".into());
            return;
        };
        // Both debits precede the first new query allocation or job capture.
        let reservation = match client.try_reserve(Lane::Io, SEARCH_COST) {
            Ok(reservation) => reservation,
            Err(reason) => {
                self.status = Some(search_refusal(reason));
                return;
            }
        };
        let storage = match client
            .quota_group()
            .reserve_external_storage(SEARCH_RESULT_STORAGE_BYTES)
        {
            Ok(storage) => Arc::new(storage),
            Err(reason) => {
                self.status = Some(search_refusal(reason));
                return;
            }
        };
        let query: Arc<str> = Arc::from(self.input.buf.trim());
        let task = SearchTask {
            query: Arc::clone(&query),
            searcher: Arc::clone(&self.searcher),
            storage,
        };
        match reservation.submit(task) {
            Ok(receipt) => {
                self.search = Some(SearchJob {
                    query,
                    input_revision: self.input_revision,
                    completion: SearchCompletion::Bank(receipt),
                });
                self.status = Some("Searching\u{2026}".to_owned());
            }
            Err(rejected) => {
                self.status = Some(search_refusal(rejected.reason));
            }
        }
    }

    fn clear_results(&mut self) {
        // Vec::clear keeps capacity, which would outlive a released debit.
        self.results = Vec::new();
        self.results_storage = None;
        self.selected_result = 0;
    }

    /// Applies a finished search, if any. Returns whether anything changed.
    pub fn poll_search(&mut self) -> bool {
        if self.is_saving() {
            return false;
        }
        let Some(job) = &mut self.search else {
            return false;
        };
        let completed = match &mut job.completion {
            SearchCompletion::Bank(receipt) => match receipt.try_take() {
                JobPoll::Pending => return false,
                JobPoll::Ready(outcome) => CompletedSearch::Bank(outcome),
                JobPoll::Lost | JobPoll::Taken => CompletedSearch::Lost,
            },
            #[cfg(test)]
            SearchCompletion::TestChannel(receiver) => match receiver.try_recv() {
                Ok(outcome) => CompletedSearch::TestChannel(outcome),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => CompletedSearch::Lost,
            },
        };
        let is_current = job.input_revision == self.input_revision
            && job.query.as_ref() == self.input.buf.trim();
        self.search = None;
        if !is_current {
            self.clear_results();
            self.status =
                Some("Earlier search finished; press Enter to search the current text".into());
            return true;
        }
        let (outcome, storage) = match completed {
            CompletedSearch::Bank(retained) => {
                let (outcome, job_hold) = retained.into_parts();
                match outcome {
                    JobOutcome::Finished(Ok(output)) => {
                        let SearchOutput { results, storage } = output;
                        drop(job_hold); // separate storage debit already owns results
                        (results, Some(storage))
                    }
                    JobOutcome::Finished(Err(message)) => {
                        drop(job_hold);
                        (Err(message), None)
                    }
                    JobOutcome::NotStarted { job, reason } => {
                        drop(job);
                        drop(job_hold);
                        let message = match reason {
                            SkipReason::Shutdown => "Address search stopped during shutdown",
                            SkipReason::Cancelled => "Address search cancelled",
                        };
                        (Err(message.to_owned()), None)
                    }
                    JobOutcome::Panicked => {
                        drop(job_hold);
                        (
                            Err("The address search stopped unexpectedly".to_owned()),
                            None,
                        )
                    }
                }
            }
            #[cfg(test)]
            CompletedSearch::TestChannel(outcome) => (outcome, None),
            CompletedSearch::Lost => (
                Err("The address search stopped unexpectedly".to_owned()),
                None,
            ),
        };
        match outcome {
            Ok(results) if results.is_empty() => {
                self.clear_results();
                self.status = Some("No matching place found".to_owned());
                self.status_storage = None;
            }
            Ok(results) => {
                self.clear_results();
                self.status = Some(format!(
                    "{} result{} \u{2014} Up/Down, Enter chooses",
                    results.len(),
                    if results.len() == 1 { "" } else { "s" }
                ));
                self.results = results;
                self.results_storage = storage;
                self.status_storage = None;
                self.focus = PickerFocus::Results;
            }
            Err(message) => {
                self.clear_results();
                self.status = Some(message);
                self.status_storage = storage;
            }
        }
        true
    }

    pub fn paste(&mut self, text: &str) {
        if self.is_saving() {
            return;
        }
        let before = self.input.buf.clone();
        for character in text.trim_end_matches(['\r', '\n']).chars() {
            if !character.is_control() {
                text_prompt::handle_key(&mut self.input, KeyCode::Char(character));
            }
        }
        self.focus = PickerFocus::Input;
        if self.input.buf != before {
            self.input_changed();
        }
    }

    fn input_changed(&mut self) {
        self.input_revision = self.input_revision.wrapping_add(1);
        self.clear_results();
        self.status = None;
        self.status_storage = None;
    }

    fn cycle_focus(&mut self, direction: i32) {
        let order = if self.results.is_empty() {
            vec![PickerFocus::Input, PickerFocus::Map]
        } else {
            vec![PickerFocus::Input, PickerFocus::Results, PickerFocus::Map]
        };
        let position = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0) as i32;
        let next = (position + direction).rem_euclid(order.len() as i32) as usize;
        self.focus = order[next];
    }

    /// Enter in the input field: coordinates apply locally, anything else is
    /// geocoded.
    fn submit_input(&mut self) {
        let text = self.input.buf.trim();
        if text.is_empty() {
            self.status = Some("Type an address or \"lat, lon\" first".to_owned());
            return;
        }
        if text.len() > MAX_SEARCH_QUERY_BYTES {
            self.status = Some(
                "Address search exceeds the 64 KiB capture limit; shorten the address and retry"
                    .into(),
            );
            return;
        }
        if let Some(location) = GeoLocation::parse_coordinates(text) {
            self.candidate = location;
            self.candidate_storage = None;
            self.input_changed();
            self.focus = PickerFocus::Map;
            self.status = Some("Coordinates set \u{2014} Enter uses them".to_owned());
            return;
        }
        self.start_search();
    }

    /// Moves the crosshair `columns` / `rows` cells on a map of the given size.
    fn move_crosshair(&mut self, columns: i32, rows: i32, map: Rect) {
        if map.width == 0 || map.height == 0 {
            return;
        }
        let (column, row) = cell_of(&self.candidate, map.width, map.height);
        let column = (i32::from(column) + columns).clamp(0, i32::from(map.width) - 1) as u16;
        let row = (i32::from(row) + rows).clamp(0, i32::from(map.height) - 1) as u16;
        self.set_candidate_from_cell(column, row, map);
    }

    fn set_candidate_from_cell(&mut self, column: u16, row: u16, map: Rect) {
        self.input_changed();
        let (longitude, latitude) = lon_lat_of(column, row, map.width, map.height);
        self.candidate = GeoLocation::new(String::new(), latitude, longitude);
        self.candidate.label = self.candidate.coordinate_text();
        self.candidate_storage = None;
    }

    pub fn handle_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        screen: Rect,
    ) -> PickerOutcome {
        if self.is_saving() && code != KeyCode::Esc {
            return PickerOutcome::Continue;
        }
        let picker_layout = layout(screen);
        match code {
            KeyCode::Esc => return PickerOutcome::Cancel,
            KeyCode::Tab => {
                self.cycle_focus(1);
                return PickerOutcome::Continue;
            }
            KeyCode::BackTab => {
                self.cycle_focus(-1);
                return PickerOutcome::Continue;
            }
            _ => {}
        }
        match self.focus {
            PickerFocus::Input => match code {
                KeyCode::Down if !self.results.is_empty() => self.focus = PickerFocus::Results,
                _ => {
                    let before = self.input.buf.clone();
                    if text_prompt::handle_key(&mut self.input, code) == PromptOutcome::Commit {
                        self.submit_input();
                    } else if self.input.buf != before {
                        self.input_changed();
                    }
                }
            },
            PickerFocus::Results => match code {
                KeyCode::Up => {
                    if self.selected_result == 0 {
                        self.focus = PickerFocus::Input;
                    } else {
                        self.selected_result -= 1;
                    }
                }
                KeyCode::Down => {
                    self.selected_result =
                        (self.selected_result + 1).min(self.results.len().saturating_sub(1));
                }
                KeyCode::Enter => self.choose_result(self.selected_result),
                _ => {}
            },
            PickerFocus::Map => {
                let step = if modifiers.contains(KeyModifiers::SHIFT) {
                    BIG_STEP_CELLS
                } else {
                    1
                };
                match code {
                    KeyCode::Left => self.move_crosshair(-step, 0, picker_layout.map),
                    KeyCode::Right => self.move_crosshair(step, 0, picker_layout.map),
                    KeyCode::Up => self.move_crosshair(0, -step, picker_layout.map),
                    KeyCode::Down => self.move_crosshair(0, step, picker_layout.map),
                    KeyCode::Enter => return PickerOutcome::Confirm,
                    _ => {}
                }
            }
        }
        PickerOutcome::Continue
    }

    /// Makes result `index` the candidate and moves focus to the map so the
    /// user can fine-tune or confirm with Enter.
    fn choose_result(&mut self, index: usize) {
        let Some(result) = self.results.get(index) else {
            return;
        };
        self.selected_result = index;
        self.candidate = result.normalized();
        self.candidate_storage = self.results_storage.clone();
        self.focus = PickerFocus::Map;
        self.status = Some("Enter uses this place; arrows fine-tune it".to_owned());
    }

    /// Left click at `position`.
    pub fn click(&mut self, position: Position, screen: Rect) -> PickerOutcome {
        let picker_layout = layout(screen);
        if self.is_saving() {
            return if picker_layout.actions.action_at(position) == Some(DialogAction::Cancel) {
                PickerOutcome::Cancel
            } else {
                PickerOutcome::Continue
            };
        }
        if let Some(action) = picker_layout.actions.action_at(position) {
            return match action {
                DialogAction::Cancel => PickerOutcome::Cancel,
                DialogAction::Confirm => PickerOutcome::Confirm,
            };
        }
        if picker_layout.input_box.contains(position) {
            self.focus = PickerFocus::Input;
        } else if let Some(index) =
            picker_layout.result_at(position, self.results.len(), self.selected_result)
        {
            self.choose_result(index);
        } else if picker_layout.map.contains(position) {
            self.focus = PickerFocus::Map;
            let column = position.x - picker_layout.map.x;
            let row = position.y - picker_layout.map.y;
            self.set_candidate_from_cell(column, row, picker_layout.map);
        }
        PickerOutcome::Continue
    }
}

/// Every rectangle of the picker, computed once from the screen size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerLayout {
    pub popup: Rect,
    pub input_box: Rect,
    pub input_area: Rect,
    pub results: Rect,
    pub map: Rect,
    pub candidate_row: Rect,
    pub credit_row: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

impl PickerLayout {
    pub fn visible_result_start(&self, selected_result: usize) -> usize {
        selected_result.saturating_sub(usize::from(self.results.height).saturating_sub(1))
    }

    /// The result row under `position`, if any of the `count` results is there.
    pub fn result_at(
        &self,
        position: Position,
        count: usize,
        selected_result: usize,
    ) -> Option<usize> {
        if !self.results.contains(position) {
            return None;
        }
        let index =
            self.visible_result_start(selected_result) + usize::from(position.y - self.results.y);
        (index < count).then_some(index)
    }
}

pub fn layout(screen: Rect) -> PickerLayout {
    let width = screen.width.saturating_sub(4).clamp(40, 110);
    let height = screen.height.saturating_sub(2).clamp(18, 40);
    let popup = centered_fixed_rect(width, height, screen);
    let inner = inset_rect(popup, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(RESULT_ROWS),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    PickerLayout {
        popup,
        input_box: rows[0],
        input_area: inset_rect(rows[0], 1),
        results: rows[1],
        map: rows[2],
        candidate_row: rows[3],
        credit_row: rows[4],
        actions: dialog_action_layout(rows[5]),
        hint_row: rows[6],
    }
}

/// Longitude and latitude at the centre of map cell (`column`, `row`).
pub fn lon_lat_of(column: u16, row: u16, columns: u16, rows: u16) -> (f64, f64) {
    let longitude = -180.0 + (f64::from(column) + 0.5) / f64::from(columns.max(1)) * 360.0;
    let latitude = 90.0 - (f64::from(row) + 0.5) / f64::from(rows.max(1)) * 180.0;
    (longitude, latitude)
}

/// The map cell (`column`, `row`) that contains `location`.
pub fn cell_of(location: &GeoLocation, columns: u16, rows: u16) -> (u16, u16) {
    let column = ((location.longitude + 180.0) / 360.0 * f64::from(columns)).floor();
    let row = ((90.0 - location.latitude) / 180.0 * f64::from(rows)).floor();
    (
        column.clamp(0.0, f64::from(columns.saturating_sub(1))) as u16,
        row.clamp(0.0, f64::from(rows.saturating_sub(1))) as u16,
    )
}

/// Braille bits of a land/sea map, one byte per cell, row-major. Samples
/// `worldmap::is_land` at every dot centre. (When the crate offers its own
/// `braille_map`, replace this adapter with it.)
pub fn land_cells(columns: u16, rows: u16) -> Vec<u8> {
    const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
    let mut cells = vec![0u8; usize::from(columns) * usize::from(rows)];
    let dot_columns = f64::from(columns.max(1)) * 2.0;
    let dot_rows = f64::from(rows.max(1)) * 4.0;
    for row in 0..usize::from(rows) {
        for column in 0..usize::from(columns) {
            let mut bits = 0u8;
            for (dot_row, row_bits) in BITS.iter().enumerate() {
                for (dot_column, bit) in row_bits.iter().enumerate() {
                    let longitude =
                        -180.0 + ((column * 2 + dot_column) as f64 + 0.5) / dot_columns * 360.0;
                    let latitude = 90.0 - ((row * 4 + dot_row) as f64 + 0.5) / dot_rows * 180.0;
                    if worldmap::is_land(longitude, latitude) {
                        bits |= bit;
                    }
                }
            }
            cells[row * usize::from(columns) + column] = bits;
        }
    }
    cells
}

fn fit(text: &str, width: usize) -> String {
    let mut fitted = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            break;
        }
        fitted.push(character);
        used += character_width;
    }
    fitted
}

fn credit_lines(target: &PickerTarget) -> &'static str {
    match target {
        PickerTarget::SharedObserver
        | PickerTarget::OpenStreetMap { search_provider: AddressProvider::CityOnly, .. } =>
            "Map: Natural Earth (public domain)\nSearch: Open-Meteo\nGeoNames (CC BY 4.0)",
        PickerTarget::OpenStreetMap { search_provider: AddressProvider::Photon, .. } =>
            "Map: Natural Earth (public domain)\nSearch: Photon (komoot)\n© OpenStreetMap contributors (ODbL)",
        PickerTarget::OpenStreetMap { search_provider: AddressProvider::Nominatim, .. } =>
            "Map: Natural Earth (public domain)\nSearch: configured Nominatim\n© OpenStreetMap contributors (ODbL)",
        PickerTarget::OpenStreetMap { search_provider: AddressProvider::Disabled, .. } =>
            "Map: Natural Earth (public domain)\nSearch: disabled\nNo provider lookup",
    }
}

#[cfg(test)]
pub fn render(frame: &mut Frame, screen: Rect, picker: &LocationPickerState) {
    render_cursor(frame, screen, picker);
}

pub fn render_cursor(
    frame: &mut Frame,
    screen: Rect,
    picker: &LocationPickerState,
) -> Option<ratatui::layout::Position> {
    let picker_layout = layout(screen);
    frame.render_widget(Clear, picker_layout.popup);
    let title = match picker.target() {
        PickerTarget::SharedObserver => "Location",
        PickerTarget::OpenStreetMap { .. } => "OpenStreetMap location",
    };
    frame.render_widget(
        theme::block(true).title(theme::chrome_title(title)),
        picker_layout.popup,
    );
    let input_title = match picker.focus {
        PickerFocus::Input => "Address or lat, lon  (Enter)",
        _ => "Address or lat, lon",
    };
    frame.render_widget(
        theme::block(picker.focus == PickerFocus::Input).title(theme::chrome_title(input_title)),
        picker_layout.input_box,
    );
    frame.render_widget(
        Paragraph::new(picker.input.buf.as_str()),
        picker_layout.input_area,
    );

    // Results list.
    let results = picker_layout.results;
    if picker.results.is_empty() {
        frame.render_widget(
            Paragraph::new(if picker.is_searching() {
                "Searching\u{2026}"
            } else {
                "Enter searches an address; \"48.86, 2.35\" is used directly."
            })
            .style(Style::new().add_modifier(Modifier::DIM)),
            results,
        );
    } else {
        let first = picker_layout.visible_result_start(picker.selected_result);
        for (offset, result) in picker
            .results
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(results.height))
        {
            let selected = offset == picker.selected_result;
            let style = match (selected, picker.focus == PickerFocus::Results) {
                (true, true) => theme::selected_style().add_modifier(Modifier::BOLD),
                (true, false) => Style::new().add_modifier(Modifier::REVERSED),
                _ => Style::new(),
            };
            let text = fit(
                &format!("{} ({})", result.label, result.coordinate_text()),
                usize::from(results.width),
            );
            frame.render_widget(
                Paragraph::new(text).style(style),
                Rect::new(
                    results.x,
                    results.y + (offset - first) as u16,
                    results.width,
                    1,
                ),
            );
        }
    }

    // World map with crosshair.
    let map = picker_layout.map;
    let land = land_cells(map.width, map.height);
    let (cross_column, cross_row) = cell_of(&picker.candidate, map.width, map.height);
    let land_ink = Style::new().fg(Color::Rgb(120, 170, 120));
    let guide = Style::new().add_modifier(Modifier::DIM);
    for row in 0..map.height {
        for column in 0..map.width {
            let bits = land[usize::from(row) * usize::from(map.width) + usize::from(column)];
            let is_crosshair = column == cross_column && row == cross_row;
            let (symbol, style) = if is_crosshair {
                (
                    '\u{253c}',
                    Style::new()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                )
            } else if bits != 0 {
                (
                    char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '),
                    land_ink,
                )
            } else if row == cross_row {
                ('\u{2500}', guide)
            } else if column == cross_column {
                ('\u{2502}', guide)
            } else {
                (' ', Style::new())
            };
            frame.buffer_mut()[(map.x + column, map.y + row)]
                .set_char(symbol)
                .set_style(style);
        }
    }

    frame.render_widget(
        Paragraph::new(fit(
            &format!(
                "Selected: {} ({})",
                picker.candidate.label,
                picker.candidate.coordinate_text()
            ),
            usize::from(picker_layout.candidate_row.width),
        ))
        .style(Style::new().add_modifier(Modifier::BOLD)),
        picker_layout.candidate_row,
    );
    frame.render_widget(
        Paragraph::new(credit_lines(picker.target()))
            .style(Style::new().add_modifier(Modifier::DIM)),
        picker_layout.credit_row,
    );
    render_dialog_actions(
        frame,
        picker_layout.actions,
        DialogActions::form(
            "Cancel",
            match picker.target() {
                PickerTarget::SharedObserver => "Use location",
                PickerTarget::OpenStreetMap { .. } => "Use map location",
            },
        ),
    );
    let hint = picker.status.clone().unwrap_or_else(|| {
        "Tab switches field \u{b7} arrows move the crosshair (Shift: big steps) \u{b7} click the map"
            .to_owned()
    });
    frame.render_widget(
        Paragraph::new(fit(&hint, usize::from(picker_layout.hint_row.width)))
            .style(Style::new().add_modifier(Modifier::DIM)),
        picker_layout.hint_row,
    );
    if picker.focus == PickerFocus::Input {
        let prefix: String = picker.input.buf.chars().take(picker.input.cursor).collect();
        let cursor = crate::modal::single_line_cursor_position(
            picker_layout.input_area,
            unicode_width::UnicodeWidthStr::width(prefix.as_str()),
        );
        frame.set_cursor_position(cursor);
        return Some(cursor);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_rows::AnimationRow;
    use crate::app::{App, Mode, SettingsState, SettingsTab};
    use crate::background_animation::AnimationKind;
    use crossterm::event::{
        Event, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{backend::TestBackend, Terminal};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const SCREEN: Rect = Rect::new(0, 0, 120, 40);

    fn place(label: &str, latitude: f64, longitude: f64) -> GeoLocation {
        GeoLocation::new(label, latitude, longitude)
    }

    fn picker_with(searcher: Searcher) -> LocationPickerState {
        LocationPickerState::with_searcher(GeoLocation::default(), searcher)
    }

    fn offline() -> Searcher {
        Arc::new(|_query: &str| Err("offline in tests".to_owned()))
    }

    #[test]
    fn changed_address_input_discards_the_previous_search_reply() {
        let mut picker = picker_with(offline());
        type_text(&mut picker, "Paris");
        let (sender, receiver) = mpsc::channel();
        picker.adopt_search_channel(receiver);
        picker.paste(", Texas");
        sender
            .send(Ok(vec![place("Paris, France", 48.85, 2.35)]))
            .unwrap();
        assert!(picker.poll_search());
        assert!(
            picker.results.is_empty(),
            "old-query results cannot be selected"
        );
        assert_eq!(picker.focus, PickerFocus::Input);
    }

    #[test]
    fn repeated_submit_keeps_the_outstanding_search_instead_of_replacing_it() {
        let mut picker = picker_with(offline());
        type_text(&mut picker, "Paris");
        let (sender, receiver) = mpsc::channel();
        picker.adopt_search_channel(receiver);
        press(&mut picker, KeyCode::Enter);
        sender
            .send(Ok(vec![place("Paris, France", 48.85, 2.35)]))
            .expect("repeated Enter must retain the admitted receiver");
        assert!(picker.poll_search());
        assert_eq!(picker.results[0].label, "Paris, France");
    }

    fn isolated_search_bank(
        job_limit: usize,
    ) -> (
        ilium_execution::Execution,
        Client,
        ilium_execution::QuotaGroup,
    ) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
        };
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 2 * SEARCH_COST.input_bytes,
            result_bytes: 2 * SEARCH_COST.result_bytes,
            worker_threads: 2,
            worker_bytes: 16 * MIB,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: disabled,
                io: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: MIB,
                },
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: job_limit,
                service_jobs: 0,
                input_bytes: job_limit * SEARCH_COST.input_bytes,
                result_bytes: job_limit * SEARCH_COST.result_bytes,
            })
            .unwrap();
        (execution, client, quota)
    }

    #[test]
    fn blocked_lookup_keeps_real_job_debit_after_picker_drop_and_refuses_reentry() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(1);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let release_rx = std::sync::Mutex::new(release_rx);
        let searcher: Searcher = Arc::new(move |_| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(vec![place("Paris", 48.85, 2.35)])
        });
        let mut picker = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            searcher,
            Some(client.clone()),
        );
        type_text(&mut picker, "Paris");
        press(&mut picker, KeyCode::Enter);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(quota.snapshot().jobs, 1);
        drop(picker);
        assert_eq!(
            quota.snapshot().jobs,
            1,
            "blocked actual callback keeps its debit"
        );

        let mut refused = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            offline(),
            Some(client.clone()),
        );
        let candidate = refused.candidate.clone();
        type_text(&mut refused, "Another place");
        press(&mut refused, KeyCode::Enter);
        assert!(!refused.is_searching());
        assert_eq!(refused.input.buf, "Another place");
        assert_eq!(refused.candidate, candidate);
        assert!(refused.status.as_deref().unwrap().contains("busy"));
        assert_eq!(quota.snapshot().jobs, 1);

        execution.request_shutdown(ShutdownMode::Cancel);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_millis(20))
            .unwrap();
        assert_eq!(report.remaining_workers, 1);
        assert_eq!(quota.snapshot().jobs, 1, "deadline is not owner exit");
        release_tx.send(()).unwrap();
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
        assert_eq!(quota.snapshot().jobs, 0);
    }

    #[test]
    fn completed_result_storage_follows_results_and_selected_candidate() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(1);
        let baseline = quota.snapshot().worker_bytes;
        let searcher: Searcher = Arc::new(|_| Ok(vec![place("Paris", 48.85, 2.35)]));
        let mut picker = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            searcher,
            Some(client),
        );
        type_text(&mut picker, "Paris");
        press(&mut picker, KeyCode::Enter);
        wait_for_search(&mut picker);
        assert_eq!(quota.snapshot().jobs, 0, "finite job debit may retire");
        assert_eq!(
            quota.snapshot().worker_bytes,
            baseline + SEARCH_RESULT_STORAGE_BYTES
        );
        press(&mut picker, KeyCode::Enter); // select result; candidate clones its label
        picker.paste(" changed"); // releases list while keeping selected candidate
        assert!(picker.results.is_empty());
        assert_eq!(
            quota.snapshot().worker_bytes,
            baseline + SEARCH_RESULT_STORAGE_BYTES
        );
        picker.focus = PickerFocus::Map;
        press(&mut picker, KeyCode::Right); // map candidate replaces charged label
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        drop(picker);
        execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn provider_error_storage_survives_job_retirement_and_long_query_is_unchanged() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(1);
        let baseline = quota.snapshot().worker_bytes;
        let query = "City ".repeat(512).trim().to_owned();
        let expected_query = query.clone();
        let searcher: Searcher = Arc::new(move |received| {
            assert_eq!(received, expected_query);
            Err("provider evidence ".repeat(4096))
        });
        let mut picker = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            searcher,
            Some(client),
        );
        picker.paste(&query);
        press(&mut picker, KeyCode::Enter);
        wait_for_search(&mut picker);
        assert_eq!(picker.input.buf, query);
        assert_eq!(quota.snapshot().jobs, 0);
        assert!(picker.status.as_ref().unwrap().len() > 64 * 1024);
        assert_eq!(
            quota.snapshot().worker_bytes,
            baseline + SEARCH_RESULT_STORAGE_BYTES
        );
        picker.paste(" changed");
        assert!(picker.status.is_none());
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        drop(picker);
        execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn oversized_provider_error_is_refused_without_losing_query() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(1);
        let baseline = quota.snapshot().worker_bytes;
        let searcher: Searcher = Arc::new(|_| Err("x".repeat(SEARCH_RESULT_STORAGE_BYTES + 1)));
        let mut picker = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            searcher,
            Some(client),
        );
        picker.paste("Retained address");
        press(&mut picker, KeyCode::Enter);
        wait_for_search(&mut picker);
        assert_eq!(picker.input.buf, "Retained address");
        assert_eq!(
            picker.status.as_deref(),
            Some("Address search result exceeds admitted storage")
        );
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        drop(picker);
        execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn stale_real_bank_answer_cannot_replace_current_query_or_candidate() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(1);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let release_rx = std::sync::Mutex::new(release_rx);
        let searcher: Searcher = Arc::new(move |_| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(vec![place("Paris, France", 48.85, 2.35)])
        });
        let mut picker = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            searcher,
            Some(client),
        );
        let candidate = picker.candidate.clone();
        let baseline = quota.snapshot().worker_bytes;
        type_text(&mut picker, "Paris");
        press(&mut picker, KeyCode::Enter);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        picker.paste(", Texas");
        release_tx.send(()).unwrap();
        wait_for_search(&mut picker);
        assert_eq!(picker.input.buf, "Paris, Texas");
        assert!(picker.results.is_empty());
        assert_eq!(picker.candidate, candidate);
        assert!(picker.status.as_deref().unwrap().contains("Earlier search"));
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        execution.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn shutdown_skips_queued_geocode_without_invoking_provider() {
        use ilium_execution::ShutdownMode;
        let (mut execution, client, quota) = isolated_search_bank(2);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let release_rx = std::sync::Mutex::new(release_rx);
        let first: Searcher = Arc::new(move |_| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(Vec::new())
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let queued: Searcher = Arc::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        });
        let mut running = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            first,
            Some(client.clone()),
        );
        type_text(&mut running, "First");
        press(&mut running, KeyCode::Enter);
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut waiting = LocationPickerState::with_searcher_and_client(
            GeoLocation::default(),
            queued,
            Some(client),
        );
        type_text(&mut waiting, "Second");
        press(&mut waiting, KeyCode::Enter);
        assert!(waiting.is_searching());
        assert_eq!(quota.snapshot().jobs, 2);
        execution.request_shutdown(ShutdownMode::Cancel);
        release_tx.send(()).unwrap();
        wait_for_search(&mut waiting);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            waiting.status.as_deref(),
            Some("Address search stopped during shutdown")
        );
        drop((running, waiting));
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(report.remaining_workers, 0);
        assert_eq!(quota.snapshot().jobs, 0);
    }

    fn press(picker: &mut LocationPickerState, code: KeyCode) -> PickerOutcome {
        picker.handle_key(code, KeyModifiers::NONE, SCREEN)
    }

    fn type_text(picker: &mut LocationPickerState, text: &str) {
        for character in text.chars() {
            press(picker, KeyCode::Char(character));
        }
    }

    fn wait_for_search(picker: &mut LocationPickerState) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while picker.is_searching() {
            assert!(Instant::now() < deadline, "the search never answered");
            picker.poll_search();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn direct_coordinates_need_no_network() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let mut picker = picker_with(Arc::new(move |_query: &str| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }));
        type_text(&mut picker, "48.857, 2.352");
        assert_eq!(press(&mut picker, KeyCode::Enter), PickerOutcome::Continue);
        assert!(!picker.is_searching(), "coordinates never start a search");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(picker.focus, PickerFocus::Map);
        assert!((picker.candidate.latitude - 48.857).abs() < 1e-9);
        assert_eq!(picker.candidate.label, "48.857N 2.352E");
        assert_eq!(press(&mut picker, KeyCode::Enter), PickerOutcome::Confirm);
        assert_eq!(picker.candidate.label, "48.857N 2.352E");
        assert!((picker.candidate.longitude - 2.352).abs() < 1e-9);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn address_search_runs_on_a_worker_thread_and_results_are_selectable() {
        let names = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&names);
        let mut picker = picker_with(Arc::new(move |query: &str| {
            seen.lock().unwrap().push((
                query.to_owned(),
                std::thread::current().name().map(str::to_owned),
            ));
            Ok(vec![
                place("Paris, France", 48.8566, 2.3522),
                place("Paris, Texas", 33.6609, -95.5555),
            ])
        }));
        type_text(&mut picker, "Paris");
        press(&mut picker, KeyCode::Enter);
        assert!(picker.is_searching());
        wait_for_search(&mut picker);
        let calls = names.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "Paris");
        assert!(calls[0].1.as_deref().unwrap().starts_with("ilium-exec-io-"));
        assert_eq!(picker.results.len(), 2);
        assert_eq!(picker.focus, PickerFocus::Results);
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_result, 1);
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_result, 1, "selection stops at the end");
        press(&mut picker, KeyCode::Up);
        press(&mut picker, KeyCode::Down);
        assert_eq!(press(&mut picker, KeyCode::Enter), PickerOutcome::Continue);
        assert_eq!(picker.candidate.label, "Paris, Texas");
        assert_eq!(picker.focus, PickerFocus::Map);
        assert_eq!(press(&mut picker, KeyCode::Enter), PickerOutcome::Confirm);
        assert_eq!(picker.candidate.label, "Paris, Texas");
        assert!((picker.candidate.latitude - 33.6609).abs() < 1e-9);
    }

    #[test]
    fn injected_channel_results_are_polled_without_any_thread_or_network() {
        let mut picker = picker_with(offline());
        let (sender, receiver) = mpsc::channel();
        picker.adopt_search_channel(receiver);
        assert!(picker.is_searching());
        assert!(!picker.poll_search(), "nothing has arrived yet");
        sender
            .send(Ok(vec![place("Oslo, Norway", 59.91, 10.75)]))
            .unwrap();
        assert!(picker.poll_search());
        assert!(!picker.is_searching());
        assert_eq!(picker.results[0].label, "Oslo, Norway");
        assert_eq!(picker.focus, PickerFocus::Results);
        assert!(
            !picker.poll_search(),
            "an answered search is not polled again"
        );

        let (sender, receiver) = mpsc::channel();
        picker.adopt_search_channel(receiver);
        sender
            .send(Err("Geocoder returned HTTP 503".to_owned()))
            .unwrap();
        assert!(picker.poll_search());
        assert_eq!(picker.status.as_deref(), Some("Geocoder returned HTTP 503"));
        assert!(picker.results.is_empty());

        let (sender, receiver) = mpsc::channel::<SearchResult>();
        picker.adopt_search_channel(receiver);
        drop(sender);
        assert!(picker.poll_search());
        assert!(picker.status.as_deref().unwrap().contains("stopped"));

        let (sender, receiver) = mpsc::channel();
        picker.adopt_search_channel(receiver);
        sender.send(Ok(Vec::new())).unwrap();
        assert!(picker.poll_search());
        assert_eq!(picker.status.as_deref(), Some("No matching place found"));
    }

    #[test]
    fn dismissing_the_picker_mid_search_never_blocks_or_panics() {
        let mut picker = picker_with(Arc::new(|_query: &str| {
            std::thread::sleep(Duration::from_millis(150));
            Ok(vec![GeoLocation::default()])
        }));
        type_text(&mut picker, "slow town");
        press(&mut picker, KeyCode::Enter);
        assert!(picker.is_searching());
        let started = Instant::now();
        drop(picker);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "no join on close"
        );
        // The bank retains the callback until it actually returns.
        std::thread::sleep(Duration::from_millis(250));
    }

    #[test]
    fn empty_input_and_unparseable_text_behave_sensibly() {
        let mut picker = picker_with(offline());
        press(&mut picker, KeyCode::Enter);
        assert!(!picker.is_searching());
        assert!(picker
            .status
            .as_deref()
            .unwrap()
            .contains("Type an address"));
        type_text(&mut picker, "91, 200");
        press(&mut picker, KeyCode::Enter);
        assert!(
            picker.is_searching(),
            "out-of-range coordinates fall back to a search"
        );
        wait_for_search(&mut picker);
        assert_eq!(picker.status.as_deref(), Some("offline in tests"));
        assert_eq!(press(&mut picker, KeyCode::Esc), PickerOutcome::Cancel);
    }

    #[test]
    fn map_cells_and_coordinates_are_consistent_at_every_size() {
        for (columns, rows) in [(40, 10), (97, 21), (2, 1), (1, 1)] {
            for row in 0..rows {
                for column in 0..columns {
                    let (longitude, latitude) = lon_lat_of(column, row, columns, rows);
                    let location = GeoLocation::new("x", latitude, longitude);
                    assert_eq!(cell_of(&location, columns, rows), (column, row));
                }
            }
        }
        let north_pole = GeoLocation::new("p", 90.0, 180.0);
        assert_eq!(cell_of(&north_pole, 40, 10), (39, 0));
        let south_west = GeoLocation::new("p", -90.0, -180.0);
        assert_eq!(cell_of(&south_west, 40, 10), (0, 9));
        assert_eq!(land_cells(30, 8).len(), 240);
    }

    #[test]
    fn arrow_keys_move_the_crosshair_and_shift_takes_big_steps() {
        let mut picker = picker_with(offline());
        picker.focus = PickerFocus::Map;
        let map = layout(SCREEN).map;
        let (column, row) = cell_of(&picker.candidate, map.width, map.height);
        press(&mut picker, KeyCode::Right);
        assert_eq!(
            cell_of(&picker.candidate, map.width, map.height),
            (column + 1, row)
        );
        press(&mut picker, KeyCode::Down);
        assert_eq!(
            cell_of(&picker.candidate, map.width, map.height),
            (column + 1, row + 1)
        );
        picker.handle_key(KeyCode::Right, KeyModifiers::SHIFT, SCREEN);
        assert_eq!(
            cell_of(&picker.candidate, map.width, map.height),
            (column + 6, row + 1),
            "Shift moves five cells"
        );
        picker.handle_key(KeyCode::Up, KeyModifiers::SHIFT, SCREEN);
        assert_eq!(
            cell_of(&picker.candidate, map.width, map.height).1,
            row.saturating_sub(4)
        );
        for _ in 0..200 {
            press(&mut picker, KeyCode::Left);
        }
        assert_eq!(
            cell_of(&picker.candidate, map.width, map.height).0,
            0,
            "clamped at the edge"
        );
        assert!(picker.candidate.longitude < -170.0);
        assert!(picker.candidate.label.contains('N') || picker.candidate.label.contains('S'));
    }

    #[test]
    fn clicks_use_the_same_geometry_as_the_renderer() {
        let mut picker = picker_with(offline());
        let picker_layout = layout(SCREEN);
        assert!(picker_layout
            .popup
            .contains(Position::new(picker_layout.map.x, picker_layout.map.y)));
        for region in [
            picker_layout.input_box,
            picker_layout.results,
            picker_layout.map,
            picker_layout.candidate_row,
            picker_layout.hint_row,
        ] {
            assert!(region.width > 0 && region.height > 0);
            assert!(
                picker_layout.popup.intersection(region) == region,
                "{region:?} inside popup"
            );
        }
        // Click the middle of the map, then confirm through the button.
        let target = Position::new(
            picker_layout.map.x + picker_layout.map.width / 2,
            picker_layout.map.y + picker_layout.map.height / 2,
        );
        assert_eq!(picker.click(target, SCREEN), PickerOutcome::Continue);
        assert_eq!(picker.focus, PickerFocus::Map);
        assert!(picker.candidate.longitude.abs() < 360.0 / f64::from(picker_layout.map.width));
        assert!(picker.candidate.latitude.abs() < 180.0 / f64::from(picker_layout.map.height));

        // The rendered crosshair sits exactly on the clicked cell.
        let mut terminal = Terminal::new(TestBackend::new(SCREEN.width, SCREEN.height)).unwrap();
        terminal
            .draw(|frame| render(frame, SCREEN, &picker))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(target.x, target.y)].symbol(),
            "\u{253c}"
        );

        let confirm = picker_layout.actions.confirm_button;
        assert_eq!(
            picker.click(Position::new(confirm.x, confirm.y), SCREEN),
            PickerOutcome::Confirm
        );
        let cancel = picker_layout.actions.cancel_button;
        assert_eq!(
            picker.click(Position::new(cancel.x, cancel.y), SCREEN),
            PickerOutcome::Cancel
        );
        // Result rows are hit-tested the same way.
        picker.results = vec![place("A", 1.0, 2.0), place("B", 3.0, 4.0)];
        let second = Position::new(picker_layout.results.x, picker_layout.results.y + 1);
        picker.click(second, SCREEN);
        assert_eq!(picker.candidate.label, "B");
        assert_eq!(picker.focus, PickerFocus::Map);
        picker.click(
            Position::new(picker_layout.input_box.x + 1, picker_layout.input_box.y + 1),
            SCREEN,
        );
        assert_eq!(picker.focus, PickerFocus::Input);
    }

    #[test]
    fn tab_cycles_focus_and_typing_edits_the_input() {
        let mut picker = picker_with(offline());
        assert_eq!(picker.focus, PickerFocus::Input);
        press(&mut picker, KeyCode::Tab);
        assert_eq!(
            picker.focus,
            PickerFocus::Map,
            "results are skipped while empty"
        );
        picker.results = vec![place("A", 0.0, 0.0)];
        press(&mut picker, KeyCode::BackTab);
        assert_eq!(picker.focus, PickerFocus::Results);
        press(&mut picker, KeyCode::Tab);
        press(&mut picker, KeyCode::Tab);
        assert_eq!(picker.focus, PickerFocus::Input);
        type_text(&mut picker, "Rome");
        press(&mut picker, KeyCode::Backspace);
        assert_eq!(picker.input.buf, "Rom");
        picker.paste("e, Italy\n");
        assert_eq!(picker.input.buf, "Rome, Italy");
    }

    #[test]
    fn clicking_a_scrolled_fifth_result_uses_the_visible_row_identity() {
        let mut picker = picker_with(offline());
        picker.results = (0..5)
            .map(|index| place(&format!("Candidate {index}"), 40.0 + f64::from(index), 2.0))
            .collect();
        picker.selected_result = 4;
        let result_rows = layout(SCREEN).results;
        assert_eq!(result_rows.height, RESULT_ROWS);
        assert_eq!(
            layout(SCREEN).visible_result_start(picker.selected_result),
            1
        );
        let first_visible = Position::new(result_rows.x, result_rows.y);
        picker.click(first_visible, SCREEN);
        assert_eq!(picker.candidate.label, "Candidate 1");
        assert_eq!(picker.selected_result, 1);
    }

    #[test]
    fn narrow_picker_keeps_search_and_map_credits_visible_during_an_error() {
        let screen = Rect::new(0, 0, 44, 20);
        for (provider, expected) in [
            (AddressProvider::Photon, "Photon (komoot)"),
            (AddressProvider::Nominatim, "configured Nominatim"),
            (AddressProvider::CityOnly, "GeoNames (CC BY 4.0)"),
            (AddressProvider::Disabled, "Search: disabled"),
        ] {
            let mut picker = LocationPickerState::for_openstreetmap(
                GeoLocation::default(),
                PathBuf::from("/project"),
                &AddressSearchSettings {
                    provider,
                    ..Default::default()
                },
                Some(crate::execution::test_client()),
            );
            picker.status = Some("Address service HTTP 503".into());
            let credit = layout(screen).credit_row;
            assert_eq!(credit.height, 3);
            let mut terminal =
                Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();
            terminal
                .draw(|frame| {
                    render_cursor(frame, screen, &picker);
                })
                .unwrap();
            let mut text = String::new();
            for row in credit.y..credit.bottom() {
                for column in credit.x..credit.right() {
                    text.push_str(terminal.backend().buffer()[(column, row)].symbol());
                }
            }
            assert!(text.contains("Natural Earth (public domain)"));
            assert!(text.contains(expected), "{provider:?}: {text}");
            assert!(
                text.contains("OpenStreetMap contributors")
                    || provider == AddressProvider::CityOnly
                    || provider == AddressProvider::Disabled
            );
        }
    }

    // ---- through the real key and mouse dispatch ----

    fn key(app: &mut App, code: KeyCode) {
        crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn click(app: &mut App, position: Position) {
        crate::mouse::handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    fn stars_app() -> (App, tempfile::TempDir) {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("test".into(), project.path().to_path_buf());
        app.set_screen_area(SCREEN);
        app.animation_settings.kind = AnimationKind::Stars;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        (app, project)
    }

    fn osm_app() -> (App, tempfile::TempDir) {
        let project = tempfile::tempdir().unwrap();
        let mut app = App::new("osm-picker-test".into(), project.path().to_path_buf());
        app.set_screen_area(SCREEN);
        app.animation_settings.kind = AnimationKind::OpenStreetMap;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Animations,
            ..Default::default()
        });
        (app, project)
    }

    fn destination_quota(worker_bytes: usize) -> ilium_execution::QuotaGroup {
        use ilium_execution::{QuotaGroup, QuotaLimits};
        QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 2,
            service_jobs: 0,
            input_bytes: 32 * MIB,
            result_bytes: 8 * MIB,
            worker_threads: 0,
            worker_bytes,
        })
    }

    #[test]
    fn escaped_resolutions_each_keep_separate_admission_and_refusal_is_retryable() {
        let (mut app, project) = stars_app();
        let quota = destination_quota(16 * MIB);
        app.location_destination_quota = quota.clone();
        // Synthetic already-admitted source version; this test isolates the
        // real quota/cache boundary without a second persistence operation.
        app.animation_location_storage =
            Some(Arc::new(quota.reserve_external_storage(4096).unwrap()));
        let first = app.effective_animation_settings().unwrap();
        let repeated = app.effective_animation_settings().unwrap();
        assert!(std::rc::Rc::ptr_eq(&first, &repeated));
        let mut held = vec![first];
        drop(repeated);
        let mut refused = false;
        for _ in 0..1024 {
            let previous_bytes = quota.snapshot().worker_bytes;
            app.bump_tree_version();
            match app.effective_animation_settings() {
                Some(view) => {
                    assert!(quota.snapshot().worker_bytes > previous_bytes);
                    held.push(view);
                }
                None => {
                    assert_eq!(quota.snapshot().worker_bytes, previous_bytes);
                    assert!(app
                        .semantic_animation_error()
                        .unwrap()
                        .contains("storage unavailable"));
                    refused = true;
                    break;
                }
            }
        }
        assert!(refused, "retained deep copies must exhaust finite storage");
        held.clear();
        assert!(
            app.effective_animation_settings().is_some(),
            "refusal remains retryable"
        );
        app.install_animation_project_settings(
            project.path().to_path_buf(),
            Ok(crate::background_animation::AnimationSettings::default()),
        );
        app.reconcile_animation_presentation();
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn confirmation_storage_refusal_preserves_picker_and_authored_location() {
        let (mut app, _project) = stars_app();
        let quota = destination_quota(1);
        app.location_destination_quota = quota.clone();
        app.open_location_picker();
        let Mode::LocationPicker(mut picker) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("picker")
        };
        picker.candidate = place("Refused destination", 48.85, 2.35);
        picker.focus = PickerFocus::Map;
        let previous = app.animation_settings.ambient.location.clone();
        assert_eq!(
            picker.handle_key(KeyCode::Enter, KeyModifiers::NONE, SCREEN),
            PickerOutcome::Confirm
        );
        let candidate = picker.candidate.clone();
        let error = app.confirm_location_picker(&mut picker).unwrap_err();
        assert!(error.contains("storage unavailable"), "{error}");
        assert_eq!(picker.candidate, candidate);
        assert_eq!(app.animation_settings.ambient.location, previous);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn writer_refusal_releases_prepared_copy_without_losing_confirmation() {
        let (mut app, _project) = osm_app();
        let quota = destination_quota(128 * MIB);
        app.location_destination_quota = quota.clone();
        app.configuration_files.as_mut().unwrap().close_admission();
        app.open_location_picker();
        let Mode::LocationPicker(mut picker) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("picker")
        };
        picker.candidate = place("Prepared but refused", 48.85, 2.35);
        picker.focus = PickerFocus::Map;
        let original = app.animation_settings.clone();
        let candidate = picker.candidate.clone();
        assert_eq!(
            picker.handle_key(KeyCode::Enter, KeyModifiers::NONE, SCREEN),
            PickerOutcome::Confirm
        );
        assert!(app.confirm_location_picker(&mut picker).is_err());
        assert_eq!(picker.candidate, candidate);
        assert!(!picker.is_saving());
        assert_eq!(app.animation_settings, original);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn accepted_save_keeps_destination_charge_after_picker_cancel_until_ack_and_replacement() {
        let (mut app, project) = osm_app();
        let quota = destination_quota(128 * MIB);
        app.location_destination_quota = quota.clone();
        crate::project_config::set_animation(project.path(), app.animation_settings.clone())
            .unwrap();
        let lock = ilium_platform::file_lock::ExclusiveFileLock::acquire(
            &project.path().join(".ilium/.config.yaml.lock"),
        )
        .unwrap();
        app.open_location_picker();
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("picker")
        };
        picker.candidate = place("Sydney pending", -33.87, 151.21);
        picker.focus = PickerFocus::Map;
        key(&mut app, KeyCode::Enter);
        let charged = quota.snapshot().worker_bytes;
        assert!(charged > 0);
        assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 1);
        key(&mut app, KeyCode::Esc); // Dismiss the modal, not the accepted write.
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(quota.snapshot().worker_bytes, charged);
        drop(lock);
        app.settle_filesystem_for_test();
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation
                .ambient
                .openstreetmap
                .selected_location()
                .unwrap()
                .label
                .as_str(),
            "Sydney pending"
        );
        assert!(app.committed_animation_location_storage.is_some());
        // Keep one effective Rc outside the cache. Its own lease must
        // survive project replacement, then release when that Rc is dropped.
        let escaped = app.effective_animation_settings().unwrap();
        assert!(quota.snapshot().worker_bytes > 0);
        app.install_animation_project_settings(
            project.path().to_path_buf(),
            Ok(crate::background_animation::AnimationSettings::default()),
        );
        app.reconcile_animation_presentation();
        assert!(quota.snapshot().worker_bytes > 0);
        drop(escaped);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn failed_save_retains_failed_candidate_until_its_owner_releases_it() {
        let (mut app, project) = osm_app();
        let quota = destination_quota(128 * MIB);
        app.location_destination_quota = quota.clone();
        app.open_location_picker();
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("picker")
        };
        picker.candidate = place("Sydney unsaved", -33.87, 151.21);
        picker.focus = PickerFocus::Map;
        std::fs::create_dir_all(project.path().join(".ilium/config.yaml")).unwrap();
        key(&mut app, KeyCode::Enter);
        assert!(quota.snapshot().worker_bytes > 0);
        app.settle_filesystem_for_test();
        assert!(matches!(&app.mode, Mode::LocationPicker(picker) if !picker.is_saving()));
        assert_eq!(
            app.failed_animation_settings
                .as_ref()
                .unwrap()
                .ambient
                .openstreetmap
                .selected_location()
                .unwrap()
                .label
                .as_str(),
            "Sydney unsaved"
        );
        assert!(app.failed_animation_location_storage.is_some());
        key(&mut app, KeyCode::Esc);
        assert!(quota.snapshot().worker_bytes > 0);
        app.failed_animation_settings = None;
        app.failed_animation_location_storage = None;
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn pending_osm_save_freezes_candidate_and_rejects_duplicate_confirmation() {
        let mut picker = picker_with(offline());
        picker.focus = PickerFocus::Map;
        let before = picker.candidate.clone();
        let token = Arc::new(());
        picker.begin_save(Arc::clone(&token));
        assert!(matches!(
            press(&mut picker, KeyCode::Enter),
            PickerOutcome::Continue
        ));
        press(&mut picker, KeyCode::Right);
        picker.paste("replacement");
        let confirm = layout(SCREEN).actions.confirm_button;
        assert!(matches!(
            picker.click(Position::new(confirm.x, confirm.y), SCREEN),
            PickerOutcome::Continue
        ));
        assert_eq!(picker.candidate, before);
        assert!(picker.matches_save(&token));
        assert!(!picker.matches_save(&Arc::new(())));
        picker.fail_save("disk refused".into());
        assert!(!picker.is_saving());
        assert!(matches!(
            press(&mut picker, KeyCode::Enter),
            PickerOutcome::Confirm
        ));
    }

    #[test]
    fn osm_confirmation_waits_for_real_writer_and_does_not_admit_duplicates() {
        let (mut app, project) = osm_app();
        crate::project_config::set_animation(project.path(), app.animation_settings.clone())
            .unwrap();
        let before = std::fs::read(project.path().join(".ilium/config.yaml")).unwrap();
        let lock = ilium_platform::file_lock::ExclusiveFileLock::acquire(
            &project.path().join(".ilium/.config.yaml.lock"),
        )
        .unwrap();
        app.open_location_picker();
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("picker")
        };
        picker.candidate = place("Sydney", -33.87, 151.21);
        picker.focus = PickerFocus::Map;
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::LocationPicker(picker) if picker.is_saving()));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 1);
        assert_eq!(
            std::fs::read(project.path().join(".ilium/config.yaml")).unwrap(),
            before
        );
        drop(lock);
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Settings(_)));
        let saved = crate::project_config::load(project.path())
            .unwrap()
            .animation;
        assert_eq!(
            saved.ambient.openstreetmap.selected_location().unwrap(),
            place("Sydney", -33.87, 151.21)
        );
    }

    #[test]
    fn osm_save_receipt_retains_failure_and_fences_replacement_picker() {
        let (mut app, project) = osm_app();
        app.open_location_picker();
        let token = Arc::new(());
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("picker")
        };
        picker.begin_save(Arc::clone(&token));
        let save = crate::filesystem::configurations::AnimationPickerSave {
            token,
            project_path: project.path().to_path_buf(),
            candidate: picker.candidate.clone(),
            location_storage: None, // synthetic receipt: no accepted write
        };
        app.finish_animation_picker_save(&save, Err("disk refused".into()));
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("failure must retain picker")
        };
        assert!(!picker.is_saving());
        assert_eq!(picker.status.as_deref(), Some("disk refused"));
        let replacement = Arc::new(());
        picker.begin_save(Arc::clone(&replacement));
        app.finish_animation_picker_save(&save, Ok(()));
        assert!(
            matches!(app.mode, Mode::LocationPicker(_)),
            "old token cannot dismiss replacement"
        );
        let replacement_save = crate::filesystem::configurations::AnimationPickerSave {
            token: replacement,
            ..save
        };
        app.finish_animation_picker_save(&replacement_save, Ok(()));
        assert!(matches!(app.mode, Mode::Settings(_)));
    }

    #[test]
    fn osm_keyboard_and_mouse_confirm_only_the_project_map_location() {
        for use_mouse in [false, true] {
            let (mut app, project) = osm_app();
            let observer = app.animation_settings.ambient.location.clone();
            open_from_location_row(&mut app);
            let Mode::LocationPicker(picker) = &mut app.mode else {
                panic!("OSM picker");
            };
            assert!(matches!(
                picker.target(),
                PickerTarget::OpenStreetMap { .. }
            ));
            if use_mouse {
                let position = Position::new(layout(SCREEN).map.x + 5, layout(SCREEN).map.y + 5);
                click(&mut app, position);
                let Mode::LocationPicker(picker) = &app.mode else {
                    panic!("OSM picker");
                };
                let selected = picker.candidate.clone();
                let confirm = layout(SCREEN).actions.confirm_button;
                click(&mut app, Position::new(confirm.x, confirm.y));
                assert_eq!(
                    app.animation_settings
                        .ambient
                        .openstreetmap
                        .selected_location()
                        .unwrap(),
                    selected
                );
            } else {
                for character in "40.7128, -74.006".chars() {
                    key(&mut app, KeyCode::Char(character));
                }
                key(&mut app, KeyCode::Enter);
                key(&mut app, KeyCode::Enter);
                assert!(
                    (app.animation_settings
                        .ambient
                        .openstreetmap
                        .selected_location()
                        .unwrap()
                        .latitude
                        - 40.7128)
                        .abs()
                        < 1e-9
                );
            }
            assert!(matches!(&app.mode, Mode::LocationPicker(picker) if picker.is_saving()));
            app.settle_filesystem_for_test();
            assert!(matches!(app.mode, Mode::Settings(_)));
            assert_eq!(app.animation_settings.ambient.location, observer);
            let saved = crate::project_config::load(project.path())
                .unwrap()
                .animation;
            assert_eq!(saved.ambient.location, observer);
            assert_eq!(
                saved.ambient.openstreetmap,
                app.animation_settings.ambient.openstreetmap
            );
            assert_eq!(saved.ambient.openstreetmap.source, 2);
        }
    }

    #[test]
    fn osm_cancel_and_save_failure_keep_the_previous_location() {
        let (mut app, project) = osm_app();
        let quota = destination_quota(128 * MIB);
        app.location_destination_quota = quota.clone();
        let before = app.animation_settings.ambient.openstreetmap.clone();
        open_from_location_row(&mut app);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.animation_settings.ambient.openstreetmap, before);
        open_from_location_row(&mut app);
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("OSM picker");
        };
        picker.candidate = place("Polar invalid", 89.0, 20.0);
        picker.focus = PickerFocus::Map;
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::LocationPicker(_)));
        assert_eq!(app.animation_settings.ambient.openstreetmap, before);
        assert_eq!(quota.snapshot().worker_bytes, 0);
        assert!(app
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("-85 to 85")));
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("OSM picker");
        };
        picker.candidate = place("Sydney", -33.87, 151.21);
        picker.focus = PickerFocus::Map;
        std::fs::create_dir_all(project.path().join(".ilium/config.yaml")).unwrap();
        key(&mut app, KeyCode::Enter);
        assert!(
            matches!(app.mode, Mode::LocationPicker(_)),
            "failed save keeps the modal open"
        );
        assert!(matches!(&app.mode, Mode::LocationPicker(picker) if picker.is_saving()));
        app.settle_filesystem_for_test();
        assert!(matches!(&app.mode, Mode::LocationPicker(picker) if !picker.is_saving()));
        assert_eq!(
            app.effective_animation_settings()
                .unwrap()
                .ambient
                .openstreetmap,
            before,
            "a failed save cannot change the rendered map"
        );
        assert!(app
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("save")));
    }

    #[test]
    fn osm_picker_confirmation_writes_the_global_settings_whatever_project_is_selected() {
        for use_mouse in [false, true] {
            let home = tempfile::tempdir().unwrap();
            let other = tempfile::tempdir().unwrap();
            let settings = crate::background_animation::AnimationSettings {
                kind: AnimationKind::OpenStreetMap,
                hue_degrees: 17,
                ..Default::default()
            };
            let other_settings = crate::background_animation::AnimationSettings {
                kind: AnimationKind::OpenStreetMap,
                hue_degrees: 219,
                ..Default::default()
            };
            crate::project_config::set_animation(home.path(), settings).unwrap();
            crate::project_config::set_animation(other.path(), other_settings).unwrap();
            let home_before = crate::project_config::load(home.path()).unwrap().animation;
            let other_before = crate::project_config::load(other.path()).unwrap().animation;
            let mut app = App::new("osm-picker-binding".into(), home.path().to_path_buf());
            app.set_screen_area(SCREEN);
            let home_project = app.tree.add_project(home.path().to_path_buf()).unwrap();
            let other_project = app.tree.add_project(other.path().to_path_buf()).unwrap();
            app.select_node(home_project);
            app.install_animation_project_settings(
                home.path().to_path_buf(),
                Ok(home_before.clone()),
            );
            app.mode = Mode::Settings(SettingsState {
                tab: SettingsTab::Animations,
                ..Default::default()
            });
            app.open_location_picker();
            let Mode::LocationPicker(picker) = &mut app.mode else {
                panic!("location picker must open")
            };
            assert!(
                matches!(picker.target(), PickerTarget::OpenStreetMap { project_path, .. }
                if project_path == home.path())
            );
            picker.candidate = place("New York", 40.7128, -74.006);
            picker.focus = PickerFocus::Map;
            // Selecting another project must neither redirect nor drop the save.
            app.select_node(other_project);
            if use_mouse {
                let actions = layout(SCREEN).actions;
                let confirm = actions.confirm_button;
                click(&mut app, Position::new(confirm.x, confirm.y));
            } else {
                key(&mut app, KeyCode::Enter);
            }
            app.settle_filesystem_for_test();
            let saved = crate::project_config::load(home.path()).unwrap().animation;
            assert_ne!(saved, home_before, "the global settings take the new place");
            assert_eq!(
                crate::project_config::load(other.path()).unwrap().animation,
                other_before,
                "a project's old animation block is never rewritten"
            );
        }
    }

    fn open_from_location_row(app: &mut App) {
        let row = app
            .animation_row_model()
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Location)
            .expect("Stars has a Location row");
        let Mode::Settings(state) = &mut app.mode else {
            panic!("Settings is open");
        };
        state.selected_row = row;
        key(app, KeyCode::Enter);
        assert!(
            matches!(app.mode, Mode::LocationPicker(_)),
            "Enter opens the picker"
        );
        assert_eq!(app.modal_stack.len(), 1);
    }

    #[test]
    fn keyboard_flow_from_the_location_row_persists_the_shared_location() {
        let (mut app, project) = stars_app();
        open_from_location_row(&mut app);
        for character in "-33.87, 151.21".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Settings(_)), "the picker closed");
        assert!(app.modal_stack.is_empty());
        // Shared-location settings use the same acknowledged async writer.
        app.settle_filesystem_for_test();
        let location = &app.animation_settings.ambient.location;
        assert!((location.latitude + 33.87).abs() < 1e-9);
        assert_eq!(location.label, "33.870S 151.210E");
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation
                .ambient
                .location,
            *location
        );
        // The Location row now shows the new label and coordinates.
        let model = app.animation_row_model();
        let view = model
            .views()
            .iter()
            .find(|view| view.label == "Location")
            .unwrap();
        assert!(view.value.contains("33.870S 151.210E"), "{}", view.value);
    }

    #[test]
    fn escape_cancels_without_touching_the_location() {
        let (mut app, project) = stars_app();
        let before = app.animation_settings.ambient.location.clone();
        open_from_location_row(&mut app);
        for character in "10, 20".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(app.animation_settings.ambient.location, before);
        assert_eq!(
            crate::project_config::load(project.path())
                .unwrap()
                .animation
                .ambient
                .location,
            GeoLocation::default(),
            "nothing was written"
        );
    }

    #[test]
    fn mouse_flow_clicks_the_map_and_confirms_with_the_button() {
        let (mut app, _project) = stars_app();
        let row = app
            .animation_row_model()
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Location)
            .unwrap();
        let area = crate::settings_ui::compute_layout(SCREEN).content_area;
        let model = app.animation_row_model();
        let scroll = crate::animation_settings_ui::follow_selection(
            area,
            &model,
            row,
            crate::animation_settings_ui::Scrolls::default(),
        );
        let Mode::Settings(state) = &mut app.mode else {
            panic!("Settings is open");
        };
        state.selected_row = row;
        scroll.store(state);
        let row_area = crate::animation_settings_ui::row_rect(area, &model, row, scroll).unwrap();
        click(&mut app, Position::new(row_area.x + 2, row_area.y));
        assert!(
            matches!(app.mode, Mode::LocationPicker(_)),
            "clicking Location opens it"
        );
        let picker_layout = layout(app.layout.screen_area);
        click(
            &mut app,
            Position::new(picker_layout.map.x + 3, picker_layout.map.y + 2),
        );
        let (expected_longitude, expected_latitude) =
            lon_lat_of(3, 2, picker_layout.map.width, picker_layout.map.height);
        let confirm = picker_layout.actions.confirm_button;
        click(&mut app, Position::new(confirm.x, confirm.y));
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(
            app.configuration_admission.accepted, 1,
            "mouse save admission failed: {:?}; status: {:?}",
            app.configuration_admission.rejection, app.status_message
        );
        let location = &app.animation_settings.ambient.location;
        assert!((location.longitude - expected_longitude).abs() < 1e-9);
        assert!((location.latitude - expected_latitude).abs() < 1e-9);
        assert_eq!(location.label, location.coordinate_text());
    }

    #[test]
    fn the_app_tick_delivers_a_finished_search_to_the_open_picker() {
        let (mut app, _project) = stars_app();
        open_from_location_row(&mut app);
        let (sender, receiver) = mpsc::channel();
        let Mode::LocationPicker(picker) = &mut app.mode else {
            panic!("picker open");
        };
        picker.adopt_search_channel(receiver);
        assert!(!app.tick_location_picker());
        // While a search runs the event loop wakes at 10 Hz.
        assert!(app.next_maintenance_delay(Instant::now()) <= Duration::from_millis(100));
        sender
            .send(Ok(vec![place("Kyoto, Japan", 35.01, 135.77)]))
            .unwrap();
        assert!(app.tick_location_picker());
        let Mode::LocationPicker(picker) = &app.mode else {
            panic!("picker open");
        };
        assert_eq!(picker.results[0].label, "Kyoto, Japan");
        // Choosing it with the keyboard stores the geocoded name.
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.animation_settings.ambient.location.label,
            "Kyoto, Japan"
        );
    }

    #[test]
    fn the_picker_renders_inside_a_small_terminal() {
        let picker = picker_with(offline());
        for (width, height) in [(80, 24), (50, 20), (40, 18)] {
            let screen = Rect::new(0, 0, width, height);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render(frame, screen, &picker))
                .unwrap();
            let picker_layout = layout(screen);
            assert!(screen.intersection(picker_layout.popup) == picker_layout.popup);
            assert!(picker_layout.map.height >= 1);
        }
    }
}
