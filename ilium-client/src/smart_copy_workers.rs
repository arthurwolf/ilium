//! Owned blocking worker for one streamed Smart Copy inference request.

use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason, Rejected,
    Retained, Retention,
};
use std::io;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilium_core::NodeId;
use ilium_inference::{InferenceRequest, InferenceSettings, InferenceStreamEvent};
use tokio::sync::mpsc::Sender;

use crate::smart_copy::MAXIMUM_JSONL_LINE_BYTES;
const RETRY_DELAY: Duration = Duration::from_millis(100);
#[cfg(test)]
type TestProvider = Arc<dyn ilium_inference::InferenceProvider>;
#[cfg(test)]
type TestProbe =
    Arc<dyn Fn() -> io::Result<Option<ilium_platform::file_lock::ExclusiveFileLock>> + Send + Sync>;

pub struct SmartCopyWorkerRequest {
    pub generation: u64,
    pub pane_id: NodeId,
    pub inference_settings: InferenceSettings,
    pub request: InferenceRequest,
    pub(crate) source_hold: Option<Arc<crate::terminal_parsing::SnapshotCharge>>,
}

#[derive(Debug)]
pub struct SmartCopyWorkerEvent {
    pub generation: u64,
    pub pane_id: NodeId,
    pub update: SmartCopyWorkerUpdate,
    pub(crate) retention: Option<Retention>,
}

#[derive(Debug)]
pub enum SmartCopyWorkerUpdate {
    ResponseStarted,
    Progress { received_characters: usize },
    JsonLine(String),
    ExactOutputTokens(u64),
    Finished,
    Failed(String),
}

#[derive(Clone)]
struct StreamMailbox(Arc<Mutex<StreamMessages>>, Option<Arc<tokio::sync::Notify>>);
struct StreamMessages {
    ordered: std::collections::VecDeque<SmartCopyWorkerEvent>,
    bytes: usize,
    failed: bool,
}
impl StreamMailbox {
    fn new(wake: Option<Arc<tokio::sync::Notify>>) -> Self {
        Self(
            Arc::new(Mutex::new(StreamMessages {
                ordered: std::collections::VecDeque::new(),
                bytes: 0,
                failed: false,
            })),
            wake,
        )
    }
    fn try_send(&self, event: SmartCopyWorkerEvent) -> Result<(), ()> {
        let mut messages = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if messages.failed {
            return Err(());
        }
        // Progress is replaceable; semantic records remain ordered. Neither
        // delivery nor cancellation waits for the interactive receiver.
        if matches!(event.update, SmartCopyWorkerUpdate::Progress { .. }) {
            if let Some(previous) = messages.ordered.back_mut() {
                if matches!(previous.update, SmartCopyWorkerUpdate::Progress { .. }) {
                    *previous = event;
                    return Ok(());
                }
            }
        }
        let bytes = std::mem::size_of::<SmartCopyWorkerEvent>()
            + match &event.update {
                SmartCopyWorkerUpdate::JsonLine(value) | SmartCopyWorkerUpdate::Failed(value) => {
                    value.capacity()
                }
                _ => 0,
            };
        if messages.ordered.len() >= 2048 || messages.bytes.saturating_add(bytes) > 20 * 1024 * 1024
        {
            messages.failed = true;
            return Err(());
        }
        messages.bytes += bytes;
        messages.ordered.push_back(event);
        if let Some(wake) = &self.1 {
            wake.notify_one();
        }
        Ok(())
    }
    fn failed(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .failed
    }
    fn drain(&self, target: &Sender<SmartCopyWorkerEvent>) -> bool {
        let mut messages = self.0.lock().unwrap_or_else(|error| error.into_inner());
        while let Some(event) = messages.ordered.pop_front() {
            let bytes = std::mem::size_of::<SmartCopyWorkerEvent>()
                + match &event.update {
                    SmartCopyWorkerUpdate::JsonLine(value)
                    | SmartCopyWorkerUpdate::Failed(value) => value.capacity(),
                    _ => 0,
                };
            match target.try_send(event) {
                Ok(()) => messages.bytes = messages.bytes.saturating_sub(bytes),
                Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                    messages.ordered.push_front(event);
                    return false;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    messages.ordered.clear();
                    messages.bytes = 0;
                    return true;
                }
            }
        }
        true
    }
}

struct CopyJob {
    request: Retained<SmartCopyWorkerRequest>,
    cancellation: Arc<AtomicBool>,
    events_tx: StreamMailbox,
    limiter: Arc<crate::naming_workers::InferenceConcurrencyLimiter>,
    #[cfg(test)]
    test_provider: Option<TestProvider>,
    #[cfg(test)]
    test_probe: Option<TestProbe>,
}
impl Job for CopyJob {
    type Output = ControlFlow<(Self, io::Error), ()>;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        let limiter = Arc::clone(&self.limiter);
        #[cfg(test)]
        if let Some(probe) = self.test_probe.clone() {
            return Ok(crate::provider_admission::run_with_probe(
                self,
                context,
                &limiter,
                |job| job.cancellation.load(Ordering::Acquire),
                Self::run_started,
                move || probe(),
            ));
        }
        Ok(crate::provider_admission::run(
            self,
            context,
            &limiter,
            |job| job.cancellation.load(Ordering::Acquire),
            Self::run_started, // Invocation permanently consumes replay eligibility, even before any event.
        ))
    }
} // No process or host permit is stored in pending UI state.
impl CopyJob {
    fn run_started(self, context: JobContext) {
        // Never return a future, response body, or detached provider owner.
        let Self {
            request,
            cancellation,
            events_tx,
            limiter: _,
            #[cfg(test)]
            test_provider,
            #[cfg(test)]
                test_probe: _,
        } = self;
        let (request, retention) = request.into_parts();
        if context.stop_requested() || cancellation.load(Ordering::Acquire) {
            let _ = send_update(
                &events_tx,
                &cancellation,
                &retention,
                request.generation,
                request.pane_id,
                SmartCopyWorkerUpdate::Failed("Smart Copy cancelled before provider call".into()),
            );
            return;
        }
        let generation = request.generation;
        let pane_id = request.pane_id;
        let error_tx = events_tx.clone();
        let error_cancel = Arc::clone(&cancellation);
        let error_retention = retention.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(test)]
            {
                run_worker(request, cancellation, events_tx, retention, test_provider)
            }
            #[cfg(not(test))]
            {
                run_worker(request, cancellation, events_tx, retention)
            }
        }))
        .is_err()
        {
            let _ = send_update(
                &error_tx,
                &error_cancel,
                &error_retention,
                generation,
                pane_id,
                SmartCopyWorkerUpdate::Failed(
                    "Smart Copy provider callback failed; stream was not replayed".into(),
                ),
            );
        }
    }
}
struct ActiveCopy {
    generation: u64,
    pane_id: NodeId,
    cancellation: Arc<AtomicBool>,
    receipt: Receipt<CopyJob>,
    finished: Option<Result<(), String>>,
    mailbox: StreamMailbox,
    retention: Retention,
}
/// At most one desired stream plus three real running/retiring receipts.
/// Cancelling a callback never claims that its kernel/provider call exited.
pub struct SmartCopyWorkers {
    events_tx: Sender<SmartCopyWorkerEvent>,
    client: Option<Client>,
    notification: Option<Arc<tokio::sync::Notify>>,
    pending: Option<CopyJob>,
    pending_retry_at: Option<Instant>,
    start_retry_at: Option<Instant>,
    active: Option<ActiveCopy>,
    retiring: Vec<ActiveCopy>,
    active_generation: Option<u64>,
    failures: std::collections::VecDeque<SmartCopyWorkerEvent>,
    #[cfg(test)]
    test_provider: Option<TestProvider>,
    #[cfg(test)]
    test_probe: Option<TestProbe>,
    #[cfg(test)]
    test_limiter: Option<Arc<crate::naming_workers::InferenceConcurrencyLimiter>>,
}
impl SmartCopyWorkers {
    pub fn new(events_tx: Sender<SmartCopyWorkerEvent>) -> Self {
        Self {
            events_tx,
            client: {
                #[cfg(test)]
                {
                    Some(crate::execution::test_client())
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            notification: None,
            pending: None,
            pending_retry_at: None,
            start_retry_at: None,
            active: None,
            retiring: Vec::new(),
            active_generation: None,
            failures: std::collections::VecDeque::new(),
            #[cfg(test)]
            test_provider: None,
            #[cfg(test)]
            test_probe: None,
            #[cfg(test)]
            test_limiter: None,
        }
    }
    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub(crate) fn set_completion_notification(&mut self, notification: Arc<tokio::sync::Notify>) {
        self.notification = Some(notification);
    }
    pub(crate) fn retry_delay(&self, now: Instant) -> Option<Duration> {
        let pending = if self.active.is_none() && self.retiring.len() < 2 && self.failures.len() < 3
        {
            self.pending.as_ref().and(self.pending_retry_at)
        } else {
            None
        };
        pending
            .into_iter()
            .chain(self.start_retry_at)
            .min()
            .map(|at| at.saturating_duration_since(now))
    }
    pub(crate) fn begin_retry_turn(&mut self, now: Instant) -> bool {
        if self.start_retry_at.is_some_and(|at| now < at) {
            return false;
        }
        self.start_retry_at = None;
        true
    }
    fn defer_start(&mut self, reason: RejectReason) {
        if !retryable_rejection(reason) {
            self.start_retry_at = None;
            return;
        }
        self.start_retry_at = Some(Instant::now() + RETRY_DELAY);
    }
    pub fn start(
        &mut self,
        request: SmartCopyWorkerRequest,
    ) -> Result<(), Box<Rejected<SmartCopyWorkerRequest>>> {
        self.cancel();
        self.collect();
        if self.retiring.len() >= 2 || self.failures.len() >= 3 {
            self.defer_start(RejectReason::JobLimit);
            return Err(Box::new(Rejected {
                reason: RejectReason::JobLimit,
                value: request,
            }));
        }
        let bytes = request
            .request
            .system_prompt
            .capacity()
            .saturating_add(request.request.user_prompt.capacity())
            .saturating_add(crate::naming_workers::inference_settings_bytes(
                &request.inference_settings,
            ));
        if bytes > 8 * 1024 * 1024 {
            return Err(Box::new(Rejected {
                reason: RejectReason::InvalidCost,
                value: request,
            }));
        }
        let Some(client) = &self.client else {
            return Err(Box::new(Rejected {
                reason: RejectReason::Closed,
                value: request,
            }));
        };
        let reservation = match client.try_reserve_external(JobCost {
            input_bytes: 8 * 1024 * 1024,
            result_bytes: 64 * 1024 * 1024,
        }) {
            Ok(reservation) => reservation,
            Err(reason) => {
                self.defer_start(reason);
                return Err(Box::new(Rejected {
                    reason,
                    value: request,
                }));
            }
        };
        let request = reservation.retain(request)?;
        self.active_generation = Some(request.view().generation);
        self.pending = Some(CopyJob {
            request,
            cancellation: Arc::new(AtomicBool::new(false)),
            events_tx: StreamMailbox::new(self.notification.clone()),
            limiter: {
                #[cfg(test)]
                {
                    self.test_limiter
                        .clone()
                        .unwrap_or_else(crate::naming_workers::shared_provider_limiter)
                }
                #[cfg(not(test))]
                {
                    crate::naming_workers::shared_provider_limiter()
                }
            },
            #[cfg(test)]
            test_provider: self.test_provider.clone(),
            #[cfg(test)]
            test_probe: self.test_probe.clone(),
        });
        self.collect();
        Ok(())
    }
    pub fn cancel(&mut self) {
        if let Some(job) = self.pending.take() {
            job.cancellation.store(true, Ordering::Release);
        }
        self.pending_retry_at = None;
        self.start_retry_at = None;
        if let Some(active) = self.active.take() {
            active.cancellation.store(true, Ordering::Release);
            active.receipt.cancel();
            self.retiring.push(active);
        }
        self.active_generation = None;
    }
    pub fn finish(&mut self, generation: u64) {
        if self.active_generation == Some(generation) {
            self.active_generation = None;
        }
        self.collect();
    }
    pub(crate) fn collect(&mut self) {
        let now = Instant::now(); // One collection turn cannot accidentally advance through multiple retry windows.
        self.retiring
            .retain_mut(|active| matches!(active.receipt.try_take(), JobPoll::Pending));
        if let Some(event) = self.failures.pop_front() {
            if let Err(tokio::sync::mpsc::error::TrySendError::Full(event)) =
                self.events_tx.try_send(event)
            {
                self.failures.push_front(event);
                self.defer_pending(now);
                return;
            }
        }
        if let Some(mut active) = self.active.take() {
            if active.finished.is_none() {
                match active.receipt.try_take() {
                    JobPoll::Pending => {
                        active.mailbox.drain(&self.events_tx);
                        self.active = Some(active);
                        return;
                    }
                    JobPoll::Ready(result) => {
                        let (outcome, result_retention) = result.into_parts();
                        active.finished = Some(match outcome {
                            JobOutcome::Finished(Ok(ControlFlow::Break((job, error)))) => {
                                let live = !job.cancellation.load(Ordering::Acquire)
                                    && self.active_generation
                                        == Some(job.request.view().generation);
                                if error.kind() == io::ErrorKind::WouldBlock && live {
                                    self.pending_retry_at = Some(now + RETRY_DELAY);
                                    self.pending = Some(job);
                                    drop(result_retention);
                                    drop(active);
                                    return;
                                }
                                drop(job);
                                if !live {
                                    Ok(())
                                } else if error.kind() == io::ErrorKind::Interrupted {
                                    Err("Smart Copy cancelled before provider body".into())
                                // Never requeue Cancelled or Shutdown work.
                                } else {
                                    Err(format!("Smart Copy provider admission failed: {error}"))
                                }
                            }
                            JobOutcome::Finished(Ok(ControlFlow::Continue(()))) => Ok(()),
                            JobOutcome::Finished(Err(error)) => Err(error),
                            JobOutcome::NotStarted { job, reason } => {
                                drop(job); // Cancelled and Shutdown are terminal even though input is recoverable.
                                Err(format!(
                                    "Smart Copy execution stopped before provider body: {reason:?}"
                                ))
                            }
                            JobOutcome::Panicked => {
                                Err("Smart Copy execution panicked; stream was not replayed".into())
                            }
                        });
                        drop(result_retention);
                    } // The callback is finished before any final mailbox drain.
                    JobPoll::Lost | JobPoll::Taken => {
                        active.finished = Some(Err(
                            "Smart Copy execution outcome unavailable; stream was not replayed"
                                .into(),
                        ));
                    }
                }
            }
            if !active.mailbox.drain(&self.events_tx) {
                self.active = Some(active);
                return;
            }
            let failure = match active.finished.take() {
                Some(Err(error)) => Some(error),
                _ if active.mailbox.failed() => Some(
                    "Smart Copy bounded output delivery failed; stream was not replayed".into(),
                ),
                _ => None,
            };
            if let Some(error) = failure {
                self.failures.push_back(SmartCopyWorkerEvent {
                    generation: active.generation,
                    pane_id: active.pane_id,
                    update: SmartCopyWorkerUpdate::Failed(error),
                    retention: Some(active.retention.clone()),
                });
            }
        }
        if self.retiring.len() >= 2 || self.failures.len() >= 3 {
            self.defer_pending(now);
            return;
        }
        if self.pending_retry_at.is_some_and(|at| now < at) {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(job) = self.pending.take() else {
            self.pending_retry_at = None;
            return;
        };
        self.pending_retry_at = None;
        if job.cancellation.load(Ordering::Acquire)
            || self.active_generation != Some(job.request.view().generation)
        {
            return;
        }
        let CopyJob {
            request,
            cancellation,
            events_tx: mailbox,
            limiter,
            #[cfg(test)]
            test_provider,
            #[cfg(test)]
            test_probe,
        } = job;
        let (request, retention) = request.into_parts();
        let generation = request.generation;
        let pane_id = request.pane_id;
        let job = CopyJob {
            request: retention.clone().retain(request),
            cancellation: Arc::clone(&cancellation),
            events_tx: mailbox.clone(),
            limiter,
            #[cfg(test)]
            test_provider,
            #[cfg(test)]
            test_probe,
        };
        match client.try_submit(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024 * 1024,
                result_bytes: 1024 * 1024,
            },
            job,
        ) {
            Ok(receipt) => {
                self.active = Some(ActiveCopy {
                    generation,
                    pane_id,
                    cancellation,
                    receipt,
                    finished: None,
                    mailbox,
                    retention,
                });
            }
            Err(rejected) => {
                if retryable_rejection(rejected.reason) {
                    self.pending_retry_at = Some(now + RETRY_DELAY);
                    self.pending = Some(rejected.value);
                    return;
                }
                self.failures.push_back(SmartCopyWorkerEvent {
                    generation,
                    pane_id,
                    update: SmartCopyWorkerUpdate::Failed(format!(
                        "Smart Copy execution admission failed: {:?}",
                        rejected.reason
                    )),
                    retention: Some(retention),
                });
            }
        }
    }
    fn defer_pending(&mut self, now: Instant) {
        if self.pending.is_none() {
            self.pending_retry_at = None;
            return;
        }
        if self.pending_retry_at.is_none_or(|at| now >= at) {
            self.pending_retry_at = Some(now + RETRY_DELAY);
        }
    }
}
impl Drop for SmartCopyWorkers {
    fn drop(&mut self) {
        self.cancel();
        for active in &self.retiring {
            active.receipt.cancel();
        }
    }
}

fn retryable_rejection(reason: RejectReason) -> bool {
    matches!(
        reason,
        RejectReason::Busy
            | RejectReason::QueueFull
            | RejectReason::JobLimit
            | RejectReason::InputBytes
            | RejectReason::ResultBytes
            | RejectReason::WorkerBytes
    )
}

fn run_worker(
    request: SmartCopyWorkerRequest,
    cancellation: Arc<AtomicBool>,
    events_tx: StreamMailbox,
    retention: Retention,
    #[cfg(test)] test_provider: Option<TestProvider>,
) {
    #[cfg(test)]
    let provider: TestProvider = test_provider.unwrap_or_else(|| {
        Arc::from(ilium_inference::provider_from_settings(
            &request.inference_settings,
        ))
    });
    #[cfg(not(test))]
    let provider = ilium_inference::provider_from_settings(&request.inference_settings);
    let mut buffer = String::new();
    let mut received_characters = 0usize;
    let mut response_started = false;
    let mut protocol_failed = false;
    let generation = request.generation;
    let pane_id = request.pane_id;
    let _source_hold = &request.source_hold;
    let result = provider.stream(&request.request, &mut |event| {
        if cancellation.load(Ordering::Acquire) {
            return false;
        }
        match event {
            InferenceStreamEvent::TextDelta(delta) => {
                if !response_started {
                    response_started = true;
                    if !send_update(
                        &events_tx,
                        &cancellation,
                        &retention,
                        generation,
                        pane_id,
                        SmartCopyWorkerUpdate::ResponseStarted,
                    ) {
                        return false;
                    }
                }
                received_characters = received_characters.saturating_add(delta.chars().count());
                buffer.push_str(&delta);
                while let Some(newline) = buffer.find('\n') {
                    let mut remainder = buffer.split_off(newline + 1);
                    std::mem::swap(&mut remainder, &mut buffer);
                    let line = remainder.trim_end_matches(['\r', '\n']).trim();
                    if line.len() > MAXIMUM_JSONL_LINE_BYTES {
                        protocol_failed = true;
                        let _ = send_update(
                            &events_tx,
                            &cancellation,
                            &retention,
                            generation,
                            pane_id,
                            SmartCopyWorkerUpdate::Failed(
                                "Smart Copy JSONL record exceeded64KiB".into(),
                            ),
                        );
                        return false;
                    }
                    if !line.is_empty()
                        && line != "```"
                        && line != "```jsonl"
                        && !send_update(
                            &events_tx,
                            &cancellation,
                            &retention,
                            generation,
                            pane_id,
                            SmartCopyWorkerUpdate::JsonLine(line.to_string()),
                        )
                    {
                        return false;
                    }
                }
                if buffer.len() > MAXIMUM_JSONL_LINE_BYTES {
                    protocol_failed = true;
                    let _ = send_update(
                        &events_tx,
                        &cancellation,
                        &retention,
                        generation,
                        pane_id,
                        SmartCopyWorkerUpdate::Failed(
                            "Smart Copy JSONL record exceeded 64 KiB".to_string(),
                        ),
                    );
                    return false;
                }
                let _ = events_tx.try_send(SmartCopyWorkerEvent {
                    generation,
                    pane_id,
                    retention: Some(retention.clone()),
                    update: SmartCopyWorkerUpdate::Progress {
                        received_characters,
                    },
                });
            }
            InferenceStreamEvent::OutputTokens(tokens) => {
                if !send_update(
                    &events_tx,
                    &cancellation,
                    &retention,
                    generation,
                    pane_id,
                    SmartCopyWorkerUpdate::ExactOutputTokens(tokens),
                ) {
                    return false;
                }
            }
        }
        !cancellation.load(Ordering::Acquire)
    });

    if cancellation.load(Ordering::Acquire) || protocol_failed || events_tx.failed() {
        return;
    }
    if let Err(error) = result {
        let _ = send_update(
            &events_tx,
            &cancellation,
            &retention,
            generation,
            pane_id,
            SmartCopyWorkerUpdate::Failed(error.to_string()),
        );
        return;
    }
    if !send_update(
        &events_tx,
        &cancellation,
        &retention,
        generation,
        pane_id,
        SmartCopyWorkerUpdate::Progress {
            received_characters,
        },
    ) {
        return;
    }
    let tail = buffer.trim();
    if !tail.is_empty()
        && tail != "```"
        && tail != "```jsonl"
        && !send_update(
            &events_tx,
            &cancellation,
            &retention,
            generation,
            pane_id,
            SmartCopyWorkerUpdate::JsonLine(tail.to_string()),
        )
    {
        return;
    }
    let _ = send_update(
        &events_tx,
        &cancellation,
        &retention,
        generation,
        pane_id,
        SmartCopyWorkerUpdate::Finished,
    );
}

fn send_update(
    events_tx: &StreamMailbox,
    cancellation: &AtomicBool,
    retention: &Retention,
    generation: u64,
    pane_id: NodeId,
    update: SmartCopyWorkerUpdate,
) -> bool {
    if cancellation.load(Ordering::Acquire) {
        return false;
    }
    events_tx
        .try_send(SmartCopyWorkerEvent {
            generation,
            pane_id,
            update,
            retention: Some(retention.clone()),
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Condvar;

    #[derive(Clone, Copy)]
    enum StubScenario {
        Empty,
        UsageOnly,
        PrefixThenError,
        Error,
        Panic,
        Block,
    }

    struct StubProvider {
        scenario: StubScenario,
        calls: AtomicUsize,
        release: (Mutex<bool>, Condvar),
    }

    impl StubProvider {
        fn new(scenario: StubScenario) -> Self {
            Self {
                scenario,
                calls: AtomicUsize::new(0),
                release: (Mutex::new(false), Condvar::new()),
            }
        }

        fn release_blocked_calls(&self) {
            let (lock, available) = &self.release;
            *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            available.notify_all();
        }
    }

    impl ilium_inference::InferenceProvider for StubProvider {
        fn kind(&self) -> ilium_inference::InferenceProviderKind {
            ilium_inference::InferenceProviderKind::Ollama
        }

        fn complete(
            &self,
            _: &InferenceRequest,
        ) -> Result<ilium_inference::InferenceResponse, ilium_inference::InferenceError> {
            Err(ilium_inference::InferenceError::Configuration(
                "test stream must not call complete".into(),
            ))
        }

        fn stream(
            &self,
            _: &InferenceRequest,
            on_event: &mut dyn FnMut(InferenceStreamEvent) -> bool,
        ) -> Result<(), ilium_inference::InferenceError> {
            use ilium_inference::InferenceError;
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.scenario {
                StubScenario::Empty => Ok(()),
                StubScenario::UsageOnly => {
                    assert!(on_event(InferenceStreamEvent::OutputTokens(7)));
                    Ok(())
                }
                StubScenario::PrefixThenError => {
                    assert!(on_event(InferenceStreamEvent::TextDelta(
                        "{\"partial\":".into()
                    )));
                    Err(InferenceError::Transport("synthetic prefix failure".into()))
                }
                StubScenario::Error => Err(InferenceError::Transport(
                    "synthetic provider failure".into(),
                )),
                StubScenario::Panic => panic!("synthetic provider panic"),
                StubScenario::Block => {
                    let (lock, available) = &self.release;
                    let held = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let (released, _) = available
                        .wait_timeout_while(held, Duration::from_secs(5), |released| !*released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    assert!(*released, "bounded synthetic provider release");
                    Ok(())
                }
            }
        }
    }

    fn request(generation: u64) -> SmartCopyWorkerRequest {
        SmartCopyWorkerRequest {
            generation,
            pane_id: NodeId(7),
            inference_settings: InferenceSettings::default(),
            request: InferenceRequest::json_only(format!("synthetic request {generation}")),
            source_hold: None,
        }
    }

    fn isolated_workers(
        scenario: StubScenario,
    ) -> (
        SmartCopyWorkers,
        tokio::sync::mpsc::Receiver<SmartCopyWorkerEvent>,
        Arc<StubProvider>,
        tempfile::TempDir,
    ) {
        let root = tempfile::tempdir().expect("isolated admission root");
        let lock_path = root.path().join("provider.lock");
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        let provider = Arc::new(StubProvider::new(scenario));
        let mut workers = SmartCopyWorkers::new(sender);
        workers.test_provider = Some(provider.clone());
        workers.test_limiter = Some(Arc::new(
            crate::naming_workers::InferenceConcurrencyLimiter::new(2),
        ));
        workers.test_probe = Some(Arc::new(move || {
            ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&lock_path)
        }));
        (workers, receiver, provider, root)
    }

    fn collect_until_terminal(
        workers: &mut SmartCopyWorkers,
        receiver: &mut tokio::sync::mpsc::Receiver<SmartCopyWorkerEvent>,
    ) -> Vec<SmartCopyWorkerUpdate> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut updates = Vec::new();
        loop {
            workers.collect();
            while let Ok(event) = receiver.try_recv() {
                let terminal = matches!(
                    &event.update,
                    SmartCopyWorkerUpdate::Finished | SmartCopyWorkerUpdate::Failed(_)
                );
                updates.push(event.update);
                if terminal {
                    workers.finish(event.generation);
                    return updates;
                }
            }
            assert!(Instant::now() < deadline, "synthetic stream did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn event(update: SmartCopyWorkerUpdate) -> SmartCopyWorkerEvent {
        SmartCopyWorkerEvent {
            generation: 41,
            pane_id: NodeId(7),
            update,
            retention: None,
        }
    }
    #[test]
    fn full_receiver_preserves_order_in_owned_mailbox_without_blocking_callback() {
        let mailbox = StreamMailbox::new(None);
        let (target, mut receiver) = tokio::sync::mpsc::channel(1);
        mailbox
            .try_send(event(SmartCopyWorkerUpdate::JsonLine("first".into())))
            .unwrap();
        mailbox
            .try_send(event(SmartCopyWorkerUpdate::ExactOutputTokens(12)))
            .unwrap();
        mailbox
            .try_send(event(SmartCopyWorkerUpdate::Finished))
            .unwrap();
        assert!(!mailbox.drain(&target));
        assert!(
            matches!(receiver.try_recv().unwrap().update, SmartCopyWorkerUpdate::JsonLine(line) if line=="first")
        );
        assert!(!mailbox.drain(&target));
        assert!(matches!(
            receiver.try_recv().unwrap().update,
            SmartCopyWorkerUpdate::ExactOutputTokens(12)
        ));
        assert!(mailbox.drain(&target));
        assert!(matches!(
            receiver.try_recv().unwrap().update,
            SmartCopyWorkerUpdate::Finished
        ));
    }
    #[test]
    fn progress_coalesces_but_semantic_overflow_cannot_claim_finished() {
        let mailbox = StreamMailbox::new(None);
        for received_characters in 0..100 {
            mailbox
                .try_send(event(SmartCopyWorkerUpdate::Progress {
                    received_characters,
                }))
                .unwrap();
        }
        assert_eq!(mailbox.0.lock().unwrap().ordered.len(), 1);
        for _ in 0..2047 {
            mailbox
                .try_send(event(SmartCopyWorkerUpdate::JsonLine("{}".into())))
                .unwrap();
        }
        assert!(mailbox
            .try_send(event(SmartCopyWorkerUpdate::JsonLine("overflow".into())))
            .is_err());
        assert!(mailbox.failed());
        assert!(mailbox
            .try_send(event(SmartCopyWorkerUpdate::Finished))
            .is_err());
        let (target, mut receiver) = tokio::sync::mpsc::channel(1);
        for index in 0..2048 {
            mailbox.drain(&target);
            let delivered = receiver.try_recv().expect("retained semantic event");
            if index == 0 {
                assert!(matches!(
                    delivered.update,
                    SmartCopyWorkerUpdate::Progress {
                        received_characters: 99
                    }
                ));
            } else {
                assert!(matches!(
                    delivered.update,
                    SmartCopyWorkerUpdate::JsonLine(line) if line == "{}"
                ));
            }
        }
        assert!(mailbox.drain(&target));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn invoked_usage_empty_prefix_failure_and_panic_streams_never_replay() {
        for scenario in [
            StubScenario::UsageOnly,
            StubScenario::Empty,
            StubScenario::PrefixThenError,
            StubScenario::Error,
            StubScenario::Panic,
        ] {
            let (mut workers, mut receiver, provider, _root) = isolated_workers(scenario);
            workers.start(request(61)).expect("admit synthetic stream");
            let updates = collect_until_terminal(&mut workers, &mut receiver);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            assert!(workers.pending.is_none());
            assert!(workers.pending_retry_at.is_none());
            let response_started = updates
                .iter()
                .any(|update| matches!(update, SmartCopyWorkerUpdate::ResponseStarted));
            match scenario {
                StubScenario::UsageOnly => {
                    assert!(!response_started);
                    assert!(updates.iter().any(|update| matches!(
                        update,
                        SmartCopyWorkerUpdate::ExactOutputTokens(7)
                    )));
                    assert!(matches!(
                        updates.last(),
                        Some(SmartCopyWorkerUpdate::Finished)
                    ));
                }
                StubScenario::Empty => {
                    assert!(!response_started);
                    assert!(matches!(
                        updates.last(),
                        Some(SmartCopyWorkerUpdate::Finished)
                    ));
                }
                StubScenario::PrefixThenError => {
                    assert!(response_started);
                    assert!(matches!(
                        updates.last(),
                        Some(SmartCopyWorkerUpdate::Failed(_))
                    ));
                }
                StubScenario::Error | StubScenario::Panic => {
                    assert!(!response_started);
                    assert!(matches!(
                        updates.last(),
                        Some(SmartCopyWorkerUpdate::Failed(_))
                    ));
                }
                StubScenario::Block => unreachable!("blocking fixture is separate"),
            }
            std::thread::sleep(RETRY_DELAY + Duration::from_millis(20));
            workers.collect();
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn refused_host_probe_arms_a_date_without_self_retry_or_provider_call() {
        let (mut workers, mut receiver, provider, _root) = isolated_workers(StubScenario::Empty);
        let probes = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&probes);
        workers.test_probe = Some(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }));
        let mut original = request(62);
        original.inference_settings.instructions.smart_copy = "authored instruction".into();
        let pointer = original.request.user_prompt.as_ptr();
        workers.start(original).expect("admit desired original");
        for expected_probe in 1..=2 {
            let settle_by = Instant::now() + Duration::from_secs(5);
            while workers.pending_retry_at.is_none() || workers.active.is_some() {
                workers.collect();
                assert!(Instant::now() < settle_by, "host refusal did not settle");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(probes.load(Ordering::SeqCst), expected_probe);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            let pending = workers.pending.as_ref().expect("same pending stream");
            assert_eq!(pending.request.view().request.user_prompt.as_ptr(), pointer);
            assert_eq!(
                pending
                    .request
                    .view()
                    .inference_settings
                    .instructions
                    .smart_copy,
                "authored instruction"
            );
            let remaining = workers.retry_delay(Instant::now()).expect("dated retry");
            if remaining > Duration::ZERO {
                workers.collect();
                assert_eq!(probes.load(Ordering::SeqCst), expected_probe);
            }
            if expected_probe == 1 {
                std::thread::sleep(remaining + Duration::from_millis(1));
                workers.collect();
            }
        }
        assert!(receiver.try_recv().is_err());
        workers.cancel();
        assert!(workers.pending.is_none());
        assert!(workers.retry_delay(Instant::now()).is_none());
    }

    #[test]
    fn unsafe_host_namespace_error_is_terminal_before_stream_invocation() {
        let (mut workers, mut receiver, provider, _root) = isolated_workers(StubScenario::Empty);
        workers.test_probe = Some(Arc::new(|| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic unsafe slot namespace",
            ))
        }));
        workers
            .start(request(63))
            .expect("admit original before preflight");
        let updates = collect_until_terminal(&mut workers, &mut receiver);
        assert!(matches!(
            updates.last(),
            Some(SmartCopyWorkerUpdate::Failed(_))
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(workers.pending.is_none());
        assert!(workers.pending_retry_at.is_none());
    }

    #[test]
    fn two_retiring_blocked_bodies_bound_new_stream_admission_until_real_exit() {
        let (mut workers, _receiver, provider, root) = isolated_workers(StubScenario::Block);
        let next_slot = Arc::new(AtomicUsize::new(0));
        let slot = Arc::clone(&next_slot);
        let root_path = root.path().to_path_buf();
        workers.test_probe = Some(Arc::new(move || {
            let number = slot.fetch_add(1, Ordering::SeqCst);
            if number >= 2 {
                return Ok(None);
            }
            ilium_platform::file_lock::ExclusiveFileLock::try_acquire(
                &root_path.join(format!("blocked-{number}.lock")),
            )
        }));
        workers.start(request(71)).expect("first stream");
        let deadline = Instant::now() + Duration::from_secs(5);
        while provider.calls.load(Ordering::SeqCst) < 1 {
            workers.collect();
            assert!(Instant::now() < deadline, "first body did not enter");
            std::thread::sleep(Duration::from_millis(1));
        }
        workers
            .start(request(72))
            .expect("second stream retires first");
        while provider.calls.load(Ordering::SeqCst) < 2 {
            workers.collect();
            assert!(Instant::now() < deadline, "second body did not enter");
            std::thread::sleep(Duration::from_millis(1));
        }
        let third = request(73);
        let pointer = third.request.user_prompt.as_ptr();
        let rejected = workers
            .start(third)
            .expect_err("two retiring bodies block third");
        assert_eq!(rejected.reason, RejectReason::JobLimit);
        assert_eq!(rejected.value.request.user_prompt.as_ptr(), pointer);
        assert_eq!(workers.retiring.len(), 2);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        provider.release_blocked_calls();
        while !workers.retiring.is_empty() {
            workers.collect();
            assert!(Instant::now() < deadline, "retiring bodies did not exit");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn full_terminal_delivery_queue_returns_new_original_without_starting_it() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(event(SmartCopyWorkerUpdate::JsonLine(
                "receiver occupied".into(),
            )))
            .expect("occupy receiver");
        let mut workers = SmartCopyWorkers::new(sender);
        for generation in 0..3 {
            workers.failures.push_back(SmartCopyWorkerEvent {
                generation,
                pane_id: NodeId(7),
                update: SmartCopyWorkerUpdate::Failed("retained failure".into()),
                retention: None,
            });
        }
        let original = request(81);
        let pointer = original.request.user_prompt.as_ptr();
        let rejected = workers
            .start(original)
            .expect_err("full terminal queue bounds starts");
        assert_eq!(rejected.reason, RejectReason::JobLimit);
        assert_eq!(rejected.value.request.user_prompt.as_ptr(), pointer);
        assert_eq!(workers.failures.len(), 3);
        assert!(workers.active.is_none());
        assert!(workers.pending.is_none());
        assert!(matches!(
            receiver.try_recv().expect("original receiver event").update,
            SmartCopyWorkerUpdate::JsonLine(_)
        ));
    }
    #[test]
    fn oversized_physical_capture_returns_original_before_provider_execution() {
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        let mut workers = SmartCopyWorkers::new(sender);
        let mut prompt = String::with_capacity(9 * 1024 * 1024);
        prompt.push_str("original authored request");
        let request = SmartCopyWorkerRequest {
            generation: 17,
            pane_id: NodeId(2),
            inference_settings: InferenceSettings::default(),
            request: InferenceRequest::json_only(prompt),
            source_hold: None,
        };
        let pointer = request.request.user_prompt.as_ptr();
        let rejected = workers.start(request).unwrap_err();
        assert_eq!(rejected.reason, RejectReason::InvalidCost);
        assert_eq!(rejected.value.request.user_prompt.as_ptr(), pointer);
        assert_eq!(
            rejected.value.request.user_prompt,
            "original authored request"
        );
        assert!(workers.active.is_none());
        assert!(workers.pending.is_none());
    }
}
