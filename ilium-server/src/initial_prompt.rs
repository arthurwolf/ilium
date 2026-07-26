//! One-shot initial-agent prompt delivery.
//!
//! `CommandWithInitialInput` is used by the editor's "Run agent from this
//! line" action. A PTY accepting bytes only proves the shell received them;
//! it does not prove the agent CLI has finished startup and installed its
//! composer. This module waits for a provider-specific visible free-form
//! prompt before using the normal server input boundary to submit the original
//! request. It prefers the live process-tree classification, with a screen
//! fallback for the small startup window before that detector's next tick.

use std::sync::Arc;
use std::time::Duration;

use ilium_core::NodeId;
use ilium_ipc::{PromptSubmissionSource, ServerEvent};

use crate::ipc::handlers::write_key_input;
use crate::pane::PaneResource;
use crate::state::ServerState;

/// Rechecks identity state even when the agent's prompt was drawn before the
/// detector's process-tree pass completed. Screen changes wake the task
/// immediately; this bounded fallback avoids depending on a further redraw.
const READINESS_RECHECK_INTERVAL: Duration = Duration::from_millis(100);

/// Starts the pane-owned task that delivers one initial prompt after visible
/// agent readiness. A caller may return to IPC immediately; the task is
/// cancelled by pane teardown or by the user's first manual terminal input.
pub(crate) async fn start(state: Arc<ServerState>, pane_id: NodeId, initial_input: String) {
    let task = tokio::spawn(deliver_when_ready(
        Arc::clone(&state),
        pane_id,
        initial_input,
    ));
    let mut panes = state.panes.write().await;
    match panes.get_mut(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => runtime.set_initial_prompt_task(task),
        Some(PaneResource::Editor { .. }) | None => task.abort(),
    }
}

/// Waits on the PTY's screen-change signal until either the detector or one of
/// the known provider composer signatures confirms readiness, then writes text
/// plus the final Enter in one PTY transaction so user input cannot interleave.
async fn deliver_when_ready(state: Arc<ServerState>, pane_id: NodeId, initial_input: String) {
    let mut screen_changed = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        runtime.session.subscribe_screen_changed()
    };

    loop {
        if pane_has_ready_agent_composer(&state, pane_id).await {
            let bytes = initial_submission_bytes(&initial_input);
            match write_key_input(
                &state,
                pane_id,
                &bytes,
                Some(PromptSubmissionSource::InitialAgentPrompt),
            )
            .await
            {
                Ok(()) => state.broadcast(ServerEvent::PanePromptSubmitted {
                    pane_id,
                    source: PromptSubmissionSource::InitialAgentPrompt,
                }),
                Err(error) => tracing::warn!(
                    pane_id = pane_id.0,
                    "initial agent prompt was not delivered after readiness: {error}"
                ),
            }
            return;
        }

        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

/// Takes one coherent pane snapshot while holding the registry read lock.
/// Detection owns the process-tree work; when that classification has not yet
/// arrived, the fallback still accepts only a known provider's exact composer
/// chrome. A new pane has no prior transcript, so that narrow screen contract
/// is stronger and more responsive than a fixed launch delay.
async fn pane_has_ready_agent_composer(state: &ServerState, pane_id: NodeId) -> bool {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return false;
    };
    let screen_snapshot = runtime.session.screen_snapshot();
    if let Some(agent_class) = runtime.detected_agent_class.as_ref() {
        return ilium_detect::is_agent_prompt_ready(agent_class, &screen_snapshot.text);
    }

    [
        ilium_core::AgentClass::Claude,
        ilium_core::AgentClass::Codex,
        ilium_core::AgentClass::Antigravity,
    ]
    .iter()
    .any(|agent_class| ilium_detect::is_agent_prompt_ready(agent_class, &screen_snapshot.text))
}

/// Encodes a multiline editor task as bracketed paste and appends the sole
/// submission Enter. Keeping both in one call to `write_key_input` makes the
/// text/Enter handoff atomic with respect to concurrent terminal input.
fn initial_submission_bytes(initial_input: &str) -> Vec<u8> {
    let mut bytes = initial_input_bytes(initial_input);
    bytes.push(b'\r');
    bytes
}

/// Encodes multiline editor content as one bracketed paste, leaving the
/// caller responsible for the one semantic submission key that follows it.
pub(crate) fn initial_input_bytes(initial_input: &str) -> Vec<u8> {
    if !initial_input.contains('\n') {
        return initial_input.as_bytes().to_vec();
    }

    let mut bracketed_paste = Vec::with_capacity(initial_input.len() + 12);
    bracketed_paste.extend_from_slice(b"\x1b[200~");
    bracketed_paste.extend_from_slice(initial_input.as_bytes());
    bracketed_paste.extend_from_slice(b"\x1b[201~");
    bracketed_paste
}

#[cfg(test)]
mod tests {
    use super::initial_submission_bytes;

    #[test]
    fn multiline_initial_prompt_is_one_bracketed_paste_with_final_enter() {
        assert_eq!(
            initial_submission_bytes("first\nsecond"),
            b"\x1b[200~first\nsecond\x1b[201~\r"
        );
    }
}
