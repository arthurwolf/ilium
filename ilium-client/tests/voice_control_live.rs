//! Live OpenAI Realtime evaluation for ilium's actual control prompt/tools.
//!
//! Run with `OPENAI_API_KEY=... cargo test -p ilium-client --test voice_control_live -- --ignored`.

use ilium_execution::{QuotaGroup, StorageAdmission};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ilium_client::app::{App, VoiceRuntimeRequest};
use ilium_client::control::{system_instructions, ControlPlane, VoiceTargetContext};
use ilium_voice::{
    ReasoningEffort, VadEagerness, VoiceCommand, VoiceConnectionState, VoiceEvent, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceToolOutput,
};
use serde_json::{json, Value};

/// Uses the production prompt and tool definitions while muting audio output.
fn live_config(target_context: VoiceTargetContext) -> VoiceRuntimeConfig {
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
        instructions: system_instructions("", target_context),
    }
}

#[derive(Debug)]
struct FixtureSourceAllocation {
    _allocation: StorageAdmission,
}
impl ilium_voice::VoiceTextAllocation for FixtureSourceAllocation {}

fn start_voice_fixture(
    config: VoiceRuntimeConfig,
    tools: Vec<ilium_voice::VoiceToolDefinition>,
    quota: QuotaGroup,
) -> Result<VoiceService, ilium_voice::VoiceError> {
    let retained_bytes = ilium_voice::runtime_capture_bytes(&config, &tools).ok_or_else(|| {
        ilium_voice::VoiceError::InvalidConfiguration("fixture startup layout overflow".into())
    })?;
    let allocation = quota
        .reserve_external_storage(retained_bytes)
        .map_err(|reason| {
            ilium_voice::VoiceError::AudioPreparation(format!(
                "fixture source admission: {reason:?}"
            ))
        })?;
    let startup = ilium_voice::OwnedVoiceStartup::charged(
        config,
        tools,
        retained_bytes,
        Arc::new(FixtureSourceAllocation {
            _allocation: allocation,
        }),
    )?;
    let admission = VoiceService::admit_startup(quota).map_err(|reason| {
        ilium_voice::VoiceError::AudioPreparation(format!("fixture metadata admission: {reason:?}"))
    })?;
    Ok(VoiceService::start(startup, admission))
}

fn charged_context(
    instructions: String,
    tools: Vec<ilium_voice::VoiceToolDefinition>,
    quota: &QuotaGroup,
) -> ilium_voice::OwnedVoiceContext {
    let retained_bytes = ilium_voice::context_capture_bytes(&instructions, &tools)
        .and_then(|bytes| bytes.checked_add(instructions.capacity()))
        .and_then(|bytes| {
            bytes.checked_add(
                tools
                    .capacity()
                    .checked_mul(std::mem::size_of::<ilium_voice::VoiceToolDefinition>())?,
            )
        })
        .expect("fixture context layout");
    let allocation = quota
        .reserve_external_storage(retained_bytes)
        .expect("admit original context source and transport");
    ilium_voice::OwnedVoiceContext::charged(
        instructions,
        tools,
        retained_bytes,
        Arc::new(FixtureSourceAllocation {
            _allocation: allocation,
        }),
    )
    .expect("charged production context")
}

/// Waits for the production Realtime session to accept its configuration.
async fn wait_until_listening(service: &mut VoiceService) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let receipt = service.next_event().await.expect("voice event channel");
            match receipt.event() {
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
    quota: &QuotaGroup,
) -> (
    ilium_voice::VoiceToolInvocation,
    ChargedTranscript,
    Vec<Arc<StorageAdmission>>,
) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut assistant_transcript = ChargedTranscript::default();
        let mut allocations = Vec::new();

        loop {
            let receipt = service.next_event().await.expect("voice event channel");
            let (event, allocation) = receipt.into_parts();
            allocations.push(allocation);
            match event {
                VoiceEvent::ToolInvocations(invocations) => {
                    assert_eq!(invocations.len(), 1, "one dictated action is one tool call");

                    break (
                        invocations.into_iter().next().expect("one invocation"),
                        assistant_transcript,
                        allocations,
                    );
                }
                VoiceEvent::AssistantTranscript(delta) => {
                    assistant_transcript.append(quota, &delta)
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
    .expect("Realtime model should call one production tool")
}

/// Captures the spoken confirmation question through its return to Listening.
async fn wait_for_confirmation_question(
    service: &mut VoiceService,
    quota: &QuotaGroup,
) -> (ChargedTranscript, Vec<Arc<StorageAdmission>>) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut assistant_transcript = ChargedTranscript::default();
        let mut allocations = Vec::new();
        let mut is_response_active = false;

        loop {
            let receipt = service.next_event().await.expect("voice event channel");
            let (event, allocation) = receipt.into_parts();
            allocations.push(allocation);
            match event {
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking) => {
                    is_response_active = true;
                }
                VoiceEvent::AssistantTranscript(delta) if is_response_active => {
                    assistant_transcript.append(quota, &delta);
                }
                VoiceEvent::StateChanged(VoiceConnectionState::Listening)
                    if is_response_active && !assistant_transcript.trim().is_empty() =>
                {
                    break (assistant_transcript, allocations);
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
    let quota = test_quota();
    let control_plane = ControlPlane::default();
    let mut service = start_voice_fixture(
        live_config(VoiceTargetContext::NoDetectedAgent),
        control_plane.tool_definitions(),
        quota.clone(),
    )
    .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Send /clear to the currently open terminal, followed by Enter."
                .to_owned()
                .into(),
        ))
        .await
        .expect("send deterministic text turn");

    let (invocation, pre_tool_transcript, _invocation_source_hold) =
        wait_for_tool_invocation(&mut service, &quota).await;
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
            result: std::sync::Arc::new(json!({
                "status": "confirmation_required",
                "token": "voice-confirm-live",
                "question": confirmation_question,
                "preparation": {
                    "status": "queued",
                    "message": "Sent text to the terminal",
                },
                "instruction": "Ask only the exact question. Do not read or repeat staged terminal text. Call ilium_confirm_action only after an explicit yes or no answer.",
            })),
            request_follow_up: true,
            terminate_session_after_delivery: false,
            // This standalone provider fixture does not use the application allocation bank.
            allocation_hold: None,
            retained_bytes: 0,
        }]))
        .await
        .expect("submit staged-terminal result");

    let (spoken_question, _spoken_source_hold) =
        wait_for_confirmation_question(&mut service, &quota).await;
    assert_eq!(spoken_question.trim(), confirmation_question);
    assert!(!spoken_question.contains("/clear"));

    sender
        .send(VoiceCommand::SendText("Yes.".to_owned().into()))
        .await
        .expect("send explicit confirmation");

    let (confirmation, pre_confirmation_transcript, _confirmation_source_hold) =
        wait_for_tool_invocation(&mut service, &quota).await;
    assert_eq!(confirmation.name, "ilium_confirm_action");
    assert!(pre_confirmation_transcript.trim().is_empty());

    let confirmation_arguments: Value =
        serde_json::from_str(&confirmation.arguments_json).expect("valid confirmation arguments");
    assert_eq!(confirmation_arguments["token"], "voice-confirm-live");
    assert_eq!(confirmation_arguments["confirmed"], true);

    shutdown_voice(service).await;
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn bare_utterance_is_forwarded_verbatim_after_agent_context_update() {
    let quota = test_quota();
    let control_plane = ControlPlane::default();
    let mut service = start_voice_fixture(
        live_config(VoiceTargetContext::NoDetectedAgent),
        control_plane.tool_definitions(),
        quota.clone(),
    )
    .expect("start voice actor");
    let sender = service.command_sender();
    let bare_utterance =
        "Change the voice fallback so unclear requests go to the current coding agent.";

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::UpdateContext(charged_context(
            system_instructions("", VoiceTargetContext::DetectedAgent),
            control_plane.tool_definitions(),
            &quota,
        )))
        .await
        .expect("update active-agent context");
    sender
        .send(VoiceCommand::SendText(bare_utterance.to_owned().into()))
        .await
        .expect("send bare agent-directed turn");

    let (invocation, pre_tool_transcript, _invocation_source_hold) =
        wait_for_tool_invocation(&mut service, &quota).await;
    assert_eq!(invocation.name, "ilium_send_to_terminal");
    assert!(
        pre_tool_transcript.trim().is_empty(),
        "agent-default dictation should call the tool without a spoken answer"
    );

    let arguments: Value =
        serde_json::from_str(&invocation.arguments_json).expect("valid tool arguments");
    assert_eq!(arguments["text"], bare_utterance);
    assert!(
        arguments.get("target").is_none() || arguments["target"].is_null(),
        "the active agent should be selected by omitting target"
    );

    shutdown_voice(service).await;
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn explicit_ilium_command_takes_priority_inside_agent_context() {
    let quota = test_quota();
    let control_plane = ControlPlane::default();
    let mut service = start_voice_fixture(
        live_config(VoiceTargetContext::DetectedAgent),
        control_plane.tool_definitions(),
        quota.clone(),
    )
    .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Open ilium's settings screen.".to_owned().into(),
        ))
        .await
        .expect("send explicit ilium-control turn");

    let (invocation, pre_tool_transcript, _invocation_source_hold) =
        wait_for_tool_invocation(&mut service, &quota).await;
    assert_eq!(invocation.name, "ilium_ui");
    assert!(
        pre_tool_transcript.trim().is_empty(),
        "explicit ilium control should call its tool without a spoken answer"
    );

    let arguments: Value =
        serde_json::from_str(&invocation.arguments_json).expect("valid tool arguments");
    assert!(matches!(
        arguments["action"].as_str(),
        Some("open_settings" | "show_settings_tab")
    ));

    shutdown_voice(service).await;
}

#[tokio::test]
#[ignore = "uses the live OpenAI Realtime API"]
async fn stop_voice_request_calls_the_dedicated_tool_and_ends_the_session() {
    let quota = test_quota();
    let mut control_plane = ControlPlane::default();
    let mut service = start_voice_fixture(
        live_config(VoiceTargetContext::NoDetectedAgent),
        control_plane.tool_definitions(),
        quota.clone(),
    )
    .expect("start voice actor");
    let sender = service.command_sender();

    wait_until_listening(&mut service).await;

    sender
        .send(VoiceCommand::SendText(
            "Stop voice mode now.".to_owned().into(),
        ))
        .await
        .expect("send deterministic stop turn");

    let (invocation, pre_tool_transcript, _invocation_source_hold) =
        wait_for_tool_invocation(&mut service, &quota).await;
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
            let receipt = service.next_event().await.expect("voice event channel");
            match receipt.event() {
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

    shutdown_voice(service).await;
}

#[derive(Default)]
struct ChargedTranscript {
    text: String,
    _allocation: Option<StorageAdmission>,
}
impl std::ops::Deref for ChargedTranscript {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}
impl ChargedTranscript {
    fn append(&mut self, quota: &QuotaGroup, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let length = self
            .text
            .len()
            .checked_add(delta.len())
            .expect("bounded transcript size");
        let allocation = quota
            .reserve_external_storage(length)
            .expect("admit transcript projection before clone");
        let mut text = String::with_capacity(length);
        text.push_str(&self.text);
        text.push_str(delta);
        self.text = text;
        self._allocation = Some(allocation);
    }
}
fn test_quota() -> QuotaGroup {
    QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 8,
        worker_bytes: 256 * 1024 * 1024,
    })
}
async fn shutdown_voice(service: VoiceService) {
    let mut outcome = service.shutdown().await;
    loop {
        for receipt in outcome.events.drain(..) {
            if let VoiceEvent::StateChanged(VoiceConnectionState::Failed(error)) = receipt.event() {
                panic!("shutdown actor failure: {error}");
            }
        }
        match outcome.state {
            ilium_voice::VoiceShutdownState::Pending(service) => {
                outcome = service.continue_shutdown().await
            }
            ilium_voice::VoiceShutdownState::Complete(exit) => {
                assert!(matches!(exit, ilium_voice::VoiceActorExit::Completed));
                assert!(outcome.undelivered_stop_outputs.is_none());
                assert!(outcome.undelivered_commands.is_empty());
                tokio::task::spawn_blocking(move || {
                    outcome
                        .audio_custody
                        .join_until(std::time::Instant::now() + Duration::from_secs(5))
                })
                .await
                .expect("audio retirement observer")
                .expect("actual audio owners joined");
                break;
            }
        }
    }
}
