//! Modal picker for the shared observer location used by the stars, city
//! lights and cloud scenes.
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

use std::sync::{
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};
use std::thread::JoinHandle;

use crossterm::event::{KeyCode, KeyModifiers};
use ilium_ambient::{worldmap, GeoLocation};
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
/// (or the whole picker) is dropped: it owns nothing but a sender, so a late
/// answer is simply discarded and no join can ever block the UI.
struct SearchJob {
    receiver: Receiver<SearchResult>,
    _worker: Option<JoinHandle<()>>,
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
        }
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
        });
        self.status = Some("Searching\u{2026}".to_owned());
    }

    fn start_search(&mut self, query: String) {
        let (sender, receiver) = mpsc::channel();
        let searcher = Arc::clone(&self.searcher);
        let worker = std::thread::Builder::new()
            .name("ilium-geocode".to_owned())
            .spawn(move || {
                // A closed receiver only means the picker was dismissed.
                let _ = sender.send(searcher(&query));
            });
        match worker {
            Ok(handle) => {
                self.search = Some(SearchJob {
                    receiver,
                    _worker: Some(handle),
                });
                self.status = Some("Searching\u{2026}".to_owned());
            }
            Err(error) => self.status = Some(format!("Could not start the search: {error}")),
        }
    }

    /// Applies a finished search, if any. Returns whether anything changed.
    pub fn poll_search(&mut self) -> bool {
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
        self.search = None;
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
        for character in text.trim_end_matches(['\r', '\n']).chars() {
            if !character.is_control() {
                text_prompt::handle_key(&mut self.input, KeyCode::Char(character));
            }
        }
        self.focus = PickerFocus::Input;
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
            self.results.clear();
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
                    if text_prompt::handle_key(&mut self.input, code) == PromptOutcome::Commit {
                        self.submit_input();
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
        if let Some(action) = picker_layout.actions.action_at(position) {
            return match action {
                DialogAction::Cancel => PickerOutcome::Cancel,
                DialogAction::Confirm => PickerOutcome::Confirm(self.candidate.clone()),
            };
        }
        if picker_layout.input_box.contains(position) {
            self.focus = PickerFocus::Input;
        } else if let Some(index) = picker_layout.result_at(position, self.results.len()) {
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
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

impl PickerLayout {
    /// The result row under `position`, if any of the `count` results is there.
    pub fn result_at(&self, position: Position, count: usize) -> Option<usize> {
        if !self.results.contains(position) {
            return None;
        }
        let index = usize::from(position.y - self.results.y);
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
        actions: dialog_action_layout(rows[4]),
        hint_row: rows[5],
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

pub fn render(frame: &mut Frame, screen: Rect, picker: &LocationPickerState) {
    let picker_layout = layout(screen);
    frame.render_widget(Clear, picker_layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("Location")),
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
        let first = picker
            .selected_result
            .saturating_sub(usize::from(results.height).saturating_sub(1));
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
    render_dialog_actions(
        frame,
        picker_layout.actions,
        DialogActions::form("Cancel", "Use location"),
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
        frame.set_cursor_position(crate::modal::single_line_cursor_position(
            picker_layout.input_area,
            unicode_width::UnicodeWidthStr::width(prefix.as_str()),
        ));
    }
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
        let scroll = crate::animation_settings_ui::scroll_for_selection(area, &model, row, 0);
        let Mode::Settings(state) = &mut app.mode else {
            panic!("Settings is open");
        };
        state.selected_row = row;
        state.scroll = scroll;
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
