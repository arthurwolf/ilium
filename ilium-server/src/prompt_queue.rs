//! Completion-driven queued-prompt delivery.
//!
//! The detection loop alone decides when an agent has finished. This module
//! then submits exactly one FIFO head plus Enter in separate input stages, and
//! advances that head only after the PTY accepts both stages. Attempt intent
//! reaches the snapshot before either stage, so crash restore cannot replay
//! an ambiguous partial submission.

use ilium_agent_debug::{
    AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField, AgentDebugSeverity,
    AgentDebugSource,
};
use ilium_core::{NodeId, QueuedPrompt};
use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{broadcast_and_persist, submit_terminal_text_locked};
use crate::pane::PaneResource;
use crate::state::ServerState;

/// Delivers one currently queued prompt after a verified agent completion.
/// A queue mutation and the write/ack pair share one transaction, so Clear
/// cannot race a just-completed agent into sending a prompt the user removed.
pub(crate) async fn deliver_next_after_completion(state: &ServerState, pane_id: NodeId) {
    let input_gate = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        std::sync::Arc::clone(&runtime.input_gate)
    };
    let _input_guard = input_gate.lock().await;
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
    if prompt.attempted_delivery {
        // Keep the authored head visible. It may have been wholly, partly, or
        // not at all delivered before a crash/interruption; no automatic
        // replay can establish which case occurred.
        return;
    }
    let marked = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
            if std::sync::Arc::ptr_eq(&input_gate, &runtime.input_gate)
                && matches!(runtime.session.input_handle().status(), ilium_pty::OwnerStatus::Running))
        {
            return;
        }
        tree.mark_queued_prompt_attempted(pane_id, &prompt)
    };
    match marked {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!("failed to mark queued prompt for pane {pane_id:?}: {error}");
            return;
        }
    }
    if let Err(error) = crate::persistence::await_snapshot_durability_barrier(state).await {
        // No byte has been submitted. Retaining the in-memory bit is a safe
        // fence if an earlier snapshot writer happened to persist it.
        tracing::error!("could not persist queued prompt attempt for pane {pane_id:?}: {error}");
        return;
    }
    if let Err(error) = submit_terminal_text_locked(
        state,
        pane_id,
        &prompt.text,
        PromptSubmissionSource::QueuedPrompt,
        &input_gate,
    )
    .await
    {
        // Delivery is ambiguous; the attempted head remains durably fenced.
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
    let mut attempted = prompt.clone();
    attempted.attempted_delivery = true;
    let acknowledged = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
            if std::sync::Arc::ptr_eq(&input_gate, &runtime.input_gate))
        {
            return;
        }
        acknowledge_delivery(&mut tree, pane_id, &attempted)
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
