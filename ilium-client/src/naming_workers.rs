//! Background provider-neutral title-inference workers: project-name
//! bootstrap and per-pane title inference share the client's bounded I/O
//! bank. Typed original inputs survive admission rejection; completed results
//! carry independent storage admission through their actual consumers.
//! Exact prompt recovery keeps bounded per-pane cursors and uses finite I/O probes.
//!
//! Session-title inference (`spawn_session_title_worker`) receives one
//! immutable `SessionTitleInput` captured by the automatic trigger router or
//! explicit row action before this background boundary.

mod exact;
pub(crate) use exact::ExactSource;
mod finite;
pub(crate) use finite::{Kind as NamingKind, Request as NamingRequest};

use std::collections::{HashMap, HashSet};
use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, NodeId};
use ilium_inference::InferenceSettings;
use tokio::sync::mpsc::Sender;

use crate::naming::DualTitle;
use crate::project_naming::ProjectNameBootstrap;

/// Whether a finished naming worker's result came from a passive trigger
/// (a session ID just resolving, a turn finishing, every second Enter
/// press) or from the user explicitly clicking the tree row's "retitle"
/// icon (`App::action_request_retitle`). `crate::tick::apply_naming_worker_event`
/// distinguishes passive and explicit triggers, but both produce automatic
/// titles. The server normalizes an explicit AI title's legacy UserSpecified
/// wire source; only a literal user rename fixes the presentation. Both use
/// the server's session, invocation and presentation compare-and-set checks
/// and cannot replace a newer accepted title or manual name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleTrigger {
    Automatic,
    Manual,
}

/// Revision and allow bit captured before an automatic inference worker starts.
/// A single atomic word lets queued workers reject a revoked decision before
/// their provider call; the event loop checks it again before side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomaticAiDecision(u64);

impl AutomaticAiDecision {
    pub fn new(revision: u64, allowed: bool) -> Self {
        Self((revision << 1) | u64::from(allowed))
    }

    fn is_current(self, decision_word: &AtomicU64) -> bool {
        self.0 & 1 != 0 && decision_word.load(Ordering::SeqCst) == self.0
    }
}

/// Keeps an asynchronous side effect tied to the automatic-AI decision that
/// admitted it. The shared word is updated by the event loop when policy or
/// onboarding changes.
#[derive(Clone)]
pub(crate) struct AutomaticAiDecisionFence {
    expected: u64,
    current: Arc<AtomicU64>,
}

impl AutomaticAiDecisionFence {
    pub(crate) fn is_current(&self) -> bool {
        self.expected & 1 != 0 && self.current.load(Ordering::SeqCst) == self.expected
    }
}

/// A finished background naming result, forwarded into the main event loop.
pub enum NamingWorkerEvent {
    Prepared {
        event: Box<NamingWorkerEvent>,
        source_hold: Arc<ilium_execution::StorageAdmission>,
    },
    ProjectName {
        decision: AutomaticAiDecision,
        result: anyhow::Result<ProjectNameBootstrap>,
    },
    SessionTitle(SessionTitleWorkerResult),
    TerminalTitle(
        NodeId,
        anyhow::Result<DualTitle>,
        TitleTrigger,
        AutomaticAiDecision,
    ),
    InferenceTest {
        provider: ilium_inference::InferenceProviderKind,
        elapsed: Duration,
        result: anyhow::Result<crate::inference_test::InferenceTestResult>,
    },
    ProviderModels {
        provider: ilium_inference::InferenceProviderKind,
        endpoint: String,
        elapsed: Duration,
        result: anyhow::Result<Vec<String>>,
    },
    Restructure(RestructureWorkerResult),
    LastPromptTranscript(LastPromptTranscriptWorkerResult),
    ExactPrepared {
        result: ExactAgentPromptTranscriptResult,
        source_hold: Arc<ExactSource>,
    },
    ExactTranscriptFailed {
        pane_id: NodeId,
        session_id: String,
        prompt_epoch: String,
        error: String,
    },
}

/// All immutable inputs captured when a session-title worker starts. Keeping
/// the session ID and title generation together makes the stale-result
/// contract explicit at the thread boundary instead of relying on callers to
/// preserve their ordering across a long parameter list.
pub struct SessionTitleWorkerRequest {
    pub home: PathBuf,
    pub input: crate::session_naming::SessionTitleInput,
    pub title_generation: u64,
    pub trigger: TitleTrigger,
}

/// Complete provider-boundary outcome forwarded to the event loop. Named
/// fields keep session/generation fencing distinct from optional diagnostic
/// request/response payloads as this event evolves.
pub struct SessionTitleWorkerResult {
    pub pane_id: NodeId,
    pub presentation_revision: u64,
    pub process_id: Option<u32>,
    pub session_id: String,
    pub title_generation: u64,
    pub provider: ilium_inference::InferenceProviderKind,
    pub elapsed: Duration,
    pub rendered_prompt: Option<String>,
    pub raw_response: Option<String>,
    pub result: anyhow::Result<DualTitle>,
    pub trigger: TitleTrigger,
    pub automatic_ai_decision: AutomaticAiDecision,
}

/// One restructure result plus the activity checkpoint visible to inference.
/// The server applies valid plans even when newer activity arrived, then uses
/// this checkpoint to keep that newer activity eligible for the next pass.
pub struct RestructureWorkerResult {
    pub project_id: NodeId,
    pub title_observations: Vec<ilium_ipc::PaneTitleObservation>,
    pub inference_activity_revisions: Vec<ilium_core::NodeActivityRevision>,
    pub automatic_ai_decision: AutomaticAiDecision,
    pub result: anyhow::Result<ilium_core::animation_recommendation::RecommendedRestructurePlan>,
}

/// Immutable inputs for one last-prompt-from-transcript check -- see
/// `App::last_prompt_transcript_context` for how the caller resolves these.
pub struct LastPromptTranscriptWorkerRequest {
    pub home: PathBuf,
    pub pane_id: NodeId,
    /// The pane's launch cwd; the locator rejects transcripts bound elsewhere.
    pub project_path: PathBuf,
    pub agent_class: AgentClass,
    pub session_id: String,
    /// This pane's last-prompt value from before the submission being
    /// checked -- see `crate::app::PendingLastPromptTranscriptCheck`. The
    /// worker keeps retrying while the transcript's most recent user message
    /// still matches this (trimmed the same way transcript entries are),
    /// since that means the agent CLI hasn't flushed this turn's message yet
    /// and the read is just seeing the previous turn's already-applied text.
    pub baseline_last_prompt: Option<String>,
}

/// The most recent user message the worker found in the transcript, if any
/// -- `None` covers both "no transcript found yet" and "found one with no
/// user entries", neither worth distinguishing to the caller, which either
/// way just leaves the live-tracked value in place.
pub struct LastPromptTranscriptWorkerResult {
    pub pane_id: NodeId,
    pub session_id: String,
    pub last_prompt: Option<String>,
}

/// One recovery lookup fenced by the file position taken before Enter.
pub struct ExactAgentPromptTranscriptRequest {
    pub home: PathBuf,
    pub pane_id: NodeId,
    pub project_path: PathBuf,
    pub agent_class: AgentClass,
    pub session_id: String,
    pub verified_path: PathBuf,
    pub baseline_length: u64,
    pub submitted_after: chrono::DateTime<chrono::Utc>,
    pub prompt_epoch: String,
}

impl std::fmt::Debug for ExactAgentPromptTranscriptRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExactAgentPromptTranscriptRequest")
    }
}

pub struct ExactAgentPromptTranscriptResult {
    pub pane_id: NodeId,
    pub session_id: String,
    pub prompt_epoch: String,
    pub last_prompt: Option<String>,
}

/// Initial wait before the first transcript read: the agent CLI needs a
/// moment to flush this turn's submitted message to its own session log, and
/// reading too early would just see the previous turn's already-applied
/// value (or no file at all yet on a session's very first prompt). Scaled
/// down under `#[cfg(test)]` so the retry-until-fresh regression tests below
/// don't have to burn the full production window.
#[cfg(not(test))]
const LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY: Duration = Duration::from_millis(400);
#[cfg(test)]
const LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY: Duration = Duration::from_millis(20);
/// Polling interval between retries once the initial wait has elapsed.
#[cfg(not(test))]
const LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL: Duration = Duration::from_millis(400);
#[cfg(test)]
const LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL: Duration = Duration::from_millis(20);
/// Upper bound on retries -- worst case ~10s total. Every candidate read is
/// checked against the submission's baseline (see
/// `LastPromptTranscriptWorkerRequest::baseline_last_prompt`) before it's
/// accepted, so a longer window than the banner-feels-instant case actually
/// needs is safe rather than risky: it only ever extends how long an opaque
/// (history-recall/completion/unsupported-escape) submission -- for which the
/// transcript is the *only* source of the banner text -- keeps waiting for a
/// slow-to-flush agent CLI, never how long a stale read can masquerade as
/// fresh.
const LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS: u32 = 25;

/// Tracks which naming workers are currently in flight, so a caller never
/// accidentally spawns a second one for the same target while the first is
/// still running.
pub struct NamingWorkers {
    finite: Option<finite::FiniteWorkers>,
    events_tx: Sender<NamingWorkerEvent>,
    inference_settings: Arc<finite::SettingsSnapshot>,
    project_name_in_flight: bool,
    session_title_in_flight: HashMap<(NodeId, String), Arc<ilium_execution::StorageAdmission>>,
    terminal_title_in_flight: HashSet<NodeId>,
    inference_test_in_flight: bool,
    model_discovery_in_flight: bool,
    restructure_in_flight: HashSet<NodeId>,
    exact: Option<exact::ExactWorkers>,
    concurrency_limiter: Arc<InferenceConcurrencyLimiter>,
    automatic_ai_decision: Arc<AtomicU64>,
    original_retry_at: Option<Instant>,
    settings_retry: Option<(Instant, ilium_execution::RejectReason)>,
}

const MAX_CONCURRENT_INFERENCE_JOBS: usize = 2;

/// One process-wide client boundary prevents startup/completion triggers from
/// turning independent per-pane workers into an unbounded provider burst.
pub(crate) struct InferenceConcurrencyLimiter {
    active_jobs: Mutex<usize>,
    available: Condvar,
    release_wake: Arc<tokio::sync::Notify>,
    maximum: usize,
}

impl InferenceConcurrencyLimiter {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            active_jobs: Mutex::new(0),
            available: Condvar::new(),
            release_wake: crate::execution::admission_notification(),
            maximum: maximum.max(1),
        }
    }

    pub(crate) fn try_acquire(self: &Arc<Self>) -> Option<InferencePermit> {
        let mut active = self.active_jobs.try_lock().ok()?;
        if *active >= self.maximum {
            return None;
        }
        *active += 1;
        Some(InferencePermit {
            limiter: Arc::clone(self),
            wake_on_release: false,
        })
    }

    #[cfg(test)]
    fn acquire(self: &Arc<Self>) -> InferencePermit {
        let mut active_jobs = self
            .active_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *active_jobs >= self.maximum {
            active_jobs = self
                .available
                .wait(active_jobs)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        *active_jobs += 1;
        InferencePermit {
            limiter: Arc::clone(self),
            wake_on_release: true,
        }
    }
}

/// Count physical captured capacities without serializing or exposing secrets.
pub(crate) fn inference_settings_bytes(settings: &InferenceSettings) -> usize {
    let instructions = &settings.instructions;
    let database = &settings.kilo_gateway.proxy_database;
    let structure = &database.structure;
    let strings = [
        &instructions.entry_naming,
        &instructions.organization,
        &instructions.naming_and_organization,
        &instructions.project_naming,
        &instructions.smart_copy,
        &instructions.ask_for_update,
        &settings.kilo_gateway.model,
        &database.uri,
        &database.database,
        &database.collection,
        &structure.ip,
        &structure.port,
        &structure.protocol,
        &structure.username,
        &structure.password,
        &structure.enabled,
        &settings.ollama.base_url,
        &settings.ollama.model,
        &settings.openai.base_url,
        &settings.openai.api_key,
        &settings.openai.model,
        &settings.anthropic.base_url,
        &settings.anthropic.api_key,
        &settings.anthropic.model,
        &settings.openrouter.api_key,
        &settings.openrouter.model,
    ];
    strings
        .iter()
        .fold(std::mem::size_of::<InferenceSettings>(), |sum, value| {
            sum.saturating_add(value.capacity())
        })
        .saturating_add(
            settings
                .kilo_gateway
                .paid_proxies
                .capacity()
                .saturating_mul(std::mem::size_of::<ilium_inference::PaidProxy>()),
        )
        .saturating_add(
            settings
                .kilo_gateway
                .paid_proxies
                .iter()
                .fold(0usize, |sum, proxy| {
                    sum.saturating_add(proxy.ip.capacity())
                        .saturating_add(proxy.protocol.capacity())
                        .saturating_add(proxy.username.capacity())
                        .saturating_add(proxy.password.capacity())
                }),
        )
}

/// All naming and streaming adapters in this process share these two slots.
/// Cross-process/provider-host accounting remains a separate explicit boundary.
pub(crate) fn shared_provider_limiter() -> Arc<InferenceConcurrencyLimiter> {
    static LIMITER: OnceLock<Arc<InferenceConcurrencyLimiter>> = OnceLock::new();
    Arc::clone(LIMITER.get_or_init(|| {
        Arc::new(InferenceConcurrencyLimiter::new(
            MAX_CONCURRENT_INFERENCE_JOBS,
        ))
    }))
}

pub(crate) struct InferencePermit {
    limiter: Arc<InferenceConcurrencyLimiter>,
    wake_on_release: bool,
}
impl InferencePermit {
    pub(crate) fn mark_started(&mut self) {
        self.wake_on_release = true;
    }
}

impl Drop for InferencePermit {
    fn drop(&mut self) {
        let mut active_jobs = self
            .limiter
            .active_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *active_jobs = active_jobs.saturating_sub(1);
        self.limiter.available.notify_one();
        drop(active_jobs);
        // A pending finite provider request must wake even when the permit
        // was released by another naming/Smart Copy owner, with no UI tick.
        if self.wake_on_release {
            self.limiter.release_wake.notify_one();
        }
    }
}

/// Contains a worker body's potential panic so a single bad turn (for
/// example a byte-slicing bug hit while scanning arbitrary terminal or
/// transcript text) degrades to a failed result instead of silently
/// dropping the worker's completion event. A dropped event would leave the
/// matching `*_in_flight` guard set forever, permanently disabling that
/// naming feature for the rest of the client's run -- mirrors
/// `search_workers::start`'s containment of the same risk.
fn catch_worker_panic<T>(
    worker_name: &str,
    body: impl FnOnce() -> anyhow::Result<T> + UnwindSafe,
) -> anyhow::Result<T> {
    panic::catch_unwind(body).unwrap_or_else(|panic_payload| {
        Err(anyhow::anyhow!(
            "{worker_name} worker panicked: {}",
            panic_payload_message(&panic_payload)
        ))
    })
}

/// Extracts a human-readable message from a caught panic payload.
/// `std::panic!` payloads are almost always `&str` or `String`; anything
/// else still logs usefully rather than silently swallowing the panic's
/// presence.
fn panic_payload_message(panic_payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic_payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = panic_payload.downcast_ref::<String>() {
        return message.clone();
    }
    "non-string panic payload".to_string()
}

impl NamingWorkers {
    pub fn new(
        events_tx: Sender<NamingWorkerEvent>,
        inference_settings: &InferenceSettings,
    ) -> Result<Self, ilium_execution::RejectReason> {
        let inference_settings = finite::SettingsSnapshot::capture(inference_settings)?;
        Ok(Self {
            #[cfg(test)]
            finite: Some(finite::FiniteWorkers::new(
                crate::execution::test_client(),
                shared_provider_limiter(),
            )),
            #[cfg(not(test))]
            finite: None,
            events_tx,
            inference_settings,
            project_name_in_flight: false,
            session_title_in_flight: HashMap::new(),
            terminal_title_in_flight: HashSet::new(),
            inference_test_in_flight: false,
            model_discovery_in_flight: false,
            restructure_in_flight: HashSet::new(),
            #[cfg(test)]
            exact: Some(exact::ExactWorkers::new(crate::execution::test_client())),
            #[cfg(not(test))]
            exact: None,
            concurrency_limiter: shared_provider_limiter(),
            automatic_ai_decision: Arc::new(AtomicU64::new(AutomaticAiDecision::new(0, true).0)),
            original_retry_at: None,
            settings_retry: None,
        })
    }

    pub(crate) fn configure_execution(&mut self, client: ilium_execution::Client) {
        self.finite = Some(finite::FiniteWorkers::new(
            client.clone(),
            Arc::clone(&self.concurrency_limiter),
        ));
        self.exact = Some(exact::ExactWorkers::new(client));
    }
    fn enqueue_finite(
        &mut self,
        kind: NamingKind,
    ) -> Result<finite::Accepted, Box<ilium_execution::Rejected<NamingRequest>>> {
        let request = NamingRequest::new(
            kind,
            Arc::clone(&self.inference_settings),
            &self.automatic_ai_decision,
        );
        self.enqueue_original(request)
    }
    fn enqueue_original(
        &mut self,
        request: NamingRequest,
    ) -> Result<finite::Accepted, Box<ilium_execution::Rejected<NamingRequest>>> {
        let Some(finite) = &mut self.finite else {
            return Err(Box::new(ilium_execution::Rejected {
                reason: ilium_execution::RejectReason::Closed,
                value: request,
            }));
        };
        let result = finite.enqueue(request, &self.automatic_ai_decision);
        if let Err(rejected) = &result {
            use ilium_execution::RejectReason;
            if matches!(
                rejected.reason,
                RejectReason::Busy
                    | RejectReason::QueueFull
                    | RejectReason::JobLimit
                    | RejectReason::InputBytes
                    | RejectReason::ResultBytes
                    | RejectReason::WorkerBytes
            ) {
                self.original_retry_at
                    .get_or_insert_with(|| Instant::now() + Duration::from_millis(100));
            }
        }
        result
    }
    pub(crate) fn begin_retry_turn(&mut self, now: Instant) -> bool {
        if self
            .original_retry_at
            .is_some_and(|deadline| deadline > now)
        {
            return false;
        }
        self.original_retry_at = None;
        true
    }
    pub(crate) fn retry_delay(&self, now: Instant) -> Option<Duration> {
        self.finite
            .as_ref()
            .and_then(|finite| finite.retry_delay(now))
            .into_iter()
            .chain(
                self.original_retry_at
                    .map(|deadline| deadline.saturating_duration_since(now)),
            )
            .chain(
                self.settings_retry
                    .map(|(deadline, _)| deadline.saturating_duration_since(now)),
            )
            .min()
    }
    pub(crate) fn retry_original(
        &mut self,
        request: NamingRequest,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        let target = match &request.kind {
            NamingKind::ProjectName(_) => (0, None),
            NamingKind::SessionTitle(_) => (1, None),
            NamingKind::TerminalTitle(input, _) => (2, Some(input.pane_id)),
            NamingKind::InferenceTest => (3, None),
            NamingKind::Models(_) => (4, None),
            NamingKind::Restructure(input, _) => (5, Some(input.project_id)),
            #[cfg(test)]
            NamingKind::LastPrompt(_) => (6, None),
        };
        let accepted = self.enqueue_original(request)?;
        self.mark_accepted(accepted);
        match target {
            (0, _) => self.project_name_in_flight = true,
            (2, Some(pane)) => {
                self.terminal_title_in_flight.insert(pane);
            }
            (3, _) => self.inference_test_in_flight = true,
            (4, _) => self.model_discovery_in_flight = true,
            (5, Some(project)) => {
                self.restructure_in_flight.insert(project);
            }
            _ => {}
        }
        Ok(())
    }
    fn mark_accepted(&mut self, accepted: finite::Accepted) {
        if let Some(key) = accepted.session_key {
            self.session_title_in_flight
                .insert(key, accepted.source_hold);
        }
    }
    pub(crate) fn collect(&mut self) {
        if let Some(exact) = &mut self.exact {
            exact.collect();
            exact.publish(&self.events_tx);
        }
        let Some(finite) = &mut self.finite else {
            return;
        };
        finite.collect();
        while let Some(prepared) = finite.take_ready() {
            let event = NamingWorkerEvent::Prepared {
                event: Box::new(prepared.event),
                source_hold: prepared.source_hold,
            };
            match self.events_tx.try_send(event) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(
                    NamingWorkerEvent::Prepared { event, source_hold },
                )) => {
                    finite.return_ready(finite::Prepared {
                        event: *event,
                        source_hold,
                    });
                    break;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    finite.close();
                    break;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    unreachable!("the sender always publishes the Prepared wrapper")
                }
            }
        }
    }
    pub(crate) fn has_pending_exact_delivery(&self) -> bool {
        self.exact
            .as_ref()
            .is_some_and(|exact| exact.delivery_pending())
    }
    pub(crate) fn close_finite(&mut self) {
        self.original_retry_at = None;
        self.settings_retry = None;
        if let Some(exact) = &mut self.exact {
            exact.close();
        }
        if let Some(finite) = &mut self.finite {
            finite.close();
        }
    }

    /// The event loop updates this after a wizard or provider decision and
    /// before it accepts an automatic worker result or dispatches new work.
    pub fn set_automatic_ai_decision(&self, revision: u64, allowed: bool) {
        self.automatic_ai_decision.store(
            AutomaticAiDecision::new(revision, allowed).0,
            Ordering::SeqCst,
        );
    }

    pub fn is_current_automatic_ai_decision(&self, decision: AutomaticAiDecision) -> bool {
        decision.is_current(&self.automatic_ai_decision)
    }

    pub(crate) fn automatic_ai_decision_fence(
        &self,
        decision: AutomaticAiDecision,
    ) -> AutomaticAiDecisionFence {
        AutomaticAiDecisionFence {
            expected: decision.0,
            current: Arc::clone(&self.automatic_ai_decision),
        }
    }

    /// Spawns the one-shot project-name bootstrap worker, unless one is
    /// already running. A no-op call (e.g. a stored name already loaded
    /// synchronously at startup) is the caller's responsibility to avoid.
    pub(crate) fn spawn_project_name_worker(
        &mut self,
        cwd: PathBuf,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        if self.project_name_in_flight {
            return Ok(());
        }
        self.enqueue_finite(NamingKind::ProjectName(cwd))?;
        self.project_name_in_flight = true;
        Ok(())
    }

    pub fn project_name_worker_finished(&mut self) {
        self.project_name_in_flight = false;
    }

    /// Spawns a session-title inference worker for `pane_id`, unless one is
    /// already running for it -- see the module docs for what triggers this.
    pub(crate) fn spawn_session_title_worker(
        &mut self,
        request: SessionTitleWorkerRequest,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        if self.session_title_in_flight.keys().any(|(pane, session)| {
            *pane == request.input.pane_id && *session == request.input.session_id
        }) {
            return Ok(());
        }
        let accepted = self.enqueue_finite(NamingKind::SessionTitle(Box::new(request)))?;
        if let Some(key) = accepted.session_key {
            self.session_title_in_flight
                .insert(key, accepted.source_hold);
        }
        Ok(())
    }

    pub fn session_title_worker_finished(&mut self, pane_id: NodeId, session_id: &str) {
        self.session_title_in_flight
            .retain(|(pane, session), _| *pane != pane_id || session != session_id);
    }

    /// At most one owned worker per pane. A newer Enter replaces its pending
    /// request and wakes it; closing/replacing a pane drops the owner.
    pub fn spawn_exact_agent_prompt_transcript_worker(
        &mut self,
        request: ExactAgentPromptTranscriptRequest,
    ) -> Result<(), Box<ilium_execution::Rejected<ExactAgentPromptTranscriptRequest>>> {
        let Some(exact) = &mut self.exact else {
            return Err(Box::new(ilium_execution::Rejected {
                reason: ilium_execution::RejectReason::Closed,
                value: request,
            }));
        };
        exact.request(request)
    }
    pub fn cancel_exact_prompt_worker(&mut self, pane: NodeId) {
        if let Some(exact) = &mut self.exact {
            exact.cancel(pane);
        }
    }
    pub fn cancel_stale_exact_prompt_workers(&mut self, keep: impl FnMut(NodeId, &str) -> bool) {
        if let Some(exact) = &mut self.exact {
            exact.cancel_stale(keep);
        }
    }

    /// Spawns a background check of the agent CLI's own session transcript
    /// for `request.pane_id`'s most recent user message -- the "OR get it
    /// from the .jsonl history file" fallback/upgrade for the last-prompt
    /// banner. Deliberately no in-flight dedup: each call is one bounded,
    /// idempotent file read (worst case ~10s), so an Enter press racing a
    /// still-running prior check just costs a redundant read rather than
    /// risking a dropped update.
    #[cfg(test)]
    pub(crate) fn spawn_last_prompt_transcript_worker(
        &mut self,
        request: LastPromptTranscriptWorkerRequest,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        self.enqueue_finite(NamingKind::LastPrompt(Box::new(request)))
            .map(|_| ())
    }

    /// Spawns a terminal-screen title inference worker for `input.pane_id`,
    /// unless one is already running for it -- see `crate::terminal_naming`
    /// and the manual/automatic request paths in `App`.
    pub(crate) fn spawn_terminal_title_worker(
        &mut self,
        input: crate::terminal_naming::TerminalTitleInput,
        trigger: TitleTrigger,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        let pane_id = input.pane_id;
        if self.terminal_title_in_flight.contains(&pane_id) {
            return Ok(());
        }
        self.enqueue_finite(NamingKind::TerminalTitle(Box::new(input), trigger))?;
        self.terminal_title_in_flight.insert(pane_id);
        Ok(())
    }

    pub fn terminal_title_worker_finished(&mut self, pane_id: NodeId) {
        self.terminal_title_in_flight.remove(&pane_id);
    }

    pub fn set_inference_settings(
        &mut self,
        settings: &InferenceSettings,
    ) -> Result<(), ilium_execution::RejectReason> {
        if &self.inference_settings.settings == settings {
            self.settings_retry = None;
            return Ok(());
        }
        let now = Instant::now();
        if let Some((deadline, reason)) = self.settings_retry {
            if deadline > now {
                return Err(reason);
            }
        }
        match finite::SettingsSnapshot::capture(settings) {
            Ok(snapshot) => {
                self.inference_settings = snapshot;
                self.settings_retry = None;
                Ok(())
            }
            Err(reason) => {
                self.settings_retry = matches!(
                    reason,
                    ilium_execution::RejectReason::Busy
                        | ilium_execution::RejectReason::WorkerBytes
                )
                .then_some((now + Duration::from_millis(100), reason));
                Err(reason)
            }
        }
    }

    pub(crate) fn spawn_inference_test_worker(
        &mut self,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        if self.inference_test_in_flight {
            return Ok(());
        }
        self.enqueue_finite(NamingKind::InferenceTest)?;
        self.inference_test_in_flight = true;
        Ok(())
    }

    pub fn inference_test_worker_finished(&mut self) {
        self.inference_test_in_flight = false;
    }

    pub(crate) fn spawn_model_discovery_worker(
        &mut self,
        provider: ilium_inference::InferenceProviderKind,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        if self.model_discovery_in_flight {
            return Ok(());
        }
        self.enqueue_finite(NamingKind::Models(provider))?;
        self.model_discovery_in_flight = true;
        Ok(())
    }
    pub fn model_discovery_worker_finished(&mut self) {
        self.model_discovery_in_flight = false;
    }

    /// Spawns the whole-tree restructure worker, unless one is already
    /// running -- `App::structure_loading` is this method's matching
    /// per-`App` guard, kept there (not read from here) since only `App`
    /// decides whether to gather `contexts` at all. Resolves agent
    /// transcripts (`crate::restructure::resolve_content_extracts`) before
    /// calling the LLM, mirroring `spawn_session_title_worker`'s own
    /// disk-I/O-inside-the-closure pattern.
    pub(crate) fn spawn_restructure_worker(
        &mut self,
        request: crate::app::PendingRestructureRequest,
        home: PathBuf,
    ) -> Result<(), Box<ilium_execution::Rejected<NamingRequest>>> {
        let project_id = request.project_id;
        if self.restructure_in_flight.contains(&project_id) {
            return Ok(());
        }
        self.enqueue_finite(NamingKind::Restructure(Box::new(request), home))?;
        self.restructure_in_flight.insert(project_id);
        Ok(())
    }

    pub fn restructure_worker_finished(&mut self, project_id: NodeId) {
        self.restructure_in_flight.remove(&project_id);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn await_finite_event(
        workers: &mut NamingWorkers,
        events: &mut tokio::sync::mpsc::Receiver<NamingWorkerEvent>,
    ) -> finite::Prepared {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            workers.collect();
            if let Ok(NamingWorkerEvent::Prepared { event, source_hold }) = events.try_recv() {
                return finite::Prepared {
                    event: *event,
                    source_hold,
                };
            }
            assert!(
                Instant::now() < deadline,
                "real finite naming worker did not publish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn returned_unstarted_provider_permit_does_not_self_wake_but_completed_body_does() {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};
        let wake = Arc::new(tokio::sync::Notify::new());
        let mut limiter = InferenceConcurrencyLimiter::new(1);
        limiter.release_wake = Arc::clone(&wake);
        let limiter = Arc::new(limiter);
        let mut notified = Box::pin(wake.notified());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            notified.as_mut().poll(&mut context),
            Poll::Pending
        ));
        let permit = limiter.try_acquire().unwrap();
        assert!(limiter.try_acquire().is_none());
        // A rejected bank admission returns a permit without running a body.
        drop(permit);
        assert!(matches!(
            notified.as_mut().poll(&mut context),
            Poll::Pending
        ));
        let mut permit = limiter.try_acquire().unwrap();
        permit.mark_started();
        drop(permit);
        assert!(matches!(
            notified.as_mut().poll(&mut context),
            Poll::Ready(())
        ));
        assert!(limiter.try_acquire().is_some());
    }

    #[test]
    fn cancelled_blocked_body_retains_both_permits_until_actual_exit() {
        use ilium_execution::{JobCost, JobOutcome, JobPoll, Lane};
        use std::ops::ControlFlow;
        let directory = tempfile::tempdir().expect("isolated lock directory");
        let path = directory.path().join("body.lock");
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job_path = path.clone();
        let job_limiter = Arc::clone(&limiter);
        let mut receipt = crate::execution::test_client()
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
                move |context| {
                    Ok::<_, std::convert::Infallible>(crate::provider_admission::run_with_probe(
                        "original".to_owned(),
                        context,
                        &job_limiter,
                        |_| false,
                        |_, _| {
                            entered_tx.send(()).expect("announce body entry");
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("bounded body release");
                        },
                        || ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&job_path),
                    ))
                },
            )
            .expect("admit isolated blocking job");
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("body entered with both permits");
        receipt.cancel();
        assert!(
            limiter.try_acquire().is_none(),
            "cancel is not physical exit"
        );
        assert!(
            ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&path)
                .expect("contended host probe")
                .is_none(),
            "cancel must retain the actual host lock"
        );
        release_tx.send(()).expect("release test body");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match receipt.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline, "body did not exit");
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(result) => {
                    let (outcome, retention) = result.into_parts();
                    assert!(matches!(
                        outcome,
                        JobOutcome::Finished(Ok(ControlFlow::Continue(())))
                    ));
                    drop(retention);
                    break;
                }
                JobPoll::Lost | JobPoll::Taken => panic!("blocked body outcome lost"),
            }
        }
        assert!(
            limiter.try_acquire().is_some(),
            "process permit released at exit"
        );
        assert!(
            ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&path)
                .expect("post-exit host probe")
                .is_some(),
            "host lock released at exit"
        );
    }

    #[test]
    fn body_panic_releases_both_permits_without_recreating_original() {
        use ilium_execution::{JobCost, JobOutcome, JobPoll, Lane};
        let directory = tempfile::tempdir().expect("isolated lock directory");
        let path = directory.path().join("panic.lock");
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let job_limiter = Arc::clone(&limiter);
        let job_path = path.clone();
        let mut receipt = crate::execution::test_client()
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
                move |context| {
                    Ok::<_, std::convert::Infallible>(crate::provider_admission::run_with_probe(
                        "consumed once".to_owned(),
                        context,
                        &job_limiter,
                        |_| false,
                        |_, _| -> () { panic!("synthetic synchronous body panic") },
                        || ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&job_path),
                    ))
                },
            )
            .expect("admit panic job");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match receipt.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < deadline, "panic did not settle");
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(result) => {
                    let (outcome, retention) = result.into_parts();
                    assert!(matches!(outcome, JobOutcome::Panicked));
                    drop(retention);
                    break;
                }
                JobPoll::Lost | JobPoll::Taken => panic!("panic outcome lost"),
            }
        }
        assert!(limiter.try_acquire().is_some());
        assert!(
            ilium_platform::file_lock::ExclusiveFileLock::try_acquire(&path)
                .expect("host lock after unwind")
                .is_some()
        );
    }

    #[test]
    fn naming_and_smart_copy_share_the_exact_process_provider_gate() {
        let (events_tx, _) = tokio::sync::mpsc::channel(1);
        let workers = NamingWorkers::new(events_tx, &InferenceSettings::default()).unwrap();
        assert!(Arc::ptr_eq(
            &workers.concurrency_limiter,
            &shared_provider_limiter()
        ));
    }

    #[test]
    fn queued_project_worker_is_cancelled_before_provider_call() {
        let cwd = tempfile::tempdir().unwrap();
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(events_tx, &InferenceSettings::default()).unwrap();
        let held_permits = (0..MAX_CONCURRENT_INFERENCE_JOBS)
            .map(|_| workers.concurrency_limiter.acquire())
            .collect::<Vec<_>>();
        workers
            .spawn_project_name_worker(cwd.path().to_path_buf())
            .unwrap();
        workers.set_automatic_ai_decision(1, false);
        drop(held_permits);

        let prepared = await_finite_event(&mut workers, &mut events_rx);
        let event = prepared.event;
        let NamingWorkerEvent::ProjectName { result, .. } = event else {
            panic!("expected project-name completion");
        };
        assert_eq!(
            result.unwrap_err().to_string(),
            "automatic AI request cancelled before provider call"
        );
        assert!(!cwd.path().join(".ilium/config.yaml").exists());
    }

    /// End-to-end regression for the last-prompt banner's ".jsonl transcript"
    /// fallback: writes a synthetic Claude Code transcript to a temp home
    /// directory (same shape `ilium-agent-session`'s own fixtures use),
    /// spawns the real worker against it, and asserts the worker's result
    /// actually carries the transcript's last user message back out. This is
    /// the durable regression coverage the throwaway
    /// `diagnose_real_claude_last_prompt` PTY test could not provide, since a
    /// nested-under-test `claude` process has transcript saving disabled.
    #[test]
    fn last_prompt_transcript_worker_reads_the_most_recent_user_message() {
        let home = tempfile::tempdir().expect("temp home dir");
        let project_path = std::path::Path::new("/work/ilium-transcript-test");
        let session_id = "33333333-3333-4333-8333-333333333333";
        // Mirrors Claude Code's own project-directory slug (every non-ASCII-
        // alphanumeric character becomes `-`), duplicated here rather than
        // reaching into `ilium-agent-session`'s private `slugify_claude_project_path`.
        let slug: String = project_path
            .to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        let project_dir = home.path().join(".claude").join("projects").join(slug);
        std::fs::create_dir_all(&project_dir).expect("create claude project dir");
        let transcript_path = project_dir.join(format!("{session_id}.jsonl"));
        let lines = [
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "first prompt, superseded"}
            }),
            serde_json::json!({
                "type": "assistant",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": [{"type": "text", "text": "on it"}]}
            }),
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "fix the failing tests"}
            }),
        ]
        .map(|entry| entry.to_string())
        .join("\n");
        std::fs::write(&transcript_path, lines).expect("write synthetic transcript");

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(events_tx, &InferenceSettings::default()).unwrap();
        let pane_id = NodeId(11);
        workers
            .spawn_last_prompt_transcript_worker(LastPromptTranscriptWorkerRequest {
                home: home.path().to_path_buf(),
                pane_id,
                project_path: project_path.to_path_buf(),
                agent_class: AgentClass::Claude,
                session_id: session_id.to_string(),
                baseline_last_prompt: None,
            })
            .unwrap();

        let prepared = await_finite_event(&mut workers, &mut events_rx);
        let event = prepared.event;
        let NamingWorkerEvent::LastPromptTranscript(result) = event else {
            panic!("expected a LastPromptTranscript event");
        };
        assert_eq!(result.pane_id, pane_id);
        assert_eq!(result.session_id, session_id);
        assert_eq!(
            result.last_prompt,
            Some("fix the failing tests".to_string()),
            "must pick the most recent user message, not the superseded first one"
        );
    }

    /// Claude Code's own project-directory slug (every non-ASCII-alphanumeric
    /// character becomes `-`), duplicated here rather than reaching into
    /// `ilium-agent-session`'s private `slugify_claude_project_path`.
    fn claude_project_slug(project_path: &std::path::Path) -> String {
        project_path
            .to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect()
    }

    fn write_claude_transcript(
        home: &std::path::Path,
        project_path: &std::path::Path,
        session_id: &str,
        user_messages: &[&str],
    ) -> std::path::PathBuf {
        let project_dir = home
            .join(".claude")
            .join("projects")
            .join(claude_project_slug(project_path));
        std::fs::create_dir_all(&project_dir).expect("create claude project dir");
        let transcript_path = project_dir.join(format!("{session_id}.jsonl"));
        let lines = user_messages
            .iter()
            .map(|content| {
                serde_json::json!({
                    "type": "user",
                    "sessionId": session_id,
                    "cwd": project_path,
                    "message": {"content": content}
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&transcript_path, lines).expect("write synthetic transcript");
        transcript_path
    }

    /// Regression for the staleness bug: a transcript read that lands before
    /// the agent CLI flushes this turn's message sees only the previous
    /// turn's already-applied text -- which is exactly what
    /// `baseline_last_prompt` holds. A worker that accepted the first
    /// `Some(_)` it read (the pre-fix behavior) would report that stale text
    /// as this submission's answer and never look again. The fixed worker
    /// must keep polling past that stale read and pick up the new message
    /// once it actually lands.
    #[test]
    fn stale_transcript_read_matching_the_baseline_is_retried_until_it_changes() {
        let home = tempfile::tempdir().expect("temp home dir");
        let project_path = std::path::Path::new("/work/ilium-transcript-stale-test");
        let session_id = "44444444-4444-4444-8444-444444444444";
        write_claude_transcript(home.path(), project_path, session_id, &["old prompt"]);

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(events_tx, &InferenceSettings::default()).unwrap();
        let pane_id = NodeId(12);
        workers
            .spawn_last_prompt_transcript_worker(LastPromptTranscriptWorkerRequest {
                home: home.path().to_path_buf(),
                pane_id,
                project_path: project_path.to_path_buf(),
                agent_class: AgentClass::Claude,
                session_id: session_id.to_string(),
                baseline_last_prompt: Some("old prompt".to_string()),
            })
            .unwrap();

        // Appended only after the worker's first (pre-fix: only) read would
        // already have happened, simulating the agent CLI's flush landing
        // late.
        std::thread::sleep(LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY * 2);
        write_claude_transcript(
            home.path(),
            project_path,
            session_id,
            &["old prompt", "new prompt"],
        );

        let prepared = await_finite_event(&mut workers, &mut events_rx);
        let event = prepared.event;
        let NamingWorkerEvent::LastPromptTranscript(result) = event else {
            panic!("expected a LastPromptTranscript event");
        };
        assert_eq!(
            result.last_prompt,
            Some("new prompt".to_string()),
            "must not settle for the baseline-matching stale read"
        );
    }

    /// When the transcript never advances past the baseline (the agent CLI
    /// never flushed this turn, or the read genuinely raced something else
    /// forever), the worker must give up and report `None` rather than
    /// eventually returning the stale baseline text as if it were fresh.
    #[test]
    fn transcript_stuck_on_the_baseline_reports_none_after_exhausting_retries() {
        let home = tempfile::tempdir().expect("temp home dir");
        let project_path = std::path::Path::new("/work/ilium-transcript-stuck-test");
        let session_id = "55555555-5555-4555-8555-555555555555";
        write_claude_transcript(home.path(), project_path, session_id, &["only prompt"]);

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(events_tx, &InferenceSettings::default()).unwrap();
        let pane_id = NodeId(13);
        workers
            .spawn_last_prompt_transcript_worker(LastPromptTranscriptWorkerRequest {
                home: home.path().to_path_buf(),
                pane_id,
                project_path: project_path.to_path_buf(),
                agent_class: AgentClass::Claude,
                session_id: session_id.to_string(),
                baseline_last_prompt: Some("only prompt".to_string()),
            })
            .unwrap();

        let prepared = await_finite_event(&mut workers, &mut events_rx);
        let event = prepared.event;
        let NamingWorkerEvent::LastPromptTranscript(result) = event else {
            panic!("expected a LastPromptTranscript event");
        };
        assert_eq!(result.last_prompt, None);
    }

    pub(crate) fn exact_prompt_request_for_test(
        home: &std::path::Path,
        pane_id: NodeId,
        epoch: &str,
    ) -> ExactAgentPromptTranscriptRequest {
        let project_path = home.join("synthetic-exact-worker-project");
        std::fs::create_dir_all(&project_path).unwrap();
        let session_id = "77777777-7777-4777-8777-777777777777";
        let directory = home
            .join(".claude/projects")
            .join(claude_project_slug(&project_path));
        std::fs::create_dir_all(&directory).unwrap();
        let verified_path = directory.join(format!("{session_id}.jsonl"));
        let submitted_after = chrono::Utc::now();
        let row = serde_json::json!({
            "type": "user", "sessionId": session_id, "cwd": project_path,
            "timestamp": submitted_after.to_rfc3339(),
            "message": {"content": "synthetic exact\nworker prompt  "}
        });
        std::fs::write(&verified_path, format!("{row}\n")).unwrap();
        ExactAgentPromptTranscriptRequest {
            home: home.to_path_buf(),
            pane_id,
            project_path,
            agent_class: AgentClass::Claude,
            session_id: session_id.to_string(),
            verified_path,
            baseline_length: 0,
            submitted_after,
            prompt_epoch: epoch.to_string(),
        }
    }

    fn receive_exact_prompt_for_test(
        workers: &mut NamingWorkers,
        receiver: &mut tokio::sync::mpsc::Receiver<NamingWorkerEvent>,
    ) -> finite::Prepared {
        await_finite_event(workers, receiver)
    }

    #[test]
    fn exact_prompt_idle_completion_releases_finite_request_and_context() {
        let home = tempfile::tempdir().unwrap();
        let pane = NodeId(771);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let mut workers = NamingWorkers::new(sender, &InferenceSettings::default()).unwrap();
        workers
            .spawn_exact_agent_prompt_transcript_worker(exact_prompt_request_for_test(
                home.path(),
                pane,
                "idle-enter",
            ))
            .unwrap();
        let prepared = receive_exact_prompt_for_test(&mut workers, &mut receiver);
        let NamingWorkerEvent::ExactPrepared { result, .. } = prepared.event else {
            panic!("exact result required")
        };
        assert_eq!(result.prompt_epoch, "idle-enter");
        assert_eq!(
            result.last_prompt.as_deref(),
            Some("synthetic exact\nworker prompt  ")
        );
        assert_eq!(workers.exact.as_ref().unwrap().context_count_for_test(), 0);
        assert_eq!(workers.exact.as_ref().unwrap().active_count_for_test(), 0);
        workers.cancel_exact_prompt_worker(pane);
    }

    #[test]
    fn exact_prompt_same_pane_supersession_reports_new_epoch_only() {
        let home = tempfile::tempdir().unwrap();
        let pane = NodeId(772);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let mut workers = NamingWorkers::new(sender, &InferenceSettings::default()).unwrap();
        workers
            .spawn_exact_agent_prompt_transcript_worker(exact_prompt_request_for_test(
                home.path(),
                pane,
                "obsolete-enter",
            ))
            .unwrap();
        workers
            .spawn_exact_agent_prompt_transcript_worker(exact_prompt_request_for_test(
                home.path(),
                pane,
                "current-enter",
            ))
            .unwrap();
        assert_eq!(workers.exact.as_ref().unwrap().context_count_for_test(), 1);
        let prepared = receive_exact_prompt_for_test(&mut workers, &mut receiver);
        let NamingWorkerEvent::ExactPrepared { result, .. } = prepared.event else {
            panic!("exact result required")
        };
        assert_eq!(result.pane_id, pane);
        assert_eq!(result.prompt_epoch, "current-enter");
        assert_eq!(
            result.last_prompt.as_deref(),
            Some("synthetic exact\nworker prompt  ")
        );
        assert!(
            receiver.try_recv().is_err(),
            "obsolete request emitted a result"
        );
    }

    #[test]
    fn exact_prompt_full_event_channel_retains_result_and_cancels_without_waiting() {
        let home = tempfile::tempdir().unwrap();
        let pane = NodeId(773);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(NamingWorkerEvent::LastPromptTranscript(
                LastPromptTranscriptWorkerResult {
                    pane_id: NodeId(774),
                    session_id: "synthetic-full-channel-sentinel".into(),
                    last_prompt: None,
                },
            ))
            .unwrap_or_else(|_| panic!("empty channel"));
        let mut workers = NamingWorkers::new(sender, &InferenceSettings::default()).unwrap();
        workers
            .spawn_exact_agent_prompt_transcript_worker(exact_prompt_request_for_test(
                home.path(),
                pane,
                "full-channel-enter",
            ))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while workers.exact.as_ref().unwrap().ready_count_for_test() == 0 {
            workers.collect();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(workers.events_tx.capacity(), 0);
        assert_eq!(
            workers.exact.as_ref().unwrap().active_count_for_test(),
            0,
            "finite IO probe is released even when output cannot publish"
        );
        workers.cancel_exact_prompt_worker(pane);
        assert_eq!(workers.exact.as_ref().unwrap().context_count_for_test(), 0);
        assert_eq!(workers.exact.as_ref().unwrap().ready_count_for_test(), 0);
        let NamingWorkerEvent::LastPromptTranscript(sentinel) = receiver.try_recv().unwrap() else {
            panic!("sentinel unchanged")
        };
        assert_eq!(sentinel.session_id, "synthetic-full-channel-sentinel");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn closing_retains_completed_exact_evidence_until_full_channel_can_drain() {
        let home = tempfile::tempdir().unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(NamingWorkerEvent::LastPromptTranscript(
                LastPromptTranscriptWorkerResult {
                    pane_id: NodeId(774),
                    session_id: "preserved-sentinel".into(),
                    last_prompt: None,
                },
            ))
            .unwrap_or_else(|_| panic!("empty channel"));
        let mut workers = NamingWorkers::new(sender, &InferenceSettings::default()).unwrap();
        workers
            .spawn_exact_agent_prompt_transcript_worker(exact_prompt_request_for_test(
                home.path(),
                NodeId(775),
                "completed-before-close",
            ))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while workers.exact.as_ref().unwrap().ready_count_for_test() == 0 {
            workers.collect();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        workers.close_finite();
        assert!(workers.has_pending_exact_delivery());
        let NamingWorkerEvent::LastPromptTranscript(sentinel) = receiver.try_recv().unwrap() else {
            panic!("sentinel must remain first")
        };
        assert_eq!(sentinel.session_id, "preserved-sentinel");
        workers.collect();
        let NamingWorkerEvent::Prepared { event, .. } = receiver.try_recv().unwrap() else {
            panic!("guarded exact delivery")
        };
        let NamingWorkerEvent::ExactPrepared { result, .. } = *event else {
            panic!("exact evidence after close")
        };
        assert_eq!(result.prompt_epoch, "completed-before-close");
        assert_eq!(
            result.last_prompt.as_deref(),
            Some("synthetic exact\nworker prompt  ")
        );
        assert!(!workers.has_pending_exact_delivery());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn inference_concurrency_limiter_never_admits_more_than_its_capacity() {
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(2));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let mut handles = Vec::new();

        for worker in 0..3 {
            let limiter = Arc::clone(&limiter);
            let release = Arc::clone(&release);
            let entered_tx = entered_tx.clone();
            handles.push(std::thread::spawn(move || {
                let _permit = limiter.acquire();
                entered_tx.send(worker).expect("report admitted worker");
                let (lock, available) = &*release;
                let mut is_released = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*is_released {
                    is_released = available
                        .wait(is_released)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }));
        }
        drop(entered_tx);

        entered_rx.recv().expect("first worker admitted");
        entered_rx.recv().expect("second worker admitted");
        assert!(entered_rx.try_recv().is_err());

        let (lock, available) = &*release;
        *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        available.notify_all();
        entered_rx
            .recv()
            .expect("queued worker eventually admitted");
        for handle in handles {
            handle.join().expect("worker exits cleanly");
        }
    }
}
