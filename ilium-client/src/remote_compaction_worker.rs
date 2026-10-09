//! Background side of remote compaction: the blocking worker that first waits
//! for the agent to reach a safe pause and then runs `ilium-remote-compaction`,
//! plus the [`Summarizer`] that sends each prompt through the configured
//! inference provider (the same one used for tree renaming and reordering).
//!
//! Everything here is pure plumbing over channels; the dialog and the flow
//! that decide what happens next live in `remote_compaction_dialog` and
//! `remote_compaction_flow`.

use std::convert::Infallible;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_core::NodeId;
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, SkipReason,
};
use ilium_inference::{
    InferenceError, InferenceRequest, InferenceSettings, InferenceStreamEvent,
    UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};
use ilium_remote_compaction::{
    compact_session, transcript_is_at_pause_point, AgentKind, CompactionEvent, CompactionOutcome,
    CompactionRequest, Summarizer, SummarizerError, SummaryRequest, SummaryResponse,
};
use tokio::sync::{mpsc, Notify};

/// A summarization call may stream for minutes on a large chunk.
pub const SUMMARY_CALL_TIMEOUT: Duration = Duration::from_secs(900);

/// How often the pause wait re-reads the transcript tail.
const PAUSE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMPACTION_WORKING_BYTES: usize = 384 * 1024 * 1024;
const COMPACTION_RESULT_BYTES: usize = 2 * 1024 * 1024;
// Pause detection parses a 4 MiB transcript tail into owned JSON values. The
// reservation includes both the input window and an explicit parse allowance.
const PAUSE_PROBE_WORKING_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROGRESS_TEXT_BYTES: usize = 4 * 1024;
const MAX_SUMMARY_TEXT_BYTES: usize = 16 * 1024 * 1024;
const UI_EVENT_CAPACITY: usize = 64;

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
    interrupt_pending: Arc<AtomicBool>,
    kind: WorkerKind,
    pane_id: NodeId,
    receipt: Receipt<RemoteCompactionTask>,
}

#[derive(Clone, Copy)]
enum WorkerKind {
    AwaitPause,
    Compact,
}

struct RemoteCompactionTask {
    job: RemoteCompactionJob,
    events: mpsc::Sender<RemoteCompactionWorkerEvent>,
    cancel: Arc<AtomicBool>,
    event_overflow: Arc<AtomicBool>,
    interrupt_pending: Arc<AtomicBool>,
    notification: Arc<Notify>,
}

impl Job for RemoteCompactionTask {
    type Output = RemoteCompactionWorkerEvent;
    type Error = Infallible;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let event = match self.job {
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
                    &|| context.stop_requested() || self.cancel.load(Ordering::Acquire),
                    &mut || {
                        self.interrupt_pending.store(true, Ordering::Release);
                        self.notification.notify_one();
                    },
                );
                RemoteCompactionWorkerEvent::PauseReached { pane_id, result }
            }
            RemoteCompactionJob::Compact {
                pane_id,
                request,
                inference_settings,
            } => {
                let limiter = crate::naming_workers::shared_provider_limiter();
                let events = self.events.clone();
                let cancel = Arc::clone(&self.cancel);
                let event_overflow = Arc::clone(&self.event_overflow);
                let notification = Arc::clone(&self.notification);
                let result = match crate::provider_admission::run(
                    request,
                    context,
                    &limiter,
                    |_| false,
                    |request, context| {
                        let summarizer =
                            InferenceSummarizer::new(*inference_settings, &cancel, &context);
                        let mut sink = |event| {
                            if context.stop_requested() {
                                cancel.store(true, Ordering::Release);
                            }
                            publish_progress(
                                &events,
                                &notification,
                                &cancel,
                                &event_overflow,
                                pane_id,
                                event,
                            );
                        };
                        let result = compact_session(&request, &summarizer, &cancel, &mut sink)
                            .map_err(|error| error.to_string());
                        annotate_progress_overflow(result, event_overflow.load(Ordering::Acquire))
                    },
                ) {
                    ControlFlow::Continue(result) => result.map_err(bounded_error),
                    ControlFlow::Break((_request, error)) => Err(bounded_error(error.to_string())),
                };
                RemoteCompactionWorkerEvent::Finished { pane_id, result }
            }
        };
        Ok(event)
    }
}

/// Owns the single in-flight worker so it can be cancelled and never outlives
/// the client.
pub struct RemoteCompactionWorkers {
    client: Client,
    events: mpsc::Sender<RemoteCompactionWorkerEvent>,
    received_events: mpsc::Receiver<RemoteCompactionWorkerEvent>,
    notification: Arc<Notify>,
    active: Option<ActiveWorker>,
}

impl RemoteCompactionWorkers {
    pub fn new(client: Client, notification: Arc<Notify>) -> Self {
        let (events, received_events) = mpsc::channel(UI_EVENT_CAPACITY);
        Self {
            client,
            events,
            received_events,
            notification,
            active: None,
        }
    }

    pub fn spawn(&mut self, job: RemoteCompactionJob) -> Result<(), RemoteCompactionWorkerEvent> {
        if self.active.is_some() {
            return Err(failure_event(
                &job,
                "A previous remote compaction is still stopping",
            ));
        }
        let pane_id = job.pane_id();
        let kind = job.kind();
        let cancel = Arc::new(AtomicBool::new(false));
        let interrupt_pending = Arc::new(AtomicBool::new(false));
        let event_overflow = Arc::new(AtomicBool::new(false));
        let cost = job.cost();
        let reservation = self.client.try_reserve(Lane::Io, cost).map_err(|reason| {
            failure_event(
                &job,
                &format!("Remote compaction admission refused: {reason:?}"),
            )
        })?;
        let task = RemoteCompactionTask {
            job,
            events: self.events.clone(),
            cancel: Arc::clone(&cancel),
            event_overflow,
            interrupt_pending: Arc::clone(&interrupt_pending),
            notification: Arc::clone(&self.notification),
        };
        let receipt = reservation.submit(task).map_err(|rejected| {
            failure_event(
                &rejected.value.job,
                &format!("Remote compaction admission refused: {:?}", rejected.reason),
            )
        })?;
        self.active = Some(ActiveWorker {
            cancel,
            interrupt_pending,
            kind,
            pane_id,
            receipt,
        });
        Ok(())
    }

    /// Asks the running job (if any) to stop at its next checkpoint.
    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.interrupt_pending.store(false, Ordering::Release);
            active.cancel.store(true, Ordering::Release);
            active.receipt.cancel();
        }
    }

    pub fn is_running(&self) -> bool {
        self.active.is_some()
    }

    /// Drains replaceable UI updates and then takes the semantic final result
    /// from its receipt. The receipt's charge remains alive through `apply`.
    pub fn collect(&mut self, mut apply: impl FnMut(RemoteCompactionWorkerEvent)) -> bool {
        let mut did_apply = false;
        while let Ok(event) = self.received_events.try_recv() {
            apply(event);
            did_apply = true;
        }
        let Some(active) = self.active.as_mut() else {
            return did_apply;
        };
        if active.interrupt_pending.swap(false, Ordering::AcqRel) {
            apply(RemoteCompactionWorkerEvent::InterruptRequested {
                pane_id: active.pane_id,
            });
            did_apply = true;
        }
        let kind = active.kind;
        let pane_id = active.pane_id;
        let result = match active.receipt.try_take() {
            JobPoll::Pending | JobPoll::Taken => None,
            JobPoll::Ready(outcome) => Some(outcome.map(|outcome| match outcome {
                JobOutcome::Finished(Ok(event)) => event,
                JobOutcome::Finished(Err(never)) => match never {},
                JobOutcome::NotStarted { job, reason } => failure_event(
                    &job.job,
                    match reason {
                        SkipReason::Cancelled => {
                            "Remote compaction was cancelled before it started"
                        }
                        SkipReason::Shutdown => "Remote compaction stopped during shutdown",
                    },
                ),
                JobOutcome::Panicked => RemoteCompactionWorkerEvent::from_failure(
                    kind,
                    pane_id,
                    "Remote compaction worker panicked",
                ),
            })),
            JobPoll::Lost => Some(active.receipt.retention().retain(
                RemoteCompactionWorkerEvent::from_failure(
                    kind,
                    pane_id,
                    "Remote compaction worker lost its completion receipt",
                ),
            )),
        };
        if let Some(outcome) = result {
            if let Some(active) = self.active.take() {
                let (event, retention) = outcome.into_parts();
                apply(event);
                drop(retention);
                drop(active);
                did_apply = true;
            }
        }
        did_apply
    }
}

impl Drop for RemoteCompactionWorkers {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl RemoteCompactionJob {
    fn pane_id(&self) -> NodeId {
        match self {
            Self::AwaitPause { pane_id, .. } | Self::Compact { pane_id, .. } => *pane_id,
        }
    }

    fn kind(&self) -> WorkerKind {
        match self {
            Self::AwaitPause { .. } => WorkerKind::AwaitPause,
            Self::Compact { .. } => WorkerKind::Compact,
        }
    }

    fn cost(&self) -> JobCost {
        let input_bytes = match self {
            Self::AwaitPause {
                transcript_path, ..
            } => PAUSE_PROBE_WORKING_BYTES + transcript_path.as_os_str().len(),
            Self::Compact {
                request,
                inference_settings,
                ..
            } => COMPACTION_WORKING_BYTES
                .saturating_add(request.transcript_path.as_os_str().len())
                .saturating_add(request.session_id.len())
                .saturating_add(request.project_cwd.as_os_str().len())
                .saturating_add(
                    request
                        .options
                        .custom_prompt
                        .as_ref()
                        .map_or(0, String::len),
                )
                .saturating_add(std::mem::size_of_val(inference_settings.as_ref())),
        };
        JobCost {
            input_bytes,
            result_bytes: COMPACTION_RESULT_BYTES,
        }
    }
}

fn failure_event(job: &RemoteCompactionJob, reason: &str) -> RemoteCompactionWorkerEvent {
    RemoteCompactionWorkerEvent::from_failure(
        job.kind(),
        job.pane_id(),
        &bounded_error(reason.to_string()),
    )
}

fn bounded_error(mut error: String) -> String {
    truncate_text(&mut error);
    error
}

fn annotate_progress_overflow<T>(result: Result<T, String>, overflowed: bool) -> Result<T, String> {
    if overflowed && matches!(&result, Err(error) if error == "compaction cancelled") {
        Err("Remote compaction stopped because its UI event queue was full".to_string())
    } else {
        result
    }
}

impl RemoteCompactionWorkerEvent {
    fn from_failure(kind: WorkerKind, pane_id: NodeId, reason: &str) -> Self {
        match kind {
            WorkerKind::AwaitPause => Self::PauseReached {
                pane_id,
                result: Err(reason.to_string()),
            },
            WorkerKind::Compact => Self::Finished {
                pane_id,
                result: Err(reason.to_string()),
            },
        }
    }
}

fn publish_progress(
    events: &mpsc::Sender<RemoteCompactionWorkerEvent>,
    notification: &Notify,
    cancel: &AtomicBool,
    event_overflow: &AtomicBool,
    pane_id: NodeId,
    mut event: CompactionEvent,
) {
    match &mut event {
        CompactionEvent::Step { title, .. } | CompactionEvent::Log(title) => truncate_text(title),
        CompactionEvent::Progress(_) | CompactionEvent::Tokens(_) => {}
    }
    let is_replaceable = matches!(
        event,
        CompactionEvent::Progress(_) | CompactionEvent::Tokens(_)
    );
    match events.try_send(RemoteCompactionWorkerEvent::Progress { pane_id, event }) {
        Ok(()) => notification.notify_one(),
        Err(mpsc::error::TrySendError::Full(_)) if is_replaceable => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            event_overflow.store(true, Ordering::Release);
            cancel.store(true, Ordering::Release);
            notification.notify_one();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            cancel.store(true, Ordering::Release);
            notification.notify_one();
        }
    }
}

fn truncate_text(text: &mut String) {
    if text.len() > MAX_PROGRESS_TEXT_BYTES {
        let mut boundary = MAX_PROGRESS_TEXT_BYTES;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
        text.push('…');
    }
}

/// Polls until `transcript_is_at_pause_point`, calling `request_interrupt`
/// once after `interrupt_after` and giving up `grace` later.
fn await_pause(
    agent: AgentKind,
    transcript_path: &std::path::Path,
    interrupt_after: Duration,
    grace: Duration,
    is_cancelled: &dyn Fn() -> bool,
    request_interrupt: &mut dyn FnMut(),
) -> Result<(), String> {
    let started = Instant::now();
    let mut is_interrupt_requested = false;
    loop {
        if is_cancelled() {
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
    context: &'cancel JobContext,
}

impl<'cancel> InferenceSummarizer<'cancel> {
    pub fn new(
        settings: InferenceSettings,
        cancel: &'cancel AtomicBool,
        context: &'cancel JobContext,
    ) -> Self {
        Self {
            settings,
            cancel,
            context,
        }
    }
}

impl Summarizer for InferenceSummarizer<'_> {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        if self.context.stop_requested() || self.cancel.load(Ordering::Acquire) {
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
        let mut exceeded_response_limit = false;
        let result = provider.stream(&inference_request, &mut |event| {
            if self.context.stop_requested() || self.cancel.load(Ordering::Acquire) {
                is_cancelled = true;
                return false;
            }
            match event {
                InferenceStreamEvent::TextDelta(delta) => {
                    if text.len().saturating_add(delta.len()) > MAX_SUMMARY_TEXT_BYTES {
                        exceeded_response_limit = true;
                        return false;
                    }
                    text.push_str(&delta);
                }
                InferenceStreamEvent::OutputTokens(tokens) => output_tokens = Some(tokens),
            }
            true
        });
        if is_cancelled {
            return Err(SummarizerError::Cancelled);
        }
        if exceeded_response_limit {
            return Err(SummarizerError::Failed(format!(
                "summary response exceeded the {MAX_SUMMARY_TEXT_BYTES}-byte limit"
            )));
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
    fn progress_overflow_never_rewrites_a_committed_success_into_failure() {
        assert_eq!(
            annotate_progress_overflow(Ok(42), true),
            Ok(42),
            "a completed transcript rewrite remains successful even if a final progress event was dropped"
        );
        assert_eq!(
            annotate_progress_overflow::<()>(Err("compaction cancelled".to_string()), true),
            Err("Remote compaction stopped because its UI event queue was full".to_string())
        );
        assert_eq!(
            annotate_progress_overflow::<()>(
                Err("transcript verification failed".to_string()),
                true
            ),
            Err("transcript verification failed".to_string()),
            "progress overload must not hide the actual domain failure"
        );
    }

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
            &|| cancel.load(Ordering::Acquire),
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
            &|| cancel.load(Ordering::Acquire),
            &mut || interrupts += 1,
        );
        assert_eq!(interrupts, 1);
        assert!(result.is_err());
    }

    #[test]
    fn pause_probe_admission_covers_tail_parsing_peak() {
        let job = RemoteCompactionJob::AwaitPause {
            pane_id: NodeId(1),
            agent: AgentKind::Claude,
            transcript_path: PathBuf::from("transcript.jsonl"),
            interrupt_after: Duration::ZERO,
        };

        assert!(
            job.cost().input_bytes >= 64 * 1024 * 1024,
            "pause detection parses up to a 4 MiB JSONL tail, so its admitted peak must cover parsing overhead"
        );
    }

    #[tokio::test]
    async fn cancellation_completes_even_when_the_ui_event_queue_is_full() {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
            ShutdownMode,
        };

        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 4,
            service_jobs: 0,
            input_bytes: PAUSE_PROBE_WORKING_BYTES + 1024 * 1024,
            result_bytes: 4 * 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 4 * 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 1,
            priority: None,
            resident_bytes_per_thread: 1024 * 1024,
        };
        let mut execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: disabled,
                io: lane,
                service: disabled,
            },
        )
        .expect("start isolated I/O worker");
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        let client = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: PAUSE_PROBE_WORKING_BYTES + 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
            })
            .expect("admit remote-compaction client")
            .with_completion_wake(move || {
                wake.notify_one();
            });
        let mut workers = RemoteCompactionWorkers::new(client, Arc::clone(&notification));
        for _ in 0..UI_EVENT_CAPACITY {
            workers
                .events
                .try_send(RemoteCompactionWorkerEvent::Progress {
                    pane_id: NodeId(1),
                    event: CompactionEvent::Progress(0.5),
                })
                .expect("fill the bounded UI event queue");
        }
        workers
            .spawn(RemoteCompactionJob::AwaitPause {
                pane_id: NodeId(1),
                agent: AgentKind::Claude,
                transcript_path: PathBuf::from("/nonexistent/remote-compaction-transcript"),
                interrupt_after: Duration::ZERO,
            })
            .expect("admit pause job");

        let attempted = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if workers
                    .active
                    .as_ref()
                    .is_some_and(|active| active.interrupt_pending.load(Ordering::Acquire))
                {
                    break;
                }
                notification.notified().await;
            }
        })
        .await
        .is_ok();
        assert!(
            attempted,
            "worker must reach interrupt publication while the UI queue is full"
        );

        // The worker has already published the interrupt wake; cancellation
        // must still allow it to return its typed final result.
        workers.cancel();
        let mut received = Vec::new();
        let completed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notified = notification.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                workers.collect(|event| received.push(event));
                if !workers.is_running() {
                    break;
                }
                notified.await;
            }
        })
        .await
        .is_ok();
        assert!(
            completed,
            "cancellation must complete the retained worker receipt while the UI queue is full"
        );
        assert!(matches!(
            received.last(),
            Some(RemoteCompactionWorkerEvent::PauseReached { result: Err(_), .. })
        ));
        assert!(
            !workers.is_running(),
            "completed receipt must release the single-flight slot"
        );
        drop(workers);
        execution.request_shutdown(ShutdownMode::Cancel);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("join isolated worker");
    }
}
