//! Provider-neutral function-tool contracts.

use serde::{Deserialize, Serialize};

/// One function exposed to a voice model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A completed function request received from the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceToolInvocation {
    pub call_id: String,
    pub name: String,
    pub arguments_json: String,
}

/// The application-owned result returned for one invocation.
#[derive(Debug, Clone)]
pub struct VoiceToolOutput {
    pub call_id: String,
    pub result: std::sync::Arc<serde_json::Value>,
    pub request_follow_up: bool,
    /// Ends the owned provider/audio session only after this result has been
    /// written to the provider. This keeps self-stop tools protocol-complete
    /// without racing the actor's ordinary shutdown signal.
    pub terminate_session_after_delivery: bool,
    /// Immutable result clones share the JSON heap and its original allocation debit.
    pub allocation_hold: Option<std::sync::Arc<dyn std::fmt::Debug + Send + Sync>>,
    pub retained_bytes: usize,
}
impl PartialEq for VoiceToolOutput {
    fn eq(&self, other: &Self) -> bool {
        self.call_id == other.call_id
            && self.result == other.result
            && self.request_follow_up == other.request_follow_up
            && self.terminate_session_after_delivery == other.terminate_session_after_delivery
    }
}
