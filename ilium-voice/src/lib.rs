//! Owned, provider-neutral live voice runtime for ilium.
//!
//! This crate contains the volatile edges of voice control: microphone and
//! speaker devices, streaming resampling, and provider wire protocols. It has
//! no dependency on ratatui, ilium domain types, IPC, or application state.

mod audio;
mod config;
mod error;
mod openai;
mod tool;

pub use audio::{available_input_devices, available_output_devices};
pub use config::{
    ReasoningEffort, VadEagerness, VoiceInputMode, VoiceModel, VoiceName, VoiceRuntimeConfig,
};
pub use error::VoiceError;
pub use tool::{VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput};

use tokio::sync::{mpsc, watch};

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

/// Commands accepted by a running voice service.
#[derive(Debug)]
pub enum VoiceCommand {
    UpdateContext {
        instructions: String,
        tools: Vec<VoiceToolDefinition>,
    },
    SubmitToolOutputs(Vec<VoiceToolOutput>),
    /// Adds a user text turn to the same conversation. The TUI does not use
    /// this for ordinary operation; it provides a deterministic accessibility
    /// and live-protocol test seam without synthesizing microphone audio.
    SendText(String),
    StartPushToTalk,
    StopPushToTalk,
}

/// Owned handle for one running voice actor.
pub struct VoiceService {
    command_sender: mpsc::Sender<VoiceCommand>,
    shutdown_sender: watch::Sender<bool>,
    event_receiver: mpsc::Receiver<VoiceEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl VoiceService {
    /// Starts the provider and audio actor. Any startup failure is reported as
    /// a `VoiceEvent` so the TUI event loop remains the sole state owner.
    pub fn start(
        config: VoiceRuntimeConfig,
        tools: Vec<VoiceToolDefinition>,
    ) -> Result<Self, VoiceError> {
        config
            .validate()
            .map_err(VoiceError::InvalidConfiguration)?;

        let (command_sender, command_receiver) = mpsc::channel(64);
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);
        let (event_sender, event_receiver) = mpsc::channel(128);
        let task = tokio::spawn(async move {
            if let Err(error) = openai::run_session(
                config,
                tools,
                command_receiver,
                shutdown_receiver,
                event_sender.clone(),
            )
            .await
            {
                let _ = event_sender
                    .send(VoiceEvent::StateChanged(VoiceConnectionState::Failed(
                        error.to_string(),
                    )))
                    .await;
            }
        });

        Ok(Self {
            command_sender,
            shutdown_sender,
            event_receiver,
            task,
        })
    }

    pub fn command_sender(&self) -> mpsc::Sender<VoiceCommand> {
        self.command_sender.clone()
    }

    pub async fn next_event(&mut self) -> Option<VoiceEvent> {
        self.event_receiver.recv().await
    }

    pub fn try_next_event(&mut self) -> Option<VoiceEvent> {
        self.event_receiver.try_recv().ok()
    }

    /// Stops the actor and waits for every owned stream and task to drop.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_sender.send(true);
        let _ = (&mut self.task).await;
    }
}

impl Drop for VoiceService {
    fn drop(&mut self) {
        self.task.abort();
    }
}
