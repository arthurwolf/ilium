//! Status-aware footer for a server-owned long-task monitor.
//!
//! The task's own status and Ilium's ability to observe it are deliberately
//! distinct. A degraded/failed monitor therefore changes the leading label
//! and adds a warning without repainting the task itself as failed. Terminal
//! task evidence stays retained; the client may hide its footer after the
//! configured delay without clearing or acknowledging it.

use ilium_core::{PaneProgress, ProgressMonitorHealth, ProgressTaskStatus};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{LineGauge, Paragraph};
use ratatui::Frame;

use crate::last_prompt_banner::wrap_lines;
use crate::theme::{self, ColorScheme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTone {
    Normal,
    Error,
    Warning,
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailRow {
    text: String,
    tone: DetailTone,
    is_seam: bool,
}

/// Fixed status row plus configured detail budget, independent of content.
pub fn reserved_height(max_detail_lines: u16) -> u16 {
    1u16.saturating_add(max_detail_lines)
}

/// Renders task state, percent, task details, monitor-health warnings, and a
/// compact identity line. A `None` progress or zero-height area renders
/// nothing.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    progress: Option<&PaneProgress>,
    max_detail_lines: u16,
    scheme: ColorScheme,
) {
    let Some(progress) = progress else {
        return;
    };
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("").style(theme::last_prompt_style(scheme)),
        area,
    );

    let [gauge_area, detail_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .areas(area);

    render_gauge(frame, gauge_area, progress, "", scheme);

    let lines: Vec<Line> = detail_rows(
        progress,
        detail_area.width,
        max_detail_lines.min(detail_area.height),
    )
    .into_iter()
    .map(|row| {
        let mut style = Style::new();
        if let Some(color) = detail_tone_color(row.tone, scheme) {
            style = style.fg(color);
        }
        if row.tone == DetailTone::Identity || row.is_seam {
            style = style.add_modifier(Modifier::DIM);
        }
        Line::from(Span::styled(row.text, style))
    })
    .collect();
    frame.render_widget(
        Paragraph::new(lines).style(theme::last_prompt_style(scheme)),
        detail_area,
    );
}

/// Renders every visible monitor of one pane in the fixed footer slot. One
/// monitor keeps the full single-task footer. Several monitors share the
/// slot: one gauge row each in registration order (so a row always belongs
/// to the same monitor), a final `+N more` row when they do not all fit, and
/// any spare rows carry the monitors' one-line messages.
pub fn render_monitors(
    frame: &mut Frame,
    area: Rect,
    monitors: &[&PaneProgress],
    max_detail_lines: u16,
    scheme: ColorScheme,
) {
    match monitors {
        [] => {}
        [only] => render(frame, area, Some(only), max_detail_lines, scheme),
        _ => render_several(frame, area, monitors, scheme),
    }
}

/// How several monitors share `height` footer rows: how many get a gauge row
/// and whether the last row says how many more there are.
fn several_monitor_rows(count: usize, height: u16) -> (usize, bool) {
    let height = usize::from(height);
    if count <= height {
        (count, false)
    } else if height <= 1 {
        (height, true)
    } else {
        (height - 1, true)
    }
}

/// The index (into the same `monitors` slice `render_monitors` drew) of the
/// monitor whose gauge or message sits on footer row `row_offset`.
pub fn monitor_at_row(count: usize, height: u16, row_offset: u16) -> Option<usize> {
    if count <= 1 {
        return (count == 1).then_some(0);
    }
    let (gauges, has_more_row) = several_monitor_rows(count, height);
    let row = usize::from(row_offset);
    if row < gauges {
        return Some(row);
    }
    if has_more_row {
        return None;
    }
    // Spare rows below the gauges list messages in the same order.
    let message_index = row - gauges;
    (message_index < count).then_some(message_index)
}

fn render_several(frame: &mut Frame, area: Rect, monitors: &[&PaneProgress], scheme: ColorScheme) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("").style(theme::last_prompt_style(scheme)),
        area,
    );
    let (gauges, has_more_row) = several_monitor_rows(monitors.len(), area.height);
    let hidden = monitors.len() - gauges;
    for (row, progress) in monitors.iter().take(gauges).enumerate() {
        let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
        let suffix = if has_more_row && area.height == 1 {
            format!(" · +{hidden} more")
        } else {
            String::new()
        };
        render_gauge(
            frame,
            row_area,
            progress,
            &format!(" · {}{suffix}", progress.report.job_id),
            scheme,
        );
    }
    if has_more_row && area.height > 1 {
        let row_area = Rect::new(area.x, area.y + gauges as u16, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("+{hidden} more monitors · `ilium progress status` lists them"),
                Style::new().add_modifier(Modifier::DIM),
            )))
            .style(theme::last_prompt_style(scheme)),
            row_area,
        );
        return;
    }
    let spare_rows = usize::from(area.height) - gauges;
    let lines: Vec<Line> = monitors
        .iter()
        .take(spare_rows)
        .map(|progress| {
            let (_, _, tone) = status_presentation(progress);
            let mut style = Style::new();
            if let Some(color) = detail_tone_color(tone, scheme) {
                style = style.fg(color);
            }
            let text = progress
                .report
                .error
                .as_deref()
                .filter(|error| !error.is_empty())
                .unwrap_or(&progress.report.message);
            Line::from(Span::styled(
                format!("{}: {}", progress.report.job_id, text),
                style,
            ))
        })
        .collect();
    let message_area = Rect::new(
        area.x,
        area.y + gauges as u16,
        area.width,
        area.height - gauges as u16,
    );
    frame.render_widget(
        Paragraph::new(lines).style(theme::last_prompt_style(scheme)),
        message_area,
    );
}

/// One status gauge row: icon, status, percent, unread marker, then
/// `label_suffix`.
fn render_gauge(
    frame: &mut Frame,
    area: Rect,
    progress: &PaneProgress,
    label_suffix: &str,
    scheme: ColorScheme,
) {
    // Domain validation rejects non-finite/out-of-range reports, but the
    // renderer remains defensive because one bad restored value must never
    // crash the entire TUI.
    let percent = progress.report.percent.clamp(0.0, 100.0);
    let ratio = (f64::from(percent) / 100.0).clamp(0.0, 1.0);
    let (icon, status_label, status_tone) = status_presentation(progress);
    let status_color = tone_color(status_tone, scheme);
    // Mirrors the sidebar's bold/dim result glyph: an outcome nobody has
    // looked at yet says so until the pane is focused or typed into.
    let unread = if progress.has_unread_outcome() {
        "  · unread"
    } else {
        ""
    };
    let label = format!("{icon} {status_label}  {percent:.0}%{unread}{label_suffix}");
    let gauge = LineGauge::default()
        .ratio(ratio)
        .label(Line::from(Span::styled(
            label,
            Style::new().fg(status_color).add_modifier(Modifier::BOLD),
        )))
        .filled_symbol(symbols::shade::FULL)
        .unfilled_symbol(symbols::shade::LIGHT)
        .filled_style(Style::new().fg(status_color).add_modifier(Modifier::BOLD))
        .unfilled_style(Style::new().fg(theme::muted_accent_bg(scheme)));
    frame.render_widget(gauge, area);
}

/// Hover content for a report's long description: the compact message as the
/// header, the multi-line details as the body, and the machine identity as
/// the dim reason line.
pub fn details_tooltip(progress: &PaneProgress) -> crate::status_icons::TooltipContent {
    let (_, status_label, _) = status_presentation(progress);
    let title = if progress.report.message.is_empty() {
        format!("{status_label}  {:.0}%", progress.report.percent)
    } else {
        progress.report.message.clone()
    };
    crate::status_icons::TooltipContent {
        title,
        body: progress.report.details.clone(),
        reason: Some(format!(
            "{status_label} {:.0}% · job {} · monitor #{}",
            progress.report.percent, progress.report.job_id, progress.monitor_id
        )),
    }
}

fn status_presentation(progress: &PaneProgress) -> (&'static str, &'static str, DetailTone) {
    match &progress.monitor_health {
        ProgressMonitorHealth::Failed { .. } => ("⚠", "MONITOR FAILED", DetailTone::Warning),
        ProgressMonitorHealth::Degraded { .. } => ("⚠", "MONITOR DEGRADED", DetailTone::Warning),
        ProgressMonitorHealth::Healthy => match progress.report.status {
            ProgressTaskStatus::NotStartedYet => ("○", "NOT STARTED", DetailTone::Warning),
            ProgressTaskStatus::Running => ("▶", "RUNNING", DetailTone::Normal),
            ProgressTaskStatus::Error => ("✕", "ERROR", DetailTone::Error),
            ProgressTaskStatus::Done => ("✓", "DONE", DetailTone::Identity),
        },
    }
}

fn detail_rows(progress: &PaneProgress, width: u16, maximum_rows: u16) -> Vec<DetailRow> {
    if maximum_rows == 0 {
        return Vec::new();
    }

    let mut critical = Vec::new();
    if let Some(error) = &progress.report.error {
        push_wrapped(
            &mut critical,
            format!("Task error: {error}"),
            DetailTone::Error,
            width,
        );
    }
    if !progress.report.message.is_empty() {
        // The compact description is one logical line; the long one lives in
        // the hover tooltip and is advertised by a trailing marker.
        let marker = if progress.report.details.is_empty() {
            ""
        } else {
            " ⓘ hover for details"
        };
        push_wrapped(
            &mut critical,
            format!("{}{marker}", progress.report.message),
            DetailTone::Normal,
            width,
        );
    }
    match &progress.monitor_health {
        ProgressMonitorHealth::Healthy => {}
        ProgressMonitorHealth::Degraded {
            consecutive_failures,
            last_error,
        } => push_wrapped(
            &mut critical,
            format!("Observation degraded ({consecutive_failures}): {last_error}"),
            DetailTone::Warning,
            width,
        ),
        ProgressMonitorHealth::Failed {
            consecutive_failures,
            last_error,
        } => push_wrapped(
            &mut critical,
            format!("Observation stopped ({consecutive_failures}): {last_error}"),
            DetailTone::Warning,
            width,
        ),
    }

    let maximum_rows = usize::from(maximum_rows);
    let mut rows = truncate_detail_rows(critical, maximum_rows);
    if rows.len() < maximum_rows {
        let identity = format!(
            "Job {} · monitor #{}",
            progress.report.job_id, progress.monitor_id
        );
        let mut identity_rows = Vec::new();
        push_wrapped(&mut identity_rows, identity, DetailTone::Identity, width);
        let available = maximum_rows - rows.len();
        if identity_rows.len() <= available {
            rows.extend(identity_rows);
        }
    }
    rows
}

fn push_wrapped(rows: &mut Vec<DetailRow>, text: String, tone: DetailTone, width: u16) {
    rows.extend(wrap_lines(&text, width).into_iter().map(|text| DetailRow {
        text,
        tone,
        is_seam: false,
    }));
}

fn truncate_detail_rows(mut rows: Vec<DetailRow>, maximum_rows: usize) -> Vec<DetailRow> {
    if rows.len() <= maximum_rows {
        return rows;
    }
    let head = maximum_rows.div_ceil(2);
    let tail = maximum_rows - head;
    let mut truncated = Vec::with_capacity(maximum_rows);
    truncated.extend(rows.drain(..head));
    if let Some(last_head) = truncated.last_mut() {
        last_head.is_seam = true;
    }
    if tail > 0 {
        let mut tail_rows = rows.split_off(rows.len() - tail);
        if let Some(first_tail) = tail_rows.first_mut() {
            first_tail.is_seam = true;
        }
        truncated.extend(tail_rows);
    }
    truncated
}

fn tone_color(tone: DetailTone, scheme: ColorScheme) -> Color {
    match (tone, scheme) {
        (DetailTone::Error, ColorScheme::Dark) => Color::Rgb(0xff, 0x75, 0x75),
        (DetailTone::Error, ColorScheme::Light) => Color::Rgb(0xa8, 0x00, 0x00),
        (DetailTone::Warning, ColorScheme::Dark) => Color::Rgb(0xff, 0xd1, 0x66),
        (DetailTone::Warning, ColorScheme::Light) => Color::Rgb(0x8a, 0x5a, 0x00),
        (DetailTone::Identity, ColorScheme::Dark) => Color::Rgb(0x7d, 0xd8, 0x93),
        (DetailTone::Identity, ColorScheme::Light) => Color::Rgb(0x1b, 0x6f, 0x32),
        (DetailTone::Normal, _) => theme::accent_bg(),
    }
}

fn detail_tone_color(tone: DetailTone, scheme: ColorScheme) -> Option<Color> {
    match tone {
        DetailTone::Normal => None,
        _ => Some(tone_color(tone, scheme)),
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::{ProgressTaskReport, ProgressValidationError};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    fn progress(
        status: ProgressTaskStatus,
        percent: f32,
        message: &str,
        error: Option<&str>,
    ) -> Result<PaneProgress, ProgressValidationError> {
        PaneProgress::new(
            17,
            ProgressTaskReport::new(
                "render-42".to_string(),
                status,
                percent,
                message.to_string(),
                String::new(),
                error.map(str::to_string),
            )?,
            123,
        )
    }

    fn rendered_rows(progress: &PaneProgress, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    Some(progress),
                    height.saturating_sub(1),
                    ColorScheme::Dark,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn rendered_monitor_rows(monitors: &[&PaneProgress], width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_monitors(
                    frame,
                    frame.area(),
                    monitors,
                    height.saturating_sub(1),
                    ColorScheme::Dark,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn monitor(monitor_id: u64, job_id: &str, percent: f32) -> PaneProgress {
        let mut progress = progress(ProgressTaskStatus::Running, percent, "working", None).unwrap();
        progress.monitor_id = monitor_id;
        progress.report.job_id = job_id.to_string();
        progress
    }

    #[test]
    fn several_monitors_share_the_footer_one_gauge_row_each() {
        let build = monitor(1, "build", 20.0);
        let tests = monitor(2, "tests", 70.0);
        let rows = rendered_monitor_rows(&[&build, &tests], 60, 4);
        assert!(
            rows[0].contains("20%") && rows[0].contains("build"),
            "{rows:?}"
        );
        assert!(
            rows[1].contains("70%") && rows[1].contains("tests"),
            "{rows:?}"
        );
        assert!(rows[2].contains("build: working"), "{rows:?}");
        assert_eq!(monitor_at_row(2, 4, 0), Some(0));
        assert_eq!(monitor_at_row(2, 4, 1), Some(1));
        assert_eq!(monitor_at_row(2, 4, 3), Some(1));
    }

    #[test]
    fn monitors_that_do_not_fit_are_counted_on_the_last_row() {
        let monitors: Vec<PaneProgress> = (1..=5)
            .map(|index| monitor(index, &format!("job-{index}"), 10.0))
            .collect();
        let references: Vec<&PaneProgress> = monitors.iter().collect();
        let rows = rendered_monitor_rows(&references, 60, 3);
        assert!(rows[0].contains("job-1"), "{rows:?}");
        assert!(rows[1].contains("job-2"), "{rows:?}");
        assert!(rows[2].contains("+3 more monitors"), "{rows:?}");
        assert_eq!(monitor_at_row(5, 3, 2), None);
        let single_row = rendered_monitor_rows(&references, 60, 1);
        assert!(single_row[0].contains("+4 more"), "{single_row:?}");
    }

    #[test]
    fn small_progress_slot_retains_the_end_of_task_details() {
        let progress = progress(
            ProgressTaskStatus::Running,
            50.0,
            "FIRST SECOND THIRD LAST",
            None,
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(5, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, Some(&progress), 4, ColorScheme::Dark);
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("FIRST"));
        assert!(text.contains("LAST"));
        assert!(!text.contains("SECOND"));
    }

    #[test]
    fn progress_slot_exists_before_a_report() {
        assert_eq!(reserved_height(4), 5);
    }

    #[test]
    fn the_footer_reserves_status_plus_the_configured_detail_budget() {
        assert_eq!(reserved_height(4), 5);
    }

    #[test]
    fn zero_detail_budget_reserves_only_the_status_row() {
        assert_eq!(reserved_height(0), 1);
    }

    #[test]
    fn terminal_statuses_have_distinct_labels_and_details() {
        let done = progress(ProgressTaskStatus::Done, 12.0, "render complete", None).unwrap();
        let error = progress(
            ProgressTaskStatus::Error,
            63.0,
            "encoder stopped",
            Some("exit status 7"),
        )
        .unwrap();

        let done_rows = rendered_rows(&done, 64, 3);
        assert!(done_rows[0].contains("✓ DONE  100%"));
        assert!(done_rows.iter().any(|row| row.contains("render complete")));
        assert!(done_rows.iter().any(|row| row.contains("Job render-42")));

        let error_rows = rendered_rows(&error, 64, 4);
        assert!(error_rows[0].contains("✕ ERROR  63%"));
        assert!(error_rows
            .iter()
            .any(|row| row.contains("Task error: exit status 7")));
    }

    #[test]
    fn monitor_failure_is_not_rendered_as_task_error() {
        let mut progress =
            progress(ProgressTaskStatus::Running, 42.0, "last valid report", None).unwrap();
        progress.monitor_health = ProgressMonitorHealth::Failed {
            consecutive_failures: 5,
            last_error: "probe timed out".to_string(),
        };

        let rows = rendered_rows(&progress, 72, 4);
        assert!(rows[0].contains("⚠ MONITOR FAILED  42%"));
        assert!(rows
            .iter()
            .any(|row| row.contains("Observation stopped (5): probe timed out")));
        assert!(!rows.iter().any(|row| row.contains("Task error:")));
    }

    #[test]
    fn identity_is_omitted_before_critical_evidence_when_space_is_tight() {
        let mut progress = progress(
            ProgressTaskStatus::Error,
            80.0,
            "phase message",
            Some("task failed"),
        )
        .unwrap();
        progress.monitor_health = ProgressMonitorHealth::Degraded {
            consecutive_failures: 2,
            last_error: "probe unavailable".to_string(),
        };

        let rows = detail_rows(&progress, 80, 2);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| !row.text.contains("monitor #")));
        assert!(rows.iter().any(|row| row.text.contains("Task error")));
        assert!(rows
            .iter()
            .any(|row| row.text.contains("Observation degraded")));
    }

    fn progress_with_details(message: &str, details: &str) -> PaneProgress {
        PaneProgress::new(
            17,
            ProgressTaskReport::new(
                "render-42".to_string(),
                ProgressTaskStatus::Running,
                40.0,
                message.to_string(),
                details.to_string(),
                None,
            )
            .unwrap(),
            123,
        )
        .unwrap()
    }

    #[test]
    fn footer_advertises_hover_details_only_when_details_exist() {
        let with = progress_with_details("Building the app - crate 4 of 10", "What: build");
        let without = progress_with_details("Building the app - crate 4 of 10", "");
        assert!(rendered_rows(&with, 80, 3)
            .iter()
            .any(|row| row.contains("Building the app - crate 4 of 10 ⓘ hover for details")));
        assert!(!rendered_rows(&without, 80, 3)
            .iter()
            .any(|row| row.contains("hover for details")));
    }

    #[test]
    fn footer_never_prints_the_long_description() {
        let progress = progress_with_details("Short line", "What: SECRET-LONG-TEXT\nWhy: more");
        assert!(!rendered_rows(&progress, 80, 6)
            .iter()
            .any(|row| row.contains("SECRET-LONG-TEXT")));
    }

    #[test]
    fn hover_tooltip_carries_message_details_and_identity() {
        let progress = progress_with_details(
            "Building the app - crate 4 of 10",
            "What: build\nWhy: release\nNow: crate 4 of 10",
        );
        let tooltip = details_tooltip(&progress);
        assert_eq!(tooltip.title, "Building the app - crate 4 of 10");
        assert_eq!(
            tooltip.body,
            "What: build\nWhy: release\nNow: crate 4 of 10"
        );
        let reason = tooltip.reason.unwrap();
        assert!(reason.contains("job render-42") && reason.contains("monitor #17"));

        let empty = progress_with_details("", "What: x");
        assert!(details_tooltip(&empty).title.contains("RUNNING"));
    }

    #[test]
    fn the_gauge_uses_distinct_filled_and_unfilled_glyphs() {
        let progress = progress(ProgressTaskStatus::Running, 50.0, "", None).unwrap();
        let rows = rendered_rows(&progress, 40, 2);
        assert!(rows[0].contains(symbols::shade::FULL));
        assert!(rows[0].contains(symbols::shade::LIGHT));
    }
}
