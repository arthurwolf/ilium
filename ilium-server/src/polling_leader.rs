//! Elected-leader runtime of the advisory polling detector.
//!
//! Composition only: discovered session endpoints are authenticated and
//! subscribed (`polling_feed`/`polling_auth` own the proofs), their batches
//! feed `PollingWindows`, candidate windows enter the fair `PollingScheduler`,
//! and each admitted window is sent through the durable ledger to the
//! Decisions provider. Findings are only published to the projection; nothing
//! is ever sent to an agent.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ilium_agent_session::polling_activity::{candidate_signal, normalized_call_shape};
use ilium_agent_session::polling_windows::{BatchApplication, PollingWindows, WindowCheckMark};
use ilium_core::polling_detection::{
    ActivityEvent, ActivityKind, Admission, AgentBinding, DetectorStatus, Finding, FindingKind,
    PollingScheduler, PollingSettings,
};
use ilium_execution::{JobCost, Lane, Retained};
use ilium_inference::decisions::{DecisionsRequestError, PollingChoice, PollingDecision};
use ilium_ipc::polling::{PollingFeedEvent, SubscriptionObservation};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::polling_checkpoint::Checkpoints;
use crate::polling_detection::{LeaderUpdate, PollingPaths};
use crate::polling_discovery::SessionEndpointHint;
use crate::polling_dispatch::DispatchError;
use crate::polling_ledger::{Completion, Ledger, LedgerError, NewAttempt};
use crate::state::ServerState;

const MODEL: &str = "gpt-6-luna";
const PRICING_VERSION: &str = "gpt-6-luna-2026-11-15";
// $0.10 / $0.01 per million tokens, expressed in nanodollars per million.
const UNCACHED_RATE: i64 = 100_000_000;
const CACHED_RATE: i64 = 10_000_000;
const CACHE_WRITE_RATE: i64 = 0;
const FEED_QUEUE: usize = 64;
const MAXIMUM_ATTEMPTS_PER_WINDOW: u8 = 3;
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Provider seam injected at composition; the default is OpenAI's fixed
/// Decisions endpoint. Tests and isolated runs inject a loopback fake, so a
/// synthetic key can never reach a real provider.
pub(crate) trait DecisionsCall: Send + Sync {
    fn decide(
        &self,
        api_key: &str,
        evidence: &str,
        threshold: f64,
    ) -> Result<PollingDecision, DecisionsRequestError>;
}

struct FnCall<F>(F);

impl<F> DecisionsCall for FnCall<F>
where
    F: Fn(&str, &str, f64) -> Result<PollingDecision, DecisionsRequestError> + Send + Sync,
{
    fn decide(
        &self,
        api_key: &str,
        evidence: &str,
        threshold: f64,
    ) -> Result<PollingDecision, DecisionsRequestError> {
        (self.0)(api_key, evidence, threshold)
    }
}

pub(crate) type Provider = Arc<dyn DecisionsCall>;

pub(crate) fn provider_from<F>(call: F) -> Provider
where
    F: Fn(&str, &str, f64) -> Result<PollingDecision, DecisionsRequestError>
        + Send
        + Sync
        + 'static,
{
    Arc::new(FnCall(call))
}

pub(crate) fn default_provider() -> Provider {
    match std::env::var("ILIUM_POLLING_DECISIONS_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => provider_from(move |key, evidence, threshold| {
            ilium_inference::decisions::request_polling_decision_loopback(
                &endpoint, key, evidence, threshold,
            )
        }),
        _ => provider_from(ilium_inference::decisions::request_polling_decision),
    }
}

enum FeedMessage {
    Observation(Retained<SubscriptionObservation>),
}

struct FeedHandle {
    task: JoinHandle<()>,
    retry_after: Instant,
}

enum Outcome {
    Decision(Box<PollingDecision>),
    Provider(DecisionsRequestError),
    Admission(DispatchError),
    Failed(String),
}

struct Dispatched {
    key: String,
    outcome: Outcome,
}

fn agent_key(binding: &AgentBinding) -> String {
    format!(
        "{}|{}|{}|{}",
        binding.session, binding.incarnation, binding.pane.0, binding.generation
    )
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub(crate) fn spawn(state: Arc<ServerState>, paths: PollingPaths) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut leader = Leader::new(state, paths);
        leader.run().await;
    })
}

struct Leader {
    state: Arc<ServerState>,
    paths: PollingPaths,
    provider: Provider,
    started: Instant,
    feeds: HashMap<uuid::Uuid, FeedHandle>,
    windows: PollingWindows,
    scheduler: PollingScheduler,
    settings: Option<PollingSettings>,
    pending: HashMap<String, (AgentBinding, WindowCheckMark)>,
    in_flight: HashMap<String, (AgentBinding, WindowCheckMark)>,
    jobs: JoinSet<Option<Dispatched>>,
    failures: HashMap<String, u8>,
    checkpoints: Checkpoints,
    checkpoints_loaded: bool,
    backoff_until: Option<Instant>,
    backoff_steps: u32,
    paused_for_configuration: Option<usize>,
    configuration_identity: usize,
    budget_blocked_month: Option<String>,
    skipped_by_filter: u64,
}

impl Leader {
    fn new(state: Arc<ServerState>, paths: PollingPaths) -> Self {
        let provider = state.polling.provider().unwrap_or_else(default_provider);
        Self {
            state,
            paths,
            provider,
            started: Instant::now(),
            feeds: HashMap::new(),
            windows: PollingWindows::default(),
            scheduler: PollingScheduler::default(),
            settings: None,
            pending: HashMap::new(),
            in_flight: HashMap::new(),
            jobs: JoinSet::new(),
            failures: HashMap::new(),
            checkpoints: Checkpoints::default(),
            checkpoints_loaded: false,
            backoff_until: None,
            backoff_steps: 0,
            paused_for_configuration: None,
            configuration_identity: 0,
            budget_blocked_month: None,
            skipped_by_filter: 0,
        }
    }

    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    async fn run(&mut self) {
        let (sender, mut receiver) = mpsc::channel::<(uuid::Uuid, FeedMessage)>(FEED_QUEUE);
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => self.maintain(&sender).await,
                Some((_, FeedMessage::Observation(observation))) = receiver.recv() => {
                    self.observe(observation).await;
                }
                Some(joined) = self.jobs.join_next(), if !self.jobs.is_empty() => {
                    if let Ok(Some(finished)) = joined {
                        self.finish(finished).await;
                    }
                }
            }
            self.pump().await;
        }
    }

    /// Reconcile grant, configuration and live feeds once per tick.
    async fn maintain(&mut self, sender: &mpsc::Sender<(uuid::Uuid, FeedMessage)>) {
        let Some(grant) = self.state.polling.leader_grant().await else {
            self.stand_down().await;
            return;
        };
        let configuration_identity = Arc::as_ptr(&grant.configuration) as usize;
        self.configuration_identity = configuration_identity;
        if self
            .paused_for_configuration
            .is_some_and(|paused| paused != configuration_identity)
        {
            self.paused_for_configuration = None;
        }
        let settings = grant.configuration.settings.clone();
        if self.settings.as_ref() != Some(&settings) {
            // A different configuration applies prospectively: drop windows
            // and queued work instead of classifying evidence gathered under
            // the previous interval, window or consent.
            self.windows.configure(&settings);
            self.scheduler.clear_pending();
            self.pending.clear();
            self.settings = Some(settings.clone());
        }
        if !self.checkpoints_loaded {
            self.checkpoints_loaded = true;
            self.checkpoints = self.load_checkpoints().await;
        }
        let Some(execution) = self.state.execution.get() else {
            return;
        };
        let registry = self.paths.registry_directory.clone();
        let discovered = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 1024 * 1024,
                },
                move |_| crate::polling_discovery::discover(&registry),
            )
            .await;
        let Ok(discovered) = discovered else { return };
        let (discovered, _retention) = discovered.into_parts();
        let live: std::collections::HashSet<uuid::Uuid> = discovered
            .endpoints
            .iter()
            .map(|hint| hint.incarnation)
            .collect();
        if !discovered.incomplete {
            self.feeds.retain(|incarnation, handle| {
                let keep = live.contains(incarnation);
                if !keep {
                    handle.task.abort();
                }
                keep
            });
        }
        let now = Instant::now();
        for hint in discovered.endpoints {
            if let Some(handle) = self.feeds.get(&hint.incarnation) {
                if !handle.task.is_finished() || handle.retry_after > now {
                    continue;
                }
            }
            let epoch = grant.owner.leader_epoch().to_owned();
            let task = tokio::spawn(run_feed(
                Arc::clone(&self.state),
                self.paths.registry_directory.clone(),
                hint.clone(),
                epoch,
                sender.clone(),
            ));
            self.feeds.insert(
                hint.incarnation,
                FeedHandle {
                    task,
                    retry_after: now + RECONNECT_DELAY,
                },
            );
        }
        let status = if self.budget_blocked_month.is_some() {
            DetectorStatus::BudgetReached
        } else if self.backoff_until.is_some_and(|until| until > now) {
            DetectorStatus::Offline
        } else if self.paused_for_configuration.is_some() {
            DetectorStatus::NoApiKey
        } else if self.windows.dropped_capacity() > 0 || !self.feeds.is_empty() {
            DetectorStatus::Watching
        } else {
            DetectorStatus::WaitingForActivity
        };
        self.state
            .polling
            .publish(LeaderUpdate {
                status: Some(Some(status)),
                counters: Some((
                    self.scheduler.coalesced(),
                    self.scheduler
                        .dropped_capacity()
                        .saturating_add(self.windows.dropped_capacity()),
                )),
                ..LeaderUpdate::default()
            })
            .await;
    }

    async fn stand_down(&mut self) {
        for (_, handle) in self.feeds.drain() {
            handle.task.abort();
        }
        if self.settings.take().is_some() {
            self.windows.configure(&PollingSettings::default());
            self.scheduler.clear_pending();
            self.pending.clear();
        }
        self.state
            .polling
            .publish(LeaderUpdate {
                status: Some(None),
                ..LeaderUpdate::default()
            })
            .await;
    }

    async fn load_checkpoints(&self) -> Checkpoints {
        let Some(execution) = self.state.execution.get() else {
            return Checkpoints::default();
        };
        let directory = self.paths.data_directory.clone();
        match execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 1024 * 1024,
                },
                move |_| Ok::<_, std::io::Error>(Checkpoints::load(&directory, unix_seconds())),
            )
            .await
        {
            Ok(loaded) => loaded.into_parts().0,
            Err(_) => Checkpoints::default(),
        }
    }

    async fn save_checkpoints(&self) {
        let Some(execution) = self.state.execution.get() else {
            return;
        };
        let directory = self.paths.data_directory.clone();
        let snapshot = self.checkpoints.clone();
        let _ = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 4096,
                },
                move |_| snapshot.save(&directory),
            )
            .await;
    }

    async fn observe(&mut self, observation: Retained<SubscriptionObservation>) {
        let now = self.now();
        let mut ready = None;
        let mut closed = None;
        if let SubscriptionObservation::Activity(event) = observation.view() {
            match event {
                PollingFeedEvent::Batch(batch) => {
                    let binding = batch.binding.clone();
                    if let BatchApplication::Applied { ready: true } =
                        self.windows.apply_batch(batch.clone(), now)
                    {
                        ready = Some(binding);
                    }
                }
                PollingFeedEvent::Closed(binding) => {
                    self.windows.unbind(binding);
                    closed = Some(binding.clone());
                }
                PollingFeedEvent::Stopped(_) => {}
            }
        }
        // The original storage charge stays held until the window consumed it.
        drop(observation);
        if let Some(binding) = closed {
            let key = agent_key(&binding);
            self.pending.remove(&key);
            self.checkpoints.forget(&key);
            self.state
                .polling
                .publish(LeaderUpdate {
                    closed: Some(binding),
                    ..LeaderUpdate::default()
                })
                .await;
        }
        if let Some(binding) = ready {
            self.consider(binding);
        }
    }

    fn consider(&mut self, binding: AgentBinding) {
        let Some(settings) = self.settings.clone() else {
            return;
        };
        let key = agent_key(&binding);
        // A recovered checkpoint keeps spacing across a detector restart.
        if self.checkpoints.last_check(&key).is_some_and(|last| {
            unix_seconds().saturating_sub(last) < u64::from(settings.minimum_spacing_seconds)
        }) {
            return;
        }
        let Some(mark) = self.windows.capture_check_mark(&binding, self.now()) else {
            return;
        };
        match self.scheduler.offer(&key) {
            Admission::Queued | Admission::Coalesced => {
                self.pending.insert(key, (binding, mark));
            }
            Admission::DroppedCapacity => {}
        }
    }

    fn blocked(&self) -> bool {
        self.paused_for_configuration.is_some()
            || self.budget_blocked_month.is_some()
            || self
                .backoff_until
                .is_some_and(|until| until > Instant::now())
    }

    async fn pump(&mut self) {
        let Some(settings) = self.settings.clone() else {
            return;
        };
        if self.blocked() {
            return;
        }
        let Some(grant) = self.state.polling.leader_grant().await else {
            return;
        };
        let key_text = grant.configuration.openai_key().to_owned();
        while self.in_flight.len() < 2 {
            let Some(key) = self.scheduler.start_next() else {
                break;
            };
            let Some((binding, mark)) = self.pending.remove(&key) else {
                self.scheduler.finish(&key);
                continue;
            };
            let events: Vec<ActivityEvent> = self
                .windows
                .events(&binding)
                .map(|events| events.cloned().collect())
                .unwrap_or_default();
            let now = self.now();
            let candidate = !settings.candidate_filter || candidate_signal(&events).is_some();
            let max_bytes = (usize::from(settings.maximum_evidence_tokens) * 4).min(47_000);
            let evidence = candidate
                .then(|| crate::polling_prompt::compact_evidence(&events, &key_text, max_bytes))
                .flatten();
            let Some(evidence) = evidence else {
                // Skipped locally: no paid call; the interval still advances.
                if !candidate {
                    self.skipped_by_filter = self.skipped_by_filter.saturating_add(1);
                }
                self.windows.mark_checked_through(&mark, now);
                self.scheduler.finish(&key);
                continue;
            };
            self.in_flight.insert(key.clone(), (binding.clone(), mark));
            self.spawn_dispatch(
                grant.owner.clone(),
                settings.clone(),
                key_text.clone(),
                key,
                binding,
                evidence,
            );
        }
    }

    fn spawn_dispatch(
        &mut self,
        owner: Arc<crate::polling_ledger::LedgerOwner>,
        settings: PollingSettings,
        api_key: String,
        key: String,
        binding: AgentBinding,
        evidence: String,
    ) {
        if self.state.execution.get().is_none() {
            self.in_flight.remove(&key);
            self.scheduler.finish(&key);
            return;
        }
        let provider = Arc::clone(&self.provider);
        let state = Arc::clone(&self.state);
        self.jobs.spawn(dispatch_job(
            state,
            DispatchRequest {
                owner,
                settings,
                api_key,
                provider,
                evidence,
            },
            key,
            binding,
        ));
    }

    async fn finish(&mut self, finished: Dispatched) {
        self.scheduler.finish(&finished.key);
        let Some((binding, mark)) = self.in_flight.remove(&finished.key) else {
            return;
        };
        let now = self.now();
        match finished.outcome {
            Outcome::Decision(decision) => {
                self.backoff_until = None;
                self.backoff_steps = 0;
                self.failures.remove(&finished.key);
                // The window may have been replaced during the request; only
                // an unchanged continuous window accepts the acknowledgement.
                let still_current = self.windows.mark_checked_through(&mark, now);
                self.checkpoints.record(&finished.key, unix_seconds());
                self.save_checkpoints().await;
                if still_current {
                    let events: Vec<ActivityEvent> = self
                        .windows
                        .events(&binding)
                        .map(|events| events.cloned().collect())
                        .unwrap_or_default();
                    let finding = build_finding(&binding, &decision, &events);
                    self.state
                        .polling
                        .publish(LeaderUpdate {
                            finding: Some(finding),
                            ..LeaderUpdate::default()
                        })
                        .await;
                }
            }
            Outcome::Provider(error) => self.provider_failure(&finished.key, &mark, error, now),
            Outcome::Admission(DispatchError::Ledger(LedgerError::BudgetReached)) => {
                self.budget_blocked_month = Some(chrono::Local::now().format("%Y-%m").to_string());
                self.windows.mark_checked_through(&mark, now);
            }
            Outcome::Admission(DispatchError::Disabled | DispatchError::InvalidSettings) => {
                self.windows.mark_checked_through(&mark, now);
            }
            Outcome::Admission(DispatchError::MissingKey) => {
                self.paused_for_configuration = Some(self.configuration_identity);
            }
            Outcome::Failed(reason) => {
                tracing::warn!(%reason, "polling dispatch failed before the provider answered");
                self.register_failure(&finished.key, &mark, now);
            }
            Outcome::Admission(_) => {
                self.register_failure(&finished.key, &mark, now);
            }
        }
    }

    fn provider_failure(
        &mut self,
        key: &str,
        mark: &WindowCheckMark,
        error: DecisionsRequestError,
        now: Duration,
    ) {
        match error {
            DecisionsRequestError::Http {
                status,
                retry_after,
            } if status == 401 || status == 403 || status == 400 || status == 404 => {
                // Authentication and invalid-request errors pause new calls
                // until the configuration changes.
                let _ = retry_after;
                self.paused_for_configuration = Some(self.configuration_identity);
                self.windows.mark_checked_through(mark, now);
            }
            DecisionsRequestError::Http {
                status,
                retry_after,
            } if status == 429 || status >= 500 => {
                let hinted = retry_after
                    .as_deref()
                    .and_then(|text| text.trim().parse::<u64>().ok())
                    .map(|seconds| Duration::from_secs(seconds.min(3600)));
                self.back_off(hinted);
                self.register_failure(key, mark, now);
            }
            DecisionsRequestError::MissingKey | DecisionsRequestError::EndpointRejected => {
                self.paused_for_configuration = Some(self.configuration_identity);
                self.windows.mark_checked_through(mark, now);
            }
            DecisionsRequestError::Transport
            | DecisionsRequestError::InvalidResponse
            | DecisionsRequestError::Http { .. } => {
                self.back_off(None);
                self.register_failure(key, mark, now);
            }
        }
    }

    fn back_off(&mut self, hinted: Option<Duration>) {
        self.backoff_steps = self.backoff_steps.saturating_add(1).min(8);
        let exponential = Duration::from_secs(5u64 << self.backoff_steps.min(6));
        let jitter = Duration::from_millis(u64::from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.subsec_millis()),
        ));
        let delay = hinted
            .unwrap_or(exponential)
            .max(Duration::from_secs(1))
            .min(Duration::from_secs(900))
            + jitter;
        self.backoff_until = Some(Instant::now() + delay);
    }

    fn register_failure(&mut self, key: &str, mark: &WindowCheckMark, now: Duration) {
        let failures = self.failures.entry(key.to_owned()).or_default();
        *failures = failures.saturating_add(1);
        if *failures >= MAXIMUM_ATTEMPTS_PER_WINDOW {
            // Give up on this window; a later interval starts fresh.
            self.failures.remove(key);
            self.windows.mark_checked_through(mark, now);
        }
    }
}

/// The admitted I/O job. A named type, not a closure, keeps the spawned
/// future's `Send` proof independent of closure lifetime inference.
use crate::polling_detection::AccountingProjection;

/// Everything one provider review needs, moved into the spawned dispatch.
struct DispatchRequest {
    owner: Arc<crate::polling_ledger::LedgerOwner>,
    settings: PollingSettings,
    api_key: String,
    provider: Provider,
    evidence: String,
}

struct DispatchJob {
    owner: Arc<crate::polling_ledger::LedgerOwner>,
    settings: PollingSettings,
    api_key: String,
    provider: Provider,
    evidence: String,
    threshold: f64,
    project: String,
}

impl ilium_execution::Job for DispatchJob {
    type Output = (Outcome, AccountingProjection);
    type Error = std::io::Error;

    fn run(self, _context: ilium_execution::JobContext) -> Result<Self::Output, Self::Error> {
        Ok(dispatch_blocking(
            &self.owner,
            &self.settings,
            &self.api_key,
            &self.provider,
            &self.evidence,
            self.threshold,
            &self.project,
        ))
    }
}

/// Spawned dispatch: the execution client is cloned inside the future, exactly
/// as the evidence reader does, so no borrowed client crosses a spawn boundary.
async fn dispatch_job(
    state: Arc<ServerState>,
    request: DispatchRequest,
    key: String,
    binding: AgentBinding,
) -> Option<Dispatched> {
    let DispatchRequest {
        owner,
        settings,
        api_key,
        provider,
        evidence,
    } = request;
    let threshold = f64::from(settings.confidence_percent) / 100.0;
    let project = binding.session.clone();
    let outcome = match state.execution.get().map(|runtime| runtime.client.clone()) {
        None => Outcome::Failed("the execution runtime is unavailable".to_owned()),
        Some(execution) => {
            let result = execution
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 32 * 1024 * 1024,
                        result_bytes: 64 * 1024,
                    },
                    DispatchJob {
                        owner,
                        settings,
                        api_key,
                        provider,
                        evidence,
                        threshold,
                        project,
                    },
                )
                .await;
            match result {
                Ok(retained) => {
                    let (outcome, accounting) = retained.into_parts().0;
                    state
                        .polling
                        .publish(LeaderUpdate {
                            accounting: accounting.map(Some),
                            ..LeaderUpdate::default()
                        })
                        .await;
                    outcome
                }
                Err(_) => {
                    Outcome::Failed("the execution runtime refused the provider job".to_owned())
                }
            }
        }
    };
    Some(Dispatched { key, outcome })
}

/// Runs on an admitted I/O worker: durable admission, provider call, durable
/// completion. A send whose outcome is unknown keeps its reservation.
fn dispatch_blocking(
    owner: &Arc<crate::polling_ledger::LedgerOwner>,
    settings: &PollingSettings,
    api_key: &str,
    provider: &Provider,
    evidence: &str,
    threshold: f64,
    project: &str,
) -> (Outcome, AccountingProjection) {
    let mut ledger = match Ledger::connect(Arc::clone(owner)) {
        Ok(ledger) => ledger,
        Err(error) => return (Outcome::Admission(DispatchError::Ledger(error)), None),
    };
    let maximum_tokens =
        match ilium_inference::decisions::maximum_polling_input_tokens(api_key, evidence) {
            Ok(tokens) => tokens,
            Err(error) => return (Outcome::Provider(error), None),
        };
    let local = chrono::Local::now();
    let reserved = i64::try_from(maximum_tokens.saturating_mul(100))
        .unwrap_or(i64::MAX)
        .max(1);
    let attempt = NewAttempt {
        request_id: uuid::Uuid::new_v4().to_string(),
        window_id: uuid::Uuid::new_v4().to_string(),
        month: local.format("%Y-%m").to_string(),
        day: local.format("%Y-%m-%d").to_string(),
        created_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        project_id: project.chars().take(256).collect(),
        model: MODEL.to_owned(),
        reserved_nanodollars: reserved,
    };
    let mut provider_error = None;
    let result =
        crate::polling_dispatch::dispatch(settings, api_key, Some(&mut ledger), &attempt, || {
            match provider.decide(api_key, evidence, threshold) {
                Ok(decision) => {
                    let Some(usage) = decision.usage else {
                        // Missing usage is unknown billing, never zero.
                        return Err(());
                    };
                    let completion = Completion {
                        input_tokens: usage.input_tokens as i64,
                        cached_tokens: usage.cached_tokens as i64,
                        cache_write_tokens: usage.cache_write_tokens as i64,
                        pricing_version: PRICING_VERSION.to_owned(),
                        uncached_rate: UNCACHED_RATE,
                        cached_rate: CACHED_RATE,
                        cache_write_rate: CACHE_WRITE_RATE,
                    };
                    Ok((decision, completion))
                }
                Err(error) => {
                    provider_error = Some(error);
                    Err(())
                }
            }
        });
    let accounting = accounting_projection(&ledger);
    let outcome = match result {
        Ok(decision) => Outcome::Decision(Box::new(decision)),
        Err(DispatchError::OutcomeUnknown) => match provider_error {
            Some(error) => Outcome::Provider(error),
            None => Outcome::Admission(DispatchError::OutcomeUnknown),
        },
        Err(error) => Outcome::Admission(error),
    };
    (outcome, accounting)
}

fn accounting_projection(
    ledger: &Ledger,
) -> Option<(
    Option<ilium_ipc::polling::PollingAccounting>,
    Option<ilium_ipc::polling::PollingAccounting>,
    Option<ilium_ipc::polling::PollingAccounting>,
)> {
    let now = chrono::Local::now();
    let day = ledger
        .summary_day(&now.format("%Y-%m-%d").to_string())
        .ok()?
        .map(crate::polling_detection::accounting)
        .transpose()
        .ok()?;
    let month = crate::polling_detection::accounting(
        ledger.summary(&now.format("%Y-%m").to_string()).ok()?,
    )
    .ok()?;
    let all_time = crate::polling_detection::accounting(ledger.summary_all_time().ok()?).ok()?;
    Some((day, Some(month), Some(all_time)))
}

/// Facts are derived locally; the provider supplies no rationale.
fn build_finding(
    binding: &AgentBinding,
    decision: &PollingDecision,
    events: &[ActivityEvent],
) -> Finding {
    let mut calls: HashMap<String, u32> = HashMap::new();
    let mut updates: HashMap<&str, u32> = HashMap::new();
    for event in events {
        match event.kind {
            ActivityKind::ToolCall if !event.text.is_empty() => {
                *calls.entry(normalized_call_shape(event)).or_default() += 1;
            }
            ActivityKind::Message if !event.text.is_empty() => {
                *updates.entry(event.text.as_str()).or_default() += 1;
            }
            _ => {}
        }
    }
    let kind = match decision.choice {
        PollingChoice::Polling => FindingKind::PossiblePolling,
        PollingChoice::RepeatedUpdates => FindingKind::RepeatedUpdates,
        PollingChoice::WaitChurn => FindingKind::WaitChurn,
        PollingChoice::NoFinding => FindingKind::NoFinding,
        PollingChoice::Insufficient => FindingKind::InsufficientEvidence,
    };
    Finding {
        finding_id: uuid::Uuid::new_v4().to_string(),
        session_id: binding.session.clone(),
        session_incarnation: binding.incarnation.clone(),
        pane_id: binding.pane,
        generation: binding.generation,
        kind,
        confidence_basis_points: decision
            .confidence
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .map(|value| (value * 10_000.0).round() as u16),
        observed_utc: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        repeated_calls: calls.values().copied().max().unwrap_or(0),
        repeated_updates: updates.values().copied().max().unwrap_or(0),
        ownership_known: false,
        evidence_incomplete: events.len() < 3 || events.iter().any(|event| event.truncated),
    }
}

/// One authenticated private subscription. Any error simply ends the task; the
/// leader reconnects after a delay and a fresh baseline records a gap.
async fn run_feed(
    state: Arc<ServerState>,
    registry_directory: std::path::PathBuf,
    hint: SessionEndpointHint,
    leader_epoch: String,
    sender: mpsc::Sender<(uuid::Uuid, FeedMessage)>,
) {
    let Some(execution) = state.execution.get() else {
        return;
    };
    let client = execution.client.clone();
    let incarnation = hint.incarnation;
    let Ok(mut connection) = crate::polling_private_connection::PrivateConnection::connect(
        &registry_directory,
        hint,
        leader_epoch,
        &client,
    )
    .await
    else {
        return;
    };
    loop {
        let Ok(observation) = connection.next(&client).await else {
            return;
        };
        if matches!(
            observation.view(),
            SubscriptionObservation::Pending | SubscriptionObservation::Subscribed
        ) {
            continue;
        }
        if matches!(
            observation.view(),
            SubscriptionObservation::Rejected
                | SubscriptionObservation::Activity(PollingFeedEvent::Stopped(_))
        ) {
            return;
        }
        // Slow consumer: never block the reader or grow a queue. Drop the
        // subscription; the reconnect baselines and records an explicit gap.
        if sender
            .try_send((incarnation, FeedMessage::Observation(observation)))
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_inference::decisions::DecisionsUsage;

    fn enabled() -> PollingSettings {
        PollingSettings {
            enabled: true,
            ..PollingSettings::default()
        }
    }

    fn decision(choice: PollingChoice) -> PollingDecision {
        PollingDecision {
            choice,
            reported_choice: Some(choice),
            probabilities: None,
            confidence: Some(0.9),
            usage: Some(DecisionsUsage {
                input_tokens: 1000,
                uncached_tokens: 900,
                cached_tokens: 100,
                cache_write_tokens: 0,
            }),
        }
    }

    fn owner(directory: &std::path::Path) -> Arc<crate::polling_ledger::LedgerOwner> {
        Ledger::open(directory).unwrap().into_owner()
    }

    #[test]
    fn a_completed_decision_is_priced_from_returned_usage_and_projected() {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path());
        let provider: Provider = provider_from(|_, _, _| Ok(decision(PollingChoice::Polling)));
        let (outcome, accounting) = dispatch_blocking(
            &owner,
            &enabled(),
            "synthetic-key",
            &provider,
            "{}",
            0.8,
            "synthetic-session",
        );
        assert!(
            matches!(outcome, Outcome::Decision(value) if value.choice == PollingChoice::Polling)
        );
        let (_, month, all_time) = accounting.unwrap();
        let month = month.unwrap();
        assert_eq!(month.attempts, 1);
        // 900 uncached at $0.10/M plus 100 cached at $0.01/M, in nanodollars.
        assert_eq!(month.known_cost_nanodollars, 91_000);
        assert_eq!(all_time.unwrap().input_tokens, 1000);
    }

    #[test]
    fn a_failed_send_keeps_a_visible_unknown_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path());
        let provider: Provider = provider_from(|_, _, _| {
            Err(DecisionsRequestError::Http {
                status: 429,
                retry_after: Some("7".into()),
            })
        });
        let (outcome, accounting) = dispatch_blocking(
            &owner,
            &enabled(),
            "synthetic-key",
            &provider,
            "{}",
            0.8,
            "synthetic-session",
        );
        assert!(matches!(
            outcome,
            Outcome::Provider(DecisionsRequestError::Http { status: 429, .. })
        ));
        let month = accounting.unwrap().1.unwrap();
        assert_eq!(
            month.unknown_attempts, 1,
            "a failed send is never assumed free"
        );
    }

    #[test]
    fn missing_usage_is_unknown_billing_not_zero() {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path());
        let provider: Provider = provider_from(|_, _, _| {
            let mut value = decision(PollingChoice::NoFinding);
            value.usage = None;
            Ok(value)
        });
        let (outcome, accounting) = dispatch_blocking(
            &owner,
            &enabled(),
            "synthetic-key",
            &provider,
            "{}",
            0.8,
            "synthetic-session",
        );
        assert!(matches!(
            outcome,
            Outcome::Admission(DispatchError::OutcomeUnknown)
        ));
        assert_eq!(accounting.unwrap().1.unwrap().unknown_attempts, 1);
    }

    #[test]
    fn a_zero_budget_never_reaches_the_provider() {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path());
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = Arc::clone(&called);
        let provider: Provider = provider_from(move |_, _, _| {
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(decision(PollingChoice::Polling))
        });
        let settings = PollingSettings {
            monthly_limit_microdollars: 0,
            ..enabled()
        };
        let (outcome, _) = dispatch_blocking(
            &owner,
            &settings,
            "synthetic-key",
            &provider,
            "{}",
            0.8,
            "synthetic-session",
        );
        assert!(matches!(
            outcome,
            Outcome::Admission(DispatchError::Ledger(LedgerError::BudgetReached))
        ));
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn findings_carry_local_counts_and_never_a_rationale() {
        let binding = AgentBinding {
            session: "synthetic-session".into(),
            incarnation: "synthetic-server".into(),
            pane: ilium_core::NodeId(4),
            generation: 2,
        };
        let call = |id: &str| ActivityEvent {
            kind: ActivityKind::ToolCall,
            timestamp: None,
            name: Some("shell".into()),
            call_id: Some(id.into()),
            text: "check-status job-17".into(),
            truncated: false,
            is_error: None,
        };
        let events = vec![call("a"), call("b"), call("c")];
        let finding = build_finding(&binding, &decision(PollingChoice::Polling), &events);
        assert_eq!(finding.kind, FindingKind::PossiblePolling);
        assert_eq!(finding.repeated_calls, 3);
        assert_eq!(finding.confidence_basis_points, Some(9000));
        assert!(!finding.ownership_known);
        assert_eq!(finding.generation, 2);
    }

    fn binding(generation: u64) -> AgentBinding {
        AgentBinding {
            session: "synthetic-session".into(),
            incarnation: "synthetic-server".into(),
            pane: ilium_core::NodeId(4),
            generation,
        }
    }

    fn status_call() -> ActivityEvent {
        ActivityEvent {
            kind: ActivityKind::ToolCall,
            timestamp: None,
            name: Some("exec_command".into()),
            call_id: None,
            text: "job status 1".into(),
            truncated: false,
            is_error: None,
        }
    }

    /// Composition of the coordinator's decisions without a live pane: window
    /// readiness, candidate filter, evidence compaction, ledger-backed dispatch
    /// through a fake provider, finding, and spacing after the check. Whole-server
    /// behaviour with real panes is covered separately.
    #[test]
    fn a_repeating_agent_flows_from_window_to_finding_and_waits_out_the_spacing() {
        let directory = tempfile::tempdir().unwrap();
        let owner = owner(directory.path());
        let settings = PollingSettings {
            enabled: true,
            event_interval: 3,
            minimum_spacing_seconds: 60,
            ..PollingSettings::default()
        };
        let mut windows = PollingWindows::default();
        windows.configure(&settings);
        let agent = binding(1);
        windows.bind(agent.clone());
        let mut now = Duration::ZERO;
        assert!(!windows.observe(&agent, status_call(), now));
        assert!(!windows.observe(&agent, status_call(), now));
        assert!(
            windows.observe(&agent, status_call(), now),
            "the interval is reached"
        );

        let events: Vec<ActivityEvent> = windows.events(&agent).unwrap().cloned().collect();
        assert!(
            candidate_signal(&events).is_some(),
            "three identical checks are a candidate"
        );
        let evidence = crate::polling_prompt::compact_evidence(&events, "synthetic-key", 12_000)
            .expect("bounded evidence");
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let provider: Provider = provider_from(move |_, _, _| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(decision(PollingChoice::Polling))
        });
        let mark = windows
            .capture_check_mark(&agent, now)
            .expect("a mark for the window");
        let (outcome, accounting) = dispatch_blocking(
            &owner,
            &settings,
            "synthetic-key",
            &provider,
            &evidence,
            0.8,
            "synthetic-session",
        );
        let Outcome::Decision(decision) = outcome else {
            panic!("the fake provider answers");
        };
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let finding = build_finding(&agent, &decision, &events);
        assert_eq!(finding.kind, FindingKind::PossiblePolling);
        assert_eq!(finding.repeated_calls, 3);
        assert_eq!(accounting.unwrap().1.unwrap().attempts, 1);

        windows.mark_checked_through(&mark, now);
        for _ in 0..3 {
            now += Duration::from_secs(1);
            assert!(
                !windows.observe(&agent, status_call(), now),
                "no catch-up burst inside the minimum spacing"
            );
        }
        now += Duration::from_secs(61);
        assert!(
            windows.observe(&agent, status_call(), now),
            "ready again after the spacing"
        );
    }

    #[test]
    fn independent_work_never_reaches_the_provider() {
        let mut windows = PollingWindows::default();
        windows.configure(&PollingSettings {
            enabled: true,
            event_interval: 3,
            minimum_spacing_seconds: 0,
            ..PollingSettings::default()
        });
        let agent = binding(1);
        windows.bind(agent.clone());
        let mut ready = false;
        for index in 0..3 {
            let mut event = status_call();
            event.text = format!("edit module {index}");
            ready = windows.observe(&agent, event, Duration::ZERO);
        }
        assert!(ready);
        let events: Vec<ActivityEvent> = windows.events(&agent).unwrap().cloned().collect();
        assert!(
            candidate_signal(&events).is_none(),
            "distinct work is skipped locally"
        );
    }
}
