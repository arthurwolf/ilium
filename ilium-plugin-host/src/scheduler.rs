//! Per-plugin queues, fairness and epoch interruption (reference-runtime-model.md,
//! reference-host-architecture.md HST-180..216).
//!
//! Skeleton: every function with behaviour is `unimplemented!` until the implementation phase.
//! The reference tests of the `sched` lane drive the scheduler through the testing hooks below
//! (`executed`, `abandoned`, `set_store_busy`, ...), which exist for those tests only.

use std::marker::PhantomData;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::{PluginId, RequestId};
use ilium_plugin_api::{PluginError, ProcessKind};

use crate::calls::CallClass;
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
    unimplemented!("host::scheduler::executor_threads")
}

/// Job bank threads for a core count (reference-host-architecture.md HST-184).
pub fn job_bank_threads(cores: usize) -> usize {
    unimplemented!("host::scheduler::job_bank_threads")
}

/// Bounded channel from worker threads to the UI thread (reference-host-architecture.md HST-182).
#[derive(Debug)]
pub struct UiWorkChannel<T> {
    capacity: usize,
    _items: PhantomData<T>,
}

impl<T> UiWorkChannel<T> {
    /// A channel that holds at most `capacity` items.
    pub fn new(capacity: usize) -> UiWorkChannel<T> {
        UiWorkChannel { capacity, _items: PhantomData }
    }

    /// Queues an item; a full channel hands it back (reference-host-architecture.md HST-182).
    pub fn try_send(&self, item: T) -> Result<(), T> {
        unimplemented!("host::scheduler::UiWorkChannel::try_send")
    }

    /// Takes the oldest queued item, if any.
    pub fn try_recv(&self) -> Option<T> {
        unimplemented!("host::scheduler::UiWorkChannel::try_recv")
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
        unimplemented!("host::scheduler::CallKind::priority")
    }

    /// Whether a call of this kind may yield to the next side mid-way (reference-host-architecture.md HST-187).
    pub fn may_yield(self) -> bool {
        unimplemented!("host::scheduler::CallKind::may_yield")
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

/// The scheduler of all plugin queues (reference-host-architecture.md).
#[derive(Debug)]
pub struct Scheduler {
    epoch_tick_ms: u64,
}

impl Scheduler {
    /// A scheduler with the given epoch tick (reference-host-runtime.md).
    pub fn new(epoch_tick_ms: u64) -> Scheduler {
        Scheduler { epoch_tick_ms }
    }

    /// Statistics of a plugin's queue, if it has one.
    pub fn stats(&self, plugin: &PluginId) -> Option<QueueStats> {
        unimplemented!("host::scheduler::Scheduler::stats")
    }

    /// Advances time: runs due calls, applies deadlines, rotates fairly between plugins.
    pub fn tick(&self, now: Instant) {
        unimplemented!("host::scheduler::Scheduler::tick")
    }

    /// Registers a plugin side with its event queue capacity (reference-host-architecture.md HST-201).
    pub fn register_side(&self, plugin: &PluginId, event_queue: usize) {
        unimplemented!("host::scheduler::Scheduler::register_side")
    }

    /// Queues a call and returns its ticket (reference-host-architecture.md HST-185).
    pub fn enqueue(&self, plugin: &PluginId, kind: CallKind, now: Instant, work: Box<dyn FnOnce() + Send>) -> u64 {
        unimplemented!("host::scheduler::Scheduler::enqueue")
    }

    /// Every call run so far, in order (testing hook of the `sched` lane, HST-186).
    pub fn executed(&self) -> Vec<ExecutedCall> {
        unimplemented!("host::scheduler::Scheduler::executed")
    }

    /// Every call dropped unrun so far (testing hook of the `sched` lane, HST-191).
    pub fn abandoned(&self) -> Vec<AbandonedCall> {
        unimplemented!("host::scheduler::Scheduler::abandoned")
    }

    /// Deadline breaches of a plugin per call class (reference-host-architecture.md HST-188).
    pub fn breach_count(&self, plugin: &PluginId, class: CallClass) -> u64 {
        unimplemented!("host::scheduler::Scheduler::breach_count")
    }

    /// Peak number of calls running in one plugin's store at once (reference-host-architecture.md HST-189).
    pub fn max_concurrent_calls_in_store(&self, plugin: &PluginId) -> usize {
        unimplemented!("host::scheduler::Scheduler::max_concurrent_calls_in_store")
    }

    /// Marks a plugin's store busy or free (testing hook of the `sched` lane, HST-189).
    pub fn set_store_busy(&self, plugin: &PluginId, busy: bool) {
        unimplemented!("host::scheduler::Scheduler::set_store_busy")
    }

    /// Records the start of a plugin's activation (reference-host-architecture.md HST-190).
    pub fn begin_activation(&self, plugin: &PluginId) {
        unimplemented!("host::scheduler::Scheduler::begin_activation")
    }

    /// Records the end of a plugin's activation (reference-host-architecture.md HST-190).
    pub fn finish_activation(&self, plugin: &PluginId, succeeded: bool) {
        unimplemented!("host::scheduler::Scheduler::finish_activation")
    }

    /// The log lines the scheduler wrote, oldest first (reference-host-architecture.md HST-191).
    pub fn log_lines(&self) -> Vec<LogLine> {
        unimplemented!("host::scheduler::Scheduler::log_lines")
    }

    /// Pushes an event to a side's queue (reference-host-architecture.md HST-202).
    pub fn push_event(&self, plugin: &PluginId, event: Event) -> EventPush {
        unimplemented!("host::scheduler::Scheduler::push_event")
    }

    /// Sets the per-second rate a source may push at (reference-host-architecture.md HST-203).
    pub fn set_source_rate(&self, plugin: &PluginId, per_second: u32) {
        unimplemented!("host::scheduler::Scheduler::set_source_rate")
    }

    /// Pushes an event from a rate-limited source; over the rate it is refused `rate-limited`
    /// (reference-host-architecture.md HST-203).
    pub fn push_event_from_source(&self, plugin: &PluginId, event: Event) -> Result<EventPush, PluginError> {
        unimplemented!("host::scheduler::Scheduler::push_event_from_source")
    }

    /// Events waiting in a side's queue (reference-host-architecture.md HST-202).
    pub fn queue_len(&self, plugin: &PluginId) -> usize {
        unimplemented!("host::scheduler::Scheduler::queue_len")
    }

    /// Sets a plugin's execution bucket balance (testing hook of the `sched` lane, HST-206).
    pub fn set_bucket_balance_ms(&self, plugin: &PluginId, balance_ms: i64) {
        unimplemented!("host::scheduler::Scheduler::set_bucket_balance_ms")
    }
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
    unimplemented!("host::scheduler::event_queue_policy")
}

/// Collects events into `on-events` batches (reference-host-architecture.md HST-204).
#[derive(Debug, Default)]
pub struct BatchBuilder {
    pending: Vec<Event>,
}

impl BatchBuilder {
    /// An empty builder.
    pub fn new() -> BatchBuilder {
        BatchBuilder::default()
    }

    /// Adds an event that arrived at `at_ms`; user actions flush at once (HST-204).
    pub fn push(&mut self, event: Event, at_ms: u64, from_user_action: bool) {
        unimplemented!("host::scheduler::BatchBuilder::push")
    }

    /// The batch to deliver now, if one is due (HST-204).
    pub fn take_ready(&mut self, now_ms: u64) -> Option<Vec<Event>> {
        unimplemented!("host::scheduler::BatchBuilder::take_ready")
    }
}

/// Flow control of one stream to a plugin (reference-host-architecture.md HST-205).
#[derive(Debug, Default)]
pub struct StreamCredit {
    in_flight: usize,
}

impl StreamCredit {
    /// Credit with nothing in flight.
    pub fn new() -> StreamCredit {
        StreamCredit::default()
    }

    /// Bytes queued for the plugin.
    pub fn enqueued(&mut self, bytes: usize) {
        unimplemented!("host::scheduler::StreamCredit::enqueued")
    }

    /// Bytes the plugin consumed.
    pub fn consumed(&mut self, bytes: usize) {
        unimplemented!("host::scheduler::StreamCredit::consumed")
    }

    /// Bytes queued and not yet consumed.
    pub fn in_flight(&self) -> usize {
        unimplemented!("host::scheduler::StreamCredit::in_flight")
    }

    /// Whether the reader may read more (HST-205).
    pub fn should_read(&self) -> bool {
        unimplemented!("host::scheduler::StreamCredit::should_read")
    }

    /// Chunk sizes for `len` bytes, each at most `MAX_CHUNK_BYTES` (HST-205).
    pub fn split_chunks(len: usize) -> Vec<usize> {
        unimplemented!("host::scheduler::StreamCredit::split_chunks")
    }
}

// ---- Execution budget (reference-host-architecture.md HST-206, HST-207) -----------------------

/// A plugin's execution-time bucket (reference-host-architecture.md HST-206).
#[derive(Debug, Default)]
pub struct ExecutionBucket {
    balance_ms: i64,
}

impl ExecutionBucket {
    /// A bucket at its starting balance.
    pub fn new() -> ExecutionBucket {
        ExecutionBucket::default()
    }

    /// Charges CPU time.
    pub fn charge_cpu_ms(&mut self, cpu_ms: u64) {
        unimplemented!("host::scheduler::ExecutionBucket::charge_cpu_ms")
    }

    /// Charges payload bytes at the documented rate (HST-206).
    pub fn charge_payload_bytes(&mut self, bytes: u64) {
        unimplemented!("host::scheduler::ExecutionBucket::charge_payload_bytes")
    }

    /// Refills for elapsed wall time, capped at the bucket size (HST-206).
    pub fn refill(&mut self, elapsed: Duration) {
        unimplemented!("host::scheduler::ExecutionBucket::refill")
    }

    /// The current balance; negative is debt.
    pub fn balance_ms(&self) -> i64 {
        unimplemented!("host::scheduler::ExecutionBucket::balance_ms")
    }

    /// Refuses a submission with `quota-exceeded` while in debt (HST-206, HST-207).
    pub fn try_submit(&self) -> Result<(), PluginError> {
        unimplemented!("host::scheduler::ExecutionBucket::try_submit")
    }
}

/// Milliseconds to wait until a debt is repaid at the refill rate (HST-207).
pub fn retry_after_ms(balance_ms: i64) -> u32 {
    unimplemented!("host::scheduler::retry_after_ms")
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
    unimplemented!("host::scheduler::cancellation_outcome")
}

/// Open requests and their final outcomes (reference-host-architecture.md HST-209).
#[derive(Debug, Default)]
pub struct RequestLedger {
    open: Vec<RequestRef>,
}

impl RequestLedger {
    /// An empty ledger.
    pub fn new() -> RequestLedger {
        RequestLedger::default()
    }

    /// Records an open request.
    pub fn open(&mut self, request: RequestRef) {
        unimplemented!("host::scheduler::RequestLedger::open")
    }

    /// Completes a request; a second completion is ignored (HST-209).
    pub fn complete(&mut self, request: RequestRef, outcome: Outcome) {
        unimplemented!("host::scheduler::RequestLedger::complete")
    }

    /// Cancels a request (HST-209).
    pub fn cancel(&mut self, request: RequestRef) {
        unimplemented!("host::scheduler::RequestLedger::cancel")
    }

    /// The final outcome of a request, if it ended.
    pub fn outcome_of(&self, request: RequestRef) -> Option<Outcome> {
        unimplemented!("host::scheduler::RequestLedger::outcome_of")
    }

    /// How many times a request was completed (at most one, HST-209).
    pub fn completion_count(&self, request: RequestRef) -> usize {
        unimplemented!("host::scheduler::RequestLedger::completion_count")
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
    unimplemented!("host::scheduler::teardown_bound")
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
    unimplemented!("host::scheduler::child_signal_schedule")
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
    unimplemented!("host::scheduler::job_cancel_verdict")
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
    unimplemented!("host::scheduler::run_hook")
}
