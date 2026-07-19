//! OpenAI Realtime WebSocket adapter.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header, HeaderValue};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::audio::AudioEngine;
use crate::{
    VoiceCommand, VoiceConnectionState, VoiceError, VoiceEvent, VoiceInputMode, VoiceRuntimeConfig,
    VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput,
};

const REALTIME_ENDPOINT: &str = "wss://api.openai.com/v1/realtime";
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_RENEWAL_INTERVAL: Duration = Duration::from_secs(55 * 60);
const SESSION_CONFIGURATION_TIMEOUT: Duration = Duration::from_secs(15);
const SOCKET_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_REMEMBERED_TOOL_CALLS: usize = 1_024;

type RealtimeSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct SessionContext {
    instructions: String,
    tools: Vec<VoiceToolDefinition>,
}

/// Mutable state owned by one logical voice session across proactive
/// WebSocket renewals. Conversation-local playback flags reset per
/// connection, while context and call deduplication survive renewal.
struct SessionState {
    context: SessionContext,
    completed_call_ids: BoundedCallIdSet,
    playing_item: Option<PlayingItem>,
    is_response_active: bool,
}

impl SessionState {
    fn new(instructions: String, tools: Vec<VoiceToolDefinition>) -> Self {
        Self {
            context: SessionContext {
                instructions,
                tools,
            },
            completed_call_ids: BoundedCallIdSet::default(),
            playing_item: None,
            is_response_active: false,
        }
    }

    fn reset_connection_state(&mut self) {
        self.playing_item = None;
        self.is_response_active = false;
    }
}

#[derive(Debug, Clone)]
struct PlayingItem {
    item_id: String,
    content_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionExit {
    Renew,
    Shutdown,
}

/// Runs one owned audio pipeline across proactively renewed provider sessions.
pub(crate) async fn run_session(
    config: VoiceRuntimeConfig,
    tools: Vec<VoiceToolDefinition>,
    mut command_receiver: mpsc::Receiver<VoiceCommand>,
    mut shutdown_receiver: watch::Receiver<bool>,
    event_sender: mpsc::Sender<VoiceEvent>,
) -> Result<(), VoiceError> {
    let mut audio = AudioEngine::start(
        config.input_device_name.as_deref(),
        config.output_device_name.as_deref(),
        config.input_mode,
        config.output_volume_percent,
    )?;
    let mut state = SessionState::new(config.instructions.clone(), tools);

    loop {
        send_event(
            &event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Connecting),
        )
        .await;
        let mut socket = tokio::select! {
            result = connect_with_timeout(&config) => result?,
            _ = shutdown_receiver.changed() => {
                send_disabled(&event_sender).await;
                return Ok(());
            }
        };
        let configure_session = async {
            send_json(
                &mut socket,
                &session_update_payload(&config, &state.context.instructions, &state.context.tools),
            )
            .await?;
            await_session_updated(&mut socket).await
        };
        tokio::select! {
            result = configure_session => result?,
            _ = shutdown_receiver.changed() => {
                close_socket(&mut socket).await;
                send_disabled(&event_sender).await;
                return Ok(());
            }
        }
        send_event(
            &event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Listening),
        )
        .await;

        match run_connected_session(
            &config,
            &mut state,
            &mut socket,
            &mut audio,
            &mut command_receiver,
            &mut shutdown_receiver,
            &event_sender,
        )
        .await?
        {
            SessionExit::Renew => {
                close_socket(&mut socket).await;
            }
            SessionExit::Shutdown => {
                close_socket(&mut socket).await;
                send_disabled(&event_sender).await;
                return Ok(());
            }
        }
    }
}

/// Waits for the server's explicit acknowledgement instead of rendering a
/// false Listening state immediately after merely writing `session.update`.
async fn await_session_updated(socket: &mut RealtimeSocket) -> Result<(), VoiceError> {
    tokio::time::timeout(SESSION_CONFIGURATION_TIMEOUT, async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or(VoiceError::SessionEnded)?
                .map_err(VoiceError::Transport)?;
            if matches!(message, Message::Close(_)) {
                return Err(VoiceError::SessionEnded);
            }
            let Message::Text(text) = message else {
                continue;
            };
            let event: Value = serde_json::from_str(&text)?;
            match event.get("type").and_then(Value::as_str) {
                Some("session.updated") => return Ok(()),
                Some("error") => {
                    let message = event
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown session configuration error")
                        .to_owned();
                    return Err(VoiceError::SessionConfigurationRejected(message));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| VoiceError::SessionConfigurationTimeout)?
}

async fn run_connected_session(
    config: &VoiceRuntimeConfig,
    state: &mut SessionState,
    socket: &mut RealtimeSocket,
    audio: &mut AudioEngine,
    command_receiver: &mut mpsc::Receiver<VoiceCommand>,
    shutdown_receiver: &mut watch::Receiver<bool>,
    event_sender: &mpsc::Sender<VoiceEvent>,
) -> Result<SessionExit, VoiceError> {
    let renewal_timer = tokio::time::sleep(SESSION_RENEWAL_INTERVAL);
    tokio::pin!(renewal_timer);
    state.reset_connection_state();

    loop {
        tokio::select! {
            _ = &mut renewal_timer => return Ok(SessionExit::Renew),
            _ = shutdown_receiver.changed() => return Ok(SessionExit::Shutdown),
            command = command_receiver.recv() => {
                let command = command.ok_or(VoiceError::CommandChannelClosed)?;
                handle_command(
                    config,
                    state,
                    socket,
                    audio,
                    event_sender,
                    command,
                ).await?;
            }
            capture = audio.next_capture() => {
                let capture = capture.ok_or(VoiceError::SessionEnded)?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(capture.pcm16_le);
                send_json(socket, &json!({
                    "type": "input_audio_buffer.append",
                    "audio": encoded,
                })).await?;
            }
            message = socket.next() => {
                let message = message
                    .ok_or(VoiceError::SessionEnded)?
                    .map_err(VoiceError::Transport)?;
                if matches!(message, Message::Close(_)) {
                    return Ok(SessionExit::Renew);
                }
                let Message::Text(text) = message else {
                    continue;
                };
                let event: Value = serde_json::from_str(&text)?;
                handle_provider_event(
                    socket,
                    audio,
                    event_sender,
                    state,
                    &event,
                ).await?;
            }
        }
    }
}

async fn handle_command(
    config: &VoiceRuntimeConfig,
    state: &mut SessionState,
    socket: &mut RealtimeSocket,
    audio: &AudioEngine,
    event_sender: &mpsc::Sender<VoiceEvent>,
    command: VoiceCommand,
) -> Result<(), VoiceError> {
    match command {
        VoiceCommand::UpdateContext {
            instructions,
            tools,
        } => {
            state.context.instructions = instructions;
            state.context.tools = tools;
            send_json(
                socket,
                &session_update_payload(config, &state.context.instructions, &state.context.tools),
            )
            .await?;
        }
        VoiceCommand::SubmitToolOutputs(outputs) => {
            let (output_events, request_follow_up) = tool_output_events(&outputs)?;
            for output_event in output_events {
                send_json(socket, &output_event).await?;
            }
            if request_follow_up {
                send_json(socket, &json!({ "type": "response.create" })).await?;
                send_event(
                    event_sender,
                    VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
                )
                .await;
            }
        }
        VoiceCommand::SendText(text) => {
            if text.trim().is_empty() {
                return Ok(());
            }
            send_json(
                socket,
                &json!({
                    "type": "conversation.item.create",
                    "item": {
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": text }],
                    },
                }),
            )
            .await?;
            send_json(socket, &json!({ "type": "response.create" })).await?;
            state.is_response_active = true;
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
            )
            .await;
        }
        VoiceCommand::StartPushToTalk => {
            if matches!(config.input_mode, VoiceInputMode::PushToTalk) {
                if state.is_response_active {
                    send_json(socket, &json!({ "type": "response.cancel" })).await?;
                    state.is_response_active = false;
                }
                let played_milliseconds = audio.interrupt_playback();
                if let Some(item) = state.playing_item.take() {
                    send_json(
                        socket,
                        &json!({
                            "type": "conversation.item.truncate",
                            "item_id": item.item_id,
                            "content_index": item.content_index,
                            "audio_end_ms": played_milliseconds,
                        }),
                    )
                    .await?;
                }
                send_json(socket, &json!({ "type": "input_audio_buffer.clear" })).await?;
                audio.set_capture_enabled(true);
                send_event(
                    event_sender,
                    VoiceEvent::StateChanged(VoiceConnectionState::Recording),
                )
                .await;
            }
        }
        VoiceCommand::StopPushToTalk => {
            if matches!(config.input_mode, VoiceInputMode::PushToTalk) {
                audio.set_capture_enabled(false);
                send_json(socket, &json!({ "type": "input_audio_buffer.commit" })).await?;
                send_json(socket, &json!({ "type": "response.create" })).await?;
                send_event(
                    event_sender,
                    VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
                )
                .await;
            }
        }
    }

    Ok(())
}

async fn handle_provider_event(
    socket: &mut RealtimeSocket,
    audio: &mut AudioEngine,
    event_sender: &mpsc::Sender<VoiceEvent>,
    state: &mut SessionState,
    event: &Value,
) -> Result<(), VoiceError> {
    match event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "input_audio_buffer.speech_started" => {
            let played_milliseconds = audio.interrupt_playback();
            if let Some(item) = state.playing_item.take() {
                send_json(
                    socket,
                    &json!({
                        "type": "conversation.item.truncate",
                        "item_id": item.item_id,
                        "content_index": item.content_index,
                        "audio_end_ms": played_milliseconds,
                    }),
                )
                .await?;
            }
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Recording),
            )
            .await;
        }
        "input_audio_buffer.speech_stopped" => {
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
            )
            .await;
        }
        "response.created" => {
            state.is_response_active = true;
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
            )
            .await;
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(delta) {
                    let item_id = event
                        .get("item_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let content_index = event
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    if state.playing_item.as_ref().map(|item| &item.item_id) != Some(&item_id) {
                        audio.begin_response_audio();
                    }
                    state.playing_item = Some(PlayingItem {
                        item_id,
                        content_index,
                    });
                    audio.enqueue_realtime_pcm16(&bytes);
                    send_event(
                        event_sender,
                        VoiceEvent::StateChanged(VoiceConnectionState::Speaking),
                    )
                    .await;
                }
            }
        }
        "conversation.item.input_audio_transcription.completed" => {
            if let Some(transcript) = event.get("transcript").and_then(Value::as_str) {
                send_event(
                    event_sender,
                    VoiceEvent::UserTranscript(transcript.to_owned()),
                )
                .await;
            }
        }
        "response.output_audio_transcript.delta" | "response.audio_transcript.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                send_event(
                    event_sender,
                    VoiceEvent::AssistantTranscript(delta.to_owned()),
                )
                .await;
            }
        }
        "response.done" => {
            state.is_response_active = false;
            let invocations = tool_invocations_from_response(event)
                .into_iter()
                .filter(|invocation| state.completed_call_ids.insert(invocation.call_id.clone()))
                .collect::<Vec<_>>();
            if !invocations.is_empty() {
                send_event(event_sender, VoiceEvent::ToolInvocations(invocations)).await;
            }
            state.playing_item = None;
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Listening),
            )
            .await;
        }
        "error" => {
            let message = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI Realtime returned an unknown error")
                .to_owned();
            send_event(event_sender, VoiceEvent::ProviderError(message)).await;
        }
        _ => {}
    }

    Ok(())
}

async fn connect(config: &VoiceRuntimeConfig) -> Result<RealtimeSocket, VoiceError> {
    // This workspace contains TLS clients with different rustls feature
    // graphs. Selecting one provider here prevents rustls from panicking when
    // feature unification leaves process-wide provider choice ambiguous.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = format!("{REALTIME_ENDPOINT}?model={}", config.model.api_name());
    let mut request = url.into_client_request().map_err(VoiceError::Connect)?;
    let authorization =
        HeaderValue::from_str(&format!("Bearer {}", config.api_key.expose_secret()))
            .map_err(|_| VoiceError::InvalidApiKeyHeader)?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(VoiceError::Connect)?;
    Ok(socket)
}

async fn connect_with_timeout(config: &VoiceRuntimeConfig) -> Result<RealtimeSocket, VoiceError> {
    tokio::time::timeout(CONNECTION_TIMEOUT, connect(config))
        .await
        .map_err(|_| VoiceError::ConnectionTimeout)?
}

async fn close_socket(socket: &mut RealtimeSocket) {
    let _ = tokio::time::timeout(SOCKET_CLOSE_TIMEOUT, socket.close(None)).await;
}

async fn send_disabled(event_sender: &mpsc::Sender<VoiceEvent>) {
    send_event(
        event_sender,
        VoiceEvent::StateChanged(VoiceConnectionState::Disabled),
    )
    .await;
}

fn session_update_payload(
    config: &VoiceRuntimeConfig,
    instructions: &str,
    tools: &[VoiceToolDefinition],
) -> Value {
    let turn_detection = match config.input_mode {
        VoiceInputMode::SemanticVad => json!({
            "type": "semantic_vad",
            "eagerness": config.vad_eagerness.api_name(),
            "create_response": true,
            "interrupt_response": true,
        }),
        VoiceInputMode::PushToTalk => Value::Null,
    };
    let tools = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "type": "session.update",
        "session": {
            "type": "realtime",
            "model": config.model.api_name(),
            "output_modalities": ["audio"],
            "reasoning": {
                "effort": config.reasoning_effort.api_name(),
            },
            "audio": {
                "input": {
                    "format": {
                        "type": "audio/pcm",
                        "rate": 24_000,
                    },
                    "transcription": {
                        "model": "gpt-realtime-whisper",
                    },
                    "turn_detection": turn_detection,
                },
                "output": {
                    "format": {
                        "type": "audio/pcm",
                        "rate": 24_000,
                    },
                    "voice": config.voice.api_name(),
                },
            },
            "instructions": instructions,
            "tools": tools,
            "tool_choice": "auto",
        },
    })
}

fn tool_invocations_from_response(event: &Value) -> Vec<VoiceToolInvocation> {
    event
        .pointer("/response/output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| {
            Some(VoiceToolInvocation {
                call_id: item.get("call_id")?.as_str()?.to_owned(),
                name: item.get("name")?.as_str()?.to_owned(),
                arguments_json: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_owned(),
            })
        })
        .collect()
}

/// Converts a completed invocation batch into provider events while retaining
/// one aggregate decision about whether the model should continue speaking.
fn tool_output_events(outputs: &[VoiceToolOutput]) -> Result<(Vec<Value>, bool), VoiceError> {
    let request_follow_up = outputs.iter().any(|output| output.request_follow_up);
    let events = outputs
        .iter()
        .map(|output| {
            Ok(json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "function_call_output",
                    "call_id": output.call_id,
                    "output": serde_json::to_string(&output.result)?,
                },
            }))
        })
        .collect::<Result<Vec<_>, VoiceError>>()?;

    Ok((events, request_follow_up))
}

async fn send_json(socket: &mut RealtimeSocket, payload: &Value) -> Result<(), VoiceError> {
    socket
        .send(Message::Text(serde_json::to_string(payload)?.into()))
        .await
        .map_err(VoiceError::Transport)
}

async fn send_event(event_sender: &mpsc::Sender<VoiceEvent>, event: VoiceEvent) {
    let _ = event_sender.send(event).await;
}

#[derive(Default)]
struct BoundedCallIdSet {
    insertion_order: VecDeque<String>,
    values: HashSet<String>,
}

impl BoundedCallIdSet {
    fn insert(&mut self, call_id: String) -> bool {
        if !self.values.insert(call_id.clone()) {
            return false;
        }

        self.insertion_order.push_back(call_id);
        if self.insertion_order.len() > MAX_REMEMBERED_TOOL_CALLS {
            if let Some(evicted) = self.insertion_order.pop_front() {
                self.values.remove(&evicted);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;
    use crate::{ReasoningEffort, VadEagerness, VoiceModel, VoiceName};

    fn config(input_mode: VoiceInputMode) -> VoiceRuntimeConfig {
        VoiceRuntimeConfig {
            api_key: SecretString::from("test-key"),
            model: VoiceModel::GptRealtime21,
            voice: VoiceName::Marin,
            reasoning_effort: ReasoningEffort::Low,
            input_mode,
            vad_eagerness: VadEagerness::High,
            input_device_name: None,
            output_device_name: None,
            output_volume_percent: 80,
            instructions: "Control ilium.".to_owned(),
        }
    }

    #[test]
    fn semantic_vad_session_payload_uses_current_nested_audio_schema() {
        let payload = session_update_payload(&config(VoiceInputMode::SemanticVad), "Prompt", &[]);

        assert_eq!(payload["session"]["model"], "gpt-realtime-2.1");
        assert_eq!(
            payload["session"]["audio"]["input"]["turn_detection"]["type"],
            "semantic_vad"
        );
        assert_eq!(
            payload["session"]["audio"]["input"]["turn_detection"]["eagerness"],
            "high"
        );
        assert_eq!(payload["session"]["audio"]["output"]["voice"], "marin");
        assert_eq!(
            payload["session"]["audio"]["output"]["format"]["rate"],
            24_000
        );
        assert_eq!(payload["session"]["reasoning"]["effort"], "low");
    }

    #[test]
    fn push_to_talk_disables_server_vad() {
        let payload = session_update_payload(&config(VoiceInputMode::PushToTalk), "Prompt", &[]);

        assert!(payload["session"]["audio"]["input"]["turn_detection"].is_null());
    }

    #[test]
    fn completed_function_calls_are_extracted_and_deduplicated() {
        let event = json!({
            "response": {
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call-1",
                        "name": "ilium_get_state",
                        "arguments": "{\"detail\":\"compact\"}"
                    },
                    {
                        "type": "function_call",
                        "call_id": "call-2",
                        "name": "ilium_ui",
                        "arguments": "{\"action\":\"open_help\"}"
                    }
                ]
            }
        });
        let invocations = tool_invocations_from_response(&event);
        let mut seen = BoundedCallIdSet::default();

        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].name, "ilium_get_state");
        assert_eq!(invocations[1].name, "ilium_ui");
        assert!(seen.insert(invocations[0].call_id.clone()));
        assert!(!seen.insert(invocations[0].call_id.clone()));
        assert!(seen.insert(invocations[1].call_id.clone()));
    }

    #[test]
    fn tool_output_batch_creates_every_output_and_one_follow_up_decision() {
        let outputs = vec![
            VoiceToolOutput {
                call_id: "call-1".to_owned(),
                result: json!({ "ok": true }),
                request_follow_up: false,
            },
            VoiceToolOutput {
                call_id: "call-2".to_owned(),
                result: json!({ "selected": "pane-2" }),
                request_follow_up: true,
            },
        ];

        let (events, request_follow_up) = tool_output_events(&outputs).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["item"]["call_id"], "call-1");
        assert_eq!(events[1]["item"]["call_id"], "call-2");
        assert_eq!(events[0]["item"]["output"], r#"{"ok":true}"#);
        assert!(request_follow_up);
    }
}
