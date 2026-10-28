//! Client-side recap bookkeeping: the generation fence, the one-in-flight
//! dedupe, and the recaps the server last reported.
//!
//! The server only stores recaps. This state decides which pane may start a
//! manual recap, stamps each job with a generation, and shows the stored text.

use std::collections::{HashMap, HashSet};

use ilium_core::NodeId;
use ilium_ipc::RecapTrigger;

/// A recap as last reported by the server for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecap {
    pub session_id: String,
    pub recap_generation: u64,
    pub trigger: RecapTrigger,
    pub short: String,
    pub long: Option<String>,
}

#[derive(Debug, Default)]
pub struct RecapState {
    generations: HashMap<NodeId, u64>,
    in_flight: HashSet<NodeId>,
    stored: HashMap<NodeId, StoredRecap>,
}

impl RecapState {
    /// Stamps a new job for `pane_id`; later stamps always compare newer.
    pub fn next_generation(&mut self, pane_id: NodeId) -> u64 {
        let generation = self.generations.entry(pane_id).or_insert(0);
        *generation += 1;
        *generation
    }

    /// Starts a manual recap unless one is already running for the pane.
    /// Returns the job's generation, or `None` when deduplicated.
    pub fn begin_manual(&mut self, pane_id: NodeId) -> Option<u64> {
        if !self.in_flight.insert(pane_id) {
            return None;
        }
        Some(self.next_generation(pane_id))
    }

    pub fn finish_manual(&mut self, pane_id: NodeId) {
        self.in_flight.remove(&pane_id);
    }

    pub fn is_in_flight(&self, pane_id: NodeId) -> bool {
        self.in_flight.contains(&pane_id)
    }

    /// Records the server's report. The local generation counter never falls
    /// behind the server, so a restarted client cannot stamp stale jobs.
    pub fn observe_update(&mut self, pane_id: NodeId, recap: StoredRecap) {
        let generation = self.generations.entry(pane_id).or_insert(0);
        *generation = (*generation).max(recap.recap_generation);
        self.stored.insert(pane_id, recap);
    }

    /// The stored recap, only while it belongs to the pane's current session.
    pub fn recap_for(&self, pane_id: NodeId, session_id: &str) -> Option<&StoredRecap> {
        self.stored
            .get(&pane_id)
            .filter(|recap| recap.session_id == session_id)
    }

    /// Forgets everything about a pane that was closed.
    pub fn forget(&mut self, pane_id: NodeId) {
        self.generations.remove(&pane_id);
        self.in_flight.remove(&pane_id);
        self.stored.remove(&pane_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(session: &str, generation: u64) -> StoredRecap {
        StoredRecap {
            session_id: session.into(),
            recap_generation: generation,
            trigger: RecapTrigger::Manual,
            short: "short".into(),
            long: Some("long".into()),
        }
    }

    #[test]
    fn manual_recaps_are_deduplicated_per_pane() {
        let mut state = RecapState::default();
        assert_eq!(state.begin_manual(NodeId(2)), Some(1));
        assert_eq!(state.begin_manual(NodeId(2)), None);
        assert_eq!(state.begin_manual(NodeId(3)), Some(1));
        state.finish_manual(NodeId(2));
        assert_eq!(state.begin_manual(NodeId(2)), Some(2));
    }

    #[test]
    fn generation_never_falls_behind_the_server() {
        let mut state = RecapState::default();
        state.observe_update(NodeId(2), stored("s", 41));
        assert_eq!(state.next_generation(NodeId(2)), 42);
    }

    #[test]
    fn stored_recap_is_hidden_after_the_session_changes() {
        let mut state = RecapState::default();
        state.observe_update(NodeId(2), stored("old", 1));
        assert!(state.recap_for(NodeId(2), "old").is_some());
        assert!(state.recap_for(NodeId(2), "new").is_none());
        state.forget(NodeId(2));
        assert!(state.recap_for(NodeId(2), "old").is_none());
    }
}
