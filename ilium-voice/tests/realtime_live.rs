//! Explicit live OpenAI Realtime protocol smoke test.
//!
//! Run with `OPENAI_API_KEY=... cargo test -p ilium-voice --test realtime_live -- --ignored`.

use std::time::Duration;

use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceToolDefinition, VoiceToolOutput,
};
use secrecy::SecretString;
use serde_json::{json, Value};

fn live_config() -> VoiceRuntimeConfig {
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY is required");
    VoiceRuntimeConfig {
        api_key: SecretString::from(api_key),
        model: VoiceModel::GptRealtimeMini,
        voice: VoiceName::Marin,
        reasoning_effort: ReasoningEffort::Low,
        input_mode: VoiceInputMode::PushToTalk,
        vad_eagerness: VadEagerness::Auto,
        input_device_name: None,
        output_device_name: None,
        output_volume_percent: 0,
        instructions: "For the live smoke test, always call ilium_echo exactly once when asked. Do not answer before calling it. After the tool succeeds, briefly acknowledge its result.".to_owned(),
    }
}

fn echo_tool() -> VoiceToolDefinition {
    VoiceToolDefinition {
        name: "ilium_echo".to_owned(),
        description: "Echo one short value for a protocol smoke test.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
            "additionalProperties": false,
        }),
    }
}

fn stop_voice_tool() -> VoiceToolDefinition {
    VoiceToolDefinition {
        name: "ilium_stop_voice_mode".to_owned(),
        description: "Immediately stop and disable the current ilium voice mode when the user asks to stop, disable, turn off, exit, or end voice mode. This ends the voice session, so do not merely acknowledge the request in speech.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        }),
    }
}

async fn wait_until_listening(service: &mut VoiceService) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Listening) => break,
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime startup failed: {error}")
                }
                VoiceEvent::ProviderError(error) => panic!("Realtime startup error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime session should become ready");
}

#[tokio::test]
#[ignore = "uses the configured microphone/speaker and the live OpenAI API"]
async fn realtime_session_configuration_is_acknowledged_and_shuts_down() {
    let mut service =
        VoiceService::start(live_config(), vec![echo_tool()]).expect("start voice actor");

    wait_until_listening(&mut service).await;
    service.shutdown().await;
}

#[tokio::test]
#[ignore = "uses the configured microphone/speaker and the live OpenAI API"]
async fn realtime_session_connects_calls_a_tool_accepts_its_result_and_shuts_down() {
    let mut service =
        VoiceService::start(live_config(), vec![echo_tool()]).expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Call ilium_echo now with the value live-tool-proof.".to_owned(),
        ))
        .await
        .expect("send deterministic text turn");

    let invocations = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::ToolInvocations(invocations) => break invocations,
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime tool call failed: {error}")
                }
                VoiceEvent::ProviderError(error) => panic!("Realtime tool-call error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime model should call the registered tool");
    assert_eq!(invocations.len(), 1);
    let invocation = invocations.into_iter().next().expect("one invocation");
    assert_eq!(invocation.name, "ilium_echo");
    assert!(invocation.arguments_json.contains("live-tool-proof"));

    sender
        .send(VoiceCommand::SubmitToolOutputs(vec![VoiceToolOutput {
            call_id: invocation.call_id,
            result: json!({ "status": "ok", "echoed": "live-tool-proof" }),
            request_follow_up: true,
            terminate_session_after_delivery: false,
        }]))
        .await
        .expect("submit live tool output");

    tokio::time::timeout(Duration::from_secs(30), async {
        let mut is_follow_up_active = false;
        let mut assistant_transcript = String::new();
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking) => {
                    is_follow_up_active = true;
                }
                VoiceEvent::AssistantTranscript(delta) if is_follow_up_active => {
                    assistant_transcript.push_str(&delta);
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Listening)
                    if is_follow_up_active && !assistant_transcript.trim().is_empty() =>
                {
                    break;
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime tool-result follow-up failed: {error}")
                }
                VoiceEvent::ProviderError(error) => {
                    panic!("Realtime rejected the tool result: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime should accept the tool result and complete its follow-up");

    service.shutdown().await;
}

#[tokio::test]
#[ignore = "uses the configured microphone/speaker and the live OpenAI API"]
async fn realtime_self_stop_returns_its_tool_result_before_shutdown() {
    let mut config = live_config();
    config.instructions = "When the user asks to stop voice mode, call ilium_stop_voice_mode with an empty object immediately. Do not speak instead because the tool ends the session.".to_owned();
    let mut service =
        VoiceService::start(config, vec![stop_voice_tool()]).expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText("Stop voice mode now.".to_owned()))
        .await
        .expect("send deterministic stop turn");

    let invocation = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::ToolInvocations(invocations) => {
                    assert_eq!(invocations.len(), 1);
                    break invocations.into_iter().next().expect("one invocation");
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime self-stop tool call failed: {error}")
                }
                VoiceEvent::ProviderError(error) => {
                    panic!("Realtime self-stop tool-call error: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime model should call the self-stop tool");
    assert_eq!(invocation.name, "ilium_stop_voice_mode");
    assert_eq!(
        serde_json::from_str::<Value>(&invocation.arguments_json).expect("valid stop arguments"),
        json!({})
    );

    sender
        .send(VoiceCommand::SubmitToolOutputsAndShutdown(vec![
            VoiceToolOutput {
                call_id: invocation.call_id,
                result: json!({ "status": "ok", "message": "Voice mode stopped" }),
                request_follow_up: false,
                terminate_session_after_delivery: true,
            },
        ]))
        .await
        .expect("submit final stop tool output");

    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Disabled) => break,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking) => {
                    panic!("self-stop must not create a follow-up response")
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime self-stop shutdown failed: {error}")
                }
                VoiceEvent::ProviderError(error) => {
                    panic!("Realtime rejected the self-stop result: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime actor should stop after writing the tool result");

    service.shutdown().await;
}
