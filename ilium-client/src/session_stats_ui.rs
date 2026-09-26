//! Rendering, geometry, and hit-testing for the costs-and-stats popover that
//! hangs off the second header icon of an agent pane.
//!
//! The popover has four tabs (Overview, Tokens, Activity, Prompts). Each tab
//! is drawn onto an off-screen [`Buffer`] as tall as its content and then a
//! window of it is copied into the frame, so charts and text scroll together
//! without any widget needing to know it is being clipped.
//!
//! [`geometry`] is a pure function of the screen and anchor cell. Rendering,
//! hover, and click hit-testing all call it, so they cannot drift apart.

use std::time::Instant;

use ilium_core::NodeId;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::ascii_chart::{self, CellKind, ChartConfig, Downsample, LabelFormatter};
use crate::session_stats::{SessionStats, TokenTotals};
use crate::session_stats_store::LoadState;
use crate::theme::{self, ColorScheme};

const POPOVER_MIN_WIDTH: u16 = 56;
const POPOVER_MIN_HEIGHT: u16 = 12;
/// Rows of off-screen canvas. Content past this is dropped, which no tab
/// approaches (the longest is the prompt list at roughly 120 rows).
const CANVAS_ROWS: u16 = 320;
/// The pinned popover's close control, drawn on the top border; the hit
/// rectangle is exactly as wide as this label so they cannot disagree.
const CLOSE_LABEL: &str = " ✕ close ";
const CLOSE_LABEL_WIDTH: u16 = 9;
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The four views of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsTab {
    Overview,
    Tokens,
    Activity,
    Prompts,
}

impl StatsTab {
    pub const ALL: [StatsTab; 4] = [Self::Overview, Self::Tokens, Self::Activity, Self::Prompts];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "◈ Overview",
            Self::Tokens => "◧ Tokens",
            Self::Activity => "▁▃▅ Activity",
            Self::Prompts => "❯ Prompts",
        }
    }

    /// The tab `steps` places after this one, wrapping around.
    pub fn stepped(self, steps: i32) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let count = Self::ALL.len() as i32;
        Self::ALL[(index + steps).rem_euclid(count) as usize]
    }
}

/// What the pointer is over inside the popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsHit {
    Tab(StatsTab),
    Close,
    Body,
}

/// One open popover: hover preview (`pinned == false`) or a pinned window.
#[derive(Debug, Clone)]
pub struct StatsPopover {
    pub pane_id: NodeId,
    pub pinned: bool,
    pub tab: StatsTab,
    pub scroll: u16,
    pub hovered: Option<StatsHit>,
    pub opened_at: Instant,
    /// Last time the tick asked for a redraw of the live clock.
    pub last_clock_redraw: Instant,
    /// Height of the current tab's content and of the visible window, kept
    /// from the last render so wheel scrolling can clamp without re-laying out.
    pub content_rows: u16,
    pub body_rows: u16,
}

impl StatsPopover {
    pub fn new(pane_id: NodeId, pinned: bool, now: Instant) -> Self {
        Self {
            pane_id,
            pinned,
            tab: StatsTab::Overview,
            scroll: 0,
            hovered: None,
            opened_at: now,
            last_clock_redraw: now,
            content_rows: 0,
            body_rows: 0,
        }
    }

    pub fn max_scroll(&self) -> u16 {
        self.content_rows.saturating_sub(self.body_rows)
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let target = i32::from(self.scroll) + delta;
        self.scroll = target.clamp(0, i32::from(self.max_scroll())) as u16;
    }

    pub fn select_tab(&mut self, tab: StatsTab) {
        if self.tab != tab {
            self.tab = tab;
            self.scroll = 0;
        }
    }
}

/// Every clickable or drawn region of one popover, derived from the anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsGeometry {
    pub area: Rect,
    pub tabs: [(StatsTab, Rect); 4],
    pub close: Rect,
    pub body: Rect,
    pub footer: Rect,
}

impl StatsGeometry {
    pub fn hit(&self, position: Position) -> Option<StatsHit> {
        if !self.area.contains(position) {
            return None;
        }
        if self.close.contains(position) {
            return Some(StatsHit::Close);
        }
        if let Some((tab, _)) = self.tabs.iter().find(|(_, rect)| rect.contains(position)) {
            return Some(StatsHit::Tab(*tab));
        }
        Some(StatsHit::Body)
    }
}

/// Lays the popover out below `anchor` (the second header icon), filling
/// `bounds` -- the right-hand panel -- to one cell inside its edges, or returns
/// `None` when the panel is too small to show it legibly. It never extends
/// past the panel, so it cannot cover the tree.
pub fn geometry(bounds: Rect, anchor: Position) -> Option<StatsGeometry> {
    let width = bounds.width.saturating_sub(2);
    let y = anchor.y.saturating_add(1);
    let height = bounds.bottom().saturating_sub(y).saturating_sub(1);
    if width < POPOVER_MIN_WIDTH || height < POPOVER_MIN_HEIGHT {
        return None;
    }
    let area = Rect::new(bounds.x + 1, y, width, height);
    let inner = area.inner(Margin::new(2, 1));

    let mut cursor = inner.x;
    let mut tabs = [(StatsTab::Overview, Rect::default()); 4];
    for (slot, tab) in tabs.iter_mut().zip(StatsTab::ALL) {
        let tab_width = UnicodeWidthStr::width(tab.label()) as u16 + 2;
        *slot = (tab, Rect::new(cursor, inner.y, tab_width, 1));
        cursor = cursor.saturating_add(tab_width + 1);
    }
    let body = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let footer = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let close = Rect::new(
        area.right().saturating_sub(12),
        area.y,
        CLOSE_LABEL_WIDTH,
        1,
    );
    Some(StatsGeometry {
        area,
        tabs,
        close,
        body,
        footer,
    })
}

/// Everything a render needs besides the popover state itself.
pub struct StatsView<'a> {
    pub stats: Option<&'a SessionStats>,
    pub load: &'a LoadState,
    /// Whether the pane's agent keeps a JSONL transcript this module can
    /// read (Claude Code and Codex); other agents have no statistics.
    pub supported: bool,
    /// Whether the pane has resolved its agent session id yet.
    pub has_session: bool,
    pub now_ms: i64,
    pub animation_ms: u128,
    pub scheme: ColorScheme,
}

struct Palette {
    input: Color,
    cache_read: Color,
    cache_write: Color,
    output: Color,
    reasoning: Color,
    good: Color,
    warn: Color,
    bad: Color,
    dim: Color,
    label: Color,
}

fn palette(scheme: ColorScheme) -> Palette {
    match scheme {
        ColorScheme::Dark => Palette {
            input: Color::Rgb(0x7d, 0xcf, 0xff),
            cache_read: Color::Rgb(0x73, 0xda, 0xca),
            cache_write: Color::Rgb(0xe0, 0xaf, 0x68),
            output: Color::Rgb(0xbb, 0x9a, 0xf7),
            reasoning: Color::Rgb(0xf7, 0x76, 0x8e),
            good: Color::Rgb(0x9e, 0xce, 0x6a),
            warn: Color::Rgb(0xe0, 0xaf, 0x68),
            bad: Color::Rgb(0xf7, 0x76, 0x8e),
            dim: Color::Rgb(0x8b, 0x91, 0xa8),
            label: Color::Rgb(0xa9, 0xb1, 0xd6),
        },
        ColorScheme::Light => Palette {
            input: Color::Rgb(0x0b, 0x72, 0xb5),
            cache_read: Color::Rgb(0x0f, 0x7b, 0x6c),
            cache_write: Color::Rgb(0x9a, 0x67, 0x00),
            output: Color::Rgb(0x6d, 0x3f, 0xc0),
            reasoning: Color::Rgb(0xb4, 0x2b, 0x47),
            good: Color::Rgb(0x2f, 0x7d, 0x1f),
            warn: Color::Rgb(0x9a, 0x67, 0x00),
            bad: Color::Rgb(0xb4, 0x2b, 0x47),
            dim: Color::Rgb(0x5b, 0x61, 0x78),
            label: Color::Rgb(0x3b, 0x42, 0x5e),
        },
    }
}

// ------------------------------------------------------------------ formatting

/// Compact count: `842`, `12.3k`, `4.56M`, `1.20B`.
pub fn compact_count(value: u64) -> String {
    let value_f = value as f64;
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}k", value_f / 1e3)
    } else if value < 1_000_000_000 {
        format!("{:.2}M", value_f / 1e6)
    } else {
        format!("{:.2}B", value_f / 1e9)
    }
}

/// Exact count with thin grouping: `1,234,567`.
pub fn grouped_count(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Two-unit duration: `3d 4h`, `2h 14m`, `5m 12s`, `42s`.
pub fn duration_label(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let (days, hours, minutes, secs) = (
        seconds / 86_400,
        seconds % 86_400 / 3600,
        seconds % 3600 / 60,
        seconds % 60,
    );
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn ago_label(now_ms: i64, at_ms: i64) -> String {
    let elapsed = now_ms.saturating_sub(at_ms).max(0) as u64;
    if elapsed < 2_000 {
        "just now".to_string()
    } else {
        format!("{} ago", duration_label(elapsed))
    }
}

fn clock_label(at_ms: i64, with_date: bool) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map_or_else(String::new, |time| {
            time.format(if with_date {
                "%Y-%m-%d %H:%M:%S"
            } else {
                "%H:%M:%S"
            })
            .to_string()
        })
}

fn percent_label(fraction: f64) -> String {
    if fraction > 0.0 && fraction < 0.001 {
        "<0.1%".to_string()
    } else if fraction < 0.1 {
        format!("{:.1}%", fraction * 100.0)
    } else {
        format!("{:.0}%", fraction * 100.0)
    }
}

fn truncate(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let cell = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + cell + 1 > width {
            break;
        }
        out.push(character);
        used += cell;
    }
    out.push('…');
    out
}

fn lerp(from: Color, to: Color, t: f64) -> Color {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        return to;
    };
    let mix =
        |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t.clamp(0.0, 1.0)) as u8;
    Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
}

// --------------------------------------------------------------------- canvas

/// A tall off-screen buffer plus a write cursor: tabs append sections and the
/// visible window is copied out afterwards.
struct Canvas {
    buffer: Buffer,
    width: u16,
    y: u16,
    palette: Palette,
}

impl Canvas {
    fn new(width: u16, background: Style, palette: Palette) -> Self {
        let area = Rect::new(0, 0, width, CANVAS_ROWS);
        let mut buffer = Buffer::empty(area);
        buffer.set_style(area, background);
        Self {
            buffer,
            width,
            y: 0,
            palette,
        }
    }

    fn room(&self, rows: u16) -> bool {
        self.y.saturating_add(rows) <= CANVAS_ROWS
    }

    fn gap(&mut self, rows: u16) {
        self.y = self.y.saturating_add(rows).min(CANVAS_ROWS);
    }

    fn line(&mut self, line: Line<'_>) {
        if !self.room(1) {
            return;
        }
        self.buffer.set_line(0, self.y, &line, self.width);
        self.y += 1;
    }

    fn text(&mut self, text: &str, style: Style) {
        self.line(Line::from(Span::styled(text.to_string(), style)));
    }

    fn heading(&mut self, title: &str, detail: &str) {
        let mut spans = vec![Span::styled(
            title.to_uppercase(),
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD),
        )];
        if !detail.is_empty() {
            spans.push(Span::styled(
                format!("  {detail}"),
                Style::new().fg(self.palette.dim),
            ));
        }
        self.line(Line::from(spans));
    }

    /// A grid of label-over-value tiles, `columns` across.
    fn tiles(&mut self, items: &[(&str, String, Color)], columns: usize) {
        if items.is_empty() || !self.room(2) {
            return;
        }
        let columns = columns.clamp(1, items.len());
        let tile_width = (usize::from(self.width) / columns).max(8);
        for row in items.chunks(columns) {
            let mut labels = Vec::new();
            let mut values = Vec::new();
            for (label, value, color) in row {
                labels.push(Span::styled(
                    pad(&truncate(&label.to_uppercase(), tile_width - 2), tile_width),
                    Style::new().fg(self.palette.dim),
                ));
                values.push(Span::styled(
                    pad(&truncate(value, tile_width - 2), tile_width),
                    Style::new().fg(*color).add_modifier(Modifier::BOLD),
                ));
            }
            self.line(Line::from(labels));
            self.line(Line::from(values));
            self.gap(1);
        }
    }

    /// One proportional bar: `label  ████▌░░░░  value  share`.
    fn bar_row(
        &mut self,
        label: &str,
        label_width: usize,
        fraction: f64,
        color: Color,
        value: &str,
        share: Option<f64>,
    ) {
        let value_width = UnicodeWidthStr::width(value);
        let share_text = share.map_or_else(String::new, percent_label);
        let trailing = value_width + 2 + share_text.len().max(5) + 1;
        let bar_width = usize::from(self.width)
            .saturating_sub(label_width + 2 + trailing)
            .max(6);
        let mut spans = vec![Span::styled(
            pad(&truncate(label, label_width), label_width + 2),
            Style::new().fg(self.palette.label),
        )];
        spans.extend(fraction_bar(fraction, bar_width, color, self.palette.dim));
        spans.push(Span::styled(
            format!("  {value}"),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        if !share_text.is_empty() {
            spans.push(Span::styled(
                format!("  {share_text:>5}"),
                Style::new().fg(self.palette.dim),
            ));
        }
        self.line(Line::from(spans));
    }

    /// An asciichart-style line chart: numeric y-axis, connected box-drawing
    /// lines, one colour per series, an optional legend and time axis.
    fn line_chart(&mut self, chart: &LineChart) {
        let columns = usize::from(self.width);
        let dim = Style::new().fg(self.palette.dim);
        let values: Vec<Vec<f64>> = chart.series.iter().map(|s| s.values.clone()).collect();
        let plot = |data_columns: usize| {
            ascii_chart::plot(
                &values,
                &ChartConfig {
                    width: data_columns,
                    height: usize::from(chart.height),
                    offset: 0,
                    downsample: Downsample::Max,
                    include_zero: chart.include_zero,
                    label_formatter: chart.label_formatter,
                },
            )
        };
        // The gutter (label + axis) depends on the value range, not on the
        // width, so one probe render tells how many columns remain for data.
        let probe = plot(columns.saturating_sub(12).max(8));
        if probe.rows.is_empty() || !self.room(chart.height + 3) {
            self.text("not enough data to draw yet", dim);
            return;
        }
        let mut data_columns = columns.saturating_sub(probe.gutter + 1).max(8);
        let mut rendered = plot(data_columns);
        for _ in 0..3 {
            let excess = rendered.width().saturating_sub(columns);
            if excess == 0 || data_columns <= 8 {
                break;
            }
            data_columns = data_columns.saturating_sub(excess).max(8);
            rendered = plot(data_columns);
        }

        for row in &rendered.rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut run_kind = CellKind::Blank;
            for cell in row {
                if !run.is_empty() && cell.kind != run_kind {
                    spans.push(Span::styled(
                        std::mem::take(&mut run),
                        self.chart_style(run_kind, chart),
                    ));
                }
                run_kind = cell.kind;
                run.push(cell.ch);
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, self.chart_style(run_kind, chart)));
            }
            self.line(Line::from(spans));
        }

        if let Some((first_ms, last_ms)) = chart.time_span {
            let with_date = last_ms - first_ms > 20 * 3_600_000;
            let start = clock_label(first_ms, with_date);
            let end = clock_label(last_ms, with_date);
            let data_width = rendered.width().saturating_sub(rendered.gutter);
            let gap = data_width.saturating_sub(start.len() + end.len());
            self.text(
                &format!(
                    "{}{start}{}{end}",
                    " ".repeat(rendered.gutter),
                    " ".repeat(gap)
                ),
                dim,
            );
        }
        if chart.series.len() > 1 || chart.series.iter().any(|s| !s.name.is_empty()) {
            let mut legend = vec![Span::raw(" ".repeat(rendered.gutter))];
            for series in &chart.series {
                if series.name.is_empty() {
                    continue;
                }
                legend.push(Span::styled("■ ", Style::new().fg(series.color)));
                legend.push(Span::styled(format!("{}   ", series.name), dim));
            }
            self.line(Line::from(legend));
        }
        if let Some(note) = chart.note {
            self.text(note, dim);
        }
    }

    fn chart_style(&self, kind: CellKind, chart: &LineChart) -> Style {
        match kind {
            CellKind::Blank => Style::new(),
            CellKind::Label => Style::new().fg(self.palette.dim),
            CellKind::Axis => Style::new().fg(self.palette.label),
            CellKind::Line(index) => Style::new()
                .fg(chart
                    .series
                    .get(index)
                    .map_or(self.palette.label, |s| s.color))
                .add_modifier(Modifier::BOLD),
        }
    }

    fn wrapped(&mut self, text: &str, indent: u16, style: Style, max_rows: usize) {
        let width = self.width.saturating_sub(indent).max(8);
        let rows = crate::last_prompt_banner::wrap_lines(text, width);
        let total = rows.len();
        for (index, row) in rows.into_iter().take(max_rows).enumerate() {
            let mut row = row;
            if index + 1 == max_rows && total > max_rows {
                row = truncate(&format!("{row} …"), usize::from(width));
            }
            self.line(Line::from(Span::styled(
                format!("{}{row}", " ".repeat(usize::from(indent))),
                style,
            )));
        }
    }
}

/// One line of a [`LineChart`]; `NaN` values leave a gap.
struct LineSeries {
    name: &'static str,
    values: Vec<f64>,
    color: Color,
}

/// What to plot and how, for [`Canvas::line_chart`].
struct LineChart {
    series: Vec<LineSeries>,
    /// Rows the value range is divided into.
    height: u16,
    include_zero: bool,
    label_formatter: Option<LabelFormatter>,
    /// First and last timestamp (ms) to print under the chart.
    time_span: Option<(i64, i64)>,
    note: Option<&'static str>,
}

fn pad(text: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

/// Left-to-right bar with eighth-block resolution on its leading edge.
fn fraction_bar(fraction: f64, width: usize, color: Color, empty: Color) -> Vec<Span<'static>> {
    let eighths = (fraction.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let (full, partial) = (eighths / 8, eighths % 8);
    let mut spans = vec![Span::styled("█".repeat(full), Style::new().fg(color))];
    let mut used = full;
    if partial > 0 && used < width {
        const PARTIALS: [&str; 7] = ["▏", "▎", "▍", "▌", "▋", "▊", "▉"];
        spans.push(Span::styled(PARTIALS[partial - 1], Style::new().fg(color)));
        used += 1;
    }
    spans.push(Span::styled(
        "░".repeat(width.saturating_sub(used)),
        Style::new().fg(lerp(empty, Color::Rgb(0, 0, 0), 0.35)),
    ));
    spans
}

/// One line split into coloured segments proportional to `parts`, using the
/// largest-remainder rule so the cells always add up to `width`.
fn segmented_bar(parts: &[(f64, Color)], width: usize) -> Line<'static> {
    let total: f64 = parts.iter().map(|(share, _)| share).sum();
    if total <= 0.0 || width == 0 {
        return Line::from("░".repeat(width));
    }
    let raw: Vec<f64> = parts
        .iter()
        .map(|(share, _)| share / total * width as f64)
        .collect();
    let mut cells: Vec<usize> = raw.iter().map(|value| value.floor() as usize).collect();
    let mut order: Vec<usize> = (0..raw.len()).collect();
    order.sort_by(|a, b| (raw[*b] - cells[*b] as f64).total_cmp(&(raw[*a] - cells[*a] as f64)));
    let mut leftover = width.saturating_sub(cells.iter().sum());
    for index in order {
        if leftover == 0 {
            break;
        }
        if parts[index].0 > 0.0 {
            cells[index] += 1;
            leftover -= 1;
        }
    }
    Line::from(
        parts
            .iter()
            .zip(cells)
            .map(|((_, color), count)| Span::styled("█".repeat(count), Style::new().fg(*color)))
            .collect::<Vec<_>>(),
    )
}

// ---------------------------------------------------------------------- render

/// Draws the popover and records the content/window heights for scrolling.
pub fn render(
    frame: &mut Frame,
    bounds: Rect,
    anchor: Position,
    popover: &mut StatsPopover,
    view: &StatsView,
) {
    let Some(layout) = geometry(bounds, anchor) else {
        return;
    };
    let colors = palette(view.scheme);
    let background = Style::new().bg(theme::muted_accent_bg(view.scheme));
    let title_style = Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD);

    frame.render_widget(Clear, layout.area);
    let pin_glyph = if popover.pinned { "◆" } else { "◇" };
    let block = theme::block(true)
        .style(background)
        .title(Line::from(vec![Span::styled(
            format!(" {pin_glyph} Costs & stats "),
            title_style,
        )]));
    frame.render_widget(block, layout.area);

    let buffer = frame.buffer_mut();
    if popover.pinned {
        let hovered = popover.hovered == Some(StatsHit::Close);
        let style = if hovered {
            Style::new()
                .fg(theme::accent_fg())
                .bg(colors.bad)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(colors.label).add_modifier(Modifier::BOLD)
        };
        buffer.set_string(layout.close.x, layout.close.y, CLOSE_LABEL, style);
    }

    for (tab, rect) in &layout.tabs {
        let active = *tab == popover.tab;
        let hovered = popover.hovered == Some(StatsHit::Tab(*tab));
        let style = if active {
            Style::new()
                .fg(theme::accent_fg())
                .bg(theme::accent_bg())
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::new()
                .fg(colors.label)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::new().fg(colors.dim)
        };
        buffer.set_string(rect.x, rect.y, format!(" {} ", tab.label()), style);
    }
    let rule_y = layout.body.y.saturating_sub(1);
    buffer.set_string(
        layout.body.x,
        rule_y,
        "─".repeat(usize::from(layout.body.width)),
        Style::new().fg(colors.dim),
    );

    let content_width = layout.body.width.saturating_sub(1);
    let mut canvas = Canvas::new(content_width, background, colors);
    draw_tab(&mut canvas, popover.tab, view);
    popover.content_rows = canvas.y;
    popover.body_rows = layout.body.height;
    popover.scroll = popover.scroll.min(popover.max_scroll());

    blit(
        frame.buffer_mut(),
        &canvas.buffer,
        layout.body,
        content_width,
        popover.scroll,
    );
    draw_scrollbar(frame.buffer_mut(), layout.body, popover, view.scheme);
    draw_footer(frame.buffer_mut(), layout.footer, popover, view);
}

fn blit(target: &mut Buffer, source: &Buffer, body: Rect, width: u16, scroll: u16) {
    for row in 0..body.height {
        let source_y = row + scroll;
        if source_y >= CANVAS_ROWS {
            break;
        }
        for column in 0..width.min(body.width) {
            if let Some(cell) = target.cell_mut((body.x + column, body.y + row)) {
                *cell = source[(column, source_y)].clone();
            }
        }
    }
}

fn draw_scrollbar(buffer: &mut Buffer, body: Rect, popover: &StatsPopover, scheme: ColorScheme) {
    if popover.max_scroll() == 0 || body.height < 3 {
        return;
    }
    let colors = palette(scheme);
    let x = body.right().saturating_sub(1);
    let track = f64::from(body.height);
    let thumb = (track * f64::from(popover.body_rows) / f64::from(popover.content_rows))
        .round()
        .clamp(1.0, track);
    let offset =
        ((track - thumb) * f64::from(popover.scroll) / f64::from(popover.max_scroll())).round();
    for row in 0..body.height {
        let in_thumb = f64::from(row) >= offset && f64::from(row) < offset + thumb;
        buffer.set_string(
            x,
            body.y + row,
            if in_thumb { "┃" } else { "│" },
            Style::new().fg(if in_thumb {
                theme::accent_bg()
            } else {
                lerp(colors.dim, Color::Rgb(0, 0, 0), 0.5)
            }),
        );
    }
}

fn draw_footer(buffer: &mut Buffer, footer: Rect, popover: &StatsPopover, view: &StatsView) {
    let colors = palette(view.scheme);
    let hint = if popover.pinned {
        "pinned · wheel scrolls · click ● or ✕ to close"
    } else {
        "preview · click ● to pin"
    };
    let status = match (view.load, view.stats) {
        (LoadState::Loading { done, total }, _) if *total > 0 => {
            let frame = FRAMES[(view.animation_ms / 90) as usize % FRAMES.len()];
            format!(
                "{frame} reading {:.0}% of {}",
                *done as f64 / *total as f64 * 100.0,
                byte_label(*total)
            )
        }
        (LoadState::Loading { .. }, _) => {
            format!(
                "{} locating transcript",
                FRAMES[(view.animation_ms / 90) as usize % FRAMES.len()]
            )
        }
        (_, Some(stats)) => format!("{} transcript", byte_label(stats.bytes_read)),
        _ => String::new(),
    };
    buffer.set_string(footer.x, footer.y, hint, Style::new().fg(colors.dim));
    let status_width = UnicodeWidthStr::width(status.as_str()) as u16;
    if status_width + 4 < footer.width.saturating_sub(hint.len() as u16) {
        buffer.set_string(
            footer.right().saturating_sub(status_width),
            footer.y,
            status,
            Style::new().fg(colors.dim),
        );
    }
}

fn byte_label(bytes: u64) -> String {
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", value / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", value / 1048576.0)
    } else {
        format!("{:.2} GB", value / 1073741824.0)
    }
}

// ------------------------------------------------------------------------ tabs

fn draw_tab(canvas: &mut Canvas, tab: StatsTab, view: &StatsView) {
    let Some(stats) = view.stats else {
        draw_placeholder(canvas, view);
        return;
    };
    match tab {
        StatsTab::Overview => draw_overview(canvas, stats, view),
        StatsTab::Tokens => draw_tokens(canvas, stats),
        StatsTab::Activity => draw_activity(canvas, stats, view),
        StatsTab::Prompts => draw_prompts(canvas, stats, view),
    }
}

fn draw_placeholder(canvas: &mut Canvas, view: &StatsView) {
    let dim = Style::new().fg(canvas.palette.dim);
    canvas.gap(1);
    if !view.supported {
        canvas.text(
            "No statistics for this agent",
            Style::new().add_modifier(Modifier::BOLD),
        );
        canvas.gap(1);
        canvas.wrapped(
            "Statistics are read from the agent's own JSONL session transcript, which \
             only Claude Code and Codex keep in a readable form.",
            0,
            dim,
            6,
        );
        return;
    }
    match view.load {
        LoadState::Unavailable(reason) => {
            canvas.text(
                "No statistics available",
                Style::new().add_modifier(Modifier::BOLD),
            );
            canvas.gap(1);
            canvas.wrapped(reason, 0, dim, 6);
        }
        _ if !view.has_session => {
            canvas.text(
                "Waiting for the agent's session",
                Style::new().add_modifier(Modifier::BOLD),
            );
            canvas.gap(1);
            canvas.wrapped(
                "The agent has not reported a session id yet. Statistics appear as soon \
                 as its transcript can be matched to this pane.",
                0,
                dim,
                6,
            );
        }
        _ => {
            let frame = FRAMES[(view.animation_ms / 90) as usize % FRAMES.len()];
            canvas.text(
                &format!("{frame} Reading the session transcript…"),
                Style::new().add_modifier(Modifier::BOLD),
            );
            if let LoadState::Loading { done, total } = view.load {
                if *total > 0 {
                    canvas.gap(1);
                    let fraction = *done as f64 / *total as f64;
                    let mut spans = fraction_bar(
                        fraction,
                        usize::from(canvas.width).saturating_sub(10),
                        theme::accent_bg(),
                        canvas.palette.dim,
                    );
                    spans.push(Span::raw(format!(" {:>3.0}%", fraction * 100.0)));
                    canvas.line(Line::from(spans));
                }
            }
        }
    }
}

fn draw_overview(canvas: &mut Canvas, stats: &SessionStats, view: &StatsView) {
    let colors_dim = canvas.palette.dim;
    let (good, warn, label) = (
        canvas.palette.good,
        canvas.palette.warn,
        canvas.palette.label,
    );
    let model = stats.model.as_deref().unwrap_or("model unknown");
    let provider = stats.provider.as_deref().unwrap_or("agent");
    let version = stats
        .cli_version
        .as_deref()
        .map_or_else(String::new, |version| format!(" {version}"));
    canvas.line(Line::from(vec![
        Span::styled(
            model.to_string(),
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {provider}{version}"),
            Style::new().fg(colors_dim),
        ),
        activity_chip(stats, view, good, colors_dim),
    ]));
    let mut facts = Vec::new();
    if let Some(effort) = &stats.effort {
        facts.push(format!("effort {effort}"));
    }
    if let Some(advisor) = &stats.advisor_model {
        facts.push(format!("advisor {advisor}"));
    }
    if let Some(plan) = &stats.plan_type {
        facts.push(format!("plan {plan}"));
    }
    if let Some(branch) = &stats.git_branch {
        facts.push(format!("branch {branch}"));
    }
    if !facts.is_empty() {
        canvas.text(&facts.join("  ·  "), Style::new().fg(colors_dim));
    }
    canvas.gap(1);

    let running = stats.first_at_ms.map_or_else(
        || "—".to_string(),
        |first| duration_label(view.now_ms.saturating_sub(first).max(0) as u64),
    );
    let active = if stats.active_ms > 0 {
        let span = stats.span_ms().unwrap_or(0).max(1);
        format!(
            "{} ({})",
            duration_label(stats.active_ms),
            percent_label((stats.active_ms as f64 / span as f64).min(1.0))
        )
    } else {
        "—".to_string()
    };
    let last = stats
        .last_at_ms
        .map_or_else(|| "—".to_string(), |last| ago_label(view.now_ms, last));
    let started = stats
        .first_at_ms
        .map_or_else(|| "—".to_string(), |first| clock_label(first, true));
    let input_color = canvas.palette.input;
    let output_color = canvas.palette.output;
    canvas.tiles(
        &[
            ("Running for", running, good),
            ("Active time", active, output_color),
            ("Last activity", last, input_color),
            ("Started", started, label),
        ],
        canvas_columns(canvas.width, 4),
    );
    let failed = stats
        .tool_errors
        .filter(|errors| *errors > 0)
        .map_or_else(String::new, |errors| format!(" ({errors} failed)"));
    canvas.tiles(
        &[
            (
                "Prompts",
                grouped_count(u64::from(stats.prompt_count)),
                label,
            ),
            (
                "Turns",
                grouped_count(u64::from(stats.turns_completed)),
                label,
            ),
            (
                "Model calls",
                grouped_count(u64::from(stats.model_calls)),
                label,
            ),
            (
                "Tool calls",
                format!("{}{failed}", grouped_count(u64::from(stats.tool_calls))),
                if failed.is_empty() { label } else { warn },
            ),
        ],
        canvas_columns(canvas.width, 4),
    );

    canvas.heading(
        "Tokens",
        &format!("{} total", grouped_count(stats.tokens.total())),
    );
    token_legend(canvas, &stats.tokens);
    canvas.gap(1);

    canvas.heading("Context window", "");
    match (
        stats.context_tokens,
        stats.context_window,
        stats.context_fill(),
    ) {
        (Some(used), Some(window), Some(fill)) => {
            let color = if fill > 0.9 {
                canvas.palette.bad
            } else if fill > 0.7 {
                warn
            } else {
                good
            };
            canvas.bar_row(
                "in use",
                8,
                fill,
                color,
                &format!("{} / {}", compact_count(used), compact_count(window)),
                Some(fill),
            );
        }
        (Some(used), _, _) => canvas.text(
            &format!(
                "{} tokens in the latest prompt · peak {}",
                compact_count(used),
                compact_count(stats.peak_context_tokens)
            ),
            Style::new(),
        ),
        _ => canvas.text("not reported yet", Style::new().fg(colors_dim)),
    }
    canvas.gap(1);

    draw_cost(canvas, stats);
    draw_rate_limits(canvas, stats, view);

    let buckets = stats.timeline(slice_count(canvas.width, stats));
    if buckets.iter().any(|bucket| bucket.tokens.output > 0) {
        canvas.heading("Output over time", "tokens per slice");
        canvas.line_chart(&LineChart {
            series: vec![LineSeries {
                name: "output tokens",
                values: buckets.iter().map(|b| b.tokens.output as f64).collect(),
                color: output_color,
            }],
            height: 7,
            include_zero: true,
            label_formatter: Some(compact_axis),
            time_span: time_span(stats),
            note: None,
        });
    }
}

/// How many time slices a chart of `width` cells should use: enough to show
/// structure, never so many that a short session is drawn as isolated ticks.
fn slice_count(width: u16, stats: &SessionStats) -> usize {
    let cells = usize::from(width).saturating_sub(14).clamp(8, 240);
    let events = stats.samples.len().max(stats.activity_ms.len() / 2);
    events.clamp(12, cells)
}

/// A green pulse while the transcript is still growing, a dim idle time after.
fn activity_chip(
    stats: &SessionStats,
    view: &StatsView,
    live: Color,
    idle: Color,
) -> Span<'static> {
    let Some(last) = stats.last_at_ms else {
        return Span::raw("");
    };
    let quiet_ms = view.now_ms.saturating_sub(last).max(0) as u64;
    if quiet_ms < 20_000 {
        // Two-beat pulse so the chip reads as alive even in a still frame.
        let beat = if (view.animation_ms / 600).is_multiple_of(2) {
            "●"
        } else {
            "◉"
        };
        Span::styled(
            format!("   {beat} active now"),
            Style::new().fg(live).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!("   ○ idle for {}", duration_label(quiet_ms)),
            Style::new().fg(idle),
        )
    }
}

fn canvas_columns(width: u16, wanted: usize) -> usize {
    usize::from(width / 20).clamp(1, wanted)
}

fn token_legend(canvas: &mut Canvas, tokens: &TokenTotals) {
    let colors = &canvas.palette;
    let (input, cache_read, cache_write, output, dim) = (
        colors.input,
        colors.cache_read,
        colors.cache_write,
        colors.output,
        colors.dim,
    );
    let parts = [
        (tokens.input as f64, input),
        (tokens.cache_read as f64, cache_read),
        (tokens.cache_write as f64, cache_write),
        (tokens.output as f64, output),
    ];
    canvas.line(segmented_bar(&parts, usize::from(canvas.width)));
    let total = tokens.total().max(1) as f64;
    let entry = |name: &str, value: u64, color: Color| {
        vec![
            Span::styled("■ ", Style::new().fg(color)),
            Span::raw(format!("{name} ")),
            Span::styled(
                format!("{} ", compact_count(value)),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}   ", percent_label(value as f64 / total)),
                Style::new().fg(dim),
            ),
        ]
    };
    let mut legend = Vec::new();
    legend.extend(entry("input", tokens.input, input));
    legend.extend(entry("cache read", tokens.cache_read, cache_read));
    legend.extend(entry("cache write", tokens.cache_write, cache_write));
    legend.extend(entry("output", tokens.output, output));
    canvas.line(Line::from(legend));
    if let Some(ratio) = tokens.cache_hit_ratio() {
        canvas.line(Line::from(vec![
            Span::styled(
                format!(
                    "{} of prompt tokens served from cache",
                    percent_label(ratio)
                ),
                Style::new().fg(dim),
            ),
            Span::styled(
                if tokens.reasoning > 0 {
                    format!("  ·  {} reasoning tokens", compact_count(tokens.reasoning))
                } else {
                    String::new()
                },
                Style::new().fg(dim),
            ),
        ]));
    }
}

fn draw_cost(canvas: &mut Canvas, stats: &SessionStats) {
    let dim = Style::new().fg(canvas.palette.dim);
    let (warn, label) = (canvas.palette.warn, canvas.palette.label);
    let Some(cost) = &stats.cost else {
        canvas.heading("Cost", "");
        let note = if stats.provider.as_deref() == Some("Anthropic") {
            "Claude Code has not recorded a cost snapshot for this session yet. It writes \
             one periodically, not on every call."
        } else {
            "This agent's transcript does not record dollar cost, so none is shown."
        };
        canvas.wrapped(note, 0, dim, 3);
        canvas.gap(1);
        return;
    };
    canvas.heading("Cost", "as recorded by Claude Code");
    canvas.line(Line::from(vec![
        Span::styled(
            format!("${:.2}", cost.total_usd),
            Style::new()
                .fg(canvas.palette.good)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "   +{} / −{} lines   API {}   tools {}",
                grouped_count(cost.lines_added),
                grouped_count(cost.lines_removed),
                duration_label(cost.api_duration_ms),
                duration_label(cost.tool_duration_ms)
            ),
            dim,
        ),
    ]));
    let peak = cost.model_costs.first().map_or(0.0, |(_, usd)| *usd);
    for (model, usd) in cost.model_costs.iter().take(4) {
        if peak > 0.0 {
            canvas.bar_row(
                &short_model(model),
                14,
                usd / peak,
                label,
                &format!("${usd:.2}"),
                (cost.total_usd > 0.0).then(|| usd / cost.total_usd),
            );
        }
    }
    if cost.has_unknown_model_cost {
        canvas.text(
            "Some models had no known price in this snapshot.",
            Style::new().fg(warn),
        );
    }
    canvas.text(
        "Claude Code writes this snapshot periodically, so it can trail live usage.",
        dim,
    );
    canvas.gap(1);
}

fn draw_rate_limits(canvas: &mut Canvas, stats: &SessionStats, view: &StatsView) {
    if stats.rate_limits.is_empty() {
        return;
    }
    let (good, warn, bad) = (canvas.palette.good, canvas.palette.warn, canvas.palette.bad);
    canvas.heading("Rate limits", "as reported by the provider");
    for (name, window) in &stats.rate_limits {
        let fraction = (window.used_percent / 100.0).clamp(0.0, 1.0);
        let color = if fraction > 0.9 {
            bad
        } else if fraction > 0.7 {
            warn
        } else {
            good
        };
        let span = window.window_minutes.map_or_else(String::new, |minutes| {
            if minutes % 1440 == 0 {
                format!("{}-day", minutes / 1440)
            } else if minutes % 60 == 0 {
                format!("{}-hour", minutes / 60)
            } else {
                format!("{minutes}-minute")
            }
        });
        let reset = window.resets_at_unix.map_or_else(String::new, |unix| {
            let remaining = (unix * 1000 - view.now_ms).max(0) as u64;
            format!("resets in {}", duration_label(remaining))
        });
        canvas.bar_row(
            format!("{name} {span}").trim(),
            14,
            fraction,
            color,
            &reset,
            Some(fraction),
        );
    }
    canvas.gap(1);
}

fn short_model(model: &str) -> String {
    let trimmed = model.strip_prefix("claude-").unwrap_or(model);
    // A trailing dated suffix such as `-20251001` adds width, not meaning.
    match trimmed.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => trimmed.to_string(),
    }
}

fn time_span(stats: &SessionStats) -> Option<(i64, i64)> {
    Some((stats.first_at_ms?, stats.last_at_ms?))
}

/// Y-axis label for token counts: `137.0k`, `4.56M`.
fn compact_axis(value: f64) -> String {
    compact_count(value.max(0.0).round() as u64)
}

/// Y-axis label for plain counts.
fn count_axis(value: f64) -> String {
    format!("{:.0}", value.max(0.0).round())
}

/// Y-axis label for millisecond durations.
fn duration_axis(value: f64) -> String {
    duration_label(value.max(0.0).round() as u64)
}

fn draw_tokens(canvas: &mut Canvas, stats: &SessionStats) {
    let colors = (
        canvas.palette.input,
        canvas.palette.cache_read,
        canvas.palette.cache_write,
        canvas.palette.output,
        canvas.palette.reasoning,
        canvas.palette.dim,
    );
    let tokens = &stats.tokens;
    canvas.heading(
        "Breakdown",
        &format!("{} tokens", grouped_count(tokens.total())),
    );
    let total = tokens.total().max(1) as f64;
    let rows = [
        ("fresh input", tokens.input, colors.0),
        ("cache read", tokens.cache_read, colors.1),
        ("cache write", tokens.cache_write, colors.2),
        ("output", tokens.output, colors.3),
    ];
    let peak = rows.iter().map(|row| row.1).max().unwrap_or(0).max(1) as f64;
    for (name, value, color) in rows {
        canvas.bar_row(
            name,
            12,
            value as f64 / peak,
            color,
            &format!("{} ", compact_count(value)),
            Some(value as f64 / total),
        );
    }
    if tokens.reasoning > 0 {
        canvas.bar_row(
            "└ reasoning",
            12,
            tokens.reasoning as f64 / peak,
            colors.4,
            &format!("{} ", compact_count(tokens.reasoning)),
            (tokens.output > 0).then(|| tokens.reasoning as f64 / tokens.output as f64),
        );
    }
    canvas.text(
        &format!(
            "exact: input {} · cache read {} · cache write {} · output {}",
            grouped_count(tokens.input),
            grouped_count(tokens.cache_read),
            grouped_count(tokens.cache_write),
            grouped_count(tokens.output)
        ),
        Style::new().fg(colors.5),
    );
    canvas.gap(1);

    canvas.heading("Averages", "");
    let calls = u64::from(stats.model_calls).max(1);
    let prompts = u64::from(stats.prompt_count).max(1);
    let label = canvas.palette.label;
    canvas.tiles(
        &[
            (
                "Output per call",
                compact_count(tokens.output / calls),
                colors.3,
            ),
            (
                "Prompt per call",
                compact_count(tokens.prompt_side() / calls),
                colors.0,
            ),
            (
                "Output per prompt",
                compact_count(tokens.output / prompts),
                label,
            ),
            (
                "Cache hit",
                tokens
                    .cache_hit_ratio()
                    .map_or_else(|| "—".to_string(), percent_label),
                colors.1,
            ),
        ],
        canvas_columns(canvas.width, 4),
    );

    if stats.models.len() > 1 || stats.models.first().is_some_and(|m| m.calls > 0) {
        canvas.heading("By model", "");
        let peak = stats
            .models
            .first()
            .map_or(1, |model| model.tokens.total().max(1)) as f64;
        for model in stats.models.iter().take(8) {
            canvas.bar_row(
                &short_model(&model.model),
                18,
                model.tokens.total() as f64 / peak,
                colors.3,
                &format!(
                    "{} · {} calls",
                    compact_count(model.tokens.total()),
                    grouped_count(u64::from(model.calls))
                ),
                Some(model.tokens.total() as f64 / total),
            );
        }
        canvas.gap(1);
    }

    draw_context_chart(canvas, stats);

    let buckets = stats.timeline(slice_count(canvas.width, stats));
    if buckets.iter().any(|bucket| bucket.tokens.total() > 0) {
        canvas.heading("Prompt-side tokens over time", "per slice");
        canvas.line_chart(&LineChart {
            series: vec![
                LineSeries {
                    name: "cache read",
                    values: buckets.iter().map(|b| b.tokens.cache_read as f64).collect(),
                    color: colors.1,
                },
                LineSeries {
                    name: "fresh input + cache write",
                    values: buckets
                        .iter()
                        .map(|b| (b.tokens.input + b.tokens.cache_write) as f64)
                        .collect(),
                    color: colors.0,
                },
            ],
            height: 9,
            include_zero: true,
            label_formatter: Some(compact_axis),
            time_span: time_span(stats),
            note: None,
        });
        canvas.gap(1);
        canvas.heading("Output tokens over time", "per slice");
        canvas.line_chart(&LineChart {
            series: vec![
                LineSeries {
                    name: "output",
                    values: buckets.iter().map(|b| b.tokens.output as f64).collect(),
                    color: colors.3,
                },
                LineSeries {
                    name: "reasoning",
                    values: buckets.iter().map(|b| b.tokens.reasoning as f64).collect(),
                    color: colors.4,
                },
            ],
            height: 8,
            include_zero: true,
            label_formatter: Some(compact_axis),
            time_span: time_span(stats),
            note: None,
        });
    }
}

fn draw_context_chart(canvas: &mut Canvas, stats: &SessionStats) {
    let buckets = stats.timeline(slice_count(canvas.width, stats));
    // A bucket with no call carries the previous prompt size forward, so the
    // line shows how full the window stayed rather than dropping to zero.
    let mut carried = f64::NAN;
    let context: Vec<f64> = buckets
        .iter()
        .map(|bucket| {
            if bucket.peak_context > 0 {
                carried = bucket.peak_context as f64;
            }
            carried
        })
        .collect();
    if context.iter().filter(|value| !value.is_nan()).count() < 2 {
        return;
    }
    canvas.heading("Context window over time", "prompt size per call");
    let peak = context.iter().copied().fold(0.0_f64, f64::max);
    let mut series = vec![LineSeries {
        name: "prompt size",
        values: context.clone(),
        color: canvas.palette.input,
    }];
    let mut note = None;
    // The window is drawn only when it is near the data; a 1M window over a
    // 20k prompt would flatten the real line into the floor.
    if let Some(window) = stats.context_window.map(|w| w as f64) {
        if window <= peak * 3.0 {
            series.push(LineSeries {
                name: "model context window",
                values: vec![window; context.len()],
                color: canvas.palette.bad,
            });
        } else {
            note = Some("context window is far above the prompt size, so it is not drawn");
        }
    }
    canvas.line_chart(&LineChart {
        series,
        height: 10,
        include_zero: true,
        label_formatter: Some(compact_axis),
        time_span: time_span(stats),
        note,
    });
    canvas.gap(1);
}

fn draw_activity(canvas: &mut Canvas, stats: &SessionStats, view: &StatsView) {
    let (input, cache_read, output, dim, good, warn) = (
        canvas.palette.input,
        canvas.palette.cache_read,
        canvas.palette.output,
        canvas.palette.dim,
        canvas.palette.good,
        canvas.palette.warn,
    );
    let buckets = stats.timeline(slice_count(canvas.width, stats));
    canvas.heading(
        "Activity",
        "events per slice · prompts, model calls, tool calls",
    );
    if buckets.iter().any(|bucket| bucket.events > 0) {
        // Whole-number axis: never more rows than distinct integer levels, so
        // two rows cannot both be labelled "1".
        let values: Vec<f64> = buckets.iter().map(|b| f64::from(b.events)).collect();
        let peak = values.iter().copied().fold(0.0_f64, f64::max);
        canvas.line_chart(&LineChart {
            series: vec![LineSeries {
                name: "events",
                values,
                color: input,
            }],
            height: if peak < 8.0 {
                peak.ceil().max(2.0) as u16
            } else {
                8
            },
            include_zero: true,
            label_formatter: Some(count_axis),
            time_span: time_span(stats),
            note: None,
        });
    } else {
        canvas.text("no timestamped events yet", Style::new().fg(dim));
    }
    canvas.gap(1);

    canvas.heading("Turns", "");
    let durations = &stats.turn_durations_ms;
    if durations.is_empty() {
        canvas.text("no completed turns recorded yet", Style::new().fg(dim));
    } else {
        let average = stats.active_ms / u64::from(stats.turns_completed).max(1);
        let mut sorted = durations.clone();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];
        let label = canvas.palette.label;
        let mut tiles = vec![
            (
                "Completed",
                grouped_count(u64::from(stats.turns_completed)),
                label,
            ),
            ("Average", duration_label(average), output),
            ("Median", duration_label(median), output),
            ("Longest", duration_label(stats.longest_turn_ms), warn),
        ];
        if let Some(first_token) = stats.average_first_token_ms {
            tiles.push(("Avg first token", format!("{first_token} ms"), good));
        }
        if stats.turns_aborted > 0 {
            tiles.push((
                "Aborted",
                grouped_count(u64::from(stats.turns_aborted)),
                warn,
            ));
        }
        canvas.tiles(&tiles, canvas_columns(canvas.width, 4));
        canvas.line_chart(&LineChart {
            series: vec![LineSeries {
                name: "turn duration",
                values: durations.iter().map(|d| *d as f64).collect(),
                color: output,
            }],
            height: 6,
            include_zero: true,
            label_formatter: Some(duration_axis),
            time_span: None,
            note: Some("last turns, oldest to newest"),
        });
    }
    canvas.gap(1);

    canvas.heading(
        "Tools",
        &format!("{} calls", grouped_count(u64::from(stats.tool_calls))),
    );
    if stats.tools.is_empty() {
        canvas.text("no tool calls recorded", Style::new().fg(dim));
    } else {
        let peak = stats.tools.first().map_or(1, |tool| tool.1).max(1) as f64;
        let total = f64::from(stats.tool_calls.max(1));
        for (name, count) in stats.tools.iter().take(10) {
            canvas.bar_row(
                name,
                16,
                f64::from(*count) / peak,
                cache_read,
                &grouped_count(u64::from(*count)),
                Some(f64::from(*count) / total),
            );
        }
        if stats.tools.len() > 10 {
            canvas.text(
                &format!("… and {} more tools", stats.tools.len() - 10),
                Style::new().fg(dim),
            );
        }
    }
    if let Some(errors) = stats.tool_errors {
        canvas.text(
            &format!("{errors} tool results reported an error"),
            Style::new().fg(if errors > 0 { warn } else { dim }),
        );
    }
    canvas.gap(1);

    canvas.heading("Session events", "");
    let mut notes = vec![format!("{} compactions", stats.compactions)];
    if stats.api_errors > 0 {
        notes.push(format!("{} API errors", stats.api_errors));
    }
    if let (Some(first), Some(last)) = (stats.first_at_ms, stats.last_at_ms) {
        notes.push(format!("first event {}", clock_label(first, true)));
        notes.push(format!("last event {}", ago_label(view.now_ms, last)));
    }
    canvas.text(&notes.join("  ·  "), Style::new().fg(dim));
    if let Some(cwd) = &stats.cwd {
        canvas.text(&format!("cwd {cwd}"), Style::new().fg(dim));
    }
}

fn draw_prompts(canvas: &mut Canvas, stats: &SessionStats, view: &StatsView) {
    let (dim, label, accent) = (canvas.palette.dim, canvas.palette.label, theme::accent_bg());
    canvas.heading(
        "Recent prompts",
        &format!(
            "newest first · {} of {} sent",
            stats.recent_prompts.len(),
            grouped_count(u64::from(stats.prompt_count))
        ),
    );
    if stats.recent_prompts.is_empty() {
        canvas.gap(1);
        canvas.text("No user prompts recorded yet.", Style::new().fg(dim));
        return;
    }
    canvas.gap(1);
    let count = stats.recent_prompts.len();
    for (index, prompt) in stats.recent_prompts.iter().rev().enumerate() {
        let number = count - index;
        let when = prompt.at_ms.map_or_else(String::new, |at| {
            format!(
                "{}  ·  {}",
                clock_label(at, false),
                ago_label(view.now_ms, at)
            )
        });
        let mut header = vec![
            Span::styled(
                format!("#{number} "),
                Style::new().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(when, Style::new().fg(label)),
            Span::styled(
                format!("  ·  {} chars", grouped_count(prompt.characters as u64)),
                Style::new().fg(dim),
            ),
        ];
        if prompt.repeats > 1 {
            header.push(Span::styled(
                format!("  ×{} sent in a row", prompt.repeats),
                Style::new().fg(canvas.palette.warn),
            ));
        }
        canvas.line(Line::from(header));
        canvas.wrapped(&prompt.text, 2, Style::new(), 4);
        canvas.gap(1);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::session_stats::{
        ModelUsage, PromptRecord, RateLimitWindow, ReportedCost, TokenSample,
    };

    fn sample_stats() -> SessionStats {
        let start = 1_790_000_000_000_i64;
        let samples: Vec<TokenSample> = (0..40)
            .map(|index| TokenSample {
                at_ms: start + index * 60_000,
                tokens: TokenTotals {
                    input: 10,
                    cache_read: 4000 + index as u64 * 100,
                    cache_write: 300,
                    output: 200 + (index as u64 % 7) * 90,
                    reasoning: 50,
                },
                context: Some(20_000 + index as u64 * 3_000),
            })
            .collect();
        SessionStats {
            provider: Some("Anthropic".into()),
            model: Some("claude-sonnet-5".into()),
            effort: Some("high".into()),
            cli_version: Some("2.1.283".into()),
            git_branch: Some("master".into()),
            first_at_ms: Some(start),
            last_at_ms: Some(start + 39 * 60_000),
            prompt_count: 3,
            model_calls: 40,
            turns_completed: 5,
            tool_calls: 20,
            tool_errors: Some(2),
            tokens: TokenTotals {
                input: 400,
                cache_read: 260_000,
                cache_write: 12_000,
                output: 12_000,
                reasoning: 2_000,
            },
            models: vec![ModelUsage {
                model: "claude-sonnet-5".into(),
                calls: 40,
                tokens: TokenTotals {
                    input: 400,
                    cache_read: 260_000,
                    cache_write: 12_000,
                    output: 12_000,
                    reasoning: 2_000,
                },
            }],
            context_tokens: Some(137_000),
            context_window: Some(200_000),
            peak_context_tokens: 137_000,
            tools: vec![("Bash".into(), 12), ("Read".into(), 8)],
            recent_prompts: vec![
                PromptRecord {
                    at_ms: Some(start),
                    text: "fix the login bug and add a regression test".into(),
                    characters: 43,
                    repeats: 1,
                },
                PromptRecord {
                    at_ms: Some(start + 600_000),
                    text: "keep going".into(),
                    characters: 10,
                    repeats: 3,
                },
            ],
            active_ms: 600_000,
            longest_turn_ms: 240_000,
            turn_durations_ms: vec![30_000, 60_000, 240_000, 12_000, 258_000],
            cost: Some(ReportedCost {
                total_usd: 12.5,
                lines_added: 120,
                lines_removed: 8,
                api_duration_ms: 400_000,
                tool_duration_ms: 90_000,
                has_unknown_model_cost: false,
                model_costs: vec![
                    ("claude-sonnet-5".into(), 12.0),
                    ("claude-haiku-4-5".into(), 0.5),
                ],
            }),
            rate_limits: vec![(
                "primary".into(),
                RateLimitWindow {
                    used_percent: 74.0,
                    window_minutes: Some(10_080),
                    resets_at_unix: Some(1_790_200_000),
                },
            )],
            samples,
            activity_ms: (0..40).map(|i| start + i * 60_000).collect(),
            bytes_read: 1_147_269,
            ..SessionStats::default()
        }
    }

    fn render_tab(tab: StatsTab, scroll: u16, size: (u16, u16)) -> (Vec<String>, StatsPopover) {
        let stats = sample_stats();
        let load = LoadState::Ready;
        let view = StatsView {
            stats: Some(&stats),
            load: &load,
            supported: true,
            has_session: true,
            now_ms: 1_790_000_000_000 + 45 * 60_000,
            animation_ms: 0,
            scheme: ColorScheme::Dark,
        };
        let mut popover = StatsPopover::new(NodeId(1), true, Instant::now());
        popover.tab = tab;
        popover.scroll = scroll;
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    Position::new(5, 0),
                    &mut popover,
                    &view,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows = (0..size.1)
            .map(|y| {
                (0..size.0)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        (rows, popover)
    }

    #[test]
    fn geometry_is_stable_and_clickable_regions_do_not_overlap() {
        let screen = Rect::new(0, 0, 140, 40);
        let layout = geometry(screen, Position::new(40, 0)).expect("fits");
        assert!(layout.area.right() <= screen.right());
        assert!(layout.body.bottom() < layout.footer.bottom());
        for (index, (_, rect)) in layout.tabs.iter().enumerate() {
            assert!(layout.area.contains(Position::new(rect.x, rect.y)));
            for (_, other) in layout.tabs.iter().skip(index + 1) {
                assert!(rect.right() <= other.x, "tabs must not overlap");
            }
        }
        let tab_centre = |tab: StatsTab| {
            let rect = layout.tabs.iter().find(|(t, _)| *t == tab).unwrap().1;
            Position::new(rect.x + 1, rect.y)
        };
        assert_eq!(
            layout.hit(tab_centre(StatsTab::Tokens)),
            Some(StatsHit::Tab(StatsTab::Tokens))
        );
        assert_eq!(
            layout.hit(Position::new(layout.close.x + 1, layout.close.y)),
            Some(StatsHit::Close)
        );
        assert_eq!(
            layout.hit(Position::new(layout.body.x + 2, layout.body.y + 2)),
            Some(StatsHit::Body)
        );
        assert_eq!(layout.hit(Position::new(0, 39)), None);
    }

    #[test]
    fn geometry_refuses_terminals_that_are_too_small() {
        assert!(geometry(Rect::new(0, 0, 50, 40), Position::new(2, 0)).is_none());
        assert!(geometry(Rect::new(0, 0, 120, 10), Position::new(2, 0)).is_none());
    }

    #[test]
    fn nothing_is_drawn_outside_the_popover_on_the_narrowest_terminal() {
        let stats = sample_stats();
        let load = LoadState::Ready;
        let view = StatsView {
            stats: Some(&stats),
            load: &load,
            supported: true,
            has_session: true,
            now_ms: 1_790_000_000_000,
            animation_ms: 0,
            scheme: ColorScheme::Dark,
        };
        for width in [POPOVER_MIN_WIDTH + 2, 70, 97] {
            let screen = Rect::new(0, 0, width, 30);
            let anchor = Position::new(width - 3, 0);
            let layout = geometry(screen, anchor).expect("fits at its minimum size");
            let mut popover = StatsPopover::new(NodeId(1), true, Instant::now());
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal
                .draw(|frame| render(frame, screen, anchor, &mut popover, &view))
                .unwrap();
            let buffer = terminal.backend().buffer();
            for y in 0..30 {
                for x in 0..width {
                    if !layout.area.contains(Position::new(x, y)) {
                        assert_eq!(
                            buffer[(x, y)].symbol(),
                            " ",
                            "({x},{y}) outside the popover"
                        );
                    }
                }
            }
            for (_, rect) in &layout.tabs {
                assert!(
                    rect.right() <= layout.area.right() - 2,
                    "tab inside the border"
                );
            }
            assert!(layout.close.right() < layout.area.right());
        }
    }

    #[test]
    fn the_popover_fills_the_right_panel_and_never_leaves_it() {
        // A tree occupies the left 40 columns; the panel is the rest.
        let panel = Rect::new(40, 0, 100, 40);
        let layout = geometry(panel, Position::new(45, 0)).expect("fits");
        assert_eq!(layout.area.x, panel.x + 1);
        assert_eq!(layout.area.right(), panel.right() - 1);
        assert_eq!(layout.area.bottom(), panel.bottom() - 1);
        assert_eq!(layout.area.y, 1, "hangs directly below the icon row");
        // A wider panel yields a wider popover: nothing caps it any more.
        let wide = geometry(Rect::new(30, 0, 250, 60), Position::new(35, 0)).unwrap();
        assert_eq!(wide.area.width, 248);
        assert!(wide.area.height > 36);
    }

    #[test]
    fn geometry_stays_on_screen_when_the_anchor_is_far_right() {
        let screen = Rect::new(0, 0, 100, 40);
        let layout = geometry(screen, Position::new(98, 0)).unwrap();
        assert!(layout.area.right() <= screen.right());
    }

    #[test]
    fn overview_renders_the_headline_facts() {
        let (rows, popover) = render_tab(StatsTab::Overview, 0, (120, 44));
        let text = rows.join("\n");
        assert!(text.contains("Costs & stats"), "{text}");
        assert!(text.contains("claude-sonnet-5"));
        assert!(text.contains("RUNNING FOR"));
        assert!(
            text.contains("45m 0s"),
            "running time uses the injected clock"
        );
        assert!(text.contains("$12.50"));
        assert!(
            text.contains("close"),
            "a pinned popover shows its close control"
        );
        assert!(popover.content_rows > 0);
    }

    #[test]
    fn hover_popover_has_no_close_control_and_says_how_to_pin() {
        let stats = sample_stats();
        let load = LoadState::Ready;
        let view = StatsView {
            stats: Some(&stats),
            load: &load,
            supported: true,
            has_session: true,
            now_ms: 1_790_000_000_000,
            animation_ms: 0,
            scheme: ColorScheme::Dark,
        };
        let mut popover = StatsPopover::new(NodeId(1), false, Instant::now());
        let mut terminal = Terminal::new(TestBackend::new(120, 44)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    Position::new(5, 0),
                    &mut popover,
                    &view,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..44)
            .map(|y| {
                (0..120)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("✕ close"));
        assert!(text.contains("click ● to pin"));
    }

    #[test]
    fn every_tab_renders_without_panicking_on_small_and_large_terminals() {
        for tab in StatsTab::ALL {
            for size in [(60, 20), (100, 30), (200, 60)] {
                let (rows, _) = render_tab(tab, 0, size);
                assert!(rows.iter().any(|row| row.contains("Costs & stats")));
            }
        }
    }

    #[test]
    fn tokens_tab_shows_breakdown_models_and_a_context_chart() {
        let (rows, popover) = render_tab(StatsTab::Tokens, 0, (120, 60));
        let text = rows.join("\n");
        assert!(text.contains("BREAKDOWN"));
        assert!(text.contains("cache read"));
        assert!(text.contains("BY MODEL"));
        assert!(text.contains("CONTEXT WINDOW OVER TIME"));
        assert!(
            text.contains('┤') && text.contains('╭') && text.contains('─'),
            "time series are drawn in asciichart style"
        );
        assert!(
            text.contains("model context window"),
            "the window line is in the legend"
        );
        assert!(popover.content_rows > 20);
    }

    #[test]
    fn activity_and_prompts_tabs_list_their_content() {
        let (rows, _) = render_tab(StatsTab::Activity, 0, (120, 70));
        let text = rows.join("\n");
        assert!(text.contains("Bash"));
        assert!(text.contains("TURNS"));
        let (rows, _) = render_tab(StatsTab::Prompts, 0, (120, 44));
        let text = rows.join("\n");
        assert!(text.contains("fix the login bug"));
        assert!(text.contains("×3 sent in a row"));
        let newest = text.find("#2").expect("newest prompt is numbered #2");
        let oldest = text.find("#1").expect("oldest prompt is numbered #1");
        assert!(newest < oldest, "newest first");
    }

    #[test]
    fn scrolling_clamps_to_the_content() {
        let (_, mut popover) = render_tab(StatsTab::Tokens, 0, (100, 26));
        assert!(popover.max_scroll() > 0, "a short window needs scrolling");
        popover.scroll_by(10_000);
        assert_eq!(popover.scroll, popover.max_scroll());
        popover.scroll_by(-10_000);
        assert_eq!(popover.scroll, 0);
        let (scrolled, _) = render_tab(StatsTab::Tokens, 5, (100, 26));
        let (top, _) = render_tab(StatsTab::Tokens, 0, (100, 26));
        assert_ne!(scrolled, top);
    }

    #[test]
    fn placeholders_explain_missing_data() {
        let cases: [(LoadState, bool, &str); 3] = [
            (LoadState::Idle, false, "Waiting for the agent's session"),
            (
                LoadState::Loading {
                    done: 50,
                    total: 100,
                },
                true,
                "Reading the session transcript",
            ),
            (
                LoadState::Unavailable("gone".into()),
                true,
                "No statistics available",
            ),
        ];
        for (load, has_session, expected) in cases {
            let view = StatsView {
                stats: None,
                load: &load,
                supported: true,
                has_session,
                now_ms: 0,
                animation_ms: 0,
                scheme: ColorScheme::Dark,
            };
            let mut popover = StatsPopover::new(NodeId(1), true, Instant::now());
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        frame.area(),
                        Position::new(5, 0),
                        &mut popover,
                        &view,
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = (0..30)
                .map(|y| {
                    (0..100)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(expected), "{expected}: {text}");
        }
    }

    /// Manual look at every tab: `cargo test -p ilium-client --lib dump_tabs
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints rendered tabs for a human to read"]
    fn dump_tabs() {
        for tab in StatsTab::ALL {
            let (rows, _) = render_tab(tab, 0, (110, 42));
            println!("===== {tab:?} =====");
            for row in rows {
                println!("{}", row.trim_end());
            }
        }
    }

    #[test]
    fn unsupported_agents_are_told_why_there_are_no_statistics() {
        let load = LoadState::Idle;
        let view = StatsView {
            stats: None,
            load: &load,
            supported: false,
            has_session: true,
            now_ms: 0,
            animation_ms: 0,
            scheme: ColorScheme::Dark,
        };
        let mut popover = StatsPopover::new(NodeId(1), true, Instant::now());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    Position::new(5, 0),
                    &mut popover,
                    &view,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..30)
            .map(|y| {
                (0..100)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("No statistics for this agent"), "{text}");
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(12_345), "12.3k");
        assert_eq!(compact_count(4_560_000), "4.56M");
        assert_eq!(grouped_count(1_234_567), "1,234,567");
        assert_eq!(grouped_count(12), "12");
        assert_eq!(duration_label(42_000), "42s");
        assert_eq!(duration_label(312_000), "5m 12s");
        assert_eq!(duration_label(8_040_000), "2h 14m");
        assert_eq!(duration_label(3 * 86_400_000 + 4 * 3_600_000), "3d 4h");
        assert_eq!(percent_label(0.0004), "<0.1%");
        assert_eq!(percent_label(0.734), "73%");
        assert_eq!(StatsTab::Prompts.stepped(1), StatsTab::Overview);
        assert_eq!(StatsTab::Overview.stepped(-1), StatsTab::Prompts);
    }

    #[test]
    fn segmented_bar_always_fills_the_requested_width() {
        for width in [1, 7, 40, 91] {
            let line = segmented_bar(
                &[
                    (1.0, Color::Red),
                    (2.0, Color::Green),
                    (0.0, Color::Blue),
                    (3.0, Color::Cyan),
                ],
                width,
            );
            assert_eq!(line.width(), width);
        }
    }
}
