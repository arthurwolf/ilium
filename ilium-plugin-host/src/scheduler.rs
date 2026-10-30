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
    /// Drops not yet reported to the plugin, as `events-dropped` records (EVT-004).
    dropped_records: Vec<ilium_plugin_api::generated::types::events::DroppedCount>,
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
            dropped_records: Vec::new(),
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
        let Some(side) = state.sides.get_mut(plugin) else {
            return Vec::new();
        };
        let mut batch: Vec<Event> = Vec::new();
        // The `events-dropped` record leads the batch and counts toward its limit.
        if maximum > 0 && !side.dropped_records.is_empty() {
            batch.push(Event::EventsDropped(std::mem::take(&mut side.dropped_records)));
        }
        let count = maximum.saturating_sub(batch.len()).min(side.events.len());
        batch.extend(side.events.drain(..count));
        batch
    }

    /// Observes pending delivery order without consuming events or changing policy state.
    pub fn snapshot_events(&self, plugin: &PluginId) -> Vec<Event> {
        lock(&self.state)
            .sides
            .get(plugin)
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
                ilium_plugin_api::generated::types::ui::PointerKind::Drag
            ) =>
        {
            QueuePolicy::Coalesce
        }
        Event::EditorPointer(pointer)
            if pointer.pointer.kind
                == ilium_plugin_api::generated::types::ui::PointerKind::Move =>
        {
            QueuePolicy::Coalesce
        }
        Event::UiAction(action)
            if matches!(
                action.kind,
                ilium_plugin_api::generated::types::ui::ActionKind::Scrolled
                    | ilium_plugin_api::generated::types::ui::ActionKind::Pointer
                    | ilium_plugin_api::generated::types::ui::ActionKind::DragOver
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

/// Adds a dropped event to the pending `events-dropped` records (EVT-004).
fn note_dropped(side: &mut SideQueue, event: &Event) {
    let (name, source) = match event {
        Event::OutputMatched(matched) => ("output-matched", Some(matched.filter_id.clone())),
        Event::TopicPublished(published) => ("topic-published", Some(published.topic.clone())),
        _ => ("audio-frames", None),
    };
    if let Some(record) = side
        .dropped_records
        .iter_mut()
        .find(|record| record.event == name && record.source == source)
    {
        record.count = record.count.saturating_add(1);
    } else {
        side.dropped_records
            .push(ilium_plugin_api::generated::types::events::DroppedCount {
                event: name.to_owned(),
                source,
                count: 1,
            });
    }
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
        if let Some(evicted) = side.events.remove(index) {
            note_dropped(side, &evicted);
        }
        side.dropped = side.dropped.saturating_add(1);
        side.events.push_back(event);
        return EventPush::DroppedOldest;
    }
    if policy == QueuePolicy::DropOldest {
        side.dropped = side.dropped.saturating_add(1);
        note_dropped(side, &event);
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::events::{Event, KeyEvent, OutputMatchedEvent};
    use ilium_plugin_api::generated::types::ids::GrabId;
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use ilium_plugin_api::ids::{NodeId, PluginId, RequestId};
    use ilium_plugin_api::{PluginError, ProcessKind};

    use super::{
        cancellation_outcome, child_signal_schedule, event_queue_policy, executor_threads,
        job_bank_threads, job_cancel_verdict, retry_after_ms, run_hook, teardown_bound,
        BatchBuilder, CallKind, CancelCause, CancelledWork, ChildSignal, EventPush,
        ExecutionBucket, HandlerSpec, JobCancelVerdict, QueuePolicy, QueueStats, RequestLedger,
        Scheduler, StreamCredit, UiWorkChannel, BATCH_FLUSH_MS, BATCH_MAX_EVENTS,
        CHILD_KILL_GRACE_MS, CREDIT_WINDOW_BYTES, EPOCH_THREAD_NAME, EXECUTOR_THREAD_PREFIX,
        HTTP_FILE_TEARDOWN_MS, JOB_CANCEL_GRACE_MS, JOB_THREAD_PREFIX, MAX_CHUNK_BYTES,
        SCENE_THREAD_NAME, SIDE_CALLS_PER_TICK, TICK_MS,
    };
    use crate::calls::CallClass;

    fn plugin(text: &str) -> PluginId {
        PluginId(text.to_string())
    }

    fn tree_changed() -> Event {
        Event::TreeChanged(vec![NodeId(1)])
    }

    fn key_pressed() -> Event {
        Event::KeyPressed(KeyEvent {
            grab: GrabId(1),
            pane: None,
            chord: "ctrl+k".to_string(),
            repeat: false,
        })
    }

    fn output_matched() -> Event {
        Event::OutputMatched(OutputMatchedEvent {
            node: NodeId(1),
            filter_id: "filter".to_string(),
            line: "line".to_string(),
            row: 0,
        })
    }

    fn noop() -> Box<dyn FnOnce() + Send> {
        Box::new(|| {})
    }

    fn handler(
        plugin_name: &str,
        process: ProcessKind,
        deadline_ms: u64,
        busy_until_ms: u64,
        run_ms: u64,
    ) -> HandlerSpec {
        HandlerSpec {
            plugin: plugin(plugin_name),
            process,
            deadline_ms,
            busy_until_ms,
            run_ms,
        }
    }

    fn expect_quota_refusal(result: Result<(), PluginError>) -> PluginError {
        match result {
            Ok(()) => panic!("expected the bucket to refuse the submission"),
            Err(error) => error,
        }
    }

    // ---- Threads and ticks ----------------------------------------------------------------------

    /// REQ: HST-079
    #[test]
    fn epoch_thread_is_named_as_the_manual_says() {
        assert_eq!(EPOCH_THREAD_NAME, "ilium-plugin-epoch");
    }

    /// REQ: HST-183
    #[test]
    fn scene_thread_is_named_as_the_manual_says() {
        assert_eq!(SCENE_THREAD_NAME, "ilium-plugin-scene");
    }

    /// REQ: HST-182
    #[test]
    fn executor_and_job_thread_name_prefixes_match_the_manual() {
        assert_eq!(EXECUTOR_THREAD_PREFIX, "ilium-plugin-");
        assert_eq!(JOB_THREAD_PREFIX, "ilium-job-");
    }

    /// REQ: HST-081
    #[test]
    fn tick_period_is_ten_milliseconds() {
        assert_eq!(TICK_MS, 10);
    }

    /// REQ: HST-182
    #[test]
    fn server_executor_threads_follow_clamp_of_half_the_cores() {
        // clamp(cores / 2, 2, 4), with integer division; includes zero cores and the upper bound.
        let expected = [(0, 2), (1, 2), (2, 2), (4, 2), (6, 3), (8, 4), (16, 4)];
        for (cores, threads) in expected {
            assert_eq!(
                executor_threads(ProcessKind::Server, cores),
                threads,
                "cores={cores}"
            );
        }
    }

    /// REQ: HST-182
    #[test]
    fn client_executor_threads_follow_clamp_of_a_quarter_of_the_cores() {
        // clamp(cores / 4, 1, 2), with integer division.
        let expected = [(0, 1), (1, 1), (4, 1), (7, 1), (8, 2), (16, 2)];
        for (cores, threads) in expected {
            assert_eq!(
                executor_threads(ProcessKind::Client, cores),
                threads,
                "cores={cores}"
            );
        }
    }

    /// REQ: HST-182
    #[test]
    fn cli_executor_is_a_single_thread_pool() {
        for cores in [1, 4, 16] {
            assert_eq!(
                executor_threads(ProcessKind::Cli, cores),
                1,
                "cores={cores}"
            );
        }
    }

    /// REQ: HST-182
    #[test]
    fn job_bank_threads_are_half_the_cores_capped_at_four_and_at_least_one() {
        // min(4, cores / 2), at least 1.
        let expected = [(0, 1), (1, 1), (2, 1), (4, 2), (6, 3), (8, 4), (16, 4)];
        for (cores, threads) in expected {
            assert_eq!(job_bank_threads(cores), threads, "cores={cores}");
        }
    }

    /// REQ: HST-181
    #[test]
    fn ui_work_channel_hands_back_an_item_when_full() {
        let channel: UiWorkChannel<u32> = UiWorkChannel::new(1);
        assert_eq!(channel.try_send(7), Ok(()));
        assert_eq!(channel.try_send(8), Err(8));
        assert_eq!(channel.try_recv(), Some(7));
        assert_eq!(channel.try_recv(), None);
    }

    /// REQ: HST-181
    #[test]
    fn empty_ui_work_channel_yields_nothing() {
        let channel: UiWorkChannel<u32> = UiWorkChannel::new(2);
        assert_eq!(channel.try_recv(), None);
    }

    // ---- Call kinds ------------------------------------------------------------------------------

    /// REQ: HST-184
    #[test]
    fn priority_table_orders_the_queued_call_kinds() {
        assert_eq!(CallKind::Activate.priority(), Some(1));
        assert_eq!(CallKind::Deactivate.priority(), Some(1));
        for kind in [
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
        ] {
            assert_eq!(kind.priority(), Some(2), "{kind:?}");
        }
        assert_eq!(CallKind::OnEvents.priority(), Some(3));
        assert_eq!(CallKind::Render.priority(), Some(4));
        assert_eq!(CallKind::Annotate.priority(), Some(5));
    }

    /// REQ: HST-184
    #[test]
    fn scene_frames_and_job_runs_bypass_the_instance_queue() {
        assert_eq!(CallKind::SceneFrame.priority(), None);
        assert_eq!(CallKind::JobRun.priority(), None);
    }

    /// REQ: HST-196
    #[test]
    fn only_activate_job_runs_and_activation_waits_may_yield() {
        for kind in CallKind::ALL {
            let expected = matches!(
                kind,
                CallKind::Activate | CallKind::JobRun | CallKind::ServiceCallWaitingForActivation
            );
            assert_eq!(kind.may_yield(), expected, "{kind:?}");
        }
    }

    // ---- Scheduler: registration, dispatch and fairness -----------------------------------------

    /// REQ: HST-202
    #[test]
    fn unregistered_plugin_has_no_queue_statistics() {
        let scheduler = Scheduler::new(TICK_MS);
        assert_eq!(scheduler.stats(&plugin("example.unknown")), None);
    }

    /// REQ: HST-202
    #[test]
    fn registered_side_starts_with_an_empty_idle_queue() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        assert_eq!(
            scheduler.stats(&alpha),
            Some(QueueStats {
                queued_events: 0,
                dropped_events: 0,
                running: false,
            })
        );
        assert_eq!(scheduler.queue_len(&alpha), 0);
    }

    /// REQ: HST-185
    #[test]
    fn enqueue_returns_distinct_increasing_tickets() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        let now = Instant::now();
        let first = scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        let second = scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        assert!(second > first, "first={first} second={second}");
    }

    /// REQ: HST-185
    #[test]
    fn one_tick_runs_at_most_four_calls_of_one_side() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        let now = Instant::now();
        for _ in 0..6 {
            scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        }
        scheduler.tick(now);
        assert_eq!(scheduler.executed().len(), SIDE_CALLS_PER_TICK);
        scheduler.tick(now);
        assert_eq!(scheduler.executed().len(), 6);
    }

    /// REQ: HST-184
    #[test]
    fn calls_run_by_priority_then_arrival_order() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        let now = Instant::now();
        scheduler.enqueue(&alpha, CallKind::Render, now, noop());
        scheduler.enqueue(&alpha, CallKind::OnEvents, now, noop());
        scheduler.enqueue(&alpha, CallKind::Activate, now, noop());
        scheduler.tick(now);
        let kinds: Vec<CallKind> = scheduler.executed().iter().map(|call| call.kind).collect();
        assert_eq!(
            kinds,
            vec![CallKind::Activate, CallKind::OnEvents, CallKind::Render]
        );
    }

    /// REQ: HST-184
    #[test]
    fn calls_of_equal_priority_keep_arrival_order() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        let now = Instant::now();
        let first = scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        let second = scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        scheduler.tick(now);
        let tickets: Vec<u64> = scheduler
            .executed()
            .iter()
            .map(|call| call.ticket)
            .collect();
        assert_eq!(tickets, vec![first, second]);
    }

    /// REQ: HST-184
    #[test]
    fn one_store_never_runs_two_calls_at_once() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        let now = Instant::now();
        for _ in 0..3 {
            scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
        }
        scheduler.tick(now);
        assert_eq!(scheduler.max_concurrent_calls_in_store(&alpha), 1);
    }

    /// REQ: HST-185
    #[test]
    fn round_robin_alternates_between_busy_plugin_sides() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        let beta = plugin("example.beta");
        scheduler.register_side(&alpha, 1024);
        scheduler.register_side(&beta, 1024);
        let now = Instant::now();
        for _ in 0..6 {
            scheduler.enqueue(&alpha, CallKind::HookHandler, now, noop());
            scheduler.enqueue(&beta, CallKind::HookHandler, now, noop());
        }
        scheduler.tick(now);
        let executed = scheduler.executed();
        let alpha_count = executed
            .iter()
            .filter(|call| call.plugin.0 == alpha.0)
            .count();
        let beta_count = executed
            .iter()
            .filter(|call| call.plugin.0 == beta.0)
            .count();
        assert_eq!(alpha_count, SIDE_CALLS_PER_TICK);
        assert_eq!(beta_count, SIDE_CALLS_PER_TICK);
        assert_ne!(
            executed[0].plugin.0, executed[1].plugin.0,
            "round robin must interleave"
        );
    }

    /// REQ: HST-187
    #[test]
    fn queued_call_with_waiting_caller_is_abandoned_past_its_deadline() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        scheduler.set_store_busy(&alpha, true);
        let queued_at = Instant::now();
        let ticket = scheduler.enqueue(&alpha, CallKind::Detector, queued_at, noop());
        scheduler.tick(queued_at + Duration::from_millis(1000));
        assert!(
            scheduler
                .abandoned()
                .iter()
                .any(|call| call.ticket == ticket && call.kind == CallKind::Detector),
            "the detector call waited 1000 ms against a 20 ms deadline"
        );
        assert!(scheduler.executed().is_empty());
    }

    /// REQ: HST-187
    #[test]
    fn queued_call_without_caller_deadline_is_never_abandoned() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        scheduler.set_store_busy(&alpha, true);
        let queued_at = Instant::now();
        scheduler.enqueue(&alpha, CallKind::OnEvents, queued_at, noop());
        scheduler.tick(queued_at + Duration::from_secs(10));
        assert!(scheduler.abandoned().is_empty());
    }

    /// REQ: HST-216
    #[test]
    fn abandoned_call_logs_queue_timeout_and_counts_a_breach_of_its_class() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        assert_eq!(scheduler.breach_count(&alpha, CallClass::Detector), 0);
        scheduler.set_store_busy(&alpha, true);
        let queued_at = Instant::now();
        scheduler.enqueue(&alpha, CallKind::Detector, queued_at, noop());
        scheduler.tick(queued_at + Duration::from_millis(1000));
        assert_eq!(scheduler.breach_count(&alpha, CallClass::Detector), 1);
        assert!(
            scheduler
                .log_lines()
                .iter()
                .any(|line| line.message.contains("host.queue-timeout")),
            "expected a host.queue-timeout log line"
        );
    }

    /// REQ: HST-197
    #[test]
    fn calls_wait_while_the_side_is_activating() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        scheduler.begin_activation(&alpha);
        let now = Instant::now();
        scheduler.enqueue(&alpha, CallKind::OnEvents, now, noop());
        scheduler.tick(now);
        assert!(
            scheduler.executed().is_empty(),
            "no call may enter an activating instance"
        );
        scheduler.finish_activation(&alpha, true);
        scheduler.tick(now);
        let kinds: Vec<CallKind> = scheduler.executed().iter().map(|call| call.kind).collect();
        assert_eq!(kinds, vec![CallKind::OnEvents]);
    }

    // ---- Event queues ----------------------------------------------------------------------------

    /// REQ: HST-202
    #[test]
    fn queue_policies_follow_the_event_table() {
        assert_eq!(event_queue_policy(&tree_changed()), QueuePolicy::Coalesce);
        assert_eq!(
            event_queue_policy(&output_matched()),
            QueuePolicy::DropOldest
        );
        assert_eq!(event_queue_policy(&key_pressed()), QueuePolicy::NeverDrop);
        assert_eq!(
            event_queue_policy(&Event::Message(
                ilium_plugin_api::generated::types::events::MessageEvent {
                    from: plugin("example.peer"),
                    from_side: ilium_plugin_api::generated::types::intents::PluginSide::Server,
                    from_client: None,
                    channel: "channel".to_string(),
                    payload: Vec::new(),
                }
            )),
            QueuePolicy::NeverDrop
        );
    }

    /// REQ: HST-202
    #[test]
    fn equal_pending_coalesced_event_merges_into_one_slot() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        assert_eq!(
            scheduler.push_event(&alpha, tree_changed()),
            EventPush::Queued
        );
        assert_eq!(
            scheduler.push_event(&alpha, tree_changed()),
            EventPush::Coalesced
        );
        assert_eq!(scheduler.queue_len(&alpha), 1);
    }

    /// REQ: HST-202
    #[test]
    fn full_queue_drops_the_oldest_drop_oldest_event() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(
            scheduler.push_event(&alpha, output_matched()),
            EventPush::Queued
        );
        assert_eq!(
            scheduler.push_event(&alpha, output_matched()),
            EventPush::DroppedOldest
        );
        assert_eq!(scheduler.queue_len(&alpha), 1);
        assert_eq!(
            scheduler.stats(&alpha).map(|stats| stats.dropped_events),
            Some(1)
        );
    }

    /// REQ: HST-202
    #[test]
    fn full_queue_discards_an_incoming_drop_oldest_event_when_none_is_pending() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(
            scheduler.push_event(&alpha, key_pressed()),
            EventPush::Queued
        );
        assert_eq!(
            scheduler.push_event(&alpha, output_matched()),
            EventPush::DiscardedIncoming
        );
        assert_eq!(
            scheduler.stats(&alpha).map(|stats| stats.dropped_events),
            Some(1)
        );
    }

    /// REQ: HST-203
    #[test]
    fn full_queue_of_never_dropped_events_makes_the_producer_stuck() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(
            scheduler.push_event(&alpha, key_pressed()),
            EventPush::Queued
        );
        assert_eq!(
            scheduler.push_event(&alpha, key_pressed()),
            EventPush::Stuck
        );
    }

    /// REQ: HST-206
    #[test]
    fn source_over_its_rate_is_refused_before_enqueue() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1024);
        scheduler.set_source_rate(&alpha, 10);
        for index in 0..10 {
            let result = scheduler.push_event_from_source(&alpha, key_pressed());
            assert!(
                result.is_ok(),
                "event {index} is within the rate and must be accepted"
            );
        }
        let refused = match scheduler.push_event_from_source(&alpha, key_pressed()) {
            Ok(push) => panic!("the 11th event in one second must be refused, got {push:?}"),
            Err(error) => error,
        };
        assert_eq!(refused.code, ErrorCode::RateLimited);
        assert!(
            refused.retry_after_ms.is_some(),
            "rate-limited must carry retry-after-ms"
        );
        assert_eq!(
            scheduler.queue_len(&alpha),
            10,
            "a refused event never reaches the queue"
        );
    }

    // ---- Batches and stream credit ---------------------------------------------------------------

    /// REQ: HST-204
    #[test]
    fn batch_limits_match_the_manual() {
        assert_eq!(BATCH_MAX_EVENTS, 256);
        assert_eq!(BATCH_FLUSH_MS, 2);
    }

    /// REQ: HST-204
    #[test]
    fn empty_batch_builder_has_nothing_to_deliver() {
        let mut builder = BatchBuilder::new();
        assert!(builder.take_ready(0).is_none());
    }

    /// REQ: HST-204
    #[test]
    fn single_event_waits_for_the_flush_window() {
        let mut builder = BatchBuilder::new();
        builder.push(tree_changed(), 0, false);
        assert!(
            builder.take_ready(1).is_none(),
            "1 ms is inside the 2 ms window"
        );
        assert_eq!(builder.take_ready(2).map(|batch| batch.len()), Some(1));
    }

    /// REQ: HST-204
    #[test]
    fn user_action_flushes_at_once() {
        let mut builder = BatchBuilder::new();
        builder.push(tree_changed(), 0, true);
        assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(1));
    }

    /// REQ: HST-204
    #[test]
    fn latency_critical_kind_flushes_at_once() {
        let mut builder = BatchBuilder::new();
        builder.push(key_pressed(), 0, false);
        assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(1));
    }

    /// REQ: HST-204
    #[test]
    fn full_batch_of_256_flushes_at_once() {
        let mut builder = BatchBuilder::new();
        for _ in 0..BATCH_MAX_EVENTS {
            builder.push(tree_changed(), 0, false);
        }
        assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(256));
        builder.push(tree_changed(), 0, false);
        assert!(
            builder.take_ready(0).is_none(),
            "one event is not a full batch"
        );
        assert_eq!(builder.take_ready(2).map(|batch| batch.len()), Some(1));
    }

    /// REQ: HST-204
    #[test]
    fn larger_pending_list_is_split_in_order() {
        let mut builder = BatchBuilder::new();
        for _ in 0..300 {
            builder.push(tree_changed(), 0, false);
        }
        assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(256));
        assert!(builder.take_ready(0).is_none());
        assert_eq!(builder.take_ready(2).map(|batch| batch.len()), Some(44));
    }

    /// REQ: HST-205
    #[test]
    fn stream_window_and_chunk_limits_match_the_manual() {
        assert_eq!(CREDIT_WINDOW_BYTES, 262_144);
        assert_eq!(MAX_CHUNK_BYTES, 65_536);
    }

    /// REQ: HST-205
    #[test]
    fn fresh_stream_may_read() {
        let credit = StreamCredit::new();
        assert_eq!(credit.in_flight(), 0);
        assert!(credit.should_read());
    }

    /// REQ: HST-205
    #[test]
    fn enqueued_and_consumed_bytes_adjust_in_flight() {
        let mut credit = StreamCredit::new();
        credit.enqueued(100);
        assert_eq!(credit.in_flight(), 100);
        credit.consumed(40);
        assert_eq!(credit.in_flight(), 60);
    }

    /// REQ: HST-205
    #[test]
    fn reader_stops_at_a_full_window() {
        let mut credit = StreamCredit::new();
        credit.enqueued(CREDIT_WINDOW_BYTES);
        assert!(!credit.should_read());
    }

    /// REQ: HST-205
    #[test]
    fn reader_resumes_only_when_in_flight_falls_to_half_the_window() {
        let mut credit = StreamCredit::new();
        credit.enqueued(CREDIT_WINDOW_BYTES);
        credit.consumed(CREDIT_WINDOW_BYTES / 2 - 1);
        assert_eq!(credit.in_flight(), CREDIT_WINDOW_BYTES / 2 + 1);
        assert!(
            !credit.should_read(),
            "above half the window the reader stays paused"
        );
        credit.consumed(1);
        assert_eq!(credit.in_flight(), CREDIT_WINDOW_BYTES / 2);
        assert!(
            credit.should_read(),
            "at exactly half the window the reader resumes"
        );
    }

    /// REQ: HST-205
    #[test]
    fn split_chunks_never_exceed_the_chunk_limit() {
        assert!(StreamCredit::split_chunks(0).is_empty());
        assert_eq!(StreamCredit::split_chunks(1), vec![1]);
        assert_eq!(StreamCredit::split_chunks(65_536), vec![65_536]);
        assert_eq!(StreamCredit::split_chunks(65_537), vec![65_536, 1]);
        assert_eq!(
            StreamCredit::split_chunks(CREDIT_WINDOW_BYTES),
            vec![65_536; 4]
        );
    }

    // ---- Execution budget ------------------------------------------------------------------------

    /// REQ: HST-207
    #[test]
    fn fresh_execution_bucket_holds_two_thousand_milliseconds() {
        let bucket = ExecutionBucket::new();
        assert_eq!(bucket.balance_ms(), 2_000);
        assert!(bucket.try_submit().is_ok());
    }

    /// REQ: HST-207
    #[test]
    fn empty_bucket_refuses_submissions_with_quota_exceeded() {
        let mut bucket = ExecutionBucket::new();
        bucket.charge_cpu_ms(1_999);
        assert_eq!(bucket.balance_ms(), 1);
        assert!(
            bucket.try_submit().is_ok(),
            "a positive balance still accepts"
        );
        bucket.charge_cpu_ms(1);
        assert_eq!(bucket.balance_ms(), 0);
        let error = expect_quota_refusal(bucket.try_submit());
        assert_eq!(error.code, ErrorCode::QuotaExceeded);
        assert_eq!(error.retry_after_ms, Some(100));
    }

    /// REQ: HST-207
    #[test]
    fn debt_gives_retry_after_until_it_is_repaid() {
        let mut bucket = ExecutionBucket::new();
        bucket.charge_cpu_ms(2_500);
        assert_eq!(bucket.balance_ms(), -500);
        let error = expect_quota_refusal(bucket.try_submit());
        assert_eq!(error.code, ErrorCode::QuotaExceeded);
        assert_eq!(error.retry_after_ms, Some(1_000));
    }

    /// REQ: HST-207
    #[test]
    fn payload_is_charged_one_millisecond_per_64_kib() {
        let mut bucket = ExecutionBucket::new();
        bucket.charge_payload_bytes(0);
        assert_eq!(bucket.balance_ms(), 2_000);
        bucket.charge_payload_bytes(65_536);
        assert_eq!(bucket.balance_ms(), 1_999);
        bucket.charge_payload_bytes(131_072);
        assert_eq!(bucket.balance_ms(), 1_997);
    }

    /// REQ: HST-207
    #[test]
    fn bucket_refills_at_half_a_millisecond_per_millisecond_capped_at_capacity() {
        let mut bucket = ExecutionBucket::new();
        bucket.charge_cpu_ms(500);
        assert_eq!(bucket.balance_ms(), 1_500);
        bucket.refill(Duration::from_millis(500));
        assert_eq!(bucket.balance_ms(), 1_750);
        bucket.refill(Duration::from_secs(10));
        assert_eq!(bucket.balance_ms(), 2_000);
    }

    /// REQ: HST-207
    #[test]
    fn retry_after_is_at_least_100_and_covers_the_debt() {
        assert_eq!(retry_after_ms(0), 100);
        assert_eq!(retry_after_ms(-1), 100);
        assert_eq!(retry_after_ms(-50), 100);
        assert_eq!(retry_after_ms(-51), 110);
        assert_eq!(retry_after_ms(-100), 200);
        assert_eq!(retry_after_ms(-500), 1_000);
    }

    // ---- Cancellation ----------------------------------------------------------------------------

    /// REQ: HST-210
    #[test]
    fn teardown_bounds_match_the_manual() {
        assert_eq!(HTTP_FILE_TEARDOWN_MS, 100);
        assert_eq!(CHILD_KILL_GRACE_MS, 2_000);
        assert_eq!(JOB_CANCEL_GRACE_MS, 1_000);
        assert_eq!(
            teardown_bound(CancelledWork::Http),
            Duration::from_millis(100)
        );
        assert_eq!(
            teardown_bound(CancelledWork::FileRead),
            Duration::from_millis(100)
        );
        assert_eq!(
            teardown_bound(CancelledWork::Child),
            Duration::from_millis(2_000)
        );
        assert_eq!(
            teardown_bound(CancelledWork::Job),
            Duration::from_millis(1_000)
        );
    }

    /// REQ: HST-210
    #[test]
    fn child_gets_term_then_kill_after_two_seconds() {
        let schedule = child_signal_schedule();
        assert_eq!(schedule.len(), 2);
        assert_eq!(schedule[0].0, ChildSignal::Term);
        assert_eq!(
            schedule[1],
            (ChildSignal::Kill, Duration::from_millis(2_000))
        );
    }

    /// REQ: HST-210
    #[test]
    fn job_is_trapped_after_its_cancel_grace_period() {
        assert_eq!(
            job_cancel_verdict(1_000, 1_999, false),
            JobCancelVerdict::Running
        );
        assert_eq!(
            job_cancel_verdict(1_000, 2_500, false),
            JobCancelVerdict::Trap
        );
        assert_eq!(
            job_cancel_verdict(1_000, 5_000, true),
            JobCancelVerdict::Running,
            "a job that returned is not trapped"
        );
    }

    /// REQ: HST-209
    #[test]
    fn each_cancellation_cause_produces_its_documented_outcome() {
        assert_eq!(
            cancellation_outcome(CancelCause::SideStopped),
            Outcome::Cancelled
        );
        assert_eq!(
            cancellation_outcome(CancelCause::PluginAsked),
            Outcome::Cancelled
        );
        assert_eq!(
            cancellation_outcome(CancelCause::Shutdown),
            Outcome::Cancelled
        );

        let revoked = cancellation_outcome(CancelCause::GrantRevoked);
        assert!(
            matches!(
                revoked,
                Outcome::Failed(PluginError {
                    code: ErrorCode::Revoked,
                    ..
                })
            ),
            "got {revoked:?}"
        );
        let timed_out = cancellation_outcome(CancelCause::DeadlinePassed);
        assert!(
            matches!(
                timed_out,
                Outcome::Failed(PluginError {
                    code: ErrorCode::TimedOut,
                    ..
                })
            ),
            "got {timed_out:?}"
        );
        let trapped = cancellation_outcome(CancelCause::ProviderTrapped);
        assert!(
            matches!(
                trapped,
                Outcome::Failed(PluginError {
                    code: ErrorCode::Cancelled,
                    ..
                })
            ),
            "got {trapped:?}"
        );
        let breach = cancellation_outcome(CancelCause::ProviderBreach);
        assert!(
            matches!(
                breach,
                Outcome::Failed(PluginError {
                    code: ErrorCode::Internal,
                    ..
                })
            ),
            "got {breach:?}"
        );
    }

    /// REQ: HST-209
    #[test]
    fn request_completes_once_and_a_later_cancel_changes_nothing() {
        let mut ledger = RequestLedger::new();
        let request = RequestRef::Call(RequestId(1));
        ledger.open(request.clone());
        ledger.complete(request.clone(), Outcome::Ok);
        ledger.complete(request.clone(), Outcome::Cancelled);
        ledger.cancel(request.clone());
        assert_eq!(ledger.outcome_of(request.clone()), Some(Outcome::Ok));
        assert_eq!(ledger.completion_count(request), 1);
    }

    /// REQ: HST-209
    #[test]
    fn cancelling_an_open_request_records_cancelled() {
        let mut ledger = RequestLedger::new();
        let request = RequestRef::Submission(7);
        ledger.open(request.clone());
        assert_eq!(ledger.outcome_of(request.clone()), None);
        ledger.cancel(request.clone());
        assert_eq!(ledger.outcome_of(request), Some(Outcome::Cancelled));
    }

    /// REQ: HST-209
    #[test]
    fn unknown_request_has_no_outcome_and_no_completion() {
        let ledger = RequestLedger::new();
        let request = RequestRef::Call(RequestId(99));
        assert_eq!(ledger.outcome_of(request.clone()), None);
        assert_eq!(ledger.completion_count(request), 0);
    }

    // ---- Hook chains -----------------------------------------------------------------------------

    /// REQ: HST-211
    #[test]
    fn empty_hook_chain_runs_nothing_and_never_blocks_the_owner() {
        let run = run_hook(ProcessKind::Server, Vec::new(), 0);
        assert!(run.runs.is_empty());
        assert_eq!(run.owner_blocked_ms, 0);
    }

    /// REQ: HST-211
    #[test]
    fn handlers_of_one_chain_run_strictly_one_after_another() {
        let handlers = vec![
            handler("example.first", ProcessKind::Server, 50, 0, 10),
            handler("example.second", ProcessKind::Server, 50, 0, 10),
        ];
        let run = run_hook(ProcessKind::Server, handlers, 0);
        assert_eq!(run.owner_blocked_ms, 0);
        assert_eq!(run.runs.len(), 2);
        assert_eq!(run.runs[0].started_ms, Some(0));
        assert_eq!(run.runs[0].ended_ms, Some(10));
        assert_eq!(run.runs[1].started_ms, Some(10));
        assert_eq!(run.runs[1].ended_ms, Some(20));
    }

    /// REQ: HST-212
    #[test]
    fn handler_that_cannot_start_within_its_deadline_is_skipped_as_pass() {
        let handlers = vec![
            handler("example.blocked", ProcessKind::Server, 20, 100, 5),
            handler("example.free", ProcessKind::Server, 50, 0, 5),
        ];
        let run = run_hook(ProcessKind::Server, handlers, 0);
        assert_eq!(run.runs[0].started_ms, None);
        assert!(run.runs[0].skipped_as_pass);
        assert!(run.runs[0].breach, "a skipped handler counts a breach");
        assert_eq!(
            run.runs[1].started_ms,
            Some(0),
            "the next handler still runs"
        );
    }

    /// REQ: HST-212
    #[test]
    fn handler_that_overruns_its_deadline_counts_a_breach() {
        let handlers = vec![handler("example.late", ProcessKind::Server, 20, 0, 30)];
        let run = run_hook(ProcessKind::Server, handlers, 0);
        assert_eq!(run.runs[0].started_ms, Some(0));
        assert_eq!(run.runs[0].ended_ms, Some(30));
        assert!(run.runs[0].breach, "a late handler counts a breach");
    }

    /// REQ: HST-211
    #[test]
    fn hook_chain_cap_is_measured_from_the_first_start() {
        let handlers = vec![
            handler("example.one", ProcessKind::Server, 100, 0, 60),
            handler("example.two", ProcessKind::Server, 100, 0, 60),
            handler("example.three", ProcessKind::Server, 100, 0, 60),
        ];
        let run = run_hook(ProcessKind::Server, handlers, 0);
        assert_eq!(
            run.runs[1].started_ms,
            Some(60),
            "started inside the 100 ms cap"
        );
        assert_eq!(
            run.runs[2].started_ms, None,
            "started at 120 ms, after the cap"
        );
        assert!(run.runs[2].skipped_by_chain_cap);
    }

    /// REQ: HST-213
    #[test]
    fn hook_chain_skips_handlers_hosted_by_another_process() {
        let handlers = vec![
            handler("example.client", ProcessKind::Client, 50, 0, 5),
            handler("example.server", ProcessKind::Server, 50, 0, 5),
        ];
        let run = run_hook(ProcessKind::Server, handlers, 0);
        assert_eq!(run.runs[0].started_ms, None);
        assert_eq!(run.runs[0].ended_ms, None);
        assert_eq!(run.runs[1].started_ms, Some(0));
    }
}
