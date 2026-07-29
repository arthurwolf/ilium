//! Client-local activity windows for ordinary terminal panes.
//!
//! The PTY stream itself is already event-driven, so terminal activity does
//! not need a polling task. `TerminalView` reports a live visible-text change
//! while parsing output, input dispatch reports accepted key bytes, and this
//! tracker keeps only the presentation window and phase used by the sidebar.

use std::collections::HashMap;
use std::time::Duration;

use ilium_core::NodeId;

/// Activity stays on the existing fast Angular cadence for this long.
pub const TERMINAL_ACTIVITY_FAST_WINDOW_MS: u128 = 5_000;

/// Activity remains visible at the slower cadence until this age.
pub const TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS: u128 = 60_000;

/// Existing Angular-loop frame duration during the recent-activity phase.
pub const TERMINAL_ACTIVITY_FAST_FRAME_MS: u64 = 90;

/// Angular-loop frame duration after activity is older than five seconds.
pub const TERMINAL_ACTIVITY_SLOW_FRAME_MS: u64 = 500;

/// The animation cadence selected from the age of the last activity edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalActivityPhase {
    Fast,
    Slow,
}

/// Last activity edge for each currently animated plain terminal.
#[derive(Debug, Default)]
pub struct TerminalActivityTracker {
    last_activity_ms: HashMap<NodeId, u128>,
}

impl TerminalActivityTracker {
    /// Starts or refreshes one pane's activity window.
    pub fn record(&mut self, pane_id: NodeId, elapsed_ms: u128) {
        self.last_activity_ms.insert(pane_id, elapsed_ms);
    }

    /// Selects the pane's animation phase from its last activity edge.
    pub fn phase(&self, pane_id: NodeId, elapsed_ms: u128) -> Option<TerminalActivityPhase> {
        let age_ms = elapsed_ms.saturating_sub(*self.last_activity_ms.get(&pane_id)?);

        if age_ms < TERMINAL_ACTIVITY_FAST_WINDOW_MS {
            return Some(TerminalActivityPhase::Fast);
        }
        if age_ms < TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS {
            return Some(TerminalActivityPhase::Slow);
        }

        None
    }

    /// Whether at least one terminal needs the existing fast redraw cadence.
    pub fn has_fast_activity(&self, elapsed_ms: u128) -> bool {
        self.last_activity_ms
            .keys()
            .copied()
            .any(|pane_id| self.phase(pane_id, elapsed_ms) == Some(TerminalActivityPhase::Fast))
    }

    /// Whether at least one terminal needs the half-second redraw cadence.
    pub fn has_slow_activity(&self, elapsed_ms: u128) -> bool {
        self.last_activity_ms
            .keys()
            .copied()
            .any(|pane_id| self.phase(pane_id, elapsed_ms) == Some(TerminalActivityPhase::Slow))
    }

    /// Whether any terminal activity indicator remains visible.
    pub fn has_visible_activity(&self, elapsed_ms: u128) -> bool {
        self.last_activity_ms
            .keys()
            .copied()
            .any(|pane_id| self.phase(pane_id, elapsed_ms).is_some())
    }

    /// Time until the earliest currently-slow pane must become idle.
    pub fn next_slow_expiry_delay(&self, elapsed_ms: u128) -> Option<Duration> {
        self.last_activity_ms
            .values()
            .filter_map(|last_activity_ms| {
                let age_ms = elapsed_ms.saturating_sub(*last_activity_ms);
                if !(TERMINAL_ACTIVITY_FAST_WINDOW_MS..TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS)
                    .contains(&age_ms)
                {
                    return None;
                }

                // A slow phase is at most 55 seconds long, so this
                // conversion cannot truncate on any supported target.
                let remaining_ms = (TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS - age_ms) as u64;
                Some(Duration::from_millis(remaining_ms))
            })
            .min()
    }

    /// Drops expired windows and reports whether visible activity state ended.
    pub fn prune_expired(&mut self, elapsed_ms: u128) -> bool {
        let previous_count = self.last_activity_ms.len();

        self.last_activity_ms.retain(|_pane_id, last_activity_ms| {
            elapsed_ms.saturating_sub(*last_activity_ms) < TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS
        });

        self.last_activity_ms.len() != previous_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_uses_fast_then_slow_phases_and_expires_at_sixty_seconds() {
        let pane_id = NodeId(7);
        let mut tracker = TerminalActivityTracker::default();

        tracker.record(pane_id, 2_000);

        assert_eq!(
            tracker.phase(pane_id, 6_999),
            Some(TerminalActivityPhase::Fast)
        );
        assert_eq!(
            tracker.phase(pane_id, 7_000),
            Some(TerminalActivityPhase::Slow)
        );
        assert_eq!(
            tracker.phase(pane_id, 61_999),
            Some(TerminalActivityPhase::Slow)
        );
        assert_eq!(tracker.phase(pane_id, 62_000), None);
        assert!(tracker.prune_expired(62_000));
        assert!(!tracker.has_visible_activity(62_000));
    }

    #[test]
    fn a_new_edge_refreshes_the_existing_window() {
        let pane_id = NodeId(11);
        let mut tracker = TerminalActivityTracker::default();

        tracker.record(pane_id, 100);
        assert_eq!(
            tracker.phase(pane_id, 5_100),
            Some(TerminalActivityPhase::Slow)
        );

        tracker.record(pane_id, 5_100);

        assert_eq!(
            tracker.phase(pane_id, 10_099),
            Some(TerminalActivityPhase::Fast)
        );
        assert_eq!(
            tracker.phase(pane_id, 10_100),
            Some(TerminalActivityPhase::Slow)
        );
    }

    #[test]
    fn phase_queries_distinguish_fast_slow_and_expired_entries() {
        let mut tracker = TerminalActivityTracker::default();
        tracker.record(NodeId(1), 9_000);
        tracker.record(NodeId(2), 4_000);
        tracker.record(NodeId(3), 0);

        assert!(tracker.has_fast_activity(10_000));
        assert!(tracker.has_slow_activity(10_000));
        assert!(tracker.has_visible_activity(10_000));

        assert!(!tracker.has_fast_activity(70_000));
        assert!(!tracker.has_slow_activity(70_000));
        assert!(!tracker.has_visible_activity(70_000));
    }

    #[test]
    fn slow_expiry_delay_tracks_the_earliest_exact_cutoff() {
        let mut tracker = TerminalActivityTracker::default();
        tracker.record(NodeId(1), 0);
        tracker.record(NodeId(2), 250);

        assert_eq!(
            tracker.next_slow_expiry_delay(59_900),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            tracker.next_slow_expiry_delay(60_000),
            Some(Duration::from_millis(250))
        );
        assert_eq!(tracker.next_slow_expiry_delay(60_250), None);
    }
}
