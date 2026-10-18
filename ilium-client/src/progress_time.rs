//! Human-readable timing for progress monitors: the compact box beside the
//! gauge (started-ago, ETA or stalled marker) and the timing lines of the
//! hover popover. Everything takes `now` as a parameter so it is testable
//! without a clock.

use chrono::{Local, TimeZone};
use ilium_core::{PaneProgress, ProgressMonitorHealth, ProgressTaskStatus};
use unicode_width::UnicodeWidthStr;

const SECOND: u64 = 1000;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
/// Ages under this read as "just now" rather than counting seconds.
const JUST_NOW_MILLIS: u64 = 10 * SECOND;
const SEGMENT_SEPARATOR: &str = " \u{b7} ";

/// `millis` as at most two adjacent units: `3 days, 12 hours`, `13 minutes`,
/// `45 seconds`. A minor unit that is zero is omitted.
pub fn human_duration(millis: u64) -> String {
    let units = [
        (DAY, "day"),
        (HOUR, "hour"),
        (MINUTE, "minute"),
        (SECOND, "second"),
    ];
    let plural = |count: u64, name: &str| {
        if count == 1 {
            format!("1 {name}")
        } else {
            format!("{count} {name}s")
        }
    };
    for (index, (size, name)) in units.iter().enumerate() {
        let major = millis / size;
        if major == 0 {
            continue;
        }
        let mut text = plural(major, name);
        if let Some((minor_size, minor_name)) = units.get(index + 1) {
            let minor = (millis % size) / minor_size;
            if minor > 0 {
                text = format!("{text}, {}", plural(minor, minor_name));
            }
        }
        return text;
    }
    "less than a second".to_string()
}

/// `13 minutes ago`, or `just now` for a very recent time.
pub fn human_ago(millis: u64) -> String {
    if millis < JUST_NOW_MILLIS {
        "just now".to_string()
    } else {
        format!("{} ago", human_duration(millis))
    }
}

/// `in 8 minutes`, or `any moment` when nothing remains.
pub fn human_in(millis: u64) -> String {
    if millis < JUST_NOW_MILLIS {
        "any moment".to_string()
    } else {
        format!("in {}", human_duration(millis))
    }
}

/// Local time as `YYYY-MM-DD HH:mm:ss`.
pub fn timestamp_text(unix_millis: u64) -> String {
    i64::try_from(unix_millis)
        .ok()
        .and_then(|millis| Local.timestamp_millis_opt(millis).single())
        .map_or_else(
            || "unknown".to_string(),
            |time| time.format("%Y-%m-%d %H:%M:%S").to_string(),
        )
}

/// One piece of the box beside the gauge; `warn` pieces take the warning
/// colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSegment {
    pub text: String,
    pub warn: bool,
}

fn segments_width(segments: &[TimeSegment]) -> usize {
    let text: usize = segments
        .iter()
        .map(|segment| UnicodeWidthStr::width(segment.text.as_str()))
        .sum();
    text + UnicodeWidthStr::width(SEGMENT_SEPARATOR) * segments.len().saturating_sub(1)
}

fn is_running(progress: &PaneProgress) -> bool {
    progress.is_live() && progress.report.status == ProgressTaskStatus::Running
}

/// Whether this monitor's task is running but has made no progress for the
/// stall threshold (ten probe intervals, at least ten minutes).
pub fn is_stalled(progress: &PaneProgress, now_unix_millis: u64) -> bool {
    is_running(progress) && progress.timing.is_stalled(now_unix_millis)
}

/// The box's content from most to least informative, so the renderer can
/// take the first variant that fits. The last variant is empty.
pub fn time_box_variants(progress: &PaneProgress, now_unix_millis: u64) -> Vec<Vec<TimeSegment>> {
    let timing = &progress.timing;
    let started = timing
        .elapsed_millis(now_unix_millis)
        .map(|elapsed| TimeSegment {
            text: format!("started {}", human_ago(elapsed)),
            warn: false,
        });
    let stalled = is_stalled(progress, now_unix_millis).then(|| TimeSegment {
        text: format!(
            "\u{26a0} no progress reported {}",
            human_duration(timing.quiet_millis(now_unix_millis).unwrap_or(0))
        ),
        warn: true,
    });
    let eta = (is_running(progress) && stalled.is_none()).then(|| TimeSegment {
        text: timing
            .estimate(now_unix_millis, progress.report.percent)
            .map_or_else(
                || "ETA \u{2014}".to_string(),
                |estimate| format!("ETA {}", human_in(estimate.expected_millis)),
            ),
        warn: false,
    });
    let tail = stalled.clone().or(eta);

    let mut variants = Vec::new();
    let full: Vec<TimeSegment> = started.iter().chain(tail.iter()).cloned().collect();
    if !full.is_empty() {
        variants.push(full);
    }
    // Next to drop is the ETA; a stall marker outranks the start time.
    if let Some(stalled) = stalled {
        variants.push(vec![stalled]);
        variants.push(vec![TimeSegment { text: "⚠ no progress reported".into(), warn: true }]);
    } else if let Some(started) = started {
        variants.push(vec![started]);
    }
    variants.dedup();
    variants.push(Vec::new());
    variants
}

/// The first variant whose width, quantised up to a multiple of eight cells
/// (so the bar does not twitch as numbers change length), fits `available`.
/// Returns the segments and the box width in cells.
pub fn fit_time_box(
    variants: Vec<Vec<TimeSegment>>,
    available: usize,
) -> Option<(Vec<TimeSegment>, usize)> {
    variants.into_iter().find_map(|segments| {
        if segments.is_empty() {
            return None;
        }
        let width = segments_width(&segments).div_ceil(8) * 8;
        (width <= available).then_some((segments, width))
    })
}

pub const fn segment_separator() -> &'static str {
    SEGMENT_SEPARATOR
}

pub fn segments_text_width(segments: &[TimeSegment]) -> usize {
    segments_width(segments)
}

/// The timing section of the hover popover: start and last progress as
/// timestamps, elapsed time, pace, ETA with its range, probe health and the
/// agent that owns the monitor.
pub fn popover_lines(
    progress: &PaneProgress,
    now_unix_millis: u64,
    owner: Option<&str>,
) -> Vec<String> {
    let timing = &progress.timing;
    let mut lines = Vec::new();
    match timing.started_at_unix_millis {
        Some(started) => lines.push(format!(
            "Started: {} ({})",
            timestamp_text(started),
            human_ago(now_unix_millis.saturating_sub(started))
        )),
        None => lines.push("Started: unknown (monitor restored from an older snapshot)".into()),
    }
    if let Some(last) = timing.last_progress_unix_millis {
        let quiet = now_unix_millis.saturating_sub(last);
        let stalled = if is_stalled(progress, now_unix_millis) {
            format!(
                " \u{26a0} no reported progress: threshold {}",
                human_duration(timing.stall_threshold_millis())
            )
        } else {
            String::new()
        };
        lines.push(format!(
            "Last progress: {} ({}){stalled}",
            timestamp_text(last),
            human_ago(quiet)
        ));
    }
    if progress.is_terminal() {
        if let Some(elapsed) = timing.elapsed_millis(progress.last_observed_unix_millis) {
            lines.push(format!("Took: {}", human_duration(elapsed)));
        }
    } else if let Some(elapsed) = timing.elapsed_millis(now_unix_millis) {
        lines.push(format!("Elapsed: {}", human_duration(elapsed)));
    }
    if is_running(progress) && !is_stalled(progress, now_unix_millis) {
        match timing.rate(progress.report.percent) {
            Some(rate) => {
                let mut text = format!("Rate: {:.1} %/min", rate.percent_per_minute);
                if let (Some(recent), Some(whole)) = (
                    rate.recent_percent_per_minute,
                    rate.whole_run_percent_per_minute,
                ) {
                    text.push_str(&format!(" (recent {recent:.1}, whole run {whole:.1})"));
                }
                lines.push(text);
            }
            None => lines.push("Rate: unknown (no progress measured yet)".into()),
        }
        match timing.estimate(now_unix_millis, progress.report.percent) {
            Some(estimate) => {
                let mut text = format!("ETA: {}", human_in(estimate.expected_millis));
                if let Some((fastest, slowest)) = estimate.range_millis() {
                    text.push_str(&format!(
                        " (range {} to {})",
                        human_duration(fastest),
                        human_duration(slowest)
                    ));
                }
                lines.push(text);
            }
            None => lines.push("ETA: unknown".into()),
        }
    }
    lines.push(format!("Last successful check: {} ({})", timestamp_text(progress.last_observed_unix_millis), human_ago(now_unix_millis.saturating_sub(progress.last_observed_unix_millis))));
    let interval = timing
        .probe_interval_millis
        .map(|millis| format!("every {}", human_duration(millis)))
        .unwrap_or_else(|| "interval unknown".into());
    let checked = human_ago(now_unix_millis.saturating_sub(progress.last_observed_unix_millis));
    let health = match &progress.monitor_health {
        ProgressMonitorHealth::Healthy => "healthy".to_string(),
        ProgressMonitorHealth::Degraded {
            consecutive_failures,
            ..
        } => format!("degraded, {consecutive_failures} consecutive failures"),
        ProgressMonitorHealth::Failed {
            consecutive_failures,
            ..
        } => format!("stopped after {consecutive_failures} consecutive failures"),
    };
    lines.push(format!(
        "Probe: {interval}, last checked {checked}, {health}"
    ));
    if let Some(owner) = owner {
        lines.push(format!("Owner: {owner}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{ProgressTaskReport, ProgressTiming};

    fn running(percent: f32, timing: ProgressTiming, observed: u64) -> PaneProgress {
        let mut progress = PaneProgress::new(
            3,
            ProgressTaskReport {
                job_id: "build".into(),
                status: ProgressTaskStatus::Running,
                percent,
                message: "Building".into(),
                details: String::new(),
                error: None,
                eta_seconds: None,
            },
            observed,
        )
        .unwrap();
        progress.timing = timing;
        progress
    }

    #[test]
    fn durations_use_two_adjacent_units() {
        assert_eq!(human_duration(45 * SECOND), "45 seconds");
        assert_eq!(human_duration(13 * MINUTE), "13 minutes");
        assert_eq!(human_duration(HOUR + 5 * MINUTE), "1 hour, 5 minutes");
        assert_eq!(
            human_duration(3 * DAY + 12 * HOUR + 7 * MINUTE),
            "3 days, 12 hours"
        );
        assert_eq!(human_duration(2 * DAY + 30 * MINUTE), "2 days");
        assert_eq!(human_duration(0), "less than a second");
    }

    #[test]
    fn ago_and_in_read_naturally() {
        assert_eq!(human_ago(3 * SECOND), "just now");
        assert_eq!(human_ago(13 * MINUTE), "13 minutes ago");
        assert_eq!(human_ago(3 * DAY + 12 * HOUR), "3 days, 12 hours ago");
        assert_eq!(human_in(8 * MINUTE), "in 8 minutes");
        assert_eq!(human_in(0), "any moment");
    }

    fn steady_timing() -> ProgressTiming {
        let mut timing = ProgressTiming::started(0, 0.0);
        timing.probe_interval_millis = Some(30 * SECOND);
        for minute in 1..=10 {
            timing.observe(minute * MINUTE, 5.0 * minute as f32, false);
        }
        timing
    }

    fn texts(variant: &[TimeSegment]) -> Vec<&str> {
        variant
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    #[test]
    fn running_box_shows_started_and_eta_then_degrades() {
        let progress = running(50.0, steady_timing(), 10 * MINUTE);
        let variants = time_box_variants(&progress, 10 * MINUTE);
        assert_eq!(
            texts(&variants[0]),
            ["started 10 minutes ago", "ETA in 10 minutes"]
        );
        assert_eq!(texts(&variants[1]), ["started 10 minutes ago"]);
        assert!(variants.last().unwrap().is_empty());
    }

    #[test]
    fn stalled_task_replaces_the_eta_with_a_warning_that_outranks_the_start() {
        let progress = running(50.0, steady_timing(), 10 * MINUTE);
        let now = 10 * MINUTE + 11 * MINUTE;
        assert!(is_stalled(&progress, now));
        let variants = time_box_variants(&progress, now);
        assert_eq!(variants[0][1].text, "\u{26a0} no progress reported 11 minutes");
        assert!(variants[0][1].warn);
        assert_eq!(texts(&variants[1]), ["\u{26a0} no progress reported 11 minutes"]);
    }

    #[test]
    fn finished_task_shows_only_when_it_started() {
        let mut progress = running(100.0, steady_timing(), 10 * MINUTE);
        progress.report.status = ProgressTaskStatus::Done;
        let variants = time_box_variants(&progress, 11 * MINUTE);
        assert_eq!(texts(&variants[0]), ["started 11 minutes ago"]);
        assert!(!is_stalled(&progress, 99 * HOUR));
    }

    #[test]
    fn unknown_timing_hides_the_start_and_never_claims_a_stall() {
        let progress = running(40.0, ProgressTiming::default(), 5);
        let variants = time_box_variants(&progress, 99 * HOUR);
        assert_eq!(
            variants.len(),
            2,
            "only the ETA placeholder and empty variants"
        );
        assert_eq!(texts(&variants[0]), ["ETA \u{2014}"]);
    }

    #[test]
    fn fitting_quantises_width_and_drops_variants_that_do_not_fit() {
        let progress = running(50.0, steady_timing(), 10 * MINUTE);
        let variants = time_box_variants(&progress, 10 * MINUTE);
        let (segments, width) = fit_time_box(variants.clone(), 80).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(width % 8, 0);
        let (segments, _) = fit_time_box(variants.clone(), 24).unwrap();
        assert_eq!(texts(&segments), ["started 10 minutes ago"]);
        assert!(fit_time_box(variants, 8).is_none());
    }

    #[test]
    fn popover_lists_every_timing_fact() {
        let progress = running(50.0, steady_timing(), 10 * MINUTE);
        let lines = popover_lines(&progress, 10 * MINUTE, Some("Claude Code in \"api\""));
        let text = lines.join("\n");
        assert!(text.contains("Started: "), "{text}");
        assert!(text.contains("Last progress: "), "{text}");
        assert!(text.contains("Elapsed: 10 minutes"), "{text}");
        assert!(text.contains("Rate: 5.0 %/min"), "{text}");
        assert!(text.contains("ETA: in 10 minutes"), "{text}");
        assert!(
            text.contains("Probe: every 30 seconds, last checked just now, healthy"),
            "{text}"
        );
        assert!(text.contains("Owner: Claude Code in \"api\""), "{text}");
    }

    #[test]
    fn popover_reports_total_time_when_finished_and_stall_when_stuck() {
        let mut done = running(100.0, steady_timing(), 12 * MINUTE);
        done.report.status = ProgressTaskStatus::Done;
        let text = popover_lines(&done, 60 * MINUTE, None).join("\n");
        assert!(text.contains("Took: 12 minutes"), "{text}");
        assert!(!text.contains("ETA"), "{text}");
        assert!(!text.contains("Owner"), "{text}");

        let stuck = running(50.0, steady_timing(), 10 * MINUTE);
        let text = popover_lines(&stuck, 25 * MINUTE, None).join("\n");
        assert!(text.contains("no reported progress: threshold 10 minutes"), "{text}");
    }

    #[test]
    fn popover_reports_degraded_probe_health() {
        let mut progress = running(50.0, steady_timing(), 10 * MINUTE);
        progress.monitor_health = ProgressMonitorHealth::Degraded {
            consecutive_failures: 3,
            last_error: "probe timed out".into(),
        };
        let text = popover_lines(&progress, 10 * MINUTE, None).join("\n");
        assert!(text.contains("degraded, 3 consecutive failures"), "{text}");
    }
}
