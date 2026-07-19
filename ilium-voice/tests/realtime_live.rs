//! Explicit live OpenAI Realtime protocol smoke test.
//!
//! Run with `OPENAI_API_KEY=... cargo test -p ilium-voice --test realtime_live -- --ignored`.

use std::time::Duration;

use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceToolDefinition, VoiceToolOutput,
};
use secrecy::SecretString;
use serde_json::json;

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
