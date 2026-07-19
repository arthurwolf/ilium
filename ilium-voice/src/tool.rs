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
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceToolOutput {
    pub call_id: String,
    pub result: serde_json::Value,
    pub request_follow_up: bool,
}
