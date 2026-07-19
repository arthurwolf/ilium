//! Live OpenAI Realtime evaluation for ilium's actual control prompt/tools.
//!
//! Run with `OPENAI_API_KEY=... cargo test -p ilium-client --test voice_control_live -- --ignored`.

use std::path::PathBuf;
use std::time::Duration;

use ilium_client::app::{App, VoiceRuntimeRequest};
use ilium_client::control::{system_instructions, ControlPlane};
use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceToolOutput,
};
use serde_json::{json, Value};

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
async fn wait_for_tool_invocation(
    service: &mut VoiceService,
) -> (ilium_voice::VoiceToolInvocation, String) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut assistant_transcript = String::new();

        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::ToolInvocations(invocations) => {
                    assert_eq!(invocations.len(), 1, "one dictated action is one tool call");

                    break (
                        invocations.into_iter().next().expect("one invocation"),
                        assistant_transcript,
                    );
                }
                VoiceEvent::AssistantTranscript(delta) => assistant_transcript.push_str(&delta),
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime tool call failed: {error}")
                }
                VoiceEvent::ProviderError(error) => panic!("Realtime tool-call error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime model should call one production tool")
}

/// Captures the spoken confirmation question through its return to Listening.
async fn wait_for_confirmation_question(service: &mut VoiceService) -> String {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut assistant_transcript = String::new();
        let mut is_response_active = false;

        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking) => {
                    is_response_active = true;
                }
                VoiceEvent::AssistantTranscript(delta) if is_response_active => {
                    assistant_transcript.push_str(&delta);
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Listening)
                    if is_response_active && !assistant_transcript.trim().is_empty() =>
                {
                    break assistant_transcript;
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime confirmation failed: {error}")
                }
                VoiceEvent::ProviderError(error) => {
                    panic!("Realtime confirmation error: {error}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime model should ask the confirmation question")
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn explicit_enter_phrase_calls_the_dedicated_submission_tool() {
    let control_plane = ControlPlane::default();
    let mut service = VoiceService::start(live_config(), control_plane.tool_definitions())
        .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Send /clear to the currently open terminal, followed by Enter.".to_owned(),
        ))
        .await
        .expect("send deterministic text turn");

    let (invocation, pre_tool_transcript) = wait_for_tool_invocation(&mut service).await;
    assert_eq!(invocation.name, "ilium_send_to_terminal");
    assert!(
        pre_tool_transcript.trim().is_empty(),
        "dictation should call the tool without a spoken preamble"
    );

    let arguments: Value =
        serde_json::from_str(&invocation.arguments_json).expect("valid tool arguments");
    assert_eq!(arguments["text"], "/clear");
    assert!(
        arguments.get("send_enter").is_none(),
        "the submission tool owns the final Enter instead of delegating it to the model"
    );
    assert!(
        arguments.get("target").is_none() || arguments["target"].is_null(),
        "the active pane should be selected by omitting target"
    );

    let confirmation_question =
        "I typed it into the target terminal without pressing Enter. Send what you see on screen?";
    sender
        .send(VoiceCommand::SubmitToolOutputs(vec![VoiceToolOutput {
            call_id: invocation.call_id,
            result: json!({
                "status": "confirmation_required",
                "token": "voice-confirm-live",
                "question": confirmation_question,
                "preparation": {
                    "status": "queued",
                    "message": "Sent text to the terminal",
                },
                "instruction": "Ask only the exact question. Do not read or repeat staged terminal text. Call ilium_confirm_action only after an explicit yes or no answer.",
            }),
            request_follow_up: true,
            terminate_session_after_delivery: false,
        }]))
        .await
        .expect("submit staged-terminal result");

    let spoken_question = wait_for_confirmation_question(&mut service).await;
    assert_eq!(spoken_question.trim(), confirmation_question);
    assert!(!spoken_question.contains("/clear"));

    sender
        .send(VoiceCommand::SendText("Yes.".to_owned()))
        .await
        .expect("send explicit confirmation");

    let (confirmation, pre_confirmation_transcript) = wait_for_tool_invocation(&mut service).await;
    assert_eq!(confirmation.name, "ilium_confirm_action");
    assert!(pre_confirmation_transcript.trim().is_empty());

    let confirmation_arguments: Value =
        serde_json::from_str(&confirmation.arguments_json).expect("valid confirmation arguments");
    assert_eq!(confirmation_arguments["token"], "voice-confirm-live");
    assert_eq!(confirmation_arguments["confirmed"], true);

    service.shutdown().await;
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn stop_voice_request_calls_the_dedicated_tool_and_ends_the_session() {
    let mut control_plane = ControlPlane::default();
    let mut service = VoiceService::start(live_config(), control_plane.tool_definitions())
        .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText("Stop voice mode now.".to_owned()))
        .await
        .expect("send deterministic stop turn");

    let (invocation, pre_tool_transcript) = wait_for_tool_invocation(&mut service).await;
    assert_eq!(invocation.name, "ilium_stop_voice_mode");
    assert!(pre_tool_transcript.trim().is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&invocation.arguments_json).expect("valid stop arguments"),
        json!({})
    );

    let mut app = App::new("voice-live".to_owned(), PathBuf::from("/tmp/project"));
    app.voice_settings.enabled = true;
    let output = control_plane.execute_invocation(&mut app, invocation);

    assert!(!output.request_follow_up);
    assert!(output.terminate_session_after_delivery);
    assert!(!app.voice_settings.enabled);
    assert_eq!(
        app.take_voice_runtime_request(),
        Some(VoiceRuntimeRequest::Stop)
    );

    sender
        .send(VoiceCommand::SubmitToolOutputsAndShutdown(vec![output]))
        .await
        .expect("submit final stop result");

    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match service.next_event().await.expect("voice event channel") {
                VoiceEvent::StateChanged(VoiceConnectionState::Disabled) => break,
                VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) => {
                    panic!("Realtime self-stop failed: {error}")
                }
                VoiceEvent::ProviderError(error) => panic!("Realtime self-stop error: {error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("Realtime voice actor should stop after returning the tool result");

    service.shutdown().await;
}
