//! Completion-driven queued-prompt delivery.
//!
//! The detection loop alone decides when an agent has finished. This module
//! then writes exactly one FIFO head plus Enter, and advances that head only
//! after the PTY accepts the complete payload.

use ilium_agent_debug::{
    AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField, AgentDebugSeverity,
    AgentDebugSource,
};
use ilium_core::{NodeId, QueuedPrompt};
use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{broadcast_and_persist, write_key_input};
use crate::state::ServerState;

/// Delivers one currently queued prompt after a verified agent completion.
/// A queue mutation and the write/ack pair share one transaction, so Clear
/// cannot race a just-completed agent into sending a prompt the user removed.
pub(crate) async fn deliver_next_after_completion(state: &ServerState, pane_id: NodeId) {
    // Held only across peek -> write -> acknowledge, matching the drop point
    // `handle_enqueue_prompt`/`handle_clear_prompt_queue` use in
    // `ipc/handlers.rs`. The slow follow-up work below (PTY write aside,
    // which must stay inside the transaction) -- broadcasting the snapshot
    // to every client and recording a debug event -- must not serialize
    // every other pane's queue mutations behind it.
    let _transaction = state.prompt_queue_transaction.lock().await;
    let prompt = {
        let tree = state.tree.read().await;
        match tree.next_queued_prompt(pane_id) {
            Ok(prompt) => prompt.cloned(),
            Err(error) => {
                tracing::warn!("prompt queue lookup failed for pane {pane_id:?}: {error}");
                return;
            }
        }
    };
    let Some(prompt) = prompt else {
        return;
    };
    let mut bytes = Vec::with_capacity(prompt.text.len() + 1);
    bytes.extend_from_slice(prompt.text.as_bytes());
    bytes.push(b'\r');
    if let Err(error) = write_key_input(
        state,
        pane_id,
        &bytes,
        Some(PromptSubmissionSource::QueuedPrompt),
    )
    .await
    {
        // Delivery is aborted -- no acknowledgement will follow, so the
        // transaction has nothing left to protect against a racing Clear.
        drop(_transaction);
        tracing::error!("queued prompt delivery failed for pane {pane_id:?}: {error}");
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Error,
                kind: AgentDebugEventKind::Error,
                summary: "Queued prompt delivery failed".to_string(),
                fields: vec![AgentDebugField::multiline("error", error)],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
        return;
    }
    let acknowledged = {
        let mut tree = state.tree.write().await;
        acknowledge_delivery(&mut tree, pane_id, &prompt)
    };
    // The protected region ends at acknowledgement; release before the
    // broadcast/persist and debug-record calls below so they don't hold up
    // every other pane's queue mutations.
    drop(_transaction);
    match acknowledged {
        Ok(true) => {
            broadcast_and_persist(state).await;
            let _ = crate::agent_debug::record(
                state,
                pane_id,
                AgentDebugSource::Pty,
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::QueuedPromptDelivered,
                    "Queued prompt delivered after agent completion",
                )
                .with_fields(vec![AgentDebugField::plain(
                    "delivery policy",
                    format!("{:?}", prompt.delivery),
                )]),
            )
            .await;
        }
        Ok(false) => {
            tracing::warn!("queued prompt head changed during delivery for pane {pane_id:?}")
        }
        Err(error) => {
            tracing::error!("queued prompt acknowledgement failed for pane {pane_id:?}: {error}")
        }
    }
}

fn acknowledge_delivery(
    tree: &mut ilium_core::Tree,
    pane_id: NodeId,
    prompt: &QueuedPrompt,
) -> Result<bool, ilium_core::TreeError> {
    tree.acknowledge_queued_prompt(pane_id, prompt)
}
