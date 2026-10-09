//! Ordered durability admission over the shared finite I/O bank. There is
//! only one running write; no pool callback waits for another callback.
use ilium_execution::{
    AdmissionFailure, Client, Job, JobCost, JobOutcome, JobPoll, Lane, Receipt, RejectReason,
    Rejected, Reservation, Retained,
};
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_WRITES: usize = 32;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteId(pub u64);

/// Admission evidence from this writer's own bounded queue or the actual
/// rejecting execution ledger. No later usage snapshot is substituted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterAdmissionFailure {
    RetainedBytes {
        requested: usize,
        used: usize,
        limit: usize,
    },
    Writes {
        requested: usize,
        used: usize,
        limit: usize,
    },
    ByteOverflow {
        input_bytes: usize,
        result_bytes: usize,
        retained_bytes: usize,
    },
    Closed,
    IdentifierExhausted,
    Execution(AdmissionFailure),
}
impl WriterAdmissionFailure {
    /// Compatibility for reason-only consumers; detailed callers retain provenance.
    pub fn reason(self) -> RejectReason {
        match self {
            Self::RetainedBytes { .. } | Self::ByteOverflow { .. } => RejectReason::InputBytes,
            Self::Writes { .. } | Self::IdentifierExhausted => RejectReason::JobLimit,
            Self::Closed => RejectReason::Closed,
            Self::Execution(failure) => failure.reason,
        }
    }
}
pub struct WriterRejected<J> {
    pub failure: WriterAdmissionFailure,
    pub value: J,
}
impl<J> std::fmt::Debug for WriterRejected<J> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WriterRejected")
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}
// Preserve the existing owned-job and build-before-capture refusal order:
// owned enqueue checked identifiers after execution admission; lazy enqueue
// checked them before capture/admission. The actual quota algorithm is shared.
#[derive(Clone, Copy)]
enum IdentifierCheck {
    BeforeExecution,
    AfterExecution,
}
struct WriteAdmission {
    bytes: usize,
    id: WriteId,
    reservation: Reservation,
}

pub enum WriteCompletion<J: Job> {
    Outcome {
        id: WriteId,
        outcome: Retained<JobOutcome<J>>,
    },
    /// The accepted adapter command did not run. Its exact input is returned;
    /// the caller must report refusal or retain it, never claim it was saved.
    Rejected { id: WriteId, rejection: Rejected<J> },
    /// Effects are unknown, so automatic retry could duplicate a write.
    Lost { id: WriteId },
}

struct Queued<J> {
    id: WriteId,
    job: J,
    cost: JobCost,
    reservation: Option<Reservation>,
}

pub struct OrderedWriter<J: Job> {
    client: Client,
    ready: Arc<tokio::sync::Notify>,
    queued: VecDeque<Queued<J>>,
    running: Option<(WriteId, JobCost, Receipt<J>)>,
    next_id: u64,
    bytes: usize,
    closing: bool,
    job_ready: fn(&J) -> bool,
    max_writes: usize,
    max_retained_bytes: usize,
}

impl<J: Job> OrderedWriter<J> {
    pub fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        Self::with_completion_targets(client, ready, None)
    }
    /// A typed adapter may keep an accepted FIFO head awaiting CPU preparation.
    /// This bounded predicate runs before submitting any I/O job, never in one.
    pub fn new_with_readiness(
        client: Client,
        ready: Arc<tokio::sync::Notify>,
        job_ready: fn(&J) -> bool,
    ) -> Self {
        let mut writer = Self::new(client, ready);
        writer.job_ready = job_ready;
        writer
    }
    /// Configure a separate, explicitly bounded FIFO for payloads whose
    /// retained domain limit differs from ordinary editor snapshots.
    pub fn new_with_readiness_and_limits(
        client: Client,
        ready: Arc<tokio::sync::Notify>,
        job_ready: fn(&J) -> bool,
        max_writes: usize,
        max_retained_bytes: usize,
    ) -> Self {
        let mut writer = Self::new_with_readiness(client, ready, job_ready);
        writer.max_writes = max_writes.max(1);
        writer.max_retained_bytes = max_retained_bytes.max(1);
        writer
    }
    /// Install before any admission. The actor callback must capture its original
    /// admitted ownership and perform only a bounded, nonblocking wake hint.
    /// This explicit two-target fanout replaces the client's prior callback;
    /// it does not chain arbitrary execution callbacks.
    pub fn new_with_actor_wake(
        client: Client,
        ready: Arc<tokio::sync::Notify>,
        actor_wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self::with_completion_targets(client, ready, Some(actor_wake))
    }
    fn with_completion_targets(
        client: Client,
        ready: Arc<tokio::sync::Notify>,
        actor_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let wake = Arc::clone(&ready);
        Self {
            client: client.with_completion_wake(move || {
                wake.notify_one();
                if let Some(actor_wake) = &actor_wake {
                    actor_wake();
                }
            }),
            ready,
            queued: VecDeque::new(),
            running: None,
            next_id: 0,
            bytes: 0,
            closing: false,
            job_ready: |_| true,
            max_writes: MAX_WRITES,
            max_retained_bytes: MAX_RETAINED_BYTES,
        }
    }
    pub fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.ready)
    }
    pub fn pending(&self) -> usize {
        self.queued.len() + usize::from(self.running.is_some())
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
    /// Reserve before copying a large immutable editor/board snapshot.
    pub fn enqueue_with(
        &mut self,
        cost: JobCost,
        build: impl FnOnce() -> J,
    ) -> Result<WriteId, RejectReason> {
        self.enqueue_with_detailed(cost, build)
            .map_err(WriterAdmissionFailure::reason)
    }
    pub fn enqueue_with_detailed(
        &mut self,
        cost: JobCost,
        build: impl FnOnce() -> J,
    ) -> Result<WriteId, WriterAdmissionFailure> {
        let admission = self.admit(cost, IdentifierCheck::BeforeExecution)?;
        // Rejection is complete before this potentially expensive capture.
        let job = build();
        Ok(self.commit(cost, job, admission))
    }
    /// Admission means saving, never saved. Costs must include physical input
    /// capacities, peak scratch and success/error result capacities.
    pub fn enqueue(&mut self, cost: JobCost, job: J) -> Result<WriteId, Rejected<J>> {
        self.enqueue_detailed(cost, job)
            .map_err(|rejected| Rejected {
                reason: rejected.failure.reason(),
                value: rejected.value,
            })
    }
    pub fn enqueue_detailed(
        &mut self,
        cost: JobCost,
        job: J,
    ) -> Result<WriteId, WriterRejected<J>> {
        let admission = match self.admit(cost, IdentifierCheck::AfterExecution) {
            Ok(admission) => admission,
            Err(failure) => {
                return Err(WriterRejected {
                    failure,
                    value: job,
                });
            }
        };
        Ok(self.commit(cost, job, admission))
    }
    fn admit(
        &self,
        cost: JobCost,
        identifier_check: IdentifierCheck,
    ) -> Result<WriteAdmission, WriterAdmissionFailure> {
        let overflow = || WriterAdmissionFailure::ByteOverflow {
            input_bytes: cost.input_bytes,
            result_bytes: cost.result_bytes,
            retained_bytes: self.bytes,
        };
        let requested = cost
            .input_bytes
            .checked_add(cost.result_bytes)
            .ok_or_else(overflow)?;
        let bytes = self.bytes.checked_add(requested).ok_or_else(overflow)?;
        if self.closing {
            return Err(WriterAdmissionFailure::Closed);
        }
        let used = self.pending();
        if used >= self.max_writes {
            return Err(WriterAdmissionFailure::Writes {
                requested: 1,
                used,
                limit: self.max_writes,
            });
        }
        if bytes > self.max_retained_bytes {
            return Err(WriterAdmissionFailure::RetainedBytes {
                requested,
                used: self.bytes,
                limit: self.max_retained_bytes,
            });
        }
        let next_id = self.next_id.checked_add(1);
        if matches!(identifier_check, IdentifierCheck::BeforeExecution) && next_id.is_none() {
            return Err(WriterAdmissionFailure::IdentifierExhausted);
        }
        let reservation = self
            .client
            .try_reserve_detailed(Lane::Io, cost)
            .map_err(WriterAdmissionFailure::Execution)?;
        let next_id = next_id.ok_or(WriterAdmissionFailure::IdentifierExhausted)?;
        Ok(WriteAdmission {
            bytes,
            id: WriteId(next_id),
            reservation,
        })
    }
    fn commit(&mut self, cost: JobCost, job: J, admission: WriteAdmission) -> WriteId {
        self.next_id = admission.id.0;
        self.bytes = admission.bytes;
        self.queued.push_back(Queued {
            id: admission.id,
            job,
            cost,
            reservation: Some(admission.reservation),
        });
        self.ready.notify_one();
        admission.id
    }
    fn release_cost(&mut self, cost: JobCost) {
        self.bytes -= cost.input_bytes + cost.result_bytes;
    }
    /// Nonblocking. The caller consumes each durable outcome before polling
    /// again, ensuring reconciliation precedes admission of the next write.
    pub fn poll(&mut self) -> Option<WriteCompletion<J>> {
        if let Some((id, cost, receipt)) = self.running.as_mut() {
            match receipt.try_take() {
                JobPoll::Pending => return None,
                JobPoll::Ready(outcome) => {
                    let id = *id;
                    let cost = *cost;
                    self.running = None;
                    self.release_cost(cost);
                    return Some(WriteCompletion::Outcome { id, outcome });
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let id = *id;
                    let cost = *cost;
                    self.running = None;
                    self.release_cost(cost);
                    return Some(WriteCompletion::Lost { id });
                }
            }
        }
        // An accepted source capture cannot be overtaken by younger saves.
        // No I/O lane is occupied waiting for its CPU loan. The source owner
        // signals this same notification on readiness or explicit cancellation.
        if !(self.job_ready)(&self.queued.front()?.job) {
            return None;
        }
        let mut queued = self.queued.pop_front()?;
        let reservation = match queued.reservation.take() {
            Some(reservation) => reservation,
            None => match self.client.try_reserve(Lane::Io, queued.cost) {
                Ok(reservation) => reservation,
                Err(
                    RejectReason::Busy
                    | RejectReason::QueueFull
                    | RejectReason::JobLimit
                    | RejectReason::InputBytes
                    | RejectReason::ResultBytes,
                ) => {
                    self.queued.push_front(queued);
                    // The process admission wake (or normal UI tick) retries
                    // after resource release; never self-wake a busy loop.
                    return None;
                }
                Err(reason) => {
                    self.release_cost(queued.cost);
                    return Some(WriteCompletion::Rejected {
                        id: queued.id,
                        rejection: Rejected {
                            reason,
                            value: queued.job,
                        },
                    });
                }
            },
        };
        match reservation.submit(queued.job) {
            Ok(receipt) => {
                self.running = Some((queued.id, queued.cost, receipt));
                None
            }
            Err(rejection) if rejection.reason == RejectReason::Busy => {
                queued.job = rejection.value;
                self.queued.push_front(queued);
                self.ready.notify_one();
                None
            }
            Err(rejection) => {
                self.release_cost(queued.cost);
                Some(WriteCompletion::Rejected {
                    id: queued.id,
                    rejection,
                })
            }
        }
    }
    /// Prevent new writes; admitted commands still drain in order. Do not
    /// cancel running durable jobs or close the shared bank before draining.
    pub fn close_admission(&mut self) {
        self.closing = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, JobContext, LaneConfig, QuotaGroup, QuotaLimits,
        ShutdownMode,
    };
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct Write {
        path: std::path::PathBuf,
        text: &'static str,
        entered: mpsc::SyncSender<&'static str>,
        gate: Option<mpsc::Receiver<()>>,
    }

    struct BlockIo {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Job for BlockIo {
        type Output = ();
        type Error = ();
        fn run(self, _context: JobContext) -> Result<(), ()> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }
    }
    impl Job for Write {
        type Output = &'static str;
        type Error = String;
        fn run(self, _context: JobContext) -> Result<Self::Output, String> {
            self.entered.send(self.text).unwrap();
            if let Some(gate) = self.gate {
                gate.recv().unwrap();
            }
            std::fs::write(self.path, self.text).map_err(|error| error.to_string())?;
            Ok(self.text)
        }
    }
    fn execution() -> Execution {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 4,
            jobs: 32,
            service_jobs: 0,
            input_bytes: 1024 * 1024,
            result_bytes: 1024 * 1024,
            worker_threads: 3,
            worker_bytes: 1024 * 1024,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: 8,
            priority: None,
            resident_bytes_per_thread: 4096,
        };
        Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane(1),
                io: lane(2),
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap()
    }
    fn completion(writer: &mut OrderedWriter<Write>) -> WriteCompletion<Write> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(outcome) = writer.poll() {
                return outcome;
            }
            assert!(Instant::now() < deadline, "Ordered write did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn blocked_write_and_shutdown_preserve_order_and_authoritative_readback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("authored.txt");
        let mut execution = execution();
        let client = execution
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 1024 * 1024,
                result_bytes: 1024 * 1024,
            })
            .unwrap();
        let mut writer = OrderedWriter::new(client, Arc::new(tokio::sync::Notify::new()));
        let (started, observed) = mpsc::sync_channel(4);
        let (release, gate) = mpsc::sync_channel(1);
        let cost = JobCost {
            input_bytes: 8192,
            result_bytes: 8192,
        };
        let first = writer
            .enqueue(
                cost,
                Write {
                    path: path.clone(),
                    text: "FIRST",
                    entered: started.clone(),
                    gate: Some(gate),
                },
            )
            .unwrap();
        let second = writer
            .enqueue(
                cost,
                Write {
                    path: path.clone(),
                    text: "SECOND",
                    entered: started.clone(),
                    gate: None,
                },
            )
            .unwrap();
        writer.close_admission();
        let rejected = writer
            .enqueue(
                cost,
                Write {
                    path: path.clone(),
                    text: "UNACCEPTED",
                    entered: started,
                    gate: None,
                },
            )
            .unwrap_err();
        assert_eq!(rejected.reason, RejectReason::Closed);
        assert_eq!(rejected.value.text, "UNACCEPTED");
        assert!(writer.poll().is_none());
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(3)).unwrap(),
            "FIRST"
        );
        assert!(writer.poll().is_none());
        assert!(
            observed.try_recv().is_err(),
            "The second I/O thread must not run a later write early"
        );
        release.send(()).unwrap();
        let WriteCompletion::Outcome { id, outcome } = completion(&mut writer) else {
            panic!("Durable result missing");
        };
        assert_eq!(id, first);
        assert!(matches!(outcome.view(), JobOutcome::Finished(Ok("FIRST"))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "FIRST");
        assert!(
            observed.try_recv().is_err(),
            "Reconciliation precedes the next write"
        );
        drop(outcome);
        let WriteCompletion::Outcome { id, outcome } = completion(&mut writer) else {
            panic!("Second durable result missing");
        };
        assert_eq!(id, second);
        assert!(matches!(outcome.view(), JobOutcome::Finished(Ok("SECOND"))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "SECOND");
        drop(outcome);
        assert_eq!(writer.pending(), 0);
        assert_eq!(writer.retained_bytes(), 0);
        drop(writer);
        execution.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(3))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn dropping_owner_after_drain_deadline_does_not_discard_accepted_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("authored.txt");
        let (execution, general, filesystem, _) = evidence_execution();
        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let mut blocker = general
            .try_submit(
                Lane::Io,
                small_cost(),
                BlockIo {
                    entered: entered_sender,
                    release: release_receiver,
                },
            )
            .unwrap();
        entered_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("the single I/O worker is blocked");

        let mut writer = OrderedWriter::new(filesystem, Arc::new(tokio::sync::Notify::new()));
        let (write_started, _write_observed) = mpsc::sync_channel(2);
        for text in ["FIRST", "SECOND"] {
            writer
                .enqueue(
                    small_cost(),
                    Write {
                        path: path.clone(),
                        text,
                        entered: write_started.clone(),
                        gate: None,
                    },
                )
                .unwrap();
        }
        assert!(
            writer.poll().is_none(),
            "first write is accepted by the bank"
        );
        assert_eq!(writer.pending(), 2, "second write remains in owner custody");

        // Client teardown can outlive its drain deadline. Accepted saves must
        // remain owned somewhere that can publish them after the blocker exits.
        drop(writer);
        release_sender.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match blocker.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline, "blocking I/O job did not settle");
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(outcome) => {
                    assert!(matches!(outcome.view(), JobOutcome::Finished(Ok(()))));
                    break;
                }
                JobPoll::Lost | JobPoll::Taken => panic!("blocking job receipt was lost"),
            }
        }
        drop(blocker);
        drop(general);
        evidence_finish(execution);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents, "SECOND",
            "the last accepted save must survive owner teardown"
        );
    }
    const MIB: usize = 1024 * 1024;
    fn evidence_execution() -> (Execution, Client, Client, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 32,
            jobs: 64,
            service_jobs: 0,
            input_bytes: 768 * MIB,
            result_bytes: 768 * MIB,
            worker_threads: 2,
            worker_bytes: 64 * MIB,
        });
        let lane = LaneConfig {
            threads: 1,
            queue_slots: 64,
            priority: None,
            resident_bytes_per_thread: MIB,
        };
        let execution = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane,
                io: lane,
                service: LaneConfig {
                    threads: 0,
                    queue_slots: 0,
                    priority: None,
                    resident_bytes_per_thread: 0,
                },
            },
        )
        .unwrap();
        let general = execution.client(evidence_limits(512 * MIB)).unwrap();
        let filesystem = general.child(evidence_limits(128 * MIB)).unwrap();
        (execution, general, filesystem, quota)
    }
    fn evidence_limits(input_bytes: usize) -> ClientLimits {
        ClientLimits {
            jobs: 60,
            service_jobs: 0,
            input_bytes,
            result_bytes: 256 * MIB,
        }
    }
    fn evidence_finish(mut execution: Execution) {
        execution.request_shutdown(ShutdownMode::Drain);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.remaining_workers, 0);
    }
    fn evidence_drain<J: Job>(writer: &mut OrderedWriter<J>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while writer.pending() != 0 {
            if let Some(completion) = writer.poll() {
                match completion {
                    WriteCompletion::Outcome { outcome, .. } => {
                        assert!(
                            matches!(outcome.view(), JobOutcome::Finished(Ok(_))),
                            "accepted command must finish on the actual IO bank"
                        );
                    }
                    WriteCompletion::Rejected { rejection, .. } => {
                        panic!("accepted command rejected: {:?}", rejection.reason)
                    }
                    WriteCompletion::Lost { .. } => panic!("accepted command receipt lost"),
                }
            }
            assert!(Instant::now() < deadline, "Ordered write did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(writer.retained_bytes(), 0);
    }
    struct OriginalJob(String);
    impl Job for OriginalJob {
        type Output = String;
        type Error = ();
        fn run(self, _context: JobContext) -> Result<String, ()> {
            Ok(self.0)
        }
    }
    fn original_job() -> OriginalJob {
        OriginalJob(String::from("original owned payload"))
    }
    fn small_cost() -> JobCost {
        JobCost {
            input_bytes: 4096,
            result_bytes: 4096,
        }
    }
    #[test]
    fn original_configuration_cost_refuses_seventh_write_locally_before_capture() {
        use super::super::configuration::{ConfigurationChange, ConfigurationWrite};
        let (execution, general, filesystem, quota) = evidence_execution();
        let directory = tempfile::tempdir().unwrap();
        let mut writer = OrderedWriter::new(filesystem, Arc::new(tokio::sync::Notify::new()));
        assert_eq!(ConfigurationWrite::COST.input_bytes, 8 * MIB);
        assert_eq!(ConfigurationWrite::COST.result_bytes, 2 * MIB);
        for _ in 0..6 {
            writer
                .enqueue_detailed(
                    ConfigurationWrite::COST,
                    ConfigurationWrite {
                        directory: directory.path().to_path_buf(),
                        change: ConfigurationChange::Separators(true),
                    },
                )
                .unwrap();
        }
        assert_eq!(writer.pending(), 6);
        assert_eq!(writer.retained_bytes(), 60 * MIB);
        let job = ConfigurationWrite {
            directory: directory.path().to_path_buf(),
            change: ConfigurationChange::Separators(false),
        };
        let original_path = job.directory.as_os_str().as_encoded_bytes().as_ptr();
        let before = quota.snapshot();
        let rejected = writer
            .enqueue_detailed(ConfigurationWrite::COST, job)
            .unwrap_err();
        assert_eq!(
            rejected.failure,
            WriterAdmissionFailure::RetainedBytes {
                requested: 10 * MIB,
                used: 60 * MIB,
                limit: 64 * MIB,
            }
        );
        assert_eq!(
            rejected
                .value
                .directory
                .as_os_str()
                .as_encoded_bytes()
                .as_ptr(),
            original_path
        );
        assert!(matches!(
            rejected.value.change,
            ConfigurationChange::Separators(false)
        ));
        assert_eq!(quota.snapshot().input_bytes, before.input_bytes);
        assert_eq!(quota.snapshot().jobs, before.jobs);
        let called = std::cell::Cell::new(false);
        let failure = writer
            .enqueue_with_detailed(ConfigurationWrite::COST, || {
                called.set(true);
                ConfigurationWrite {
                    directory: directory.path().to_path_buf(),
                    change: ConfigurationChange::Separators(false),
                }
            })
            .unwrap_err();
        assert_eq!(failure, rejected.failure);
        assert!(!called.get(), "refusal must precede the authored capture");
        assert_eq!(writer.pending(), 6);
        drop(rejected);
        evidence_drain(&mut writer);
        drop(writer);
        drop(general);
        evidence_finish(execution);
    }
    #[test]
    fn local_write_slots_preserve_original_job_and_refuse_lazy_capture() {
        let (execution, general, filesystem, _) = evidence_execution();
        let mut writer = OrderedWriter::new(filesystem, Arc::new(tokio::sync::Notify::new()));
        for _ in 0..MAX_WRITES {
            writer
                .enqueue_detailed(small_cost(), original_job())
                .unwrap();
        }
        let job = original_job();
        let pointer = job.0.as_ptr();
        let rejected = writer.enqueue_detailed(small_cost(), job).unwrap_err();
        assert_eq!(
            rejected.failure,
            WriterAdmissionFailure::Writes {
                requested: 1,
                used: 32,
                limit: 32,
            }
        );
        assert_eq!(rejected.value.0.as_ptr(), pointer);
        let called = std::cell::Cell::new(false);
        assert_eq!(
            writer
                .enqueue_with_detailed(small_cost(), || {
                    called.set(true);
                    original_job()
                })
                .unwrap_err(),
            rejected.failure
        );
        assert!(!called.get());
        drop(rejected);
        evidence_drain(&mut writer);
        drop(writer);
        drop(general);
        evidence_finish(execution);
    }
    #[test]
    fn closed_overflow_and_exhausted_identifiers_are_distinct_local_refusals() {
        let (execution, general, filesystem, quota) = evidence_execution();
        let ready = Arc::new(tokio::sync::Notify::new());
        let mut writer = OrderedWriter::new(filesystem.clone(), Arc::clone(&ready));
        let job = original_job();
        let pointer = job.0.as_ptr();
        let rejected = writer
            .enqueue_detailed(
                JobCost {
                    input_bytes: usize::MAX,
                    result_bytes: 1,
                },
                job,
            )
            .unwrap_err();
        assert_eq!(
            rejected.failure,
            WriterAdmissionFailure::ByteOverflow {
                input_bytes: usize::MAX,
                result_bytes: 1,
                retained_bytes: 0,
            }
        );
        assert_eq!(rejected.value.0.as_ptr(), pointer);
        writer
            .enqueue_detailed(small_cost(), original_job())
            .unwrap();
        let before = quota.snapshot().input_bytes;
        let rejected = writer
            .enqueue_detailed(
                JobCost {
                    input_bytes: usize::MAX,
                    result_bytes: 0,
                },
                original_job(),
            )
            .unwrap_err();
        assert_eq!(
            rejected.failure,
            WriterAdmissionFailure::ByteOverflow {
                input_bytes: usize::MAX,
                result_bytes: 0,
                retained_bytes: 8192,
            }
        );
        assert_eq!(quota.snapshot().input_bytes, before);
        writer.close_admission();
        let job = original_job();
        let pointer = job.0.as_ptr();
        let rejected = writer.enqueue_detailed(small_cost(), job).unwrap_err();
        assert_eq!(rejected.failure, WriterAdmissionFailure::Closed);
        assert_eq!(rejected.value.0.as_ptr(), pointer);
        let called = std::cell::Cell::new(false);
        assert_eq!(
            writer
                .enqueue_with_detailed(small_cost(), || {
                    called.set(true);
                    original_job()
                })
                .unwrap_err(),
            WriterAdmissionFailure::Closed
        );
        assert!(!called.get());
        evidence_drain(&mut writer);
        drop(writer);
        let mut writer = OrderedWriter::new(filesystem, ready);
        writer.next_id = u64::MAX;
        let job = original_job();
        let pointer = job.0.as_ptr();
        let before = quota.snapshot();
        let rejected = writer.enqueue_detailed(small_cost(), job).unwrap_err();
        assert_eq!(
            rejected.failure,
            WriterAdmissionFailure::IdentifierExhausted
        );
        assert_eq!(rejected.value.0.as_ptr(), pointer);
        assert_eq!(quota.snapshot().input_bytes, before.input_bytes);
        assert_eq!(quota.snapshot().jobs, before.jobs);
        assert_eq!(writer.pending(), 0);
        let called = std::cell::Cell::new(false);
        assert_eq!(
            writer
                .enqueue_with_detailed(small_cost(), || {
                    called.set(true);
                    original_job()
                })
                .unwrap_err(),
            WriterAdmissionFailure::IdentifierExhausted
        );
        assert!(!called.get());
        drop(writer);
        drop(general);
        evidence_finish(execution);
    }
    fn execution_refusal_case(boundary: ilium_execution::AdmissionBoundary) {
        use ilium_execution::{AdmissionBoundary, QuotaResource};
        let (execution, general, filesystem, quota) = evidence_execution();
        let hold_client = match boundary {
            AdmissionBoundary::Root => execution.client(evidence_limits(768 * MIB)).unwrap(),
            AdmissionBoundary::Client {
                ancestor_distance: 0,
            } => filesystem.clone(),
            AdmissionBoundary::Client {
                ancestor_distance: 1,
            } => general.clone(),
            _ => panic!("unsupported fixture boundary"),
        };
        let used = match boundary {
            AdmissionBoundary::Root => 768 * MIB,
            AdmissionBoundary::Client {
                ancestor_distance: 0,
            } => 128 * MIB,
            _ => 512 * MIB,
        };
        let hold = hold_client
            .try_reserve_external(JobCost {
                input_bytes: used,
                result_bytes: 0,
            })
            .unwrap();
        let before = quota.snapshot();
        let mut writer = OrderedWriter::new(filesystem, Arc::new(tokio::sync::Notify::new()));
        let cost = JobCost {
            input_bytes: 8 * MIB,
            result_bytes: 2 * MIB,
        };
        let job = original_job();
        let pointer = job.0.as_ptr();
        let rejected = writer.enqueue_detailed(cost, job).unwrap_err();
        let WriterAdmissionFailure::Execution(failure) = rejected.failure else {
            panic!("writer-local capacity is free; execution must identify the rejecting ledger");
        };
        assert_eq!(failure.reason, RejectReason::InputBytes);
        let evidence = failure.quota.unwrap();
        assert_eq!(evidence.boundary, boundary);
        assert_eq!(evidence.resource, QuotaResource::InputBytes);
        assert_eq!(
            (evidence.requested, evidence.used, evidence.limit),
            (8 * MIB, used, used)
        );
        assert_eq!(rejected.value.0.as_ptr(), pointer);
        assert_eq!(writer.pending(), 0);
        assert_eq!(writer.retained_bytes(), 0);
        assert_eq!(quota.snapshot().input_bytes, before.input_bytes);
        assert_eq!(quota.snapshot().jobs, before.jobs);
        let called = std::cell::Cell::new(false);
        assert_eq!(
            writer
                .enqueue_with_detailed(cost, || {
                    called.set(true);
                    original_job()
                })
                .unwrap_err(),
            rejected.failure
        );
        assert!(!called.get());
        drop(hold);
        assert_eq!(quota.snapshot().input_bytes, 0);
        assert_eq!(
            evidence.used, used,
            "captured evidence survives release unchanged"
        );
        // A later accepted original still follows normal actual IO completion.
        writer.enqueue_detailed(cost, rejected.value).unwrap();
        evidence_drain(&mut writer);
        drop(writer);
        drop(hold_client);
        drop(general);
        evidence_finish(execution);
    }
    #[test]
    fn writer_refusal_identifies_local_filesystem_execution_ledger() {
        execution_refusal_case(ilium_execution::AdmissionBoundary::Client {
            ancestor_distance: 0,
        });
    }
    #[test]
    fn writer_refusal_identifies_general_ancestor_execution_ledger() {
        execution_refusal_case(ilium_execution::AdmissionBoundary::Client {
            ancestor_distance: 1,
        });
    }
    #[test]
    fn writer_refusal_identifies_process_root_execution_ledger() {
        execution_refusal_case(ilium_execution::AdmissionBoundary::Root);
    }
    struct PreparedWriteFixture {
        original: Write,
        source_ready: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Job for PreparedWriteFixture {
        type Output = &'static str;
        type Error = String;
        fn run(self, context: JobContext) -> Result<Self::Output, String> {
            assert!(self.source_ready.load(std::sync::atomic::Ordering::Acquire));
            self.original.run(context)
        }
    }
    #[test]
    fn accepted_not_ready_head_preserves_fifo_without_occupying_io_workers() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut owner = execution();
        let client = owner
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 1024 * 1024,
                result_bytes: 1024 * 1024,
            })
            .unwrap();
        let notification = Arc::new(tokio::sync::Notify::new());
        let mut writer = OrderedWriter::new_with_readiness(
            client.clone(),
            notification.clone(),
            |job: &PreparedWriteFixture| job.source_ready.load(Ordering::Acquire),
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ordered.txt");
        let (entered, observed) = mpsc::sync_channel(2);
        let prepared = Arc::new(AtomicBool::new(false));
        let first = writer
            .enqueue(
                small_cost(),
                PreparedWriteFixture {
                    original: Write {
                        path: path.clone(),
                        text: "FIRST",
                        entered: entered.clone(),
                        gate: None,
                    },
                    source_ready: prepared.clone(),
                },
            )
            .unwrap();
        let second = writer
            .enqueue(
                small_cost(),
                PreparedWriteFixture {
                    original: Write {
                        path: path.clone(),
                        text: "SECOND",
                        entered,
                        gate: None,
                    },
                    source_ready: Arc::new(AtomicBool::new(true)),
                },
            )
            .unwrap();
        let bytes = writer.retained_bytes();
        let (cpu_entered, cpu_observed) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut blocked = client
            .try_submit(Lane::Cpu, small_cost(), move |_| {
                cpu_entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok::<_, String>(())
            })
            .unwrap();
        cpu_observed.recv_timeout(Duration::from_secs(3)).unwrap();
        let wake = notification.clone();
        let mut preparation = client
            .try_submit(Lane::Cpu, small_cost(), move |_| {
                prepared.store(true, Ordering::Release);
                wake.notify_one();
                Ok::<_, String>(())
            })
            .unwrap();
        writer.close_admission();
        assert!(writer.poll().is_none());
        assert_eq!(writer.pending(), 2);
        assert_eq!(writer.retained_bytes(), bytes);
        assert!(matches!(
            observed.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(!path.exists());
        // This real I/O job completes while CPU preparation is still blocked.
        // A callback waiting on CPU would occupy an I/O worker unnecessarily.
        let (io_entered, io_observed) = mpsc::sync_channel(1);
        let mut independent = client
            .try_submit(Lane::Io, small_cost(), move |_| {
                io_entered.send(()).unwrap();
                Ok::<_, String>(())
            })
            .unwrap();
        io_observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(writer.poll().is_none());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut completed = Vec::new();
        while completed.len() != 2 {
            if let Some(completion) = writer.poll() {
                match completion {
                    WriteCompletion::Outcome { id, outcome } => {
                        assert!(matches!(outcome.view(), JobOutcome::Finished(Ok(_))));
                        completed.push(id);
                    }
                    _ => panic!("accepted original prepared write did not finish"),
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(completed, [first, second]);
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            "FIRST"
        );
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            "SECOND"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "SECOND");
        assert_eq!(writer.retained_bytes(), 0);
        assert!(matches!(blocked.try_take(), JobPoll::Ready(_)));
        assert!(matches!(preparation.try_take(), JobPoll::Ready(_)));
        assert!(matches!(independent.try_take(), JobPoll::Ready(_)));
        drop((blocked, preparation, independent, writer, client));
        owner.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            owner
                .join_until_background(deadline)
                .unwrap()
                .remaining_workers,
            0
        );
    }
}
