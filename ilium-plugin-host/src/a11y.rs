//! Accessible rendering of plugin views: attribute cues, focus order, screen-reader mode and
//! reduced motion (guide-accessibility, USR-300 .. USR-307).

use std::time::Duration;

use serde_json::Value;

/// The accessibility settings that change how a view is drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct A11ySettings {
    /// Colour is off; meaning must show in attributes.
    pub no_color: bool,
    /// Linear screen-reader layout with role words.
    pub screen_reader: bool,
    /// At most one full redraw per second and no decorative animation.
    pub reduced_motion: bool,
    /// Theme name.
    pub theme: String,
    /// Contrast setting, `normal` or `high`.
    pub contrast: String,
}

/// The text attributes of one cell run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    /// Bold text.
    pub bold: bool,
    /// Reverse video.
    pub reverse: bool,
    /// Underlined text.
    pub underline: bool,
}

/// A key the accessibility screen handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Moves focus to the next focusable element.
    Tab,
    /// Closes a modal dialog.
    Esc,
}

/// One rendered frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    rows: Vec<String>,
    /// The text of each widget with the attributes of its cells, in drawing order.
    runs: Vec<(String, CellStyle)>,
}

const PLAIN: CellStyle = CellStyle {
    bold: false,
    reverse: false,
    underline: false,
};
const FOCUSED: CellStyle = CellStyle {
    bold: true,
    reverse: true,
    underline: false,
};

impl Frame {
    /// Every row of the frame as text.
    pub fn rows(&self) -> Vec<String> {
        self.rows.clone()
    }

    /// The first row containing `needle`.
    pub fn row_containing(&self, needle: &str) -> Option<String> {
        self.rows.iter().find(|row| row.contains(needle)).cloned()
    }

    /// The attributes of the cells that show `needle` (plain when nothing shows it).
    pub fn style_of(&self, needle: &str) -> CellStyle {
        self.runs
            .iter()
            .find(|(text, _)| text.contains(needle))
            .map_or(PLAIN, |(_, style)| *style)
    }
}

/// One widget of a parsed view.
#[derive(Debug, Clone)]
struct Widget {
    id: String,
    kind: String,
    text: String,
    role: String,
    row: usize,
    col: usize,
    selected: bool,
    checked: bool,
}

impl Widget {
    fn is_focusable(&self) -> bool {
        matches!(self.kind.as_str(), "button" | "toggle" | "input" | "list_row")
    }
}

/// A parsed view document.
#[derive(Debug, Clone, Default)]
struct ParsedView {
    widgets: Vec<Widget>,
    modal: bool,
    timer_ms: Option<u64>,
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Parses a view document; an invalid document yields an empty view.
fn parse_view(view_json: &str) -> ParsedView {
    let Ok(document) = serde_json::from_str::<Value>(view_json) else {
        return ParsedView::default();
    };
    let empty = Vec::new();
    let entries = document
        .get("widgets")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let widgets = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| Widget {
            id: text_field(entry, "id").unwrap_or_default(),
            kind: text_field(entry, "kind").unwrap_or_default(),
            text: text_field(entry, "label")
                .filter(|label| !label.is_empty())
                .or_else(|| text_field(entry, "text"))
                .unwrap_or_default(),
            role: text_field(entry, "role").unwrap_or_default(),
            row: entry
                .get("row")
                .and_then(Value::as_u64)
                .map_or(index, |row| row as usize),
            col: entry.get("col").and_then(Value::as_u64).unwrap_or(0) as usize,
            selected: entry.get("selected").and_then(Value::as_bool).unwrap_or(false),
            checked: entry.get("checked").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect();
    ParsedView {
        widgets,
        modal: document.get("modal").and_then(Value::as_bool).unwrap_or(false),
        timer_ms: document.get("timer_ms").and_then(Value::as_u64),
    }
}

/// The text a widget shows. Colour-only meaning always keeps a text cue (USR-300).
fn widget_text(widget: &Widget) -> String {
    let cue = match widget.role.as_str() {
        "danger" | "error" => "! ",
        "success" => "+ ",
        "warning" => "* ",
        _ => "",
    };
    match widget.kind.as_str() {
        "button" => format!("[ {} ]", widget.text),
        "toggle" => format!("[{}] {}", if widget.checked { "x" } else { " " }, widget.text),
        "list_row" if widget.selected => format!("> {}", widget.text),
        "list_row" => format!("  {}", widget.text),
        "spinner" => "[busy]".to_string(),
        _ => format!("{cue}{}", widget.text),
    }
}

/// The attributes of a widget; state cues use attributes, not colour (USR-300, USR-304).
fn widget_style(widget: &Widget, is_focused: bool) -> CellStyle {
    if is_focused {
        return FOCUSED;
    }
    CellStyle {
        bold: matches!(widget.role.as_str(), "danger" | "error") || widget.selected,
        reverse: false,
        underline: matches!(widget.role.as_str(), "danger" | "error"),
    }
}

/// A drawn view and where the focusable elements start.
struct Drawing {
    frame: Frame,
    /// First cell (column, row) of each focusable element in focus order.
    focus_cells: Vec<(u16, u16)>,
}

/// Draws the view with the element at `focus_index` (among the focusable ones) focused.
fn draw(view: &ParsedView, settings: &A11ySettings, focus_index: usize) -> Drawing {
    let focus_order: Vec<usize> = (0..view.widgets.len())
        .filter(|index| view.widgets[*index].is_focusable())
        .collect();
    let focused_widget = focus_order.get(focus_index).copied();
    let mut runs = Vec::new();
    let mut focus_cells = vec![(0u16, 0u16); focus_order.len()];
    let mut rows = Vec::new();

    if settings.screen_reader {
        for (line_index, widget) in view.widgets.iter().enumerate() {
            let position = focus_order.iter().position(|index| *index == line_index);
            let text = reader_line(widget, position, focus_order.len());
            if let Some(position) = position {
                focus_cells[position] = (0, line_index as u16);
            }
            runs.push((text.clone(), widget_style(widget, focused_widget == Some(line_index))));
            rows.push(text);
        }
        return Drawing {
            frame: Frame { rows, runs },
            focus_cells,
        };
    }

    let mut order: Vec<usize> = (0..view.widgets.len()).collect();
    order.sort_by_key(|index| (view.widgets[*index].row, view.widgets[*index].col, *index));
    let mut current_row: Option<usize> = None;
    let mut line = String::new();
    for index in order {
        let widget = &view.widgets[index];
        if current_row != Some(widget.row) {
            if current_row.is_some() {
                rows.push(std::mem::take(&mut line));
            }
            current_row = Some(widget.row);
        }
        if !line.is_empty() {
            line.push_str("  ");
        }
        let text = widget_text(widget);
        if let Some(position) = focus_order.iter().position(|focusable| *focusable == index) {
            focus_cells[position] = (line.chars().count() as u16, rows.len() as u16);
        }
        line.push_str(&text);
        runs.push((text, widget_style(widget, focused_widget == Some(index))));
    }
    if current_row.is_some() {
        rows.push(line);
    }
    Drawing {
        frame: Frame { rows, runs },
        focus_cells,
    }
}

/// One linear screen-reader line with role words and position (USR-305).
fn reader_line(widget: &Widget, position: Option<usize>, focusable_count: usize) -> String {
    let role_word = match widget.kind.as_str() {
        "list_row" => "list row",
        "label" | "" => "text",
        other => other,
    };
    let mut line = format!("{role_word} {}", widget.text);
    if widget.kind == "toggle" {
        line.push_str(if widget.checked { ", checked" } else { ", unchecked" });
    }
    if widget.selected {
        line.push_str(", selected");
    }
    if let Some(position) = position {
        line.push_str(&format!(", {} of {focusable_count}", position + 1));
    }
    line
}

/// Renders a view JSON document into a frame under the settings. An invalid document renders an
/// empty frame; validation is the job of `view::validate_view`.
pub fn render_view(view_json: &str, settings: &A11ySettings) -> Frame {
    draw(&parse_view(view_json), settings, 0).frame
}

/// A timer below this period that only redraws is ignored under reduced motion (USR-307).
const MIN_MOTION_PERIOD_MS: u64 = 1_000;

/// A live accessible screen over one view.
#[derive(Debug)]
pub struct A11yScreen {
    view: ParsedView,
    settings: A11ySettings,
    focus_index: usize,
    is_closed: bool,
    clock_ms: u64,
    next_tick_ms: Option<u64>,
    redraw_times_ms: Vec<u64>,
    log_codes: Vec<String>,
}

impl A11yScreen {
    /// Opens a screen over a view JSON document.
    pub fn open(view_json: &str, settings: &A11ySettings) -> A11yScreen {
        let view = parse_view(view_json);
        let next_tick_ms = view.timer_ms.filter(|period| *period > 0);
        A11yScreen {
            view,
            settings: settings.clone(),
            focus_index: 0,
            is_closed: false,
            clock_ms: 0,
            next_tick_ms,
            redraw_times_ms: Vec::new(),
            log_codes: Vec::new(),
        }
    }

    fn focusable_ids(&self) -> Vec<&str> {
        self.view
            .widgets
            .iter()
            .filter(|widget| widget.is_focusable())
            .map(|widget| widget.id.as_str())
            .collect()
    }

    /// The ids of the focusable elements in Tab order.
    pub fn focus_order(&self) -> Vec<String> {
        self.focusable_ids().into_iter().map(str::to_string).collect()
    }

    /// The id of the focused element (empty when nothing can be focused).
    pub fn focused_id(&self) -> &str {
        self.focusable_ids()
            .get(self.focus_index)
            .copied()
            .unwrap_or("")
    }

    /// Sends a key.
    pub fn press(&mut self, key: Key) {
        match key {
            Key::Tab => {
                let count = self.focusable_ids().len();
                if count > 0 {
                    self.focus_index = (self.focus_index + 1) % count;
                }
            }
            Key::Esc => {
                if self.view.modal {
                    self.is_closed = true;
                }
            }
        }
    }

    /// Whether the screen is closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    fn drawing(&self) -> Drawing {
        draw(&self.view, &self.settings, self.focus_index)
    }

    /// The current frame.
    pub fn frame(&self) -> Frame {
        self.drawing().frame
    }

    /// How many elements show a focus indicator (exactly one while anything is focusable).
    pub fn focus_indicator_count(&self) -> usize {
        usize::from(!self.focusable_ids().is_empty())
    }

    /// The terminal cursor position (column, row); it sits on the focused element (USR-305).
    pub fn cursor_position(&self) -> (u16, u16) {
        self.focused_cell()
    }

    /// The first cell of the focused element (column, row).
    pub fn focused_cell(&self) -> (u16, u16) {
        self.drawing()
            .focus_cells
            .get(self.focus_index)
            .copied()
            .unwrap_or((0, 0))
    }

    /// Advances the screen clock, running the view timer. Under reduced motion at most one full
    /// redraw happens per second and a redraw-only timer below one second is logged once as
    /// `ui.motion-ignored` (USR-307).
    pub fn advance(&mut self, by: Duration) {
        let target_ms = self.clock_ms.saturating_add(by.as_millis() as u64);
        let period = self.view.timer_ms.filter(|period| *period > 0);
        while let (Some(period), Some(tick_ms)) = (period, self.next_tick_ms) {
            if tick_ms > target_ms {
                break;
            }
            self.run_tick(tick_ms, period);
            self.next_tick_ms = Some(tick_ms + period);
        }
        self.clock_ms = target_ms;
    }

    fn run_tick(&mut self, tick_ms: u64, period: u64) {
        if !self.settings.reduced_motion {
            self.redraw_times_ms.push(tick_ms);
            return;
        }
        if period < MIN_MOTION_PERIOD_MS && !self.log_codes.iter().any(|code| code == "ui.motion-ignored") {
            self.log_codes.push("ui.motion-ignored".to_string());
        }
        let last_redraw = self.redraw_times_ms.last().copied();
        let is_allowed = last_redraw.is_none_or(|last| tick_ms.saturating_sub(last) >= MIN_MOTION_PERIOD_MS);
        if is_allowed {
            self.redraw_times_ms.push(tick_ms);
        }
    }

    /// Full-view redraws during the last second.
    pub fn redraws_in_last_second(&self) -> u32 {
        let window_start = self.clock_ms.saturating_sub(MIN_MOTION_PERIOD_MS);
        self.redraw_times_ms
            .iter()
            .filter(|time| **time > window_start)
            .count() as u32
    }

    /// Whether host decorative animations run (they stop under reduced motion).
    pub fn host_decorations_running(&self) -> bool {
        !self.settings.reduced_motion
    }

    /// The log codes written so far.
    pub fn log_codes(&self) -> Vec<String> {
        self.log_codes.clone()
    }
}
