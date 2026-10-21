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
pub fn executor_threads(_process: ProcessKind, _cores: usize) -> usize {
    unimplemented!("host::scheduler::executor_threads")
}

/// Job bank threads for a core count (reference-host-architecture.md HST-184).
pub fn job_bank_threads(_cores: usize) -> usize {
    unimplemented!("host::scheduler::job_bank_threads")
}

/// Bounded channel from worker threads to the UI thread (reference-host-architecture.md HST-182).
#[derive(Debug)]
pub struct UiWorkChannel<T> {
    _items: PhantomData<T>,
}

impl<T> UiWorkChannel<T> {
    /// A channel that holds at most `capacity` items.
    pub fn new(_capacity: usize) -> UiWorkChannel<T> {
        UiWorkChannel {
            _items: PhantomData,
        }
    }

    /// Queues an item; a full channel hands it back (reference-host-architecture.md HST-182).
    pub fn try_send(&self, _item: T) -> Result<(), T> {
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
pub struct Scheduler;

impl Scheduler {
    /// A scheduler with the given epoch tick (reference-host-runtime.md).
    pub fn new(_epoch_tick_ms: u64) -> Scheduler {
        Scheduler
    }

    /// Statistics of a plugin's queue, if it has one.
    pub fn stats(&self, _plugin: &PluginId) -> Option<QueueStats> {
        unimplemented!("host::scheduler::Scheduler::stats")
    }

    /// Advances time: runs due calls, applies deadlines, rotates fairly between plugins.
    pub fn tick(&self, _now: Instant) {
        unimplemented!("host::scheduler::Scheduler::tick")
    }

    /// Registers a plugin side with its event queue capacity (reference-host-architecture.md HST-201).
    pub fn register_side(&self, _plugin: &PluginId, _event_queue: usize) {
        unimplemented!("host::scheduler::Scheduler::register_side")
    }

    /// Queues a call and returns its ticket (reference-host-architecture.md HST-185).
    pub fn enqueue(
        &self,
        _plugin: &PluginId,
        _kind: CallKind,
        _now: Instant,
        _work: Box<dyn FnOnce() + Send>,
    ) -> u64 {
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
    pub fn breach_count(&self, _plugin: &PluginId, _class: CallClass) -> u64 {
        unimplemented!("host::scheduler::Scheduler::breach_count")
    }

    /// Peak number of calls running in one plugin's store at once (reference-host-architecture.md HST-189).
    pub fn max_concurrent_calls_in_store(&self, _plugin: &PluginId) -> usize {
        unimplemented!("host::scheduler::Scheduler::max_concurrent_calls_in_store")
    }

    /// Marks a plugin's store busy or free (testing hook of the `sched` lane, HST-189).
    pub fn set_store_busy(&self, _plugin: &PluginId, _busy: bool) {
        unimplemented!("host::scheduler::Scheduler::set_store_busy")
    }

    /// Records the start of a plugin's activation (reference-host-architecture.md HST-190).
    pub fn begin_activation(&self, _plugin: &PluginId) {
        unimplemented!("host::scheduler::Scheduler::begin_activation")
    }

    /// Records the end of a plugin's activation (reference-host-architecture.md HST-190).
    pub fn finish_activation(&self, _plugin: &PluginId, _succeeded: bool) {
        unimplemented!("host::scheduler::Scheduler::finish_activation")
    }

    /// The log lines the scheduler wrote, oldest first (reference-host-architecture.md HST-191).
    pub fn log_lines(&self) -> Vec<LogLine> {
        unimplemented!("host::scheduler::Scheduler::log_lines")
    }

    /// Pushes an event to a side's queue (reference-host-architecture.md HST-202).
    pub fn push_event(&self, _plugin: &PluginId, _event: Event) -> EventPush {
        unimplemented!("host::scheduler::Scheduler::push_event")
    }

    /// Sets the per-second rate a source may push at (reference-host-architecture.md HST-203).
    pub fn set_source_rate(&self, _plugin: &PluginId, _per_second: u32) {
        unimplemented!("host::scheduler::Scheduler::set_source_rate")
    }

    /// Pushes an event from a rate-limited source; over the rate it is refused `rate-limited`
    /// (reference-host-architecture.md HST-203).
    pub fn push_event_from_source(
        &self,
        _plugin: &PluginId,
        _event: Event,
    ) -> Result<EventPush, PluginError> {
        unimplemented!("host::scheduler::Scheduler::push_event_from_source")
    }

    /// Events waiting in a side's queue (reference-host-architecture.md HST-202).
    pub fn queue_len(&self, _plugin: &PluginId) -> usize {
        unimplemented!("host::scheduler::Scheduler::queue_len")
    }

    /// Sets a plugin's execution bucket balance (testing hook of the `sched` lane, HST-206).
    pub fn set_bucket_balance_ms(&self, _plugin: &PluginId, _balance_ms: i64) {
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
pub fn event_queue_policy(_event: &Event) -> QueuePolicy {
    unimplemented!("host::scheduler::event_queue_policy")
}

/// Collects events into `on-events` batches (reference-host-architecture.md HST-204).
#[derive(Debug, Default)]
pub struct BatchBuilder;

impl BatchBuilder {
    /// An empty builder.
    pub fn new() -> BatchBuilder {
        BatchBuilder
    }

    /// Adds an event that arrived at `at_ms`; user actions flush at once (HST-204).
    pub fn push(&mut self, _event: Event, _at_ms: u64, _from_user_action: bool) {
        unimplemented!("host::scheduler::BatchBuilder::push")
    }

    /// The batch to deliver now, if one is due (HST-204).
    pub fn take_ready(&mut self, _now_ms: u64) -> Option<Vec<Event>> {
        unimplemented!("host::scheduler::BatchBuilder::take_ready")
    }
}

/// Flow control of one stream to a plugin (reference-host-architecture.md HST-205).
#[derive(Debug, Default)]
pub struct StreamCredit;

impl StreamCredit {
    /// Credit with nothing in flight.
    pub fn new() -> StreamCredit {
        StreamCredit
    }

    /// Bytes queued for the plugin.
    pub fn enqueued(&mut self, _bytes: usize) {
        unimplemented!("host::scheduler::StreamCredit::enqueued")
    }

    /// Bytes the plugin consumed.
    pub fn consumed(&mut self, _bytes: usize) {
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
    pub fn split_chunks(_len: usize) -> Vec<usize> {
        unimplemented!("host::scheduler::StreamCredit::split_chunks")
    }
}

// ---- Execution budget (reference-host-architecture.md HST-206, HST-207) -----------------------

/// A plugin's execution-time bucket (reference-host-architecture.md HST-206).
#[derive(Debug, Default)]
pub struct ExecutionBucket;

impl ExecutionBucket {
    /// A bucket at its starting balance.
    pub fn new() -> ExecutionBucket {
        ExecutionBucket
    }

    /// Charges CPU time.
    pub fn charge_cpu_ms(&mut self, _cpu_ms: u64) {
        unimplemented!("host::scheduler::ExecutionBucket::charge_cpu_ms")
    }

    /// Charges payload bytes at the documented rate (HST-206).
    pub fn charge_payload_bytes(&mut self, _bytes: u64) {
        unimplemented!("host::scheduler::ExecutionBucket::charge_payload_bytes")
    }

    /// Refills for elapsed wall time, capped at the bucket size (HST-206).
    pub fn refill(&mut self, _elapsed: Duration) {
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
pub fn retry_after_ms(_balance_ms: i64) -> u32 {
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
pub fn cancellation_outcome(_cause: CancelCause) -> Outcome {
    unimplemented!("host::scheduler::cancellation_outcome")
}

/// Open requests and their final outcomes (reference-host-architecture.md HST-209).
#[derive(Debug, Default)]
pub struct RequestLedger;

impl RequestLedger {
    /// An empty ledger.
    pub fn new() -> RequestLedger {
        RequestLedger
    }

    /// Records an open request.
    pub fn open(&mut self, _request: RequestRef) {
        unimplemented!("host::scheduler::RequestLedger::open")
    }

    /// Completes a request; a second completion is ignored (HST-209).
    pub fn complete(&mut self, _request: RequestRef, _outcome: Outcome) {
        unimplemented!("host::scheduler::RequestLedger::complete")
    }

    /// Cancels a request (HST-209).
    pub fn cancel(&mut self, _request: RequestRef) {
        unimplemented!("host::scheduler::RequestLedger::cancel")
    }

    /// The final outcome of a request, if it ended.
    pub fn outcome_of(&self, _request: RequestRef) -> Option<Outcome> {
        unimplemented!("host::scheduler::RequestLedger::outcome_of")
    }

    /// How many times a request was completed (at most one, HST-209).
    pub fn completion_count(&self, _request: RequestRef) -> usize {
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
pub fn teardown_bound(_work: CancelledWork) -> Duration {
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
pub fn job_cancel_verdict(_flag_set_at_ms: u64, _now_ms: u64, _returned: bool) -> JobCancelVerdict {
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
pub fn run_hook(
    _owner_process: ProcessKind,
    _handlers: Vec<HandlerSpec>,
    _start_ms: u64,
) -> HookRun {
    unimplemented!("host::scheduler::run_hook")
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
        job_bank_threads, job_cancel_verdict, run_hook, retry_after_ms, teardown_bound,
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
            assert_eq!(executor_threads(ProcessKind::Server, cores), threads, "cores={cores}");
        }
    }

    /// REQ: HST-182
    #[test]
    fn client_executor_threads_follow_clamp_of_a_quarter_of_the_cores() {
        // clamp(cores / 4, 1, 2), with integer division.
        let expected = [(0, 1), (1, 1), (4, 1), (7, 1), (8, 2), (16, 2)];
        for (cores, threads) in expected {
            assert_eq!(executor_threads(ProcessKind::Client, cores), threads, "cores={cores}");
        }
    }

    /// REQ: HST-182
    #[test]
    fn cli_executor_is_a_single_thread_pool() {
        for cores in [1, 4, 16] {
            assert_eq!(executor_threads(ProcessKind::Cli, cores), 1, "cores={cores}");
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
        assert_eq!(kinds, vec![CallKind::Activate, CallKind::OnEvents, CallKind::Render]);
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
        let tickets: Vec<u64> = scheduler.executed().iter().map(|call| call.ticket).collect();
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
        let alpha_count = executed.iter().filter(|call| call.plugin.0 == alpha.0).count();
        let beta_count = executed.iter().filter(|call| call.plugin.0 == beta.0).count();
        assert_eq!(alpha_count, SIDE_CALLS_PER_TICK);
        assert_eq!(beta_count, SIDE_CALLS_PER_TICK);
        assert_ne!(executed[0].plugin.0, executed[1].plugin.0, "round robin must interleave");
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
        assert!(scheduler.executed().is_empty(), "no call may enter an activating instance");
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
        assert_eq!(event_queue_policy(&output_matched()), QueuePolicy::DropOldest);
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
        assert_eq!(scheduler.push_event(&alpha, tree_changed()), EventPush::Queued);
        assert_eq!(scheduler.push_event(&alpha, tree_changed()), EventPush::Coalesced);
        assert_eq!(scheduler.queue_len(&alpha), 1);
    }

    /// REQ: HST-202
    #[test]
    fn full_queue_drops_the_oldest_drop_oldest_event() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(scheduler.push_event(&alpha, output_matched()), EventPush::Queued);
        assert_eq!(scheduler.push_event(&alpha, output_matched()), EventPush::DroppedOldest);
        assert_eq!(scheduler.queue_len(&alpha), 1);
        assert_eq!(scheduler.stats(&alpha).map(|stats| stats.dropped_events), Some(1));
    }

    /// REQ: HST-202
    #[test]
    fn full_queue_discards_an_incoming_drop_oldest_event_when_none_is_pending() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(scheduler.push_event(&alpha, key_pressed()), EventPush::Queued);
        assert_eq!(
            scheduler.push_event(&alpha, output_matched()),
            EventPush::DiscardedIncoming
        );
        assert_eq!(scheduler.stats(&alpha).map(|stats| stats.dropped_events), Some(1));
    }

    /// REQ: HST-203
    #[test]
    fn full_queue_of_never_dropped_events_makes_the_producer_stuck() {
        let scheduler = Scheduler::new(TICK_MS);
        let alpha = plugin("example.alpha");
        scheduler.register_side(&alpha, 1);
        assert_eq!(scheduler.push_event(&alpha, key_pressed()), EventPush::Queued);
        assert_eq!(scheduler.push_event(&alpha, key_pressed()), EventPush::Stuck);
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
            assert!(result.is_ok(), "event {index} is within the rate and must be accepted");
        }
        let refused = match scheduler.push_event_from_source(&alpha, key_pressed()) {
            Ok(push) => panic!("the 11th event in one second must be refused, got {push:?}"),
            Err(error) => error,
        };
        assert_eq!(refused.code, ErrorCode::RateLimited);
        assert!(refused.retry_after_ms.is_some(), "rate-limited must carry retry-after-ms");
        assert_eq!(scheduler.queue_len(&alpha), 10, "a refused event never reaches the queue");
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
        assert!(builder.take_ready(1).is_none(), "1 ms is inside the 2 ms window");
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
        assert!(builder.take_ready(0).is_none(), "one event is not a full batch");
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
        assert!(!credit.should_read(), "above half the window the reader stays paused");
        credit.consumed(1);
        assert_eq!(credit.in_flight(), CREDIT_WINDOW_BYTES / 2);
        assert!(credit.should_read(), "at exactly half the window the reader resumes");
    }

    /// REQ: HST-205
    #[test]
    fn split_chunks_never_exceed_the_chunk_limit() {
        assert!(StreamCredit::split_chunks(0).is_empty());
        assert_eq!(StreamCredit::split_chunks(1), vec![1]);
        assert_eq!(StreamCredit::split_chunks(65_536), vec![65_536]);
        assert_eq!(StreamCredit::split_chunks(65_537), vec![65_536, 1]);
        assert_eq!(StreamCredit::split_chunks(CREDIT_WINDOW_BYTES), vec![65_536; 4]);
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
        assert!(bucket.try_submit().is_ok(), "a positive balance still accepts");
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
        assert_eq!(teardown_bound(CancelledWork::Http), Duration::from_millis(100));
        assert_eq!(teardown_bound(CancelledWork::FileRead), Duration::from_millis(100));
        assert_eq!(teardown_bound(CancelledWork::Child), Duration::from_millis(2_000));
        assert_eq!(teardown_bound(CancelledWork::Job), Duration::from_millis(1_000));
    }

    /// REQ: HST-210
    #[test]
    fn child_gets_term_then_kill_after_two_seconds() {
        let schedule = child_signal_schedule();
        assert_eq!(schedule.len(), 2);
        assert_eq!(schedule[0].0, ChildSignal::Term);
        assert_eq!(schedule[1], (ChildSignal::Kill, Duration::from_millis(2_000)));
    }

    /// REQ: HST-210
    #[test]
    fn job_is_trapped_after_its_cancel_grace_period() {
        assert_eq!(job_cancel_verdict(1_000, 1_999, false), JobCancelVerdict::Running);
        assert_eq!(job_cancel_verdict(1_000, 2_500, false), JobCancelVerdict::Trap);
        assert_eq!(
            job_cancel_verdict(1_000, 5_000, true),
            JobCancelVerdict::Running,
            "a job that returned is not trapped"
        );
    }

    /// REQ: HST-209
    #[test]
    fn each_cancellation_cause_produces_its_documented_outcome() {
        assert_eq!(cancellation_outcome(CancelCause::SideStopped), Outcome::Cancelled);
        assert_eq!(cancellation_outcome(CancelCause::PluginAsked), Outcome::Cancelled);
        assert_eq!(cancellation_outcome(CancelCause::Shutdown), Outcome::Cancelled);

        let revoked = cancellation_outcome(CancelCause::GrantRevoked);
        assert!(
            matches!(revoked, Outcome::Failed(PluginError { code: ErrorCode::Revoked, .. })),
            "got {revoked:?}"
        );
        let timed_out = cancellation_outcome(CancelCause::DeadlinePassed);
        assert!(
            matches!(timed_out, Outcome::Failed(PluginError { code: ErrorCode::TimedOut, .. })),
            "got {timed_out:?}"
        );
        let trapped = cancellation_outcome(CancelCause::ProviderTrapped);
        assert!(
            matches!(trapped, Outcome::Failed(PluginError { code: ErrorCode::Cancelled, .. })),
            "got {trapped:?}"
        );
        let breach = cancellation_outcome(CancelCause::ProviderBreach);
        assert!(
            matches!(breach, Outcome::Failed(PluginError { code: ErrorCode::Internal, .. })),
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
        assert_eq!(run.runs[1].started_ms, Some(0), "the next handler still runs");
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
        assert_eq!(run.runs[1].started_ms, Some(60), "started inside the 100 ms cap");
        assert_eq!(run.runs[2].started_ms, None, "started at 120 ms, after the cap");
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
