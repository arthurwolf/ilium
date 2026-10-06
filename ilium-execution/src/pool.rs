use std::cell::Cell;
use std::fmt;
use std::io;
use std::mem::size_of;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use ilium_platform::owned_worker::{
    spawn_owned, OwnedWorker, StopToken, WorkerExit, WorkerKind, MAX_OWNED_WORKERS,
};
use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};

use crate::budget::{
    job_debit, job_debit_detailed, AdmissionFailure, AdmissionGroup, ClientLimits, Debit, JobCost,
    QuotaGroup, QuotaSnapshot, RejectReason, Tenant,
};
use crate::job::{
    recover_unpublished, task, ErasedJob, Job, JobHold, Metrics, Receipt, Rejected, Wake,
};
use crate::retirement::{
    RetirementFailure, RetirementReservation, RetirementState, RETIREMENT_SLOTS,
};

const OPEN: u8 = 0;
const DRAIN: u8 = 1;
const CANCEL: u8 = 2;
const IDLE_CHECK: Duration = Duration::from_millis(25);
// These are cooperative declarations, not allocator measurements. The pinned
// crossbeam bounded array stores one erased job pointer plus a stamp per slot;
// extra bytes cover alignment/waker bookkeeping and fixed bank allocations.
const BANK_FIXED_METADATA_BYTES: usize = 4096;
const BANK_QUEUE_SLOT_BYTES: usize = 128;
thread_local! {
    static ON_BANK_THREAD: Cell<bool> = const { Cell::new(false) };
    static ON_CPU_BANK_THREAD: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn on_cpu_bank_thread() -> bool {
    ON_CPU_BANK_THREAD.with(Cell::get)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Cpu,
    Io,
    Service,
}
impl Lane {
    fn index(self) -> usize {
        match self {
            Self::Cpu => 0,
            Self::Io => 1,
            Self::Service => 2,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Io => "io",
            Self::Service => "service",
        }
    }
}
const LANES: [Lane; 3] = [Lane::Cpu, Lane::Io, Lane::Service];

#[derive(Debug, Clone, Copy)]
pub struct LaneConfig {
    pub threads: usize,
    /// Waiting jobs PLUS caller-owned unsubmitted reservations, not running jobs.
    pub queue_slots: usize,
    /// Applied once on these dedicated OS threads, never on a Tokio worker.
    /// None keeps inherited priority; it does not promise real-time scheduling.
    pub priority: Option<WorkerPriority>,
    /// Declared resident worker state. OS stacks/allocator overhead are not RSS
    /// accounted by this figure; actual stack size is platform-supervisor owned.
    pub resident_bytes_per_thread: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionConfig {
    pub cpu: LaneConfig,
    pub io: LaneConfig,
    pub service: LaneConfig,
}
impl ExecutionConfig {
    fn array(self) -> [LaneConfig; 3] {
        [self.cpu, self.io, self.service]
    }
}

#[derive(Debug)]
pub enum StartError {
    InvalidConfig(&'static str),
    Admission(RejectReason),
    Allocation,
    Spawn {
        lane: Lane,
        index: usize,
        source: io::Error,
    },
}
/// Construction refusal with the SAME bank that already started, if any.
/// No worker identifiers are inferred from a process-global registry.
pub struct StartFailure {
    error: StartError,
    execution: Option<Execution>,
}
impl From<StartError> for StartFailure {
    fn from(error: StartError) -> Self {
        Self {
            error,
            execution: None,
        }
    }
}
impl fmt::Debug for StartFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartFailure")
            .field("error", &self.error)
            .field("partial_execution_retained", &self.execution.is_some())
            .finish()
    }
}
impl StartFailure {
    pub fn into_parts(self) -> (StartError, Option<Execution>) {
        (self.error, self.execution)
    }
    /// Legacy bootstrap callers still observe actual partial-bank exit. On a
    /// deadline the original Spawn source and bank stay in the typed io source.
    fn into_legacy_error(self) -> StartError {
        let (primary, execution) = self.into_parts();
        let Some(mut execution) = execution else {
            return primary;
        };
        execution.request_shutdown(ShutdownMode::Cancel);
        let observed = execution.join_until_background(Instant::now() + Duration::from_secs(5));
        if matches!(&observed, Ok(report) if report.shutdown_complete) {
            drop(execution);
            return primary;
        }
        // The only partial-construction boundary is the original spawn call.
        let StartError::Spawn {
            lane,
            index,
            source,
        } = primary
        else {
            unreachable!("partial bank exists only after a worker spawn refusal")
        };
        let kind = source.kind();
        StartError::Spawn {
            lane,
            index,
            source: io::Error::new(
                kind,
                StartCleanupError {
                    primary: StartError::Spawn {
                        lane,
                        index,
                        source,
                    },
                    state: std::sync::Mutex::new(StartCleanupState {
                        execution,
                        observed,
                    }),
                },
            ),
        }
    }
}
/// Exceptional legacy bootstrap custody; Display never substitutes for it.
pub struct StartCleanupError {
    primary: StartError,
    state: std::sync::Mutex<StartCleanupState>,
}
struct StartCleanupState {
    execution: Execution,
    observed: Result<JoinReport, JoinUseError>,
}
impl StartCleanupError {
    pub fn primary(&self) -> &StartError {
        &self.primary
    }
    pub fn observe_background(&self, deadline: Instant) -> Result<bool, JoinUseError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let report = state.execution.join_until_background(deadline)?;
        let complete = report.shutdown_complete;
        state.observed = Ok(report);
        Ok(complete)
    }
}
impl fmt::Debug for StartCleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        f.debug_struct("StartCleanupError")
            .field("primary", &self.primary)
            .field("observation", &state.observed)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for StartCleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; partial execution remains in startup custody",
            self.primary
        )
    }
}
impl std::error::Error for StartCleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StartError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownMode {
    Drain,
    Cancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Accepting,
    Draining,
    Cancelling,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinUseError {
    OnBankThread,
    StillAccepting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinObservation {
    Exited {
        worker_id: u64,
        lane: Lane,
        exit: WorkerExit,
    },
    Deadline {
        worker_id: u64,
        lane: Lane,
    },
}
#[derive(Debug)]
pub struct JoinReport {
    pub observations: Vec<JoinObservation>,
    pub remaining_workers: usize,
    pub health: Health,
    /// Physical workers have joined and no admitted work or retained original
    /// remains. A deadline or exceptional recovery custody keeps this false.
    /// This reports lifecycle completion, not successful business results.
    pub shutdown_complete: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct LaneHealth {
    pub threads_configured: usize,
    pub queue_capacity: usize,
    pub waiting_or_reserved: usize,
    pub enqueued: usize,
    pub retained_service_claims: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub panicked: usize,
    pub abandoned_receipts: usize,
    pub undelivered: usize,
    pub lost: usize,
    pub wake_panics: usize,
    /// Callback-loop exit, NOT proof of OS thread exit (TLS can still run).
    pub bodies_exited: usize,
    /// Actual platform-supervisor joins observed by join_until_background.
    pub joined: usize,
    pub thread_panics: usize,
    /// CPU-only, independent of the ordinary job and queue-slot counters.
    pub retirement_live: usize,
    pub retirement_queued: usize,
    /// Original envelopes retained for explicit recovery after a primary
    /// handoff refusal; each still occupies its original live slot and debit.
    pub retirement_recovery_pending: usize,
    pub retirement_completed: usize,
    pub retirement_panicked: usize,
    /// Count of primary handoff refusals, including unsuccessful retries. A
    /// failed original remains in finite recovery custody until resolved.
    pub retirement_handoff_failed: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct Health {
    pub phase: Phase,
    /// Whole shared group, possibly including OTHER Execution instances.
    pub quota: QuotaSnapshot,
    pub lanes: [LaneHealth; 3],
}

struct Signal {
    pending: AtomicUsize,
    enqueued: AtomicUsize,
    service_slots: Arc<AtomicUsize>,
}
impl Signal {
    fn new() -> Self {
        Self {
            pending: AtomicUsize::new(0),
            enqueued: AtomicUsize::new(0),
            service_slots: Arc::new(AtomicUsize::new(0)),
        }
    }
}

pub(crate) struct QueueSlot {
    signal: Arc<Signal>,
}
impl Drop for QueueSlot {
    fn drop(&mut self) {
        self.signal.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Bank {
    config: LaneConfig,
    queue_sender: Sender<Box<dyn ErasedJob>>,
    queue_receiver: Receiver<Box<dyn ErasedJob>>,
    signal: Arc<Signal>,
    metrics: Arc<Metrics>,
}
impl Bank {
    fn new(config: LaneConfig) -> Self {
        // Disabled banks have no admission and keep only one inert slot.
        let (queue_sender, queue_receiver) =
            crossbeam_channel::bounded::<Box<dyn ErasedJob>>(config.queue_slots.max(1));
        Self {
            config,
            queue_sender,
            queue_receiver,
            signal: Arc::new(Signal::new()),
            metrics: Arc::new(Metrics::default()),
        }
    }
    fn health(&self, retirement: Option<&RetirementState>) -> LaneHealth {
        let m = &self.metrics;
        let load = |value: &AtomicUsize| value.load(Ordering::Acquire);
        LaneHealth {
            threads_configured: self.config.threads,
            queue_capacity: self.config.queue_slots,
            waiting_or_reserved: load(&self.signal.pending),
            enqueued: load(&self.signal.enqueued),
            retained_service_claims: load(&self.signal.service_slots),
            running: load(&m.running),
            succeeded: load(&m.succeeded),
            failed: load(&m.failed),
            skipped: load(&m.skipped),
            panicked: load(&m.panicked),
            abandoned_receipts: load(&m.abandoned),
            undelivered: load(&m.undelivered),
            lost: load(&m.lost),
            wake_panics: load(&m.wake_panics),
            bodies_exited: load(&m.bodies_exited),
            joined: load(&m.joined),
            thread_panics: load(&m.thread_panics),
            retirement_live: retirement.map_or(0, RetirementState::live),
            retirement_queued: retirement.map_or(0, RetirementState::queued),
            retirement_recovery_pending: retirement.map_or(0, RetirementState::recovery_pending),
            retirement_completed: retirement.map_or(0, RetirementState::completed),
            retirement_panicked: retirement.map_or(0, RetirementState::panicked),
            retirement_handoff_failed: retirement.map_or(0, RetirementState::handoff_failed),
        }
    }
}

pub(crate) struct Shared {
    quota: QuotaGroup,
    banks: [Bank; 3],
    retirement: RetirementState,
    phase: AtomicU8,
    stop: StopToken,
    // Last field: all bank/channel storage is destroyed before this charge.
    _metadata_charge: Debit,
}
impl Shared {
    pub(crate) fn cancelling(&self) -> bool {
        self.phase.load(Ordering::Acquire) == CANCEL
    }
    fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Acquire) {
            OPEN => Phase::Accepting,
            DRAIN => Phase::Draining,
            _ => Phase::Cancelling,
        }
    }
    fn close(&self, mode: ShutdownMode) {
        let desired = match mode {
            ShutdownMode::Drain => DRAIN,
            ShutdownMode::Cancel => CANCEL,
        };
        // Drain cannot reopen or downgrade a prior cancellation.
        self.phase.fetch_max(desired, Ordering::AcqRel);
        if self.cancelling() {
            self.stop.stop();
        }
        // Workers use a bounded receive timeout to observe close even when a
        // caller still owns an unsubmitted reservation.
    }
    fn health(&self) -> Health {
        Health {
            phase: self.phase(),
            quota: self.quota.snapshot(),
            lanes: std::array::from_fn(|i| {
                self.banks[i].health((i == Lane::Cpu.index()).then_some(&self.retirement))
            }),
        }
    }

    fn try_reserve_retirement<T: Send + 'static>(
        &self,
        declared_bytes: usize,
    ) -> Result<RetirementReservation<T>, RejectReason> {
        if self.banks[Lane::Cpu.index()].config.threads == 0 {
            return Err(RejectReason::InvalidCost);
        }
        self.retirement
            .try_reserve(&self.quota, declared_bytes, || {
                self.phase.load(Ordering::Acquire) == OPEN
            })
    }
}

#[derive(Clone)]
pub struct ExecutionMonitor {
    shared: Arc<Shared>,
}
impl ExecutionMonitor {
    pub fn health(&self) -> Health {
        self.shared.health()
    }
    pub fn request_shutdown(&self, mode: ShutdownMode) {
        self.shared.close(mode);
    }
}

struct WorkerRecord {
    lane: Lane,
    owner: OwnedWorker,
}

fn bank_metadata_bytes(
    configs: &[LaneConfig; 3],
    total_threads: usize,
) -> Result<usize, StartError> {
    let overflow = || StartError::InvalidConfig("bank metadata byte declaration overflow");
    let mut bytes = size_of::<Shared>()
        .checked_add(size_of::<usize>() * 2) // Shared Arc header.
        .and_then(|value| value.checked_add(total_threads.checked_mul(size_of::<WorkerRecord>())?))
        .ok_or_else(overflow)?;
    for config in configs {
        let slots = config.queue_slots.max(1);
        bytes = bytes
            .checked_add(BANK_FIXED_METADATA_BYTES)
            .and_then(|value| value.checked_add(slots.checked_mul(BANK_QUEUE_SLOT_BYTES)?))
            .and_then(|value| value.checked_add(size_of::<Signal>() + size_of::<Metrics>()))
            .ok_or_else(overflow)?;
    }
    // Primary CPU queue plus a separate finite exceptional-custody queue.
    // Every queued/recoverable original retains one of the same 64 live slots.
    // The fixed metadata charge also covers the RecoveryCustody Arc header.
    bytes = bytes
        .checked_add(
            RETIREMENT_SLOTS
                .checked_mul(BANK_QUEUE_SLOT_BYTES)
                .and_then(|n| n.checked_mul(2))
                .ok_or_else(overflow)?,
        )
        .and_then(|value| value.checked_add(size_of::<RetirementState>()))
        .ok_or_else(overflow)?;
    Ok(bytes)
}

/// Sole logical owner of the prestarted banks. Actual JoinHandles and retirement
/// remain exclusively in ilium-platform; no new supervisor/reaper is created.
#[must_use]
pub struct Execution {
    shared: Arc<Shared>,
    workers: Vec<WorkerRecord>,
}
impl Execution {
    /// Background/bootstrap ONLY: platform spawning can lock and start threads.
    /// No job is accepted before construction returns. Partial-start failure
    /// cancels siblings; their platform records retain every worker debit until
    /// actual join. The supplied QuotaGroup remains observable after failure.
    pub fn start(quota: QuotaGroup, config: ExecutionConfig) -> Result<Self, StartError> {
        Self::start_with_custody(quota, config).map_err(StartFailure::into_legacy_error)
    }
    /// Return the original refusal and any actual partial bank to its caller.
    /// The caller must close and physically observe this owner before retry.
    pub fn start_with_custody(
        quota: QuotaGroup,
        config: ExecutionConfig,
    ) -> Result<Self, StartFailure> {
        Self::start_with_spawn_probe(quota, config, |_, _| Ok(()))
    }
    fn start_with_spawn_probe(
        quota: QuotaGroup,
        config: ExecutionConfig,
        mut before_spawn: impl FnMut(Lane, usize) -> io::Result<()>,
    ) -> Result<Self, StartFailure> {
        if ON_BANK_THREAD.with(Cell::get) {
            return Err(
                StartError::InvalidConfig("cannot create banks inside bank callbacks").into(),
            );
        }
        let configs = config.array();
        let mut total = 0usize;
        for c in &configs {
            if (c.threads == 0) != (c.queue_slots == 0) {
                return Err(
                    StartError::InvalidConfig("disabled bank must have zero queue slots").into(),
                );
            }
            if c.queue_slots > quota.snapshot().limits.jobs {
                return Err(
                    StartError::InvalidConfig("queue slots exceed shared job limit").into(),
                );
            }
            total = total
                .checked_add(c.threads)
                .ok_or(StartError::InvalidConfig("thread overflow"))?;
        }
        if total > MAX_OWNED_WORKERS {
            return Err(StartError::InvalidConfig("banks exceed platform worker capacity").into());
        }
        if total == 0 {
            return Err(StartError::InvalidConfig("at least one bank thread is required").into());
        }
        // Reserve retained bank storage and all thread resources BEFORE
        // allocating queues or spawning. Monitors can outlive actual joins.
        let metadata_charge = quota
            .bank_metadata(bank_metadata_bytes(&configs, total)?)
            .map_err(StartError::Admission)?;
        let mut reservations: Vec<(Lane, usize, Debit)> = Vec::new();
        reservations
            .try_reserve_exact(total)
            .map_err(|_| StartError::Allocation)?;
        for lane in LANES {
            let c = configs[lane.index()];
            for index in 0..c.threads {
                reservations.push((
                    lane,
                    index,
                    quota
                        .worker(c.resident_bytes_per_thread)
                        .map_err(StartError::Admission)?,
                ));
            }
        }
        let shared = Arc::new(Shared {
            quota,
            banks: [
                Bank::new(config.cpu),
                Bank::new(config.io),
                Bank::new(config.service),
            ],
            retirement: RetirementState::new(),
            phase: AtomicU8::new(OPEN),
            stop: StopToken::default(),
            _metadata_charge: metadata_charge,
        });
        let mut workers = Vec::new();
        workers
            .try_reserve_exact(total)
            .map_err(|_| StartError::Allocation)?;
        let mut execution = Self { shared, workers };
        for (lane, index, debit) in reservations {
            let body_shared = Arc::clone(&execution.shared);
            let stop = execution.shared.stop.child();
            let name = format!("ilium-exec-{}-{index}", lane.name());
            let owner = before_spawn(lane, index).and_then(|()| {
                spawn_owned(
                    &name,
                    WorkerKind::Cooperative,
                    stop,
                    move || {
                        // Captured by PLATFORM WorkerState, not merely the callback.
                        // It survives body return, TLS teardown and hung retirement.
                        // Remaining private ticket references may extend it further.
                        let _keep_debit_alive = &debit;
                    },
                    move |stop| run_bank(body_shared, lane, stop),
                )
            });
            let owner = match owner {
                Ok(owner) => owner,
                Err(source) => {
                    return Err(StartFailure {
                        error: StartError::Spawn {
                            lane,
                            index,
                            source,
                        },
                        execution: Some(execution),
                    })
                }
            };
            execution.workers.push(WorkerRecord { lane, owner });
        }
        Ok(execution)
    }

    pub fn monitor(&self) -> ExecutionMonitor {
        ExecutionMonitor {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn retirement(&self) -> RetirementHandle {
        RetirementHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Nonblocking registration. Clones share this tenant's limits; a new
    /// client() call reserves a separate identity under the shared client cap.
    pub fn client(&self, limits: ClientLimits) -> Result<Client, RejectReason> {
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed);
        }
        let tenant = self.shared.quota.client(limits)?;
        Ok(Client {
            shared: Arc::clone(&self.shared),
            tenant,
            wake: None,
        })
    }

    /// Registers in a bank-independent aggregate using this execution's own
    /// queue, callback owner and shutdown lifecycle. Foreign quotas refuse.
    pub fn client_in_group(
        &self,
        group: &AdmissionGroup,
        limits: ClientLimits,
    ) -> Result<Client, RejectReason> {
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed);
        }
        let tenant = group.bind_child(&self.shared.quota, limits)?;
        Ok(Client {
            shared: Arc::clone(&self.shared),
            tenant,
            wake: None,
        })
    }

    pub fn request_shutdown(&self, mode: ShutdownMode) {
        self.shared.close(mode);
    }

    /// Background ONLY. A deadline is an observation, NOT cancellation, success
    /// or detachment. Unfinished owners stay here; Drop later requests Cancel.
    /// Uses one absolute deadline for the entire bank set, not per-worker timeouts.
    pub fn join_until_background(&mut self, deadline: Instant) -> Result<JoinReport, JoinUseError> {
        if ON_BANK_THREAD.with(Cell::get) {
            return Err(JoinUseError::OnBankThread);
        }
        if self.shared.phase.load(Ordering::Acquire) == OPEN {
            return Err(JoinUseError::StillAccepting);
        }
        let mut observations = Vec::with_capacity(self.workers.len());
        let mut index = 0;
        while index < self.workers.len() {
            let record = &self.workers[index];
            let lane = record.lane;
            let ticket = record.owner.ticket();
            let id = ticket.id();
            match ticket.join_until(deadline) {
                Ok(exit) => {
                    let m = &self.shared.banks[lane.index()].metrics;
                    m.joined.fetch_add(1, Ordering::Relaxed);
                    if exit == WorkerExit::Panicked {
                        m.thread_panics.fetch_add(1, Ordering::Relaxed);
                    }
                    observations.push(JoinObservation::Exited {
                        worker_id: id,
                        lane,
                        exit,
                    });
                    drop(self.workers.swap_remove(index));
                }
                Err(_) => {
                    observations.push(JoinObservation::Deadline {
                        worker_id: id,
                        lane,
                    });
                    index += 1;
                }
            }
            // Ticket destruction here also releases a finished WorkerState's
            // captured debit, once its platform record has gone.
            drop(ticket);
        }
        // A final CPU callback can exit after a producer observed its queue
        // connected. Rescue any stranded typed primary envelopes before
        // reporting shutdown; no payload is destroyed on this caller.
        self.shared.retirement.rescue_orphaned_primary();
        let health = self.shared.health();
        let shutdown_complete = self.workers.is_empty()
            && health.lanes.iter().all(|lane| {
                lane.waiting_or_reserved == 0
                    && lane.enqueued == 0
                    && lane.retained_service_claims == 0
                    && lane.running == 0
            })
            && health.lanes[Lane::Cpu.index()].retirement_live == 0;
        Ok(JoinReport {
            observations,
            remaining_workers: self.workers.len(),
            health,
            shutdown_complete,
        })
    }
}
impl Drop for Execution {
    fn drop(&mut self) {
        self.shared.close(ShutdownMode::Cancel);
        // OwnedWorker Drop only signals; the existing supervisor keeps handles.
    }
}

#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
    tenant: Arc<Tenant>,
    wake: Option<Wake>,
}

#[derive(Clone)]
pub struct RetirementHandle {
    shared: Arc<Shared>,
}

impl RetirementHandle {
    /// Call before constructing an expensive Send payload. This uses the
    /// existing CPU workers and an independent finite metadata/storage limit;
    /// it never claims one of the ordinary document jobs.
    pub fn try_reserve<T: Send + 'static>(
        &self,
        declared_bytes: usize,
    ) -> Result<RetirementReservation<T>, RejectReason> {
        self.shared.try_reserve_retirement(declared_bytes)
    }

    /// Nonblocking exceptional-custody handoff to this explicit owner. The
    /// returned ticket keeps the original debit and all guards alive; it can
    /// be retried or recovered by exact payload type. Keep this handle alive
    /// until `retirement_recovery_pending` reaches zero.
    pub fn try_take_failed(&self) -> Option<RetirementFailure> {
        self.shared.retirement.try_take_failed()
    }
}

impl Client {
    pub fn retirement(&self) -> RetirementHandle {
        RetirementHandle {
            shared: Arc::clone(&self.shared),
        }
    }
    /// Shared physical/storage admission capability for external adapters.
    /// Cloning it registers no tenant, starts no bank and mutates no counters.
    pub fn quota_group(&self) -> QuotaGroup {
        self.shared.quota.clone()
    }

    /// A lifecycle hint, not an admission guarantee: shutdown can race this
    /// observation, so publication still uses the normal checked reservation.
    pub fn is_open(&self) -> bool {
        self.shared.phase.load(Ordering::Acquire) == OPEN
    }

    /// Immutable single-CPU-job ceilings across the root and all ancestors.
    /// Does not promise admission: current jobs and shutdown may race this
    /// observation. Disabled CPU workers/queue and zero job budgets are terminal
    /// capability refusals, rather than retryable fullness.
    pub fn maximum_cpu_job_cost(&self) -> Result<JobCost, RejectReason> {
        if !self.is_open() {
            return Err(RejectReason::Closed);
        }
        let bank = &self.shared.banks[Lane::Cpu.index()];
        if bank.config.threads == 0 || bank.config.queue_slots == 0 {
            return Err(RejectReason::InvalidCost);
        }
        let root = self.shared.quota.snapshot().limits;
        if root.jobs == 0 {
            return Err(RejectReason::JobLimit);
        }
        let tenant = self.tenant.maximum_job_cost()?;
        Ok(JobCost {
            input_bytes: root.input_bytes.min(tenant.input_bytes),
            result_bytes: root.result_bytes.min(tenant.result_bytes),
        })
    }

    /// Registers a feature identity beneath this client's aggregate limits.
    /// Every job and retained outcome charges both identities through its
    /// final owner. Clones share a child; registering siblings cannot multiply
    /// the parent allowance. Ownership depth is bounded to eight identities.
    pub fn child(&self, limits: ClientLimits) -> Result<Self, RejectReason> {
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed);
        }
        let tenant = self
            .shared
            .quota
            .child_client(limits, Arc::clone(&self.tenant))?;
        Ok(Self {
            shared: Arc::clone(&self.shared),
            tenant,
            wake: self.wake.clone(),
        })
    }

    pub fn usage(&self) -> QuotaSnapshot {
        self.tenant.snapshot()
    }

    /// Reserve owned payloads in an explicitly supervised external actor.
    /// This charges shared job/input/result limits, never a finite bank slot.
    /// The external owner remains responsible for ordering, cancellation and
    /// shutdown; this reservation neither executes nor cancels its effects.
    pub fn try_reserve_external(&self, cost: JobCost) -> Result<ExternalReservation, RejectReason> {
        let root = &self.shared.quota.ledger;
        let root_guard = root.gate()?;
        let tenant_guard = self.tenant.ledger.gate()?;
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed);
        }
        if cost.input_bytes < size_of::<ExternalReservation>() {
            return Err(RejectReason::InvalidCost);
        }
        let debit = job_debit(root, &self.tenant, cost, false)?;
        let hold = Arc::new(JobHold {
            _debit: debit,
            service_slots: None,
        });
        drop(tenant_guard);
        drop(root_guard);
        Ok(ExternalReservation { cost, hold })
    }

    /// Configure before submitting. The hook runs on a bank thread after the
    /// result is available. Use ONLY a bounded try_send of a wake hint into the
    /// existing event channel. Poll receipts on every such event; Full can mean
    /// an existing wake/event will already cause a poll. No payload is sent here.
    /// With no hook, the adapter must arrange polling while receipts are pending.
    pub fn with_completion_wake(mut self, wake: impl Fn() + Send + Sync + 'static) -> Self {
        self.wake = Some(Arc::new(wake));
        self
    }

    /// Reserve before constructing a large captured input. Reservations occupy
    /// waiting slots and budgets, even while the caller prepares the payload.
    /// Drop/submit outstanding reservations during shutdown; a retained unused
    /// reservation deliberately prevents a bank from declaring a drained exit.
    pub fn try_reserve(&self, lane: Lane, cost: JobCost) -> Result<Reservation, RejectReason> {
        self.try_reserve_detailed(lane, cost)
            .map_err(|failure| failure.reason)
    }

    /// Same admission and ownership as try_reserve, with immutable evidence
    /// from the actual rejecting ledger rather than a later usage snapshot.
    pub fn try_reserve_detailed(
        &self,
        lane: Lane,
        cost: JobCost,
    ) -> Result<Reservation, AdmissionFailure> {
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed.into());
        }
        let root = &self.shared.quota.ledger;
        let root_guard = root.gate()?;
        let tenant_guard = self.tenant.ledger.gate()?;
        if self.shared.phase.load(Ordering::Acquire) != OPEN {
            return Err(RejectReason::Closed.into());
        }
        let bank = &self.shared.banks[lane.index()];
        if bank.signal.pending.load(Ordering::Acquire) >= bank.config.queue_slots {
            return Err(RejectReason::QueueFull.into());
        }
        let service = lane == Lane::Service;
        if service && bank.signal.service_slots.load(Ordering::Acquire) >= bank.config.threads {
            return Err(RejectReason::ServiceBankFull.into());
        }
        let debit = job_debit_detailed(root, &self.tenant, cost, service)?;
        bank.signal.pending.fetch_add(1, Ordering::AcqRel);
        if service {
            bank.signal.service_slots.fetch_add(1, Ordering::AcqRel);
        }
        let queue = QueueSlot {
            signal: Arc::clone(&bank.signal),
        };
        let hold = Arc::new(JobHold {
            _debit: debit,
            service_slots: service.then(|| Arc::clone(&bank.signal.service_slots)),
        });
        drop(tenant_guard);
        drop(root_guard);
        Ok(Reservation {
            shared: Arc::clone(&self.shared),
            lane,
            cost,
            queue,
            hold,
            wake: self.wake.clone(),
        })
    }

    /// Rejection gives the EXACT job back. It has neither run nor been queued.
    /// Treat Busy as backpressure; never spin or build an unbounded retry list.
    pub fn try_submit<J: Job>(
        &self,
        lane: Lane,
        cost: JobCost,
        job: J,
    ) -> Result<Receipt<J>, Rejected<J>> {
        match self.try_reserve(lane, cost) {
            Ok(reservation) => reservation.submit(job),
            Err(reason) => Err(Rejected { reason, value: job }),
        }
    }
}

/// A lifetime debit for an existing actor's accepted command or retained data.
/// Admission precedes expensive capture; no OS work is performed by this type.
#[must_use]
pub struct ExternalReservation {
    cost: JobCost,
    hold: Arc<JobHold>,
}
impl ExternalReservation {
    pub fn validate_value_type<T>(&self) -> Result<(), RejectReason> {
        if size_of::<T>() > self.cost.input_bytes.saturating_add(self.cost.result_bytes) {
            return Err(RejectReason::InvalidCost);
        }
        Ok(())
    }

    pub fn retain<T>(self, value: T) -> Result<crate::job::Retained<T>, Rejected<T>> {
        if let Err(reason) = self.validate_value_type::<T>() {
            return Err(Rejected { reason, value });
        }
        Ok(crate::job::Retained::from_external(value, self.hold))
    }
}

#[must_use]
pub struct Reservation {
    shared: Arc<Shared>,
    lane: Lane,
    cost: JobCost,
    queue: QueueSlot,
    hold: Arc<JobHold>,
    wake: Option<Wake>,
}
impl Reservation {
    /// Shares the original admission through preparation or refused publication.
    /// Clones cover the same allocation; they admit no additional payload.
    pub fn retention(&self) -> crate::job::Retention {
        crate::job::Retained::from_external((), Arc::clone(&self.hold))
            .into_parts()
            .1
    }

    /// Type-size preflight before a caller performs stateful preparation. Heap
    /// capacities and scratch still require the caller's own JobCost audit.
    pub fn validate_job_type<J: Job>(&self) -> Result<(), RejectReason> {
        if self.cost.input_bytes < size_of::<J>()
            || self.cost.result_bytes < size_of::<J::Output>().max(size_of::<J::Error>())
        {
            return Err(RejectReason::InvalidCost);
        }
        Ok(())
    }

    pub fn submit<J: Job>(self, job: J) -> Result<Receipt<J>, Rejected<J>> {
        if let Err(reason) = self.validate_job_type::<J>() {
            return Err(Rejected { reason, value: job });
        }
        let bank = &self.shared.banks[self.lane.index()];
        // Reservation is the admission linearization point. Pending remains
        // nonzero until the worker dequeues or this call rejects, so Drain
        // cannot exit ahead of a late publication. Cancel returns NotStarted.
        // Queue length < queue_slots while this reservation is unsubmitted;
        // bounded try_send therefore cannot report Full for an honest caller.
        let (task, receipt) = task(
            job,
            self.cost,
            self.shared.stop.child(),
            self.queue,
            Arc::clone(&bank.metrics),
            self.hold,
            self.wake,
        );
        bank.signal.enqueued.fetch_add(1, Ordering::AcqRel);
        match bank.queue_sender.try_send(task) {
            Ok(()) => Ok(receipt),
            Err(error) => {
                bank.signal.enqueued.fetch_sub(1, Ordering::AcqRel);
                let (reason, task) = match error {
                    TrySendError::Full(task) => (RejectReason::QueueFull, task),
                    TrySendError::Disconnected(task) => (RejectReason::Closed, task),
                };
                Err(Rejected {
                    reason,
                    value: recover_unpublished(task, receipt),
                })
            }
        }
    }
}

struct ThreadBodyExit {
    shared: Arc<Shared>,
    lane: Lane,
}
impl Drop for ThreadBodyExit {
    fn drop(&mut self) {
        let bank = &self.shared.banks[self.lane.index()];
        let bodies_exited = bank.metrics.bodies_exited.fetch_add(1, Ordering::AcqRel) + 1;
        if self.lane == Lane::Cpu && bodies_exited == bank.config.threads {
            self.shared.retirement.mark_cpu_bodies_exited();
        }
        if std::thread::panicking() {
            self.shared.close(ShutdownMode::Cancel);
        }
        ON_BANK_THREAD.with(|flag| flag.set(false));
        ON_CPU_BANK_THREAD.with(|flag| flag.set(false));
    }
}

fn run_bank(shared: Arc<Shared>, lane: Lane, stop: StopToken) {
    ON_BANK_THREAD.with(|flag| flag.set(true));
    ON_CPU_BANK_THREAD.with(|flag| flag.set(lane == Lane::Cpu));
    let _body_exit = ThreadBodyExit {
        shared: Arc::clone(&shared),
        lane,
    };
    let bank = &shared.banks[lane.index()];
    if let Some(priority) = bank.config.priority {
        lower_current_thread(priority);
    }
    let mut prefer_retirement = true;
    loop {
        if stop.is_stopped() {
            shared.close(ShutdownMode::Cancel);
        }
        if lane == Lane::Cpu {
            if let Ok(failed) = shared.retirement.recovery_receiver().try_recv() {
                shared.retirement.run_failed(failed);
                prefer_retirement = false;
                continue;
            }
            if prefer_retirement {
                if let Ok(task) = shared.retirement.receiver().try_recv() {
                    shared.retirement.run(task);
                    prefer_retirement = false;
                    continue;
                }
            }
            if let Ok(job) = bank.queue_receiver.try_recv() {
                bank.signal.enqueued.fetch_sub(1, Ordering::AcqRel);
                // A callback's own panic is normally caught by Task. This
                // outer boundary protects the bank from a panicking envelope
                // destructor or error-path callback outside that contract.
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.execute(&shared)))
                    .is_err()
                {
                    bank.metrics.thread_panics.fetch_add(1, Ordering::Relaxed);
                }
                prefer_retirement = true;
                continue;
            }
            if let Ok(task) = shared.retirement.receiver().try_recv() {
                shared.retirement.run(task);
                prefer_retirement = false;
                continue;
            }
            crossbeam_channel::select! {
                recv(bank.queue_receiver) -> result => {
                    if let Ok(job) = result {
                        bank.signal.enqueued.fetch_sub(1, Ordering::AcqRel);
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.execute(&shared))).is_err() {
                            bank.metrics.thread_panics.fetch_add(1, Ordering::Relaxed);
                        }
                        prefer_retirement = true;
                        continue;
                    }
                    shared.close(ShutdownMode::Cancel);
                    return;
                }
                recv(shared.retirement.receiver()) -> result => {
                    if let Ok(task) = result {
                        shared.retirement.run(task);
                        prefer_retirement = false;
                        continue;
                    }
                    shared.close(ShutdownMode::Cancel);
                    return;
                }
                recv(shared.retirement.recovery_receiver()) -> result => {
                    if let Ok(failed) = result {
                        shared.retirement.run_failed(failed);
                        prefer_retirement = false;
                        continue;
                    }
                    shared.close(ShutdownMode::Cancel);
                    return;
                }
                default(IDLE_CHECK) => {}
            }
        } else {
            match bank.queue_receiver.recv_timeout(IDLE_CHECK) {
                Ok(job) => {
                    bank.signal.enqueued.fetch_sub(1, Ordering::AcqRel);
                    job.execute(&shared);
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    shared.close(ShutdownMode::Cancel);
                    return;
                }
            }
        }
        if shared.phase.load(Ordering::Acquire) != OPEN
            && bank.signal.pending.load(Ordering::Acquire) == 0
            && (lane != Lane::Cpu || shared.retirement.live() == 0)
        {
            return;
        }
    }
}

#[cfg(test)]
mod retirement_failure_tests {
    use super::*;
    use crate::budget::{JobCost, QuotaLimits};

    struct DropSpy {
        quota: QuotaGroup,
        notice: std::sync::mpsc::Sender<(std::thread::ThreadId, usize)>,
    }

    impl Drop for DropSpy {
        fn drop(&mut self) {
            let _ = self.notice.send((
                std::thread::current().id(),
                self.quota.snapshot().worker_bytes,
            ));
        }
    }

    #[test]
    fn unresolved_recovery_custody_keeps_shutdown_incomplete_until_original_requeued() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
            worker_threads: 1,
            worker_bytes: 1024 * 1024,
        });
        let disabled = LaneConfig {
            threads: 0,
            queue_slots: 0,
            priority: None,
            resident_bytes_per_thread: 0,
        };
        let mut execution = Execution::start(
            quota,
            ExecutionConfig {
                cpu: LaneConfig {
                    threads: 1,
                    queue_slots: 1,
                    priority: None,
                    resident_bytes_per_thread: 4096,
                },
                io: disabled,
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let blocker = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok::<(), ()>(())
                },
            )
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let permit = execution.retirement().try_reserve::<Vec<u8>>(4096).unwrap();
        let mut original = permit.attach(vec![7; 1024]);
        original.force_disconnected_for_test();
        drop(original);
        let health = execution.monitor().health();
        assert_eq!(
            health.lanes[Lane::Cpu.index()].retirement_recovery_pending,
            1
        );
        assert_eq!(health.lanes[Lane::Cpu.index()].retirement_live, 1);
        assert_eq!(health.lanes[Lane::Cpu.index()].retirement_handoff_failed, 1);
        execution.request_shutdown(ShutdownMode::Cancel);
        let first = execution
            .join_until_background(Instant::now() + Duration::from_millis(100))
            .unwrap();
        assert!(!first.shutdown_complete);
        assert_eq!(
            first.health.lanes[Lane::Cpu.index()].retirement_recovery_pending,
            1
        );

        let failure = execution.retirement().try_take_failed().unwrap();
        let recovered = failure.try_into_typed::<Vec<u8>>().ok().unwrap();
        assert_eq!(recovered.len(), 1024);
        drop(recovered); // The disconnected primary returns it to custody.
        assert_eq!(
            execution.monitor().health().lanes[0].retirement_recovery_pending,
            1
        );
        release_sender.send(()).unwrap();
        drop(blocker);
        let final_report = execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert_eq!(final_report.remaining_workers, 0);
        assert_eq!(
            final_report.health.lanes[Lane::Cpu.index()].retirement_recovery_pending,
            0
        );
        assert!(final_report.shutdown_complete);
    }

    #[test]
    fn ordinary_owner_drops_leave_failed_original_with_live_cpu_bank() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 1,
            service_jobs: 0,
            input_bytes: 4096,
            result_bytes: 4096,
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
                    resident_bytes_per_thread: 4096,
                },
                io: disabled,
                service: disabled,
            },
        )
        .unwrap();
        let client = execution
            .client(ClientLimits {
                jobs: 1,
                service_jobs: 0,
                input_bytes: 4096,
                result_bytes: 4096,
            })
            .unwrap();
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(0);
        let blocker = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
                move |_| {
                    started_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok::<(), ()>(())
                },
            )
            .unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let retirement = client.retirement();
        let original_guard = Arc::new(quota.reserve_external_storage(8192).unwrap());
        let (notice_sender, notice_receiver) = std::sync::mpsc::channel();
        let permit = retirement.try_reserve::<DropSpy>(4096).unwrap();
        let mut original = permit.attach(DropSpy {
            quota: quota.clone(),
            notice: notice_sender,
        });
        original.set_storage_guard(Arc::clone(&original_guard));
        drop(original_guard);
        let ui_thread = std::thread::current().id();
        original.force_disconnected_for_test();
        drop(original);
        assert!(notice_receiver.try_recv().is_err());
        assert_eq!(
            execution.monitor().health().lanes[0].retirement_recovery_pending,
            1
        );

        drop(retirement);
        drop(client);
        drop(blocker);
        drop(execution); // Logical owner cancels; platform body still owns CPU.
        assert!(notice_receiver.try_recv().is_err());
        release_sender.send(()).unwrap();
        let (destructor_thread, charged_during_drop) = notice_receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_ne!(destructor_thread, ui_thread);
        assert!(charged_during_drop >= 8192);
        let deadline = Instant::now() + Duration::from_secs(3);
        while quota.snapshot().worker_threads != 0 {
            assert!(Instant::now() < deadline, "physical worker never joined");
            std::thread::yield_now();
        }
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}

#[cfg(test)]
#[path = "root_recovery_scan_tests.rs"]
mod root_recovery_scan_tests;

#[cfg(test)]
#[path = "startup_custody_tests.rs"]
mod startup_custody_tests;
