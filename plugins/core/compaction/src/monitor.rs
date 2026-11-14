//! The automatic trigger of remote compaction: decides when a pane's context fill calls for a compaction (ported
//! from the kernel `RemoteCompactionMonitor`; times are milliseconds of the host's monotonic clock).

use std::collections::HashMap;

/// Consecutive failures after which a pane is left alone until its context drops back below the threshold.
pub const MAX_CONSECUTIVE_FAILURES: u8 = 3;

/// Hysteresis: failures and cooldown reset once the context fill is this many percentage points under the
/// threshold (the agent compacted itself, or the user cleared the conversation).
const RESET_MARGIN_PERCENT: f64 = 5.0;

/// Per-pane state of the automatic trigger.
#[derive(Debug, Clone, Copy, Default)]
struct PaneMonitorState {
    last_attempt_millis: Option<u64>,
    consecutive_failures: u8,
}

/// Decides when a pane's context fill calls for an automatic compaction.
#[derive(Debug, Default)]
pub struct Monitor {
    panes: HashMap<u64, PaneMonitorState>,
}

impl Monitor {
    /// True when `pane` at `fill_percent` should be compacted now.
    pub fn should_trigger(
        &mut self,
        pane: u64,
        fill_percent: f64,
        threshold_percent: f64,
        cooldown_millis: u64,
        now_millis: u64,
    ) -> bool {
        let state = self.panes.entry(pane).or_default();
        if fill_percent < threshold_percent - RESET_MARGIN_PERCENT {
            *state = PaneMonitorState::default();
            return false;
        }
        if fill_percent < threshold_percent {
            return false;
        }
        if state.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return false;
        }
        state
            .last_attempt_millis
            .is_none_or(|attempt| now_millis.saturating_sub(attempt) >= cooldown_millis)
    }

    pub fn record_attempt(&mut self, pane: u64, now_millis: u64) {
        self.panes.entry(pane).or_default().last_attempt_millis = Some(now_millis);
    }

    pub fn record_failure(&mut self, pane: u64) {
        let state = self.panes.entry(pane).or_default();
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    }

    pub fn record_success(&mut self, pane: u64) {
        self.panes.entry(pane).or_default().consecutive_failures = 0;
    }

    pub fn forget(&mut self, pane: u64) {
        self.panes.remove(&pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: u64 = 9;

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_triggers_at_the_threshold_only
    #[test]
    fn monitor_triggers_at_the_threshold_only() {
        let mut monitor = Monitor::default();
        assert!(!monitor.should_trigger(PANE, 64.9, 65.0, 600_000, 0));
        assert!(monitor.should_trigger(PANE, 65.0, 65.0, 600_000, 0));
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_waits_out_the_cooldown_after_an_attempt
    #[test]
    fn monitor_waits_out_the_cooldown_after_an_attempt() {
        let mut monitor = Monitor::default();
        monitor.record_attempt(PANE, 1_000);
        assert!(!monitor.should_trigger(PANE, 80.0, 65.0, 600_000, 1_000 + 599_000));
        assert!(monitor.should_trigger(PANE, 80.0, 65.0, 600_000, 1_000 + 600_000));
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_stops_after_repeated_failures_until_the_context_drops
    #[test]
    fn monitor_stops_after_repeated_failures_until_the_context_drops() {
        let mut monitor = Monitor::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            monitor.record_failure(PANE);
        }
        assert!(!monitor.should_trigger(PANE, 90.0, 65.0, 0, 0));
        // Within the hysteresis margin nothing resets.
        assert!(!monitor.should_trigger(PANE, 61.0, 65.0, 0, 0));
        assert!(!monitor.should_trigger(PANE, 90.0, 65.0, 0, 0));
        // A real drop (the agent compacted itself) re-arms the pane.
        assert!(!monitor.should_trigger(PANE, 10.0, 65.0, 0, 0));
        assert!(monitor.should_trigger(PANE, 90.0, 65.0, 0, 0));
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_success_clears_the_failure_streak
    #[test]
    fn a_success_clears_the_failure_streak() {
        let mut monitor = Monitor::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            monitor.record_failure(PANE);
        }
        monitor.record_success(PANE);
        assert!(monitor.should_trigger(PANE, 90.0, 65.0, 0, 0));
    }

    #[test]
    fn a_forgotten_pane_starts_fresh() {
        let mut monitor = Monitor::default();
        monitor.record_attempt(PANE, 0);
        monitor.forget(PANE);
        assert!(monitor.should_trigger(PANE, 70.0, 65.0, 600_000, 1));
    }
}
