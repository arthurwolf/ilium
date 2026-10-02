//! An explicitly started, owned Realtime demo with one isolated capability.
//! This adapter never constructs a ControlPlane or observes terminal context.

use ilium_voice::{
    VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode, VoiceRuntimeConfig,
    VoiceService, VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput,
};

use super::voice_demo::{self, VoiceDemo};
use crate::config::VoiceSettings;

const MAX_TRANSCRIPT_CHARS: usize = 2048;
const MAX_TEXT_CHARS: usize = 1024;

#[derive(Debug, Clone)]
pub struct VoiceDemoState {
    pub connection: VoiceConnectionState,
    pub bulb_on: bool,
    pub acknowledged_calls: u64,
    pub last_tool_status: Option<String>,
    pub user_transcript: String,
    pub assistant_transcript: String,
    pub is_running: bool,
}

impl Default for VoiceDemoState {
    fn default() -> Self {
        Self {
            connection: VoiceConnectionState::Disabled,
            bulb_on: false,
            acknowledged_calls: 0,
            last_tool_status: None,
            user_transcript: String::new(),
            assistant_transcript: String::new(),
            is_running: false,
        }
    }
}

/// Owned by the central client event loop; `next_event` can be a select branch.
/// No additional task is spawned beyond the existing VoiceService actor.
#[derive(Default)]
pub struct VoiceDemoRuntime {
    pub state: VoiceDemoState,
    service: Option<VoiceService>,
    started_settings: Option<VoiceSettings>,
    redaction_key: String,
    demo: VoiceDemo,
}

pub fn tool_definitions() -> Vec<VoiceToolDefinition> {
    vec![VoiceToolDefinition {
        name: voice_demo::TOOL_NAME.into(),
        description: "Set the onboarding demonstration lightbulb on or off. This controls only the simulated bulb.".into(),
        parameters: voice_demo::tool_schema(),
    }]
}

/// Reuses the configured devices, voice, model and turn policy. The additive
/// real-control prompt is deliberately excluded from this capability sandbox.
pub fn runtime_config(settings: &VoiceSettings) -> Result<VoiceRuntimeConfig, String> {
    let instructions = ilium_prompts::render("voice/onboarding-lightbulb", &serde_json::json!({}))
        .map_err(|error| format!("Could not prepare the voice demonstration: {error}"))?;
    Ok(config_with_instructions(settings, instructions))
}

fn config_with_instructions(settings: &VoiceSettings, instructions: String) -> VoiceRuntimeConfig {
    let api_key = if settings.api_key.trim().is_empty() {
        std::env::var("OPENAI_API_KEY").unwrap_or_default()
    } else {
        settings.api_key.clone()
    };
    VoiceRuntimeConfig {
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
    /// Only the explicit Test/Retry action calls this. `enabled` is the future
    /// real-control preference; it does not grant permission to start this demo.
    pub async fn start_test(&mut self, settings: &VoiceSettings) -> Result<(), String> {
        self.shutdown().await;
        self.demo = VoiceDemo::default();
        self.state = VoiceDemoState::default();
        self.redaction_key = if settings.api_key.trim().is_empty() {
            std::env::var("OPENAI_API_KEY").unwrap_or_default()
        } else {
            settings.api_key.clone()
        };
        let config = match runtime_config(settings) {
            Ok(config) => config,
            Err(error) => {
                self.fail(error.clone());
                return Err(error);
            }
        };
        if let Err(error) = config.validate() {
            self.fail(error.clone());
            return Err(error);
        }
        if tokio::runtime::Handle::try_current().is_err() {
            let error = "The voice test needs the client's async runtime".to_owned();
            self.fail(error.clone());
            return Err(error);
        }
        match VoiceService::start(config, tool_definitions()) {
            Ok(service) => {
                self.service = Some(service);
                self.started_settings = Some(settings.clone());
                self.state.connection = VoiceConnectionState::Connecting;
                self.state.is_running = true;
                Ok(())
            }
            Err(error) => {
                let error = error.to_string();
                self.fail(error.clone());
                Err(error)
            }
        }
    }

    /// Call after navigation/settings changes, before accepting more events.
    /// A configuration edit stops the test; it never silently opens a new mic.
    pub async fn reconcile(&mut self, settings: &VoiceSettings, is_voice_step: bool) {
        if !is_voice_step {
            self.shutdown().await;
            return;
        }
        if self
            .started_settings
            .as_ref()
            .is_some_and(|started| !started.has_same_runtime_configuration(settings))
        {
            self.shutdown().await;
            self.state.last_tool_status = Some("Settings changed. Select Test to use them.".into());
        }
    }

    pub async fn next_event(&mut self) -> Option<VoiceEvent> {
        match &mut self.service {
            Some(service) => service.next_event().await,
            None => std::future::pending().await,
        }
    }

    /// Execute only the local bulb tool, then queue its real receipt back to
    /// the same provider actor. Queue acceptance is not a provider-delivery ack.
    pub async fn handle_event(&mut self, event: VoiceEvent) {
        match event {
            VoiceEvent::StateChanged(state) => {
                if matches!(state, VoiceConnectionState::Thinking) {
                    self.state.assistant_transcript.clear();
                }
                self.state.connection = match state {
                    VoiceConnectionState::Failed(error) => {
                        VoiceConnectionState::Failed(self.redact(&error))
                    }
                    other => other,
                };
            }
            VoiceEvent::UserTranscript(text) => {
                self.state.user_transcript = bounded(&self.redact(&text), MAX_TRANSCRIPT_CHARS);
            }
            VoiceEvent::AssistantTranscript(delta) => {
                let delta = self.redact(&delta);
                self.state.assistant_transcript.push_str(&delta);
                self.state.assistant_transcript =
                    bounded(&self.state.assistant_transcript, MAX_TRANSCRIPT_CHARS);
            }
            VoiceEvent::ProviderError(error) => {
                self.state.connection = VoiceConnectionState::Failed(self.redact(&error));
            }
            VoiceEvent::ToolInvocations(invocations) => {
                let outputs = execute_invocations(&mut self.demo, &mut self.state, invocations);
                if !outputs.is_empty() {
                    if let Err(error) = self.send(VoiceCommand::SubmitToolOutputs(outputs)).await {
                        self.fail(error);
                    }
                }
            }
        }
        if let VoiceConnectionState::Failed(error) = &self.state.connection {
            let error = error.clone();
            self.shutdown().await;
            self.fail(error);
        }
    }

    /// Handle the `None` result from the active event branch once, then remove
    /// the actor so the event loop cannot busy-spin on a closed channel.
    pub async fn channel_closed(&mut self) {
        let failure = match &self.state.connection {
            VoiceConnectionState::Failed(error) => error.clone(),
            _ => "The voice test stopped unexpectedly. Select Test to retry.".into(),
        };
        self.shutdown().await;
        self.fail(failure);
    }

    pub async fn push_to_talk(&mut self, is_pressed: bool) -> Result<(), String> {
        if self
            .started_settings
            .as_ref()
            .is_none_or(|settings| settings.input_mode != VoiceInputMode::PushToTalk)
        {
            return Err("Choose Push to talk before recording a manual turn".into());
        }
        self.send(if is_pressed {
            VoiceCommand::StartPushToTalk
        } else {
            VoiceCommand::StopPushToTalk
        })
        .await
    }

    /// Optional text/accessibility seam uses the actual provider conversation.
    /// It does not invent recognition or a successful microphone recording.
    pub async fn send_text(&mut self, text: &str) -> Result<(), String> {
        if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
            return Err("Enter 1–1024 characters for the voice test".into());
        }
        self.send(VoiceCommand::SendText(text.to_owned())).await
    }

    pub async fn shutdown(&mut self) {
        if let Some(service) = self.service.take() {
            // Timeout drops the shutdown future's owned VoiceService, whose
            // Drop implementation aborts its actor. No pre-existing service
            // is touched, and the TUI cannot hang indefinitely during exit.
            if tokio::time::timeout(std::time::Duration::from_secs(5), service.shutdown())
                .await
                .is_err()
            {
                self.state.last_tool_status =
                    Some("Voice cleanup timed out; the owned actor was cancelled.".into());
            }
        }
        self.started_settings = None;
        self.state.is_running = false;
        self.state.connection = VoiceConnectionState::Disabled;
    }

    async fn send(&self, command: VoiceCommand) -> Result<(), String> {
        let Some(service) = &self.service else {
            return Err("Select Test to start the voice demonstration".into());
        };
        service
            .command_sender()
            .send(command)
            .await
            .map_err(|_| "The voice test disconnected. Select Test to retry.".into())
    }

    fn fail(&mut self, error: String) {
        self.state.connection = VoiceConnectionState::Failed(bounded(&error, MAX_TRANSCRIPT_CHARS));
    }

    fn redact(&self, text: &str) -> String {
        let configured_key = self
            .started_settings
            .as_ref()
            .map(|settings| settings.api_key.trim())
            .unwrap_or("");
        let key = if self.redaction_key.trim().is_empty() {
            configured_key
        } else {
            self.redaction_key.trim()
        };
        if key.is_empty() {
            text.into()
        } else {
            text.replace(key, "[REDACTED]")
        }
    }
}

fn bounded(text: &str, maximum: usize) -> String {
    text.chars().take(maximum).collect()
}

fn execute_invocations(
    demo: &mut VoiceDemo,
    state: &mut VoiceDemoState,
    invocations: Vec<VoiceToolInvocation>,
) -> Vec<VoiceToolOutput> {
    invocations.into_iter().map(|invocation| {
        let receipt = demo.execute(&invocation.call_id, &invocation.name, &invocation.arguments_json);
        state.bulb_on = demo.on;
        if receipt.ok {
            state.acknowledged_calls = state.acknowledged_calls.saturating_add(1);
            state.last_tool_status = Some(format!("Tool executed · light {}", if demo.on { "on" } else { "off" }));
        } else {
            state.last_tool_status = receipt.error.clone();
        }
        VoiceToolOutput {
            call_id: invocation.call_id,
            result: serde_json::json!({"ok": receipt.ok, "on": receipt.on, "error": receipt.error}),
            request_follow_up: true,
            terminate_session_after_delivery: false,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(id: &str, name: &str, arguments: &str) -> VoiceToolInvocation {
        VoiceToolInvocation {
            call_id: id.into(),
            name: name.into(),
            arguments_json: arguments.into(),
        }
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
    async fn invalid_test_configuration_never_starts_audio_and_lifecycle_errors_survive_cleanup() {
        let mut runtime = VoiceDemoRuntime::default();
        let settings = VoiceSettings {
            api_key: "fixture-secret".into(),
            output_volume_percent: 101,
            ..VoiceSettings::default()
        };
        assert!(runtime.start_test(&settings).await.is_err());
        assert!(!runtime.state.is_running);
        assert!(runtime.service.is_none());
        assert!(runtime.send_text("").await.is_err());
        assert!(runtime.send_text("turn on").await.is_err());
        assert!(runtime.push_to_talk(true).await.is_err());
        runtime
            .handle_event(VoiceEvent::UserTranscript("灯".repeat(3000)))
            .await;
        assert_eq!(
            runtime.state.user_transcript.chars().count(),
            MAX_TRANSCRIPT_CHARS
        );
        runtime
            .handle_event(VoiceEvent::AssistantTranscript("old response".into()))
            .await;
        runtime
            .handle_event(VoiceEvent::StateChanged(VoiceConnectionState::Thinking))
            .await;
        assert!(runtime.state.assistant_transcript.is_empty());
        runtime
            .handle_event(VoiceEvent::ProviderError("failed fixture-secret".into()))
            .await;
        assert!(!runtime.state.is_running);
        assert!(runtime.service.is_none());
        runtime.channel_closed().await;
        assert_eq!(
            runtime.state.connection,
            VoiceConnectionState::Failed("failed [REDACTED]".into())
        );
        runtime.reconcile(&settings, false).await;
        assert_eq!(runtime.state.connection, VoiceConnectionState::Disabled);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), runtime.next_event())
                .await
                .is_err()
        );
    }

    #[test]
    fn only_actual_tool_execution_changes_light_and_receipts_are_protocol_ready() {
        let mut demo = VoiceDemo::default();
        let mut state = VoiceDemoState::default();
        let outputs = execute_invocations(
            &mut demo,
            &mut state,
            vec![invocation("1", voice_demo::TOOL_NAME, r#"{"on":true}"#)],
        );
        assert!(state.bulb_on);
        assert_eq!(outputs[0].result["on"], true);
        let outputs = execute_invocations(
            &mut demo,
            &mut state,
            vec![
                invocation("2", "create_pane", "{}"),
                invocation("3", voice_demo::TOOL_NAME, r#"{"on":false}"#),
            ],
        );
        assert_eq!(outputs[0].result["ok"], false);
        assert!(!state.bulb_on);
        assert_eq!(state.acknowledged_calls, 2);
        assert!(outputs
            .iter()
            .all(|output| !output.terminate_session_after_delivery));
    }

    #[test]
    fn duplicate_old_receipt_never_relights_current_bulb() {
        let mut demo = VoiceDemo::default();
        let mut state = VoiceDemoState::default();
        execute_invocations(
            &mut demo,
            &mut state,
            vec![
                invocation("1", voice_demo::TOOL_NAME, r#"{"on":true}"#),
                invocation("2", voice_demo::TOOL_NAME, r#"{"on":false}"#),
            ],
        );
        let replay = execute_invocations(
            &mut demo,
            &mut state,
            vec![invocation("1", voice_demo::TOOL_NAME, r#"{"on":true}"#)],
        );
        assert_eq!(replay[0].result["on"], true);
        assert!(!state.bulb_on);
    }

    #[test]
    fn transcript_boundaries_preserve_utf8_and_credentials_are_redacted() {
        assert_eq!(bounded("é灯é灯", 3), "é灯é");
        let runtime = VoiceDemoRuntime {
            started_settings: Some(VoiceSettings {
                api_key: "fixture-secret".into(),
                ..VoiceSettings::default()
            }),
            ..VoiceDemoRuntime::default()
        };
        assert_eq!(runtime.redact("error fixture-secret"), "error [REDACTED]");
    }
}
