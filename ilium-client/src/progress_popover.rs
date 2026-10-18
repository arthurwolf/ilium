//! Progress diagnostics and the last accepted check, with bounded observation history.
use ilium_core::PaneProgress;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub(crate) fn report_lines(progress: &PaneProgress) -> Vec<String> {
    let report = &progress.report;
    let status = match report.status {
        ilium_core::ProgressTaskStatus::NotStartedYet => "not-started-yet",
        ilium_core::ProgressTaskStatus::Running => "running",
        ilium_core::ProgressTaskStatus::Done => "done",
        ilium_core::ProgressTaskStatus::Error => "error",
    };
    let mut lines = vec![
        format!("checked: {}", crate::progress_time::timestamp_text(progress.last_observed_unix_millis)),
        format!("├─ job_id: {}", report.job_id),
        format!("├─ status: {status}"),
        format!("├─ percent: {}", report.percent),
        format!("├─ message: {}", report.message),
        "├─ details:".into(),
    ];
    if report.details.is_empty() {
        lines.push("│  └─ (empty)".into());
    } else {
        lines.extend(report.details.lines().map(|line| format!("│  │ {line}")));
    }
    lines.push(format!("├─ eta_seconds: {}", report.eta_seconds.map_or_else(|| "not supplied".into(), |value| value.to_string())));
    lines.push(format!("└─ error: {}", report.error.as_deref().unwrap_or("none")));
    lines
}

/// Time-scaled bins expose gaps; the event strip retains mixed outcomes in a busy bin.
pub(crate) fn timeline_lines(progress: &PaneProgress, now: u64, width: u16) -> Vec<String> {
    use ilium_core::ProgressCheckOutcome;
    let checks = &progress.timing.checks;
    if checks.is_empty() {
        return vec!["Recent checks: history unavailable".into()];
    }
    let interval = progress.timing.probe_interval_millis.unwrap_or(60_000);
    let window = interval.saturating_mul(10).max(600_000);
    let settled = progress.is_terminal() || progress.monitor_health.is_failed();
    let end = if settled { checks.last().map_or(progress.last_observed_unix_millis, |check| check.unix_millis) } else { now };
    let start = end.saturating_sub(window);
    // Retention is bounded: absence before the first retained attempt proves nothing.
    let coverage_start = checks.first().map_or(end, |check| check.unix_millis);
    let columns = usize::from(width.saturating_sub(2).clamp(4, 48));
    let mut bins: Vec<char> = (0..columns).map(|index| {
        let bin_start = start.saturating_add(((u128::from(window) * index as u128) / columns as u128) as u64);
        if bin_start < coverage_start { '?' } else { '·' }
    }).collect();
    for check in checks.iter().filter(|check| check.unix_millis >= start && check.unix_millis <= end) {
        let index = ((check.unix_millis.saturating_sub(start) as u128 * columns as u128) / u128::from(window.max(1))) as usize;
        let cell = &mut bins[index.min(columns - 1)];
        let symbol = match check.outcome { ProgressCheckOutcome::Progress => '▲', ProgressCheckOutcome::Heartbeat => '●', ProgressCheckOutcome::Failed => '×' };
        // Failure remains visible even when another check shares its time bin.
        if *cell != '×' && (symbol != '●' || matches!(*cell, '·' | '?')) { *cell = symbol; }
    }
    let recent: String = checks.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().map(|check| match check.outcome { ProgressCheckOutcome::Progress => '▲', ProgressCheckOutcome::Heartbeat => '●', ProgressCheckOutcome::Failed => '×' }).collect();
    vec![
        format!("Checks over {} → {}", crate::progress_time::human_duration(window), if settled { "last attempt" } else { "now" }),
        format!("┊{}┊", bins.iter().collect::<String>()),
        format!("Last {} checks: {recent}", checks.len().min(10)),
        "● healthy check  ▲ progress  × failed check  · no recorded check  ? unavailable".into(),
        "A healthy check confirms the probe, not that the job is alive.".into(),
    ]
}

fn wrap(lines: &[String], width: u16) -> Vec<String> {
    lines.iter().flat_map(|line| crate::last_prompt_banner::wrap_lines(line, width.max(1))).collect()
}

fn fit(mut lines: Vec<String>, height: u16) -> Vec<String> {
    if lines.len() > usize::from(height) {
        lines.truncate(usize::from(height));
        if let Some(last) = lines.last_mut() {
            *last = "… F9: full data".into();
        }
    }
    lines
}

fn panels(screen: Rect, anchor: Position) -> (Rect, Rect, Rect) {
    let width = screen.width.min(140);
    let height = screen.height.min(40);
    let x = anchor.x.min(screen.right().saturating_sub(width)).max(screen.x);
    let y = anchor.y.saturating_sub(height).max(screen.y).min(screen.bottom().saturating_sub(height));
    let outer = Rect::new(x, y, width, height);
    let inner = Rect::new(x.saturating_add(1), y.saturating_add(1), width.saturating_sub(2), height.saturating_sub(2));
    if width >= 92 {
        let left_width = inner.width / 2;
        (outer, Rect::new(inner.x, inner.y, left_width, inner.height), Rect::new(inner.x + left_width, inner.y, inner.width - left_width, inner.height))
    } else {
        let left_height = inner.height / 2;
        (outer, Rect::new(inner.x, inner.y, inner.width, left_height), Rect::new(inner.x, inner.y + left_height, inner.width, inner.height - left_height))
    }
}

pub fn render(frame: &mut Frame, screen: Rect, anchor: Position, progress: &PaneProgress, now: u64, owner: Option<&str>) {
    if screen.width < 8 || screen.height < 6 { return; }
    let (outer, left, right) = panels(screen, anchor);
    frame.render_widget(Clear, outer);
    frame.render_widget(crate::theme::block(true).title(" Task progress · F9 details "), outer);
    let existing = crate::progress_bar::summary_tooltip(progress, now, owner);
    let mut diagnostics = vec![existing.title, existing.body];
    if let Some(reason) = existing.reason { diagnostics.push(reason); }
    let left_block = Block::default().title(" Progress and observation ");
    let right_block = Block::default().borders(if outer.width >= 92 { Borders::LEFT } else { Borders::TOP }).title(" Latest successful check data ");
    let left_content = left_block.inner(left);
    let right_content = right_block.inner(right);
    frame.render_widget(left_block, left);
    frame.render_widget(right_block, right);
    let timeline = wrap(&timeline_lines(progress, now, left_content.width), left_content.width);
    let timeline_height = if left_content.height >= 12 { (timeline.len() as u16).min(left_content.height / 2) } else { 0 };
    let diagnostics_area = Rect::new(left_content.x, left_content.y, left_content.width, left_content.height.saturating_sub(timeline_height));
    frame.render_widget(Paragraph::new(fit(wrap(&diagnostics, diagnostics_area.width), diagnostics_area.height).join("\n")), diagnostics_area);
    if timeline_height > 0 {
        let timeline_area = Rect::new(left_content.x, diagnostics_area.bottom(), left_content.width, timeline_height);
        frame.render_widget(Paragraph::new(fit(timeline, timeline_height).join("\n")), timeline_area);
    }
    frame.render_widget(Paragraph::new(fit(wrap(&report_lines(progress), right_content.width), right_content.height).join("\n")), right_content);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{ProgressTaskReport, ProgressTaskStatus};
    use ratatui::{backend::TestBackend, Terminal};
    fn progress() -> PaneProgress {
        PaneProgress::new(1, ProgressTaskReport { job_id: "compiler".into(), status: ProgressTaskStatus::Running, percent: 20.0, message: "Compiling project".into(), details: "What: compiling\nNow: phase two".into(), eta_seconds: Some(120), error: None }, 1000).unwrap()
    }
    #[test]
    fn timeline_distinguishes_progress_heartbeat_failure_and_gaps() {
        use ilium_core::{ProgressCheck, ProgressCheckOutcome};
        let mut progress = progress();
        progress.timing.checks = vec![
            ProgressCheck { unix_millis: 100_000, outcome: ProgressCheckOutcome::Heartbeat, percent: Some(20.0) },
            ProgressCheck { unix_millis: 200_000, outcome: ProgressCheckOutcome::Progress, percent: Some(21.0) },
            ProgressCheck { unix_millis: 300_000, outcome: ProgressCheckOutcome::Failed, percent: None },
        ];
        let text = timeline_lines(&progress, 600_000, 40).join("\n");
        assert!(text.contains("●▲×"), "{text}");
        assert!(text.contains('·'), "{text}");
        assert!(text.contains("10 minutes"), "{text}");
        progress.timing.probe_interval_millis = Some(120_000);
        assert!(timeline_lines(&progress, 600_000, 40)[0].contains("20 minutes"));
    }
    #[test]
    fn unavailable_history_is_not_presented_as_observed_silence() {
        use ilium_core::{ProgressCheck, ProgressCheckOutcome};
        let mut progress = progress();
        progress.timing.checks = (537..=600).map(|second| ProgressCheck { unix_millis: second * 1000, outcome: ProgressCheckOutcome::Heartbeat, percent: Some(20.0) }).collect();
        let timeline = timeline_lines(&progress, 600_000, 48);
        assert!(timeline[1].contains('?'));
        assert!(timeline[1].contains('●'));
        assert!(!timeline[1].contains('·'), "{}", timeline[1]);
    }
    #[test]
    fn completed_and_failed_monitor_timelines_freeze_at_last_attempt() {
        let mut progress = progress();
        progress.report.status = ProgressTaskStatus::Done;
        assert_eq!(timeline_lines(&progress, 10_000, 40), timeline_lines(&progress, 9_000_000, 40));
        assert!(timeline_lines(&progress, 10_000, 40)[0].contains("last attempt"));
        progress.report.status = ProgressTaskStatus::Running;
        progress.monitor_health = ilium_core::ProgressMonitorHealth::Failed { consecutive_failures: 3, last_error: "timeout".into() };
        assert_eq!(timeline_lines(&progress, 10_000, 40), timeline_lines(&progress, 9_000_000, 40));
    }
    #[test]
    fn full_diagnostic_reaches_data_beyond_hover_capacity() {
        let mut progress = progress();
        progress.report.details = (0..80).map(|index| format!("line {index}")).collect::<Vec<_>>().join("\n");
        let diagnostic = crate::progress_bar::details_tooltip(&progress, 2000, None);
        assert!(diagnostic.body.contains("│  │ line 79"));
        assert!(diagnostic.body.contains("eta_seconds: 120"));
        assert!(diagnostic.body.contains("error: none"));
    }
    #[test]
    fn full_report_keeps_fields_and_multiline_details() {
        let text = report_lines(&progress()).join("\n");
        for field in ["job_id: compiler", "status: running", "percent: 20", "message:", "What: compiling", "Now: phase two", "eta_seconds: 120", "error: none"] { assert!(text.contains(field), "{text}"); }
    }
    #[test]
    fn compact_and_wide_panels_stay_inside_nonzero_screen() {
        for width in [8, 32, 80, 120, 180] {
            let screen = Rect::new(3, 4, width, 30);
            let (outer, left, right) = panels(screen, Position::new(200, 200));
            assert_eq!(outer.intersection(screen), outer);
            assert_eq!(left.intersection(outer), left);
            assert_eq!(right.intersection(outer), right);
            if width >= 92 { assert_eq!(left.y, right.y); } else { assert_eq!(left.x, right.x); }
        }
    }
    #[test]
    fn rendered_wide_and_compact_check_data_is_visible() {
        for (width, height) in [(120, 36), (60, 40), (20, 10)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, frame.area(), Position::new(0, 0), &progress(), 2000, None)).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            let capture = (0..height).map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n");
            println!("PROGRESS POPOVER {width} × {height}\n{capture}");
            assert!(text.contains("Task progress"), "{text}");
            if width >= 60 { assert!(text.contains("job_id: compiler"), "{text}"); assert!(text.contains("Latest successful check data"), "{text}"); }
        }
    }
}
