//! Modal world-map picker for the shared observer or an independent OSM map.
//!
//! Three ways to choose a place, all ending in the same candidate that
//! Enter (or the "Use location" button) confirms:
//! * an address, geocoded on a worker thread this state owns;
//! * a direct "lat, lon" entry, parsed locally (no network);
//! * a Braille world map with a crosshair (arrows, Shift for big steps, or a
//!   mouse click).
//!
//! Render and input share [`layout`], so hit-testing can never drift from
//! what is drawn.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};
use std::thread::JoinHandle;

use crossterm::event::{KeyCode, KeyModifiers};
use ilium_ambient::{worldmap, AddressProvider, AddressSearchSettings, GeoLocation};
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

/// A blocking address lookup, run on a worker thread.
pub type Searcher = Arc<dyn Fn(&str) -> Result<Vec<GeoLocation>, String> + Send + Sync>;

type SearchResult = Result<Vec<GeoLocation>, String>;

const MAX_SEARCH_WORKERS: usize = 4;
#[derive(Default)]
struct SearchGate(AtomicUsize);
static SEARCH_GATE: std::sync::OnceLock<Arc<SearchGate>> = std::sync::OnceLock::new();
struct SearchAdmission(Arc<SearchGate>);
impl SearchGate {
    fn acquire(self: &Arc<Self>) -> Option<SearchAdmission> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_SEARCH_WORKERS).then_some(count + 1)
            })
            .ok()
            .map(|_| SearchAdmission(Arc::clone(self)))
    }
}
impl Drop for SearchAdmission {
    fn drop(&mut self) {
        let gate = &self.0;
        gate.0.fetch_sub(1, Ordering::AcqRel);
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
    Confirm(GeoLocation),
}

/// One in-flight address search. The worker thread is detached when the job
/// (or the whole picker) is dropped: the actual worker retains its admission
/// slot through completion, and a late answer is discarded without blocking UI.
struct SearchJob {
    receiver: Receiver<SearchResult>,
    _worker: Option<JoinHandle<()>>,
    query: String,
    input_revision: u64,
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
    search_gate: Arc<SearchGate>,
    target: PickerTarget,
    input_revision: u64,
    pending_save: Option<Arc<()>>,
}

impl LocationPickerState {
    pub fn new(current: GeoLocation) -> Self {
        Self::with_searcher(
            current,
            Arc::new(|query: &str| ilium_ambient::geocode::search(query)),
        )
    }

    pub fn with_searcher(current: GeoLocation, searcher: Searcher) -> Self {
        Self {
            input: TextPromptState::new(""),
            results: Vec::new(),
            selected_result: 0,
            candidate: current.normalized(),
            focus: PickerFocus::Input,
            status: None,
            search: None,
            searcher,
            search_gate: Arc::clone(SEARCH_GATE.get_or_init(|| Arc::new(SearchGate::default()))),
            target: PickerTarget::SharedObserver,
            input_revision: 0,
            pending_save: None,
        }
    }

    pub fn for_openstreetmap(
        current: GeoLocation,
        project_path: PathBuf,
        settings: AddressSearchSettings,
    ) -> Self {
        let search_provider = settings.provider;
        let searcher: Searcher = Arc::new(move |query| {
            ilium_ambient::openstreetmap_address_search::search(&settings, query)
        });
        let mut picker = Self::with_searcher(current, searcher);
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

    /// Test seam: adopts a channel the test feeds directly (no thread, no
    /// network) as the in-flight search.
    #[cfg(test)]
    pub fn adopt_search_channel(&mut self, receiver: Receiver<SearchResult>) {
        self.search = Some(SearchJob {
            receiver,
            _worker: None,
            query: self.input.buf.trim().to_owned(),
            input_revision: self.input_revision,
        });
        self.status = Some("Searching\u{2026}".to_owned());
    }

    fn start_search(&mut self, query: String) {
        if self.search.is_some() {
            self.status = Some("A search is still running; wait before submitting again".into());
            return;
        }
        let Some(admission) = self.search_gate.acquire() else {
            self.status = Some("Address workers are busy; try again shortly".into());
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let searcher = Arc::clone(&self.searcher);
        let worker_query = query.clone();
        let worker = std::thread::Builder::new()
            .name("ilium-geocode".to_owned())
            .spawn(move || {
                let _admission = admission;
                // A closed receiver only means the picker was dismissed.
                let _ = sender.send(searcher(&worker_query));
            });
        match worker {
            Ok(handle) => {
                self.search = Some(SearchJob {
                    receiver,
                    _worker: Some(handle),
                    query,
                    input_revision: self.input_revision,
                });
                self.status = Some("Searching\u{2026}".to_owned());
            }
            Err(error) => self.status = Some(format!("Could not start the search: {error}")),
        }
    }

    /// Applies a finished search, if any. Returns whether anything changed.
    pub fn poll_search(&mut self) -> bool {
        if self.is_saving() {
            return false;
        }
        let Some(job) = &self.search else {
            return false;
        };
        let outcome = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                Err("The address search stopped unexpectedly".to_owned())
            }
        };
        let is_current =
            job.input_revision == self.input_revision && job.query == self.input.buf.trim();
        self.search = None;
        if !is_current {
            self.results.clear();
            self.selected_result = 0;
            self.status =
                Some("Earlier search finished; press Enter to search the current text".into());
            return true;
        }
        match outcome {
            Ok(results) if results.is_empty() => {
                self.results.clear();
                self.status = Some("No matching place found".to_owned());
            }
            Ok(results) => {
                self.status = Some(format!(
                    "{} result{} \u{2014} Up/Down, Enter chooses",
                    results.len(),
                    if results.len() == 1 { "" } else { "s" }
                ));
                self.results = results;
                self.selected_result = 0;
                self.focus = PickerFocus::Results;
            }
            Err(message) => {
                self.results.clear();
                self.status = Some(message);
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
        self.results.clear();
        self.selected_result = 0;
        self.status = None;
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
        let text = self.input.buf.trim().to_owned();
        if text.is_empty() {
            self.status = Some("Type an address or \"lat, lon\" first".to_owned());
            return;
        }
        if let Some(location) = GeoLocation::parse_coordinates(&text) {
            self.candidate = location;
            self.input_changed();
            self.focus = PickerFocus::Map;
            self.status = Some("Coordinates set \u{2014} Enter uses them".to_owned());
            return;
        }
        self.start_search(text);
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
                    KeyCode::Enter => return PickerOutcome::Confirm(self.candidate.clone()),
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
                DialogAction::Confirm => PickerOutcome::Confirm(self.candidate.clone()),
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

    #[test]
    fn worker_slot_survives_picker_drop_and_capacity_recovers_after_completion() {
        let gate = Arc::new(SearchGate::default());
        let held: Vec<_> = (0..MAX_SEARCH_WORKERS - 1)
            .map(|_| gate.acquire().unwrap())
            .collect();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
        let mut picker = picker_with(Arc::new(move |_query| {
            ready_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            Ok(Vec::new())
        }));
        picker.search_gate = Arc::clone(&gate);
        type_text(&mut picker, "held address");
        press(&mut picker, KeyCode::Enter);
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(gate.0.load(Ordering::Acquire), MAX_SEARCH_WORKERS);
        drop(picker);
        assert_eq!(gate.0.load(Ordering::Acquire), MAX_SEARCH_WORKERS);

        let mut blocked = picker_with(offline());
        blocked.search_gate = Arc::clone(&gate);
        type_text(&mut blocked, "another address");
        press(&mut blocked, KeyCode::Enter);
        assert!(!blocked.is_searching());
        assert!(blocked.status.as_deref().unwrap().contains("busy"));

        release_tx.send(()).unwrap();
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(3);
        while gate.0.load(Ordering::Acquire) != 0 {
            assert!(
                Instant::now() < deadline,
                "completed worker retained admission"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let final_slot = gate.acquire().unwrap();
        drop(final_slot);
        assert_eq!(gate.0.load(Ordering::Acquire), 0);
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
        let PickerOutcome::Confirm(location) = press(&mut picker, KeyCode::Enter) else {
            panic!("Enter on the map confirms");
        };
        assert_eq!(location.label, "48.857N 2.352E");
        assert!((location.longitude - 2.352).abs() < 1e-9);
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
        assert_eq!(
            calls,
            vec![("Paris".to_owned(), Some("ilium-geocode".to_owned()))]
        );
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
        let PickerOutcome::Confirm(location) = press(&mut picker, KeyCode::Enter) else {
            panic!("confirming uses the geocoded name");
        };
        assert_eq!(location.label, "Paris, Texas");
        assert!((location.latitude - 33.6609).abs() < 1e-9);
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
        // The detached worker sends into a closed channel and just ends.
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
        let PickerOutcome::Confirm(location) =
            picker.click(Position::new(confirm.x, confirm.y), SCREEN)
        else {
            panic!("Use location confirms");
        };
        assert_eq!(location.label, picker.candidate.label);
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
                AddressSearchSettings {
                    provider,
                    ..Default::default()
                },
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
            PickerOutcome::Confirm(_)
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
    fn osm_picker_confirmation_cannot_write_into_a_different_selected_project() {
        for use_mouse in [false, true] {
            let first = tempfile::tempdir().unwrap();
            let second = tempfile::tempdir().unwrap();
            let first_settings = crate::background_animation::AnimationSettings {
                kind: AnimationKind::OpenStreetMap,
                hue_degrees: 17,
                ..Default::default()
            };
            let second_settings = crate::background_animation::AnimationSettings {
                kind: AnimationKind::OpenStreetMap,
                hue_degrees: 219,
                ..Default::default()
            };
            crate::project_config::set_animation(first.path(), first_settings).unwrap();
            crate::project_config::set_animation(second.path(), second_settings).unwrap();
            let first_before = crate::project_config::load(first.path()).unwrap().animation;
            let second_before = crate::project_config::load(second.path())
                .unwrap()
                .animation;
            let mut app = App::new("osm-picker-binding".into(), first.path().to_path_buf());
            app.set_screen_area(SCREEN);
            let first_id = app.tree.add_project(first.path().to_path_buf()).unwrap();
            let second_id = app.tree.add_project(second.path().to_path_buf()).unwrap();
            app.select_node(first_id);
            app.install_animation_project_settings(
                first.path().to_path_buf(),
                Ok(first_before.clone()),
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
                if project_path == first.path())
            );
            picker.candidate = place("New York", 40.7128, -74.006);
            picker.focus = PickerFocus::Map;
            app.select_node(second_id);
            if use_mouse {
                let actions = layout(SCREEN).actions;
                let confirm = actions.confirm_button;
                click(&mut app, Position::new(confirm.x, confirm.y));
            } else {
                key(&mut app, KeyCode::Enter);
            }
            assert_eq!(
                crate::project_config::load(first.path()).unwrap().animation,
                first_before,
                "the opening project must remain unchanged"
            );
            assert_eq!(
                crate::project_config::load(second.path())
                    .unwrap()
                    .animation,
                second_before,
                "an old picker cannot change the newly selected project"
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
