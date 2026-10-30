//! Server-owned recent activity for tree visibility decisions.
//!
//! Unlike the client animation, this tracker also observes output from panes
//! without terminal subscribers. The clock is monotonic so wall-clock changes
//! cannot make a previously idle pane appear newly active.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ilium_core::NodeId;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Observation {
    pub(crate) observed_at: Instant,
    pub(crate) revision: u64,
    last_published_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct RecentActivity {
    observations: HashMap<NodeId, Observation>,
}

impl RecentActivity {
    /// Returns whether clients should receive an update. Limit output bursts
    /// to one event per pane per second while retaining every latest edge.
    pub(crate) fn record(&mut self, node_id: NodeId, revision: u64, now: Instant) -> bool {
        let observation = match self.observations.entry(node_id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Observation {
                    observed_at: now,
                    revision,
                    last_published_at: now,
                });
                return true;
            }
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        };
        // Concurrent output and detection may complete out of order. Never
        // let an older revision move the observation backward.
        if revision > observation.revision {
            observation.revision = revision;
            observation.observed_at = now.max(observation.observed_at);
            if now.saturating_duration_since(observation.last_published_at)
                >= Duration::from_secs(1)
            {
                observation.last_published_at = now;
                return true;
            }
        }
        false
    }

    pub(crate) fn snapshot(&self) -> &HashMap<NodeId, Observation> {
        &self.observations
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(NodeId) -> bool) {
        self.observations.retain(|node_id, _| keep(*node_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn older_revisions_cannot_replace_newer_observations() {
        let now = Instant::now();
        let mut activity = RecentActivity::default();
        activity.record(NodeId(7), 2, now);
        activity.record(NodeId(7), 1, now + Duration::from_secs(1));
        let observation = activity.snapshot()[&NodeId(7)];
        assert_eq!(observation.revision, 2);
        assert_eq!(observation.observed_at, now);
        activity.record(NodeId(7), 3, now + Duration::from_secs(2));
        assert_eq!(
            activity.snapshot()[&NodeId(7)].observed_at,
            now + Duration::from_secs(2)
        );
    }

    #[test]
    fn closed_panes_can_be_pruned() {
        let now = Instant::now();
        let mut activity = RecentActivity::default();
        activity.record(NodeId(1), 1, now);
        activity.record(NodeId(2), 1, now);
        activity.retain(|node_id| node_id == NodeId(2));
        assert!(!activity.snapshot().contains_key(&NodeId(1)));
        assert!(activity.snapshot().contains_key(&NodeId(2)));
    }

    #[test]
    fn output_bursts_are_coalesced_without_losing_the_latest_observation() {
        let now = Instant::now();
        let mut activity = RecentActivity::default();
        assert!(activity.record(NodeId(3), 1, now));
        assert!(!activity.record(NodeId(3), 2, now + Duration::from_millis(900)));
        assert_eq!(
            activity.snapshot()[&NodeId(3)].observed_at,
            now + Duration::from_millis(900)
        );
        assert!(activity.record(NodeId(3), 3, now + Duration::from_secs(1)));
    }
}
