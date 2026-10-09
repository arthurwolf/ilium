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
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::audio::{AudioEngine, CapturedAudio};
use crate::{
    VoiceCommand, VoiceConnectionState, VoiceError, VoiceEvent, VoiceInputMode, VoiceRuntimeConfig,
    VoiceToolDefinition, VoiceToolInvocation, VoiceToolOutput,
};

const REALTIME_ENDPOINT: &str = "wss://api.openai.com/v1/realtime";
/// Test and demo seam: points the adapter at a local scripted Realtime server so
/// the full voice path can be exercised without the network or an API key.
const ENDPOINT_OVERRIDE_ENV: &str = "ILIUM_VOICE_REALTIME_URL";
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_RENEWAL_INTERVAL: Duration = Duration::from_secs(55 * 60);
const SESSION_CONFIGURATION_TIMEOUT: Duration = Duration::from_secs(15);
const SOCKET_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_REMEMBERED_TOOL_CALLS: usize = 1_024;
/// Bound both a provider message and the expanded JSON value built from it.
/// Audio deltas are separately limited to 256 KiB of PCM before encoding.
const MAX_PROVIDER_EVENT_BYTES: usize = 8 * 1024 * 1024;

type RealtimeSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct SessionContext {
    instructions: String,
    tools: Vec<VoiceToolDefinition>,
    // Installed source lease, replaced only after the old source is dropped.
    allocation: Option<std::sync::Arc<dyn crate::VoiceTextAllocation>>,
}

/// Mutable state owned by one logical voice session across proactive
/// WebSocket renewals. Conversation-local playback flags reset per
/// connection, while context and call deduplication survive renewal.
struct SessionState {
    context: SessionContext,
    completed_call_ids: BoundedCallIdSet,
    playing_item: Option<PlayingItem>,
    is_response_active: bool,
    /// Function calls from the last completed response were handed to the
    /// application and their outputs have not come back yet. The provider
    /// allows only one response at a time and the application's follow-up
    /// `response.create` must not collide with a typed turn, so typed turns
    /// wait for the outputs.
    is_awaiting_tool_outputs: bool,
    /// Typed sentences (`VoiceCommand::SendText`) not yet turned into a user
    /// item because a response was in flight. Each becomes its own turn, in
    /// order, exactly like successive spoken sentences. Survives a proactive
    /// reconnect, unlike the flags above.
    pending_text: VecDeque<crate::OwnedVoiceText>,
    retained_context_bytes: usize,
    retained_context_allocation: Option<std::sync::Arc<dyn crate::VoiceTextAllocation>>,
    retained_text_capacity: usize,
    // The websocket output Vec retains spare capacity after send/flush. The
    // largest typed-frame allocation owner survives through the actual session
    // and socket lifetime, rather than releasing at logical turn completion.
    retained_text_allocation: Option<std::sync::Arc<dyn crate::VoiceTextAllocation>>,
}

impl SessionState {
    fn new(instructions: String, tools: Vec<VoiceToolDefinition>) -> Self {
        Self {
            context: SessionContext {
                instructions,
                tools,
                allocation: None,
            },
            completed_call_ids: BoundedCallIdSet::default(),
            playing_item: None,
            is_response_active: false,
            is_awaiting_tool_outputs: false,
            pending_text: VecDeque::new(),
            retained_context_bytes: 0,
            retained_context_allocation: None,
            retained_text_capacity: 0,
            retained_text_allocation: None,
        }
    }

    fn reset_connection_state(&mut self) {
        self.playing_item = None;
        self.is_response_active = false;
        self.is_awaiting_tool_outputs = false;
    }

    /// Whether the provider can take a new typed turn right now.
    fn can_start_typed_turn(&self) -> bool {
        !self.is_response_active && !self.is_awaiting_tool_outputs
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
    startup: crate::OwnedVoiceStartup,
    command_receiver: &mut mpsc::Receiver<VoiceCommand>,
    mut shutdown_receiver: watch::Receiver<bool>,
    event_sender: crate::EventSender,
    quota: ilium_execution::QuotaGroup,
    custody: crate::AudioCustody,
) -> Result<(), VoiceError> {
    let crate::OwnedVoiceStartup {
        config,
        tools,
        retained_bytes,
        allocation,
    } = startup;
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
        &quota,
        &custody,
    )
    .await?;
    let mut state = SessionState::new(config.instructions.clone(), tools);
    state.context.allocation = Some(allocation.clone());
    state.retained_context_bytes = retained_bytes;
    state.retained_context_allocation = Some(allocation);

    loop {
        send_event(
            &event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Connecting),
        )
        .await?;
        let mut socket = tokio::select! {
            result = connect_with_timeout(&config) => result?,
            _ = shutdown_receiver.changed() => {
                send_disabled(&event_sender).await?;
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
                send_disabled(&event_sender).await?;
                return Ok(());
            }
        }
        tracing::info!("OpenAI Realtime session configuration accepted");
        send_event(
            &event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Listening),
        )
        .await?;

        match run_connected_session(
            &config,
            &mut state,
            &mut socket,
            &mut audio,
            command_receiver,
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
                send_disabled(&event_sender).await?;
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
    event_sender: &crate::EventSender,
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
                let capture = capture?.ok_or(VoiceError::SessionEnded)?;
                append_audio_capture(socket, capture).await?;
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
                let audio_delta = matches!(
                    event.get("type").and_then(Value::as_str),
                    Some("response.audio.delta" | "response.output_audio.delta")
                );
                let handling = handle_provider_event(
                    socket,
                    audio,
                    event_sender,
                    state,
                    &event,
                );
                if audio_delta {
                    // Lossless DSP backpressure must still observe explicit
                    // session shutdown. This cancels audio, not a tool result.
                    tokio::select! {
                        biased;
                        _ = shutdown_receiver.changed() => return Ok(SessionExit::Shutdown),
                        result = handling => result?,
                    }
                } else {
                    handling.await?;
                }
            }
        }
    }
}

async fn handle_command(
    config: &VoiceRuntimeConfig,
    state: &mut SessionState,
    socket: &mut RealtimeSocket,
    audio: &mut AudioEngine,
    event_sender: &crate::EventSender,
    command: VoiceCommand,
) -> Result<CommandOutcome, VoiceError> {
    match command {
        VoiceCommand::UpdateContext(context) => {
            let (instructions, tools, retained_bytes, allocation) = context.into_parts();
            state.context.instructions = instructions;
            state.context.tools = tools;
            state.context.allocation = Some(allocation.clone());
            let mut previous_allocation = None;
            if retained_bytes > state.retained_context_bytes {
                state.retained_context_bytes = retained_bytes;
                previous_allocation = state.retained_context_allocation.replace(allocation);
            }
            send_json(
                socket,
                &session_update_payload(config, &state.context.instructions, &state.context.tools),
            )
            .await?;
            drop(previous_allocation);
        }
        VoiceCommand::SubmitToolOutputs(outputs) => {
            submit_tool_outputs(socket, event_sender, state, &outputs, true).await?;
        }
        VoiceCommand::SubmitToolOutputsAndShutdown(outputs) => {
            submit_tool_outputs(socket, event_sender, state, &outputs, false).await?;
            return Ok(CommandOutcome::Shutdown);
        }
        VoiceCommand::SendText(mut text) => {
            text.trim_in_place();
            if text.as_str().is_empty() {
                return Ok(CommandOutcome::Continue);
            }
            state.pending_text.push_back(text);
            start_pending_typed_turn(socket, event_sender, state).await?;
        }
        VoiceCommand::StartPushToTalk => {
            if matches!(config.input_mode, VoiceInputMode::PushToTalk) {
                if state.is_response_active {
                    send_json(socket, &json!({ "type": "response.cancel" })).await?;
                    state.is_response_active = false;
                }
                let played_milliseconds = audio.interrupt_playback().await?;
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
                .await?;
            }
        }
        VoiceCommand::StopPushToTalk => {
            if matches!(config.input_mode, VoiceInputMode::PushToTalk) {
                // Fence the in-flight callback and DSP capture ordinal before
                // committing. Every accepted tail sample precedes the commit.
                for capture in audio.pause_capture_and_drain().await? {
                    append_audio_capture(socket, capture).await?;
                }
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
                .await?;
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
    event_sender: &crate::EventSender,
    state: &mut SessionState,
    outputs: &[VoiceToolOutput],
    can_request_follow_up: bool,
) -> Result<(), VoiceError> {
    let (output_events, request_follow_up) = tool_output_events(outputs)?;
    for output_event in output_events {
        send_json(socket, &output_event).await?;
    }
    state.is_awaiting_tool_outputs = false;
    if can_request_follow_up && request_follow_up {
        send_json(socket, &json!({ "type": "response.create" })).await?;
        // The provider has not yet said `response.created`; marking the
        // response active now keeps a typed turn from racing this follow-up.
        state.is_response_active = true;
        send_event(
            event_sender,
            VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
        )
        .await?;
    } else if can_request_follow_up {
        // No follow-up will end the turn, so no `response.done` is coming to
        // release a typed sentence that queued behind these tool calls.
        start_pending_typed_turn(socket, event_sender, state).await?;
    }

    Ok(())
}

/// Turns the oldest queued typed sentence into a user message and asks for a
/// response, if the provider is free. This is the same input the model gets
/// from a recognised utterance, so it picks tools and answers identically.
async fn start_pending_typed_turn(
    socket: &mut RealtimeSocket,
    event_sender: &crate::EventSender,
    state: &mut SessionState,
) -> Result<(), VoiceError> {
    if !state.can_start_typed_turn() {
        return Ok(());
    }
    let Some(text) = state.pending_text.pop_front() else {
        return Ok(());
    };
    let mut previous_allocation = None;
    if text.retained_capacity() > state.retained_text_capacity {
        state.retained_text_capacity = text.retained_capacity();
        // Keep both old and new ownership through actual buffer growth. The
        // old lease drops after the new preadmitted envelope takes custody.
        previous_allocation =
            std::mem::replace(&mut state.retained_text_allocation, text.allocation());
    }
    send_json(
        socket,
        &json!({
            "type": "conversation.item.create",
            "item": {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text.as_str().trim() }],
            },
        }),
    )
    .await?;
    drop(previous_allocation);
    send_json(socket, &json!({ "type": "response.create" })).await?;
    state.is_response_active = true;
    send_event(
        event_sender,
        VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
    )
    .await?;
    Ok(())
}

/// Encodes one captured microphone frame and appends it to the provider's
/// input audio buffer.
async fn append_audio_capture(
    socket: &mut RealtimeSocket,
    capture: CapturedAudio,
) -> Result<(), VoiceError> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(capture.pcm16_le);
    send_json(
        socket,
        &json!({
            "type": "input_audio_buffer.append",
            "audio": encoded,
        }),
    )
    .await
}

async fn handle_provider_event(
    socket: &mut RealtimeSocket,
    audio: &mut AudioEngine,
    event_sender: &crate::EventSender,
    state: &mut SessionState,
    event: &Value,
) -> Result<(), VoiceError> {
    match event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "input_audio_buffer.speech_started" => {
            let played_milliseconds = audio.interrupt_playback().await?;
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
            .await?;
        }
        "input_audio_buffer.speech_stopped" => {
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
            )
            .await?;
        }
        "response.created" => {
            state.is_response_active = true;
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Thinking),
            )
            .await?;
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                // Reject before allocating the decoded PCM. Output conversion
                // itself is off-thread; this also bounds one actor admission
                // interval so response controls cannot queue behind huge audio.
                let maximum_encoded_bytes = crate::audio::MAX_PROVIDER_PCM_BYTES.div_ceil(3) * 4;
                if delta.len() > maximum_encoded_bytes {
                    return Err(VoiceError::AudioPreparation(
                        "provider audio delta exceeds bounded admission".into(),
                    ));
                }
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
                            audio.begin_response_audio().await?;
                        }
                        state.playing_item = Some(PlayingItem {
                            item_id,
                            content_index,
                        });
                        audio.enqueue_realtime_pcm16(&bytes).await?;
                        send_event(
                            event_sender,
                            VoiceEvent::StateChanged(VoiceConnectionState::Speaking),
                        )
                        .await?;
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
                event_sender
                    .send_with(transcript.len(), || {
                        VoiceEvent::UserTranscript(transcript.to_owned())
                    })
                    .await?;
            }
        }
        "response.output_audio_transcript.delta" | "response.audio_transcript.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                event_sender
                    .send_with(delta.len(), || {
                        VoiceEvent::AssistantTranscript(delta.to_owned())
                    })
                    .await?;
            }
        }
        "response.done" => {
            state.is_response_active = false;
            let bytes = tool_invocation_allocation_bytes(event)?;
            if bytes == 0 {
                state.is_awaiting_tool_outputs = false;
            } else {
                event_sender
                    .send_optional_with(bytes, |allocation| {
                        let invocations = tool_invocations_from_response(event)
                            .into_iter()
                            .filter(|invocation| {
                                state.completed_call_ids.insert_guarded(
                                    invocation.call_id.clone(),
                                    Some(allocation.clone()),
                                )
                            })
                            .collect::<Vec<_>>();
                        state.is_awaiting_tool_outputs = !invocations.is_empty();
                        (!invocations.is_empty())
                            .then_some(VoiceEvent::ToolInvocations(invocations))
                    })
                    .await?;
            }
            state.playing_item = None;
            send_event(
                event_sender,
                VoiceEvent::StateChanged(VoiceConnectionState::Listening),
            )
            .await?;
            // A typed sentence that arrived mid-response runs now (and puts
            // the state back to Thinking); with tool calls pending it waits
            // for their outputs instead.
            start_pending_typed_turn(socket, event_sender, state).await?;
        }
        "error" => {
            let message = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI Realtime returned an unknown error");
            tracing::error!(
                provider_event = %diagnostic_json(event),
                error = %message,
                "OpenAI Realtime provider error"
            );
            event_sender
                .send_with(message.len(), || {
                    VoiceEvent::ProviderError(message.to_owned())
                })
                .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Returns the Realtime endpoint. The override is honoured only for loopback
/// `ws://` URLs, so captured microphone audio can never be redirected to a
/// remote host through the environment.
fn realtime_endpoint() -> Result<String, VoiceError> {
    resolve_realtime_endpoint(std::env::var(ENDPOINT_OVERRIDE_ENV).ok().as_deref())
}

/// Pure core of [`realtime_endpoint`], separated so the loopback guard is
/// testable without touching the process environment.
fn resolve_realtime_endpoint(override_value: Option<&str>) -> Result<String, VoiceError> {
    let Some(value) = override_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(REALTIME_ENDPOINT.to_owned());
    };
    if !is_loopback_websocket_url(value) {
        return Err(VoiceError::Connect(format!(
            "{ENDPOINT_OVERRIDE_ENV} must be a loopback ws:// URL"
        )));
    }
    Ok(value.to_owned())
}

/// True only for `ws://<loopback host>:<port>[/path]`. The authority is
/// parsed rather than prefix-matched: `ws://127.0.0.1:80@example.com/` starts
/// with a loopback prefix yet connects to `example.com`.
fn is_loopback_websocket_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("ws://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        && !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
}

async fn connect(config: &VoiceRuntimeConfig) -> Result<RealtimeSocket, VoiceError> {
    // This workspace contains TLS clients with different rustls feature
    // graphs. Selecting one provider here prevents rustls from panicking when
    // feature unification leaves process-wide provider choice ambiguous.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = format!("{}?model={}", realtime_endpoint()?, config.model.api_name());
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
    let (socket, response) = match tokio_tungstenite::connect_async_with_config(
        request,
        Some(realtime_socket_config()),
        false,
    )
    .await
    {
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

fn realtime_socket_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_PROVIDER_EVENT_BYTES);
    config.max_frame_size = Some(MAX_PROVIDER_EVENT_BYTES);
    config
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

async fn send_disabled(event_sender: &crate::EventSender) -> Result<(), VoiceError> {
    send_event(
        event_sender,
        VoiceEvent::StateChanged(VoiceConnectionState::Disabled),
    )
    .await
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

fn tool_invocation_allocation_bytes(event: &Value) -> Result<usize, VoiceError> {
    let mut bytes = 0usize;
    for item in event
        .pointer("/response/output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        let (Some(call_id), Some(name)) = (
            item.get("call_id").and_then(Value::as_str),
            item.get("name").and_then(Value::as_str),
        ) else {
            continue;
        };
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        // Original capture + filtering scratch: geometric Vec capacity <=2n
        // for each phase, plus original strings and two dedup ID captures.
        let cost = std::mem::size_of::<VoiceToolInvocation>()
            .checked_mul(4)
            .and_then(|cost| cost.checked_add(256))
            .and_then(|cost| cost.checked_add(call_id.len().checked_mul(3)?))
            .and_then(|cost| cost.checked_add(name.len()))
            .and_then(|cost| cost.checked_add(arguments.len()))
            .and_then(|cost| bytes.checked_add(cost))
            .ok_or_else(|| {
                VoiceError::AudioPreparation("voice tool event allocation size overflow".into())
            })?;
        bytes = cost;
    }
    if bytes == 0 {
        return Ok(0);
    }
    bytes
        .checked_add(8 * std::mem::size_of::<VoiceToolInvocation>())
        .ok_or_else(|| {
            VoiceError::AudioPreparation("voice tool scratch allocation size overflow".into())
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

async fn send_event(
    event_sender: &crate::EventSender,
    event: VoiceEvent,
) -> Result<(), VoiceError> {
    match event {
        VoiceEvent::StateChanged(state) => event_sender.send_state(state).await,
        _ => Err(VoiceError::AudioPreparation(
            "owned provider event requires preallocation admission".into(),
        )),
    }
}

/// Parses and records one complete provider text event. Base64 audio is
/// replaced with byte/character counts so diagnostics retain sequencing and
/// correlation metadata without producing multi-gigabyte text logs.
fn parse_provider_event(text: &str) -> Result<Value, VoiceError> {
    if text.len() > MAX_PROVIDER_EVENT_BYTES {
        return Err(VoiceError::ProviderEventTooLarge {
            bytes: text.len(),
            limit: MAX_PROVIDER_EVENT_BYTES,
        });
    }
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
    values: HashSet<String>,
    // Guard-last: the dedup string heap survives emitted-event receipt release.
    insertion_order: VecDeque<(
        String,
        Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
    )>,
}

impl BoundedCallIdSet {
    #[cfg(test)]
    fn insert(&mut self, call_id: String) -> bool {
        self.insert_guarded(call_id, None)
    }

    fn insert_guarded(
        &mut self,
        call_id: String,
        allocation: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
    ) -> bool {
        if !self.values.insert(call_id.clone()) {
            return false;
        }

        self.insertion_order.push_back((call_id, allocation));
        if self.insertion_order.len() > MAX_REMEMBERED_TOOL_CALLS {
            if let Some((evicted, _allocation)) = self.insertion_order.pop_front() {
                self.values.remove(&evicted);
                drop(evicted);
                drop(_allocation);
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

    #[test]
    fn provider_event_json_expansion_has_a_hard_input_limit() {
        let padding = "x".repeat(MAX_PROVIDER_EVENT_BYTES);
        let text = format!("{{\"type\":\"test\",\"padding\":\"{padding}\"}}");
        assert!(matches!(
            parse_provider_event(&text),
            Err(VoiceError::ProviderEventTooLarge { bytes, limit })
                if bytes == text.len() && limit == MAX_PROVIDER_EVENT_BYTES
        ));
    }

    #[test]
    fn provider_event_parser_accepts_messages_within_the_transport_limit() {
        let event = parse_provider_event(r#"{"type":"test"}"#).unwrap();
        assert_eq!(event["type"], "test");
    }

    #[test]
    fn realtime_transport_caps_both_frames_and_reassembled_messages() {
        let config = realtime_socket_config();
        assert_eq!(config.max_message_size, Some(MAX_PROVIDER_EVENT_BYTES));
        assert_eq!(config.max_frame_size, Some(MAX_PROVIDER_EVENT_BYTES));
    }

    #[tokio::test]
    async fn connected_actor_shutdown_cancels_actual_full_audio_admission_and_joins_dsp() {
        use std::time::Instant;
        use tokio_tungstenite::tungstenite::protocol::Role;
        let quota = crate::test_quota();
        let custody = crate::AudioCustody::new(&quota).unwrap();
        let (mut audio, backpressured) =
            AudioEngine::backpressure_fixture(&quota, &custody).unwrap();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (client, accepted) = tokio::join!(TcpStream::connect(address), listener.accept());
        let mut socket = WebSocketStream::from_raw_socket(
            MaybeTlsStream::Plain(client.unwrap()),
            Role::Client,
            None,
        )
        .await;
        let mut provider =
            WebSocketStream::from_raw_socket(accepted.unwrap().0, Role::Server, None).await;
        let config = config(VoiceInputMode::PushToTalk);
        let mut state = SessionState::new(String::new(), Vec::new());
        let (_commands, mut commands) = mpsc::channel(64);
        let (shutdown, mut shutdown_receiver) = watch::channel(false);
        let (events, event_receiver) = mpsc::channel(128);
        let events = crate::EventSender {
            sender: events,
            quota: quota.clone(),
        };
        // Sixteen8KiB commands exceed one active output plus eight mailbox
        // slots. No callback consumes the thirty-sample playback ring.
        let delta = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 128 * 1024]);
        let message = Message::Text(
            json!({
                "type":"response.output_audio.delta", "delta":delta,
                "item_id":"blocked-audio", "content_index":0,
            })
            .to_string()
            .into(),
        );
        drop(delta);
        let outcome = {
            let actor = run_connected_session(
                &config,
                &mut state,
                &mut socket,
                &mut audio,
                &mut commands,
                &mut shutdown_receiver,
                &events,
            );
            tokio::pin!(actor);
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::select! {
                    outcome = &mut actor => panic!("actor ended before provider delivery: {outcome:?}"),
                    sent = provider.send(message) => sent.unwrap(),
                }
                loop {
                    if backpressured() { break; }
                    tokio::select! {
                        outcome = &mut actor => panic!("actor ended before actual audio backpressure: {outcome:?}"),
                        _ = tokio::time::sleep(Duration::from_millis(1)) => {},
                    }
                }
            }).await.expect("real DSP filled playback and mailbox");
            assert!(backpressured());
            shutdown.send(true).unwrap();
            tokio::time::timeout(Duration::from_secs(2), &mut actor)
                .await
                .expect("shutdown must interrupt audio handler backpressure")
                .unwrap()
        };
        assert_eq!(outcome, SessionExit::Shutdown);
        assert_eq!(quota.snapshot().worker_threads, 1);
        drop(audio);
        custody
            .join_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(custody.pending_owners(), 0);
        assert_eq!(quota.snapshot().worker_threads, 0);
        // Native-equivalent ring source/storage remains guarded after actual
        // DSP join until its final original custody and probe are released.
        assert!(quota.snapshot().worker_bytes >= 64 * 1024 * 1024);
        drop(backpressured);
        drop(provider);
        drop(socket);
        drop(listener);
        drop(state);
        drop(config);
        drop(events);
        drop(event_receiver);
        drop(custody);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn realtime_endpoint_defaults_to_the_provider_and_accepts_only_loopback_overrides() {
        assert_eq!(resolve_realtime_endpoint(None).unwrap(), REALTIME_ENDPOINT);
        assert_eq!(
            resolve_realtime_endpoint(Some("  ")).unwrap(),
            REALTIME_ENDPOINT
        );
        for accepted in [
            "ws://127.0.0.1:8080/v1/realtime",
            "ws://localhost:1/v1/realtime",
            "ws://[::1]:9000/v1/realtime",
        ] {
            assert_eq!(resolve_realtime_endpoint(Some(accepted)).unwrap(), accepted);
        }
        for rejected in [
            "wss://api.openai.com/v1/realtime",
            "ws://example.com:80/v1/realtime",
            "wss://127.0.0.1:8080/v1/realtime",
            "ws://127.0.0.1/v1/realtime",
            // Userinfo makes the real host `example.com` despite the prefix.
            "ws://127.0.0.1:80@example.com/v1/realtime",
            "ws://localhost:80@example.com",
            "ws://127.0.0.1.example.com:80/v1/realtime",
            "ws://127.0.0.1:80x/v1/realtime",
        ] {
            assert!(
                resolve_realtime_endpoint(Some(rejected)).is_err(),
                "{rejected} must be rejected"
            );
        }
    }

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
                result: std::sync::Arc::new(json!({ "ok": true })),
                request_follow_up: false,
                terminate_session_after_delivery: true,
                allocation_hold: None,
                retained_bytes: 0,
            },
            VoiceToolOutput {
                call_id: "call-2".to_owned(),
                result: std::sync::Arc::new(json!({ "selected": "pane-2" })),
                request_follow_up: true,
                terminate_session_after_delivery: false,
                allocation_hold: None,
                retained_bytes: 0,
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
