//! Live OpenAI Realtime evaluation for ilium's actual control prompt/tools.
//!
//! Run with `OPENAI_API_KEY=... cargo test -p ilium-client --test voice_control_live -- --ignored`.

use std::time::Duration;

use ilium_client::control::{system_instructions, ControlPlane};
use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService,
};
use serde_json::Value;

/// Uses the production prompt and tool definitions while muting audio output.
fn live_config() -> VoiceRuntimeConfig {
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY is required");

    VoiceRuntimeConfig {
        api_key: api_key.into(),
        model: VoiceModel::GptRealtime21,
        voice: VoiceName::Marin,
        reasoning_effort: ReasoningEffort::Low,
        input_mode: VoiceInputMode::PushToTalk,
        vad_eagerness: VadEagerness::Auto,
        input_device_name: None,
        output_device_name: None,
        output_volume_percent: 0,
        instructions: system_instructions(""),
    }
}

/// Waits for the production Realtime session to accept its configuration.
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

/// Waits for the first model-selected production tool invocation.
async fn wait_for_tool_invocation(service: &mut VoiceService) -> ilium_voice::VoiceToolInvocation {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::ToolInvocations(invocations) => {
                    assert_eq!(invocations.len(), 1, "one dictated action is one tool call");

                    break invocations.into_iter().next().expect("one invocation");
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime tool call failed: {error}")
                }
                VoiceEvent::ProviderError(error) => panic!("Realtime tool-call error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime model should call the terminal-submission tool")
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn exact_focused_terminal_phrase_calls_the_dedicated_submission_tool() {
    let control_plane = ControlPlane::default();
    let mut service = VoiceService::start(live_config(), control_plane.tool_definitions())
        .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Send /clear to the currently open terminal.".to_owned(),
        ))
        .await
        .expect("send deterministic text turn");

    let invocation = wait_for_tool_invocation(&mut service).await;
    assert_eq!(invocation.name, "ilium_send_to_terminal");

    let arguments: Value =
        serde_json::from_str(&invocation.arguments_json).expect("valid tool arguments");
    assert_eq!(arguments["text"], "/clear");
    assert_eq!(arguments["send_enter"], true);
    assert!(
        arguments.get("target").is_none() || arguments["target"].is_null(),
        "the active pane should be selected by omitting target"
    );

    service.shutdown().await;
}
