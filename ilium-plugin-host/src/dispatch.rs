//! The dispatcher: submissions in, outcomes out, deduplication and pending-request accounting
//! (reference-events-and-intents.md ABI-008, ABI-010, ABI-013).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::ids::RequestId;
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::{ErrorCode, PluginError};

/// ABI-013: pending requests per plugin.
pub const MAX_PENDING_PER_PLUGIN: usize = 64;

/// ABI-008: a call that returns more submissions than this traps (the 257th).
pub const MAX_SUBMISSIONS_PER_CALL: usize = 256;

/// Dispatcher settings (reference-events-and-intents.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchConfig {
    /// Pending requests allowed per plugin.
    pub max_pending_per_plugin: usize,
    /// Submissions allowed per guest call.
    pub max_submissions_per_call: usize,
    /// Deduplication window in milliseconds (ABI-010).
    pub dedup_window_ms: u64,
}

/// What one [`Dispatcher::pump`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpReport {
    /// Rounds run.
    pub rounds: u32,
    /// Events handed to guests.
    pub events_delivered: usize,
    /// Intents applied to the kernel.
    pub intents_applied: usize,
    /// Calls that trapped.
    pub traps: usize,
}

/// Acceptance checks must be read-only: the entire batch is checked before any intent is applied.
/// The owner supplies side grants and declaration checks; `KernelFacade` has no preflight API.
pub trait SubmissionAdmission: Send + Sync {
    /// Refuse an unauthorized or malformed intent without mutating kernel state.
    fn check(&self, origin: &Origin, intent: &Intent) -> Result<(), PluginError>;
}

struct ClosedAdmission;
impl SubmissionAdmission for ClosedAdmission {
    fn check(&self, origin: &Origin, _: &Intent) -> Result<(), PluginError> {
        if matches!(origin, Origin::Plugin { .. }) {
            Err(error(
                ErrorCode::NotPermitted,
                "plugin admission is not connected",
            ))
        } else {
            Ok(())
        }
    }
}

struct Pending {
    origin: Origin,
    submission: Submission,
    request: RequestRef,
}

#[derive(Default)]
struct State {
    pending: VecDeque<Pending>,
    counts: HashMap<PluginId, usize>,
    outcomes: HashMap<PluginId, VecDeque<(Origin, RequestResult)>>,
    dedup: HashMap<(PluginId, String), (Instant, RequestRef)>,
    events: VecDeque<Event>,
    next_request: u64,
    traps: usize,
}

/// Routes accepted submissions to the kernel. Guest invocation belongs to the scheduler.
pub struct Dispatcher {
    config: DispatchConfig,
    kernel: Arc<dyn KernelFacade>,
    admission: Arc<dyn SubmissionAdmission>,
    state: Mutex<State>,
    pump_lock: Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError::new(code, message)
}

fn plugin(origin: &Origin) -> Option<&PluginId> {
    match origin {
        Origin::Plugin { plugin, .. } => Some(plugin),
        _ => None,
    }
}

fn malformed(submission: &Submission) -> Option<PluginError> {
    let mandatory = matches!(
        submission.intent,
        Intent::SendInput(_) | Intent::AgentAction(_)
    );
    let valid = submission.dedup_key.as_ref().is_none_or(|key| {
        !key.is_empty()
            && key.len() <= 128
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    });
    if !valid || (mandatory && submission.dedup_key.is_none()) {
        Some(error(
            ErrorCode::InvalidArgument,
            "invalid or missing dedup key",
        ))
    } else {
        None
    }
}

impl Dispatcher {
    /// Creates a dispatcher with plugin submissions denied until an admission adapter is supplied.
    /// Kernel/user origins remain usable. Production plugin hosts use `with_admission`.
    pub fn new(kernel: Arc<dyn KernelFacade>, config: DispatchConfig) -> Dispatcher {
        Self::with_admission(kernel, config, Arc::new(ClosedAdmission))
    }

    /// Connects read-only grant/declaration checks before atomic batch admission.
    pub fn with_admission(
        kernel: Arc<dyn KernelFacade>,
        mut config: DispatchConfig,
        admission: Arc<dyn SubmissionAdmission>,
    ) -> Dispatcher {
        config.max_submissions_per_call = config
            .max_submissions_per_call
            .min(MAX_SUBMISSIONS_PER_CALL);
        config.max_pending_per_plugin = config.max_pending_per_plugin.min(256);
        config.dedup_window_ms = 600_000;
        Dispatcher {
            config,
            kernel,
            admission,
            state: Mutex::new(State::default()),
            pump_lock: Mutex::new(()),
        }
    }

    /// Queues a returned submission; completions retain the guest's correlation number.
    pub fn submit(&self, origin: Origin, submission: Submission) -> RequestRef {
        let request = RequestRef::Submission(submission.correlation);
        self.submit_batch(origin, vec![submission]);
        request
    }

    /// Admits a guest result atomically for malformed submissions and missing grants.
    /// False means the call trapped at the submission count boundary: nothing completes.
    pub fn submit_batch(&self, origin: Origin, submissions: Vec<Submission>) -> bool {
        if submissions.len() > self.config.max_submissions_per_call {
            lock(&self.state).traps += 1;
            return false;
        }
        // Malformation wins even when an earlier submission lacks a grant (RQO-009).
        let refusal = submissions
            .iter()
            .enumerate()
            .find_map(|(index, item)| malformed(item).map(|e| (index, e)))
            .or_else(|| {
                submissions.iter().enumerate().find_map(|(index, item)| {
                    self.admission
                        .check(&origin, &item.intent)
                        .err()
                        .map(|e| (index, e))
                })
            });
        let mut state = lock(&self.state);
        for (index, submission) in submissions.into_iter().enumerate() {
            let request = RequestRef::Submission(submission.correlation);
            if let Some((bad_index, reason)) = &refusal {
                let reason = if index == *bad_index {
                    reason.clone()
                } else {
                    error(
                        ErrorCode::Cancelled,
                        &format!("batch refused: submission {bad_index}"),
                    )
                };
                Self::complete(&mut state, &origin, request, Outcome::Refused(reason));
                continue;
            }
            Self::enqueue(
                &mut state,
                &self.config,
                origin.clone(),
                submission,
                request,
            );
        }
        true
    }

    /// Import submission returns a host request id; immediate refusals return no id.
    pub fn try_submit_call(
        &self,
        origin: Origin,
        submission: Submission,
    ) -> Result<RequestId, PluginError> {
        if let Some(reason) = malformed(&submission) {
            return Err(reason);
        }
        self.admission.check(&origin, &submission.intent)?;
        let mut state = lock(&self.state);
        if plugin(&origin).is_some_and(|id| {
            state.counts.get(id).copied().unwrap_or(0) >= self.config.max_pending_per_plugin
        }) {
            return Err(error(ErrorCode::Busy, "pending request limit reached"));
        }
        let next = state
            .next_request
            .checked_add(1)
            .ok_or_else(|| error(ErrorCode::Busy, "request ids exhausted"))?;
        state.next_request = next;
        let request = RequestId(next);
        Self::enqueue(
            &mut state,
            &self.config,
            origin,
            submission,
            RequestRef::Call(request),
        );
        Ok(request)
    }

    fn enqueue(
        state: &mut State,
        config: &DispatchConfig,
        origin: Origin,
        submission: Submission,
        request: RequestRef,
    ) {
        if let Some(id) = plugin(&origin) {
            let count = state.counts.entry(id.clone()).or_default();
            if *count >= config.max_pending_per_plugin {
                Self::complete(
                    state,
                    &origin,
                    request,
                    Outcome::Refused(error(ErrorCode::Busy, "pending request limit reached")),
                );
                return;
            }
            *count += 1;
        }
        state.pending.push_back(Pending {
            origin,
            submission,
            request,
        });
    }

    fn complete(state: &mut State, origin: &Origin, request: RequestRef, outcome: Outcome) {
        if let Some(id) = plugin(origin) {
            state
                .outcomes
                .entry(id.clone())
                .or_default()
                .push_back((origin.clone(), RequestResult { request, outcome }));
        }
    }

    /// Transfers kernel events into a bounded staging queue. On pressure the caller retains
    /// ownership and must retry; no subscriptions or guest deliveries are fabricated here.
    pub fn deliver(&self, events: Vec<Event>) -> Result<(), Vec<Event>> {
        let mut state = lock(&self.state);
        if events.len() > 4096usize.saturating_sub(state.events.len()) {
            return Err(events);
        }
        state.events.extend(events);
        Ok(())
    }

    /// Scheduler adapter drains emission order, then performs subscription/grant routing and
    /// per-plugin queue policy. Taking these events is not itself a guest call.
    pub fn take_events(&self, maximum: usize) -> Vec<Event> {
        let mut state = lock(&self.state);
        let count = maximum.min(state.events.len());
        state.events.drain(..count).collect()
    }

    /// Takes completions for enqueueing as request-completed events for this plugin.
    /// Pending capacity is released here, at the handoff to the plugin queue (API-073).
    pub fn outcomes(&self, plugin: &PluginId) -> Vec<RequestResult> {
        let mut state = lock(&self.state);
        let results: Vec<_> = state
            .outcomes
            .remove(plugin)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, result)| result)
            .collect();
        if let Some(count) = state.counts.get_mut(plugin) {
            *count = count.saturating_sub(
                results
                    .iter()
                    .filter(|result| !matches!(result.outcome, Outcome::Refused(_)))
                    .count(),
            );
        }
        results
    }

    /// Production completion handoff keeps side and instance identity. The adapter must
    /// enqueue every returned result; use this instead of the plugin-wide diagnostic drain.
    pub fn outcomes_for(&self, origin: &Origin) -> Vec<RequestResult> {
        let Some(id) = plugin(origin) else {
            return Vec::new();
        };
        let mut state = lock(&self.state);
        let mut results = Vec::new();
        if let Some(queue) = state.outcomes.get_mut(id) {
            queue.retain(|(owner, result)| {
                if owner == origin {
                    results.push(result.clone());
                    false
                } else {
                    true
                }
            });
        }
        if let Some(count) = state.counts.get_mut(id) {
            *count = count.saturating_sub(
                results
                    .iter()
                    .filter(|result| !matches!(result.outcome, Outcome::Refused(_)))
                    .count(),
            );
        }
        results
    }

    /// Stops queued work of exactly one side instance and completes each cancelled request.
    /// Kernel application already in flight is owned by the kernel and cannot be undone here.
    pub fn cancel_origin(&self, origin: &Origin) {
        let mut state = lock(&self.state);
        let mut cancelled = Vec::new();
        state.pending.retain(|item| {
            if &item.origin == origin {
                cancelled.push(item.request.clone());
                false
            } else {
                true
            }
        });
        for request in cancelled {
            Self::complete(&mut state, origin, request, Outcome::Cancelled);
        }
    }

    /// Runs bounded application rounds. Each round snapshots the ready work so reentrant
    /// kernel submissions wait for the next round. No guest delivery occurs in this adapter.
    pub fn pump(&self, max_rounds: u32) -> PumpReport {
        let _serial = lock(&self.pump_lock);
        let mut report = PumpReport {
            rounds: 0,
            events_delivered: 0,
            intents_applied: 0,
            traps: 0,
        };
        for _ in 0..max_rounds {
            let pending: Vec<_> = {
                let mut state = lock(&self.state);
                state.pending.drain(..).collect()
            };
            if pending.is_empty() {
                break;
            }
            report.rounds += 1;
            for item in pending {
                let now = Instant::now();
                let key = plugin(&item.origin)
                    .zip(item.submission.dedup_key.as_ref())
                    .map(|(id, key)| (id.clone(), key.clone()));
                let duplicate = {
                    let mut state = lock(&self.state);
                    state.dedup.retain(|_, (at, _)| {
                        now.duration_since(*at) < Duration::from_millis(self.config.dedup_window_ms)
                    });
                    key.as_ref()
                        .and_then(|key| state.dedup.get(key).map(|(_, request)| request.clone()))
                };
                let outcome = if let Some(original) = duplicate {
                    Outcome::Duplicate(original)
                } else {
                    report.intents_applied += 1;
                    let outcome = self
                        .kernel
                        .apply_intent(&item.origin, item.submission.intent);
                    if !matches!(outcome, Outcome::Refused(_)) {
                        if let Some(key) = key {
                            lock(&self.state)
                                .dedup
                                .insert(key, (now, item.request.clone()));
                        }
                    }
                    outcome
                };
                let mut state = lock(&self.state);
                // Kernel refusals never enter pending accounting; release their admitted slot.
                if matches!(outcome, Outcome::Refused(_)) {
                    if let Some(id) = plugin(&item.origin) {
                        if let Some(count) = state.counts.get_mut(id) {
                            *count = count.saturating_sub(1);
                        }
                    }
                }
                Self::complete(&mut state, &item.origin, item.request, outcome);
            }
        }
        report.traps = std::mem::take(&mut lock(&self.state).traps);
        report
    }

    /// The settings in force.
    pub fn config(&self) -> &DispatchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::{DispatchConfig, Dispatcher, MAX_PENDING_PER_PLUGIN, MAX_SUBMISSIONS_PER_CALL};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::intents::{
        AgentActionSpec, Delivery, Intent, RenameNodeSpec, SendInputSpec, Submission,
    };
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef, RequestResult};
    use ilium_plugin_api::generated::types::tree::TitleSource;
    use ilium_plugin_api::ids::PluginId;
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Kernel stand-in that records every applied intent and answers with scripted outcomes,
    /// then `Ok` once the script is empty.
    #[derive(Default)]
    struct RecordingKernel {
        applied: Mutex<Vec<(Origin, Intent)>>,
        scripted_outcomes: Mutex<VecDeque<Outcome>>,
    }

    impl RecordingKernel {
        fn new() -> Arc<RecordingKernel> {
            Arc::new(RecordingKernel::default())
        }

        fn script(&self, outcome: Outcome) {
            self.scripted_outcomes
                .lock()
                .expect("scripted outcomes lock")
                .push_back(outcome);
        }

        fn applied_intents(&self) -> Vec<(Origin, Intent)> {
            self.applied.lock().expect("applied intents lock").clone()
        }
    }

    impl KernelFacade for RecordingKernel {
        fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
            self.applied
                .lock()
                .expect("applied intents lock")
                .push((origin.clone(), intent));
            self.scripted_outcomes
                .lock()
                .expect("scripted outcomes lock")
                .pop_front()
                .unwrap_or(Outcome::Ok)
        }

        fn tree(&self) -> &dyn TreeRead {
            &EMPTY_TREE
        }

        fn panes(&self) -> &dyn PaneRead {
            &EMPTY_PANES
        }

        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }
    }

    struct EmptyTree;

    impl TreeRead for EmptyTree {
        fn nodes(&self) -> Vec<TreeNode> {
            Vec::new()
        }

        fn node(&self, _id: NodeId) -> Option<TreeNode> {
            None
        }

        fn epoch(&self) -> u64 {
            0
        }
    }

    static EMPTY_TREE: EmptyTree = EmptyTree;

    struct EmptyPanes;

    impl PaneRead for EmptyPanes {
        fn screen_text(&self, _pane: NodeId) -> Option<String> {
            None
        }

        fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
            None
        }

        fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
            None
        }
    }

    static EMPTY_PANES: EmptyPanes = EmptyPanes;

    fn default_config() -> DispatchConfig {
        DispatchConfig {
            max_pending_per_plugin: MAX_PENDING_PER_PLUGIN,
            max_submissions_per_call: MAX_SUBMISSIONS_PER_CALL,
            dedup_window_ms: 600_000,
        }
    }

    fn dispatcher_over(kernel: &Arc<RecordingKernel>) -> Dispatcher {
        Dispatcher::new(kernel.clone(), default_config())
    }

    fn plugin_origin(plugin_name: &str) -> Origin {
        Origin::Plugin {
            plugin: PluginId(plugin_name.to_string()),
            side: Side::Server,
            instance: 1,
        }
    }

    fn plugin_id(plugin_name: &str) -> PluginId {
        PluginId(plugin_name.to_string())
    }

    /// A rename submission whose target node equals its correlation number, so the applied
    /// order can be read back from the kernel.
    fn rename_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::RenameNode(RenameNodeSpec {
                node: NodeId(correlation),
                title: "build".to_string(),
                source: TitleSource::Plugin,
                expected_revision: None,
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn send_input_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::SendInput(SendInputSpec {
                pane: NodeId(7),
                text: "ls".to_string(),
                delivery: Delivery::Typed,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: true,
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn agent_action_submission(correlation: u64, dedup_key: Option<&str>) -> Submission {
        Submission {
            intent: Intent::AgentAction(AgentActionSpec {
                pane: NodeId(7),
                action_id: "approve".to_string(),
            }),
            correlation,
            dedup_key: dedup_key.map(str::to_string),
        }
    }

    fn outcome_for(results: &[RequestResult], correlation: u64) -> Outcome {
        results
            .iter()
            .find(|result| result.request == RequestRef::Submission(correlation))
            .map(|result| result.outcome.clone())
            .unwrap_or_else(|| panic!("no outcome for submission {correlation}: {results:?}"))
    }

    fn refused_code(outcome: &Outcome) -> ErrorCode {
        match outcome {
            Outcome::Refused(error) => error.code,
            other => panic!("expected refused outcome, got {other:?}"),
        }
    }

    fn applied_nodes(kernel: &RecordingKernel) -> Vec<u64> {
        kernel
            .applied_intents()
            .iter()
            .map(|(_, intent)| match intent {
                Intent::RenameNode(spec) => spec.node.0,
                other => panic!("unexpected applied intent {other:?}"),
            })
            .collect()
    }

    /// REQ: ABI-011
    #[test]
    fn submit_returns_submission_ref_carrying_the_plugin_correlation() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        let reference = dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(42, None),
        );

        assert_eq!(reference, RequestRef::Submission(42));
    }

    /// REQ: RQO-001
    #[test]
    fn submit_queues_without_applying_until_pump_runs() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, None),
        );

        assert!(kernel.applied_intents().is_empty());
        assert!(dispatcher
            .outcomes(&plugin_id("com.example.alpha"))
            .is_empty());
    }

    /// REQ: RQO-003
    #[test]
    fn pump_applies_submissions_in_submission_order() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=3 {
            dispatcher.submit(
                plugin_origin("com.example.alpha"),
                rename_submission(correlation, None),
            );
        }

        dispatcher.pump(8);

        assert_eq!(applied_nodes(&kernel), vec![1, 2, 3]);
    }

    /// REQ: RQO-001
    #[test]
    fn every_submission_completes_exactly_once() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=10 {
            dispatcher.submit(
                plugin_origin("com.example.alpha"),
                rename_submission(correlation, None),
            );
        }

        dispatcher.pump(4);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(results.len(), 10);
        for correlation in 1..=10 {
            let completions = results
                .iter()
                .filter(|result| result.request == RequestRef::Submission(correlation))
                .count();
            assert_eq!(
                completions, 1,
                "submission {correlation} completed {completions} times"
            );
        }
    }

    /// REQ: ABI-008
    #[test]
    fn pump_report_counts_applied_intents_and_no_traps_for_valid_submissions() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=3 {
            dispatcher.submit(
                plugin_origin("com.example.alpha"),
                rename_submission(correlation, None),
            );
        }

        let report = dispatcher.pump(8);

        assert_eq!(report.intents_applied, 3);
        assert_eq!(report.traps, 0);
        assert!(
            report.rounds >= 1 && report.rounds <= 8,
            "rounds was {}",
            report.rounds
        );
    }

    /// REQ: RQO-001
    #[test]
    fn pump_on_an_empty_queue_delivers_and_applies_nothing() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);

        let report = dispatcher.pump(5);

        assert_eq!(report.events_delivered, 0);
        assert_eq!(report.intents_applied, 0);
        assert_eq!(report.traps, 0);
        assert!(report.rounds <= 5, "rounds was {}", report.rounds);
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-001
    #[test]
    fn pump_with_zero_rounds_runs_nothing_and_a_later_pump_applies_the_queue() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, None),
        );

        let idle_report = dispatcher.pump(0);

        assert_eq!(idle_report.rounds, 0);
        assert_eq!(idle_report.intents_applied, 0);
        assert!(kernel.applied_intents().is_empty());

        let report = dispatcher.pump(1);

        assert_eq!(report.intents_applied, 1);
        assert_eq!(
            dispatcher.outcomes(&plugin_id("com.example.alpha")).len(),
            1
        );
    }

    /// REQ: ABI-008
    #[test]
    fn per_call_submission_cap_is_256_so_the_257th_traps() {
        assert_eq!(MAX_SUBMISSIONS_PER_CALL, 256);
        assert_eq!(MAX_SUBMISSIONS_PER_CALL + 1, 257);
    }

    /// REQ: ABI-013
    #[test]
    fn pending_limit_is_64_per_plugin_by_default() {
        assert_eq!(MAX_PENDING_PER_PLUGIN, 64);
    }

    /// REQ: ABI-013
    #[test]
    fn completed_submissions_free_pending_slots_for_new_submissions() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        for correlation in 1..=64 {
            dispatcher.submit(
                plugin_origin("com.example.alpha"),
                rename_submission(correlation, None),
            );
        }
        dispatcher.pump(8);

        for correlation in 65..=128 {
            dispatcher.submit(
                plugin_origin("com.example.alpha"),
                rename_submission(correlation, None),
            );
        }
        let report = dispatcher.pump(8);

        assert_eq!(report.intents_applied, 64);
        assert_eq!(kernel.applied_intents().len(), 128);
        assert_eq!(
            dispatcher.outcomes(&plugin_id("com.example.alpha")).len(),
            128
        );
    }

    /// REQ: ABI-011
    #[test]
    fn outcomes_are_handed_only_to_the_plugin_that_submitted_them() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, None),
        );
        dispatcher.submit(
            plugin_origin("com.example.beta"),
            rename_submission(2, None),
        );

        dispatcher.pump(8);
        let alpha = dispatcher.outcomes(&plugin_id("com.example.alpha"));
        let beta = dispatcher.outcomes(&plugin_id("com.example.beta"));

        assert_eq!(
            alpha,
            vec![RequestResult {
                request: RequestRef::Submission(1),
                outcome: Outcome::Ok,
            }]
        );
        assert_eq!(
            beta,
            vec![RequestResult {
                request: RequestRef::Submission(2),
                outcome: Outcome::Ok,
            }]
        );
    }

    /// REQ: RQO-001
    #[test]
    fn outcomes_for_a_plugin_without_submissions_are_empty() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, None),
        );
        dispatcher.pump(8);

        assert!(dispatcher
            .outcomes(&plugin_id("com.example.unknown"))
            .is_empty());
    }

    /// REQ: RQO-004
    #[test]
    fn repeated_dedup_key_completes_as_duplicate_naming_the_first_submission() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("finish:node-12")),
        );
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(2, Some("finish:node-12")),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(applied_nodes(&kernel), vec![1]);
        assert_eq!(outcome_for(&results, 1), Outcome::Ok);
        assert_eq!(
            outcome_for(&results, 2),
            Outcome::Duplicate(RequestRef::Submission(1))
        );
    }

    /// REQ: RQO-004
    #[test]
    fn dedup_key_is_scoped_to_the_plugin_so_two_plugins_do_not_collide() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("shared.key")),
        );
        dispatcher.submit(
            plugin_origin("com.example.beta"),
            rename_submission(2, Some("shared.key")),
        );

        dispatcher.pump(8);

        assert_eq!(kernel.applied_intents().len(), 2);
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.beta")), 2),
            Outcome::Ok
        );
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_of_exactly_128_characters_is_accepted() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        let key = "k".repeat(128);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some(&key)),
        );

        dispatcher.pump(8);

        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_of_129_characters_is_refused_as_invalid_argument() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        let key = "k".repeat(129);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some(&key)),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(
            refused_code(&outcome_for(&results, 1)),
            ErrorCode::InvalidArgument
        );
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn empty_dedup_key_is_refused_as_invalid_argument() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("")),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(
            refused_code(&outcome_for(&results, 1)),
            ErrorCode::InvalidArgument
        );
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_with_a_character_outside_the_allowed_set_is_refused() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("finish node")),
        );
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(2, Some("a@b")),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(
            refused_code(&outcome_for(&results, 1)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused_code(&outcome_for(&results, 2)),
            ErrorCode::InvalidArgument
        );
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: RQO-020
    #[test]
    fn dedup_key_using_every_allowed_character_class_is_accepted() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("Aa0.9_:/-")),
        );

        dispatcher.pump(8);

        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: RQO-011
    #[test]
    fn refused_submission_does_not_reserve_its_dedup_key() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Refused(PluginError {
            code: ErrorCode::Conflict,
            message: "node is busy".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("k1")),
        );
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(2, Some("k1")),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::Conflict);
        assert_eq!(outcome_for(&results, 2), Outcome::Ok);
        assert_eq!(kernel.applied_intents().len(), 2);
    }

    /// REQ: RQO-022
    #[test]
    fn accepted_submission_that_failed_keeps_its_dedup_key_reserved() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Failed(PluginError {
            code: ErrorCode::TimedOut,
            message: "timed out".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, Some("k1")),
        );
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(2, Some("k1")),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert!(matches!(outcome_for(&results, 1), Outcome::Failed(_)));
        assert_eq!(
            outcome_for(&results, 2),
            Outcome::Duplicate(RequestRef::Submission(1))
        );
        assert_eq!(kernel.applied_intents().len(), 1);
    }

    /// REQ: EVT-021
    #[test]
    fn send_input_without_dedup_key_is_refused_and_never_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            send_input_submission(1, None),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(
            refused_code(&outcome_for(&results, 1)),
            ErrorCode::InvalidArgument
        );
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: EVT-021
    #[test]
    fn agent_action_without_dedup_key_is_refused_and_never_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            agent_action_submission(1, None),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(
            refused_code(&outcome_for(&results, 1)),
            ErrorCode::InvalidArgument
        );
        assert!(kernel.applied_intents().is_empty());
    }

    /// REQ: EVT-021
    #[test]
    fn send_input_with_dedup_key_is_applied() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            send_input_submission(1, Some("input-1")),
        );

        dispatcher.pump(8);

        assert_eq!(kernel.applied_intents().len(), 1);
        assert_eq!(
            outcome_for(&dispatcher.outcomes(&plugin_id("com.example.alpha")), 1),
            Outcome::Ok
        );
    }

    /// REQ: ABI-032
    #[test]
    fn kernel_refusal_applies_to_its_own_submission_only() {
        let kernel = RecordingKernel::new();
        kernel.script(Outcome::Refused(PluginError {
            code: ErrorCode::NotFound,
            message: "no such node".to_string(),
            retry_after_ms: None,
        }));
        let dispatcher = dispatcher_over(&kernel);
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(1, None),
        );
        dispatcher.submit(
            plugin_origin("com.example.alpha"),
            rename_submission(2, None),
        );

        dispatcher.pump(8);
        let results = dispatcher.outcomes(&plugin_id("com.example.alpha"));

        assert_eq!(refused_code(&outcome_for(&results, 1)), ErrorCode::NotFound);
        assert_eq!(outcome_for(&results, 2), Outcome::Ok);
    }

    /// REQ: HST-011
    #[test]
    fn deliver_with_no_subscribers_hands_nothing_to_any_plugin() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher
            .deliver(vec![Event::TreeChanged(vec![NodeId(1)])])
            .expect("the tree change is accepted");

        let report = dispatcher.pump(4);

        assert_eq!(report.events_delivered, 0);
    }

    /// REQ: HST-011
    #[test]
    fn deliver_of_an_empty_batch_is_a_no_op() {
        let kernel = RecordingKernel::new();
        let dispatcher = dispatcher_over(&kernel);
        dispatcher
            .deliver(Vec::new())
            .expect("an empty batch is accepted");

        let report = dispatcher.pump(2);

        assert_eq!(report.events_delivered, 0);
        assert_eq!(report.intents_applied, 0);
        assert_eq!(report.traps, 0);
    }

    /// REQ: RQO-004
    #[test]
    fn config_reports_the_settings_it_was_built_with_including_the_600_second_window() {
        let kernel = RecordingKernel::new();
        let config = DispatchConfig {
            max_pending_per_plugin: MAX_PENDING_PER_PLUGIN,
            max_submissions_per_call: MAX_SUBMISSIONS_PER_CALL,
            dedup_window_ms: 600_000,
        };
        let dispatcher = Dispatcher::new(kernel.clone(), config);

        assert_eq!(*dispatcher.config(), config);
        assert_eq!(dispatcher.config().dedup_window_ms, 600_000);
    }
}
