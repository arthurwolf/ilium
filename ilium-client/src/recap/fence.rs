//! Stale fencing and the in-process recap cache.
//!
//! A recap is computed against one pane and one agent session. Inference
//! takes seconds, so the pane may have been closed or re-attached to another
//! session by the time text arrives. Text is kept only when the pane still
//! holds the session it was computed for. The cache is per process: recaps are
//! never written to disk or shared between viewers.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::model::PaneRecap;

/// Identity a recap was computed against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFence {
    pub pane_id: String,
    /// `None` for shell panes, which have no agent session.
    pub session_id: Option<String>,
}

/// True when the captured identity still matches the live pane. A pane that
/// has disappeared (`latest` is `None`) never matches.
pub fn fence_still_current(captured: &PaneFence, latest: Option<&PaneFence>) -> bool {
    latest.is_some_and(|latest| latest == captured)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub pane_id: String,
    pub evidence_fingerprint: u64,
}

/// Fingerprint of the exact evidence text sent to the model. A changed
/// transcript or screen produces a new key, so cached text never outlives its
/// evidence.
pub fn evidence_fingerprint(evidence_json: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    evidence_json.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Default)]
pub struct RecapCache {
    entries: HashMap<CacheKey, PaneRecap>,
}

impl RecapCache {
    pub fn get(&self, key: &CacheKey) -> Option<&PaneRecap> {
        self.entries.get(key)
    }

    /// Stores only summarized results. Partial, failed, stale and cancelled
    /// results are never reused.
    pub fn store(&mut self, key: CacheKey, recap: PaneRecap) {
        if recap.state == super::model::RecapPaneState::Summarized {
            self.entries.insert(key, recap);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_rejects_reattached_session_and_vanished_pane() {
        let captured = PaneFence {
            pane_id: "p1".into(),
            session_id: Some("s1".into()),
        };
        let same = captured.clone();
        let reattached = PaneFence {
            pane_id: "p1".into(),
            session_id: Some("s2".into()),
        };
        assert!(fence_still_current(&captured, Some(&same)));
        assert!(!fence_still_current(&captured, Some(&reattached)));
        assert!(!fence_still_current(&captured, None));
    }

    #[test]
    fn fingerprint_changes_with_evidence() {
        assert_eq!(evidence_fingerprint("a"), evidence_fingerprint("a"));
        assert_ne!(evidence_fingerprint("a"), evidence_fingerprint("b"));
    }
}
