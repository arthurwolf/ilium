//! Per-plugin queues, fairness and epoch interruption (reference-runtime-model.md,
//! reference-host-architecture.md HST-180..216).
//!
//! Work is owned by named worker banks; instance queues serialize calls and rotate fairly.

use ilium_execution::{reserve_admitted_worker, QuotaGroup, QuotaLimits, WorkerStartError};
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::{PluginId, RequestId};
use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind};

use crate::calls::{deadline_for, CallClass, ResolvedLimits};
use crate::telemetry::LogLine;

// ---- Threads and ticks (reference-host-architecture.md HST-180..184) ---------------------------

/// Side calls one plugin side may run per scheduler tick before the next side gets its turn
/// (reference-host-architecture.md HST-181).
pub const SIDE_CALLS_PER_TICK: usize = 4;
/// Length of one scheduler tick in milliseconds (reference-host-runtime.md HST-180).
pub const TICK_MS: u64 = 10;
/// Name of the scene thread (reference-host-architecture.md HST-182).
pub const SCENE_THREAD_NAME: &str = "ilium-scene";
/// Name of the epoch ticker thread (reference-host-runtime.md HST-180).
pub const EPOCH_THREAD_NAME: &str = "ilium-epoch";
/// Prefix of the executor thread names (reference-host-architecture.md HST-183).
pub const EXECUTOR_THREAD_PREFIX: &str = "ilium-exec-";
/// Prefix of the job bank thread names (reference-host-architecture.md HST-184).
pub const JOB_THREAD_PREFIX: &str = "ilium-job-";

/// Executor threads of a process for a core count (reference-host-architecture.md HST-183).
pub fn executor_threads(process: ProcessKind, cores: usize) -> usize {
    match process {
        ProcessKind::Server => (cores / 2).clamp(2, 4),
        ProcessKind::Client => (cores / 4).clamp(1, 2),
        ProcessKind::Cli | ProcessKind::Helper => 1,
    }
}

/// Job bank threads for a core count (reference-host-architecture.md HST-184).
pub fn job_bank_threads(cores: usize) -> usize {
    (cores / 2).clamp(1, 4)
}

/// Bounded channel from worker threads to the UI thread (reference-host-architecture.md HST-182).
#[derive(Debug)]
pub struct UiWorkChannel<T> {
    items: Mutex<VecDeque<T>>,
    capacity: usize,
}

impl<T> UiWorkChannel<T> {
    /// A channel that holds at most `capacity` items.
    pub fn new(capacity: usize) -> UiWorkChannel<T> {
        UiWorkChannel {
            items: Mutex::new(VecDeque::new()),
            capacity,
        }
    }

    /// Queues an item; a full channel hands it back (reference-host-architecture.md HST-182).
    pub fn try_send(&self, item: T) -> Result<(), T> {
        let mut items = lock(&self.items);
        if items.len() >= self.capacity {
            Err(item)
        } else {
            items.push_back(item);
            Ok(())
        }
    }

    /// Takes the oldest queued item, if any.
    pub fn try_recv(&self) -> Option<T> {
        lock(&self.items).pop_front()
    }
}

// ---- Call kinds and the scheduler (reference-host-architecture.md HST-185..199) --------------

/// Kind of a queued plugin call, in priority order (reference-host-architecture.md HST-185).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CallKind {
    /// `lifecycle.activate`.
    Activate,
    /// `lifecycle.deactivate`.
    Deactivate,
    /// A hook handler.
    HookHandler,
    /// A collect handler.
    Collect,
    /// `service-provider.handle`.
    ServiceProvider,
    /// `inference-provider`.
    InferenceProvider,
    /// `menu-provider.menu-items`.
    MenuItems,
    /// `views.query-catalog`.
    QueryCatalog,
    /// `views.list-virtual-children`.
    ListVirtualChildren,
    /// `token-provider.tokens`.
    Tokens,
    /// `settings-validate`.
    SettingsValidate,
    /// `detector.detect`.
    Detector,
    /// A client-request answer.
    ClientRequestAnswer,
    /// `handler.on-events`.
    OnEvents,
    /// `renderer.render`.
    Render,
    /// `renderer.annotate`.
    Annotate,
    /// `scene-renderer.frame`.
    SceneFrame,
    /// `job-runner.run`.
    JobRun,
    /// A service call parked until the provider's activation ends.
    ServiceCallWaitingForActivation,
}

impl CallKind {
    /// Every kind, in priority order (reference-host-architecture.md HST-185).
    pub const ALL: [CallKind; 19] = [
        CallKind::Activate,
        CallKind::Deactivate,
        CallKind::HookHandler,
        CallKind::Collect,
        CallKind::ServiceProvider,
        CallKind::InferenceProvider,
        CallKind::MenuItems,
        CallKind::QueryCatalog,
        CallKind::ListVirtualChildren,
        CallKind::Tokens,
        CallKind::SettingsValidate,
        CallKind::Detector,
        CallKind::ClientRequestAnswer,
        CallKind::OnEvents,
        CallKind::Render,
        CallKind::Annotate,
        CallKind::SceneFrame,
        CallKind::JobRun,
        CallKind::ServiceCallWaitingForActivation,
    ];

    /// The queue priority, `None` for kinds that bypass the queue (reference-host-architecture.md HST-185).
    pub fn priority(self) -> Option<u8> {
        match self {
            Self::Activate | Self::Deactivate => Some(1),
            Self::OnEvents => Some(3),
            Self::Render => Some(4),
            Self::Annotate => Some(5),
            Self::SceneFrame | Self::JobRun => None,
            _ => Some(2),
        }
    }

    /// Whether a call of this kind may yield to the next side mid-way (reference-host-architecture.md HST-187).
    pub fn may_yield(self) -> bool {
        matches!(
            self,
            Self::Activate | Self::JobRun | Self::ServiceCallWaitingForActivation
        )
    }
}

/// One call the scheduler ran, as seen by the `sched` tests (reference-host-architecture.md HST-186).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedCall {
    /// The plugin side.
    pub plugin: PluginId,
    /// The call kind.
    pub kind: CallKind,
    /// The ticket `enqueue` returned.
    pub ticket: u64,
    /// Name of the thread that ran it.
    pub thread_name: String,
    /// How many times it yielded.
    pub yields: u32,
}

/// One call the scheduler dropped unrun (reference-host-architecture.md HST-191).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbandonedCall {
    /// The plugin side.
    pub plugin: PluginId,
    /// The call kind.
    pub kind: CallKind,
    /// The ticket `enqueue` returned.
    pub ticket: u64,
}

/// What happened to an event pushed to a side's queue (reference-host-architecture.md HST-202).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPush {
    /// Appended.
    Queued,
    /// Merged into an equal pending event.
    Coalesced,
    /// Appended after the oldest was dropped.
    DroppedOldest,
    /// Not appended.
    DiscardedIncoming,
    /// A never-drop queue is full: the producer is told to stop.
    Stuck,
}

/// Queue statistics of one plugin side (reference-host-architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStats {
    /// Events waiting.
    pub queued_events: usize,
    /// Events dropped for a full queue since activation.
    pub dropped_events: u64,
    /// Whether a call is running.
    pub running: bool,
}

type Work = Box<dyn FnOnce() + Send>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct PendingCall {
    kind: CallKind,
    ticket: u64,
    queued_at: Instant,
    work: Work,
}

struct SideQueue {
    calls: Vec<PendingCall>,
    events: VecDeque<Event>,
    capacity: usize,
    dropped: u64,
    busy: bool,
    activating: bool,
    activation_failed: bool,
    peak: usize,
    call_window_start: Option<Instant>,
    calls_in_window: usize,
    breaches: Vec<(CallClass, u64)>,
    source_rate: Option<u32>,
    source_times: VecDeque<Instant>,
    bucket: ExecutionBucket,
    limits: Option<ResolvedLimits>,
}

impl Default for SideQueue {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            events: VecDeque::new(),
            capacity: 1024,
            dropped: 0,
            busy: false,
            activating: false,
            activation_failed: false,
            peak: 0,
            call_window_start: None,
            calls_in_window: 0,
            breaches: Vec::new(),
            source_rate: None,
            source_times: VecDeque::new(),
            bucket: ExecutionBucket::new(),
            limits: None,
        }
    }
}

#[derive(Default)]
struct SchedulerState {
    sides: BTreeMap<PluginId, SideQueue>,
    ready_order: Vec<PluginId>,
    ticket: u64,
    last_tick: Option<Instant>,
    executed: Vec<ExecutedCall>,
    abandoned: Vec<AbandonedCall>,
    logs: Vec<LogLine>,
}

impl SchedulerState {
    fn side(&mut self, plugin: &PluginId) -> &mut SideQueue {
        if !self.sides.contains_key(plugin) {
            self.ready_order.push(plugin.clone());
        }
        self.sides.entry(plugin.clone()).or_default()
    }
}

/// An owned worker bank. Dropping closes admission; the platform supervisor retains
/// physical custody through join, including workers still returning from guest code.
struct WorkerBank {
    sender: Option<mpsc::SyncSender<Work>>,
    workers: Vec<OwnedWorker>,
}

impl WorkerBank {
    fn empty() -> Self {
        Self {
            sender: None,
            workers: Vec::new(),
        }
    }

    fn try_new(prefix: &str, count: usize, quota: &QuotaGroup) -> Result<Self, WorkerStartError> {
        if count == 0 {
            return Ok(Self::empty());
        }
        // Reserve the whole bank before allocating its channel or worker captures.
        // Each declaration includes an equal share of the bounded mailbox metadata.
        let resident_per_worker = (1024 * std::mem::size_of::<Work>()
            + std::mem::size_of::<mpsc::Receiver<Work>>()
            + std::mem::size_of::<Mutex<mpsc::Receiver<Work>>>())
        .div_ceil(count);
        let reservations = (0..count)
            .map(|_| reserve_admitted_worker(quota, 2 * 1024 * 1024, resident_per_worker))
            .collect::<Result<Vec<_>, _>>()?;
        let (sender, receiver) = mpsc::sync_channel::<Work>(1024);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut bank = Self {
            sender: Some(sender),
            workers: Vec::new(),
        };
        for (index, reservation) in reservations.into_iter().enumerate() {
            let receiver = receiver.clone();
            let name = if count == 1 && prefix == SCENE_THREAD_NAME {
                prefix.to_owned()
            } else {
                format!("{prefix}{index}")
            };
            let worker = reservation
                .spawn(
                    &name,
                    WorkerKind::Cooperative,
                    StopToken::default(),
                    || {},
                    move |stop| {
                        ilium_platform::thread_priority::lower_current_thread(
                            ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                        );
                        while !stop.is_stopped() {
                            let work = lock(&receiver).recv();
                            let Ok(work) = work else {
                                break;
                            };
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
                        }
                    },
                )
                .map_err(WorkerStartError::Platform)?;
            bank.workers.push(worker);
        }
        Ok(bank)
    }

    fn send(&self, work: Work) -> Result<(), Work> {
        match &self.sender {
            Some(sender) => sender.try_send(work).map_err(|error| match error {
                mpsc::TrySendError::Full(work) | mpsc::TrySendError::Disconnected(work) => work,
            }),
            None => Err(work),
        }
    }
}

impl Drop for WorkerBank {
    fn drop(&mut self) {
        self.sender.take();
        self.workers.clear();
    }
}

/// Per-instance queues and owned execution banks. `tick` is a dispatcher operation:
/// the client UI submits through `UiWorkChannel` and never calls it to run guests.
pub struct Scheduler {
    state: Mutex<SchedulerState>,
    dispatch: Mutex<()>,
    executor: WorkerBank,
    scenes: WorkerBank,
    jobs: WorkerBank,
    epoch_tick_ms: u64,
    default_limits: ResolvedLimits,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Scheduler")
            .field("epoch_tick_ms", &self.epoch_tick_ms)
            .finish_non_exhaustive()
    }
}

impl Scheduler {
    /// Convenience for isolated callers: server executor sizing plus a scene bank,
    /// with an independent quota. Production hosts must use `try_with_config` to
    /// share their process quota and select the appropriate process worker topology.
    pub fn new(epoch_tick_ms: u64) -> Scheduler {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 9,
            worker_bytes: 32 * 1024 * 1024,
        });
        match Self::try_build(
            ProcessKind::Server,
            epoch_tick_ms,
            ResolvedLimits(BTreeMap::new()),
            &quota,
            1,
        ) {
            Ok(scheduler) => scheduler,
            Err(error) => {
                let mut state = SchedulerState::default();
                state.logs.push(LogLine {
                    plugin: String::new(),
                    level: "error".into(),
                    message: format!("host.worker-unavailable: {error}"),
                });
                Self {
                    state: Mutex::new(state),
                    dispatch: Mutex::new(()),
                    executor: WorkerBank::empty(),
                    scenes: WorkerBank::empty(),
                    jobs: WorkerBank::empty(),
                    epoch_tick_ms,
                    default_limits: ResolvedLimits(BTreeMap::new()),
                }
            }
        }
    }

    /// Production startup: all native workers retain the supplied shared quota
    /// admission through physical join. Only clients own the scene worker.
    /// `limits` supplies default resolved deadlines; register per-side limits when
    /// plugin tiers or manifest requests differ within the hosting process.
    pub fn try_with_config(
        process: ProcessKind,
        epoch_tick_ms: u64,
        limits: ResolvedLimits,
        quota: &QuotaGroup,
    ) -> Result<Self, WorkerStartError> {
        Self::try_build(
            process,
            epoch_tick_ms,
            limits,
            quota,
            usize::from(process == ProcessKind::Client),
        )
    }

    fn try_build(
        process: ProcessKind,
        epoch_tick_ms: u64,
        limits: ResolvedLimits,
        quota: &QuotaGroup,
        scene_workers: usize,
    ) -> Result<Self, WorkerStartError> {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        Ok(Self {
            state: Mutex::new(SchedulerState::default()),
            dispatch: Mutex::new(()),
            executor: WorkerBank::try_new(
                EXECUTOR_THREAD_PREFIX,
                executor_threads(process, cores),
                quota,
            )?,
            scenes: WorkerBank::try_new(SCENE_THREAD_NAME, scene_workers, quota)?,
            jobs: WorkerBank::try_new(
                JOB_THREAD_PREFIX,
                if process == ProcessKind::Helper {
                    0
                } else {
                    job_bank_threads(cores)
                },
                quota,
            )?,
            epoch_tick_ms,
            default_limits: limits,
        })
    }

    /// Registers the effective per-side limits after trust-tier clamping.
    pub fn register_side_with_limits(&self, plugin: &PluginId, limits: ResolvedLimits) {
        let mut state = lock(&self.state);
        let side = state.side(plugin);
        if let Some(capacity) = limits
            .0
            .get(&ilium_plugin_api::limits::LimitKey::EventQueue)
        {
            side.capacity = usize::try_from(*capacity).unwrap_or(usize::MAX);
        }
        side.limits = Some(limits);
    }

    /// Current event queue and store admission statistics.
    pub fn stats(&self, plugin: &PluginId) -> Option<QueueStats> {
        lock(&self.state).sides.get(plugin).map(|side| QueueStats {
            queued_events: side.events.len(),
            dropped_events: side.dropped,
            running: side.busy,
        })
    }

    /// Dispatches one fair pass, with one in-flight call per instance Store.
    /// Separate scene/job Stores bypass ordinary instance admission.
    pub fn tick(&self, now: Instant) {
        let _dispatch = lock(&self.dispatch);
        let order = {
            let mut state = lock(&self.state);
            if let Some(previous) = state.last_tick {
                if now < previous {
                    return;
                }
                let gap = now.duration_since(previous);
                if gap > Duration::from_secs(1) {
                    for side in state.sides.values_mut() {
                        for call in &mut side.calls {
                            call.queued_at += gap;
                        }
                    }
                    state.last_tick = Some(now);
                    return;
                }
                for side in state.sides.values_mut() {
                    side.bucket.refill(gap);
                }
            }
            state.last_tick = Some(now);
            let order = state.ready_order.clone();
            for plugin in &order {
                let mut expired = Vec::new();
                {
                    let side = state.side(plugin);
                    let limits = side.limits.as_ref().unwrap_or(&self.default_limits);
                    side.calls.retain(|call| {
                        let overdue =
                            caller_deadline(call.kind, limits).is_some_and(|(_, deadline)| {
                                now.saturating_duration_since(call.queued_at) > deadline
                            });
                        if overdue {
                            expired.push((call.kind, call.ticket));
                        }
                        !overdue
                    });
                    for (kind, _) in &expired {
                        if let Some((class, _)) = caller_deadline(
                            *kind,
                            side.limits.as_ref().unwrap_or(&self.default_limits),
                        ) {
                            if let Some((_, count)) =
                                side.breaches.iter_mut().find(|(key, _)| *key == class)
                            {
                                *count += 1;
                            } else {
                                side.breaches.push((class, 1));
                            }
                        }
                    }
                }
                for (kind, ticket) in expired {
                    state.abandoned.push(AbandonedCall {
                        plugin: plugin.clone(),
                        kind,
                        ticket,
                    });
                    state.logs.push(LogLine {
                        plugin: plugin.0.clone(),
                        level: "warn".into(),
                        message: format!("host.queue-timeout: {kind:?}"),
                    });
                }
            }
            order
        };
        // Process ordinary calls in round-robin rounds, dispatching every side before
        // joining this round. A slow side therefore cannot delay another side's start.
        for _round in 0..SIDE_CALLS_PER_TICK {
            let mut pending = Vec::new();
            for plugin in &order {
                let call = {
                    let mut state = lock(&self.state);
                    let side = state.side(plugin);
                    if side.call_window_start.is_none_or(|start| {
                        now.saturating_duration_since(start) >= Duration::from_millis(TICK_MS)
                    }) {
                        side.call_window_start = Some(now);
                        side.calls_in_window = 0;
                    }
                    let index = side
                        .calls
                        .iter()
                        .enumerate()
                        .filter(|(_, call)| {
                            call.kind.priority().is_none()
                                || (side.calls_in_window < SIDE_CALLS_PER_TICK
                                    && !side.busy
                                    && !side.activation_failed
                                    && (!side.activating || call.kind == CallKind::Activate))
                        })
                        .min_by_key(|(_, call)| (call.kind.priority().unwrap_or(0), call.ticket))
                        .map(|(index, _)| index);
                    index.map(|index| {
                        let call = side.calls.remove(index);
                        if call.kind.priority().is_some() {
                            side.busy = true;
                            side.peak = side.peak.max(1);
                            side.calls_in_window += 1;
                        }
                        call
                    })
                };
                let Some(call) = call else {
                    continue;
                };
                let kind = call.kind;
                let ticket = call.ticket;
                let (done, result) = mpsc::channel();
                let worker: Work = Box::new(move || {
                    let name = std::thread::current()
                        .name()
                        .unwrap_or("unnamed-plugin-worker")
                        .to_owned();
                    let succeeded =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(call.work)).is_ok();
                    let _ = done.send((name, succeeded));
                });
                let bank = match kind {
                    CallKind::SceneFrame => &self.scenes,
                    CallKind::JobRun => &self.jobs,
                    _ => &self.executor,
                };
                if bank.send(worker).is_ok() {
                    pending.push((plugin.clone(), kind, ticket, result));
                } else {
                    let mut state = lock(&self.state);
                    if kind.priority().is_some() {
                        state.side(plugin).busy = false;
                    }
                    state.abandoned.push(AbandonedCall {
                        plugin: plugin.clone(),
                        kind,
                        ticket,
                    });
                    state.logs.push(LogLine {
                        plugin: plugin.0.clone(),
                        level: "error".into(),
                        message: "host.worker-unavailable: call could not be dispatched".into(),
                    });
                }
            }
            if pending.is_empty() {
                break;
            }
            for (plugin, kind, ticket, result) in pending {
                let outcome = result.recv();
                let mut state = lock(&self.state);
                if kind.priority().is_some() {
                    state.side(&plugin).busy = false;
                }
                match outcome {
                    Ok((thread_name, succeeded)) => {
                        if !succeeded {
                            state.logs.push(LogLine {
                                plugin: plugin.0.clone(),
                                level: "error".into(),
                                message: "trap.panic: scheduled guest adapter panicked".into(),
                            });
                        }
                        state.executed.push(ExecutedCall {
                            plugin,
                            kind,
                            ticket,
                            thread_name,
                            yields: 0,
                        });
                    }
                    Err(_) => state.abandoned.push(AbandonedCall {
                        plugin,
                        kind,
                        ticket,
                    }),
                }
            }
        }
    }

    /// Registers or updates event queue capacity without discarding queued work.
    pub fn register_side(&self, plugin: &PluginId, event_queue: usize) {
        lock(&self.state).side(plugin).capacity = event_queue;
    }
    /// Queues real work; ticket order is stable within a priority.
    pub fn enqueue(&self, plugin: &PluginId, kind: CallKind, now: Instant, work: Work) -> u64 {
        let mut state = lock(&self.state);
        state.ticket = state.ticket.saturating_add(1);
        let ticket = state.ticket;
        state.side(plugin).calls.push(PendingCall {
            kind,
            ticket,
            queued_at: now,
            work,
        });
        ticket
    }
    /// Completed call observations in dispatch order.
    pub fn executed(&self) -> Vec<ExecutedCall> {
        lock(&self.state).executed.clone()
    }
    /// Calls removed before execution.
    pub fn abandoned(&self) -> Vec<AbandonedCall> {
        lock(&self.state).abandoned.clone()
    }
    /// Number of queued-call deadline breaches of this class.
    pub fn breach_count(&self, plugin: &PluginId, class: CallClass) -> u64 {
        lock(&self.state)
            .sides
            .get(plugin)
            .and_then(|side| side.breaches.iter().find(|(key, _)| *key == class))
            .map_or(0, |(_, count)| *count)
    }
    /// Maximum simultaneous ordinary Store calls.
    pub fn max_concurrent_calls_in_store(&self, plugin: &PluginId) -> usize {
        lock(&self.state)
            .sides
            .get(plugin)
            .map_or(0, |side| side.peak)
    }
    /// External guest-runtime Store admission, also used by deterministic tests.
    pub fn set_store_busy(&self, plugin: &PluginId, busy: bool) {
        lock(&self.state).side(plugin).busy = busy;
    }
    /// Holds other ordinary calls until activation succeeds.
    pub fn begin_activation(&self, plugin: &PluginId) {
        let mut state = lock(&self.state);
        let side = state.side(plugin);
        side.activating = true;
        side.activation_failed = false;
    }
    /// Releases admission only after successful activation.
    pub fn finish_activation(&self, plugin: &PluginId, succeeded: bool) {
        let mut state = lock(&self.state);
        let side = state.side(plugin);
        side.activating = false;
        side.activation_failed = !succeeded;
    }
    /// Captured operational diagnostics.
    pub fn log_lines(&self) -> Vec<LogLine> {
        lock(&self.state).logs.clone()
    }
    /// Applies the event-kind policy before inserting into the bounded queue.
    pub fn push_event(&self, plugin: &PluginId, event: Event) -> EventPush {
        push_to_side(lock(&self.state).side(plugin), event)
    }
    /// Configures a monotonic one-second sliding window.
    pub fn set_source_rate(&self, plugin: &PluginId, per_second: u32) {
        lock(&self.state).side(plugin).source_rate = Some(per_second);
    }
    /// Rate checks happen before insertion and refused arrivals never enter the window.
    pub fn push_event_from_source(
        &self,
        plugin: &PluginId,
        event: Event,
    ) -> Result<EventPush, PluginError> {
        let mut state = lock(&self.state);
        let side = state.side(plugin);
        let now = Instant::now();
        side.source_times
            .retain(|at| now.saturating_duration_since(*at) < Duration::from_secs(1));
        if side
            .source_rate
            .is_some_and(|rate| side.source_times.len() >= rate as usize)
        {
            let mut error =
                PluginError::new(ErrorCode::RateLimited, "Plugin event source rate exceeded");
            let remaining = side.source_times.front().map_or(1000, |at| {
                1000u64.saturating_sub(now.saturating_duration_since(*at).as_millis() as u64)
            });
            error.retry_after_ms = Some(
                remaining
                    .div_ceil(10)
                    .max(1)
                    .saturating_mul(10)
                    .min(u64::from(u32::MAX)) as u32,
            );
            return Err(error);
        }
        if side.source_rate.is_some() {
            side.source_times.push_back(now);
        }
        Ok(push_to_side(side, event))
    }
    /// Transfers up to `maximum` queued events in delivery order to the caller.
    /// The caller owns the drained events; queue policies have already been applied.
    pub fn take_events(&self, plugin: &PluginId, maximum: usize) -> Vec<Event> {
        let mut state = lock(&self.state);
        let Some(side) = state.sides.get_mut(plugin) else { return Vec::new(); };
        let count = maximum.min(side.events.len());
        side.events.drain(..count).collect()
    }

    /// Observes pending delivery order without consuming events or changing policy state.
    pub fn snapshot_events(&self, plugin: &PluginId) -> Vec<Event> {
        lock(&self.state).sides.get(plugin)
            .map_or_else(Vec::new, |side| side.events.iter().cloned().collect())
    }

    /// Number of undelivered events.
    pub fn queue_len(&self, plugin: &PluginId) -> usize {
        lock(&self.state)
            .sides
            .get(plugin)
            .map_or(0, |side| side.events.len())
    }
    /// Overrides the side's quota balance for deterministic runtime tests.
    pub fn set_bucket_balance_ms(&self, plugin: &PluginId, balance_ms: i64) {
        lock(&self.state).side(plugin).bucket.balance = balance_ms;
    }
}

fn caller_deadline(kind: CallKind, limits: &ResolvedLimits) -> Option<(CallClass, Duration)> {
    let (class, ms) = match kind {
        CallKind::OnEvents | CallKind::Render | CallKind::SceneFrame | CallKind::JobRun => {
            return None
        }
        CallKind::Activate => (CallClass::Activate, 2000),
        CallKind::Deactivate => (CallClass::Deactivate, 500),
        CallKind::MenuItems => (CallClass::MenuItems, 20),
        CallKind::QueryCatalog => (CallClass::QueryCatalog, 20),
        CallKind::ListVirtualChildren => (CallClass::ListVirtualChildren, 20),
        CallKind::Tokens => (CallClass::Tokens, 10),
        CallKind::SettingsValidate => (CallClass::SettingsValidate, 100),
        CallKind::Detector => (CallClass::Detector, 20),
        CallKind::Annotate => (CallClass::Annotate, 10),
        CallKind::HookHandler => (CallClass::Event, 20),
        _ => (CallClass::Command, 50),
    };
    let deadline = if kind == CallKind::HookHandler {
        Duration::from_millis(ms)
    } else {
        deadline_for(class, limits)
    };
    Some((class, deadline))
}

// ---- Event queues, batches and stream credit (reference-host-architecture.md HST-200..205) -----

/// Most events in one `handler.on-events` batch (reference-host-architecture.md HST-204).
pub const BATCH_MAX_EVENTS: usize = 256;
/// Longest wait before a non-empty batch is flushed, in milliseconds (HST-204).
pub const BATCH_FLUSH_MS: u64 = 2;
/// Bytes of stream data in flight before the reader pauses (reference-host-architecture.md HST-205).
pub const CREDIT_WINDOW_BYTES: usize = 262_144;
/// Largest stream chunk handed to a plugin (reference-host-architecture.md HST-205).
pub const MAX_CHUNK_BYTES: usize = 65_536;

/// How a full event queue treats a new event (reference-host-architecture.md HST-202).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePolicy {
    /// Equal pending events merge.
    Coalesce,
    /// The oldest event makes room.
    DropOldest,
    /// Nothing is dropped; the producer stalls.
    NeverDrop,
}

/// The queue policy of an event kind (reference-host-architecture.md HST-202).
pub fn event_queue_policy(event: &Event) -> QueuePolicy {
    match event {
        Event::TreeChanged(_)
        | Event::PaneFocused(_)
        | Event::PaneResized(_)
        | Event::PaneTitleChanged(_)
        | Event::AgentGoalChanged(_)
        | Event::EditorChanged(_)
        | Event::SettingsChanged(_)
        | Event::TimerFired(_)
        | Event::JobProgress(_)
        | Event::ViewVisibility(_)
        | Event::FsChanged(_)
        | Event::ThemeChanged
        | Event::EventsDropped(_)
        | Event::ScreenChanged(_)
        | Event::SignalChanged(_)
        | Event::AgentModelChanged(_)
        | Event::EditorViewportChanged(_)
        | Event::AttentionChanged(_)
        | Event::KernelSettingsChanged(_)
        | Event::KeymapChanged
        | Event::DisplayModeChanged(_) => QueuePolicy::Coalesce,
        Event::Pointer(pointer)
            if matches!(
                pointer.kind,
                ilium_plugin_api::generated::types::ui::PointerKind::Move
                    | ilium_plugin_api::generated::types::ui::PointerKind::Drag
            ) =>
        {
            QueuePolicy::Coalesce
        }
        Event::EditorPointer(_) => QueuePolicy::Coalesce,
        Event::UiAction(action)
            if matches!(
                action.kind,
                ilium_plugin_api::generated::types::ui::ActionKind::Scrolled
            ) =>
        {
            QueuePolicy::Coalesce
        }
        Event::OutputMatched(_) | Event::TopicPublished(_) | Event::AudioFrames(_) => {
            QueuePolicy::DropOldest
        }
        _ => QueuePolicy::NeverDrop,
    }
}

// Serde is the canonical event wire representation. Selecting only identity fields
// avoids coupling queue identity to changing payloads such as revisions or values.
fn coalesce_key(event: &Event) -> (String, Vec<serde_json::Value>) {
    let serialized = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let payload = serialized
        .as_object()
        .and_then(|object| object.values().next());
    let fields: &[&str] = match event {
        Event::PaneResized(_)
        | Event::PaneTitleChanged(_)
        | Event::AgentGoalChanged(_)
        | Event::Pointer(_)
        | Event::AttentionChanged(_) => &["node"],
        Event::EditorChanged(_) => &["buffer"],
        Event::JobProgress(_) => &["job"],
        Event::ViewVisibility(_) => &["view"],
        Event::FsChanged(_) => &["watch"],
        Event::ScreenChanged(_) | Event::AgentModelChanged(_) | Event::EditorViewportChanged(_) => {
            &["pane"]
        }
        Event::SignalChanged(_) => &["pane", "signal"],
        Event::EditorPointer(_) => &["buffer", "pane"],
        Event::KernelSettingsChanged(_) => &["tab"],
        Event::UiAction(_) => &["view", "key", "kind"],
        _ => &[],
    };
    let identity = if matches!(event, Event::TimerFired(_)) {
        vec![payload.cloned().unwrap_or_default()]
    } else {
        fields
            .iter()
            .map(|field| {
                payload
                    .and_then(|value| value.get(*field))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    };
    (event.case_name().to_owned(), identity)
}

fn merge_event(previous: Event, mut incoming: Event) -> Event {
    match (&previous, &mut incoming) {
        (Event::TreeChanged(old), Event::TreeChanged(new)) => {
            let mut union = old.clone();
            for node in new.iter() {
                if !union.contains(node) {
                    union.push(*node);
                }
            }
            *new = union;
        }
        (Event::FsChanged(old), Event::FsChanged(new)) => {
            let mut union = old.paths.clone();
            for path in &new.paths {
                if !union.contains(path) {
                    union.push(path.clone());
                }
            }
            new.paths = union;
        }
        (Event::KernelSettingsChanged(old), Event::KernelSettingsChanged(new)) => {
            let mut union = old.keys.clone();
            for key in &new.keys {
                if !union.contains(key) {
                    union.push(key.clone());
                }
            }
            new.keys = union;
        }
        (Event::SettingsChanged(old), Event::SettingsChanged(new)) => {
            let mut union = old.keys.clone();
            for key in &new.keys {
                if !union.contains(key) {
                    union.push(key.clone());
                }
            }
            new.keys = union;
        }
        _ => {}
    }
    incoming
}

fn push_to_side(side: &mut SideQueue, mut event: Event) -> EventPush {
    if let Event::NodeRemoved(node) = &event {
        side.events.retain_mut(|pending| {
            if let Event::TreeChanged(nodes) = pending {
                nodes.retain(|id| id != node);
                return !nodes.is_empty();
            }
            if event_queue_policy(pending) != QueuePolicy::Coalesce {
                return true;
            }
            let serialized = serde_json::to_value(&*pending).unwrap_or_default();
            let payload = serialized
                .as_object()
                .and_then(|object| object.values().next());
            !payload.is_some_and(|value| {
                value.get("node").or_else(|| value.get("pane")) == Some(&serde_json::json!(node))
            })
        });
    }
    let policy = event_queue_policy(&event);
    if policy == QueuePolicy::Coalesce {
        let key = coalesce_key(&event);
        if let Some(index) = side
            .events
            .iter()
            .position(|pending| coalesce_key(pending) == key)
        {
            if let Some(previous) = side.events.remove(index) {
                event = merge_event(previous, event);
            }
            side.events.push_back(event);
            return EventPush::Coalesced;
        }
    }
    if side.events.len() < side.capacity {
        side.events.push_back(event);
        return EventPush::Queued;
    }
    if let Some(index) = side
        .events
        .iter()
        .position(|pending| event_queue_policy(pending) == QueuePolicy::DropOldest)
    {
        side.events.remove(index);
        side.dropped = side.dropped.saturating_add(1);
        side.events.push_back(event);
        return EventPush::DroppedOldest;
    }
    if policy == QueuePolicy::DropOldest {
        side.dropped = side.dropped.saturating_add(1);
        return EventPush::DiscardedIncoming;
    }
    EventPush::Stuck
}

fn latency_critical(event: &Event) -> bool {
    matches!(
        event,
        Event::KeyPressed(_)
            | Event::KeyReleased(_)
            | Event::CommandInvoked(_)
            | Event::UiAction(_)
            | Event::Pointer(_)
    )
}

/// Collects events into `on-events` batches (reference-host-architecture.md HST-204).
#[derive(Debug, Default)]
pub struct BatchBuilder {
    events: VecDeque<(Event, u64, bool)>,
}

impl BatchBuilder {
    /// An empty builder.
    pub fn new() -> BatchBuilder {
        Self::default()
    }

    /// Adds an event that arrived at `at_ms`; user actions flush at once (HST-204).
    pub fn push(&mut self, event: Event, at_ms: u64, from_user_action: bool) {
        self.events.push_back((event, at_ms, from_user_action));
    }

    /// The batch to deliver now, if one is due (HST-204).
    pub fn take_ready(&mut self, now_ms: u64) -> Option<Vec<Event>> {
        let (_, first, _) = self.events.front()?;
        let immediate = self
            .events
            .iter()
            .any(|(event, _, user)| *user || latency_critical(event));
        if self.events.len() < BATCH_MAX_EVENTS
            && !immediate
            && now_ms.saturating_sub(*first) < BATCH_FLUSH_MS
        {
            return None;
        }
        let count = self.events.len().min(BATCH_MAX_EVENTS);
        Some(
            self.events
                .drain(..count)
                .map(|(event, _, _)| event)
                .collect(),
        )
    }
}

/// Flow control of one stream to a plugin (reference-host-architecture.md HST-205).
#[derive(Debug, Default)]
pub struct StreamCredit {
    bytes: usize,
    paused: bool,
}

impl StreamCredit {
    /// Credit with nothing in flight.
    pub fn new() -> StreamCredit {
        Self::default()
    }

    /// Bytes queued for the plugin.
    pub fn enqueued(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.paused |= self.bytes >= CREDIT_WINDOW_BYTES;
    }

    /// Bytes the plugin consumed.
    pub fn consumed(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_sub(bytes);
        if self.bytes <= CREDIT_WINDOW_BYTES / 2 {
            self.paused = false;
        }
    }

    /// Bytes queued and not yet consumed.
    pub fn in_flight(&self) -> usize {
        self.bytes
    }

    /// Whether the reader may read more (HST-205).
    pub fn should_read(&self) -> bool {
        !self.paused
    }

    /// Chunk sizes for `len` bytes, each at most `MAX_CHUNK_BYTES` (HST-205).
    pub fn split_chunks(len: usize) -> Vec<usize> {
        let mut chunks = vec![MAX_CHUNK_BYTES; len / MAX_CHUNK_BYTES];
        if !len.is_multiple_of(MAX_CHUNK_BYTES) {
            chunks.push(len % MAX_CHUNK_BYTES);
        }
        chunks
    }
}

// ---- Execution budget (reference-host-architecture.md HST-206, HST-207) -----------------------

/// A plugin's execution-time bucket (reference-host-architecture.md HST-206).
#[derive(Debug)]
pub struct ExecutionBucket {
    balance: i64,
    refill_remainder_ns: u128,
}

impl Default for ExecutionBucket {
    fn default() -> Self {
        Self {
            balance: 2_000,
            refill_remainder_ns: 0,
        }
    }
}

impl ExecutionBucket {
    /// A bucket at its starting balance.
    pub fn new() -> ExecutionBucket {
        Self::default()
    }

    /// Charges CPU time.
    pub fn charge_cpu_ms(&mut self, cpu_ms: u64) {
        self.balance =
            (i128::from(self.balance) - i128::from(cpu_ms)).max(i128::from(i64::MIN)) as i64;
    }

    /// Charges payload bytes at the documented rate (HST-206).
    pub fn charge_payload_bytes(&mut self, bytes: u64) {
        self.charge_cpu_ms(bytes.div_ceil(MAX_CHUNK_BYTES as u64));
    }

    /// Refills for elapsed wall time, capped at the bucket size (HST-206).
    pub fn refill(&mut self, elapsed: Duration) {
        let units = elapsed.as_nanos().saturating_add(self.refill_remainder_ns);
        let refill = units / 2_000_000;
        self.refill_remainder_ns = units % 2_000_000;
        self.balance =
            (i128::from(self.balance) + refill.min(i64::MAX as u128) as i128).min(2_000) as i64;
        if self.balance == 2_000 {
            self.refill_remainder_ns = 0;
        }
    }

    /// The current balance; negative is debt.
    pub fn balance_ms(&self) -> i64 {
        self.balance
    }

    /// Refuses a submission with `quota-exceeded` while in debt (HST-206, HST-207).
    pub fn try_submit(&self) -> Result<(), PluginError> {
        if self.balance > 0 {
            return Ok(());
        }
        let mut error =
            PluginError::new(ErrorCode::QuotaExceeded, "Plugin execution quota is empty");
        error.retry_after_ms = Some(retry_after_ms(self.balance));
        Err(error)
    }
}

/// Milliseconds to wait until a debt is repaid at the refill rate (HST-207).
pub fn retry_after_ms(balance_ms: i64) -> u32 {
    crate::limits_rules::retry_after_ms(balance_ms).min(i64::from(u32::MAX / 10 * 10)) as u32
}

// ---- Cancellation (reference-host-architecture.md HST-208..216) -------------------------------

/// Teardown bound of a cancelled HTTP or file read, in milliseconds (HST-212).
pub const HTTP_FILE_TEARDOWN_MS: u64 = 100;
/// Grace between `SIGTERM` and `SIGKILL` of a cancelled child, in milliseconds (HST-213).
pub const CHILD_KILL_GRACE_MS: u64 = 2_000;
/// Grace a cancelled job gets to return before it counts as a trap, in milliseconds (HST-214).
pub const JOB_CANCEL_GRACE_MS: u64 = 1_000;

/// Why a request was cancelled (reference-host-architecture.md HST-208).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelCause {
    /// The side stopped.
    SideStopped,
    /// A grant it needed was revoked.
    GrantRevoked,
    /// The plugin asked.
    PluginAsked,
    /// The host is shutting down.
    Shutdown,
    /// Its deadline passed.
    DeadlinePassed,
    /// The provider trapped.
    ProviderTrapped,
    /// The provider breached a limit.
    ProviderBreach,
}

/// The outcome a cancelled request reports (HST-208).
pub fn cancellation_outcome(cause: CancelCause) -> Outcome {
    let code = match cause {
        CancelCause::SideStopped | CancelCause::PluginAsked | CancelCause::Shutdown => {
            return Outcome::Cancelled
        }
        CancelCause::GrantRevoked => ErrorCode::Revoked,
        CancelCause::DeadlinePassed => ErrorCode::TimedOut,
        CancelCause::ProviderTrapped => ErrorCode::Cancelled,
        CancelCause::ProviderBreach => ErrorCode::Internal,
    };
    Outcome::Failed(PluginError::new(code, "Plugin request cancelled"))
}

/// Open requests and their final outcomes (reference-host-architecture.md HST-209).
#[derive(Debug, Default)]
pub struct RequestLedger {
    requests: Vec<(RequestRef, Option<Outcome>)>,
}

impl RequestLedger {
    /// An empty ledger.
    pub fn new() -> RequestLedger {
        Self::default()
    }

    /// Records an open request.
    pub fn open(&mut self, request: RequestRef) {
        if !self.requests.iter().any(|(id, _)| *id == request) {
            self.requests.push((request, None));
        }
    }

    /// Completes a request; a second completion is ignored (HST-209).
    pub fn complete(&mut self, request: RequestRef, outcome: Outcome) {
        if let Some((_, result)) = self.requests.iter_mut().find(|(id, _)| *id == request) {
            if result.is_none() {
                *result = Some(outcome);
            }
        }
    }

    /// Cancels a request (HST-209).
    pub fn cancel(&mut self, request: RequestRef) {
        self.complete(request, Outcome::Cancelled);
    }

    /// The final outcome of a request, if it ended.
    pub fn outcome_of(&self, request: RequestRef) -> Option<Outcome> {
        self.requests
            .iter()
            .find(|(id, _)| *id == request)
            .and_then(|(_, result)| result.clone())
    }

    /// How many times a request was completed (at most one, HST-209).
    pub fn completion_count(&self, request: RequestRef) -> usize {
        usize::from(self.outcome_of(request).is_some())
    }
}

/// Kind of work a cancellation tears down (HST-212..214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelledWork {
    /// An HTTP request.
    Http,
    /// A file read.
    FileRead,
    /// A child process.
    Child,
    /// A job.
    Job,
}

/// Longest teardown of cancelled work (HST-212..214).
pub fn teardown_bound(work: CancelledWork) -> Duration {
    Duration::from_millis(match work {
        CancelledWork::Http | CancelledWork::FileRead => HTTP_FILE_TEARDOWN_MS,
        CancelledWork::Child => CHILD_KILL_GRACE_MS,
        CancelledWork::Job => JOB_CANCEL_GRACE_MS,
    })
}

/// Signals sent to a cancelled child (HST-213).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSignal {
    /// `SIGTERM`.
    Term,
    /// `SIGKILL`.
    Kill,
}

/// The signals and the delay before each one (HST-213).
pub fn child_signal_schedule() -> Vec<(ChildSignal, Duration)> {
    vec![
        (ChildSignal::Term, Duration::ZERO),
        (
            ChildSignal::Kill,
            Duration::from_millis(CHILD_KILL_GRACE_MS),
        ),
    ]
}

/// Whether a cancelled job is still allowed to run or counts as a trap (HST-214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCancelVerdict {
    /// Within the grace period.
    Running,
    /// Past the grace period without returning.
    Trap,
}

/// The verdict for a job whose cancel flag was set at `flag_set_at_ms` (HST-214).
pub fn job_cancel_verdict(flag_set_at_ms: u64, now_ms: u64, returned: bool) -> JobCancelVerdict {
    if !returned && now_ms.saturating_sub(flag_set_at_ms) >= JOB_CANCEL_GRACE_MS {
        JobCancelVerdict::Trap
    } else {
        JobCancelVerdict::Running
    }
}

// ---- Hook chains (reference-host-architecture.md HST-215, HST-216) ---------------------------

/// One handler of a hook chain, as the `sched` tests describe it (HST-215).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerSpec {
    /// The plugin.
    pub plugin: PluginId,
    /// The process its side runs in.
    pub process: ProcessKind,
    /// Its deadline.
    pub deadline_ms: u64,
    /// Until when its side is busy with another call.
    pub busy_until_ms: u64,
    /// How long the handler runs once started.
    pub run_ms: u64,
}

/// What happened to one handler of a chain (HST-215).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRun {
    /// The plugin.
    pub plugin: PluginId,
    /// When it started, if it ran.
    pub started_ms: Option<u64>,
    /// When it ended, if it ran.
    pub ended_ms: Option<u64>,
    /// Skipped and treated as a pass.
    pub skipped_as_pass: bool,
    /// Breached its deadline.
    pub breach: bool,
    /// Skipped because the chain's time cap was reached.
    pub skipped_by_chain_cap: bool,
}

/// The run of one hook chain (HST-215, HST-216).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    /// The request the hook belongs to.
    pub request: RequestId,
    /// How long the owner waited on the chain.
    pub owner_blocked_ms: u64,
    /// Each handler's run, in chain order.
    pub runs: Vec<HandlerRun>,
    /// Log ids written during the run.
    pub log_ids: Vec<String>,
}

/// Runs a hook chain started at `start_ms` by a side of `owner_process` (HST-215, HST-216).
pub fn run_hook(owner_process: ProcessKind, handlers: Vec<HandlerSpec>, start_ms: u64) -> HookRun {
    static NEXT_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request = RequestId(NEXT_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let mut result = HookRun {
        request,
        owner_blocked_ms: 0,
        runs: Vec::new(),
        log_ids: Vec::new(),
    };
    let mut now = start_ms;
    let mut first_start = None;
    for handler in handlers
        .into_iter()
        .filter(|handler| handler.process == owner_process)
    {
        let due = now;
        let started = now.max(handler.busy_until_ms);
        let capped = first_start.is_some_and(|first| started.saturating_sub(first) >= 100);
        let cannot_start = started.saturating_sub(due) > handler.deadline_ms;
        let mut run = HandlerRun {
            plugin: handler.plugin,
            started_ms: None,
            ended_ms: None,
            skipped_as_pass: capped || cannot_start,
            breach: cannot_start,
            skipped_by_chain_cap: capped,
        };
        if capped {
            if !result.log_ids.iter().any(|id| id == "extension.chain-cap") {
                result.log_ids.push("extension.chain-cap".into());
            }
        } else if !cannot_start {
            first_start.get_or_insert(started);
            now = started.saturating_add(handler.run_ms);
            run.started_ms = Some(started.saturating_sub(start_ms));
            run.ended_ms = Some(now.saturating_sub(start_ms));
            run.breach = handler.run_ms > handler.deadline_ms;
        }
        result.runs.push(run);
    }
    result
}
