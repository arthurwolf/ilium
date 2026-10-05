//! Background side of remote compaction: the blocking worker that first waits
//! for the agent to reach a safe pause and then runs `ilium-remote-compaction`,
//! plus the [`Summarizer`] that sends each prompt through the configured
//! inference provider (the same one used for tree renaming and reordering).
//!
//! Everything here is pure plumbing over channels; the dialog and the flow
//! that decide what happens next live in `remote_compaction_dialog` and
//! `remote_compaction_flow`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_core::NodeId;
use ilium_inference::{
    InferenceError, InferenceRequest, InferenceSettings, InferenceStreamEvent,
    UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};
use ilium_remote_compaction::{
    compact_session, transcript_is_at_pause_point, AgentKind, CompactionEvent, CompactionOutcome,
    CompactionRequest, Summarizer, SummarizerError, SummaryRequest, SummaryResponse,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A summarization call may stream for minutes on a large chunk.
pub const SUMMARY_CALL_TIMEOUT: Duration = Duration::from_secs(900);

/// How often the pause wait re-reads the transcript tail.
const PAUSE_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Extra time after the interrupt key before the wait gives up.
pub const PAUSE_AFTER_INTERRUPT_GRACE: Duration = Duration::from_secs(45);

/// Work the event loop hands to the blocking worker.
#[derive(Debug, Clone)]
pub enum RemoteCompactionJob {
    /// Poll the transcript until the agent is between turns. After
    /// `interrupt_after` an interrupt is requested once.
    AwaitPause {
        pane_id: NodeId,
        agent: AgentKind,
        transcript_path: PathBuf,
        interrupt_after: Duration,
    },
    /// Run the whole compaction on the stopped agent's transcript.
    Compact {
        pane_id: NodeId,
        request: CompactionRequest,
        inference_settings: Box<InferenceSettings>,
    },
}

/// Messages a running worker sends back to the event loop.
#[derive(Debug)]
pub enum RemoteCompactionWorkerEvent {
    /// The wait passed its soft deadline: the agent should be interrupted.
    InterruptRequested { pane_id: NodeId },
    /// Outcome of an `AwaitPause` job.
    PauseReached {
        pane_id: NodeId,
        result: Result<(), String>,
    },
    Progress {
        pane_id: NodeId,
        event: CompactionEvent,
    },
    Finished {
        pane_id: NodeId,
        result: Result<CompactionOutcome, String>,
    },
}

struct ActiveWorker {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Owns the single in-flight worker so it can be cancelled and never outlives
/// the client.
pub struct RemoteCompactionWorkers {
    events: mpsc::Sender<RemoteCompactionWorkerEvent>,
    active: Option<ActiveWorker>,
}

impl RemoteCompactionWorkers {
    pub fn new(events: mpsc::Sender<RemoteCompactionWorkerEvent>) -> Self {
        Self {
            events,
            active: None,
        }
    }

    pub fn spawn(&mut self, job: RemoteCompactionJob) {
        if let Some(previous) = self.active.take() {
            previous.cancel.store(true, Ordering::SeqCst);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let events = self.events.clone();
        let handle = tokio::task::spawn_blocking(move || match job {
            RemoteCompactionJob::AwaitPause {
                pane_id,
                agent,
                transcript_path,
                interrupt_after,
            } => {
                let result = await_pause(
                    agent,
                    &transcript_path,
                    interrupt_after,
                    PAUSE_AFTER_INTERRUPT_GRACE,
                    &worker_cancel,
                    &mut || {
                        let _ =
                            events.blocking_send(RemoteCompactionWorkerEvent::InterruptRequested {
                                pane_id,
                            });
                    },
                );
                let _ = events
                    .blocking_send(RemoteCompactionWorkerEvent::PauseReached { pane_id, result });
            }
            RemoteCompactionJob::Compact {
                pane_id,
                request,
                inference_settings,
            } => {
                let summarizer = InferenceSummarizer::new(*inference_settings, &worker_cancel);
                let progress_events = events.clone();
                let mut sink = |event: CompactionEvent| {
                    // The receiver only disappears while the client shuts down.
                    let _ = progress_events
                        .blocking_send(RemoteCompactionWorkerEvent::Progress { pane_id, event });
                };
                let result = compact_session(&request, &summarizer, &worker_cancel, &mut sink)
                    .map_err(|error| error.to_string());
                let _ =
                    events.blocking_send(RemoteCompactionWorkerEvent::Finished { pane_id, result });
            }
        });
        self.active = Some(ActiveWorker { cancel, handle });
    }

    /// Asks the running job (if any) to stop at its next checkpoint.
    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn is_running(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| !active.handle.is_finished())
    }
}

impl Drop for RemoteCompactionWorkers {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Polls until `transcript_is_at_pause_point`, calling `request_interrupt`
/// once after `interrupt_after` and giving up `grace` later.
fn await_pause(
    agent: AgentKind,
    transcript_path: &std::path::Path,
    interrupt_after: Duration,
    grace: Duration,
    cancel: &AtomicBool,
    request_interrupt: &mut dyn FnMut(),
) -> Result<(), String> {
    let started = Instant::now();
    let mut is_interrupt_requested = false;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".to_string());
        }
        if transcript_is_at_pause_point(agent, transcript_path) {
            return Ok(());
        }
        let waited = started.elapsed();
        if !is_interrupt_requested && waited >= interrupt_after {
            is_interrupt_requested = true;
            request_interrupt();
        }
        if waited >= interrupt_after + grace {
            return Err("the agent did not reach a safe pause in time".to_string());
        }
        std::thread::sleep(PAUSE_POLL_INTERVAL);
    }
}

/// Sends the crate's prompts through the configured inference provider.
pub struct InferenceSummarizer<'cancel> {
    settings: InferenceSettings,
    cancel: &'cancel AtomicBool,
}

impl<'cancel> InferenceSummarizer<'cancel> {
    pub fn new(settings: InferenceSettings, cancel: &'cancel AtomicBool) -> Self {
        Self { settings, cancel }
    }
}

impl Summarizer for InferenceSummarizer<'_> {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(SummarizerError::Cancelled);
        }
        let provider = ilium_inference::provider_from_settings(&self.settings);
        let inference_request = InferenceRequest {
            system_prompt: request.system.clone(),
            user_prompt: request.user.clone(),
            max_tokens: UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
            timeout: Some(SUMMARY_CALL_TIMEOUT),
        };
        let mut text = String::new();
        let mut output_tokens = None;
        let mut is_cancelled = false;
        let result = provider.stream(&inference_request, &mut |event| {
            if self.cancel.load(Ordering::SeqCst) {
                is_cancelled = true;
                return false;
            }
            match event {
                InferenceStreamEvent::TextDelta(delta) => text.push_str(&delta),
                InferenceStreamEvent::OutputTokens(tokens) => output_tokens = Some(tokens),
            }
            true
        });
        if is_cancelled {
            return Err(SummarizerError::Cancelled);
        }
        result.map_err(classify_inference_error)?;
        Ok(SummaryResponse {
            text,
            input_tokens: None,
            output_tokens,
        })
    }
}

/// Providers report an oversized prompt in free text, so the status code and
/// the usual phrases decide whether the crate should re-chunk.
fn classify_inference_error(error: InferenceError) -> SummarizerError {
    let text = error.to_string();
    let lowered = text.to_lowercase();
    let mentions_size = [
        "context length",
        "context window",
        "context_length",
        "maximum context",
        "too long",
        "too large",
        "prompt is too",
        "exceeds the",
        "token limit",
        "max_tokens",
    ]
    .iter()
    .any(|phrase| lowered.contains(phrase));
    let is_size_status = matches!(
        &error,
        InferenceError::Http { status, .. } if matches!(*status, 413)
    );
    let is_bad_request_about_size = matches!(
        &error,
        InferenceError::Http { status, .. } if matches!(*status, 400 | 422)
    ) && mentions_size;
    if is_size_status || is_bad_request_about_size {
        SummarizerError::ContextTooLong(text)
    } else {
        SummarizerError::Failed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_too_large_asks_for_a_smaller_chunk() {
        let error = InferenceError::Http {
            status: 413,
            message: "payload too large".to_string(),
        };
        assert!(matches!(
            classify_inference_error(error),
            SummarizerError::ContextTooLong(_)
        ));
    }

    #[test]
    fn bad_request_about_context_length_asks_for_a_smaller_chunk() {
        let error = InferenceError::Http {
            status: 400,
            message: "This model's maximum context length is 128000 tokens".to_string(),
        };
        assert!(matches!(
            classify_inference_error(error),
            SummarizerError::ContextTooLong(_)
        ));
    }

    #[test]
    fn other_failures_stay_plain_failures() {
        for error in [
            InferenceError::Http {
                status: 429,
                message: "rate limited".to_string(),
            },
            InferenceError::Http {
                status: 400,
                message: "invalid api key".to_string(),
            },
            InferenceError::Transport("connection reset".to_string()),
        ] {
            assert!(matches!(
                classify_inference_error(error),
                SummarizerError::Failed(_)
            ));
        }
    }

    #[test]
    fn pause_wait_returns_when_cancelled() {
        let cancel = AtomicBool::new(true);
        let result = await_pause(
            AgentKind::Claude,
            std::path::Path::new("/nonexistent/transcript.jsonl"),
            Duration::from_secs(60),
            Duration::from_secs(60),
            &cancel,
            &mut || {},
        );
        assert_eq!(result, Err("cancelled".to_string()));
    }

    #[test]
    fn pause_wait_requests_one_interrupt_then_gives_up() {
        let cancel = AtomicBool::new(false);
        let mut interrupts = 0;
        let result = await_pause(
            AgentKind::Claude,
            std::path::Path::new("/nonexistent/transcript.jsonl"),
            Duration::ZERO,
            Duration::from_millis(600),
            &cancel,
            &mut || interrupts += 1,
        );
        assert_eq!(interrupts, 1);
        assert!(result.is_err());
    }
}
