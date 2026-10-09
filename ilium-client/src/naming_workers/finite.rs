//! Typed finite naming work with one nonblocking provider preflight per IO attempt.
//! Refusal returns the same captured input to a bounded coordinator retry timer.
use super::*;
use ilium_execution::{Job, JobContext, StorageAdmission};
use std::ops::ControlFlow;

pub(crate) enum Kind {
    ProjectName(PathBuf),
    SessionTitle(Box<SessionTitleWorkerRequest>),
    TerminalTitle(
        Box<crate::terminal_naming::TerminalTitleInput>,
        TitleTrigger,
    ),
    InferenceTest,
    Models(ilium_inference::InferenceProviderKind),
    Restructure(Box<crate::app::PendingRestructureRequest>, PathBuf),
    #[cfg(test)]
    LastPrompt(Box<LastPromptTranscriptWorkerRequest>),
}

/// Immutable configuration shared by base ownership and every rejected original.
pub(crate) struct SettingsSnapshot {
    pub settings: InferenceSettings,
    _hold: Arc<StorageAdmission>,
}
impl SettingsSnapshot {
    pub(super) fn capture(
        settings: &InferenceSettings,
    ) -> Result<Arc<Self>, ilium_execution::RejectReason> {
        let bytes = inference_settings_bytes(settings);
        if bytes > MAX_CAPTURE_BYTES {
            return Err(ilium_execution::RejectReason::InvalidCost);
        }
        let hold = Arc::new(crate::execution::process_quota().reserve_external_storage(bytes)?);
        Ok(Arc::new(Self {
            settings: settings.clone(),
            _hold: hold,
        }))
    }
}
/// The exact desired request travels back to its owner on every refusal.
pub(crate) struct Request {
    pub kind: Kind,
    pub settings: Arc<SettingsSnapshot>,
    pub decision: AutomaticAiDecision,
    pub decision_word: Arc<AtomicU64>,
    source_hold: Option<Arc<StorageAdmission>>,
}
impl Request {
    pub(super) fn new(kind: Kind, settings: Arc<SettingsSnapshot>, word: &Arc<AtomicU64>) -> Self {
        Self {
            kind,
            settings,
            decision: AutomaticAiDecision(word.load(Ordering::SeqCst)),
            decision_word: Arc::clone(word),
            source_hold: None,
        }
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.kind.captured_bytes()
    }
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.kind.fmt(f)
    }
}

pub(super) struct Accepted {
    pub session_key: Option<(NodeId, String)>,
    pub source_hold: Arc<StorageAdmission>,
}

pub(super) struct Captured {
    pub request: Request,
    pub decision: AutomaticAiDecision,
    pub decision_word: Arc<AtomicU64>,
    pub source_hold: Arc<StorageAdmission>,
}

pub(super) struct NamingJob {
    pub captured: Captured,
    pub limiter: Arc<InferenceConcurrencyLimiter>,
}

pub(super) struct Prepared {
    pub event: NamingWorkerEvent,
    // Last field: actual result payload must die before its resident debit.
    pub source_hold: Arc<StorageAdmission>,
}

impl Job for NamingJob {
    type Output = ControlFlow<(Captured, std::io::Error), Prepared>;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        let Self { captured, limiter } = self;
        if !captured.request.kind.uses_provider() {
            return Ok(ControlFlow::Continue(run_captured(captured, context)));
        }
        Ok(crate::provider_admission::run(
            captured,
            context,
            &limiter,
            Captured::revoked,
            run_captured,
        ))
    }
}

impl Captured {
    fn revoked(&self) -> bool {
        self.request.kind.uses_automatic_ai() && !self.decision.is_current(&self.decision_word)
    } // The same atomic identity follows every refused attempt.
    fn failed(self, error: std::io::Error) -> Prepared {
        let error = if error.kind() == std::io::ErrorKind::Interrupted
            && self.request.kind.uses_automatic_ai()
        {
            anyhow::Error::new(error).context("automatic AI request cancelled before provider call")
        } else {
            anyhow::Error::new(error)
        };
        let event = self.request.kind.failure_event_with(
            &self.request.settings.settings,
            self.decision,
            error,
        );
        Prepared {
            event,
            source_hold: self.source_hold,
        }
    } // No settings, generation, or decision are refreshed while failing an original.
}

fn run_captured(captured: Captured, context: JobContext) -> Prepared {
    let Captured {
        request,
        decision,
        decision_word,
        source_hold,
    } = captured;
    let cancelled = context.stop_requested();
    let settings = &request.settings.settings;
    let failure = request.kind.failure_event(settings, decision);
    let event = panic::catch_unwind(AssertUnwindSafe(|| {
        run_request(
            request.kind,
            settings,
            decision,
            &decision_word,
            cancelled,
            &context,
        )
    }))
    .unwrap_or(failure);
    Prepared { event, source_hold }
}

fn provider_allowed(
    cancelled: bool,
    decision: AutomaticAiDecision,
    word: &AtomicU64,
) -> anyhow::Result<()> {
    if cancelled || !decision.is_current(word) {
        anyhow::bail!("automatic AI request cancelled before provider call");
    }
    Ok(())
}

fn run_request(
    request: Kind,
    settings: &InferenceSettings,
    decision: AutomaticAiDecision,
    word: &AtomicU64,
    cancelled: bool,
    context: &JobContext,
) -> NamingWorkerEvent {
    match request {
        Kind::ProjectName(cwd) => NamingWorkerEvent::ProjectName {
            decision,
            result: provider_allowed(cancelled, decision, word).and_then(|()| {
                catch_worker_panic("project name", || {
                    crate::project_naming::infer_project_name_without_persisting(&cwd, settings)
                })
            }),
        },
        Kind::SessionTitle(request) => {
            let SessionTitleWorkerRequest {
                home,
                input,
                title_generation,
                trigger,
            } = *request;
            let started_at = Instant::now();
            let trace = match provider_allowed(cancelled, decision, word) {
                Err(error) => crate::session_naming::SessionTitleInferenceTrace {
                    rendered_prompt: None,
                    raw_response: None,
                    result: Err(error),
                },
                Ok(()) => panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::session_naming::infer_pane_title_with_trace(settings, &home, &input)
                }))
                .unwrap_or_else(|_| {
                    crate::session_naming::SessionTitleInferenceTrace {
                        rendered_prompt: None,
                        raw_response: None,
                        result: Err(anyhow::anyhow!("session title worker panicked")),
                    }
                }),
            };
            NamingWorkerEvent::SessionTitle(SessionTitleWorkerResult {
                pane_id: input.pane_id,
                presentation_revision: input.presentation_revision,
                process_id: input.process_id,
                session_id: input.session_id,
                title_generation,
                provider: settings.selected_provider,
                elapsed: started_at.elapsed(),
                rendered_prompt: trace.rendered_prompt,
                raw_response: trace.raw_response,
                result: trace.result,
                trigger,
                automatic_ai_decision: decision,
            })
        }
        Kind::TerminalTitle(input, trigger) => NamingWorkerEvent::TerminalTitle(
            input.pane_id,
            provider_allowed(cancelled, decision, word).and_then(|()| {
                catch_worker_panic("terminal title", || {
                    crate::terminal_naming::infer_terminal_title(settings, &*input)
                })
            }),
            trigger,
            decision,
        ),
        Kind::InferenceTest => {
            let started_at = Instant::now();
            let result = if cancelled {
                Err(anyhow::anyhow!(
                    "inference test cancelled before provider call"
                ))
            } else {
                catch_worker_panic("inference test", || crate::inference_test::run(settings))
            };
            NamingWorkerEvent::InferenceTest {
                provider: settings.selected_provider,
                elapsed: started_at.elapsed(),
                result,
            }
        }
        Kind::Models(provider) => {
            // This worker-local derivative is covered by finite scratch admission.
            let mut catalogue_settings = settings.clone();
            catalogue_settings.selected_provider = provider;
            let settings = &catalogue_settings;
            let endpoint = match provider {
                ilium_inference::InferenceProviderKind::KiloGateway => {
                    ilium_inference::kilo_gateway_model_catalog_url()
                }
                ilium_inference::InferenceProviderKind::Ollama => format!(
                    "{}/api/tags",
                    settings.ollama.base_url.trim_end_matches('/')
                ),
                ilium_inference::InferenceProviderKind::OpenAi
                | ilium_inference::InferenceProviderKind::Anthropic => {
                    ilium_inference::model_catalog_endpoint(settings).unwrap_or_default()
                }
                _ => provider.label().to_owned(),
            };
            let started_at = Instant::now();
            let result = if cancelled {
                Err(anyhow::anyhow!(
                    "model discovery cancelled before provider call"
                ))
            } else {
                catch_worker_panic("model discovery", || {
                    ilium_inference::provider_from_settings(settings)
                        .list_models()
                        .map_err(anyhow::Error::from)
                })
            };
            NamingWorkerEvent::ProviderModels {
                provider,
                endpoint,
                elapsed: started_at.elapsed(),
                result,
            }
        }
        Kind::Restructure(request, home) => {
            let crate::app::PendingRestructureRequest {
                animation_home,
                recommendation_snapshot,
                project_id,
                mut contexts,
                title_observations,
                protected_split_views,
                current_structure,
                inference_activity_revisions,
                ..
            } = *request;
            let result = provider_allowed(cancelled, decision, word).and_then(|()| {
                // Context mutations are job-local and discarded on panic; no
                // shared state can observe a half-resolved context vector.
                catch_worker_panic(
                    "restructure",
                    AssertUnwindSafe(|| {
                        crate::restructure::resolve_content_extracts(&mut contexts, &home);
                        provider_allowed(context.stop_requested(), decision, word)?;
                        crate::restructure::infer_project_restructure(
                            settings,
                            &contexts,
                            &current_structure,
                            &protected_split_views,
                            &recommendation_snapshot,
                            &animation_home,
                        )
                    }),
                )
            });
            NamingWorkerEvent::Restructure(RestructureWorkerResult {
                project_id,
                title_observations,
                inference_activity_revisions,
                automatic_ai_decision: decision,
                result,
            })
        }
        #[cfg(test)]
        Kind::LastPrompt(request) => {
            let LastPromptTranscriptWorkerRequest {
                home,
                pane_id,
                project_path,
                agent_class,
                session_id,
                baseline_last_prompt,
            } = *request;
            let baseline = baseline_last_prompt.as_deref().map(str::trim);
            let mut last_prompt = None;
            if !cancelled {
                std::thread::sleep(LAST_PROMPT_TRANSCRIPT_INITIAL_DELAY);
                for attempt in 0..LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS {
                    if context.stop_requested() {
                        break;
                    }
                    let candidate = TranscriptLocator::new(&home, &project_path)
                        .transcript_for_session(&agent_class, &session_id)
                        .and_then(|transcript| {
                            crate::transcript_context::recent_user_prompts(
                                &agent_class,
                                &transcript.path,
                            )
                            .ok()
                        })
                        .and_then(|prompts| prompts.into_iter().next_back());
                    if candidate
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty() && Some(value) != baseline)
                    {
                        last_prompt = candidate;
                        break;
                    }
                    if attempt + 1 < LAST_PROMPT_TRANSCRIPT_MAX_ATTEMPTS {
                        std::thread::sleep(LAST_PROMPT_TRANSCRIPT_RETRY_INTERVAL);
                    }
                }
            }
            NamingWorkerEvent::LastPromptTranscript(LastPromptTranscriptWorkerResult {
                pane_id,
                session_id,
                last_prompt,
            })
        }
    }
}

pub(super) const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_RESULT_BYTES: usize = 32 * 1024 * 1024;
pub(super) const WORKING_BYTES: usize = 128 * 1024 * 1024;

fn agent_bytes(agent: &AgentClass) -> usize {
    match agent {
        AgentClass::Other(value) => value.capacity(),
        _ => 0,
    }
}
fn optional_bytes(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::capacity)
}
fn strings_bytes(values: &Vec<String>) -> usize {
    values
        .capacity()
        .saturating_mul(std::mem::size_of::<String>())
        .saturating_add(values.iter().fold(0usize, |bytes, value| {
            bytes.saturating_add(value.capacity())
        }))
}
impl Kind {
    fn uses_automatic_ai(&self) -> bool {
        matches!(
            self,
            Self::ProjectName(_)
                | Self::SessionTitle(_)
                | Self::TerminalTitle(_, _)
                | Self::Restructure(_, _)
        )
    }
    pub(super) fn uses_provider(&self) -> bool {
        #[cfg(test)]
        if matches!(self, Self::LastPrompt(_)) {
            return false;
        }
        true
    }
    /// Physical captured capacity, including spare Vec/String allocations.
    /// No serialization, provider access or filesystem work occurs here.
    pub(super) fn captured_bytes(&self) -> usize {
        let payload = match self {
            Self::ProjectName(path) => path.capacity(),
            Self::InferenceTest | Self::Models(_) => 0,
            Self::SessionTitle(request) => {
                let input = &request.input;
                std::mem::size_of::<SessionTitleWorkerRequest>()
                    .saturating_add(request.home.capacity())
                    .saturating_add(input.project_name.capacity())
                    .saturating_add(input.project_path.capacity())
                    .saturating_add(agent_bytes(&input.agent_class))
                    .saturating_add(input.session_id.capacity())
                    .saturating_add(input.current_title.capacity())
                    .saturating_add(optional_bytes(&input.current_short_title))
                    .saturating_add(optional_bytes(&input.current_icon))
                    .saturating_add(input.terminal_screen.capacity())
                    .saturating_add(input.parent_group.capacity())
                    .saturating_add(strings_bytes(&input.nearby_titles))
            }
            Self::TerminalTitle(input, _) => {
                std::mem::size_of::<crate::terminal_naming::TerminalTitleInput>()
                    .saturating_add(input.project_name.capacity())
                    .saturating_add(input.project_path.capacity())
                    .saturating_add(input.current_title.capacity())
                    .saturating_add(input.screen_text.capacity())
                    .saturating_add(input.parent_group.capacity())
                    .saturating_add(strings_bytes(&input.nearby_titles))
            }
            #[cfg(test)]
            Self::LastPrompt(request) => std::mem::size_of::<LastPromptTranscriptWorkerRequest>()
                .saturating_add(request.home.capacity())
                .saturating_add(request.project_path.capacity())
                .saturating_add(agent_bytes(&request.agent_class))
                .saturating_add(request.session_id.capacity())
                .saturating_add(optional_bytes(&request.baseline_last_prompt)),
            Self::Restructure(request, home) => {
                let snapshot = &request.recommendation_snapshot;
                let mut bytes =
                    std::mem::size_of::<crate::app::PendingRestructureRequest>()
                        .saturating_add(home.capacity())
                        .saturating_add(request.project_name.capacity())
                        .saturating_add(request.animation_home.capacity())
                        .saturating_add(request.current_structure.capacity())
                        .saturating_add(
                            request
                                .contexts
                                .capacity()
                                .saturating_mul(
                                    std::mem::size_of::<crate::restructure::LeafContext>(),
                                ),
                        )
                        .saturating_add(
                            request.title_observations.capacity().saturating_mul(
                                std::mem::size_of::<ilium_ipc::PaneTitleObservation>(),
                            ),
                        )
                        .saturating_add(request.protected_split_views.capacity().saturating_mul(
                            std::mem::size_of::<crate::restructure::ProtectedSplitViewContext>(),
                        ))
                        .saturating_add(
                            request
                                .inference_activity_revisions
                                .capacity()
                                .saturating_mul(std::mem::size_of::<
                                    ilium_core::NodeActivityRevision,
                                >()),
                        )
                        .saturating_add(
                            snapshot
                                .fixed_groups
                                .capacity()
                                .saturating_mul(std::mem::size_of::<(NodeId, String)>()),
                        );
                for (_, name) in &snapshot.fixed_groups {
                    bytes = bytes.saturating_add(name.capacity());
                }
                // The owned LeafContext accessor includes the private lookup.
                for context in &request.contexts {
                    bytes = bytes.saturating_add(context.retained_bytes());
                }
                for observation in &request.title_observations {
                    bytes = bytes
                        .saturating_add(observation.agent_class.as_ref().map_or(0, agent_bytes))
                        .saturating_add(optional_bytes(&observation.session_id));
                }
                for split in &request.protected_split_views {
                    bytes = bytes
                        .saturating_add(split.current_title.capacity())
                        .saturating_add(
                            split
                                .ordered_pane_ids
                                .capacity()
                                .saturating_mul(std::mem::size_of::<NodeId>()),
                        );
                }
                bytes
            }
        };
        payload.saturating_add(std::mem::size_of::<Self>())
    }
}

impl Kind {
    fn failure_event(
        &self,
        settings: &InferenceSettings,
        decision: AutomaticAiDecision,
    ) -> NamingWorkerEvent {
        self.failure_event_with(
            settings,
            decision,
            anyhow::anyhow!("naming worker failed before publishing its result"),
        )
    }
    fn failure_event_with(
        &self,
        settings: &InferenceSettings,
        decision: AutomaticAiDecision,
        error: anyhow::Error,
    ) -> NamingWorkerEvent {
        match self {
            Self::ProjectName(_) => NamingWorkerEvent::ProjectName {
                decision,
                result: Err(error),
            },
            Self::SessionTitle(request) => {
                NamingWorkerEvent::SessionTitle(SessionTitleWorkerResult {
                    pane_id: request.input.pane_id,
                    presentation_revision: request.input.presentation_revision,
                    process_id: request.input.process_id,
                    session_id: request.input.session_id.clone(),
                    title_generation: request.title_generation,
                    provider: settings.selected_provider,
                    elapsed: Duration::ZERO,
                    rendered_prompt: None,
                    raw_response: None,
                    result: Err(error),
                    trigger: request.trigger,
                    automatic_ai_decision: decision,
                })
            }
            Self::TerminalTitle(input, trigger) => {
                NamingWorkerEvent::TerminalTitle(input.pane_id, Err(error), *trigger, decision)
            }
            Self::InferenceTest => NamingWorkerEvent::InferenceTest {
                provider: settings.selected_provider,
                elapsed: Duration::ZERO,
                result: Err(error),
            },
            Self::Models(provider) => NamingWorkerEvent::ProviderModels {
                provider: *provider,
                endpoint: String::new(),
                elapsed: Duration::ZERO,
                result: Err(error),
            },
            Self::Restructure(request, _) => {
                NamingWorkerEvent::Restructure(RestructureWorkerResult {
                    project_id: request.project_id,
                    title_observations: request.title_observations.clone(),
                    inference_activity_revisions: request.inference_activity_revisions.clone(),
                    automatic_ai_decision: decision,
                    result: Err(error),
                })
            }
            #[cfg(test)]
            Self::LastPrompt(request) => {
                NamingWorkerEvent::LastPromptTranscript(LastPromptTranscriptWorkerResult {
                    pane_id: request.pane_id,
                    session_id: request.session_id.clone(),
                    last_prompt: None,
                })
            }
        }
    }
}

const MAX_OWNED_REQUESTS: usize = 8;
const MAX_ACTIVE_REQUESTS: usize = 2;
const ADMISSION_RETRY_INTERVAL: Duration = Duration::from_millis(100);
struct Active {
    receipt: ilium_execution::Receipt<NamingJob>,
    fallback: Prepared,
}
/// Finite naming admission/receipt owner. It never owns native threads;
/// every callback executes in the shared client's existing I/O bank.
pub(super) struct FiniteWorkers {
    client: ilium_execution::Client,
    limiter: Arc<InferenceConcurrencyLimiter>,
    pending: std::collections::VecDeque<(Captured, Option<Instant>)>,
    active: Vec<Active>,
    ready: std::collections::VecDeque<Prepared>,
    closing: bool,
}
impl FiniteWorkers {
    pub(super) fn new(
        client: ilium_execution::Client,
        limiter: Arc<InferenceConcurrencyLimiter>,
    ) -> Self {
        Self {
            client,
            limiter,
            pending: Default::default(),
            active: Vec::new(),
            ready: Default::default(),
            closing: false,
        }
    }
    pub(super) fn enqueue(
        &mut self,
        mut request: Request,
        _word: &Arc<AtomicU64>,
    ) -> Result<Accepted, Box<ilium_execution::Rejected<Request>>> {
        use ilium_execution::{RejectReason, Rejected};
        let refused = |reason, value| Box::new(Rejected { reason, value });
        if self.closing {
            return Err(refused(RejectReason::Closed, request));
        }
        let bytes = request
            .retained_bytes()
            .saturating_add(std::mem::size_of::<(Captured, Option<Instant>)>());
        if bytes > MAX_CAPTURE_BYTES {
            return Err(refused(RejectReason::InvalidCost, request));
        }
        if request.source_hold.is_none() {
            match crate::execution::process_quota()
                .reserve_external_storage(MAX_CAPTURE_BYTES + MAX_RESULT_BYTES)
            {
                Ok(hold) => request.source_hold = Some(Arc::new(hold)),
                Err(reason) => return Err(refused(reason, request)),
            }
        }
        if self.pending.len() + self.active.len() + self.ready.len() >= MAX_OWNED_REQUESTS {
            return Err(refused(RejectReason::QueueFull, request));
        }
        let Some(source_hold) = request.source_hold.as_ref().map(Arc::clone) else {
            return Err(refused(RejectReason::InvalidCost, request));
        };
        let session_key = match &request.kind {
            Kind::SessionTitle(request) => {
                Some((request.input.pane_id, request.input.session_id.clone()))
            }
            _ => None,
        };
        let accepted = Accepted {
            session_key,
            source_hold: Arc::clone(&source_hold),
        };
        let captured = Captured {
            decision: request.decision,
            decision_word: Arc::clone(&request.decision_word),
            request,
            source_hold,
        };
        self.pending.push_back((captured, None));
        self.collect();
        Ok(accepted)
    }
    pub(super) fn collect(&mut self) {
        use ilium_execution::{JobCost, JobOutcome, JobPoll, Lane, RejectReason, SkipReason};
        let mut index = 0;
        while index < self.active.len() {
            match self.active[index].receipt.try_take() {
                JobPoll::Pending => {
                    index += 1;
                }
                JobPoll::Ready(outcome) => {
                    let active = self.active.remove(index);
                    let (outcome, hold) = outcome.into_parts();
                    match outcome {
                        JobOutcome::Finished(Ok(ControlFlow::Continue(prepared))) => {
                            self.ready.push_back(prepared)
                        }
                        JobOutcome::Finished(Ok(ControlFlow::Break((captured, error)))) => {
                            if !self.closing
                                && error.kind() == std::io::ErrorKind::WouldBlock
                                && !captured.revoked()
                            {
                                self.pending.push_back((
                                    captured,
                                    Some(Instant::now() + ADMISSION_RETRY_INTERVAL),
                                ));
                            } else {
                                let error = if error.kind() == std::io::ErrorKind::WouldBlock {
                                    std::io::Error::new(
                                        std::io::ErrorKind::Interrupted,
                                        "provider request cancelled before retry",
                                    )
                                } else {
                                    error
                                };
                                self.ready.push_back(captured.failed(error));
                            }
                        }
                        JobOutcome::Finished(Err(never)) => match never {},
                        JobOutcome::NotStarted { job, reason } => {
                            let message = match reason {
                                SkipReason::Cancelled => "provider job cancelled before execution",
                                SkipReason::Shutdown => {
                                    "provider execution shut down before job start"
                                }
                            };
                            self.ready
                                .push_back(job.captured.failed(std::io::Error::new(
                                    std::io::ErrorKind::Interrupted,
                                    message,
                                )));
                        }
                        JobOutcome::Panicked => self.ready.push_back(active.fallback),
                    }
                    // Independent result storage already follows Prepared.
                    // A cached model list must not keep this finite job slot.
                    drop(hold);
                }
                JobPoll::Lost | JobPoll::Taken => {
                    let active = self.active.remove(index);
                    self.ready.push_back(active.fallback);
                }
            }
        }
        let mut index = 0;
        while index < self.pending.len() {
            if !self.pending[index].0.revoked() {
                index += 1;
                continue;
            }
            let Some((captured, _)) = self.pending.remove(index) else {
                break;
            };
            self.ready.push_back(captured.failed(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "automatic AI decision revoked before retry",
            )));
        }
        let now = Instant::now(); // One eligibility instant prevents a refused original from cycling inside this collection pass.
        while !self.closing && self.active.len() < MAX_ACTIVE_REQUESTS {
            let Some(index) = self
                .pending
                .iter()
                .position(|(_, deadline)| deadline.is_none_or(|deadline| deadline <= now))
            else {
                break;
            };
            let Some((captured, _)) = self.pending.remove(index) else {
                break;
            };
            let source_hold = Arc::clone(&captured.source_hold);
            let fallback = Prepared {
                event: captured
                    .request
                    .kind
                    .failure_event(&captured.request.settings.settings, captured.decision),
                source_hold: Arc::clone(&source_hold),
            };
            let job = NamingJob {
                captured,
                limiter: Arc::clone(&self.limiter),
            };
            match self.client.try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: WORKING_BYTES,
                    result_bytes: MAX_RESULT_BYTES,
                },
                job,
            ) {
                Ok(receipt) => self.active.push(Active { receipt, fallback }),
                Err(rejected)
                    if matches!(
                        rejected.reason,
                        RejectReason::Busy
                            | RejectReason::QueueFull
                            | RejectReason::JobLimit
                            | RejectReason::InputBytes
                            | RejectReason::ResultBytes
                    ) =>
                {
                    self.pending.push_back((
                        rejected.value.captured,
                        Some(Instant::now() + ADMISSION_RETRY_INTERVAL),
                    ));
                }
                Err(rejected) => {
                    self.ready
                        .push_back(
                            rejected
                                .value
                                .captured
                                .failed(std::io::Error::other(format!(
                                    "provider execution admission failed: {:?}",
                                    rejected.reason
                                ))),
                        )
                }
            }
        }
    }
    pub(super) fn retry_delay(&self, now: Instant) -> Option<Duration> {
        if self.closing || self.active.len() >= MAX_ACTIVE_REQUESTS {
            return None;
        }
        self.pending
            .iter()
            .filter_map(|(_, deadline)| {
                deadline.map(|deadline| deadline.saturating_duration_since(now))
            })
            .min()
    }
    pub(super) fn take_ready(&mut self) -> Option<Prepared> {
        self.ready.pop_front()
    }
    pub(super) fn return_ready(&mut self, prepared: Prepared) {
        self.ready.push_front(prepared);
    }
    pub(super) fn close(&mut self) {
        self.closing = true;
        while let Some(captured) = self.pending.pop_front() {
            // Shutdown cancels only computations, never an authored save.
            drop(captured);
        }
        for active in &mut self.active {
            active.receipt.cancel();
        }
    }
}
impl Drop for FiniteWorkers {
    fn drop(&mut self) {
        self.close();
    }
}

impl std::fmt::Debug for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ProjectName(_) => "ProjectName",
            Self::SessionTitle(_) => "SessionTitle",
            Self::TerminalTitle(_, _) => "TerminalTitle",
            Self::InferenceTest => "InferenceTest",
            Self::Models(_) => "Models",
            Self::Restructure(_, _) => "Restructure",
            #[cfg(test)]
            Self::LastPrompt(_) => "LastPrompt",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refused_original_keeps_exact_settings_and_input_allocation() {
        let mut authored = InferenceSettings::default();
        authored.instructions.entry_naming = "original authored instruction".into();
        let snapshot = SettingsSnapshot::capture(&authored).unwrap();
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let _blocked = limiter.try_acquire().unwrap();
        let mut workers = FiniteWorkers::new(crate::execution::test_client(), limiter);
        let word = Arc::new(AtomicU64::new(AutomaticAiDecision::new(9, true).0));
        for _ in 0..MAX_OWNED_REQUESTS {
            workers
                .enqueue(
                    Request::new(Kind::InferenceTest, Arc::clone(&snapshot), &word),
                    &word,
                )
                .unwrap();
        }
        let input = Box::new(crate::terminal_naming::TerminalTitleInput {
            pane_id: NodeId(91),
            project_name: "original project".into(),
            project_path: PathBuf::from("/synthetic/project"),
            current_title: "original title".into(),
            screen_text: "original exact screen".into(),
            parent_group: "original group".into(),
            nearby_titles: Vec::new(),
        });
        let original_pointer = input.screen_text.as_ptr();
        let original = Request::new(
            Kind::TerminalTitle(input, TitleTrigger::Manual),
            Arc::clone(&snapshot),
            &word,
        );
        let refused = match workers.enqueue(original, &word) {
            Err(refused) => refused,
            Ok(_) => panic!("full queue accepted original"),
        };
        assert_eq!(refused.reason, ilium_execution::RejectReason::QueueFull);
        assert!(Arc::ptr_eq(&refused.value.settings, &snapshot));
        word.store(AutomaticAiDecision::new(10, false).0, Ordering::SeqCst);
        assert_eq!(refused.value.decision, AutomaticAiDecision::new(9, true));
        assert!(!refused.value.decision.is_current(&word));
        authored.instructions.entry_naming = "new unrelated setting".into();
        let newer = SettingsSnapshot::capture(&authored).unwrap();
        assert!(!Arc::ptr_eq(&newer, &refused.value.settings));
        assert_eq!(
            refused.value.settings.settings.instructions.entry_naming,
            "original authored instruction"
        );
        assert!(
            refused.value.source_hold.is_some(),
            "refusal must retain its already admitted physical source lease"
        );
        let Kind::TerminalTitle(input, trigger) = refused.value.kind else {
            panic!("original kind changed")
        };
        assert_eq!(input.screen_text.as_ptr(), original_pointer);
        assert_eq!(input.project_path, PathBuf::from("/synthetic/project"));
        assert_eq!(input.pane_id, NodeId(91));
        assert_eq!(trigger, TitleTrigger::Manual);
        let settle_by = Instant::now() + Duration::from_secs(5);
        while !workers.active.is_empty() {
            workers.collect();
            assert!(Instant::now() < settle_by, "preflight did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(workers.pending.len(), MAX_OWNED_REQUESTS);
        assert!(workers
            .pending
            .iter()
            .all(|(_, deadline)| deadline.is_some()));
        assert!(
            workers.retry_delay(Instant::now()).is_some(),
            "preflight refusal retains only a dated retry, not an IO waiter"
        );
    }

    fn await_one_dated_refusal(workers: &mut FiniteWorkers) {
        let settle_by = Instant::now() + Duration::from_secs(5);
        loop {
            workers.collect();
            if workers.active.is_empty()
                && workers.pending.len() == 1
                && workers.pending[0].1.is_some()
            {
                return;
            }
            assert!(
                Instant::now() < settle_by,
                "provider refusal did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn repeated_dated_refusals_retain_exact_original_settings_decision_and_source() {
        let mut settings = InferenceSettings::default();
        settings.instructions.entry_naming = "authored before refusal".into();
        let snapshot = SettingsSnapshot::capture(&settings).expect("snapshot");
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let held = limiter.try_acquire().expect("hold process slot");
        let mut workers = FiniteWorkers::new(crate::execution::test_client(), limiter);
        let decision = AutomaticAiDecision::new(31, true);
        let word = Arc::new(AtomicU64::new(decision.0));
        let input = Box::new(crate::terminal_naming::TerminalTitleInput {
            pane_id: NodeId(92),
            project_name: "authored project".into(),
            project_path: PathBuf::from("/synthetic/original"),
            current_title: "authored title".into(),
            screen_text: "exact authored screen".into(),
            parent_group: "authored group".into(),
            nearby_titles: Vec::new(),
        });
        let screen_pointer = input.screen_text.as_ptr();
        let accepted = workers
            .enqueue(
                Request::new(
                    Kind::TerminalTitle(input, TitleTrigger::Manual),
                    Arc::clone(&snapshot),
                    &word,
                ),
                &word,
            )
            .expect("admit original");
        for attempt in 0..2 {
            await_one_dated_refusal(&mut workers);
            let (captured, retry_at) = &workers.pending[0];
            assert_eq!(captured.decision, decision);
            assert!(Arc::ptr_eq(&captured.request.settings, &snapshot));
            assert!(Arc::ptr_eq(&captured.source_hold, &accepted.source_hold));
            assert_eq!(
                captured.request.settings.settings.instructions.entry_naming,
                "authored before refusal"
            );
            let Kind::TerminalTitle(input, TitleTrigger::Manual) = &captured.request.kind else {
                panic!("refusal changed request kind or trigger");
            };
            assert_eq!(input.screen_text.as_ptr(), screen_pointer);
            assert_eq!(input.project_path, PathBuf::from("/synthetic/original"));
            let deadline = retry_at.expect("dated retry");
            assert!(deadline > Instant::now(), "refusal needs a future deadline");
            workers.collect();
            assert!(
                workers.active.is_empty(),
                "early collection restarted provider work"
            );
            if attempt == 0 {
                std::thread::sleep(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
                );
                workers.collect();
            }
        }
        settings.instructions.entry_naming = "later unrelated setting".into();
        assert_eq!(
            snapshot.settings.instructions.entry_naming,
            "authored before refusal"
        );
        word.store(AutomaticAiDecision::new(32, false).0, Ordering::SeqCst);
        workers.collect();
        assert!(
            workers.pending.is_empty(),
            "revoked original must not retry"
        );
        let prepared = workers.take_ready().expect("terminal revocation result");
        assert!(Arc::ptr_eq(&prepared.source_hold, &accepted.source_hold));
        let NamingWorkerEvent::TerminalTitle(NodeId(92), result, TitleTrigger::Manual, actual) =
            prepared.event
        else {
            panic!("wrong terminal result");
        };
        assert_eq!(actual, decision);
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cancelled before provider call"));
        drop(held);
    }

    #[test]
    fn local_transcript_work_completes_behind_a_delayed_provider_original() {
        let home = tempfile::tempdir().expect("synthetic transcript home");
        let project_path = PathBuf::from("/work/p4-local");
        let session_id = "44444444-4444-4444-8444-444444444444";
        let transcript_dir = home.path().join(".claude/projects/-work-p4-local");
        std::fs::create_dir_all(&transcript_dir).expect("synthetic transcript directory");
        std::fs::write(
            transcript_dir.join(format!("{session_id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path.to_string_lossy().to_string(),
                "message": {"content": "local work completed"}
            })
            .to_string(),
        )
        .expect("synthetic transcript");
        let snapshot = SettingsSnapshot::capture(&InferenceSettings::default()).expect("snapshot");
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let _held = limiter.try_acquire().expect("hold process slot");
        let mut workers = FiniteWorkers::new(crate::execution::test_client(), limiter);
        let word = Arc::new(AtomicU64::new(AutomaticAiDecision::new(41, true).0));
        workers
            .enqueue(
                Request::new(Kind::InferenceTest, Arc::clone(&snapshot), &word),
                &word,
            )
            .expect("queue provider original");
        await_one_dated_refusal(&mut workers);
        let delayed_until = Instant::now() + Duration::from_secs(2);
        workers.pending[0].1 = Some(delayed_until);
        workers
            .enqueue(
                Request::new(
                    Kind::LastPrompt(Box::new(LastPromptTranscriptWorkerRequest {
                        home: home.path().to_path_buf(),
                        pane_id: NodeId(93),
                        project_path,
                        agent_class: AgentClass::Claude,
                        session_id: session_id.into(),
                        baseline_last_prompt: None,
                    })),
                    snapshot,
                    &word,
                ),
                &word,
            )
            .expect("queue independent local work");
        let settle_by = Instant::now() + Duration::from_secs(1);
        loop {
            workers.collect();
            if let Some(prepared) = workers.take_ready() {
                let NamingWorkerEvent::LastPromptTranscript(result) = prepared.event else {
                    panic!("provider original ran before its deadline");
                };
                assert_eq!(result.last_prompt.as_deref(), Some("local work completed"));
                break;
            }
            assert!(
                Instant::now() < settle_by,
                "local work blocked behind provider retry"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(workers.pending.len(), 1);
        assert_eq!(workers.pending[0].1, Some(delayed_until));
    }

    fn queued_local_job_is_terminal(shutdown_bank: bool) {
        use ilium_execution::{
            ClientLimits, Execution, ExecutionConfig, JobCost, JobPoll, Lane, LaneConfig,
            QuotaGroup, QuotaLimits, ShutdownMode,
        };
        let mib = 1024 * 1024;
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 4,
            service_jobs: 0,
            input_bytes: 512 * mib,
            result_bytes: 128 * mib,
            worker_threads: 1,
            worker_bytes: 16 * mib,
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
                cpu: disabled,
                io: LaneConfig {
                    threads: 1,
                    queue_slots: 2,
                    priority: None,
                    resident_bytes_per_thread: mib,
                },
                service: disabled,
            },
        )
        .expect("isolated one-thread IO bank");
        let client = execution
            .client(ClientLimits {
                jobs: 3,
                service_jobs: 0,
                input_bytes: 256 * mib,
                result_bytes: 64 * mib,
            })
            .expect("isolated client");
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut blocker = client
            .try_submit(
                Lane::Io,
                JobCost {
                    input_bytes: 1024,
                    result_bytes: 1024,
                },
                move |_| {
                    entered_tx.send(()).expect("blocker entered");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("bounded blocker release");
                    Ok::<(), std::convert::Infallible>(())
                },
            )
            .expect("occupy sole IO worker");
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("blocker running");
        let limiter = Arc::new(InferenceConcurrencyLimiter::new(1));
        let mut workers = FiniteWorkers::new(client, limiter);
        let snapshot = SettingsSnapshot::capture(&InferenceSettings::default()).expect("snapshot");
        let word = Arc::new(AtomicU64::new(AutomaticAiDecision::new(51, true).0));
        let home = tempfile::tempdir().expect("unused local transcript home");
        workers
            .enqueue(
                Request::new(
                    Kind::LastPrompt(Box::new(LastPromptTranscriptWorkerRequest {
                        home: home.path().to_path_buf(),
                        pane_id: NodeId(94),
                        project_path: PathBuf::from("/synthetic/queued"),
                        agent_class: AgentClass::Claude,
                        session_id: "queued-local-probe".into(),
                        baseline_last_prompt: None,
                    })),
                    snapshot,
                    &word,
                ),
                &word,
            )
            .expect("queue local job behind running blocker");
        assert_eq!(workers.active.len(), 1);
        if shutdown_bank {
            execution.request_shutdown(ShutdownMode::Cancel);
        } else {
            workers.close();
        }
        release_tx.send(()).expect("release running blocker");
        let settle_by = Instant::now() + Duration::from_secs(5);
        loop {
            workers.collect();
            if let Some(prepared) = workers.take_ready() {
                assert!(matches!(
                    prepared.event,
                    NamingWorkerEvent::LastPromptTranscript(_)
                ));
                break;
            }
            assert!(
                Instant::now() < settle_by,
                "queued cancellation did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(workers.pending.is_empty(), "NotStarted work was requeued");
        assert!(workers.active.is_empty());
        loop {
            match blocker.try_take() {
                JobPoll::Pending => {
                    assert!(Instant::now() < settle_by, "blocker did not retire");
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobPoll::Ready(_) => break,
                JobPoll::Lost | JobPoll::Taken => panic!("blocker outcome lost"),
            }
        }
        execution.request_shutdown(ShutdownMode::Cancel);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("join isolated bank");
        assert_eq!(report.remaining_workers, 0);
    }

    #[test]
    fn queued_cancelled_work_is_terminal() {
        queued_local_job_is_terminal(false);
    }

    #[test]
    fn queued_shutdown_work_is_terminal() {
        queued_local_job_is_terminal(true);
    }
    #[test]
    fn spare_settings_capacity_is_rejected_before_clone() {
        let mut settings = InferenceSettings::default();
        settings.instructions.entry_naming = String::with_capacity(MAX_CAPTURE_BYTES + 1);
        assert!(matches!(
            SettingsSnapshot::capture(&settings),
            Err(ilium_execution::RejectReason::InvalidCost)
        ));
    }
}
