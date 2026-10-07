//! Owned, provider-neutral live voice runtime for ilium.
//!
//! This crate contains the volatile edges of voice control: microphone and
//! speaker devices, streaming resampling, and provider wire protocols. It has
//! no dependency on ratatui, ilium domain types, IPC, or application state.

mod audio;
mod audio_preparation;
mod config;
mod device_catalogue;
mod error;
mod openai;
mod tool;

pub use audio::{available_input_devices, available_output_devices, AudioCustody};
pub use config::{
    ReasoningEffort, VadEagerness, VoiceInputMode, VoiceModel, VoiceName, VoiceRuntimeConfig,
};
pub use error::VoiceError;
pub use secrecy::ExposeSecret;
pub use tool::{VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput};

use tokio::sync::{mpsc, watch};

/// Physical source layouts plus peak provider JSON/socket derivatives.
/// Requested layout allowance, not allocator RSS. Trusted generated schemas
/// receive2048 bytes per object/key plus an extra root. serde_json's enabled
/// IndexMap backing has opaque spare capacity: the producer's declaration must
/// additionally cover any externally preallocated map; this is not reflection
/// over hidden allocator state.
pub fn context_capture_bytes(instructions: &str, tools: &[VoiceToolDefinition]) -> Option<usize> {
    // Rust1.96.1 alloc/collections/btree/node.rs: B6,11 key/value
    // slots,12 child edges, parent pointer/two u16s. Extra64 covers padding.
    let node_layout = std::mem::size_of::<String>()
        .checked_add(std::mem::size_of::<serde_json::Value>())?
        .checked_mul(11)?
        .checked_add(std::mem::size_of::<usize>().checked_mul(14)?)?
        .checked_add(64)?;
    if node_layout > 2048 {
        return None;
    }
    fn value_bytes(value: &serde_json::Value, depth: usize) -> Option<usize> {
        if depth > 64 {
            return None;
        }
        use serde_json::Value;
        match value {
            Value::String(text) => Some(text.capacity()),
            Value::Array(values) => values.iter().try_fold(
                values
                    .capacity()
                    .checked_mul(std::mem::size_of::<Value>())?,
                |total, value| total.checked_add(value_bytes(value, depth + 1)?),
            ),
            Value::Object(values) => values.iter().try_fold(
                values.len().checked_add(1)?.checked_mul(2048)?,
                |total, (key, value)| {
                    total
                        .checked_add(key.capacity())?
                        .checked_add(value_bytes(value, depth + 1)?)
                },
            ),
            _ => Some(0),
        }
    }
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("voice encoded length overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let source = tools.iter().try_fold(
        instructions.len().checked_add(
            tools
                .len()
                .checked_mul(std::mem::size_of::<VoiceToolDefinition>())?,
        )?,
        |total, tool| {
            total
                .checked_add(tool.name.capacity())?
                .checked_add(tool.description.capacity())?
                .checked_add(value_bytes(&tool.parameters, 0)?)
        },
    )?;
    let mut encoded = Counter(0);
    serde_json::to_writer(&mut encoded, &(instructions, tools)).ok()?;
    // Config/session source coexist, serde payload clones, JSON Vec and socket
    // output Vec requested capacities use locked serde_json1.0.151 ser.rs
    // Vec128 and RawVec amortized max(required,2*old_capacity). 16KiB covers
    // minimum buffers and fixed session scaffolding; allocator RSS excluded.
    source
        .checked_mul(3)?
        .checked_add(encoded.0.checked_add(16 * 1024)?.checked_mul(4)?)
}

pub fn runtime_capture_bytes(
    config: &VoiceRuntimeConfig,
    tools: &Vec<VoiceToolDefinition>,
) -> Option<usize> {
    context_capture_bytes(&config.instructions, tools)?
        .checked_add(config.instructions.capacity())?
        .checked_add(
            tools
                .capacity()
                .checked_mul(std::mem::size_of::<VoiceToolDefinition>())?,
        )?
        .checked_add(
            config
                .input_device_name
                .as_ref()
                .map_or(0, String::capacity)
                .checked_mul(4)?,
        )?
        .checked_add(
            config
                .output_device_name
                .as_ref()
                .map_or(0, String::capacity)
                .checked_mul(4)?,
        )?
        .checked_add(config.api_key.expose_secret().len().checked_mul(8)?)
}

/// Provider configuration audited on the producer's CPU owner before spawn.
/// Private fields prohibit unchecked raw startup and keep source custody last.
pub struct OwnedVoiceStartup {
    config: VoiceRuntimeConfig,
    tools: Vec<VoiceToolDefinition>,
    retained_bytes: usize,
    allocation: std::sync::Arc<dyn VoiceTextAllocation>,
}
impl OwnedVoiceStartup {
    pub fn charged(
        config: VoiceRuntimeConfig,
        tools: Vec<VoiceToolDefinition>,
        retained_bytes: usize,
        allocation: std::sync::Arc<dyn VoiceTextAllocation>,
    ) -> Result<Self, VoiceError> {
        config
            .validate()
            .map_err(VoiceError::InvalidConfiguration)?;
        let required = runtime_capture_bytes(&config, &tools).ok_or_else(|| {
            VoiceError::InvalidConfiguration("voice startup layout overflow or depth limit".into())
        })?;
        if required > retained_bytes {
            return Err(VoiceError::InvalidConfiguration(
                "voice startup exceeds admitted source/transport allocation".into(),
            ));
        }
        Ok(Self {
            config,
            tools,
            retained_bytes,
            allocation,
        })
    }
    pub fn config(&self) -> &VoiceRuntimeConfig {
        &self.config
    }
}
/// Observable lifecycle state. The client renders this without knowing about
/// WebSockets or audio devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceConnectionState {
    Disabled,
    Connecting,
    Listening,
    Recording,
    Thinking,
    Speaking,
    Failed(String),
}

/// Events emitted by the runtime and consumed by the application event loop.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceEvent {
    StateChanged(VoiceConnectionState),
    ToolInvocations(Vec<VoiceToolInvocation>),
    UserTranscript(String),
    AssistantTranscript(String),
    ProviderError(String),
}

/// One original event and its admitted allocation, transferred without cloning.
#[derive(Debug)]
pub struct VoiceEventReceipt {
    event: VoiceEvent,
    allocation: std::sync::Arc<ilium_execution::StorageAdmission>,
}
impl VoiceEventReceipt {
    pub fn event(&self) -> &VoiceEvent {
        &self.event
    }
    /// Shares custody only; does not authorize another payload allocation.
    pub fn allocation(&self) -> std::sync::Arc<ilium_execution::StorageAdmission> {
        self.allocation.clone()
    }
    pub fn into_parts(
        self,
    ) -> (
        VoiceEvent,
        std::sync::Arc<ilium_execution::StorageAdmission>,
    ) {
        (self.event, self.allocation)
    }
}

#[derive(Debug)]
pub enum VoiceActorExit {
    Completed,
    Failed(VoiceError),
    Panicked(tokio::task::JoinError),
    Canceled,
}

pub enum VoiceShutdownState {
    Complete(VoiceActorExit),
    /// Consume this batch's events before resuming the SAME actor/receiver.
    Pending(Box<VoiceService>),
}

/// At most 128 original events. No actor exit or event loss is inferred when
/// the batch fills: Pending preserves the service and any staged stop command.
pub struct VoiceShutdownOutcome {
    pub events: std::collections::VecDeque<VoiceEventReceipt>,
    pub state: VoiceShutdownState,
    pub undelivered_stop_outputs: Option<Vec<VoiceToolOutput>>,
    pub undelivered_commands: std::collections::VecDeque<VoiceCommand>,
    pub audio_custody: AudioCustody,
    // Keep fixed channel/batch metadata charged through its actual last owner.
    _metadata: AudioCustody,
}

#[derive(Clone)]
pub(crate) struct EventSender {
    sender: mpsc::Sender<VoiceEventReceipt>,
    quota: ilium_execution::QuotaGroup,
}
impl EventSender {
    // byte_count is derived from borrowed source BEFORE constructing payloads.
    pub(crate) async fn send_with(
        &self,
        byte_count: usize,
        make: impl FnOnce() -> VoiceEvent,
    ) -> Result<(), VoiceError> {
        self.send_optional_with(byte_count, |_| Some(make())).await
    }
    pub(crate) async fn send_optional_with(
        &self,
        byte_count: usize,
        make: impl FnOnce(std::sync::Arc<ilium_execution::StorageAdmission>) -> Option<VoiceEvent>,
    ) -> Result<(), VoiceError> {
        let permit = self.sender.reserve().await.map_err(|_| {
            VoiceError::AudioPreparation("voice event receiver closed before admission".into())
        })?;
        let bytes = byte_count
            .checked_add(
                std::mem::size_of::<VoiceEventReceipt>()
                    + std::mem::size_of::<ilium_execution::StorageAdmission>()
                    + 2 * std::mem::size_of::<usize>(),
            )
            .ok_or_else(|| {
                VoiceError::AudioPreparation("voice event allocation size overflow".into())
            })?;
        let allocation = self
            .quota
            .reserve_external_storage(bytes)
            .map_err(|reason| {
                VoiceError::AudioPreparation(format!("voice event admission: {reason:?}"))
            })?;
        let allocation = std::sync::Arc::new(allocation);
        if let Some(event) = make(allocation.clone()) {
            permit.send(VoiceEventReceipt { event, allocation });
        }
        Ok(())
    }
    pub(crate) async fn send_state(&self, state: VoiceConnectionState) -> Result<(), VoiceError> {
        let capacity = match &state {
            VoiceConnectionState::Failed(text) => text.capacity(),
            _ => 0,
        };
        self.send_with(capacity, || VoiceEvent::StateChanged(state))
            .await
    }
}

/// Allocation ownership follows typed text through the actor and provider
/// publication. This lease grants no allocation or cloning authority.
pub trait VoiceTextAllocation: std::fmt::Debug + Send + Sync {}

#[derive(Debug)]
pub struct OwnedVoiceText {
    text: String,
    _allocation: Option<std::sync::Arc<dyn VoiceTextAllocation>>,
}
impl OwnedVoiceText {
    pub fn charged(text: String, allocation: std::sync::Arc<dyn VoiceTextAllocation>) -> Self {
        Self {
            text,
            _allocation: Some(allocation),
        }
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
    pub(crate) fn retained_capacity(&self) -> usize {
        self.text.capacity()
    }
    pub(crate) fn allocation(&self) -> Option<std::sync::Arc<dyn VoiceTextAllocation>> {
        self._allocation.clone()
    }
    pub(crate) fn trim_in_place(&mut self) {
        let start = self.text.len() - self.text.trim_start().len();
        let end = self.text.trim_end().len();
        self.text.truncate(end.max(start));
        self.text.drain(..start);
    }
}
impl From<String> for OwnedVoiceText {
    fn from(text: String) -> Self {
        Self {
            text,
            _allocation: None,
        }
    }
}

/// Original context plus its producer admission. The declaration must include
/// tools/JSON, temporary serialization and retained transport-buffer capacity.
#[derive(Debug)]
pub struct OwnedVoiceContext {
    instructions: String,
    tools: Vec<VoiceToolDefinition>,
    retained_bytes: usize,
    allocation: std::sync::Arc<dyn VoiceTextAllocation>,
}
impl OwnedVoiceContext {
    pub fn charged(
        instructions: String,
        tools: Vec<VoiceToolDefinition>,
        retained_bytes: usize,
        allocation: std::sync::Arc<dyn VoiceTextAllocation>,
    ) -> Result<Self, VoiceError> {
        let required = context_capture_bytes(&instructions, &tools)
            .and_then(|bytes| bytes.checked_add(instructions.capacity()))
            .and_then(|bytes| {
                bytes.checked_add(
                    tools
                        .capacity()
                        .checked_mul(std::mem::size_of::<VoiceToolDefinition>())?,
                )
            })
            .ok_or_else(|| {
                VoiceError::InvalidConfiguration(
                    "voice context layout overflow or depth limit".into(),
                )
            })?;
        if required > retained_bytes {
            return Err(VoiceError::InvalidConfiguration(
                "voice context exceeds admitted source/transport allocation".into(),
            ));
        }
        Ok(Self {
            instructions,
            tools,
            retained_bytes,
            allocation,
        })
    }
    pub fn instructions(&self) -> &str {
        &self.instructions
    }
    pub fn tools(&self) -> &[VoiceToolDefinition] {
        &self.tools
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub(crate) fn into_parts(
        self,
    ) -> (
        String,
        Vec<VoiceToolDefinition>,
        usize,
        std::sync::Arc<dyn VoiceTextAllocation>,
    ) {
        (
            self.instructions,
            self.tools,
            self.retained_bytes,
            self.allocation,
        )
    }
}
/// Commands accepted by a running voice service.
#[derive(Debug)]
pub enum VoiceCommand {
    UpdateContext(OwnedVoiceContext),
    SubmitToolOutputs(Vec<VoiceToolOutput>),
    /// Writes final function results, suppresses a follow-up response, and
    /// closes the provider/audio session in that exact order.
    SubmitToolOutputsAndShutdown(Vec<VoiceToolOutput>),
    /// Adds a user text turn to the same conversation. The TUI does not use
    /// this for ordinary operation; it provides a deterministic accessibility
    /// and live-protocol test seam without synthesizing microphone audio.
    SendText(OwnedVoiceText),
    StartPushToTalk,
    StopPushToTalk,
}

struct ActorCompletionWake(std::sync::Arc<tokio::sync::Notify>);
impl Drop for ActorCompletionWake {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

/// Owned handle for one running voice actor.
/// Preadmitted actor/channel metadata. The private quota is the same owner
/// used by the actor and its native workers; refusal cannot consume startup.
pub struct VoiceStartupAdmission {
    quota: ilium_execution::QuotaGroup,
    audio_custody: AudioCustody,
}

pub struct VoiceService {
    instance_identity: std::sync::Arc<()>,
    command_sender: mpsc::Sender<VoiceCommand>,
    command_custody: std::sync::Arc<tokio::sync::Mutex<mpsc::Receiver<VoiceCommand>>>,
    undelivered_commands: std::collections::VecDeque<VoiceCommand>,
    commands_drained: bool,
    shutdown_sender: watch::Sender<bool>,
    event_receiver: mpsc::Receiver<VoiceEventReceipt>,
    task: Option<tokio::task::JoinHandle<Result<(), VoiceError>>>,
    actor_exit: Option<VoiceActorExit>,
    actor_completion: std::sync::Arc<tokio::sync::Notify>,
    pending_stop_outputs: Option<Vec<VoiceToolOutput>>,
    stop_command_closed: bool,
    audio_custody: AudioCustody,
}

impl VoiceService {
    pub fn instance_identity(&self) -> std::sync::Arc<()> {
        std::sync::Arc::clone(&self.instance_identity)
    }
    /// Reserve actor/channel metadata without consuming a prepared startup.
    /// A refusal leaves the caller's original configuration available for retry.
    pub fn admit_startup(
        quota: ilium_execution::QuotaGroup,
    ) -> Result<VoiceStartupAdmission, ilium_execution::RejectReason> {
        let audio_custody = AudioCustody::try_new(&quota)?;
        Ok(VoiceStartupAdmission {
            quota,
            audio_custody,
        })
    }

    /// Start from configuration and metadata already owned by this admission.
    /// Provider/device failures arrive through events and the actor exit receipt.
    pub fn start(startup: OwnedVoiceStartup, admission: VoiceStartupAdmission) -> Self {
        let VoiceStartupAdmission {
            quota,
            audio_custody,
        } = admission;
        let OwnedVoiceStartup {
            config,
            tools,
            retained_bytes,
            allocation,
        } = startup;
        audio_custody.retain_startup(allocation.clone());
        let actor_custody = audio_custody.clone();
        let (command_sender, command_receiver) = mpsc::channel(64);
        let command_custody = std::sync::Arc::new(tokio::sync::Mutex::new(command_receiver));
        let actor_commands = command_custody.clone();
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let (sender, event_receiver) = mpsc::channel(128);
        let event_sender = EventSender {
            sender,
            quota: quota.clone(),
        };
        let actor_completion = std::sync::Arc::new(tokio::sync::Notify::new());
        let completion_wake = ActorCompletionWake(actor_completion.clone());
        let task = tokio::spawn(async move {
            let _completion_wake = completion_wake;
            // One steady owner; UI never waits for this mutex. Outside-actor
            // custody preserves originals even if the actor panics/cancels.
            let mut command_receiver = actor_commands.lock().await;
            let result = openai::run_session(
                OwnedVoiceStartup {
                    config,
                    tools,
                    retained_bytes,
                    allocation,
                },
                &mut command_receiver,
                shutdown_receiver,
                event_sender.clone(),
                quota,
                actor_custody,
            )
            .await;
            if let Err(error) = &result {
                tracing::error!(%error, "OpenAI Realtime voice session failed");
                // Count formatting without allocating the owned error string.
                struct Count(usize);
                impl std::fmt::Write for Count {
                    fn write_str(&mut self, text: &str) -> std::fmt::Result {
                        self.0 = self.0.checked_add(text.len()).ok_or(std::fmt::Error)?;
                        Ok(())
                    }
                }
                use std::fmt::Write;
                let mut count = Count(0);
                if write!(&mut count, "{error}").is_ok() {
                    if let Err(delivery_error) = event_sender.send_optional_with(count.0, |_| {
                        let mut text = String::with_capacity(count.0);
                        match write!(&mut text, "{error}") {
                            Ok(()) => Some(VoiceEvent::StateChanged(VoiceConnectionState::Failed(text))),
                            Err(_) => {
                                tracing::error!("voice failure formatting failed; original actor error retained");
                                None
                            }
                        }
                    }).await {
                        tracing::error!(%delivery_error, "voice failure event not admitted; original actor error retained in terminal receipt");
                    }
                } else {
                    tracing::error!(
                        "voice failure size measurement failed; original actor error retained"
                    );
                }
            }
            result
        });

        Self {
            instance_identity: std::sync::Arc::new(()),
            command_sender,
            command_custody,
            undelivered_commands: std::collections::VecDeque::with_capacity(64),
            commands_drained: false,
            shutdown_sender,
            event_receiver,
            task: Some(task),
            actor_exit: None,
            actor_completion,
            pending_stop_outputs: None,
            stop_command_closed: false,
            audio_custody,
        }
    }

    /// Native retirement is separate from provider actor completion.
    pub fn audio_custody(&self) -> AudioCustody {
        self.audio_custody.clone()
    }
    pub fn command_sender(&self) -> mpsc::Sender<VoiceCommand> {
        self.command_sender.clone()
    }

    pub async fn next_event(&mut self) -> Option<VoiceEventReceipt> {
        self.event_receiver.recv().await
    }

    pub fn try_next_event(&mut self) -> Result<VoiceEventReceipt, mpsc::error::TryRecvError> {
        self.event_receiver.try_recv()
    }

    /// Nonblocking: keep this service and receiver alive while draining FIFO.
    pub fn request_shutdown(&self) {
        let _ = self.shutdown_sender.send(true);
    }

    /// Readiness hint only. Actual actor outcome is consumed by shutdown batch.
    pub fn actor_is_finished(&self) -> bool {
        self.task
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    /// Wake hint; consume the actual join outcome before claiming actor exit.
    pub fn completion_notification(&self) -> std::sync::Arc<tokio::sync::Notify> {
        self.actor_completion.clone()
    }

    /// One bounded original-output slot, admitted without waiting on commands.
    pub fn stage_stop_outputs(
        &mut self,
        outputs: Vec<VoiceToolOutput>,
    ) -> Result<(), Vec<VoiceToolOutput>> {
        if self.pending_stop_outputs.is_some()
            || self.actor_is_finished()
            || self.stop_command_closed
        {
            return Err(outputs);
        }
        self.pending_stop_outputs = Some(outputs);
        self.try_publish_staged_stop_outputs();
        Ok(())
    }
    pub fn has_staged_stop_outputs(&self) -> bool {
        self.pending_stop_outputs.is_some()
    }

    /// Retry under a command/event/completion/tick wake. Full retains originals.
    pub fn try_publish_staged_stop_outputs(&mut self) -> bool {
        if self.pending_stop_outputs.is_none() || self.stop_command_closed {
            return false;
        }
        match self.command_sender.try_reserve() {
            Ok(permit) => {
                if let Some(outputs) = self.pending_stop_outputs.take() {
                    permit.send(VoiceCommand::SubmitToolOutputsAndShutdown(outputs));
                    true
                } else {
                    false
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => false,
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.stop_command_closed = true;
                self.request_shutdown();
                false
            }
        }
    }

    pub async fn finish_actor_if_ready(self) -> VoiceShutdownOutcome {
        if self.actor_is_finished() {
            self.drain_shutdown_batch().await
        } else {
            self.pending_outcome(std::collections::VecDeque::new())
        }
    }

    pub async fn shutdown(self) -> VoiceShutdownOutcome {
        self.request_shutdown();
        self.drain_shutdown_batch().await
    }

    pub async fn shutdown_after_tool_outputs(
        mut self,
        outputs: Vec<VoiceToolOutput>,
    ) -> VoiceShutdownOutcome {
        if self.pending_stop_outputs.is_some() {
            let mut outcome = self.pending_outcome(std::collections::VecDeque::new());
            outcome.undelivered_stop_outputs = Some(outputs);
            return outcome;
        }
        self.pending_stop_outputs = Some(outputs);
        self.drain_shutdown_batch().await
    }

    /// Resumes the exact prior batch, including ordered self-stop delivery.
    pub async fn continue_shutdown(self) -> VoiceShutdownOutcome {
        self.drain_shutdown_batch().await
    }

    fn pending_outcome(
        self,
        events: std::collections::VecDeque<VoiceEventReceipt>,
    ) -> VoiceShutdownOutcome {
        let audio_custody = self.audio_custody.clone();
        VoiceShutdownOutcome {
            events,
            state: VoiceShutdownState::Pending(Box::new(self)),
            undelivered_stop_outputs: None,
            undelivered_commands: std::collections::VecDeque::new(),
            _metadata: audio_custody.clone(),
            audio_custody,
        }
    }

    fn record_actor_exit(
        &mut self,
        joined: Result<Result<(), VoiceError>, tokio::task::JoinError>,
    ) {
        self.task.take();
        self.actor_exit = Some(match joined {
            Ok(Ok(())) => VoiceActorExit::Completed,
            Ok(Err(error)) => VoiceActorExit::Failed(error),
            Err(error) if error.is_cancelled() => VoiceActorExit::Canceled,
            Err(error) => VoiceActorExit::Panicked(error),
        });
    }

    fn recover_undelivered_commands(&mut self) {
        if self.task.is_some() || self.commands_drained {
            return;
        }
        let Ok(mut receiver) = self.command_custody.try_lock() else {
            return;
        };
        receiver.close();
        while self.undelivered_commands.len() < 64 {
            match receiver.try_recv() {
                Ok(command) => self.undelivered_commands.push_back(command),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.commands_drained = true;
                    break;
                }
                // Outstanding previously reserved permits can still publish.
                // Pending keeps originals until actual channel EOF, no wait.
                Err(mpsc::error::TryRecvError::Empty) => break,
            }
        }
        if self.undelivered_commands.len() == 64 {
            // Receiver was closed before draining. Queued values plus already
            // reserved permits total <=64. All 64 recovered and all semaphore
            // slots free proves EOF; never pop a hypothetical extra original.
            self.commands_drained = receiver.is_closed()
                && receiver.is_empty()
                && receiver.capacity() == receiver.max_capacity();
        }
    }

    async fn drain_shutdown_batch(mut self) -> VoiceShutdownOutcome {
        let mut events = std::collections::VecDeque::with_capacity(128);
        loop {
            while events.len() < 128 {
                match self.event_receiver.try_recv() {
                    Ok(event) => events.push_back(event),
                    Err(_) => break,
                }
            }
            if self.task.is_none() && self.event_receiver.is_empty() {
                self.recover_undelivered_commands();
                if !self.commands_drained {
                    return self.pending_outcome(events);
                }
                return VoiceShutdownOutcome {
                    events,
                    state: VoiceShutdownState::Complete(
                        self.actor_exit.take().unwrap_or(VoiceActorExit::Canceled),
                    ),
                    undelivered_stop_outputs: self.pending_stop_outputs.take(),
                    undelivered_commands: std::mem::take(&mut self.undelivered_commands),
                    audio_custody: self.audio_custody.clone(),
                    _metadata: self.audio_custody.clone(),
                };
            }
            if events.len() == 128 {
                return self.pending_outcome(events);
            }
            let command_sender = self.command_sender.clone();
            tokio::select! {
                event = self.event_receiver.recv(), if !self.event_receiver.is_closed() || !self.event_receiver.is_empty() => {
                    if let Some(event) = event { events.push_back(event); }
                }
                joined = async { match self.task.as_mut() { Some(task) => task.await, None => std::future::pending().await } }, if self.task.is_some() => {
                    self.record_actor_exit(joined);
                }
                permit = command_sender.reserve(), if self.pending_stop_outputs.is_some() && !self.stop_command_closed && self.task.is_some() => {
                    match permit {
                        Ok(permit) => {
                            if let Some(outputs) = self.pending_stop_outputs.take() {
                                permit.send(VoiceCommand::SubmitToolOutputsAndShutdown(outputs));
                            }
                        }
                        Err(_) => { self.stop_command_closed = true; self.request_shutdown(); },
                    }
                }
            }
        }
    }
}

impl Drop for VoiceService {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[cfg(test)]
mod owned_text_tests {
    use super::*;
    #[derive(Debug)]
    struct AllocationProbe(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl VoiceTextAllocation for AllocationProbe {}
    impl Drop for AllocationProbe {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    #[test]
    fn context_audit_charges_full_single_entry_nodes_and_rejects_spare_capacity() {
        let tools = vec![VoiceToolDefinition {
            name: String::new(),
            description: String::new(),
            parameters: serde_json::json!({"one":{"two":true}}),
        }];
        let measured = context_capture_bytes("", &tools).unwrap();
        let empty = context_capture_bytes("", &[]).unwrap();
        assert!(
            measured >= empty + 2 * 2 * 2048 * 3,
            "two single-entry maps each own full root/node allowances"
        );
        let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut instructions = String::with_capacity(1024 * 1024);
        instructions.push_str("short");
        assert!(OwnedVoiceContext::charged(
            instructions,
            Vec::new(),
            measured,
            std::sync::Arc::new(AllocationProbe(released.clone()))
        )
        .is_err());
        assert!(released.load(std::sync::atomic::Ordering::Acquire));
    }
    #[tokio::test]
    async fn original_context_and_allocation_survive_ordered_queue() {
        let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let instructions = String::from("original context λ");
        let pointer = instructions.as_ptr();
        let declared_bytes =
            context_capture_bytes(&instructions, &[]).unwrap() + instructions.capacity();
        let context = OwnedVoiceContext::charged(
            instructions,
            Vec::new(),
            declared_bytes,
            std::sync::Arc::new(AllocationProbe(released.clone())),
        )
        .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(VoiceCommand::UpdateContext(context))
            .unwrap();
        assert!(!released.load(std::sync::atomic::Ordering::Acquire));
        let VoiceCommand::UpdateContext(context) = receiver.recv().await.unwrap() else {
            panic!("original context");
        };
        let (instructions, tools, declared, allocation) = context.into_parts();
        assert_eq!(instructions.as_ptr(), pointer);
        assert!(tools.is_empty());
        assert_eq!(declared, declared_bytes);
        assert!(!released.load(std::sync::atomic::Ordering::Acquire));
        drop(instructions);
        drop(tools);
        drop(allocation);
        assert!(released.load(std::sync::atomic::Ordering::Acquire));
    }
    #[tokio::test]
    async fn original_text_capacity_and_owner_survive_actor_queue_and_in_place_trim() {
        let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut text = String::with_capacity(8192);
        text.push_str("  literal λ  ");
        let pointer = text.as_ptr();
        let capacity = text.capacity();
        let payload =
            OwnedVoiceText::charged(text, std::sync::Arc::new(AllocationProbe(released.clone())));
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender.try_send(VoiceCommand::SendText(payload)).unwrap();
        assert!(!released.load(std::sync::atomic::Ordering::Acquire));
        let VoiceCommand::SendText(mut payload) = receiver.recv().await.unwrap() else {
            panic!("typed original");
        };
        payload.trim_in_place();
        assert_eq!(payload.as_str(), "literal λ");
        assert_eq!(payload.as_str().as_ptr(), pointer);
        assert_eq!(payload.text.capacity(), capacity);
        assert!(!released.load(std::sync::atomic::Ordering::Acquire));
        drop(payload);
        assert!(released.load(std::sync::atomic::Ordering::Acquire));
    }
}

#[cfg(test)]
pub(crate) fn test_quota() -> ilium_execution::QuotaGroup {
    ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 8,
        worker_bytes: 128 * 1024 * 1024,
    })
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use std::time::Duration;

    fn fixture_service(
        quota: ilium_execution::QuotaGroup,
        actor: impl FnOnce(
            EventSender,
            watch::Receiver<bool>,
            ActorCompletionWake,
        ) -> tokio::task::JoinHandle<Result<(), VoiceError>>,
    ) -> VoiceService {
        let audio_custody = AudioCustody::new(&quota).unwrap();
        let (command_sender, receiver) = mpsc::channel(64);
        let command_custody = std::sync::Arc::new(tokio::sync::Mutex::new(receiver));
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let (sender, event_receiver) = mpsc::channel(128);
        let actor_completion = std::sync::Arc::new(tokio::sync::Notify::new());
        let task = actor(
            EventSender { sender, quota },
            shutdown_receiver,
            ActorCompletionWake(actor_completion.clone()),
        );
        VoiceService {
            instance_identity: std::sync::Arc::new(()),
            command_sender,
            command_custody,
            undelivered_commands: std::collections::VecDeque::with_capacity(64),
            commands_drained: false,
            shutdown_sender,
            event_receiver,
            task: Some(task),
            actor_exit: None,
            actor_completion,
            pending_stop_outputs: None,
            stop_command_closed: false,
            audio_custody,
        }
    }

    #[tokio::test]
    async fn full_event_queue_shutdown_preserves_fifo_original_bytes_and_actual_actor_exit() {
        let quota = crate::test_quota();
        let (filled, blocked_actor) = tokio::sync::oneshot::channel();
        let service = fixture_service(quota.clone(), move |sender, mut shutdown, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                for index in 0..128 {
                    let source = format!("event{index:03}");
                    sender
                        .send_with(source.capacity(), || {
                            VoiceEvent::AssistantTranscript(source)
                        })
                        .await?;
                }
                let original = String::from("original-last-transcript");
                let original_pointer = original.as_ptr() as usize;
                let _ = filled.send(original_pointer);
                // Queue is full: the actor really blocks before observing stop.
                sender
                    .send_with(original.capacity(), || VoiceEvent::UserTranscript(original))
                    .await?;
                let _ = shutdown.changed().await;
                Ok(())
            })
        });
        let original_pointer = blocked_actor.await.unwrap();
        assert_eq!(service.event_receiver.len(), 128);
        assert!(!service.actor_is_finished());
        service.request_shutdown();
        let mut outcome = tokio::time::timeout(Duration::from_secs(2), service.shutdown())
            .await
            .unwrap();
        assert_eq!(outcome.events.len(), 128);
        assert!(matches!(outcome.state, VoiceShutdownState::Pending(_)));
        let mut observed = 0usize;
        loop {
            assert!(outcome.events.len() <= 128);
            for receipt in outcome.events.drain(..) {
                match receipt.event() {
                    VoiceEvent::AssistantTranscript(text) => {
                        assert_eq!(text, &format!("event{observed:03}"))
                    }
                    VoiceEvent::UserTranscript(text) => {
                        assert_eq!(observed, 128);
                        assert_eq!(text.as_ptr() as usize, original_pointer);
                    }
                    _ => panic!("unexpected fixture event"),
                }
                observed += 1;
            }
            match outcome.state {
                VoiceShutdownState::Pending(service) => {
                    outcome =
                        tokio::time::timeout(Duration::from_secs(2), service.continue_shutdown())
                            .await
                            .unwrap();
                }
                VoiceShutdownState::Complete(exit) => {
                    assert!(matches!(exit, VoiceActorExit::Completed));
                    assert!(outcome.undelivered_stop_outputs.is_none());
                    drop(outcome.audio_custody);
                    drop(outcome.events);
                    drop(outcome._metadata);
                    break;
                }
            }
        }
        assert_eq!(observed, 129);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn not_ready_finish_returns_original_service_without_waiting_or_receiver_loss() {
        let (release, ready) = tokio::sync::oneshot::channel();
        let service = fixture_service(crate::test_quota(), move |_, _, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                let _ = ready.await;
                Ok(())
            })
        });
        let identity = service.instance_identity();
        let outcome = service.finish_actor_if_ready().await;
        let VoiceShutdownState::Pending(service) = outcome.state else {
            panic!("blocked actor must stay pending");
        };
        assert!(std::sync::Arc::ptr_eq(
            &identity,
            &service.instance_identity()
        ));
        assert!(outcome.events.is_empty());
        release.send(()).unwrap();
        let outcome = service.continue_shutdown().await;
        assert!(matches!(
            outcome.state,
            VoiceShutdownState::Complete(VoiceActorExit::Completed)
        ));
    }

    #[tokio::test]
    async fn event_admission_precedes_capture_and_receipt_holds_exact_storage() {
        let quota = crate::test_quota();
        let (sender, mut receiver) = mpsc::channel(1);
        let sender = EventSender {
            sender,
            quota: quota.clone(),
        };
        let captured = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = captured.clone();
        let oversize = quota.snapshot().limits.worker_bytes;
        assert!(sender
            .send_with(oversize, || {
                probe.store(true, std::sync::atomic::Ordering::Release);
                VoiceEvent::AssistantTranscript("must not capture".into())
            })
            .await
            .is_err());
        assert!(!captured.load(std::sync::atomic::Ordering::Acquire));
        assert!(receiver.try_recv().is_err());
        let original = String::from("retained-source");
        let pointer = original.as_ptr();
        sender
            .send_with(original.capacity(), || VoiceEvent::UserTranscript(original))
            .await
            .unwrap();
        let receipt = receiver.recv().await.unwrap();
        drop(sender);
        let (event, allocation) = receipt.into_parts();
        assert!(quota.snapshot().worker_bytes > 0);
        let VoiceEvent::UserTranscript(text) = event else {
            panic!("original transcript");
        };
        assert_eq!(text.as_ptr(), pointer);
        drop(text);
        assert!(quota.snapshot().worker_bytes > 0);
        drop(allocation);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[tokio::test]
    async fn staged_stop_outputs_preserve_original_allocation_and_command_fifo_on_full_queue() {
        let quota = crate::test_quota();
        let audio_custody = AudioCustody::new(&quota).unwrap();
        let (command_sender, commands) = mpsc::channel(64);
        let command_custody = std::sync::Arc::new(tokio::sync::Mutex::new(commands));
        let actor_commands = command_custody.clone();
        let (shutdown_sender, shutdown) = watch::channel(false);
        let (sender, event_receiver) = mpsc::channel(128);
        let actor_completion = std::sync::Arc::new(tokio::sync::Notify::new());
        let wake = ActorCompletionWake(actor_completion.clone());
        let (release, ready) = tokio::sync::oneshot::channel();
        let (drained, drain_ready) = tokio::sync::oneshot::channel();
        let (received, observed) = tokio::sync::oneshot::channel();
        let event_sender = EventSender { sender, quota };
        let task = tokio::spawn(async move {
            let _wake = wake;
            let mut commands = actor_commands.lock().await;
            let _ = ready.await;
            assert!(!*shutdown.borrow());
            for _ in 0..64 {
                assert!(matches!(
                    commands.recv().await,
                    Some(VoiceCommand::StartPushToTalk)
                ));
            }
            let _ = drained.send(());
            let Some(VoiceCommand::SubmitToolOutputsAndShutdown(outputs)) = commands.recv().await
            else {
                panic!("ordered original stop outputs");
            };
            let _ = received.send(outputs.as_ptr() as usize);
            event_sender
                .send_state(VoiceConnectionState::Disabled)
                .await?;
            Ok(())
        });
        let mut service = VoiceService {
            instance_identity: std::sync::Arc::new(()),
            command_sender,
            command_custody,
            undelivered_commands: std::collections::VecDeque::with_capacity(64),
            commands_drained: false,
            shutdown_sender,
            event_receiver,
            task: Some(task),
            actor_exit: None,
            actor_completion,
            pending_stop_outputs: None,
            stop_command_closed: false,
            audio_custody,
        };
        for _ in 0..64 {
            service
                .command_sender
                .try_send(VoiceCommand::StartPushToTalk)
                .unwrap();
        }
        let outputs = vec![VoiceToolOutput {
            call_id: "original-call-id".into(),
            result: std::sync::Arc::new(serde_json::json!({"ok": true})),
            request_follow_up: false,
            terminate_session_after_delivery: true,
            allocation_hold: None,
            retained_bytes: 0,
        }];
        let pointer = outputs.as_ptr() as usize;
        service.stage_stop_outputs(outputs).unwrap();
        assert!(service.has_staged_stop_outputs());
        assert!(!service.try_publish_staged_stop_outputs());
        let refused = vec![VoiceToolOutput {
            call_id: "second-original".into(),
            result: std::sync::Arc::new(serde_json::Value::Null),
            request_follow_up: false,
            terminate_session_after_delivery: true,
            allocation_hold: None,
            retained_bytes: 0,
        }];
        let refused_pointer = refused.as_ptr();
        let refused = service.stage_stop_outputs(refused).unwrap_err();
        assert_eq!(refused.as_ptr(), refused_pointer);
        drop(refused);
        release.send(()).unwrap();
        drain_ready.await.unwrap();
        assert!(service.try_publish_staged_stop_outputs());
        assert!(!service.has_staged_stop_outputs());
        assert_eq!(observed.await.unwrap(), pointer);
        let outcome = service.continue_shutdown().await;
        assert!(matches!(
            outcome.state,
            VoiceShutdownState::Complete(VoiceActorExit::Completed)
        ));
        assert_eq!(outcome.events.len(), 1);
        assert!(matches!(
            outcome.events[0].event(),
            VoiceEvent::StateChanged(VoiceConnectionState::Disabled)
        ));
        assert!(outcome.undelivered_stop_outputs.is_none());
    }

    #[tokio::test]
    async fn original_actor_failure_and_cancellation_are_distinct_terminal_receipts() {
        let error = String::from("original actor failure");
        let pointer = error.as_ptr() as usize;
        let service = fixture_service(crate::test_quota(), move |_, _, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                Err(VoiceError::AudioPreparation(error))
            })
        });
        let outcome = service.shutdown().await;
        let VoiceShutdownState::Complete(VoiceActorExit::Failed(VoiceError::AudioPreparation(
            error,
        ))) = outcome.state
        else {
            panic!("typed original failure");
        };
        assert_eq!(error.as_ptr() as usize, pointer);
        let (_hold, blocked) = tokio::sync::oneshot::channel::<()>();
        let service = fixture_service(crate::test_quota(), move |_, _, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                let _ = blocked.await;
                Ok(())
            })
        });
        service.task.as_ref().unwrap().abort();
        let outcome = service.shutdown().await;
        assert!(matches!(
            outcome.state,
            VoiceShutdownState::Complete(VoiceActorExit::Canceled)
        ));
    }
    #[tokio::test]
    async fn canceled_actor_retains_queued_original_command_and_source_guard() {
        let (_hold, blocked) = tokio::sync::oneshot::channel::<()>();
        let service = fixture_service(crate::test_quota(), move |_, _, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                let _ = blocked.await;
                Ok(())
            })
        });
        let original = String::from("original queued typed bytes");
        let pointer = original.as_ptr();
        service
            .command_sender
            .try_send(VoiceCommand::SendText(original.into()))
            .unwrap();
        service.task.as_ref().unwrap().abort();
        let outcome = service.shutdown().await;
        assert!(matches!(
            outcome.state,
            VoiceShutdownState::Complete(VoiceActorExit::Canceled)
        ));
        assert_eq!(outcome.undelivered_commands.len(), 1);
        let VoiceCommand::SendText(text) = &outcome.undelivered_commands[0] else {
            panic!("original command");
        };
        assert_eq!(text.as_str().as_ptr(), pointer);
        assert_eq!(text.as_str(), "original queued typed bytes");
    }

    #[tokio::test]
    async fn outstanding_previously_admitted_command_permit_keeps_terminal_outcome_pending() {
        let service = fixture_service(crate::test_quota(), move |_, _, wake| {
            tokio::spawn(async move {
                let _wake = wake;
                Ok(())
            })
        });
        let permit = service
            .command_sender
            .clone()
            .reserve_owned()
            .await
            .unwrap();
        let outcome = service.shutdown().await;
        assert!(outcome.events.is_empty());
        let VoiceShutdownState::Pending(service) = outcome.state else {
            panic!("reserved original cannot be inferred complete");
        };
        let original = String::from("late admitted original");
        let pointer = original.as_ptr();
        permit.send(VoiceCommand::SendText(original.into()));
        let outcome = service.continue_shutdown().await;
        assert!(matches!(
            outcome.state,
            VoiceShutdownState::Complete(VoiceActorExit::Completed)
        ));
        assert_eq!(outcome.undelivered_commands.len(), 1);
        let VoiceCommand::SendText(text) = &outcome.undelivered_commands[0] else {
            panic!("late original");
        };
        assert_eq!(text.as_str().as_ptr(), pointer);
    }
}
