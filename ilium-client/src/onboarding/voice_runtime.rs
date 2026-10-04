//! Explicit onboarding voice ownership. Event preparation runs on the shared
//! CPU bank; interaction methods never wait for commands or actor/native exit.
use super::voice_demo::{self, VoiceDemo};
use crate::{config::VoiceSettings, voice_retirement::VoiceRetirement};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, QuotaGroup, Receipt, RejectReason,
    StorageAdmission,
};
use ilium_voice::{
    OwnedVoiceText, VoiceActorExit, VoiceCommand, VoiceConnectionState, VoiceEvent,
    VoiceEventReceipt, VoiceInputMode, VoiceService, VoiceShutdownOutcome, VoiceShutdownState,
    VoiceTextAllocation, VoiceToolDefinition, VoiceToolOutput,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::Notify;
const MAX_TRANSCRIPT_CHARS: usize = 2048;
const MAX_TEXT_CHARS: usize = 1024;
const MAX_COMMANDS: usize = 32;
const STATE_BYTES: usize = 64 * 1024;
const CACHE_BYTES: usize = 1024 * 1024;
const PREPARATION_COST: JobCost = JobCost {
    input_bytes: 4096,
    result_bytes: 4096,
};

#[derive(Debug, Clone)]
pub struct VoiceDemoState {
    pub connection: Arc<VoiceConnectionState>,
    pub bulb_on: bool,
    pub acknowledged_calls: u64,
    pub last_tool_status: Option<Arc<str>>,
    pub user_transcript: Arc<str>,
    pub assistant_transcript: Arc<str>,
    pub is_running: bool,
    /// Stop remains available for a queued Test or native retirement.
    pub can_stop: bool,
    // A UI clone shares the original heap and its allocation declaration.
    pub(crate) allocation_hold: Option<Arc<StorageAdmission>>,
    pub(crate) connection_hold: Option<Arc<StorageAdmission>>,
}
impl Default for VoiceDemoState {
    fn default() -> Self {
        Self {
            connection: Arc::new(VoiceConnectionState::Disabled),
            bulb_on: false,
            acknowledged_calls: 0,
            last_tool_status: None,
            user_transcript: Arc::from(""),
            assistant_transcript: Arc::from(""),
            is_running: false,
            can_stop: false,
            allocation_hold: None,
            connection_hold: None,
        }
    }
}
struct DemoEngine {
    demo: Mutex<VoiceDemo>,
    _allocation: Arc<StorageAdmission>,
}
struct SourceEvent {
    event: VoiceEvent,
    _metadata: Arc<StorageAdmission>,
}
struct PrepareEvent {
    source: Arc<SourceEvent>,
    state: VoiceDemoState,
    key: Arc<str>,
    engine: Arc<DemoEngine>,
    state_allocation: Arc<StorageAdmission>,
    quota: QuotaGroup,
    _key_allocation: Option<Arc<StorageAdmission>>,
}
struct PreparedEvent {
    state: VoiceDemoState,
    command: Option<VoiceCommand>,
}
enum PreparationError {
    Admission,
    Failed(String),
}
impl Job for PrepareEvent {
    type Output = PreparedEvent;
    type Error = PreparationError;
    fn run(self, context: JobContext) -> Result<PreparedEvent, PreparationError> {
        if context.stop_requested() {
            return Err(PreparationError::Failed(
                "Voice event preparation cancelled before execution".into(),
            ));
        }
        let output_bytes = match &self.source.event {
            VoiceEvent::ToolInvocations(calls) => {
                calls.iter().try_fold(4096usize, |total, call| {
                    total
                        .checked_add(call.call_id.capacity())
                        .and_then(|n| n.checked_add(2048))
                })
            }
            _ => Some(4096),
        }
        .ok_or_else(|| PreparationError::Failed("Demo output size overflow".into()))?;
        if output_bytes > self.quota.snapshot().limits.worker_bytes {
            return Err(PreparationError::Failed(
                "Demo output exceeds the process allocation limit; original event cancelled".into(),
            ));
        }
        let output_allocation = Arc::new(
            self.quota
                .reserve_external_storage(output_bytes)
                .map_err(|reason| {
                    if matches!(
                        reason,
                        RejectReason::Closed
                            | RejectReason::InvalidCost
                            | RejectReason::AccountingPoisoned
                    ) {
                        PreparationError::Failed(format!(
                            "Demo output admission failed: {reason:?}"
                        ))
                    } else {
                        PreparationError::Admission
                    }
                })?,
        );
        let mut state = self.state;
        // Shared old strings must retain their prior lease. Copy only the bounded
        // visible projection on this CPU owner into the new declared heap.
        state.user_transcript = Arc::from(state.user_transcript.as_ref());
        state.assistant_transcript = Arc::from(state.assistant_transcript.as_ref());
        state.last_tool_status = state.last_tool_status.as_deref().map(Arc::from);
        state.connection = Arc::new(state.connection.as_ref().clone());
        state.allocation_hold = Some(self.state_allocation);
        state.connection_hold = None;
        let redact = |text: &str| redacted_prefix(text, &self.key, MAX_TRANSCRIPT_CHARS);
        let mut command = None;
        match &self.source.event {
            VoiceEvent::StateChanged(connection) => {
                if matches!(connection, VoiceConnectionState::Thinking) {
                    state.assistant_transcript = Arc::from("");
                }
                state.connection = Arc::new(match connection {
                    VoiceConnectionState::Failed(error) => {
                        VoiceConnectionState::Failed(redact(error))
                    }
                    other => other.clone(),
                });
            }
            VoiceEvent::UserTranscript(text) => state.user_transcript = Arc::from(redact(text)),
            VoiceEvent::AssistantTranscript(delta) => {
                let prefix = redacted_prefix(delta, &self.key, MAX_TRANSCRIPT_CHARS);
                let text = state
                    .assistant_transcript
                    .chars()
                    .chain(prefix.chars())
                    .take(MAX_TRANSCRIPT_CHARS)
                    .collect::<String>();
                state.assistant_transcript = Arc::from(text);
            }
            VoiceEvent::ProviderError(error) => {
                state.connection = Arc::new(VoiceConnectionState::Failed(redact(error)))
            }
            VoiceEvent::ToolInvocations(invocations) => {
                // This mutex is accessed only by the one ordered CPU job. The
                // UI never locks the engine or runs JSON/tool transformations.
                let mut demo = self.engine.demo.lock().map_err(|_| {
                    PreparationError::Failed(
                        "Demo executor panicked; state is not replayable".to_owned(),
                    )
                })?;
                let mut outputs = Vec::with_capacity(invocations.len());
                for invocation in invocations {
                    let receipt = demo.execute(
                        &invocation.call_id,
                        &invocation.name,
                        &invocation.arguments_json,
                    );
                    state.bulb_on = demo.on;
                    if receipt.ok {
                        state.acknowledged_calls = state.acknowledged_calls.saturating_add(1);
                        state.last_tool_status = Some(Arc::from(if demo.on {
                            "Tool executed · light on"
                        } else {
                            "Tool executed · light off"
                        }));
                    } else {
                        state.last_tool_status = receipt.error.as_deref().map(Arc::from);
                    }
                    outputs.push(VoiceToolOutput { call_id:invocation.call_id.clone(),
                        result:Arc::new(serde_json::json!({"ok":receipt.ok,"on":receipt.on,"error":receipt.error})),
                        request_follow_up:true, terminate_session_after_delivery:false,
                        allocation_hold:Some(output_allocation.clone()), retained_bytes:2048 + invocation.call_id.capacity() });
                }
                if !outputs.is_empty() {
                    command = Some(VoiceCommand::SubmitToolOutputs(outputs));
                }
            }
        }
        Ok(PreparedEvent { state, command })
    }
}
struct QueuedCommand {
    command: Option<VoiceCommand>,
    ready: bool,
    _allocation: Arc<StorageAdmission>,
}
/// Exact canceled originals, including their metadata custody; never replayed
/// into a replacement conversation.
pub struct CancelledDemoCommands {
    commands: VecDeque<QueuedCommand>,
    _metadata: Option<Arc<StorageAdmission>>,
}
impl CancelledDemoCommands {
    pub fn count(&self) -> usize {
        self.commands
            .iter()
            .filter(|entry| entry.command.is_some())
            .count()
    }
    /// Explicit cancellation disposition. Payload destructors run before the
    /// entry's allocation guard, so a raw uncharged command never escapes.
    pub fn dispose(mut self) -> usize {
        let count = self.count();
        while let Some(mut entry) = self.commands.pop_front() {
            drop(entry.command.take());
            drop(entry);
        }
        count
    }
}

#[derive(Debug)]
struct TextAdmission {
    _allocation: Arc<StorageAdmission>,
}
impl VoiceTextAllocation for TextAdmission {}
/// One actor receipt batch. None means the same actor still needs draining;
/// every command/output is an exact original, not a retry into a new session.
pub struct DemoShutdownOutcome {
    pub actor_exit: Option<VoiceActorExit>,
    pub undelivered_commands: VecDeque<VoiceCommand>,
    pub undelivered_stop_outputs: Option<Vec<VoiceToolOutput>>,
    _metadata: VoiceShutdownOutcome,
}
struct PendingTest {
    settings: VoiceSettings,
    _allocation: Arc<StorageAdmission>,
}

pub struct VoiceDemoRuntime {
    pub state: VoiceDemoState,
    client: Client,
    notification: Arc<Notify>,
    service: Option<VoiceService>,
    started_settings: Option<PendingTest>,
    pending_test: Option<PendingTest>,
    startup: crate::voice_preparation::VoicePreparation,
    preparing_test: Option<PendingTest>,
    prepared_startup: Option<crate::voice_preparation::PreparedVoice>,
    startup_retry_at: Option<Instant>,
    redaction_key: Arc<str>,
    key_allocation: Option<Arc<StorageAdmission>>,
    engine: Option<Arc<DemoEngine>>,
    source: Option<Arc<SourceEvent>>,
    preparation: Option<Receipt<PrepareEvent>>,
    commands: VecDeque<QueuedCommand>,
    cancelled: Option<CancelledDemoCommands>,
    cancelled_event: Option<Arc<SourceEvent>>,
    stopping: bool,
    event_channel_closed: bool,
    external_owner_pending: bool,
    terminal_native_failure: bool,
    shutdown_batch: Option<VoiceShutdownOutcome>,
    completed_shutdown: Option<DemoShutdownOutcome>,
    retirement: VoiceRetirement,
    runtime_metadata: Option<Arc<StorageAdmission>>,
}
pub fn tool_definitions() -> Vec<VoiceToolDefinition> {
    vec![VoiceToolDefinition {
        name: voice_demo::TOOL_NAME.into(),
        description: "Set the onboarding demonstration lightbulb on or off. This controls only the simulated bulb.".into(),
        parameters: voice_demo::tool_schema(),
    }]
}

#[cfg(test)]
fn config_with_instructions(
    settings: &VoiceSettings,
    instructions: String,
) -> ilium_voice::VoiceRuntimeConfig {
    let api_key = settings.api_key.clone();
    ilium_voice::VoiceRuntimeConfig {
        api_key: api_key.into(),
        model: settings.model,
        voice: settings.voice,
        reasoning_effort: settings.reasoning_effort,
        input_mode: settings.input_mode,
        vad_eagerness: settings.vad_eagerness,
        input_device_name: settings.input_device_name.clone(),
        output_device_name: settings.output_device_name.clone(),
        output_volume_percent: settings.output_volume_percent,
        instructions,
    }
}

impl VoiceDemoRuntime {
    pub fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        let client = client.with_completion_wake(move || wake.notify_one());
        Self {
            state: VoiceDemoState::default(),
            retirement: VoiceRetirement::new(client.clone()),
            startup: crate::voice_preparation::VoicePreparation::new_with_notification(
                client.clone(),
                notification.clone(),
            ),
            client,
            notification,
            service: None,
            started_settings: None,
            pending_test: None,
            preparing_test: None,
            prepared_startup: None,
            startup_retry_at: None,
            redaction_key: Arc::from(""),
            key_allocation: None,
            engine: None,
            source: None,
            preparation: None,
            commands: VecDeque::new(),
            cancelled: None,
            cancelled_event: None,
            stopping: false,
            event_channel_closed: false,
            external_owner_pending: false,
            terminal_native_failure: false,
            shutdown_batch: None,
            completed_shutdown: None,
            runtime_metadata: None,
        }
    }
    /// The parent supplies the NORMAL role's actual actor/native custody,
    /// including retirement. This flag never cancels or replaces a demo intent.
    pub fn set_external_owner_pending(&mut self, pending: bool) {
        if self.external_owner_pending && !pending {
            self.notification.notify_one();
        }
        self.external_owner_pending = pending;
        self.refresh_control_state();
    }
    pub fn native_retirement_failed(&self) -> bool {
        self.terminal_native_failure
    }
    fn refresh_control_state(&mut self) {
        self.state.is_running = self
            .service
            .as_ref()
            .is_some_and(|service| !service.actor_is_finished());
        self.state.can_stop = self.service.is_some()
            || self.stopping
            || self.pending_test.is_some()
            || self.preparing_test.is_some()
            || self.startup.is_pending()
            || self.retirement.is_pending();
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub fn startup_retry_delay(&self, now: Instant) -> Option<std::time::Duration> {
        if self.external_owner_pending || self.stopping {
            return None;
        }
        self.startup
            .retry_delay(now)
            .into_iter()
            .chain(
                self.startup_retry_at
                    .filter(|_| self.pending_test.is_some() || self.prepared_startup.is_some())
                    .map(|deadline| deadline.saturating_duration_since(now)),
            )
            .min()
    }
    pub fn actor_notification(&self) -> Option<Arc<Notify>> {
        self.service
            .as_ref()
            .map(VoiceService::completion_notification)
    }
    pub fn pending_work(&self) -> bool {
        self.service.is_some()
            || self.stopping
            || self.retirement.is_pending()
            || self.preparation.is_some()
            || self.source.is_some()
            || !self.commands.is_empty()
            || self.shutdown_batch.is_some()
            || self.pending_test.is_some()
            || self.preparing_test.is_some()
            || self.prepared_startup.is_some()
            || self.startup.is_pending()
            || self.completed_shutdown.is_some()
            || self.cancelled.is_some()
            || self.cancelled_event.is_some()
    }
    pub fn take_cancelled_event(&mut self) -> Option<(VoiceEvent, Arc<StorageAdmission>)> {
        let source = self.cancelled_event.take()?;
        match Arc::try_unwrap(source) {
            Ok(source) => Some((source.event, source._metadata)),
            Err(source) => {
                self.cancelled_event = Some(source);
                None
            }
        }
    }
    pub fn take_shutdown_outcome(&mut self) -> Option<DemoShutdownOutcome> {
        self.completed_shutdown.take()
    }
    pub fn take_cancelled_commands(&mut self) -> Option<CancelledDemoCommands> {
        self.cancelled.take()
    }
    fn storage(&self, bytes: usize) -> Result<Arc<StorageAdmission>, String> {
        self.client
            .quota_group()
            .reserve_external_storage(bytes)
            .map(Arc::new)
            .map_err(|reason| format!("Voice demo allocation admission: {reason:?}"))
    }
    fn capture_test(&self, settings: &VoiceSettings) -> Result<PendingTest, String> {
        let bytes = settings
            .api_key
            .capacity()
            .checked_mul(4)
            .and_then(|n| n.checked_add(settings.custom_prompt.capacity()))
            .and_then(|n| {
                n.checked_add(
                    settings
                        .input_device_name
                        .as_ref()
                        .map_or(0, String::capacity)
                        * 2,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    settings
                        .output_device_name
                        .as_ref()
                        .map_or(0, String::capacity)
                        * 2,
                )
            })
            .and_then(|n| n.checked_add(STATE_BYTES))
            .ok_or("Voice settings size overflow")?;
        let allocation = self.storage(bytes)?;
        Ok(PendingTest {
            settings: settings.clone(),
            _allocation: allocation,
        })
    }
    /// A Test pressed during retirement is one explicit intent. It starts only
    /// once old actor, commands, event preparation and native owners settle.
    pub async fn start_test(&mut self, settings: &VoiceSettings) -> Result<(), String> {
        if self.pending_test.is_some()
            || self.preparing_test.is_some()
            || self.prepared_startup.is_some()
        {
            return Err("A voice Test is already waiting for cleanup".into());
        }
        if self.runtime_metadata.is_none() {
            self.runtime_metadata = Some(self.storage(16 * 1024)?);
        }
        let pending = self.capture_test(settings)?;
        if self.external_owner_pending || self.service.is_some() || self.pending_work() {
            self.pending_test = Some(pending);
            self.request_stop();
            return Ok(());
        }
        match self.start_captured(pending) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.fail(&error);
                Err(error)
            }
        }
    }
    fn start_captured(&mut self, pending: PendingTest) -> Result<(), String> {
        if self.external_owner_pending {
            self.pending_test = Some(pending);
            self.refresh_control_state();
            return Ok(());
        }
        use crate::voice_preparation::{PreparationKind, PrepareRefusal};
        match self
            .startup
            .request(&pending.settings, PreparationKind::DemoStartup)
        {
            Ok(()) => {
                self.preparing_test = Some(pending);
                self.startup_retry_at = None;
                self.state.connection = Arc::new(VoiceConnectionState::Connecting);
                self.refresh_control_state();
                Ok(())
            }
            Err(PrepareRefusal::Admission(reason))
                if matches!(
                    reason,
                    RejectReason::Busy
                        | RejectReason::WorkerBytes
                        | RejectReason::QueueFull
                        | RejectReason::JobLimit
                        | RejectReason::InputBytes
                        | RejectReason::ResultBytes
                ) =>
            {
                self.pending_test = Some(pending);
                self.startup_retry_at = Some(Instant::now() + std::time::Duration::from_millis(20));
                self.refresh_control_state();
                Ok(())
            }
            Err(error) => Err(format!("Voice demo preparation refused: {error:?}")),
        }
    }
    fn collect_startup(&mut self, now: Instant) {
        if let Some(prepared) = self.startup.collect() {
            self.prepared_startup = Some(prepared);
        }
        if self.external_owner_pending
            || self.stopping
            || self.service.is_some()
            || self.retirement.is_pending()
            || self.preparation.is_some()
            || self.source.is_some()
            || self.shutdown_batch.is_some()
            || self.completed_shutdown.is_some()
            || self.cancelled.is_some()
            || self.cancelled_event.is_some()
            || self.startup_retry_at.is_some_and(|deadline| now < deadline)
        {
            return;
        }
        if self.prepared_startup.is_none() {
            return;
        }
        // A preparation error needs no engine. Publish it using the metadata
        // already admitted with Test, even when further storage is unavailable.
        if self
            .prepared_startup
            .as_ref()
            .is_some_and(|ready| ready.value.is_err())
        {
            if let Some(prepared) = self.prepared_startup.take() {
                if let Err(error) = prepared.value {
                    self.fail(&error);
                }
            }
            self.preparing_test = None;
            self.startup_retry_at = None;
            self.refresh_control_state();
            return;
        }
        // Reserve presentation owners before consuming the prepared original.
        // Busy admission keeps both original settings and startup proof intact.
        let allocations = self.client.quota_group();
        let reserve = || -> Result<_, RejectReason> {
            let engine = Arc::new(allocations.reserve_external_storage(CACHE_BYTES)?);
            let state = Arc::new(allocations.reserve_external_storage(STATE_BYTES)?);
            let actor = VoiceService::admit_startup(allocations.clone())?;
            Ok((engine, state, actor))
        };
        let (allocation, state_allocation, actor_admission) = match reserve() {
            Ok(value) => value,
            Err(RejectReason::Busy | RejectReason::WorkerBytes) => {
                self.startup_retry_at = Some(now + std::time::Duration::from_millis(20));
                return;
            }
            Err(reason) => {
                self.prepared_startup = None;
                self.preparing_test = None;
                self.startup_retry_at = None;
                self.fail(&format!("Voice demo startup storage failed: {reason:?}"));
                return;
            }
        };
        self.startup_retry_at = None;
        let Some(prepared) = self.prepared_startup.take() else {
            return;
        };
        let Some(pending) = self.preparing_test.take() else {
            return;
        };
        if !matches!(
            prepared.kind(),
            crate::voice_preparation::PreparationKind::DemoStartup
        ) || !prepared
            .settings()
            .has_same_runtime_configuration(&pending.settings)
        {
            self.fail("Voice demo preparation returned an obsolete startup");
            return;
        }
        let key_allocation = prepared.allocation();
        let startup = match prepared.value {
            Ok(crate::voice_preparation::PreparedValue::Startup(startup)) => startup,
            Ok(crate::voice_preparation::PreparedValue::Context(_)) => {
                self.fail("Voice demo preparation returned an unexpected context");
                return;
            }
            Err(error) => {
                self.fail(&error);
                return;
            }
        };
        use ilium_voice::ExposeSecret;
        let redaction_key: Arc<str> = Arc::from(startup.config().api_key.expose_secret());
        let service = VoiceService::start(startup, actor_admission);
        self.redaction_key = redaction_key;
        self.key_allocation = Some(key_allocation);
        self.engine = Some(Arc::new(DemoEngine {
            demo: Mutex::new(VoiceDemo::default()),
            _allocation: allocation,
        }));
        self.state = VoiceDemoState {
            connection: Arc::new(VoiceConnectionState::Connecting),
            is_running: true,
            allocation_hold: Some(state_allocation),
            ..VoiceDemoState::default()
        };
        self.started_settings = Some(pending);
        self.service = Some(service);
        self.stopping = false;
        self.event_channel_closed = false;
        self.refresh_control_state();
    }
    pub async fn reconcile(&mut self, settings: &VoiceSettings, is_voice_step: bool) {
        if !is_voice_step {
            self.pending_test = None;
            self.request_stop();
            return;
        }
        if self
            .pending_test
            .as_ref()
            .is_some_and(|pending| !pending.settings.has_same_runtime_configuration(settings))
            || self
                .preparing_test
                .as_ref()
                .is_some_and(|pending| !pending.settings.has_same_runtime_configuration(settings))
        {
            self.pending_test = None;
            self.request_stop();
            self.state.last_tool_status =
                Some(Arc::from("Settings changed. Select Test to use them."));
        }
        if self
            .started_settings
            .as_ref()
            .is_some_and(|old| !old.settings.has_same_runtime_configuration(settings))
        {
            if self
                .pending_test
                .as_ref()
                .is_some_and(|pending| !pending.settings.has_same_runtime_configuration(settings))
            {
                self.pending_test = None;
            }
            self.request_stop();
            self.state.last_tool_status =
                Some(Arc::from("Settings changed. Select Test to use them."));
        }
    }
    pub async fn next_event(&mut self) -> Option<VoiceEventReceipt> {
        if self.event_channel_closed
            || self.source.is_some()
            || self.preparation.is_some()
            || self.shutdown_batch.is_some()
        {
            return std::future::pending().await;
        }
        match &mut self.service {
            Some(service) => service.next_event().await,
            None => std::future::pending().await,
        }
    }
    pub async fn handle_event(
        &mut self,
        receipt: VoiceEventReceipt,
    ) -> Result<(), VoiceEventReceipt> {
        // Caller receives only when the previous source has settled. Admission
        // refusal preserves the original receipt here and backpressures actor.
        if self.source.is_some() {
            return Err(receipt);
        }
        let (event, metadata) = receipt.into_parts();
        self.source = Some(Arc::new(SourceEvent {
            event,
            _metadata: metadata,
        }));
        self.try_prepare();
        Ok(())
    }
    fn try_prepare(&mut self) {
        if self.preparation.is_some() {
            return;
        }
        let (Some(source), Some(engine)) = (&self.source, &self.engine) else {
            return;
        };
        if self.commands.len() >= MAX_COMMANDS {
            return;
        }
        let Ok(state_allocation) = self.storage(STATE_BYTES) else {
            return;
        };
        let Ok(placeholder_allocation) = self.storage(4096) else {
            return;
        };
        let job = PrepareEvent {
            source: source.clone(),
            state: self.state.clone(),
            key: self.redaction_key.clone(),
            engine: engine.clone(),
            state_allocation,
            quota: self.client.quota_group(),
            _key_allocation: self.key_allocation.clone(),
        };
        match self.client.try_submit(Lane::Cpu, PREPARATION_COST, job) {
            Ok(receipt) => {
                self.preparation = Some(receipt);
                // Every source gets an ordered placeholder; harmless empty
                // projections consume it, tools replace it with their output.
                self.commands.push_back(QueuedCommand {
                    command: None,
                    ready: false,
                    _allocation: placeholder_allocation,
                });
            }
            Err(rejected) => {
                if !self.client.is_open() {
                    self.fail("Voice preparation bank closed; original event cancelled");
                    self.cancel_failed_preparation();
                }
                drop(rejected); // original source remains owned for ordinary retry
            }
        }
    }
    fn collect_preparation(&mut self) {
        let Some(receipt) = &mut self.preparation else {
            return;
        };
        let outcome = match receipt.try_take() {
            JobPoll::Pending => return,
            JobPoll::Ready(value) => Some(value),
            JobPoll::Lost | JobPoll::Taken => None,
        };
        self.preparation = None;
        let Some(outcome) = outcome else {
            self.fail("Voice preparation lost its receipt; original event retained");
            self.cancel_failed_preparation();
            return;
        };
        let (outcome, _hold) = outcome.into_parts();
        match outcome {
            JobOutcome::Finished(Ok(prepared)) => {
                self.state = prepared.state;
                if self.stopping {
                    self.state.is_running = false;
                }
                if let Some(entry) = self
                    .commands
                    .iter_mut()
                    .find(|entry| entry.command.is_none())
                {
                    entry.command = prepared.command;
                    entry.ready = true;
                }
                // Remove the single no-command placeholder after completion.
                if self
                    .commands
                    .front()
                    .is_some_and(|entry| entry.ready && entry.command.is_none())
                {
                    self.commands.pop_front();
                }
                self.source = None;
                if matches!(
                    self.state.connection.as_ref(),
                    VoiceConnectionState::Failed(_)
                ) {
                    self.request_stop();
                }
            }
            JobOutcome::NotStarted { .. } => {
                self.commands.retain(|entry| entry.command.is_some());
            }
            JobOutcome::Finished(Err(PreparationError::Admission)) => {
                self.commands.retain(|entry| entry.ready);
            }
            JobOutcome::Finished(Err(PreparationError::Failed(error))) => {
                self.fail(&error);
                self.cancel_failed_preparation();
            }
            JobOutcome::Panicked => {
                self.fail("Voice preparation panicked; semantic execution is uncertain");
                self.cancel_failed_preparation();
            }
        }
    }
    fn cancel_failed_preparation(&mut self) {
        self.cancelled_event = self.source.take();
        self.commands.retain(|entry| entry.ready);
        self.request_stop();
    }
    fn publish_commands(&mut self) {
        if self.stopping {
            return;
        }
        let Some(service) = &self.service else {
            return;
        };
        let sender = service.command_sender();
        while self.commands.front().is_some_and(|entry| entry.ready) {
            if self
                .commands
                .front()
                .is_some_and(|entry| entry.command.is_none())
            {
                self.commands.pop_front();
                continue;
            }
            let Ok(permit) = sender.try_reserve() else {
                break;
            };
            if let Some(mut entry) = self.commands.pop_front() {
                if let Some(command) = entry.command.take() {
                    permit.send(command);
                }
            }
        }
    }
    pub async fn collect(&mut self, now: Instant) {
        self.collect_startup(now);
        self.collect_preparation();
        self.publish_commands();
        self.retirement.collect(now);
        self.terminal_native_failure |= self.retirement.has_terminal_failure();
        if let Some(error) = self.retirement.take_failure() {
            self.fail(&error);
        }
        if self.source.is_none() && self.preparation.is_none() && self.completed_shutdown.is_none()
        {
            if let Some(batch) = &mut self.shutdown_batch {
                if let Some(event) = batch.events.pop_front() {
                    if let Err(original) = self.handle_event(event).await {
                        if let Some(batch) = &mut self.shutdown_batch {
                            batch.events.push_front(original);
                        }
                    }
                } else {
                    let Some(mut batch) = self.shutdown_batch.take() else {
                        return;
                    };
                    let actor_exit = match std::mem::replace(
                        &mut batch.state,
                        VoiceShutdownState::Complete(VoiceActorExit::Completed),
                    ) {
                        VoiceShutdownState::Pending(service) => {
                            self.service = Some(*service);
                            None
                        }
                        VoiceShutdownState::Complete(exit) => {
                            if let Err(original) =
                                self.retirement.retain(batch.audio_custody.clone())
                            {
                                drop(original);
                                batch.state = VoiceShutdownState::Complete(exit);
                                self.shutdown_batch = Some(batch);
                                return;
                            }
                            self.engine = None;
                            self.started_settings = None;
                            Some(exit)
                        }
                    };
                    self.completed_shutdown = Some(DemoShutdownOutcome {
                        actor_exit,
                        undelivered_commands: std::mem::take(&mut batch.undelivered_commands),
                        undelivered_stop_outputs: batch.undelivered_stop_outputs.take(),
                        _metadata: batch,
                    });
                }
            }
        }
        if self.stopping
            && self.preparation.is_none()
            && self.cancelled.is_none()
            && !self.commands.is_empty()
        {
            self.cancelled = Some(CancelledDemoCommands {
                commands: std::mem::take(&mut self.commands),
                _metadata: self.runtime_metadata.clone(),
            });
        }
        if self.stopping
            && self.shutdown_batch.is_none()
            && self
                .service
                .as_ref()
                .is_some_and(VoiceService::actor_is_finished)
        {
            if let Some(service) = self.service.take() {
                self.shutdown_batch = Some(service.finish_actor_if_ready().await);
            }
        }
        if self.stopping
            && self.service.is_none()
            && self.shutdown_batch.is_none()
            && self.preparation.is_none()
            && self.source.is_none()
        {
            if !self.commands.is_empty() && self.cancelled.is_none() {
                self.cancelled = Some(CancelledDemoCommands {
                    commands: std::mem::take(&mut self.commands),
                    _metadata: self.runtime_metadata.clone(),
                });
            }
            if !self.retirement.is_pending()
                && !self.startup.is_pending()
                && self.completed_shutdown.is_none()
                && self.cancelled.is_none()
                && self.cancelled_event.is_none()
            {
                self.stopping = false;
            }
        }
        if !self.external_owner_pending
            && !self.stopping
            && !self.pending_work_except_test()
            && self.startup_retry_at.is_none_or(|deadline| now >= deadline)
        {
            if let Some(pending) = self.pending_test.take() {
                if let Err(error) = self.start_captured(pending) {
                    self.fail(&error);
                }
            }
        }
        self.try_prepare();
        self.collect_startup(now);
        self.refresh_control_state();
    }
    fn pending_work_except_test(&self) -> bool {
        self.service.is_some()
            || self.startup.is_pending()
            || self.preparing_test.is_some()
            || self.prepared_startup.is_some()
            || self.retirement.is_pending()
            || self.preparation.is_some()
            || self.source.is_some()
            || self.completed_shutdown.is_some()
            || self.cancelled.is_some()
            || self.cancelled_event.is_some()
            || self.shutdown_batch.is_some()
    }
    pub async fn channel_closed(&mut self) {
        self.event_channel_closed = true;
        if !self.stopping {
            self.fail("The voice test stopped unexpectedly. Select Test to retry.");
        }
        self.request_stop();
    }
    pub async fn push_to_talk(&mut self, is_pressed: bool) -> Result<(), String> {
        if self
            .started_settings
            .as_ref()
            .is_none_or(|settings| settings.settings.input_mode != VoiceInputMode::PushToTalk)
        {
            return Err("Choose Push to talk before recording a manual turn".into());
        }
        self.queue_command(
            if is_pressed {
                VoiceCommand::StartPushToTalk
            } else {
                VoiceCommand::StopPushToTalk
            },
            None,
        )
    }
    pub async fn send_text(&mut self, text: &str) -> Result<(), String> {
        if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return Err("Enter 1–1024 characters for the voice test".into());
        }
        if self.commands.len() >= MAX_COMMANDS || self.stopping || self.service.is_none() {
            return Err("Voice command queue unavailable; text was not accepted".into());
        }
        let allocation = self.storage(text.len() + 4096)?;
        let text = OwnedVoiceText::charged(
            text.to_owned(),
            Arc::new(TextAdmission {
                _allocation: allocation.clone(),
            }),
        );
        self.queue_command(VoiceCommand::SendText(text), Some(allocation))
    }
    fn queue_command(
        &mut self,
        command: VoiceCommand,
        allocation: Option<Arc<StorageAdmission>>,
    ) -> Result<(), String> {
        if self.stopping || self.service.is_none() {
            return Err("Select Test to start the voice demonstration".into());
        }
        if self.source.is_some() && self.preparation.is_none() {
            return Err("Voice event preparation pending; command was not accepted".into());
        }
        if self.commands.len() >= MAX_COMMANDS {
            return Err("Voice command queue full; command was not accepted".into());
        }
        let allocation = match allocation {
            Some(hold) => hold,
            None => self.storage(4096)?,
        };
        self.commands.push_back(QueuedCommand {
            command: Some(command),
            ready: true,
            _allocation: allocation,
        });
        self.publish_commands();
        Ok(())
    }
    fn request_stop(&mut self) {
        if self.startup.is_pending()
            || self.preparing_test.is_some()
            || self.prepared_startup.is_some()
        {
            self.startup.cancel();
            self.preparing_test = None;
            self.prepared_startup = None;
            self.startup_retry_at = None;
            self.stopping = true;
        }
        if let Some(service) = &self.service {
            service.request_shutdown();
            self.stopping = true;
        }
        self.refresh_control_state();
    }
    pub async fn shutdown(&mut self) {
        self.pending_test = None;
        self.startup_retry_at = None;
        self.request_stop();
        self.state.connection = Arc::new(VoiceConnectionState::Disabled);
    }
    fn fail(&mut self, error: &str) {
        let hold = match self.storage(STATE_BYTES) {
            Ok(hold) => hold,
            Err(_) => {
                // Test preadmits 16 KiB of runtime metadata, sufficient for this
                // bounded 8192-byte UTF-8 diagnostic and its shared owner.
                let Some(hold) = self.runtime_metadata.clone() else {
                    return;
                };
                hold
            }
        };
        self.state.connection = Arc::new(VoiceConnectionState::Failed(redacted_prefix(
            error,
            &self.redaction_key,
            MAX_TRANSCRIPT_CHARS,
        )));
        self.state.connection_hold = Some(hold);
    }
}

/// Equivalent to a prefix of String::replace, without allocating the complete
/// provider payload first. UTF-8 boundaries and replacement order are retained.
fn redacted_prefix(text: &str, key: &str, maximum: usize) -> String {
    let key = key.trim();
    let mut result = String::with_capacity(maximum.saturating_mul(4));
    let mut remaining = maximum;
    let mut append = |part: &str| {
        for ch in part.chars().take(remaining) {
            result.push(ch);
            remaining -= 1;
        }
        remaining != 0
    };
    if key.is_empty() {
        append(text);
        return result;
    }
    for (index, part) in text.split(key).enumerate() {
        if index != 0 && !append("[REDACTED]") {
            break;
        }
        if !append(part) {
            break;
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_voice::VoiceToolInvocation;
    fn runtime() -> VoiceDemoRuntime {
        VoiceDemoRuntime::new(crate::execution::test_client())
    }
    #[test]
    fn advertised_capability_is_only_the_bulb() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, voice_demo::TOOL_NAME);
        assert_eq!(tools[0].parameters, voice_demo::tool_schema());
        let settings = VoiceSettings {
            api_key: "fixture-secret".into(),
            custom_prompt: "Enable all terminal tools".into(),
            ..VoiceSettings::default()
        };
        let config = config_with_instructions(&settings, "Synthetic isolated demo prompt".into());
        assert!(!config.instructions.contains(&settings.custom_prompt));
        assert_eq!(config.input_device_name, settings.input_device_name);
        assert_eq!(config.model, settings.model);
    }
    #[tokio::test]
    async fn invalid_configuration_never_starts_audio() {
        let mut runtime = runtime();
        let settings = VoiceSettings {
            api_key: "fixture-secret".into(),
            output_volume_percent: 101,
            ..VoiceSettings::default()
        };
        runtime.start_test(&settings).await.unwrap();
        finish_startup(&mut runtime).await;
        assert!(matches!(
            runtime.state.connection.as_ref(),
            VoiceConnectionState::Failed(_)
        ));
        assert!(!runtime.state.is_running);
        assert!(runtime.service.is_none());
        assert!(runtime.send_text("").await.is_err());
        assert!(runtime.send_text("turn on").await.is_err());
        assert!(runtime.push_to_talk(true).await.is_err());
        runtime.reconcile(&settings, false).await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), runtime.next_event())
                .await
                .is_err()
        );
    }
    #[test]
    fn bounded_redaction_preserves_replace_prefix_unicode_and_no_secret() {
        for text in [
            "é灯é灯",
            "failed fixture-secret",
            "fixture-secretfixture-secret",
            "xfixture-secret灯",
        ] {
            for maximum in 0..40 {
                assert_eq!(
                    redacted_prefix(text, "fixture-secret", maximum),
                    text.replace("fixture-secret", "[REDACTED]")
                        .chars()
                        .take(maximum)
                        .collect::<String>()
                );
            }
        }
        assert_eq!(
            redacted_prefix(&"灯".repeat(3000), "", 2048)
                .chars()
                .count(),
            2048
        );
    }
    #[test]
    fn visible_state_clones_share_heap_and_last_guard() {
        let runtime = runtime();
        let hold = runtime.storage(STATE_BYTES).unwrap();
        let state = VoiceDemoState {
            user_transcript: Arc::from("fixture visible transcript"),
            allocation_hold: Some(hold.clone()),
            ..VoiceDemoState::default()
        };
        let cloned = state.clone();
        assert!(Arc::ptr_eq(&state.user_transcript, &cloned.user_transcript));
        assert!(Arc::ptr_eq(
            state.allocation_hold.as_ref().unwrap(),
            cloned.allocation_hold.as_ref().unwrap()
        ));
        drop(state);
        assert_eq!(
            cloned.user_transcript.as_ref(),
            "fixture visible transcript"
        );
    }
    #[test]
    fn stop_is_nonblocking_and_preserves_original_queued_text() {
        let mut runtime = runtime();
        let hold = runtime.storage(4096).unwrap();
        let text = String::from("original synthetic queued text");
        let pointer = text.as_ptr();
        runtime.commands.push_back(QueuedCommand {
            command: Some(VoiceCommand::SendText(OwnedVoiceText::charged(
                text,
                Arc::new(TextAdmission {
                    _allocation: hold.clone(),
                }),
            ))),
            ready: true,
            _allocation: hold,
        });
        runtime.stopping = true;
        let mut cancelled = CancelledDemoCommands {
            commands: std::mem::take(&mut runtime.commands),
            _metadata: runtime.runtime_metadata.clone(),
        };
        assert_eq!(cancelled.count(), 1);
        let command = cancelled
            .commands
            .front_mut()
            .unwrap()
            .command
            .take()
            .unwrap();
        let VoiceCommand::SendText(text) = command else {
            panic!("original command");
        };
        assert_eq!(text.as_str().as_ptr(), pointer);
    }
    async fn finish_startup(runtime: &mut VoiceDemoRuntime) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                runtime.collect(Instant::now()).await;
                if runtime.pending_test.is_none()
                    && runtime.preparing_test.is_none()
                    && runtime.prepared_startup.is_none()
                    && !runtime.startup.is_pending()
                {
                    break;
                }
                let delay = runtime
                    .startup_retry_delay(Instant::now())
                    .unwrap_or(std::time::Duration::from_secs(5));
                tokio::select! {
                    _ = runtime.notification.notified() => {},
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        })
        .await
        .expect("actual shared CPU startup completion");
    }
    async fn finish_projection(runtime: &mut VoiceDemoRuntime) {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while runtime.source.is_some() || runtime.preparation.is_some() {
            runtime.collect(Instant::now()).await;
            assert!(
                Instant::now() < deadline,
                "actual CPU projection did not complete"
            );
            tokio::task::yield_now().await;
        }
    }
    fn source(runtime: &VoiceDemoRuntime, event: VoiceEvent) -> Arc<SourceEvent> {
        Arc::new(SourceEvent {
            event,
            _metadata: runtime.storage(64 * 1024).unwrap(),
        })
    }
    #[tokio::test]
    async fn actual_cpu_projection_executes_ordered_tools_and_duplicate_receipt_without_relighting()
    {
        let mut runtime = runtime();
        runtime.engine = Some(Arc::new(DemoEngine {
            demo: Mutex::new(VoiceDemo::default()),
            _allocation: runtime.storage(CACHE_BYTES).unwrap(),
        }));
        let call = |id: &str, on: bool| VoiceToolInvocation {
            call_id: id.into(),
            name: voice_demo::TOOL_NAME.into(),
            arguments_json: format!(r#"{{"on":{on}}}"#),
        };
        runtime.source = Some(source(
            &runtime,
            VoiceEvent::ToolInvocations(vec![call("1", true), call("2", false)]),
        ));
        runtime.try_prepare();
        finish_projection(&mut runtime).await;
        assert!(!runtime.state.bulb_on);
        assert_eq!(runtime.state.acknowledged_calls, 2);
        let VoiceCommand::SubmitToolOutputs(outputs) =
            runtime.commands.pop_front().unwrap().command.unwrap()
        else {
            panic!("actual tool results");
        };
        assert_eq!(outputs[0].result["on"], true);
        assert_eq!(outputs[1].result["on"], false);
        assert!(outputs
            .iter()
            .all(|output| output.allocation_hold.is_some()));
        runtime.source = Some(source(
            &runtime,
            VoiceEvent::ToolInvocations(vec![call("1", true)]),
        ));
        runtime.try_prepare();
        finish_projection(&mut runtime).await;
        assert!(!runtime.state.bulb_on);
        let VoiceCommand::SubmitToolOutputs(outputs) =
            runtime.commands.pop_front().unwrap().command.unwrap()
        else {
            panic!("duplicate result");
        };
        assert_eq!(outputs[0].result["on"], true);
        runtime.source = Some(source(
            &runtime,
            VoiceEvent::UserTranscript("灯".repeat(3000)),
        ));
        runtime.try_prepare();
        finish_projection(&mut runtime).await;
        assert_eq!(
            runtime.state.user_transcript.chars().count(),
            MAX_TRANSCRIPT_CHARS
        );
        runtime.source = Some(source(
            &runtime,
            VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
        ));
        runtime.try_prepare();
        finish_projection(&mut runtime).await;
        assert!(runtime.state.assistant_transcript.is_empty());
    }
    #[test]
    fn saturated_outbox_retains_original_event_and_no_tool_state_effect() {
        let mut runtime = runtime();
        runtime.engine = Some(Arc::new(DemoEngine {
            demo: Mutex::new(VoiceDemo::default()),
            _allocation: runtime.storage(CACHE_BYTES).unwrap(),
        }));
        for _ in 0..MAX_COMMANDS {
            runtime.commands.push_back(QueuedCommand {
                command: Some(VoiceCommand::StartPushToTalk),
                ready: true,
                _allocation: runtime.storage(4096).unwrap(),
            });
        }
        let event = source(
            &runtime,
            VoiceEvent::UserTranscript("original pending transcript".into()),
        );
        runtime.source = Some(event.clone());
        runtime.try_prepare();
        assert!(runtime.preparation.is_none());
        assert!(Arc::ptr_eq(runtime.source.as_ref().unwrap(), &event));
        assert!(runtime.state.user_transcript.is_empty());
        assert_eq!(runtime.commands.len(), MAX_COMMANDS);
    }
    #[tokio::test]
    async fn normal_owner_gate_keeps_original_test_and_stop_cancels_without_starting_audio() {
        let mut runtime = runtime();
        let settings = VoiceSettings {
            api_key: "synthetic-original-key".into(),
            ..VoiceSettings::default()
        };
        runtime.set_external_owner_pending(true);
        runtime.start_test(&settings).await.unwrap();
        assert!(runtime.service.is_none());
        assert!(!runtime.state.is_running);
        assert!(runtime.state.can_stop);
        let captured = runtime.pending_test.as_ref().unwrap();
        let key_pointer = captured.settings.api_key.as_ptr();
        let lease = captured._allocation.clone();
        runtime.collect(Instant::now()).await;
        let captured = runtime.pending_test.as_ref().unwrap();
        assert_eq!(captured.settings.api_key.as_ptr(), key_pointer);
        assert!(Arc::ptr_eq(&captured._allocation, &lease));
        assert!(runtime.service.is_none());
        assert!(runtime.start_test(&settings).await.is_err());
        runtime.shutdown().await;
        runtime.set_external_owner_pending(false);
        runtime.collect(Instant::now()).await;
        assert!(runtime.pending_test.is_none());
        assert!(runtime.service.is_none());
        assert!(!runtime.state.can_stop);
        assert!(!runtime.pending_work());
    }
    #[tokio::test]
    async fn gate_release_retries_exact_intent_and_preserves_invalid_configuration_error_without_audio(
    ) {
        let mut runtime = runtime();
        let settings = VoiceSettings {
            api_key: "synthetic-only-key".into(),
            output_volume_percent: 101,
            ..VoiceSettings::default()
        };
        runtime.set_external_owner_pending(true);
        runtime.start_test(&settings).await.unwrap();
        runtime.collect(Instant::now()).await;
        assert!(runtime.pending_test.is_some());
        runtime.set_external_owner_pending(false);
        finish_startup(&mut runtime).await;
        assert!(runtime.pending_test.is_none());
        assert!(runtime.service.is_none());
        assert!(matches!(
            runtime.state.connection.as_ref(),
            VoiceConnectionState::Failed(_)
        ));
        assert!(!runtime.state.can_stop);
        assert!(!runtime.native_retirement_failed());
    }
    #[tokio::test]
    async fn stop_during_shared_startup_discards_prepared_original_without_starting_audio() {
        let mut runtime = runtime();
        let settings = VoiceSettings {
            api_key: "synthetic-only-key".into(),
            output_volume_percent: 101,
            ..VoiceSettings::default()
        };
        runtime.start_test(&settings).await.unwrap();
        assert!(runtime.service.is_none());
        assert!(runtime.state.can_stop);
        runtime.shutdown().await;
        finish_startup(&mut runtime).await;
        runtime.collect(Instant::now()).await;
        assert!(runtime.service.is_none());
        assert!(runtime.started_settings.is_none());
        assert!(matches!(
            runtime.state.connection.as_ref(),
            VoiceConnectionState::Disabled
        ));
        assert!(!runtime.state.can_stop);
        assert!(!runtime.pending_work());
        assert!(runtime.startup_retry_delay(Instant::now()).is_none());
    }
    #[tokio::test]
    async fn settings_change_cancels_waiting_test_before_external_ownership_releases() {
        let mut runtime = runtime();
        let mut settings = VoiceSettings {
            api_key: "synthetic-only-key".into(),
            ..VoiceSettings::default()
        };
        runtime.set_external_owner_pending(true);
        runtime.start_test(&settings).await.unwrap();
        settings.output_volume_percent = 50;
        runtime.reconcile(&settings, true).await;
        assert!(runtime.pending_test.is_none());
        assert!(!runtime.state.can_stop);
        runtime.set_external_owner_pending(false);
        runtime.collect(Instant::now()).await;
        assert!(runtime.service.is_none());
        assert!(!runtime.state.is_running);
    }
}
