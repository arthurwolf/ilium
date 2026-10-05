//! Settings > Optimization: the compaction optimizer tab.
//!
//! Two sub-tabs (Codex | Claude Code) sit in a fixed two-row header; below it
//! one scrollable page shows, depending on the selected agent's
//! [`ScanView`]:
//!
//! * **idle**: the "Scan sessions" button (a scan never starts by itself);
//! * **listing / scanning**: phase label, a byte-weighted progress bar, files
//!   done of total, elapsed time, the current file name and a Cancel button;
//! * **report**: the recommendation card with the "Optimal: ... - Apply to
//!   <agent>" button at the top, then the three-way comparison, statistics,
//!   cost mix, fixed prefix, simulation table and chart, per-model optima,
//!   rework sensitivity, regimes and warnings ([`report`]). A failed or
//!   cancelled scan keeps showing the previous report with a note.
//!
//! The Apply confirmation is a modal drawn over the whole Settings screen
//! ([`modal`]).
//!
//! Geometry is produced once, by [`build_body`] and [`header`], and shared by
//! rendering, mouse hit testing, scroll bounds and help anchors, so they
//! cannot drift apart (the pattern of the other settings tabs). State lives in
//! [`crate::compaction_app`]; this module is presentation only.

mod modal;
mod report;
mod simulation;
mod text;

#[cfg(test)]
pub(crate) mod tests;

use ilium_compaction_analysis::AgentKind;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::agent_config_writer::ApplyRecord;
use crate::app::App;
use crate::compaction_app::{
    agent_label, config_target, AgentPanel, CurrentSetting, Note, NoteTone, OPTIMIZATION_AGENTS,
};
use crate::compaction_report::{
    format_bytes, format_elapsed, format_percent, group_thousands, CompactionReport,
};
use crate::compaction_scan::{ScanPhase, ScanProgress, ScanView};
use crate::theme;

pub(crate) use modal::{
    modal_action_at, modal_max_scroll, modal_page_height, render as render_modal,
};
use text::{bar_parts, pad_right, truncate_to, wrap_text};

/// The confirmation text as plain strings (tests).
#[cfg(test)]
pub(crate) fn modal_lines_for_tests(
    pending: &crate::compaction_app::PendingApply,
    width: u16,
) -> Vec<String> {
    modal::modal_lines(pending, width)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
pub(crate) use modal::modal_layout as modal_layout_for_tests;

/// Left margin shared with the other settings tabs.
pub(crate) const INSET: usize = 2;
/// Columns kept free on the right for the scrollbar.
const RIGHT_MARGIN: usize = 2;
/// Rows of the fixed header: the sub-tabs and the key hint.
pub const HEADER_ROWS: u16 = 2;
/// The longest the scan progress bar grows.
const MAX_BAR_WIDTH: usize = 60;

/// What a click or key on the tab asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Switch the sub-tab.
    SelectAgent(AgentKind),
    /// "Scan sessions" / "Re-scan".
    Scan,
    /// Stop the running scan.
    Cancel,
    /// Open the Apply confirmation.
    Apply,
    /// Open the Apply confirmation for the simulated, extrapolated optimum.
    ApplyExtrapolated,
    /// Restore the value before the last apply.
    Revert,
}

/// The rectangle of one button inside the page, in page coordinates: `x`
/// relative to the content area, lines relative to the first body line (or
/// to the first header row for header buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonSpan {
    pub action: Action,
    pub first_line: u16,
    pub last_line: u16,
    pub x_start: u16,
    pub x_end: u16,
}

/// A help topic anchored to a page line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpMark {
    pub topic: &'static str,
    pub line: u16,
}

/// The scrollable page.
#[derive(Debug, Clone)]
pub struct Body {
    pub lines: Vec<Line<'static>>,
    pub buttons: Vec<ButtonSpan>,
    pub marks: Vec<HelpMark>,
}

/// The fixed header.
#[derive(Debug, Clone)]
pub struct Header {
    pub lines: Vec<Line<'static>>,
    pub buttons: Vec<ButtonSpan>,
}

/// Everything the page needs, borrowed from the app.
#[derive(Debug, Clone)]
pub struct Screen<'a> {
    pub agent: AgentKind,
    pub view: ScanView<'a>,
    /// The report of the last successful scan (also while a later scan failed
    /// or was cancelled).
    pub report: Option<&'a CompactionReport>,
    pub panel: &'a AgentPanel,
}

impl<'a> Screen<'a> {
    pub fn of(app: &'a App) -> Self {
        let agent = app.optimization.selected_agent;
        Self {
            agent,
            view: app.compaction_optimizer.view(agent),
            report: app.compaction_optimizer.report(agent),
            panel: app.optimization.panel(agent),
        }
    }
}

// ----------------------------------------------------------------- styles

pub(crate) fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

pub(crate) fn accent() -> Style {
    Style::new()
        .fg(theme::accent_bg())
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn warning() -> Style {
    Style::new().fg(Color::Yellow)
}

fn success() -> Style {
    Style::new().fg(Color::Green)
}

fn failure() -> Style {
    Style::new().fg(Color::Red)
}

fn primary_button_style() -> Style {
    theme::selected_style().add_modifier(Modifier::BOLD)
}

fn secondary_button_style() -> Style {
    accent()
}

// ---------------------------------------------------------------- builder

/// Accumulates the lines, buttons and help marks of the page.
pub(crate) struct Builder {
    lines: Vec<Line<'static>>,
    buttons: Vec<ButtonSpan>,
    marks: Vec<HelpMark>,
    width: usize,
}

impl Builder {
    pub(crate) fn new(width: u16) -> Self {
        Self {
            lines: Vec::new(),
            buttons: Vec::new(),
            marks: Vec::new(),
            width: usize::from(width),
        }
    }

    /// Cells available to wrapped text.
    pub(crate) fn text_width(&self) -> usize {
        self.width.saturating_sub(INSET + RIGHT_MARGIN).max(24)
    }

    /// Cells available to a table (indent included).
    pub(crate) fn table_width(&self) -> usize {
        self.width.saturating_sub(RIGHT_MARGIN).max(INSET + 24)
    }

    fn here(&self) -> u16 {
        self.lines.len().min(usize::from(u16::MAX)) as u16
    }

    pub(crate) fn blank(&mut self) {
        self.lines.push(Line::from(""));
    }

    /// Anchors a help topic to the next line.
    pub(crate) fn mark(&mut self, topic: &'static str) {
        self.marks.push(HelpMark {
            topic,
            line: self.here(),
        });
    }

    pub(crate) fn heading(&mut self, title: &str, topic: &'static str) {
        self.marks.push(HelpMark {
            topic,
            line: self.here(),
        });
        self.lines.push(Line::from(Span::styled(
            format!("{}{title}", " ".repeat(INSET)),
            accent(),
        )));
    }

    /// Wrapped text at the standard indent.
    pub(crate) fn paragraph(&mut self, text: &str, style: Style) {
        self.paragraph_at(INSET, text, style);
    }

    pub(crate) fn paragraph_at(&mut self, indent: usize, text: &str, style: Style) {
        let width = self.width.saturating_sub(indent + RIGHT_MARGIN).max(24);
        for line in wrap_text(text, width) {
            self.lines.push(Line::from(Span::styled(
                format!("{}{line}", " ".repeat(indent)),
                style,
            )));
        }
    }

    /// One prepared line of spans (already sized by the caller).
    pub(crate) fn line(&mut self, spans: Vec<Span<'static>>) {
        self.lines.push(Line::from(spans));
    }

    pub(crate) fn push_lines(&mut self, lines: Vec<Line<'static>>) {
        self.lines.extend(lines);
    }

    /// `label: value`, the label dim, continuation lines indented.
    pub(crate) fn field(&mut self, label: &str, value: &str) {
        self.field_styled(label, value, Style::new());
    }

    pub(crate) fn field_styled(&mut self, label: &str, value: &str, value_style: Style) {
        let full = format!("{label}: {value}");
        let label_cells = label.chars().count() + 1;
        for (index, line) in wrap_text(&full, self.text_width()).into_iter().enumerate() {
            let mut spans = vec![Span::raw(" ".repeat(if index == 0 {
                INSET
            } else {
                INSET + 2
            }))];
            if index == 0 && line.chars().count() >= label_cells {
                let split = line
                    .char_indices()
                    .nth(label_cells)
                    .map_or(line.len(), |(position, _)| position);
                spans.push(Span::styled(line[..split].to_owned(), dim()));
                spans.push(Span::styled(line[split..].to_owned(), value_style));
            } else {
                spans.push(Span::styled(line, value_style));
            }
            self.lines.push(Line::from(spans));
        }
    }

    pub(crate) fn bullet(&mut self, text: &str, style: Style) {
        let width = self.text_width().saturating_sub(2);
        for (index, line) in wrap_text(text, width).into_iter().enumerate() {
            let lead = if index == 0 { "- " } else { "  " };
            self.lines.push(Line::from(Span::styled(
                format!("{}{lead}{line}", " ".repeat(INSET)),
                style,
            )));
        }
    }

    /// A button that wraps over several lines when its text is long; every
    /// line is clickable and padded to the same width so it reads as a block.
    pub(crate) fn button_block(&mut self, action: Action, text: &str, style: Style) {
        let inner = self.text_width().saturating_sub(2);
        let rows = wrap_text(text, inner);
        let block_width = rows
            .iter()
            .map(|row| unicode_width::UnicodeWidthStr::width(row.as_str()))
            .max()
            .unwrap_or(0);
        let first_line = self.here();
        for row in &rows {
            self.lines.push(Line::from(vec![
                Span::raw(" ".repeat(INSET)),
                Span::styled(format!(" {} ", pad_right(row, block_width)), style),
            ]));
        }
        self.buttons.push(ButtonSpan {
            action,
            first_line,
            last_line: self.here().saturating_sub(1).max(first_line),
            x_start: INSET as u16,
            x_end: (INSET + block_width + 2) as u16,
        });
    }

    /// Buttons side by side; the row wraps when they do not fit. Each label
    /// is shown inside `[ ]`.
    pub(crate) fn button_row(&mut self, items: &[(Action, String, Style)]) {
        let limit = self.width.saturating_sub(RIGHT_MARGIN);
        let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(INSET))];
        let mut x = INSET;
        let mut pending: Vec<ButtonSpan> = Vec::new();
        let mut line = self.here();
        for (action, label, style) in items {
            let shown = format!("[ {label} ]");
            let cells = unicode_width::UnicodeWidthStr::width(shown.as_str());
            if x > INSET && x + cells > limit {
                self.lines.push(Line::from(std::mem::take(&mut spans)));
                self.buttons.append(&mut pending);
                line += 1;
                spans = vec![Span::raw(" ".repeat(INSET))];
                x = INSET;
            }
            spans.push(Span::styled(shown, *style));
            pending.push(ButtonSpan {
                action: *action,
                first_line: line,
                last_line: line,
                x_start: x as u16,
                x_end: (x + cells) as u16,
            });
            spans.push(Span::raw("  "));
            x += cells + 2;
        }
        self.lines.push(Line::from(spans));
        self.buttons.append(&mut pending);
    }

    pub(crate) fn finish(self) -> Body {
        Body {
            lines: self.lines,
            buttons: self.buttons,
            marks: self.marks,
        }
    }
}

// ----------------------------------------------------------------- header

/// The sub-tab row and the key hint.
pub fn header(selected: AgentKind, width: u16) -> Header {
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(INSET))];
    let mut buttons = Vec::new();
    let mut x = INSET;
    for agent in OPTIMIZATION_AGENTS {
        let label = format!(" {} ", agent_label(agent));
        let cells = unicode_width::UnicodeWidthStr::width(label.as_str());
        let style = if agent == selected {
            primary_button_style()
        } else {
            accent()
        };
        spans.push(Span::styled(label, style));
        buttons.push(ButtonSpan {
            action: Action::SelectAgent(agent),
            first_line: 0,
            last_line: 0,
            x_start: x as u16,
            x_end: (x + cells) as u16,
        });
        spans.push(Span::raw("  "));
        x += cells + 2;
    }
    let hint = truncate_to(
        "←/→ agent · s scan · a apply · r revert · Esc cancels a scan · ↑/↓ PgUp/PgDn scroll",
        usize::from(width).saturating_sub(INSET + RIGHT_MARGIN),
    );
    Header {
        lines: vec![
            Line::from(spans),
            Line::from(Span::styled(format!("{}{hint}", " ".repeat(INSET)), dim())),
        ],
        buttons,
    }
}

// ------------------------------------------------------------------- body

/// Builds the scrollable page for `screen` at `width` columns.
pub fn build_body(screen: &Screen<'_>, width: u16) -> Body {
    let mut builder = Builder::new(width);
    builder.blank();
    match &screen.view {
        ScanView::Listing { files_found } => listing(&mut builder, screen, *files_found),
        ScanView::Scanning(progress) => scanning(&mut builder, screen, progress),
        ScanView::Idle | ScanView::Ready(_) | ScanView::Failed(_) | ScanView::Cancelled => {
            match screen.report {
                Some(report) => report::ready(&mut builder, screen, report),
                None => idle(&mut builder, screen),
            }
        }
    }
    builder.finish()
}

/// The text of the scan outcome that is not a report ("failed", "cancelled").
pub(crate) fn scan_note(screen: &Screen<'_>) -> Option<String> {
    let kept = if screen.report.is_some() {
        " The report below is from the previous scan."
    } else {
        ""
    };
    match &screen.view {
        ScanView::Failed(message) => Some(format!("The last scan failed: {message}.{kept}")),
        ScanView::Cancelled => Some(format!("The last scan was cancelled.{kept}")),
        _ => None,
    }
}

/// The note of the last apply or revert, styled by its tone.
pub(crate) fn push_apply_note(builder: &mut Builder, note: Option<&Note>) {
    let Some(note) = note else {
        return;
    };
    let style = match note.tone {
        NoteTone::Success => success(),
        NoteTone::Error => failure(),
    };
    builder.paragraph(&note.text, style);
}

/// Shows what the configuration currently says about the setting.
pub(crate) fn push_current_setting(builder: &mut Builder, screen: &Screen<'_>) {
    let target = config_target(screen.agent);
    let location = screen.panel.config_path.as_ref().map_or_else(
        || target.file_name().to_owned(),
        |path| path.display().to_string(),
    );
    let text = match &screen.panel.current {
        CurrentSetting::Unknown => "not read yet".to_owned(),
        CurrentSetting::Value(value) => format!("{} (in {location})", group_thousands(*value)),
        CurrentSetting::NotSet => format!("not set in {location}: the CLI default applies"),
        CurrentSetting::NoFile => format!("not set ({location} does not exist)"),
        CurrentSetting::Unreadable(reason) => format!("unreadable: {reason}"),
    };
    let style = match screen.panel.current {
        CurrentSetting::Unreadable(_) => warning(),
        _ => Style::new(),
    };
    builder.field_styled(&format!("Current {}", target.key()), &text, style);
}

fn idle(builder: &mut Builder, screen: &Screen<'_>) {
    let target = config_target(screen.agent);
    let (transcripts, setting_file) = match screen.agent {
        AgentKind::Codex => ("~/.codex/sessions", "~/.codex/config.toml"),
        AgentKind::ClaudeCode => ("~/.claude/projects", "~/.claude/settings.json"),
    };
    builder.heading(
        &format!(
            "COMPACTION OPTIMIZER - {}",
            agent_label(screen.agent).to_uppercase()
        ),
        "OPT-02",
    );
    builder.paragraph(
        &format!(
            "Reads your {} session transcripts in {transcripts}, replays how every session grew under different auto-compaction thresholds and recommends the cheapest value of {}. Nothing is scanned until you press the button, and nothing in {setting_file} changes until you confirm.",
            agent_label(screen.agent),
            target.key()
        ),
        dim(),
    );
    builder.blank();
    push_current_setting(builder, screen);
    builder.blank();
    if let Some(note) = scan_note(screen) {
        builder.paragraph(&note, warning());
        builder.blank();
    }
    push_apply_note(builder, screen.panel.note.as_ref());
    builder.button_block(Action::Scan, "Scan sessions", primary_button_style());
    revert_row(builder, screen);
    builder.blank();
    builder.paragraph(
        "Enter or s starts the scan. A first scan of a large history can take minutes; later scans reuse a cache of unchanged files.",
        dim(),
    );
}

/// Text of the Revert button; the short form when the long one would not fit
/// in `max_width` cells.
pub(crate) fn revert_label(record: &ApplyRecord, max_width: usize) -> String {
    let previous = record
        .previous_value
        .map_or_else(|| "remove the key".to_owned(), group_thousands);
    let long = format!(
        "Revert {} to {previous} (applied {})",
        record.target.key(),
        record.applied_at
    );
    if unicode_width::UnicodeWidthStr::width(long.as_str()) + 4 <= max_width {
        long
    } else {
        format!("Revert to {previous}")
    }
}

/// The Revert button, when an unreverted change exists.
pub(crate) fn revert_row(builder: &mut Builder, screen: &Screen<'_>) {
    if let Some(record) = &screen.panel.record {
        builder.blank();
        builder.mark("OPT-05");
        let label = revert_label(record, builder.text_width());
        builder.button_row(&[(Action::Revert, label, secondary_button_style())]);
    }
}

fn phase_label(phase: ScanPhase) -> &'static str {
    match phase {
        ScanPhase::Listing => "Listing files",
        ScanPhase::Reading => "Reading transcripts",
        ScanPhase::Analyzing => "Analyzing (statistics, replay, optimizer)",
    }
}

fn scan_heading(builder: &mut Builder, screen: &Screen<'_>) {
    builder.heading(
        &format!(
            "SCANNING {} SESSIONS",
            agent_label(screen.agent).to_uppercase()
        ),
        "OPT-03",
    );
}

fn listing(builder: &mut Builder, screen: &Screen<'_>, files_found: u64) {
    scan_heading(builder, screen);
    builder.field("Phase", phase_label(ScanPhase::Listing));
    builder.field("Transcript files found", &group_thousands(files_found));
    scan_footer(builder, screen);
}

fn scanning(builder: &mut Builder, screen: &Screen<'_>, progress: &ScanProgress) {
    scan_heading(builder, screen);
    builder.field("Phase", phase_label(progress.phase));
    let bar_width = builder
        .text_width()
        .saturating_sub(8)
        .clamp(8, MAX_BAR_WIDTH);
    let (filled, empty) = bar_parts(progress.fraction(), bar_width);
    builder.line(vec![
        Span::raw(" ".repeat(INSET)),
        Span::styled(filled, accent()),
        Span::styled(empty, dim()),
        Span::raw(format!(" {:>6}", format_percent(progress.fraction()))),
    ]);
    builder.field(
        "Files",
        &format!(
            "{} of {}",
            group_thousands(progress.files_done),
            group_thousands(progress.files_total)
        ),
    );
    builder.field(
        "Data",
        &format!(
            "{} of {}",
            format_bytes(progress.bytes_done),
            format_bytes(progress.bytes_total)
        ),
    );
    builder.field("Elapsed", &format_elapsed(progress.elapsed.as_secs_f64()));
    if !progress.current_file_name.is_empty() {
        builder.field("Current file", &progress.current_file_name);
    }
    scan_footer(builder, screen);
}

fn scan_footer(builder: &mut Builder, screen: &Screen<'_>) {
    builder.blank();
    builder.button_row(&[(
        Action::Cancel,
        "Cancel".to_owned(),
        secondary_button_style(),
    )]);
    builder.blank();
    let kept = if screen.report.is_some() {
        "The previous report is kept if this scan fails or is cancelled."
    } else {
        "Esc or c cancels the scan; leaving Settings does not stop it."
    };
    builder.paragraph(kept, dim());
}

// ----------------------------------------------------- geometry and input

/// The scrollable area below the header.
pub fn body_area(content: Rect) -> Rect {
    let header = HEADER_ROWS.min(content.height);
    Rect::new(
        content.x,
        content.y.saturating_add(header),
        content.width,
        content.height.saturating_sub(header),
    )
}

/// Lines a page-up or page-down moves.
pub fn page_height(content: Rect) -> u16 {
    body_area(content).height.saturating_sub(1).max(1)
}

/// The largest valid scroll of the page at `content`.
pub fn max_scroll(app: &App, content: Rect) -> u16 {
    max_scroll_in(&Screen::of(app), content)
}

/// [`max_scroll`] for an explicit screen.
pub fn max_scroll_in(screen: &Screen<'_>, content: Rect) -> u16 {
    let body = build_body(screen, content.width);
    (body.lines.len().min(usize::from(u16::MAX)) as u16).saturating_sub(body_area(content).height)
}

/// The button a click at `position` lands on.
pub fn hit(app: &App, content: Rect, scroll: u16, position: Position) -> Option<Action> {
    hit_in(&Screen::of(app), content, scroll, position)
}

/// [`hit`] for an explicit screen.
pub fn hit_in(
    screen: &Screen<'_>,
    content: Rect,
    scroll: u16,
    position: Position,
) -> Option<Action> {
    if !content.contains(position) {
        return None;
    }
    let x = position.x - content.x;
    let find = |buttons: &[ButtonSpan], line: u16| {
        buttons
            .iter()
            .find(|button| {
                (button.first_line..=button.last_line).contains(&line)
                    && (button.x_start..button.x_end).contains(&x)
            })
            .map(|button| button.action)
    };
    let y = position.y - content.y;
    if y < HEADER_ROWS {
        return find(&header(screen.agent, content.width).buttons, y);
    }
    let body_rect = body_area(content);
    if !body_rect.contains(position) {
        return None;
    }
    let line = position.y - body_rect.y + scroll;
    find(&build_body(screen, content.width).buttons, line)
}

/// What Enter does: scan when there is nothing to apply, otherwise open the
/// Apply confirmation. Nothing while a scan runs.
pub fn primary_action(app: &App) -> Option<Action> {
    let screen = Screen::of(app);
    match screen.view {
        ScanView::Listing { .. } | ScanView::Scanning(_) => None,
        _ if screen
            .report
            .is_some_and(CompactionReport::has_recommendation) =>
        {
            Some(Action::Apply)
        }
        _ => Some(Action::Scan),
    }
}

/// Draws the header and the scrolled page into the Settings content area.
pub fn render(frame: &mut Frame, content: Rect, app: &App, scroll: u16) {
    let screen = Screen::of(app);
    let header_rect = Rect::new(
        content.x,
        content.y,
        content.width,
        HEADER_ROWS.min(content.height),
    );
    frame.render_widget(
        Paragraph::new(header(screen.agent, content.width).lines),
        header_rect,
    );
    let body = build_body(&screen, content.width);
    crate::settings_ui::render_scrollable(frame, body_area(content), body.lines, scroll);
}

/// Help anchors as (topic, row below the top of the content area), only for
/// rows that are on screen at `scroll`.
pub fn help_anchors(app: &App, content: Rect, scroll: u16) -> Vec<(&'static str, u16)> {
    help_anchors_in(&Screen::of(app), content, scroll)
}

/// [`help_anchors`] for an explicit screen.
pub fn help_anchors_in(
    screen: &Screen<'_>,
    content: Rect,
    scroll: u16,
) -> Vec<(&'static str, u16)> {
    let mut anchors = vec![("OPT-01", 0)];
    let body_rect = body_area(content);
    let body = build_body(screen, content.width);
    for mark in body.marks {
        let Some(offset) = mark.line.checked_sub(scroll) else {
            continue;
        };
        if offset < body_rect.height {
            anchors.push((mark.topic, HEADER_ROWS + offset));
        }
    }
    anchors
}
