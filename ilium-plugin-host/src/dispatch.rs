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
    ilium_plugin_api::submission::check_shape(submission).err()
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
        self.take_outcomes_for(origin, usize::MAX)
    }

    /// Transfer a bounded prefix, retaining the remaining outcomes and their
    /// pending-request admission with this dispatcher until a later handoff.
    pub fn take_outcomes_for(&self, origin: &Origin, maximum: usize) -> Vec<RequestResult> {
        let Some(id) = plugin(origin) else {
            return Vec::new();
        };
        let mut state = lock(&self.state);
        let mut results = Vec::new();
        if let Some(queue) = state.outcomes.get_mut(id) {
            queue.retain(|(owner, result)| {
                if owner == origin && results.len() < maximum {
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
