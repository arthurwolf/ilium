//! Process-fenced wait observation on the server's admitted I/O lane.

use std::sync::Arc;

use ilium_core::cache_keepalive::WaitObservation;
use ilium_core::{AgentClass, NodeId};
use ilium_execution::{JobCost, Lane};

use crate::cache_wait_evidence::{WaitEvidence, WaitTranscriptCursor};
use crate::pane::{CacheWaitSource, PaneResource};
use crate::state::ServerState;

/// One bounded scan may consume up to 16 MiB of transcript and temporary
/// JSON parsing memory. Longer or concurrently changing logs return no
/// evidence; neither condition permits a provider request.
const MAX_CHUNKS: usize = 64;
const JOB_INPUT_BYTES: usize = 160 * 1024 * 1024;
const JOB_RESULT_BYTES: usize = 64 * 1024;

struct ReadResult {
    source: CacheWaitSource,
    evidence: Option<WaitEvidence>,
    process_current: bool,
}

/// Call at a candidate refresh deadline, not in the interactive pane lock.
/// Returning None means uncertain or obsolete evidence, not an idle agent.
pub(crate) async fn observe_cache_wait(
    state: &Arc<ServerState>,
    pane_id: NodeId,
) -> Option<(AgentClass, WaitObservation)> {
    let source = {
        let panes = state.panes.read().await;
        let PaneResource::Terminal(runtime) = panes.get(&pane_id)? else { return None };
        runtime.cache_wait_source()?
    };
    let execution = state.execution.get()?.client.clone();
    let result = execution.run(
        Lane::Io,
        JobCost { input_bytes: JOB_INPUT_BYTES, result_bytes: JOB_RESULT_BYTES },
        move |context: ilium_execution::JobContext| {
            if context.stop_requested()
                || !ilium_platform::process_control::captured_process_is_current(&source.process_identity)?
            {
                return Ok::<_, std::io::Error>(ReadResult { source, evidence: None, process_current: false });
            }
            let mut cursor = WaitTranscriptCursor::new(
                source.transcript_path.clone(), source.session_id.clone(),
                source.process.class.clone(), source.generation,
            );
            let mut evidence = None;
            for _ in 0..MAX_CHUNKS {
                if context.stop_requested() { break }
                match cursor.read_available(source.generation, &source.transcript_path) {
                    Ok(Some(current)) => { evidence = Some(current); break }
                    Ok(None) if cursor.caught_up() => break,
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
            let process_current = !context.stop_requested()
                && ilium_platform::process_control::captured_process_is_current(&source.process_identity)?;
            Ok::<_, std::io::Error>(ReadResult { source, evidence, process_current })
        },
    ).await.ok()?;
    let read = result.view();
    if !read.process_current {
        return None;
    }
    let panes = state.panes.read().await;
    let PaneResource::Terminal(runtime) = panes.get(&pane_id)? else { return None };
    if !runtime.cache_wait_source_is_current(&read.source) {
        return None;
    }
    let current = runtime.cache_wait_source()?;
    Some((current.process.class, wait_observation(current.monitor_running, read.evidence)))
}

/// Monitor liveness comes from the pane, independently of transcript parsing.
/// Unknown transcript state must never turn into a positive tool-wait claim.
fn wait_observation(monitor_running: bool, evidence: Option<WaitEvidence>) -> WaitObservation {
    let evidence = evidence.unwrap_or_default();
    WaitObservation {
        monitor_running,
        command_waiting: evidence.command,
        subagent_waiting: evidence.subagent,
        wait_started_millis: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_liveness_does_not_depend_on_transcript_availability() {
        let observation = wait_observation(true, None);
        assert!(observation.monitor_running);
        assert!(!observation.command_waiting);
        assert!(!observation.subagent_waiting);
        let observation = wait_observation(false, Some(WaitEvidence {
            monitor: false, command: true, subagent: true,
        }));
        assert!(!observation.monitor_running);
        assert!(observation.command_waiting);
        assert!(observation.subagent_waiting);
    }
}
