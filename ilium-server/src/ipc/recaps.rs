//! Storage and fencing for client-produced pane recaps.
//!
//! The server never calls a model. A client sends `ApplyRecap`; this module
//! accepts it only for the pane's live session and only when no newer
//! generation is already stored, then keeps it, persists it with the session
//! snapshot, and tells every client through `RecapUpdated`.

use std::sync::Arc;

use ilium_core::{NodeId, Tree};
use ilium_ipc::{RecapTrigger, ServerEvent};

use crate::pane::PaneResource;
use crate::persistence::PersistedRecap;
use crate::state::ServerState;

/// Upper bounds on stored text, in characters. A client that exceeds them is
/// buggy; the request is dropped rather than truncated.
const MAXIMUM_SHORT_CHARACTERS: usize = 1_000;
const MAXIMUM_LONG_CHARACTERS: usize = 8_000;

/// Decides what to store for `incoming`, or `None` when it must be dropped.
/// Same session: an older generation is stale. A different session replaces
/// whatever the previous session left. A batch result keeps the stored long
/// recap only inside the same session.
pub(crate) fn fenced_recap(
    stored: Option<&PersistedRecap>,
    incoming: PersistedRecap,
    trigger: RecapTrigger,
) -> Option<PersistedRecap> {
    if incoming.short.trim().is_empty()
        || incoming.short.chars().count() > MAXIMUM_SHORT_CHARACTERS
        || incoming
            .long
            .as_ref()
            .is_some_and(|long| long.chars().count() > MAXIMUM_LONG_CHARACTERS)
    {
        return None;
    }
    let same_session = stored.filter(|stored| stored.session_id == incoming.session_id);
    if same_session.is_some_and(|stored| incoming.recap_generation < stored.recap_generation) {
        return None;
    }
    let mut accepted = incoming;
    match trigger {
        RecapTrigger::Batch => accepted.long = same_session.and_then(|stored| stored.long.clone()),
        RecapTrigger::Manual => {
            accepted.long.as_ref()?;
        }
    }
    Some(accepted)
}

pub(crate) async fn handle_apply_recap(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    session_id: String,
    recap_generation: u64,
    trigger: RecapTrigger,
    short: String,
    long: Option<String>,
) {
    {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        let live_session = runtime.session_id.as_deref().unwrap_or("");
        if tree.get(pane_id).is_none()
            || runtime.is_session_identity_invalidated
            || live_session != session_id
        {
            tracing::debug!(pane_id = pane_id.0, "stale recap rejected: session changed");
            return;
        }
    }
    let incoming = PersistedRecap {
        pane_id,
        session_id,
        recap_generation,
        short,
        long,
    };
    let accepted = {
        let mut recaps = state.recaps.write().await;
        let Some(accepted) = fenced_recap(recaps.get(&pane_id), incoming, trigger) else {
            tracing::debug!(pane_id = pane_id.0, "stale or invalid recap rejected");
            return;
        };
        recaps.insert(pane_id, accepted.clone());
        accepted
    };
    state.broadcast(updated_event(&accepted, trigger));
    state.request_snapshot_save();
}

fn updated_event(recap: &PersistedRecap, trigger: RecapTrigger) -> ServerEvent {
    ServerEvent::RecapUpdated {
        pane_id: recap.pane_id,
        session_id: recap.session_id.clone(),
        recap_generation: recap.recap_generation,
        trigger,
        short: recap.short.clone(),
        long: recap.long.clone(),
    }
}

/// Events for the exact tree already captured by the attach seed. Never
/// reacquire the source tree here: a queued writer could make a nested read
/// wait for the outer read guard that the caller still owns.
pub(crate) async fn replay_events(state: &ServerState, tree: &Tree) -> Vec<ServerEvent> {
    let recaps = state.recaps.read().await;
    let mut stored: Vec<&PersistedRecap> = recaps
        .values()
        .filter(|recap| tree.get(recap.pane_id).is_some())
        .collect();
    stored.sort_by_key(|recap| recap.pane_id.0);
    stored
        .into_iter()
        .map(|recap| updated_event(recap, RecapTrigger::Manual))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recap(session: &str, generation: u64, short: &str, long: Option<&str>) -> PersistedRecap {
        PersistedRecap {
            pane_id: NodeId(2),
            session_id: session.into(),
            recap_generation: generation,
            short: short.into(),
            long: long.map(str::to_owned),
        }
    }

    #[test]
    fn batch_replaces_short_and_keeps_stored_long() {
        let stored = recap("s", 3, "old short", Some("old long"));
        let accepted = fenced_recap(
            Some(&stored),
            recap("s", 4, "new short", None),
            RecapTrigger::Batch,
        )
        .unwrap();
        assert_eq!(accepted.short, "new short");
        assert_eq!(accepted.long.as_deref(), Some("old long"));
    }

    #[test]
    fn manual_replaces_both_and_requires_long() {
        let stored = recap("s", 3, "old short", Some("old long"));
        let accepted = fenced_recap(
            Some(&stored),
            recap("s", 4, "new short", Some("new long")),
            RecapTrigger::Manual,
        )
        .unwrap();
        assert_eq!(accepted.long.as_deref(), Some("new long"));
        assert!(fenced_recap(
            Some(&stored),
            recap("s", 5, "short only", None),
            RecapTrigger::Manual
        )
        .is_none());
    }

    #[test]
    fn older_generation_in_same_session_is_stale() {
        let stored = recap("s", 5, "kept", Some("kept long"));
        assert!(fenced_recap(
            Some(&stored),
            recap("s", 4, "late", None),
            RecapTrigger::Batch
        )
        .is_none());
        // Equal generation is accepted: the same job may report twice.
        assert!(fenced_recap(
            Some(&stored),
            recap("s", 5, "again", None),
            RecapTrigger::Batch
        )
        .is_some());
    }

    #[test]
    fn new_session_replaces_old_and_drops_its_long_recap() {
        let stored = recap("old-session", 99, "old", Some("old long"));
        let accepted = fenced_recap(
            Some(&stored),
            recap("new-session", 1, "fresh", None),
            RecapTrigger::Batch,
        )
        .unwrap();
        assert_eq!(accepted.long, None);
        assert_eq!(accepted.session_id, "new-session");
    }

    #[test]
    fn empty_and_oversized_text_is_rejected() {
        assert!(fenced_recap(None, recap("s", 1, "  ", None), RecapTrigger::Batch).is_none());
        let huge = "x".repeat(MAXIMUM_SHORT_CHARACTERS + 1);
        assert!(fenced_recap(None, recap("s", 1, &huge, None), RecapTrigger::Batch).is_none());
    }
}
