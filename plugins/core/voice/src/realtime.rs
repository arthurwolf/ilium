//! The OpenAI Realtime protocol as pure data: endpoint guard, session payload, provider event parsing,
//! credential-free diagnostics and tool call extraction. Moved from `ilium-voice`; the socket, the
//! environment read and the tracing events around these functions are host work.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::credentials::redacted_json_credentials;
use crate::error::VoiceError;

/// The provider endpoint used unless a loopback override is accepted.
pub const REALTIME_ENDPOINT: &str = "wss://api.openai.com/v1/realtime";
/// The environment variable the host reads for the loopback override.
pub const ENDPOINT_OVERRIDE_ENV: &str = "ILIUM_VOICE_REALTIME_URL";
/// The largest provider text event accepted.
pub const MAX_PROVIDER_EVENT_BYTES: usize = 8 * 1024 * 1024;
/// How many tool call ids are remembered for de-duplication.
pub const MAX_REMEMBERED_TOOL_CALLS: usize = 1_024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceModel {
    #[default]
    #[serde(rename = "gpt-realtime-2.1")]
    GptRealtime21,
    #[serde(rename = "gpt-realtime-2.1-mini")]
    GptRealtimeMini,
}

impl VoiceModel {
    pub const ALL: [Self; 2] = [Self::GptRealtime21, Self::GptRealtimeMini];

    pub const fn api_name(self) -> &'static str {
        match self {
            Self::GptRealtime21 => "gpt-realtime-2.1",
            Self::GptRealtimeMini => "gpt-realtime-2.1-mini",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::GptRealtime21 => "GPT Realtime 2.1",
            Self::GptRealtimeMini => "GPT Realtime 2.1 Mini",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceName {
    #[default]
    Marin,
    Cedar,
    Alloy,
    Ash,
    Ballad,
    Coral,
    Echo,
    Sage,
    Shimmer,
    Verse,
}

impl VoiceName {
    pub const ALL: [Self; 10] = [
        Self::Marin,
        Self::Cedar,
        Self::Alloy,
        Self::Ash,
        Self::Ballad,
        Self::Coral,
        Self::Echo,
        Self::Sage,
        Self::Shimmer,
        Self::Verse,
    ];

    pub const fn api_name(self) -> &'static str {
        match self {
            Self::Marin => "marin",
            Self::Cedar => "cedar",
            Self::Alloy => "alloy",
            Self::Ash => "ash",
            Self::Ballad => "ballad",
            Self::Coral => "coral",
            Self::Echo => "echo",
            Self::Sage => "sage",
            Self::Shimmer => "shimmer",
            Self::Verse => "verse",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Marin => "Marin",
            Self::Cedar => "Cedar",
            Self::Alloy => "Alloy",
            Self::Ash => "Ash",
            Self::Ballad => "Ballad",
            Self::Coral => "Coral",
            Self::Echo => "Echo",
            Self::Sage => "Sage",
            Self::Shimmer => "Shimmer",
            Self::Verse => "Verse",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Minimal,
    #[default]
    Low,
    Medium,
}

impl ReasoningEffort {
    pub const ALL: [Self; 3] = [Self::Minimal, Self::Low, Self::Medium];

    pub const fn api_name(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Low => "Low",
            Self::Medium => "Medium",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VadEagerness {
    #[default]
    Auto,
    Low,
    Medium,
    High,
}

impl VadEagerness {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Low, Self::Medium, Self::High];

    pub const fn api_name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceInputMode {
    #[default]
    SemanticVad,
    PushToTalk,
}

impl VoiceInputMode {
    pub const ALL: [Self; 2] = [Self::SemanticVad, Self::PushToTalk];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SemanticVad => "Semantic VAD",
            Self::PushToTalk => "Push to talk",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// The session settings the Realtime payload depends on (the credential never enters the plugin).
#[derive(Debug, Clone)]
pub struct VoiceSessionConfig {
    pub model: VoiceModel,
    pub voice: VoiceName,
    pub reasoning_effort: ReasoningEffort,
    pub input_mode: VoiceInputMode,
    pub vad_eagerness: VadEagerness,
}

/// A function the model may call.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A completed function request received from the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceToolInvocation {
    pub call_id: String,
    pub name: String,
    pub arguments_json: String,
}

/// The result returned for one invocation.
#[derive(Debug, Clone)]
pub struct VoiceToolOutput {
    pub call_id: String,
    pub result: Value,
    pub request_follow_up: bool,
}

/// Returns the Realtime endpoint for an optional override. The override is honoured only for loopback `ws://`
/// URLs, so captured microphone audio can never be redirected to a remote host through the environment.
pub fn resolve_realtime_endpoint(override_value: Option<&str>) -> Result<String, VoiceError> {
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
pub fn is_loopback_websocket_url(url: &str) -> bool {
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

/// The `session.update` event configuring audio formats, turn detection, instructions and tools.
pub fn session_update_payload(
    config: &VoiceSessionConfig,
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

/// Parses one complete provider text event within the size limit.
pub fn parse_provider_event(text: &str) -> Result<Value, VoiceError> {
    if text.len() > MAX_PROVIDER_EVENT_BYTES {
        return Err(VoiceError::ProviderEventTooLarge {
            bytes: text.len(),
            limit: MAX_PROVIDER_EVENT_BYTES,
        });
    }
    Ok(serde_json::from_str(text)?)
}

/// Clones a protocol event and replaces known binary and credential fields.
/// Text, non-secret tool arguments/results, provider errors, IDs, usage, and
/// session metadata remain complete because those are actionable diagnostics.
pub fn diagnostic_json(payload: &Value) -> Value {
    let mut sanitized = redacted_json_credentials(payload);
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

/// Extracts the completed function calls of a `response.done` event. A call missing its id or name is skipped
/// (it can never be answered); the host logs the skipped item.
pub fn tool_invocations_from_response(event: &Value) -> Vec<VoiceToolInvocation> {
    event
        .pointer("/response/output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| {
            let call_id = item.get("call_id").and_then(Value::as_str)?;
            let name = item.get("name").and_then(Value::as_str)?;
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

/// Converts a completed invocation batch into provider events while retaining one aggregate decision about
/// whether the model should continue speaking.
pub fn tool_output_events(outputs: &[VoiceToolOutput]) -> Result<(Vec<Value>, bool), VoiceError> {
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

/// Remembers the most recent tool call ids so a repeated `response.done` does not run a call twice.
#[derive(Default)]
pub struct BoundedCallIdSet {
    values: std::collections::HashSet<String>,
    insertion_order: std::collections::VecDeque<String>,
}

impl BoundedCallIdSet {
    /// True when the id was new; the oldest id is forgotten past [`MAX_REMEMBERED_TOOL_CALLS`].
    pub fn insert(&mut self, call_id: String) -> bool {
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
    use super::*;

    /// REQ: PORT:ilium-voice/src/openai.rs::realtime_endpoint_defaults_to_the_provider_and_accepts_only_loopback_overrides
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

    fn config(input_mode: VoiceInputMode) -> VoiceSessionConfig {
        VoiceSessionConfig {
            model: VoiceModel::GptRealtime21,
            voice: VoiceName::Marin,
            reasoning_effort: ReasoningEffort::Low,
            input_mode,
            vad_eagerness: VadEagerness::High,
        }
    }

    /// REQ: PORT:ilium-voice/src/openai.rs::semantic_vad_session_payload_uses_current_nested_audio_schema
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

    /// REQ: PORT:ilium-voice/src/openai.rs::push_to_talk_disables_server_vad
    #[test]
    fn push_to_talk_disables_server_vad() {
        let payload = session_update_payload(&config(VoiceInputMode::PushToTalk), "Prompt", &[]);

        assert!(payload["session"]["audio"]["input"]["turn_detection"].is_null());
    }

    /// REQ: PORT:ilium-voice/src/openai.rs::diagnostics_omit_binary_audio_but_retain_text_tools_and_provider_errors
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
    }

    /// REQ: PORT:ilium-voice/src/openai.rs::completed_function_calls_are_extracted_and_deduplicated
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

    /// REQ: PORT:ilium-voice/src/openai.rs::tool_output_batch_creates_every_output_and_one_follow_up_decision
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
        assert!(!events[0].to_string().contains("terminate_session"));
        assert!(request_follow_up);
    }

    /// REQ: PORT:ilium-voice/src/openai.rs::provider_event_json_expansion_has_a_hard_input_limit
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

    /// REQ: PORT:ilium-voice/src/openai.rs::provider_event_parser_accepts_messages_within_the_transport_limit
    #[test]
    fn provider_event_parser_accepts_messages_within_the_transport_limit() {
        let event = parse_provider_event(r#"{"type":"test"}"#).unwrap();
        assert_eq!(event["type"], "test");
    }
}
