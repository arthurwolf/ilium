//! Client-only visibility of retained progress results. Hiding a footer must
//! never clear server state, acknowledge an outcome, or affect PTY delivery.

use ilium_core::PaneProgress;

pub fn footer_is_visible(
    progress: Option<&PaneProgress>,
    hide_after_seconds: u32,
    now_unix_millis: u64,
) -> bool {
    let Some(progress) = progress else {
        return false;
    };
    !progress.is_terminal()
        || hide_after_seconds == 0
        || now_unix_millis.saturating_sub(progress.last_observed_unix_millis)
            < u64::from(hide_after_seconds) * 1000
}

/// The monitors whose footer rows are still shown, in registration order.
pub fn visible_monitors(
    monitors: &[PaneProgress],
    hide_after_seconds: u32,
    now_unix_millis: u64,
) -> Vec<&PaneProgress> {
    monitors
        .iter()
        .filter(|progress| footer_is_visible(Some(progress), hide_after_seconds, now_unix_millis))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{ProgressTaskReport, ProgressTaskStatus};

    fn report(status: ProgressTaskStatus) -> PaneProgress {
        PaneProgress::new(
            17,
            ProgressTaskReport {
                job_id: "expiry-regression".into(),
                status,
                percent: 100.0,
                message: "retained result".into(),
                details: String::new(),
                error: (status == ProgressTaskStatus::Error).then(|| "task failed".into()),
            },
            1000,
        )
        .unwrap()
    }

    #[test]
    fn completed_footer_expires_at_configured_deadline_without_mutation() {
        let progress = report(ProgressTaskStatus::Done);
        let original = progress.clone();
        assert!(footer_is_visible(Some(&progress), 60, 60_999));
        assert!(!footer_is_visible(Some(&progress), 60, 61_000));
        assert!(!footer_is_visible(Some(&progress), 60, 100_000));
        assert_eq!(progress, original);
    }

    #[test]
    fn zero_duration_keeps_completed_results_visible() {
        assert!(footer_is_visible(
            Some(&report(ProgressTaskStatus::Done)),
            0,
            u64::MAX
        ));
    }

    #[test]
    fn running_at_one_hundred_percent_is_not_completion() {
        assert!(footer_is_visible(
            Some(&report(ProgressTaskStatus::Running)),
            60,
            u64::MAX
        ));
        let mut unknown_outcome = report(ProgressTaskStatus::Running);
        unknown_outcome.monitor_health = ilium_core::ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "probe unavailable; task outcome unknown".into(),
        };
        assert!(footer_is_visible(Some(&unknown_outcome), 60, u64::MAX));
    }

    #[test]
    fn task_errors_expire_without_removing_the_error_record() {
        let progress = report(ProgressTaskStatus::Error);
        assert!(!footer_is_visible(Some(&progress), 2, 3000));
        assert_eq!(progress.report.error.as_deref(), Some("task failed"));
    }

    #[test]
    fn absent_report_is_hidden_and_future_timestamp_does_not_underflow() {
        assert!(!footer_is_visible(None, 60, 0));
        assert!(footer_is_visible(
            Some(&report(ProgressTaskStatus::Done)),
            60,
            0
        ));
    }
}
