# API gaps of lane host-sched (tests/sched and tests/loom)

All items are used by the tests exactly as written. Module path is `ilium_plugin_host::<module>`.

## Cargo (HST-all)
- ilium-plugin-host Cargo.toml: `[[test]] name = "sched" path = "tests/sched/main.rs" required-features = ["testing"]` and `[[test]] name = "loom" path = "tests/loom/main.rs" required-features = ["loom"]` (or a self dev-dependency with feature `testing`); the sched target uses `HostHarness`, `NativeGuest`, `FakeKernel`.
- ilium-plugin-host dev-dependency: none new (proptest, tempfile, serde_json, tokio `rt` are present).

## scheduler.rs (HST-180..216)
- `pub const SIDE_CALLS_PER_TICK: usize = 4; pub const TICK_MS: u64 = 10;` HST-185
- `pub const BATCH_MAX_EVENTS: usize = 256; pub const BATCH_FLUSH_MS: u64 = 2;` HST-204
- `pub const CREDIT_WINDOW_BYTES: usize = 262_144; pub const MAX_CHUNK_BYTES: usize = 65_536;` HST-205
- `pub const HTTP_FILE_TEARDOWN_MS: u64 = 100; pub const CHILD_KILL_GRACE_MS: u64 = 2_000; pub const JOB_CANCEL_GRACE_MS: u64 = 1_000;` HST-210
- `pub const EXECUTION_BUCKET_CAPACITY_MS: i64 = 2_000; pub const EXECUTION_BUCKET_REFILL_MS_PER_SECOND: u64 = 500;` HST-207
- `pub const SCENE_THREAD_NAME: &str = "ilium-plugin-scene"; pub const EPOCH_THREAD_NAME: &str = "ilium-plugin-epoch"; pub const EXECUTOR_THREAD_PREFIX: &str = "ilium-plugin-"; pub const JOB_THREAD_PREFIX: &str = "ilium-job-";` HST-180, HST-183
- `pub fn executor_threads(process: ilium_plugin_api::ProcessKind, cores: usize) -> usize` (Server clamp(cores/2,2,4); Client clamp(cores/4,1,2); Cli 1) HST-182
- `pub fn job_bank_threads(cores: usize) -> usize` (min(4, cores/2), at least 1) HST-182
- `pub enum CallKind { Activate, Deactivate, HookHandler, Collect, ServiceProvider, InferenceProvider, MenuItems, QueryCatalog, ListVirtualChildren, Tokens, SettingsValidate, Detector, ClientRequestAnswer, OnEvents, Render, Annotate, SceneFrame, JobRun, ServiceCallWaitingForActivation }` deriving Debug, Clone, Copy, PartialEq, Eq, Hash; `CallKind::ALL: [CallKind; 19]`; `fn priority(self) -> Option<u8>` (None for SceneFrame, JobRun); `fn may_yield(self) -> bool`; `fn class(self) -> Option<CallClass>`. HST-184, HST-196
- `pub struct ExecutedCall { pub plugin: PluginId, pub kind: CallKind, pub ticket: u64, pub thread_name: String, pub yields: u32 }` (Debug, Clone) and `pub struct AbandonedCall { pub plugin: PluginId, pub kind: CallKind, pub ticket: u64 }` HST-184, HST-187
- `Scheduler::enqueue(&self, plugin: &PluginId, kind: CallKind, now: Instant, work: Box<dyn FnOnce() + Send>) -> u64` (returns a ticket, ascending) HST-184
- `Scheduler::executed(&self) -> Vec<ExecutedCall>` (in start order) HST-184
- `Scheduler::abandoned(&self) -> Vec<AbandonedCall>` HST-187
- `Scheduler::breach_count(&self, plugin: &PluginId, class: CallClass) -> u64` HST-216
- `Scheduler::max_concurrent_calls_in_store(&self, plugin: &PluginId) -> usize` HST-184
- `Scheduler::set_store_busy(&self, plugin: &PluginId, busy: bool)` (test hook: a call occupies the Store) HST-187
- `Scheduler::begin_activation(&self, plugin: &PluginId)` and `Scheduler::finish_activation(&self, plugin: &PluginId, succeeded: bool)` HST-197
- `Scheduler::log_lines(&self) -> Vec<crate::telemetry::LogLine>` HST-216
- `Scheduler::register_side(&self, plugin: &PluginId, event_queue: usize)` HST-202
- `pub enum EventPush { Queued, Coalesced, DroppedOldest, DiscardedIncoming, Stuck }` (Debug, Clone, Copy, PartialEq, Eq) and `Scheduler::push_event(&self, plugin: &PluginId, event: Event, now: Instant) -> EventPush` HST-202, HST-203
- `pub enum QueuePolicy { Coalesce, DropOldest, NeverDropped }` and `pub fn event_queue_policy(event: &Event) -> QueuePolicy` HST-202
- `Scheduler::set_source_rate(&self, plugin: &PluginId, source: &str, per_second: u32)` and `Scheduler::push_event_from_source(&self, plugin: &PluginId, source: &str, event: Event, now: Instant) -> Result<EventPush, PluginError>` (refused `rate-limited` before enqueue) HST-206
- `Scheduler::set_bucket_balance_ms(&self, plugin: &PluginId, balance_ms: i64)` HST-208
- `pub struct BatchBuilder` with `new() -> BatchBuilder`, `push(&mut self, event: Event, at_ms: u64, from_user_action: bool)`, `take_ready(&mut self, now_ms: u64) -> Option<Vec<Event>>` HST-204, HST-214
- `pub struct StreamCredit` with `new() -> StreamCredit`, `enqueued(&mut self, bytes: usize)`, `consumed(&mut self, bytes: usize)`, `in_flight(&self) -> usize`, `should_read(&self) -> bool`, `split_chunks(len: usize) -> Vec<usize>` (associated fn) HST-205
- `pub struct ExecutionBucket` with `new() -> ExecutionBucket`, `charge_cpu_ms(&mut self, ms: u64)`, `charge_payload_bytes(&mut self, bytes: u64)`, `refill(&mut self, elapsed_ms: u64)`, `balance_ms(&self) -> i64`, `try_submit(&self) -> Result<(), PluginError>`; `pub fn retry_after_ms(balance_ms: i64) -> u32` HST-207
- `pub enum CancelCause { SideStopped, GrantRevoked, PluginAsked, Shutdown, DeadlinePassed, ProviderTrapped, ProviderBreach }` and `pub fn cancellation_outcome(cause: CancelCause) -> ilium_plugin_api::generated::types::requests::Outcome` (pending-request outcome) HST-209
- `pub struct RequestLedger` with `new()`, `open(&mut self, request: RequestRef)`, `complete(&mut self, request: RequestRef, outcome: Outcome)`, `cancel(&mut self, request: RequestRef)`, `outcome_of(&self, request: RequestRef) -> Option<Outcome>`, `completion_count(&self, request: RequestRef) -> usize` HST-209
- `pub enum CancelledWork { Http, FileRead, Child, Job }`, `pub fn teardown_bound(work: CancelledWork) -> Duration` (Http/FileRead 100 ms, Job 1000 ms), `pub fn child_signal_schedule() -> Vec<(ChildSignal, Duration)>`, `pub enum ChildSignal { Term, Kill }` (Debug, Clone, Copy, PartialEq, Eq), `pub fn job_cancel_verdict(flag_set_at_ms: u64, now_ms: u64, returned: bool) -> JobCancelVerdict`, `pub enum JobCancelVerdict { Running, Trap }` HST-210
- `pub struct HandlerSpec { pub plugin: PluginId, pub process: ProcessKind, pub deadline_ms: u64, pub busy_until_ms: u64, pub run_ms: u64 }`, `pub struct HandlerRun { pub plugin: PluginId, pub started_ms: Option<u64>, pub ended_ms: Option<u64>, pub skipped_as_pass: bool, pub breach: bool, pub skipped_by_chain_cap: bool }`, `pub struct HookRun { pub request: RequestId, pub owner_blocked_ms: u64, pub runs: Vec<HandlerRun>, pub log_ids: Vec<String> }`, `pub fn run_hook(owner_process: ProcessKind, handlers: Vec<HandlerSpec>, start_ms: u64) -> HookRun` (simulated clock; `log_ids` holds `extension.chain-cap`) HST-211..213

## calls.rs (HST-188..195)
- `CallClass` gains variants `MenuItems`, `QueryCatalog`, `ListVirtualChildren`, `Tokens`, `SettingsValidate` (fixed deadlines 20, 20, 20, 10, 100 ms, independent of the limits map). HST-188, HST-190
- `pub const SOFT_GRACE_MS: u64 = 100;` HST-189
- `pub enum BreachVerdict { Timely, HardTrap, SoftLate, SoftTrap }` (Debug, Clone, Copy, PartialEq, Eq) and `pub fn evaluate_breach(class: CallClass, elapsed: Duration, limits: &ResolvedLimits) -> BreachVerdict` HST-189
- `pub fn batch_call_class(batch: &[Event]) -> CallClass` (Command when a command-invoked is present, else Event) HST-192
- `pub fn streak_limit(class: CallClass) -> Option<u32>` and `pub struct StreakCounter` with `new(class: CallClass) -> StreakCounter`, `record(&mut self, timely: bool) -> bool` (true: trap the instance) HST-190
- `pub struct CallBudget` with `new(class: CallClass, limits: &ResolvedLimits) -> CallBudget`, `guest_ms(&mut self, ms: u64)`, `import_ms(&mut self, ms: u64)`, `returned(&mut self)`, `post_return_ms(&mut self, ms: u64)`, `used(&self) -> Duration`, `verdict(&self) -> BreachVerdict`, `tick_gap(&mut self, gap_ms: u64)` (a gap above 1000 ms is ignored) HST-194, HST-195

## testing/harness.rs (HST-191, 198..203, 214, 215)
- `HarnessHandle::deactivate(&self, reason: DeactivationReason) -> Option<Vec<u8>>` HST-191
- `HarnessHandle::counted_failures(&self) -> u32` HST-191, HST-203
- `HarnessHandle::call_outcomes(&self) -> Vec<(u64, Outcome)>` (completion of the calls behind cli-invoked `invocation`, http-called `call`, client-request `call`) HST-200
- `HarnessHandle::seed_retained(&self, key: &str)`, `HarnessHandle::replace_retained(&self, key: &str)`, `HarnessHandle::retained_contributions(&self) -> Vec<RetainedContribution>` with `pub struct RetainedContribution { pub key: String, pub stale: bool }` HST-201
- `HarnessHandle::trap_count(&self) -> usize` is NOT needed (use `failures()`).

## scheduler.rs, UI channel (HST-181)
- `pub struct UiWorkChannel<T>` with `new(capacity: usize) -> UiWorkChannel<T>`, `try_send(&self, item: T) -> Result<(), T>` (Err gives the item back when full), `try_recv(&self) -> Option<T>`.

## scheduler.rs, queue length (HST-202, HST-205)
- `Scheduler::queue_len(&self, plugin: &PluginId) -> usize` (events currently queued for the side).
