//! Time scales and the "work ribbon" for the costs-and-stats charts.
//!
//! Every chart in the popover plots the same session timeline. A
//! [`StatsScale`] picks how much of it is shown (the whole session or the most
//! recent hour, ...), and [`work_ribbon`] folds the agent's recorded state
//! changes ([`WorkMark`]) and progress-bar runs ([`ProgressSpan`]) into one
//! cell per chart column, so the ribbon lines up with the plot above it.
//!
//! Pure functions of a [`SessionStats`]: no rendering, no clock.

use crate::session_stats::{ProgressOutcome, SessionStats, WorkKind};

/// How much of the session the time charts show, ending at the last event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsScale {
    #[default]
    All,
    Day,
    SixHours,
    Hour,
    QuarterHour,
    FiveMinutes,
}

impl StatsScale {
    pub const ALL: [StatsScale; 6] = [
        Self::All,
        Self::Day,
        Self::SixHours,
        Self::Hour,
        Self::QuarterHour,
        Self::FiveMinutes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Day => "24h",
            Self::SixHours => "6h",
            Self::Hour => "1h",
            Self::QuarterHour => "15m",
            Self::FiveMinutes => "5m",
        }
    }

    /// Length of the shown window; `None` means the whole session.
    pub fn duration_ms(self) -> Option<i64> {
        match self {
            Self::All => None,
            Self::Day => Some(24 * 3_600_000),
            Self::SixHours => Some(6 * 3_600_000),
            Self::Hour => Some(3_600_000),
            Self::QuarterHour => Some(15 * 60_000),
            Self::FiveMinutes => Some(5 * 60_000),
        }
    }

    /// First and last millisecond shown: the last `duration_ms` of the
    /// session, never starting before its first event.
    pub fn window(self, stats: &SessionStats) -> Option<(i64, i64)> {
        let (first, last) = (stats.first_at_ms?, stats.last_at_ms?);
        let start = match self.duration_ms() {
            None => first,
            Some(duration) => last.saturating_sub(duration).max(first),
        };
        Some((start, last.max(start)))
    }
}

/// One chart column of the ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonCell {
    /// What the agent spent most of the column's time doing.
    pub kind: WorkKind,
    /// A progress bar was running during the column.
    pub progress_running: bool,
}

/// The ribbon and the progress outcomes under it, one entry per column.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkRibbon {
    /// `None` where the transcript recorded nothing yet.
    pub cells: Vec<Option<RibbonCell>>,
    /// Terminal progress outcome per column (`Success`, `Failure`, `Unknown`).
    pub outcomes: Vec<Option<ProgressOutcome>>,
}

impl WorkRibbon {
    pub fn has_data(&self) -> bool {
        self.cells.iter().any(Option::is_some)
    }

    /// Kinds that appear in at least one column, in legend order.
    pub fn kinds_present(&self) -> Vec<WorkKind> {
        WorkKind::ALL
            .into_iter()
            .filter(|kind| self.cells.iter().flatten().any(|cell| cell.kind == *kind))
            .collect()
    }

    pub fn has_running_progress(&self) -> bool {
        self.cells
            .iter()
            .flatten()
            .any(|cell| cell.progress_running)
    }

    pub fn has_outcome(&self, outcome: ProgressOutcome) -> bool {
        self.outcomes
            .iter()
            .flatten()
            .any(|found| *found == outcome)
    }
}

fn outcome_rank(outcome: ProgressOutcome) -> u8 {
    match outcome {
        ProgressOutcome::Failure => 3,
        ProgressOutcome::Unknown => 2,
        ProgressOutcome::Success => 1,
        ProgressOutcome::Running | ProgressOutcome::Cleared => 0,
    }
}

/// Folds the session's state changes into `columns` equal slices of
/// `start_ms..=end_ms`.
pub fn work_ribbon(stats: &SessionStats, start_ms: i64, end_ms: i64, columns: usize) -> WorkRibbon {
    let mut ribbon = WorkRibbon {
        cells: vec![None; columns],
        outcomes: vec![None; columns],
    };
    if columns == 0 || end_ms < start_ms {
        return ribbon;
    }
    let session_end = stats.last_at_ms.unwrap_or(end_ms);
    let width = ((end_ms - start_ms).max(1) as f64 / columns as f64).max(1.0);
    let marks = &stats.work_marks;
    // Mark `index` owns `at_ms .. next.at_ms`; the last one runs to the end
    // of the session.
    let mark_end = |index: usize| marks.get(index + 1).map_or(session_end, |next| next.at_ms);

    for (column, cell) in ribbon.cells.iter_mut().enumerate() {
        let from = start_ms + (column as f64 * width) as i64;
        let to = start_ms + ((column + 1) as f64 * width) as i64;
        // First mark that could reach into this column.
        let first = marks
            .partition_point(|mark| mark.at_ms <= from)
            .saturating_sub(1);
        let mut overlap = [0_i64; WorkKind::ALL.len()];
        for (index, mark) in marks.iter().enumerate().skip(first) {
            if mark.at_ms >= to.max(from + 1) {
                break;
            }
            let covered = mark_end(index).min(to) - mark.at_ms.max(from);
            if covered > 0 {
                overlap[mark.kind as usize] += covered;
            }
        }
        let best = WorkKind::ALL
            .into_iter()
            .filter(|kind| overlap[*kind as usize] > 0)
            .max_by_key(|kind| (overlap[*kind as usize], *kind != WorkKind::Idle));
        let progress_running = stats.progress_spans.iter().any(|span| {
            let span_end = span.end_ms.unwrap_or(session_end);
            span.start_ms < to.max(from + 1) && span_end >= from
        });
        *cell = best.map(|kind| RibbonCell {
            kind,
            progress_running,
        });
    }

    for span in &stats.progress_spans {
        let Some(end) = span.end_ms else {
            continue;
        };
        if outcome_rank(span.outcome) == 0 || end < start_ms || end > end_ms {
            continue;
        }
        let column = (((end - start_ms) as f64 / width) as usize).min(columns - 1);
        let slot = &mut ribbon.outcomes[column];
        if slot.is_none_or(|held| outcome_rank(span.outcome) > outcome_rank(held)) {
            *slot = Some(span.outcome);
        }
    }
    ribbon
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_stats::{ProgressSpan, WorkMark};

    fn stats_with(marks: &[(i64, WorkKind)], spans: Vec<ProgressSpan>, last: i64) -> SessionStats {
        SessionStats {
            first_at_ms: Some(marks.first().map_or(0, |mark| mark.0)),
            last_at_ms: Some(last),
            work_marks: marks
                .iter()
                .map(|(at_ms, kind)| WorkMark {
                    at_ms: *at_ms,
                    kind: *kind,
                })
                .collect(),
            progress_spans: spans,
            ..SessionStats::default()
        }
    }

    #[test]
    fn scale_windows_end_at_the_last_event_and_never_precede_the_first() {
        let stats = stats_with(&[(1_000, WorkKind::Model)], vec![], 10_000_000);
        assert_eq!(StatsScale::All.window(&stats), Some((1_000, 10_000_000)));
        assert_eq!(
            StatsScale::FiveMinutes.window(&stats),
            Some((10_000_000 - 300_000, 10_000_000))
        );
        let short = stats_with(&[(1_000, WorkKind::Model)], vec![], 60_000);
        assert_eq!(StatsScale::Day.window(&short), Some((1_000, 60_000)));
        assert_eq!(StatsScale::All.window(&SessionStats::default()), None);
    }

    #[test]
    fn ribbon_columns_take_the_dominant_state_and_flag_running_progress() {
        let stats = stats_with(
            &[
                (0, WorkKind::Model),
                (40, WorkKind::Shell),
                (80, WorkKind::Idle),
            ],
            vec![ProgressSpan {
                start_ms: 40,
                end_ms: Some(79),
                outcome: ProgressOutcome::Success,
            }],
            100,
        );
        let ribbon = work_ribbon(&stats, 0, 100, 10);
        let kinds: Vec<_> = ribbon.cells.iter().map(|cell| cell.unwrap().kind).collect();
        assert_eq!(kinds[0], WorkKind::Model);
        assert_eq!(kinds[5], WorkKind::Shell);
        assert_eq!(kinds[9], WorkKind::Idle);
        let running: Vec<_> = ribbon
            .cells
            .iter()
            .map(|cell| cell.unwrap().progress_running)
            .collect();
        assert_eq!(
            running,
            [false, false, false, false, true, true, true, true, false, false]
        );
        assert_eq!(ribbon.outcomes[7], Some(ProgressOutcome::Success));
        assert_eq!(ribbon.outcomes.iter().flatten().count(), 1);
        assert!(ribbon.has_running_progress());
        assert_eq!(
            ribbon.kinds_present(),
            vec![WorkKind::Idle, WorkKind::Model, WorkKind::Shell]
        );
    }

    #[test]
    fn failure_outranks_success_in_one_column_and_cleared_runs_show_no_icon() {
        let stats = stats_with(
            &[(0, WorkKind::Model)],
            vec![
                ProgressSpan {
                    start_ms: 0,
                    end_ms: Some(50),
                    outcome: ProgressOutcome::Success,
                },
                ProgressSpan {
                    start_ms: 0,
                    end_ms: Some(51),
                    outcome: ProgressOutcome::Failure,
                },
                ProgressSpan {
                    start_ms: 0,
                    end_ms: Some(90),
                    outcome: ProgressOutcome::Cleared,
                },
            ],
            100,
        );
        let ribbon = work_ribbon(&stats, 0, 100, 4);
        assert_eq!(ribbon.outcomes[2], Some(ProgressOutcome::Failure));
        assert!(ribbon.outcomes[3].is_none());
    }

    #[test]
    fn open_progress_runs_to_the_end_and_empty_input_is_safe() {
        let stats = stats_with(
            &[(0, WorkKind::Shell)],
            vec![ProgressSpan {
                start_ms: 50,
                end_ms: None,
                outcome: ProgressOutcome::Running,
            }],
            100,
        );
        let ribbon = work_ribbon(&stats, 0, 100, 4);
        assert!(!ribbon.cells[0].unwrap().progress_running);
        assert!(ribbon.cells[3].unwrap().progress_running);
        assert!(!work_ribbon(&SessionStats::default(), 0, 100, 4).has_data());
        assert!(work_ribbon(&stats, 100, 0, 4)
            .cells
            .iter()
            .all(Option::is_none));
        assert!(work_ribbon(&stats, 0, 100, 0).cells.is_empty());
    }
}
