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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandOutcome {
    Continue,
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
    tracing::info!(
        model = config.model.api_name(),
        voice = config.voice.api_name(),
        input_mode = ?config.input_mode,
        tool_count = tools.len(),
        instructions = %config.instructions,
        "OpenAI Realtime voice session starting"
    );
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
        tracing::info!("OpenAI Realtime WebSocket connected");
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
        tracing::info!("OpenAI Realtime session configuration accepted");
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
                tracing::info!("OpenAI Realtime session reached renewal boundary");
                close_socket(&mut socket).await;
            }
            SessionExit::Shutdown => {
                tracing::info!("OpenAI Realtime voice session shutting down");
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
            let event = parse_provider_event(&text)?;
            match event.get("type").and_then(Value::as_str) {
                Some("session.updated") => return Ok(()),
                Some("error") => {
                    let message = event
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown session configuration error")
                        .to_owned();
                    tracing::error!(provider_event = %diagnostic_json(&event), error = %message, "OpenAI Realtime rejected session configuration");
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
                let outcome = handle_command(
                    config,
                    state,
                    socket,
                    audio,
                    event_sender,
                    command,
                ).await?;
                if matches!(outcome, CommandOutcome::Shutdown) {
                    return Ok(SessionExit::Shutdown);
                }
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
                let event = parse_provider_event(&text)?;
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
) -> Result<CommandOutcome, VoiceError> {
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
            submit_tool_outputs(socket, event_sender, &outputs, true).await?;
        }
        VoiceCommand::SubmitToolOutputsAndShutdown(outputs) => {
            submit_tool_outputs(socket, event_sender, &outputs, false).await?;
            return Ok(CommandOutcome::Shutdown);
        }
        VoiceCommand::SendText(text) => {
            if text.trim().is_empty() {
                return Ok(CommandOutcome::Continue);
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
                // Mirrors SendText's eager update: without this, a StartPushToTalk
                // that arrives before the provider's own "response.created" event
                // would see a stale `false` and skip cancelling the response this
                // call just requested.
                state.is_response_active = true;
                send_event(
                    event_sender,
                    VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
                )
                .await;
            }
        }
    }

    Ok(CommandOutcome::Continue)
}

/// Writes every result before optionally creating the model's follow-up turn.
/// A self-stop call passes `false`, making its function output the final
/// provider frame before the caller closes the ordered WebSocket stream.
async fn submit_tool_outputs(
    socket: &mut RealtimeSocket,
    event_sender: &mpsc::Sender<VoiceEvent>,
    outputs: &[VoiceToolOutput],
    can_request_follow_up: bool,
) -> Result<(), VoiceError> {
    let (output_events, request_follow_up) = tool_output_events(outputs)?;
    for output_event in output_events {
        send_json(socket, &output_event).await?;
    }
    if can_request_follow_up && request_follow_up {
        send_json(socket, &json!({ "type": "response.create" })).await?;
        send_event(
            event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
        )
        .await;
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
                match base64::engine::general_purpose::STANDARD.decode(delta) {
                    Ok(bytes) => {
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
                    Err(error) => {
                        // Dropping a malformed chunk silently would hide a real
                        // protocol problem behind an audio glitch with no trace.
                        tracing::warn!(
                            %error,
                            "OpenAI Realtime sent an audio delta that was not valid base64"
                        );
                    }
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
            tracing::error!(
                provider_event = %diagnostic_json(event),
                error = %message,
                "OpenAI Realtime provider error"
            );
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
    let diagnostic_url = ilium_logging::redacted_url(&url);
    tracing::info!(
        method = "GET",
        url = %diagnostic_url,
        headers = ?[("Authorization", "<redacted>"), ("Upgrade", "websocket")],
        "HTTP WebSocket upgrade started"
    );
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| VoiceError::Connect(error.to_string()))?;
    let authorization =
        HeaderValue::from_str(&format!("Bearer {}", config.api_key.expose_secret()))
            .map_err(|_| VoiceError::InvalidApiKeyHeader)?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    let (socket, response) = match tokio_tungstenite::connect_async(request).await {
        Ok(result) => result,
        Err(error) => {
            let diagnostic_error = diagnostic_websocket_connect_error(&error);
            tracing::error!(
                method = "GET",
                url = %diagnostic_url,
                error = %diagnostic_error,
                "HTTP WebSocket upgrade failed"
            );
            // Keep the propagated error as useful as the durable event without
            // reintroducing raw response headers through a later Debug render.
            return Err(VoiceError::Connect(diagnostic_error.to_string()));
        }
    };
    tracing::info!(
        method = "GET",
        url = %diagnostic_url,
        status = response.status().as_u16(),
        response_headers = ?redacted_response_headers(response.headers()),
        "HTTP WebSocket upgrade completed"
    );
    Ok(socket)
}

fn diagnostic_websocket_connect_error(error: &tokio_tungstenite::tungstenite::Error) -> Value {
    let diagnostic = match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => json!({
            "status": response.status().as_u16(),
            "headers": redacted_response_headers(response.headers()),
            "body": response
                .body()
                .as_ref()
                .map(|body| String::from_utf8_lossy(body).into_owned()),
        }),
        _ => json!({ "message": error.to_string() }),
    };
    ilium_logging::redacted_json_credentials(&diagnostic)
}

fn redacted_response_headers(
    headers: &tokio_tungstenite::tungstenite::http::HeaderMap,
) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<non-text header>");
            (
                name.as_str().to_owned(),
                ilium_logging::redacted_header_value(name.as_str(), value),
            )
        })
        .collect()
}

async fn connect_with_timeout(config: &VoiceRuntimeConfig) -> Result<RealtimeSocket, VoiceError> {
    tokio::time::timeout(CONNECTION_TIMEOUT, connect(config))
        .await
        .map_err(|_| VoiceError::ConnectionTimeout)?
}

async fn close_socket(socket: &mut RealtimeSocket) {
    match tokio::time::timeout(SOCKET_CLOSE_TIMEOUT, socket.close(None)).await {
        Ok(Ok(())) => tracing::info!("OpenAI Realtime WebSocket closed"),
        Ok(Err(error)) => {
            tracing::warn!(%error, error_debug = ?error, "OpenAI Realtime WebSocket close failed")
        }
        Err(_) => tracing::warn!("OpenAI Realtime WebSocket close timed out"),
    }
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
            let call_id = item.get("call_id").and_then(Value::as_str);
            let name = item.get("name").and_then(Value::as_str);
            let (Some(call_id), Some(name)) = (call_id, name) else {
                // A function_call missing call_id/name can never be answered
                // with a matching function_call_output, so the model is left
                // waiting on a tool result that will never arrive. Surface it
                // instead of silently dropping the invocation.
                tracing::warn!(
                    item = %diagnostic_json(item),
                    "OpenAI Realtime sent a function_call missing call_id or name"
                );
                return None;
            };
            Some(VoiceToolInvocation {
                call_id: call_id.to_owned(),
                name: name.to_owned(),
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
    let serialized = serde_json::to_string(payload).map_err(|error| {
        tracing::error!(
            payload = %diagnostic_json(payload),
            error = %error,
            "failed to serialize OpenAI Realtime outbound event"
        );
        VoiceError::Protocol(error)
    })?;
    tracing::info!(
        event_type = payload
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown"),
        payload = %diagnostic_json(payload),
        "OpenAI Realtime event sent"
    );
    socket
        .send(Message::Text(serialized.into()))
        .await
        .map_err(|error| {
            tracing::error!(
                event_type = payload
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown"),
                payload = %diagnostic_json(payload),
                error = %error,
                error_debug = ?error,
                "OpenAI Realtime event send failed"
            );
            VoiceError::Transport(error)
        })
}

async fn send_event(event_sender: &mpsc::Sender<VoiceEvent>, event: VoiceEvent) {
    if event_sender.send(event).await.is_err() {
        tracing::warn!("voice event receiver closed before event delivery");
    }
}

/// Parses and records one complete provider text event. Base64 audio is
/// replaced with byte/character counts so diagnostics retain sequencing and
/// correlation metadata without producing multi-gigabyte text logs.
fn parse_provider_event(text: &str) -> Result<Value, VoiceError> {
    let event: Value = serde_json::from_str(text).map_err(|error| {
        tracing::error!(
            raw_provider_text_bytes = text.len(),
            error = %error,
            "OpenAI Realtime event was not valid JSON"
        );
        VoiceError::Protocol(error)
    })?;
    tracing::info!(
        event_type = event
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown"),
        payload = %diagnostic_json(&event),
        "OpenAI Realtime event received"
    );
    Ok(event)
}

/// Clones a protocol event and replaces known binary and credential fields.
/// Text, non-secret tool arguments/results, provider errors, IDs, usage, and
/// session metadata remain complete because those are actionable diagnostics.
fn diagnostic_json(payload: &Value) -> Value {
    let mut sanitized = ilium_logging::redacted_json_credentials(payload);
    let event_type = sanitized
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let binary_field = match event_type {
        "input_audio_buffer.append" => "audio",
        "response.output_audio.delta" | "response.audio.delta" => "delta",
        _ => return sanitized,
    };
    if let Some(encoded) = sanitized.get(binary_field).and_then(Value::as_str) {
        let summary = format!(
            "<base64 audio omitted: {} characters, approximately {} bytes>",
            encoded.len(),
            encoded.len().saturating_mul(3) / 4
        );
        if let Some(object) = sanitized.as_object_mut() {
            object.insert(binary_field.to_owned(), Value::String(summary));
        }
    }
    sanitized
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
    fn diagnostics_omit_binary_audio_but_retain_text_tools_and_provider_errors() {
        let outbound_audio = diagnostic_json(&json!({
            "type": "input_audio_buffer.append",
            "audio": "QUJDREVGRw==",
            "event_id": "event-1",
        }));
        assert_eq!(outbound_audio["event_id"], "event-1");
        assert!(outbound_audio["audio"]
            .as_str()
            .unwrap()
            .contains("base64 audio omitted"));
        assert!(!outbound_audio.to_string().contains("QUJDREVGRw=="));

        let tool_and_error = diagnostic_json(&json!({
            "type": "error",
            "error": {"message": "invalid tool arguments"},
            "tool": {"name": "ilium_ui", "arguments": "{\"action\":\"open_help\"}"},
        }));
        assert_eq!(tool_and_error["error"]["message"], "invalid tool arguments");
        assert_eq!(tool_and_error["tool"]["name"], "ilium_ui");
        assert!(tool_and_error["tool"]["arguments"]
            .as_str()
            .unwrap()
            .contains("open_help"));

        let credential_tool = diagnostic_json(&json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "name": "ilium_write_setting",
                "arguments": "{\"path\":\"voice.api_key\",\"value\":\"provider-secret\",\"unrelated\":\"kept\"}"
            }
        }));
        let credential_diagnostic = credential_tool.to_string();
        assert!(!credential_diagnostic.contains("provider-secret"));
        assert!(credential_diagnostic.contains("<redacted>"));
        assert!(credential_diagnostic.contains("kept"));

        let mut headers = tokio_tungstenite::tungstenite::http::HeaderMap::new();
        headers.insert("set-cookie", HeaderValue::from_static("session=secret"));
        headers.insert("x-request-id", HeaderValue::from_static("request-42"));
        let headers = redacted_response_headers(&headers);
        assert!(headers.contains(&("set-cookie".to_owned(), "<redacted>".to_owned())));
        assert!(headers.contains(&("x-request-id".to_owned(), "request-42".to_owned())));

        let rejection_response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .header("set-cookie", "session=rejection-cookie-secret")
            .body(Some(
                br#"{"api_key":"rejection-body-secret","message":"denied"}"#.to_vec(),
            ))
            .unwrap();
        let rejection = diagnostic_websocket_connect_error(
            &tokio_tungstenite::tungstenite::Error::Http(Box::new(rejection_response)),
        )
        .to_string();
        assert!(rejection.contains("denied"));
        assert!(rejection.contains("<redacted>"));
        assert!(!rejection.contains("rejection-cookie-secret"));
        assert!(!rejection.contains("rejection-body-secret"));
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
                terminate_session_after_delivery: true,
            },
            VoiceToolOutput {
                call_id: "call-2".to_owned(),
                result: json!({ "selected": "pane-2" }),
                request_follow_up: true,
                terminate_session_after_delivery: false,
            },
        ];

        let (events, request_follow_up) = tool_output_events(&outputs).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["item"]["call_id"], "call-1");
        assert_eq!(events[1]["item"]["call_id"], "call-2");
        assert_eq!(events[0]["item"]["output"], r#"{"ok":true}"#);
        assert!(!events[0].to_string().contains("terminate_session"));
        assert!(request_follow_up);
    }
}
