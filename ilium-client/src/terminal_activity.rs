//! Client-local activity windows for ordinary terminal panes.
//!
//! The PTY stream itself is already event-driven, so terminal activity does
//! not need a polling task. Live visible-text changes and client-queued input
//! refresh this tracker; input contents are never retained as evidence.

use std::collections::{HashMap, HashSet};
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

/// One bounded row sample from the live screen at the accepted update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VisibleRowEvidence {
    pub(crate) row_number: u16,
    pub(crate) text: String,
    pub(crate) blank: bool,
    pub(crate) truncated: bool,
}

/// Evidence captured when the parsed visible-text fingerprint changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VisibleTextEvidence {
    pub(crate) first_sequence: u64,
    pub(crate) sequence: u64,
    pub(crate) changed_rows: Option<usize>,
    pub(crate) rows: Vec<VisibleRowEvidence>,
}

/// The latest local cause for a plain terminal's activity indicator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TerminalActivityCause {
    VisibleTextChanged(VisibleTextEvidence),
    KeyInputQueued { byte_count: usize },
    TerminalTextQueued,
}

#[derive(Debug)]
struct TerminalActivityEntry {
    elapsed_ms: u128,
    cause: TerminalActivityCause,
}

/// A consistent phase and cause read from one tracker entry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TerminalActivitySnapshot<'a> {
    pub(crate) phase: TerminalActivityPhase,
    pub(crate) age_ms: u128,
    pub(crate) cause: &'a TerminalActivityCause,
}

/// Last activity edge for each currently animated plain terminal.
#[derive(Debug, Default)]
pub struct TerminalActivityTracker {
    last_activity_ms: HashMap<NodeId, TerminalActivityEntry>,
}

impl TerminalActivityTracker {
    /// Starts or refreshes a timing-only test entry.
    #[cfg(test)]
    pub(crate) fn record(&mut self, pane_id: NodeId, elapsed_ms: u128) {
        self.record_with_cause(pane_id, elapsed_ms, TerminalActivityCause::TerminalTextQueued);
    }

    /// Starts or refreshes one pane's window and replaces its prior cause.
    pub(crate) fn record_with_cause(
        &mut self,
        pane_id: NodeId,
        elapsed_ms: u128,
        cause: TerminalActivityCause,
    ) {
        self.last_activity_ms
            .insert(pane_id, TerminalActivityEntry { elapsed_ms, cause });
    }

    /// Selects the pane's animation phase from its last activity edge.
    pub fn phase(&self, pane_id: NodeId, elapsed_ms: u128) -> Option<TerminalActivityPhase> {
        self.snapshot(pane_id, elapsed_ms)
            .map(|snapshot| snapshot.phase)
    }

    /// Reads the phase and latest cause from the same unexpired observation.
    pub(crate) fn snapshot(
        &self,
        pane_id: NodeId,
        elapsed_ms: u128,
    ) -> Option<TerminalActivitySnapshot<'_>> {
        let entry = self.last_activity_ms.get(&pane_id)?;
        let age_ms = elapsed_ms.saturating_sub(entry.elapsed_ms);
        if age_ms >= TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS {
            return None;
        }

        let phase = if age_ms < TERMINAL_ACTIVITY_FAST_WINDOW_MS {
            TerminalActivityPhase::Fast
        } else {
            TerminalActivityPhase::Slow
        };

        Some(TerminalActivitySnapshot {
            phase,
            age_ms,
            cause: &entry.cause,
        })
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
            .filter_map(|entry| {
                let age_ms = elapsed_ms.saturating_sub(entry.elapsed_ms);
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

    /// Time until the earliest fast indicator changes to its slow phase.
    /// The renderer's glyph clock is global, but this per-pane boundary is
    /// independent and can arrive before the next 90 ms glyph frame.
    pub fn next_fast_expiry_delay(&self, elapsed_ms: u128) -> Option<Duration> {
        self.last_activity_ms
            .values()
            .filter_map(|entry| {
                let age_ms = elapsed_ms.saturating_sub(entry.elapsed_ms);
                if age_ms >= TERMINAL_ACTIVITY_FAST_WINDOW_MS {
                    return None;
                }

                let remaining_ms = (TERMINAL_ACTIVITY_FAST_WINDOW_MS - age_ms) as u64;
                Some(Duration::from_millis(remaining_ms))
            })
            .min()
    }

    /// Drops expired windows and reports whether visible activity state ended.
    pub fn prune_expired(&mut self, elapsed_ms: u128) -> bool {
        let previous_count = self.last_activity_ms.len();

        self.last_activity_ms.retain(|_pane_id, entry| {
            elapsed_ms.saturating_sub(entry.elapsed_ms) < TERMINAL_ACTIVITY_VISIBLE_WINDOW_MS
        });

        self.last_activity_ms.len() != previous_count
    }

    /// Drops observations for panes absent from the authoritative tree snapshot.
    pub(crate) fn retain_panes(&mut self, live_pane_ids: &HashSet<NodeId>) -> bool {
        let previous_count = self.last_activity_ms.len();
        self.last_activity_ms
            .retain(|pane_id, _| live_pane_ids.contains(pane_id));
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

    #[test]
    fn fast_expiry_delay_tracks_the_earliest_phase_change() {
        let mut tracker = TerminalActivityTracker::default();
        tracker.record(NodeId(1), 0);
        tracker.record(NodeId(2), 250);

        assert_eq!(
            tracker.next_fast_expiry_delay(4_900),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            tracker.next_fast_expiry_delay(5_000),
            Some(Duration::from_millis(250))
        );
        assert_eq!(tracker.next_fast_expiry_delay(5_250), None);
    }

    #[test]
    fn latest_activity_cause_replaces_prior_output_evidence() {
        let pane_id = NodeId(41);
        let mut tracker = TerminalActivityTracker::default();
        let output = VisibleTextEvidence {
            first_sequence: 1,
            sequence: 1,
            changed_rows: Some(1),
            rows: vec![VisibleRowEvidence {
                row_number: 1,
                text: "OLD_OUTPUT".to_owned(),
                blank: false,
                truncated: false,
            }],
        };

        tracker.record_with_cause(
            pane_id,
            100,
            TerminalActivityCause::VisibleTextChanged(output),
        );
        tracker.record_with_cause(
            pane_id,
            100,
            TerminalActivityCause::KeyInputQueued { byte_count: 3 },
        );

        let snapshot = tracker.snapshot(pane_id, 100).expect("latest observation");
        assert_eq!(snapshot.phase, TerminalActivityPhase::Fast);
        assert_eq!(snapshot.age_ms, 0);
        assert!(matches!(
            snapshot.cause,
            TerminalActivityCause::KeyInputQueued { byte_count: 3 }
        ));
        assert_eq!(tracker.last_activity_ms.len(), 1);
    }

    #[test]
    fn snapshot_removal_drops_activity_evidence_for_deleted_panes() {
        let pane_id = NodeId(45);
        let mut tracker = TerminalActivityTracker::default();
        tracker.record_with_cause(
            pane_id,
            100,
            TerminalActivityCause::KeyInputQueued { byte_count: 2 },
        );

        assert!(tracker.retain_panes(&HashSet::new()));
        assert!(tracker.snapshot(pane_id, 101).is_none());
        assert!(!tracker.retain_panes(&HashSet::new()));
    }
}
