//! Bounded waveform preparation for the interactive Sound Studio.
//!
//! One CPU job may run and one latest request may wait. New edits replace the
//! waiting request and cooperatively cancel the obsolete callback. The result
//! retains its shared admission for as long as the waveform is displayed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ilium_execution::{
    Client, ClientLimits, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt,
    RejectReason, Retained, SkipReason,
};
use tokio::sync::Notify;

use super::studio::{PreparedSoundPreview, SoundPreviewRequest};

const PREVIEW_COLUMNS: usize = 120;
const PREVIEW_RESULT_BYTES: usize =
    PREVIEW_COLUMNS * std::mem::size_of::<ilium_sound::WaveformColumn>();
// The normalized design renders at most 3 s of 44.1 kHz mono i16 samples.
const MAX_PCM_BYTES: usize = 44_100 * 3 * std::mem::size_of::<i16>();
// Covers the transient waveform-column vector, Vec bookkeeping, and fixed-size
// design/job state while PCM remains live inside waveform_preview.
const WORKING_BYTES: usize = MAX_PCM_BYTES + 64 * 1024;
const RETRY_DELAY: Duration = Duration::from_millis(25);
const JOB_COST: JobCost = JobCost {
    input_bytes: WORKING_BYTES,
    result_bytes: PREVIEW_RESULT_BYTES,
};

pub(crate) const fn limits() -> ClientLimits {
    ClientLimits {
        jobs: 1,
        service_jobs: 0,
        input_bytes: WORKING_BYTES,
        result_bytes: PREVIEW_RESULT_BYTES,
    }
}

#[derive(Clone)]
struct PreviewKey {
    identity: Arc<()>,
    revision: u64,
}

impl PreviewKey {
    fn matches(&self, request: &SoundPreviewRequest) -> bool {
        self.revision == request.revision && Arc::ptr_eq(&self.identity, &request.studio_identity)
    }

    fn from_request(request: &SoundPreviewRequest) -> Self {
        Self {
            identity: Arc::clone(&request.studio_identity),
            revision: request.revision,
        }
    }
}

struct PreviewJob {
    request: SoundPreviewRequest,
    #[cfg(test)]
    worker_probe: std::sync::mpsc::Sender<std::thread::ThreadId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewFailure {
    Cancelled,
    WorkerPanicked,
    ReceiptLost,
    Admission(RejectReason),
}

impl Job for PreviewJob {
    type Output = PreparedSoundPreview;
    type Error = PreviewFailure;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        #[cfg(test)]
        let _ = self.worker_probe.send(std::thread::current().id());
        if context.stop_requested() {
            return Err(PreviewFailure::Cancelled);
        }
        let Some(columns) =
            ilium_sound::try_waveform_preview(&self.request.design, PREVIEW_COLUMNS, || {
                context.stop_requested()
            })
        else {
            return Err(PreviewFailure::Cancelled);
        };
        Ok(PreparedSoundPreview {
            studio_identity: self.request.studio_identity,
            revision: self.request.revision,
            columns,
        })
    }
}

struct Running {
    key: PreviewKey,
    receipt: Receipt<PreviewJob>,
}

pub(crate) struct SoundStudioPreview {
    client: Client,
    notification: Arc<Notify>,
    desired: Option<PreviewKey>,
    pending: Option<SoundPreviewRequest>,
    running: Option<Running>,
    ready: Option<Retained<PreparedSoundPreview>>,
    failure: Option<(PreviewKey, PreviewFailure)>,
    retry_at: Option<Instant>,
    closing: bool,
    #[cfg(test)]
    worker_probe: std::sync::mpsc::Sender<std::thread::ThreadId>,
    #[cfg(test)]
    worker_probe_rx: std::sync::mpsc::Receiver<std::thread::ThreadId>,
}

impl SoundStudioPreview {
    pub(crate) fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = Arc::clone(&notification);
        #[cfg(test)]
        let (worker_probe, worker_probe_rx) = std::sync::mpsc::channel();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            desired: None,
            pending: None,
            running: None,
            ready: None,
            failure: None,
            retry_at: None,
            closing: false,
            #[cfg(test)]
            worker_probe,
            #[cfg(test)]
            worker_probe_rx,
        }
    }

    pub(crate) fn notification(&self) -> Arc<Notify> {
        Arc::clone(&self.notification)
    }

    pub(crate) fn request(&mut self, request: SoundPreviewRequest, now: Instant) -> bool {
        if self.closing {
            return false;
        }
        let key = PreviewKey::from_request(&request);
        if self
            .desired
            .as_ref()
            .is_some_and(|desired| same_key(desired, &key))
        {
            if self
                .running
                .as_ref()
                .is_some_and(|running| same_key(&running.key, &key))
                || self
                    .ready
                    .as_ref()
                    .is_some_and(|ready| same_result(ready.view(), &key))
            {
                return false;
            }
        } else {
            self.desired = Some(key.clone());
            self.ready = None;
            self.failure = None;
            self.retry_at = None;
        }

        if self
            .running
            .as_ref()
            .is_some_and(|running| !same_key(&running.key, &key))
        {
            self.running
                .as_ref()
                .expect("checked running owner")
                .receipt
                .cancel();
        }
        self.pending = Some(request);
        self.start_pending(now)
    }

    pub(crate) fn collect(&mut self, now: Instant) -> bool {
        let poll = match self.running.as_mut() {
            Some(running) => running.receipt.try_take(),
            None => return self.start_pending(now),
        };
        match poll {
            JobPoll::Pending => false,
            JobPoll::Lost | JobPoll::Taken => {
                let running = self.running.take().expect("polled running owner");
                self.fail_if_desired(running.key, PreviewFailure::ReceiptLost);
                self.start_pending(now) | true
            }
            JobPoll::Ready(outcome) => {
                let running = self.running.take().expect("completed running owner");
                let (outcome, retention) = outcome.into_parts();
                match outcome {
                    JobOutcome::Finished(Ok(prepared)) => {
                        let prepared = retention.retain(prepared);
                        if !self.closing
                            && self
                                .desired
                                .as_ref()
                                .is_some_and(|key| same_result(prepared.view(), key))
                        {
                            self.ready = Some(prepared);
                        } else {
                            drop(prepared);
                        }
                    }
                    JobOutcome::Finished(Err(issue)) => {
                        drop(retention);
                        self.fail_if_desired(running.key, issue);
                    }
                    JobOutcome::NotStarted { job, reason } => {
                        drop(retention);
                        if !self.closing
                            && self
                                .desired
                                .as_ref()
                                .is_some_and(|key| key.matches(&job.request))
                        {
                            if reason == SkipReason::Shutdown {
                                self.fail_if_desired(running.key, PreviewFailure::Cancelled);
                            } else {
                                self.pending = Some(job.request);
                            }
                        }
                    }
                    JobOutcome::Panicked => {
                        drop(retention);
                        self.fail_if_desired(running.key, PreviewFailure::WorkerPanicked);
                    }
                }
                self.start_pending(now) | true
            }
        }
    }

    pub(crate) fn take_ready(&mut self) -> Option<Retained<PreparedSoundPreview>> {
        self.ready.take()
    }

    pub(crate) fn take_failure(&mut self) -> Option<PreviewFailure> {
        self.failure.take().and_then(|(failed, issue)| {
            self.desired
                .as_ref()
                .filter(|desired| same_key(failed, desired))
                .map(|_| issue)
        })
    }

    pub(crate) fn retry_delay(&self, now: Instant) -> Option<Duration> {
        self.retry_at.map(|at| at.saturating_duration_since(now))
    }

    pub(crate) fn close(&mut self) {
        self.closing = true;
        self.desired = None;
        self.pending = None;
        self.ready = None;
        self.retry_at = None;
        if let Some(running) = &self.running {
            running.receipt.cancel();
        }
        self.collect(Instant::now());
    }

    pub(crate) fn is_settled(&self) -> bool {
        self.pending.is_none() && self.running.is_none() && self.ready.is_none()
    }

    fn start_pending(&mut self, now: Instant) -> bool {
        if self.closing || self.running.is_some() || self.retry_at.is_some_and(|retry| retry > now)
        {
            return false;
        }
        self.retry_at = None;
        let Some(request) = self.pending.take() else {
            return false;
        };
        if !self
            .desired
            .as_ref()
            .is_some_and(|key| key.matches(&request))
        {
            return true;
        }
        let key = PreviewKey::from_request(&request);
        let reservation = match self.client.try_reserve(Lane::Cpu, JOB_COST) {
            Ok(reservation) => reservation,
            Err(reason) => {
                self.pending = Some(request);
                self.retry_at = Some(now + RETRY_DELAY);
                if !retryable(reason) {
                    self.failure = self
                        .desired
                        .clone()
                        .map(|key| (key, PreviewFailure::Admission(reason)));
                }
                return true;
            }
        };
        match reservation.submit(PreviewJob {
            request,
            #[cfg(test)]
            worker_probe: self.worker_probe.clone(),
        }) {
            Ok(receipt) => self.running = Some(Running { key, receipt }),
            Err(rejected) if retryable(rejected.reason) => {
                self.pending = Some(rejected.value.request);
                self.retry_at = Some(now + RETRY_DELAY);
            }
            Err(rejected) => {
                self.failure = Some((key, PreviewFailure::Admission(rejected.reason)));
            }
        }
        true
    }

    fn fail_if_desired(&mut self, key: PreviewKey, issue: PreviewFailure) {
        if !self.closing
            && self
                .desired
                .as_ref()
                .is_some_and(|desired| same_key(desired, &key))
        {
            self.failure = Some((key, issue));
            self.ready = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };

    fn execution() -> (Execution, QuotaGroup, Client) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 2 * 1024 * 1024,
            result_bytes: 1024 * 1024,
            worker_threads: 1,
            worker_bytes: 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 64 * 1024,
                },
                io: disabled,
                service: disabled,
            },
        )
        .expect("preview CPU worker");
        let client = execution.client(limits()).expect("preview client");
        (execution, quota, client)
    }

    #[test]
    fn waveform_is_prepared_on_cpu_worker_and_remains_charged_until_replaced() {
        let caller_thread = std::thread::current().id();
        let (mut execution, quota, client) = execution();
        let mut preview = SoundStudioPreview::new(client);
        let settings = ilium_sound::SoundSettings::default();
        let mut studio = super::super::studio::SoundStudio::new(settings);
        let request = studio.preview_request();
        assert!(preview.request(request.clone(), Instant::now()));

        let retained = (0..400)
            .find_map(|_| {
                let now = Instant::now();
                preview.collect(now);
                let result = preview.take_ready();
                if result.is_none() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                result
            })
            .expect("bounded preview should complete");

        let worker_thread = preview
            .worker_probe_rx
            .try_recv()
            .expect("worker callback reports its OS thread");
        assert_ne!(worker_thread, caller_thread);
        assert!(studio.install_preview(retained));
        assert_eq!(
            studio.preview_columns(),
            ilium_sound::waveform_preview(&request.design, PREVIEW_COLUMNS)
        );
        let charged = quota.snapshot();
        assert_eq!(charged.jobs, 1);
        assert_eq!(charged.input_bytes, WORKING_BYTES);
        assert_eq!(charged.result_bytes, PREVIEW_RESULT_BYTES);

        studio.changed();
        let released = quota.snapshot();
        assert_eq!(released.jobs, 0);
        assert_eq!(released.input_bytes, 0);
        assert_eq!(released.result_bytes, 0);

        preview.close();
        drop(preview);
        execution.request_shutdown(ShutdownMode::Drain);
        execution
            .join_until_background(Instant::now() + Duration::from_secs(2))
            .expect("preview CPU worker joins");
    }
}

fn same_key(left: &PreviewKey, right: &PreviewKey) -> bool {
    left.revision == right.revision && Arc::ptr_eq(&left.identity, &right.identity)
}

fn same_result(result: &PreparedSoundPreview, key: &PreviewKey) -> bool {
    result.revision == key.revision && Arc::ptr_eq(&result.studio_identity, &key.identity)
}

fn retryable(reason: RejectReason) -> bool {
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
