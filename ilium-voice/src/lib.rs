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
    /// Writes final function results, suppresses a follow-up response, and
    /// closes the provider/audio session in that exact order.
    SubmitToolOutputsAndShutdown(Vec<VoiceToolOutput>),
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
                tracing::error!(%error, "OpenAI Realtime voice session failed");
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
        // A `JoinError` here means the session task panicked (it never
        // otherwise returns `Err`) or was cancelled out from under us -
        // either is a real fault that must not vanish silently.
        if let Err(join_error) = (&mut self.task).await {
            tracing::error!(%join_error, "voice session task did not exit cleanly");
        }
    }

    /// Completes a self-stop function call before ending the actor. Falling
    /// back to the ordinary shutdown signal covers an actor that already
    /// closed its command receiver after a provider or device failure.
    pub async fn shutdown_after_tool_outputs(mut self, outputs: Vec<VoiceToolOutput>) {
        if self
            .command_sender
            .send(VoiceCommand::SubmitToolOutputsAndShutdown(outputs))
            .await
            .is_err()
        {
            let _ = self.shutdown_sender.send(true);
        }
        // Same rationale as `shutdown`: surface a panicked/cancelled task
        // instead of discarding the `JoinError`.
        if let Err(join_error) = (&mut self.task).await {
            tracing::error!(%join_error, "voice session task did not exit cleanly");
        }
    }
}

impl Drop for VoiceService {
    fn drop(&mut self) {
        self.task.abort();
    }
}
