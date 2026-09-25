//! Status-aware footer for a server-owned long-task monitor.
//!
//! The task's own status and Ilium's ability to observe it are deliberately
//! distinct. A degraded/failed monitor therefore changes the leading label
//! and adds a warning without repainting the task itself as failed. Terminal
//! task evidence remains visible until it is explicitly cleared or replaced.

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

/// The exact number of rows the footer needs for `progress` at `width`:
/// one status/gauge row plus the bounded detail rows. Job/monitor identity is
/// included only when the configured detail budget has room after task and
/// monitor-health evidence.
pub fn reserved_height(progress: Option<&PaneProgress>, width: u16, max_detail_lines: u16) -> u16 {
    let Some(progress) = progress else {
        return 0;
    };
    1 + detail_rows(progress, width, max_detail_lines).len() as u16
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

    // Domain validation rejects non-finite/out-of-range reports, but the
    // renderer remains defensive because one bad restored value must never
    // crash the entire TUI.
    let percent = progress.report.percent.clamp(0.0, 100.0);
    let ratio = (f64::from(percent) / 100.0).clamp(0.0, 1.0);
    let (icon, status_label, status_tone) = status_presentation(progress);
    let status_color = tone_color(status_tone, scheme);
    let label = format!("{icon} {status_label}  {percent:.0}%");
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
    frame.render_widget(gauge, gauge_area);

    let lines: Vec<Line> = detail_rows(progress, detail_area.width, max_detail_lines)
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
        push_wrapped(
            &mut critical,
            progress.report.message.clone(),
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

    #[test]
    fn no_progress_reserves_nothing() {
        assert_eq!(reserved_height(None, 80, 4), 0);
    }

    #[test]
    fn running_task_reserves_status_message_and_identity_rows() {
        let progress = progress(ProgressTaskStatus::Running, 50.0, "frame 10/100", None).unwrap();
        assert_eq!(reserved_height(Some(&progress), 80, 4), 3);
    }

    #[test]
    fn zero_detail_budget_reserves_only_the_status_row() {
        let progress = progress(ProgressTaskStatus::Running, 50.0, "working", None).unwrap();
        assert_eq!(reserved_height(Some(&progress), 80, 0), 1);
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

    #[test]
    fn the_gauge_uses_distinct_filled_and_unfilled_glyphs() {
        let progress = progress(ProgressTaskStatus::Running, 50.0, "", None).unwrap();
        let rows = rendered_rows(&progress, 40, 2);
        assert!(rows[0].contains(symbols::shade::FULL));
        assert!(rows[0].contains(symbols::shade::LIGHT));
    }
}
