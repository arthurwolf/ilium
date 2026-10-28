//! Test-support rig for the client widget renderer (WID-005..029, HST-250, HST-257).
//! The rig keeps a small widget tree, renders it into a cell grid (wide glyphs take two cells,
//! combining marks none), and routes clicks, hovers, keys and clock ticks back through the
//! rendered regions, so every assertion is made against what is on the screen.
//!
//! It deliberately avoids the host's `testing` feature: this module is part of the normal
//! library build, so it only uses the host's text, sanitiser and manifest-code surface.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use ilium_plugin_api::codes;
use ilium_plugin_api::generated::types::{
    ActionKind, ActionValue, Constraint, UiAction, ViewTarget,
};
use ilium_plugin_host::render::sanitize_plugin_text;
use ilium_plugin_host::text::{fit_to_box, terminal_width, wrap_to_width};

/// The view every rig widget belongs to; actions for any other view are counted as misrouted.
const PLUGIN_VIEW: u32 = 0;
/// Open popups allowed per view (WID-012).
const POPUP_QUOTA: usize = 4;
/// Hover time before a tooltip shows (WID-015).
const TOOLTIP_DELAY_MS: u64 = 500;
/// Longest tooltip, in characters, ellipsis included (WID-015).
const TOOLTIP_LIMIT: usize = 200;
/// Largest image the widget decodes (WID-028).
const IMAGE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
/// Debounce of `changed` for text inputs and short text areas (WID-026).
const DEFAULT_DEBOUNCE_MS: u64 = 600;
/// Debounce of a text area above `LARGE_TEXT_BYTES` (WID-026).
const LARGE_DEBOUNCE_MS: u64 = 1000;
/// Text size above which the longer debounce applies (WID-026).
const LARGE_TEXT_BYTES: usize = 10_000;
/// Diagnostic of a layout whose constraint count differs from its child count (WID-005).
const INVALID_TREE_CODE: &str = "ui.invalid-tree";
/// Pane ids `1..=LIVE_PANE_LIMIT` exist in the rig; any other id was never created.
const LIVE_PANE_LIMIT: u64 = 16;
/// Widgets the manifest must list under `unstable` (WID-022).
const UNSTABLE_WIDGETS: &[&str] = &["calendar"];
/// Block characters of the text-line chart, lowest to highest.
const TEXT_LEVELS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

fn cell_count(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Solves one row of layout constraints into cell widths (WID-006, WID-007).
///
/// Length, min, max, percentage (clamped to 100) and ratio are sized in order, each limited to
/// what is left of the row; fill segments then share the remainder by weight; any cells still
/// unplaced go to the last min segment (or the last segment), so the cells sum to the width.
pub fn solve_row(width: u16, constraints: &[Constraint]) -> Vec<u16> {
    let total = u64::from(width);
    let mut sizes = vec![0u64; constraints.len()];
    let mut remaining = total;
    let mut fill_weight: u64 = 0;
    for (index, constraint) in constraints.iter().enumerate() {
        let wanted = match constraint {
            Constraint::Length(cells) | Constraint::Min(cells) | Constraint::Max(cells) => {
                u64::from(*cells)
            }
            Constraint::Percentage(percent) => total * u64::from((*percent).min(100)) / 100,
            Constraint::Ratio((numerator, denominator)) => {
                if *denominator == 0 {
                    0
                } else {
                    total * u64::from(*numerator) / u64::from(*denominator)
                }
            }
            Constraint::Fill(weight) => {
                fill_weight += u64::from(*weight);
                0
            }
        };
        let granted = wanted.min(remaining);
        sizes[index] = granted;
        remaining -= granted;
    }
    if fill_weight > 0 && remaining > 0 {
        let pool = remaining;
        let mut given = 0;
        for (index, constraint) in constraints.iter().enumerate() {
            if let Constraint::Fill(weight) = constraint {
                let share = pool * u64::from(*weight) / fill_weight;
                sizes[index] = share;
                given += share;
            }
        }
        // Integer division leaves a few cells; hand them out one per fill, left to right.
        let mut left = pool - given;
        for (index, constraint) in constraints.iter().enumerate() {
            if left == 0 {
                break;
            }
            if matches!(constraint, Constraint::Fill(weight) if *weight > 0) {
                sizes[index] += 1;
                left -= 1;
            }
        }
        remaining = left;
    }
    if remaining > 0 {
        let target = constraints
            .iter()
            .rposition(|constraint| matches!(constraint, Constraint::Min(_)))
            .or_else(|| constraints.len().checked_sub(1));
        if let Some(index) = target {
            sizes[index] += remaining;
        }
    }
    sizes
        .into_iter()
        .map(|size| u16::try_from(size).unwrap_or(u16::MAX))
        .collect()
}

/// Installs a plugin using `widget`; `Err` carries the diagnostic code (WID-022).
pub fn install_with_widget(widget: &str, listed_in_unstable: bool) -> Result<(), String> {
    if UNSTABLE_WIDGETS.contains(&widget) && !listed_in_unstable {
        return Err(codes::manifest::UNSTABLE_MISSING.to_string());
    }
    Ok(())
}

/// A recorded `ui-action` event sent to the plugin (WID-008, WID-027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiActionRecord {
    /// Key of the node that was activated.
    pub key: String,
    /// Action id; equals the key for built-in widgets.
    pub action_id: String,
    /// Value carried by the action (for example "true" for a toggle).
    pub value: String,
}

impl UiActionRecord {
    /// Number of fields in the ui-action event (WID-027), counted on the wire form of the
    /// generated `ui-action` record.
    pub fn field_count(&self) -> usize {
        let event = UiAction {
            view: ViewTarget::Panel(PLUGIN_VIEW.to_string()),
            key: self.key.clone(),
            action_id: self.action_id.clone(),
            kind: ActionKind::Activated,
            value: Some(ActionValue::Text(self.value.clone())),
        };
        match serde_json::to_value(&event) {
            Ok(serde_json::Value::Object(fields)) => fields.len(),
            _ => 0,
        }
    }
}

/// One drawn widget of the rig's tree.
#[derive(Debug, Clone)]
enum Node {
    Button { key: String, label: String, tooltip: Option<String> },
    Toggle { key: String, label: String, value: bool },
    Confirm { key: String, label: String },
    TextInput { key: String },
    TextArea { key: String, debounce_ms: Option<u32> },
    Paragraph { text: String, action_key: Option<String> },
    Gauge { ratio: f64 },
    ProgressRow { percent: f32 },
    Chart { values: Vec<f64> },
    Calendar { year: u32, month: u8 },
    Notice { text: String },
    List { key: String, items: u32 },
    Layout { widths: Vec<u16> },
    PointerOnly { label: String },
    FocusItem { key: String, tab_index: Option<i16> },
}

/// A popup layered over the content; it takes no layout space (WID-029).
#[derive(Debug, Clone)]
struct ShownPopup {
    id: String,
    modal: bool,
    text: String,
    action_key: Option<String>,
    row: u16,
    col: u16,
}

/// What the plugin currently displays: content plus the layers Escape unwinds (WID-011).
#[derive(Debug, Clone, Default)]
struct Tree {
    nodes: Vec<Node>,
    popups: Vec<ShownPopup>,
    key_capture: bool,
    selection: bool,
    modal_panel: bool,
}

/// What a click on a region does.
#[derive(Debug, Clone)]
enum Target {
    Activate(String),
    Toggle(usize),
    Focus(String),
    Inert,
}

/// A rectangle of one row that reacts to the pointer.
#[derive(Debug, Clone)]
struct Region {
    row: u16,
    first_cell: u16,
    end_cell: u16,
    target: Target,
    tooltip: Option<String>,
}

/// The rendered cell grid and the pointer regions drawn into it, in paint order.
#[derive(Debug)]
struct Frame {
    cells: Vec<Vec<String>>,
    regions: Vec<Region>,
}

impl Frame {
    fn new(columns: u16, rows: u16) -> Frame {
        Frame {
            cells: vec![vec![" ".to_string(); usize::from(columns)]; usize::from(rows)],
            regions: Vec::new(),
        }
    }

    /// Draws `text` from `col` on `row` and returns the first cell after it. A wide glyph
    /// takes two cells and is dropped when it would cross the right edge; a combining mark
    /// joins the previous cell and takes none.
    fn put(&mut self, row: u16, col: u16, text: &str) -> u16 {
        let Some(line) = self.cells.get_mut(usize::from(row)) else {
            return col;
        };
        let mut at = usize::from(col);
        for glyph in text.chars() {
            let mut buffer = [0u8; 4];
            let glyph_text: &str = glyph.encode_utf8(&mut buffer);
            let width = terminal_width(glyph_text);
            if width == 0 {
                let mut back = at;
                while back > 0 {
                    back -= 1;
                    if !line[back].is_empty() {
                        line[back].push(glyph);
                        break;
                    }
                }
                continue;
            }
            if at + width > line.len() {
                break;
            }
            line[at] = glyph_text.to_string();
            if width == 2 {
                line[at + 1] = String::new();
            }
            at += width;
        }
        cell_count(at)
    }

    fn add_region(&mut self, row: u16, first_cell: u16, end_cell: u16, target: Target) {
        self.regions.push(Region { row, first_cell, end_cell, target, tooltip: None });
    }

    fn rows_as_text(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|line| line.concat().trim_end().to_string())
            .collect()
    }

    /// The first cell where `text` starts on the screen, scanning rows top to bottom.
    fn locate(&self, text: &str) -> Option<(u16, u16)> {
        for (row, line) in self.cells.iter().enumerate() {
            let mut joined = String::new();
            let mut starts: Vec<(usize, usize)> = Vec::new();
            for (cell, glyph) in line.iter().enumerate() {
                if !glyph.is_empty() {
                    starts.push((joined.len(), cell));
                    joined.push_str(glyph);
                }
            }
            if let Some(position) = joined.find(text) {
                if let Some((_, cell)) = starts.iter().find(|(offset, _)| *offset == position) {
                    return Some((cell_count(row), cell_count(*cell)));
                }
            }
        }
        None
    }

    /// The topmost region covering a cell.
    fn region_at(&self, row: u16, cell: u16) -> Option<&Region> {
        self.regions
            .iter()
            .rev()
            .find(|region| region.row == row && region.first_cell <= cell && cell < region.end_cell)
    }
}

/// A popup id the view declares in its tree, and the ones it has open (WID-012, WID-029).
#[derive(Debug, Clone, Default)]
struct ViewPopups {
    declared: Vec<String>,
    open: Vec<String>,
}

/// A pointer hover waiting for the tooltip delay.
#[derive(Debug, Clone)]
struct Hover {
    tooltip: String,
    since_ms: u64,
}

/// Renders a ui-tree into a screen buffer and feeds input back (WID-005..029).
#[derive(Debug)]
pub struct WidgetRig {
    columns: u16,
    rows: u16,
    grants: BTreeSet<String>,
    braille: bool,
    tree: Tree,
    queued: Option<Tree>,
    frame_snapshot: Option<String>,
    blocked: bool,
    now_ms: u64,
    actions: Vec<UiActionRecord>,
    action_views: Vec<u32>,
    closed: Vec<String>,
    scroll: BTreeMap<String, u32>,
    texts: BTreeMap<String, String>,
    pending_changes: BTreeMap<String, u64>,
    focus: Option<String>,
    view_focused: bool,
    hover: Option<Hover>,
    views: Vec<ViewPopups>,
}

impl WidgetRig {
    /// Creates a rig with the given screen size (WID-005). View 0 declares the popups `p0` to
    /// `p9` and `same`; view 1 declares only `same`.
    pub fn new(columns: u16, rows: u16) -> WidgetRig {
        let mut first_view = ViewPopups::default();
        for index in 0..10 {
            first_view.declared.push(format!("p{index}"));
        }
        first_view.declared.push("same".to_string());
        let second_view = ViewPopups { declared: vec!["same".to_string()], open: Vec::new() };
        WidgetRig {
            columns,
            rows,
            grants: BTreeSet::new(),
            braille: false,
            tree: Tree::default(),
            queued: None,
            frame_snapshot: None,
            blocked: false,
            now_ms: 0,
            actions: Vec::new(),
            action_views: Vec::new(),
            closed: Vec::new(),
            scroll: BTreeMap::new(),
            texts: BTreeMap::new(),
            pending_changes: BTreeMap::new(),
            focus: None,
            view_focused: true,
            hover: None,
            views: vec![first_view, second_view],
        }
    }

    /// Grants a capability by name (WID-020).
    pub fn grant(mut self, name: &str) -> WidgetRig {
        self.grants.insert(name.to_string());
        self
    }

    /// Switches a terminal capability such as braille on or off (WID-024).
    pub fn capability(mut self, name: &str, on: bool) -> WidgetRig {
        if name == "braille" {
            self.braille = on;
        }
        self
    }

    // ----- rendering -----

    fn render(&self, tree: &Tree) -> Frame {
        let mut frame = Frame::new(self.columns, self.rows);
        let mut row: u16 = 0;
        for (index, node) in tree.nodes.iter().enumerate() {
            if row >= self.rows {
                break;
            }
            row += self.draw_node(&mut frame, index, node, row);
        }
        for popup in &tree.popups {
            let end = frame.put(popup.row, popup.col, &popup.text);
            let target = match &popup.action_key {
                Some(key) => Target::Activate(key.clone()),
                None => Target::Inert,
            };
            frame.add_region(popup.row, popup.col, end, target);
        }
        frame
    }

    /// Draws one node starting at `row` and returns the number of rows it used.
    fn draw_node(&self, frame: &mut Frame, index: usize, node: &Node, row: u16) -> u16 {
        match node {
            Node::Button { key, label, tooltip } => {
                let end = frame.put(row, 0, label);
                frame.add_region(row, 0, end, Target::Activate(key.clone()));
                if let Some(region) = frame.regions.last_mut() {
                    region.tooltip = tooltip.clone();
                }
                1
            }
            Node::Toggle { label, value, .. } => {
                let mark = if *value { "x" } else { " " };
                let end = frame.put(row, 0, &format!("[{mark}] {label}"));
                frame.add_region(row, 0, end, Target::Toggle(index));
                1
            }
            Node::Confirm { key, label } => {
                let end = frame.put(row, 0, label);
                frame.add_region(row, 0, end, Target::Activate(key.clone()));
                1
            }
            Node::TextInput { key } => {
                let content = self.texts.get(key).filter(|text| !text.is_empty());
                let end = frame.put(row, 0, content.map_or("_", String::as_str));
                frame.add_region(row, 0, end, Target::Focus(key.clone()));
                1
            }
            Node::TextArea { key, .. } => {
                let content = self.texts.get(key).filter(|text| !text.is_empty());
                let lines = wrap_to_width(content.map_or("_", String::as_str), usize::from(self.columns));
                let mut used = 0;
                for line in &lines {
                    let end = frame.put(row + used, 0, line);
                    frame.add_region(row + used, 0, end, Target::Focus(key.clone()));
                    used += 1;
                }
                used.max(1)
            }
            Node::Paragraph { text, action_key } => {
                let mut used: u16 = 0;
                for paragraph in text.split('\n') {
                    for line in wrap_to_width(paragraph, usize::from(self.columns)) {
                        let end = frame.put(row + used, 0, &line);
                        if let Some(key) = action_key {
                            frame.add_region(row + used, 0, end, Target::Activate(key.clone()));
                        }
                        used += 1;
                    }
                }
                used.max(1)
            }
            Node::Gauge { ratio } => {
                let filled = gauge_cells(*ratio, self.columns);
                let empty = self.columns.saturating_sub(filled);
                let bar = "\u{2588}".repeat(usize::from(filled)) + &"\u{2591}".repeat(usize::from(empty));
                frame.put(row, 0, &bar);
                1
            }
            Node::ProgressRow { percent } => {
                let shown = if percent.is_nan() { 0.0 } else { percent.clamp(0.0, 100.0) };
                let label = format!("{shown:.0}% ");
                let start = frame.put(row, 0, &label);
                let room = self.columns.saturating_sub(start);
                let filled = (f64::from(shown) / 100.0 * f64::from(room)).round() as u16;
                frame.put(row, start, &"\u{2588}".repeat(usize::from(filled.min(room))));
                1
            }
            Node::Chart { values } => {
                let line = if self.braille { braille_line(values) } else { text_line(values) };
                frame.put(row, 0, &line);
                1
            }
            Node::Calendar { year, month } => draw_calendar(frame, row, *year, *month),
            Node::Notice { text } => {
                frame.put(row, 0, text);
                1
            }
            Node::List { key, items } => {
                let offset = self.scroll.get(key).copied().unwrap_or(0);
                let visible = u32::from(self.rows.saturating_sub(row)).min(items.saturating_sub(offset));
                for line in 0..visible {
                    frame.put(row + cell_count(line as usize), 0, &format!("item {}", offset + line));
                }
                cell_count(visible as usize).max(1)
            }
            Node::Layout { widths } => {
                let mut at: u16 = 0;
                for (child, width) in widths.iter().enumerate() {
                    frame.put(row, at, &fit_to_box(&format!("child {child}"), usize::from(*width)));
                    at = at.saturating_add(*width);
                }
                1
            }
            Node::PointerOnly { label } => {
                let end = frame.put(row, 0, label);
                frame.add_region(row, 0, end, Target::Inert);
                1
            }
            Node::FocusItem { key, .. } => {
                let end = frame.put(row, 0, key);
                frame.add_region(row, 0, end, Target::Focus(key.clone()));
                1
            }
        }
    }

    /// Whole screen as text (WID-005..029).
    pub fn screen_text(&self) -> String {
        self.screen_rows().join("\n")
    }

    /// Screen rows as text (WID-015).
    pub fn screen_rows(&self) -> Vec<String> {
        self.render(&self.tree).rows_as_text()
    }

    /// Text of the frame currently shown (WID-029).
    pub fn frame_text(&self) -> String {
        match &self.frame_snapshot {
            Some(snapshot) => snapshot.clone(),
            None => self.screen_text(),
        }
    }

    /// Milliseconds one paint took (HST-250, HST-257). A paint never waits for the plugin, so
    /// a blocked plugin costs the same as a live one.
    pub fn paint_frame_ms(&mut self) -> u64 {
        let started = Instant::now();
        let frame = self.render(&self.tree);
        let drawn = frame.cells.len();
        let elapsed = started.elapsed().as_millis();
        debug_assert!(drawn <= usize::from(self.rows));
        u64::try_from(elapsed).unwrap_or(u64::MAX)
    }

    /// Starts a frame, applying queued tree updates (WID-029).
    pub fn begin_frame(&mut self) {
        if let Some(next) = self.queued.take() {
            self.tree = next;
            self.prune_focus();
        }
        self.frame_snapshot = Some(self.screen_text());
    }

    /// Ends the current frame (WID-029).
    pub fn end_frame(&mut self) {
        self.frame_snapshot = None;
    }

    /// Makes the plugin stop answering (HST-250): while blocked it can queue no new tree.
    pub fn block_plugin(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// Advances the virtual clock and fires the debounces and timers that expired
    /// (WID-009, WID-018).
    pub fn advance_ms(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
        let mut due: Vec<(u64, String)> = self
            .pending_changes
            .iter()
            .filter(|(_, deadline)| **deadline <= self.now_ms)
            .map(|(key, deadline)| (*deadline, key.clone()))
            .collect();
        due.sort();
        for (_, key) in due {
            self.pending_changes.remove(&key);
            let value = self.texts.get(&key).cloned().unwrap_or_default();
            self.record_action(&key, value);
        }
    }

    // ----- showing widgets -----

    fn replace_tree(&mut self, nodes: Vec<Node>) {
        self.tree = Tree { nodes, ..Tree::default() };
        self.hover = None;
        self.prune_focus();
    }

    fn replace_with_node(&mut self, node: Node) {
        self.replace_tree(vec![node]);
    }

    fn prune_focus(&mut self) {
        let keys: Vec<String> = focusables(&self.tree).into_iter().map(|(key, _)| key).collect();
        if let Some(current) = &self.focus {
            if !keys.contains(current) {
                self.focus = None;
            }
        }
    }

    /// The plugin asks for focus; the host grants it only to a focused view (HST-271).
    fn request_focus(&mut self, key: &str) {
        if self.view_focused {
            self.focus = Some(key.to_string());
        }
    }

    /// Shows this widget (WID-008).
    pub fn show_button(&mut self, key: &str, label: &str) {
        self.replace_with_node(Node::Button {
            key: key.to_string(),
            label: label.to_string(),
            tooltip: None,
        });
    }

    /// Shows this widget (WID-008).
    pub fn show_button_row(&mut self, key: &str, label: &str) {
        self.show_button(key, label);
    }

    /// Shows this widget (WID-018).
    pub fn show_button_with_tooltip(&mut self, key: &str, label: &str, tooltip: &str) {
        self.replace_with_node(Node::Button {
            key: key.to_string(),
            label: label.to_string(),
            tooltip: Some(tooltip.to_string()),
        });
    }

    /// Shows this widget (WID-008).
    pub fn show_toggle(&mut self, key: &str, label: &str, value: bool) {
        self.replace_with_node(Node::Toggle {
            key: key.to_string(),
            label: label.to_string(),
            value,
        });
    }

    /// Shows this widget (WID-008).
    pub fn show_confirm(&mut self, key: &str, label: &str) {
        self.replace_with_node(Node::Confirm { key: key.to_string(), label: label.to_string() });
    }

    /// Shows this widget (WID-025).
    pub fn show_text_input(&mut self, key: &str) {
        self.replace_with_node(Node::TextInput { key: key.to_string() });
        self.request_focus(key);
    }

    /// Shows this widget (WID-009).
    pub fn show_text_area(&mut self, key: &str, debounce_ms: Option<u32>) {
        self.replace_with_node(Node::TextArea { key: key.to_string(), debounce_ms });
        self.request_focus(key);
    }

    /// Shows this widget (WID-009).
    pub fn show_text_area_with_initial(&mut self, key: &str, debounce_ms: Option<u32>, initial: &str) {
        self.texts.insert(key.to_string(), initial.to_string());
        self.show_text_area(key, debounce_ms);
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph(&mut self, text: &str) {
        self.replace_with_node(Node::Paragraph { text: text.to_string(), action_key: None });
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph_with_action_span(&mut self, key: &str, text: &str) {
        self.replace_with_node(Node::Paragraph {
            text: text.to_string(),
            action_key: Some(key.to_string()),
        });
    }

    /// Shows this widget (WID-023).
    pub fn show_gauge(&mut self, ratio: f64) {
        self.replace_with_node(Node::Gauge { ratio });
    }

    /// Shows this widget (WID-023).
    pub fn show_progress_row(&mut self, percent: f32) {
        self.replace_with_node(Node::ProgressRow { percent });
    }

    /// Shows this widget (WID-024).
    pub fn show_chart(&mut self, values: &[f64]) {
        self.replace_with_node(Node::Chart { values: values.to_vec() });
    }

    /// Shows this widget (WID-022).
    pub fn show_calendar(&mut self, year: u32, month: u8) {
        self.replace_with_node(Node::Calendar { year, month });
    }

    fn show_notice(&mut self, text: &str) {
        self.replace_with_node(Node::Notice { text: text.to_string() });
    }

    /// Shows this widget (WID-019).
    pub fn show_image(&mut self, _asset: &str) {
        // The rig's package holds no assets, so every named asset is missing (WID-028).
        self.show_notice("image not available");
    }

    /// Shows this widget (WID-019).
    pub fn show_image_bytes(&mut self, bytes: &[u8]) {
        if recognised_image(bytes) && bytes.len() <= IMAGE_LIMIT_BYTES {
            self.show_notice(&format!("image {} bytes", bytes.len()));
        } else {
            self.show_notice("image not available");
        }
    }

    /// Shows this widget (WID-019).
    pub fn show_image_of_size(&mut self, bytes: usize) {
        if bytes > IMAGE_LIMIT_BYTES {
            self.show_notice("image not available");
        } else {
            self.show_notice(&format!("image {bytes} bytes"));
        }
    }

    /// Shows this widget (WID-020).
    pub fn show_code_view_foreign_buffer(&mut self) {
        if self.grants.contains("editor.read") {
            self.show_notice("fn main() {}");
        } else {
            self.show_notice("not permitted");
        }
    }

    /// Shows this widget (WID-020).
    pub fn show_code_view_missing_buffer(&mut self) {
        if self.grants.contains("editor.read") {
            self.show_notice("not found");
        } else {
            self.show_notice("not permitted");
        }
    }

    /// Shows this widget (WID-020).
    pub fn show_terminal_view_pane(&mut self, pane: u64) {
        if !self.grants.contains("terminal.read-screen") {
            self.show_notice("not permitted");
        } else if (1..=LIVE_PANE_LIMIT).contains(&pane) {
            self.show_notice(&format!("pane {pane} $ _"));
        } else {
            self.show_notice("not found");
        }
    }

    /// Shows this widget (WID-021).
    pub fn show_list_of(&mut self, key: &str, items: u32) {
        self.replace_with_node(Node::List { key: key.to_string(), items });
    }

    /// Shows this widget (WID-010).
    pub fn show_layout_with_popup(&mut self) {
        let widths = solve_row(self.columns, &[Constraint::Fill(1), Constraint::Fill(1)]);
        self.replace_with_node(Node::Layout { widths });
        self.tree.popups.push(ShownPopup {
            id: "menu".to_string(),
            modal: false,
            text: "Menu".to_string(),
            action_key: None,
            row: 1,
            col: 0,
        });
    }

    /// Shows this widget (WID-010).
    pub fn show_modal_popup(&mut self, key: &str) {
        self.replace_tree(Vec::new());
        self.tree.popups.push(ShownPopup {
            id: key.to_string(),
            modal: true,
            text: format!("Dialog {key}"),
            action_key: None,
            row: 0,
            col: 0,
        });
    }

    /// Shows this widget (WID-010).
    pub fn show_popup_over_button(&mut self, key: &str, under: &str) {
        self.show_button(under, under);
        self.tree.popups.push(ShownPopup {
            id: key.to_string(),
            modal: false,
            text: "Popup".to_string(),
            action_key: Some(key.to_string()),
            row: 0,
            col: 0,
        });
    }

    /// Shows this widget (WID-008).
    pub fn show_pointer_only_node(&mut self, _key: &str, label: &str) {
        self.replace_with_node(Node::PointerOnly { label: label.to_string() });
    }

    /// Shows this widget (WID-011).
    pub fn show_escape_stack(&mut self) {
        self.show_button("escape-anchor", "Anchor");
        self.tree.key_capture = true;
        self.tree.selection = true;
        self.tree.modal_panel = true;
        self.tree.popups.push(ShownPopup {
            id: "escape-popup".to_string(),
            modal: false,
            text: "Menu".to_string(),
            action_key: None,
            row: 1,
            col: 0,
        });
    }

    /// Shows this widget (WID-026).
    pub fn show_focus_order_tree(&mut self, order: &[(&str, Option<i16>)]) {
        let nodes = order
            .iter()
            .map(|(key, tab_index)| Node::FocusItem { key: (*key).to_string(), tab_index: *tab_index })
            .collect();
        self.replace_tree(nodes);
    }

    /// Queues a tree update that the next `begin_frame` applies (WID-029). A blocked plugin
    /// cannot send one.
    pub fn queue_tree_paragraph(&mut self, text: &str) {
        if self.blocked {
            return;
        }
        self.queued = Some(Tree {
            nodes: vec![Node::Paragraph { text: text.to_string(), action_key: None }],
            ..Tree::default()
        });
    }

    /// Shows a layout row; `Err` carries the diagnostic code (WID-005).
    pub fn show_layout(&mut self, children: usize, constraints: &[Constraint]) -> Result<(), String> {
        if children != constraints.len() {
            return Err(INVALID_TREE_CODE.to_string());
        }
        let widths = solve_row(self.columns, constraints);
        self.replace_with_node(Node::Layout { widths });
        Ok(())
    }

    // ----- input -----

    fn record_action(&mut self, key: &str, value: String) {
        self.actions.push(UiActionRecord {
            key: key.to_string(),
            action_id: key.to_string(),
            value,
        });
        self.action_views.push(PLUGIN_VIEW);
    }

    /// Input or popup operation (WID-008).
    pub fn click_text(&mut self, text: &str) {
        let frame = self.render(&self.tree);
        let Some((row, cell)) = frame.locate(text) else {
            return;
        };
        let Some(target) = frame.region_at(row, cell).map(|region| region.target.clone()) else {
            return;
        };
        match target {
            Target::Activate(key) => self.record_action(&key, String::new()),
            Target::Toggle(index) => {
                let mut flipped = None;
                if let Some(Node::Toggle { key, value, .. }) = self.tree.nodes.get_mut(index) {
                    *value = !*value;
                    flipped = Some((key.clone(), *value));
                }
                if let Some((key, value)) = flipped {
                    self.record_action(&key, value.to_string());
                }
            }
            Target::Focus(key) => self.request_focus(&key),
            Target::Inert => {}
        }
    }

    /// Input or popup operation (WID-018).
    pub fn hover_text(&mut self, text: &str) {
        let frame = self.render(&self.tree);
        self.hover = frame
            .locate(text)
            .and_then(|(row, cell)| frame.region_at(row, cell))
            .and_then(|region| region.tooltip.clone())
            .map(|tooltip| Hover {
                tooltip: sanitize_plugin_text(&tooltip, TOOLTIP_LIMIT),
                since_ms: self.now_ms,
            });
    }

    /// Input or popup operation (WID-009).
    pub fn type_text(&mut self, text: &str) {
        let Some(key) = self.focus.clone() else {
            return;
        };
        let explicit = self.tree.nodes.iter().find_map(|node| match node {
            Node::TextArea { key: node_key, debounce_ms } if *node_key == key => Some(*debounce_ms),
            Node::TextInput { key: node_key } if *node_key == key => Some(None),
            _ => None,
        });
        let Some(explicit) = explicit else {
            return;
        };
        let content = self.texts.entry(key.clone()).or_default();
        content.push_str(text);
        let delay = match explicit {
            Some(milliseconds) => u64::from(milliseconds),
            None if content.len() > LARGE_TEXT_BYTES => LARGE_DEBOUNCE_MS,
            None => DEFAULT_DEBOUNCE_MS,
        };
        self.pending_changes.insert(key, self.now_ms + delay);
    }

    /// Input or popup operation (WID-011).
    pub fn press_escape(&mut self) {
        let _ = self.press_escape_and_report_layer();
    }

    /// Input or popup operation (WID-011). Unwinds one layer in the documented order and
    /// reports which one; `None` means Escape was not consumed.
    pub fn press_escape_and_report_layer(&mut self) -> Option<&'static str> {
        if self.tree.key_capture {
            self.tree.key_capture = false;
            return Some("key-capture");
        }
        if self.tree.selection {
            self.tree.selection = false;
            return Some("selection");
        }
        if let Some(popup) = self.tree.popups.pop() {
            debug_assert!(popup.modal || !popup.id.is_empty());
            self.closed.push(popup.id);
            return Some("popup");
        }
        if self.tree.modal_panel {
            self.tree.modal_panel = false;
            return Some("modal-panel");
        }
        None
    }

    /// Input or popup operation (WID-026).
    pub fn press_tab(&mut self) {
        if !self.view_focused {
            return;
        }
        let order = tab_order(&self.tree);
        if order.is_empty() {
            return;
        }
        let next = match self.focus.as_ref().and_then(|key| order.iter().position(|item| item == key)) {
            Some(position) => (position + 1) % order.len(),
            None => 0,
        };
        self.focus = order.get(next).cloned();
    }

    /// Input or popup operation (WID-021).
    pub fn scroll_list(&mut self, key: &str, rows: u32) {
        let items = self.tree.nodes.iter().find_map(|node| match node {
            Node::List { key: node_key, items } if node_key == key => Some(*items),
            _ => None,
        });
        if let Some(items) = items {
            let entry = self.scroll.entry(key.to_string()).or_insert(0);
            *entry = entry.saturating_add(rows).min(items.saturating_sub(1));
        }
    }

    /// Input or popup operation (WID-025).
    pub fn unfocus_view(&mut self) {
        self.view_focused = false;
        self.focus = None;
    }

    /// Input or popup operation (WID-012). Without a view, the id must be declared in exactly
    /// one view.
    pub fn open_popup(&mut self, id: &str) -> Result<(), String> {
        let declaring: Vec<usize> = self
            .views
            .iter()
            .enumerate()
            .filter(|(_, view)| view.declared.iter().any(|declared| declared == id))
            .map(|(index, _)| index)
            .collect();
        match declaring.as_slice() {
            [] => Err("not-found".to_string()),
            [only] => self.open_in_view(*only, id),
            _ => Err("invalid-argument".to_string()),
        }
    }

    /// Input or popup operation (WID-012).
    pub fn open_popup_in_view(&mut self, view: u32, id: &str) -> Result<(), String> {
        let index = usize::try_from(view).unwrap_or(usize::MAX);
        let declared = self
            .views
            .get(index)
            .is_some_and(|candidate| candidate.declared.iter().any(|name| name == id));
        if !declared {
            return Err("not-found".to_string());
        }
        self.open_in_view(index, id)
    }

    fn open_in_view(&mut self, index: usize, id: &str) -> Result<(), String> {
        let open_elsewhere = self
            .views
            .iter()
            .enumerate()
            .any(|(other, view)| other != index && view.open.iter().any(|name| name == id));
        if open_elsewhere {
            return Err("invalid-argument".to_string());
        }
        let Some(view) = self.views.get_mut(index) else {
            return Err("not-found".to_string());
        };
        if view.open.iter().any(|name| name == id) {
            return Ok(());
        }
        if view.open.len() >= POPUP_QUOTA {
            return Err("quota-exceeded".to_string());
        }
        view.open.push(id.to_string());
        Ok(())
    }

    /// Input or popup operation (WID-012). Closing an id that is not open succeeds.
    pub fn close_popup(&mut self, id: &str) -> Result<(), String> {
        for view in &mut self.views {
            view.open.retain(|name| name != id);
        }
        Ok(())
    }

    // ----- state queries -----

    /// State query (WID-026).
    pub fn focused_key(&self) -> Option<String> {
        self.focus.clone()
    }

    /// State query (WID-025). True when the plugin asked for focus and the view could grant it.
    pub fn plugin_requests_focus(&self, key: &str) -> bool {
        self.view_focused && self.focus.as_deref() == Some(key)
    }

    /// State query (WID-011).
    pub fn closed_events(&self) -> Vec<String> {
        self.closed.clone()
    }

    /// State query (WID-008).
    pub fn actions(&self) -> Vec<UiActionRecord> {
        self.actions.clone()
    }

    /// State query (WID-008). Actions that were routed to a view other than the plugin's.
    pub fn actions_for_other_views(&self) -> usize {
        self.action_views.iter().filter(|view| **view != PLUGIN_VIEW).count()
    }

    /// State query (WID-010).
    pub fn layout_child_widths(&self) -> Vec<u16> {
        self.tree
            .nodes
            .iter()
            .find_map(|node| match node {
                Node::Layout { widths } => Some(widths.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// State query (WID-023).
    pub fn gauge_filled_cells(&self) -> u16 {
        self.tree
            .nodes
            .iter()
            .find_map(|node| match node {
                Node::Gauge { ratio } => Some(gauge_cells(*ratio, self.columns)),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// State query (WID-018). The tooltip text once the pointer has rested on its widget for
    /// the hover delay.
    pub fn tooltip_text(&self) -> Option<String> {
        self.hover
            .as_ref()
            .filter(|hover| self.now_ms.saturating_sub(hover.since_ms) >= TOOLTIP_DELAY_MS)
            .map(|hover| hover.tooltip.clone())
    }
}

/// Filled cells of a gauge: the ratio clamped to 0..1 (NaN draws as 0) times the width.
fn gauge_cells(ratio: f64, columns: u16) -> u16 {
    if ratio.is_nan() {
        return 0;
    }
    let filled = (ratio.clamp(0.0, 1.0) * f64::from(columns)).round();
    (filled as u16).min(columns)
}

/// Focusable nodes in tree order with their tab index.
fn focusables(tree: &Tree) -> Vec<(String, Option<i16>)> {
    tree.nodes
        .iter()
        .filter_map(|node| match node {
            Node::Button { key, .. }
            | Node::Toggle { key, .. }
            | Node::Confirm { key, .. }
            | Node::TextInput { key }
            | Node::TextArea { key, .. } => Some((key.clone(), None)),
            Node::FocusItem { key, tab_index } => Some((key.clone(), *tab_index)),
            _ => None,
        })
        .collect()
}

/// Tab order (WID-018): positive indices ascending, then none or 0 in tree order; negative
/// indices are never visited.
fn tab_order(tree: &Tree) -> Vec<String> {
    let mut positive: Vec<(i16, usize, String)> = Vec::new();
    let mut natural: Vec<String> = Vec::new();
    for (position, (key, tab_index)) in focusables(tree).into_iter().enumerate() {
        match tab_index {
            Some(index) if index < 0 => {}
            Some(index) if index >= 1 => positive.push((index, position, key)),
            _ => natural.push(key),
        }
    }
    positive.sort_by_key(|(index, position, _)| (*index, *position));
    positive
        .into_iter()
        .map(|(_, _, key)| key)
        .chain(natural)
        .collect()
}

/// True when the bytes start with the magic number of a format the image widget decodes.
fn recognised_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF8")
        || bytes.starts_with(b"BM")
        || (bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

fn normalised(values: &[f64]) -> Vec<f64> {
    let finite = |value: &f64| value.is_finite();
    let low = values.iter().copied().filter(finite).fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().filter(finite).fold(f64::NEG_INFINITY, f64::max);
    values
        .iter()
        .map(|value| {
            if !value.is_finite() || high <= low {
                0.5
            } else {
                (value - low) / (high - low)
            }
        })
        .collect()
}

/// One braille cell per two values; each value fills 1 to 4 dots of its column from the bottom.
fn braille_line(values: &[f64]) -> String {
    const LEFT: [u32; 4] = [0x40, 0x04, 0x02, 0x01];
    const RIGHT: [u32; 4] = [0x80, 0x20, 0x10, 0x08];
    let levels: Vec<usize> = normalised(values)
        .into_iter()
        .map(|height| (height * 3.0).round() as usize + 1)
        .collect();
    levels
        .chunks(2)
        .filter_map(|pair| {
            let mut bits = 0u32;
            for dot in LEFT.iter().take(pair[0]) {
                bits |= dot;
            }
            for dot in RIGHT.iter().take(pair.get(1).copied().unwrap_or(0)) {
                bits |= dot;
            }
            char::from_u32(0x2800 + bits)
        })
        .collect()
}

/// The text-line fallback: one block character per value.
fn text_line(values: &[f64]) -> String {
    normalised(values)
        .into_iter()
        .map(|height| TEXT_LEVELS[((height * 7.0).round() as usize).min(7)])
        .collect()
}

fn days_in_month(year: u32, month: u8) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
    }
}

/// Column of the first day of the month, Monday first (0 = Monday).
fn first_weekday_monday_first(year: u32, month: u8) -> u32 {
    const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let adjusted = if month < 3 { year.saturating_sub(1) } else { year };
    let sunday_first =
        (adjusted + adjusted / 4 - adjusted / 100 + adjusted / 400 + OFFSETS[usize::from(month - 1)] + 1) % 7;
    (sunday_first + 6) % 7
}

/// Draws a title, the weekday header and the weeks of one month; returns the rows used.
fn draw_calendar(frame: &mut Frame, row: u16, year: u32, month: u8) -> u16 {
    if !(1..=12).contains(&month) {
        frame.put(row, 0, "invalid month");
        return 1;
    }
    frame.put(row, 0, &format!("{year}-{month:02}"));
    frame.put(row + 1, 0, "Mo Tu We Th Fr Sa Su");
    let mut cells: Vec<String> = vec!["  ".to_string(); first_weekday_monday_first(year, month) as usize];
    cells.extend((1..=days_in_month(year, month)).map(|day| format!("{day:>2}")));
    let mut used: u16 = 2;
    for week in cells.chunks(7) {
        frame.put(row + used, 0, &week.join(" "));
        used += 1;
    }
    used
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_api::generated::types::Constraint;

    /// REQ: WID-007
    #[test]
    fn solve_row_fixed_then_fill_takes_the_remaining_width() {
        let widths = solve_row(40, &[Constraint::Length(10), Constraint::Fill(1)]);
        assert_eq!(widths, vec![10, 30]);
        assert_eq!(widths.iter().sum::<u16>(), 40);
    }

    /// REQ: WID-006
    #[test]
    fn solve_row_clamps_percentage_above_100_to_the_full_width() {
        let widths = solve_row(40, &[Constraint::Percentage(150)]);
        assert_eq!(widths, vec![40]);
    }

    /// REQ: WID-005
    #[test]
    fn show_layout_with_mismatched_constraint_count_is_an_error() {
        let mut rig = WidgetRig::new(40, 10);
        let result = rig.show_layout(3, &[Constraint::Length(10), Constraint::Fill(1)]);
        assert!(result.is_err(), "3 children with 2 constraints must fail");
    }

    /// REQ: WID-022
    #[test]
    fn install_with_calendar_not_listed_in_unstable_is_rejected() {
        let result = install_with_widget("calendar", false);
        let diagnostic = result.expect_err("calendar without unstable listing must fail");
        assert!(diagnostic.contains("manifest.unstable-missing"));
    }

    /// REQ: WID-008
    #[test]
    fn clicking_a_toggle_sends_one_ui_action_with_key_as_action_id() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_toggle("dark", "Dark", false);
        rig.click_text("Dark");
        let actions = rig.actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].key, "dark");
        assert_eq!(actions[0].action_id, "dark");
        assert_eq!(actions[0].value, "true");
    }

    /// REQ: WID-027
    #[test]
    fn ui_action_event_has_five_fields() {
        let record = UiActionRecord {
            key: "dark".to_string(),
            action_id: "dark".to_string(),
            value: "true".to_string(),
        };
        assert_eq!(record.field_count(), 5);
    }

    /// REQ: WID-023
    #[test]
    fn gauge_clamps_ratio_above_one_and_draws_nan_as_empty() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_gauge(1.0);
        let full_cells = rig.gauge_filled_cells();
        rig.show_gauge(1.5);
        assert_eq!(rig.gauge_filled_cells(), full_cells);
        rig.show_gauge(f64::NAN);
        assert_eq!(rig.gauge_filled_cells(), 0);
    }

    /// REQ: WID-012
    #[test]
    fn fifth_open_popup_in_one_view_fails_with_quota_exceeded() {
        let mut rig = WidgetRig::new(40, 10);
        for index in 1..=4 {
            let popup_id = format!("p{index}");
            rig.open_popup_in_view(0, &popup_id)
                .expect("the first four popups in a view must open");
        }
        let fifth = rig.open_popup_in_view(0, "p5");
        let diagnostic = fifth.expect_err("the fifth popup in one view must be refused");
        assert!(diagnostic.contains("quota-exceeded"));
    }

    /// REQ: WID-011
    #[test]
    fn escape_closes_modal_popup_and_sends_closed() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_modal_popup("confirm");
        rig.press_escape();
        let closed = rig.closed_events();
        assert!(
            closed.iter().any(|event| event.contains("confirm")),
            "closed events were {closed:?}"
        );
    }

    /// REQ: WID-015
    #[test]
    fn tooltip_longer_than_200_scalars_is_cut_to_200_ending_in_ellipsis() {
        let mut rig = WidgetRig::new(40, 10);
        let long_tooltip = "x".repeat(300);
        rig.show_button_with_tooltip("save", "Save", &long_tooltip);
        rig.hover_text("Save");
        rig.advance_ms(1000);
        let shown = rig.tooltip_text().expect("tooltip must be shown");
        assert_eq!(shown.chars().count(), 200);
        assert!(shown.ends_with('\u{2026}'));
    }
}
