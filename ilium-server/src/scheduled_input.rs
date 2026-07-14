//! Detached-server execution for durable terminal input countdowns.
//!
//! The pure schedule data lives in `ilium-core::Tree`, while this module owns
//! wall-clock reads, sleeping, PTY writes, and the single cancellable task that
//! coordinates every pane. One executor is preferable to one task per timer:
//! replacement and pane removal only wake a rescan and cannot leak stale tasks.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ilium_core::{NodeId, ScheduledPaneInput, Tree};
use ilium_ipc::ServerEvent;
use tokio::task::JoinHandle;

use crate::ipc::handlers::{broadcast_and_persist, write_key_input};
use crate::state::ServerState;

const MILLIS_PER_SECOND: u64 = 1000;

/// Converts a client-relative delay into the absolute deadline persisted in
/// the tree. Zero is rejected so accepting the dialog always produces a
/// visible countdown rather than an input race in the same event-loop tick.
pub(crate) fn deadline_from_delay(delay_seconds: u64) -> Result<u64, String> {
    if delay_seconds == 0 {
        return Err("scheduled input delay must be at least one second".to_string());
    }
    let delay_millis = delay_seconds
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or_else(|| "scheduled input delay is too large".to_string())?;
    current_unix_millis()?
        .checked_add(delay_millis)
        .ok_or_else(|| "scheduled input deadline is too large".to_string())
}

/// Starts the one task that sleeps until the nearest current deadline. Its
/// handle is owned and aborted by `ilium_server::run` during server shutdown.
pub(crate) fn spawn(state: Arc<ServerState>) -> JoinHandle<()> {
    tokio::spawn(run(state))
}

async fn run(state: Arc<ServerState>) {
    loop {
        let next_deadline = {
            let tree = state.tree.read().await;
            nearest_deadline(&tree)
        };
        let Some(deadline) = next_deadline else {
            state.scheduled_input_changed.notified().await;
            continue;
        };

        let now = match current_unix_millis() {
            Ok(now) => now,
            Err(message) => {
                tracing::error!("scheduled input clock failed: {message}");
                state.scheduled_input_changed.notified().await;
                continue;
            }
        };
        if deadline > now {
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(deadline - now)) => {}
                () = state.scheduled_input_changed.notified() => continue,
            }
        }
        execute_due_inputs(&state).await;
    }
}

/// Returns only the earliest deadline; the executor rescans after every wake
/// so schedule replacement and removal cannot leave an obsolete sleep active.
fn nearest_deadline(tree: &Tree) -> Option<u64> {
    tree.scheduled_pane_inputs()
        .map(|(_, scheduled_input)| scheduled_input.execute_at_unix_millis)
        .min()
}

async fn execute_due_inputs(state: &Arc<ServerState>) {
    let now = match current_unix_millis() {
        Ok(now) => now,
        Err(message) => {
            tracing::error!("scheduled input clock failed: {message}");
            return;
        }
    };
    let due_inputs: Vec<(NodeId, ScheduledPaneInput)> = {
        let tree = state.tree.read().await;
        tree.scheduled_pane_inputs()
            .filter(|(_, scheduled_input)| scheduled_input.execute_at_unix_millis <= now)
            .map(|(pane_id, scheduled_input)| (pane_id, scheduled_input.clone()))
            .collect()
    };
    let mut tree_changed = false;
    for (pane_id, scheduled_input) in due_inputs {
        if !is_current_schedule(state, pane_id, &scheduled_input).await {
            continue;
        }
        if let Err(message) = write_scheduled_input(state, pane_id, &scheduled_input).await {
            tracing::error!("scheduled input failed for pane {pane_id:?}: {message}");
            state.broadcast(ServerEvent::Error {
                message: format!("Scheduled input failed for pane {pane_id:?}: {message}"),
            });
        }
        let cleared = {
            let mut tree = state.tree.write().await;
            tree.clear_scheduled_pane_input_if_matches(pane_id, &scheduled_input)
                .unwrap_or(false)
        };
        tree_changed |= cleared;
    }
    if tree_changed {
        broadcast_and_persist(state).await;
    }
}

/// Rechecks immediately before writing because a replacement can arrive after
/// `execute_due_inputs` collected its snapshot but before the PTY lock is free.
async fn is_current_schedule(
    state: &ServerState,
    pane_id: NodeId,
    expected: &ScheduledPaneInput,
) -> bool {
    let tree = state.tree.read().await;
    let is_current = tree
        .scheduled_pane_inputs()
        .any(|(candidate_id, candidate)| candidate_id == pane_id && candidate == expected);
    is_current
}

/// Sends text and Enter as separate writes, exactly like direct typing. This
/// preserves shell-title tracking and the Enter-triggered detection refresh.
async fn write_scheduled_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    scheduled_input: &ScheduledPaneInput,
) -> Result<(), String> {
    if !scheduled_input.text.is_empty() {
        write_key_input(state, pane_id, scheduled_input.text.as_bytes()).await?;
    }
    if scheduled_input.send_enter {
        write_key_input(state, pane_id, b"\r").await?;
    }
    Ok(())
}

fn current_unix_millis() -> Result<u64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis();
    u64::try_from(millis).map_err(|_| "system clock is outside supported range".to_string())
}

#[cfg(test)]
mod tests {
    use ilium_core::{PaneContentKind, ScheduledPaneInput, ROOT_ID};

    use super::*;

    #[test]
    fn relative_deadline_rejects_zero_and_overflow() {
        assert_eq!(
            deadline_from_delay(0).unwrap_err(),
            "scheduled input delay must be at least one second"
        );
        assert_eq!(
            deadline_from_delay(u64::MAX).unwrap_err(),
            "scheduled input delay is too large"
        );
    }

    #[test]
    fn nearest_deadline_tracks_replacement_and_multiple_panes() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", PaneContentKind::Terminal)
            .unwrap();
        tree.schedule_pane_input(
            first,
            ScheduledPaneInput {
                execute_at_unix_millis: 5000,
                text: "first".to_string(),
                send_enter: false,
            },
        )
        .unwrap();
        tree.schedule_pane_input(
            second,
            ScheduledPaneInput {
                execute_at_unix_millis: 3000,
                text: String::new(),
                send_enter: true,
            },
        )
        .unwrap();

        assert_eq!(nearest_deadline(&tree), Some(3000));

        tree.schedule_pane_input(
            second,
            ScheduledPaneInput {
                execute_at_unix_millis: 7000,
                text: "replacement".to_string(),
                send_enter: true,
            },
        )
        .unwrap();
        assert_eq!(nearest_deadline(&tree), Some(5000));
    }
}
