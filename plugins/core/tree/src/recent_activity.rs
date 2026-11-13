//! Recent output activity of panes, for the tree's "hide inactive" decisions (port of
//! `ilium-server/src/recent_activity.rs`).
//!
//! The clock is a monotonic millisecond count handed in by the caller (the host clock import), so
//! a wall-clock change cannot make an idle pane look newly active and the rules stay pure.

use std::collections::HashMap;

/// Output bursts are published at most once per pane per this many milliseconds.
pub const PUBLISH_INTERVAL_MS: u64 = 1_000;

/// What is known about the latest output of one pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    /// Monotonic milliseconds of the newest output seen.
    pub observed_at: u64,
    /// The newest output revision seen.
    pub revision: u64,
    last_published_at: u64,
}

/// The per-pane observations.
#[derive(Debug, Default)]
pub struct RecentActivity {
    observations: HashMap<u64, Observation>,
}

impl RecentActivity {
    /// Records an output revision of a pane. Returns whether clients should receive an update:
    /// the first sight of a pane, and at most one update per pane per second after that, while
    /// the latest edge is always kept.
    pub fn record(&mut self, node: u64, revision: u64, now: u64) -> bool {
        let Some(observation) = self.observations.get_mut(&node) else {
            self.observations.insert(
                node,
                Observation {
                    observed_at: now,
                    revision,
                    last_published_at: now,
                },
            );
            return true;
        };
        // Output and detection may complete out of order: an older revision never moves the
        // observation backward.
        if revision > observation.revision {
            observation.revision = revision;
            observation.observed_at = now.max(observation.observed_at);
            if now.saturating_sub(observation.last_published_at) >= PUBLISH_INTERVAL_MS {
                observation.last_published_at = now;
                return true;
            }
        }
        false
    }

    /// The observation of a pane, if any.
    pub fn observation(&self, node: u64) -> Option<&Observation> {
        self.observations.get(&node)
    }

    /// Forgets every pane for which `keep` is false.
    pub fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.observations.retain(|node, _| keep(*node));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_revisions_cannot_replace_newer_observations() {
        let mut activity = RecentActivity::default();
        activity.record(7, 2, 0);
        activity.record(7, 1, 1_000);
        let observation = activity.observation(7).copied().unwrap();
        assert_eq!(observation.revision, 2);
        assert_eq!(observation.observed_at, 0);
        activity.record(7, 3, 2_000);
        assert_eq!(activity.observation(7).unwrap().observed_at, 2_000);
    }

    #[test]
    fn closed_panes_can_be_pruned() {
        let mut activity = RecentActivity::default();
        activity.record(1, 1, 0);
        activity.record(2, 1, 0);
        activity.retain(|node| node == 2);
        assert!(activity.observation(1).is_none());
        assert!(activity.observation(2).is_some());
    }

    #[test]
    fn output_bursts_are_coalesced_without_losing_the_latest_observation() {
        let mut activity = RecentActivity::default();
        assert!(activity.record(3, 1, 0));
        assert!(!activity.record(3, 2, 900));
        assert_eq!(activity.observation(3).unwrap().observed_at, 900);
        assert!(activity.record(3, 3, 1_000));
    }
}
