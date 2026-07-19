//! Provider-neutral configuration for a live voice session.

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// The OpenAI Realtime model selected for a session.
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

/// Built-in voices currently accepted by the Realtime API.
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

/// How much deliberation the model should use before selecting a tool.
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

/// Whether the server detects turns or ilium explicitly commits them.
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

/// Semantic VAD responsiveness.
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

fn stepped_value<T: Copy + PartialEq>(values: &[T], current: T, direction: i32) -> T {
    let index = values
        .iter()
        .position(|candidate| *candidate == current)
        .unwrap_or(0) as i32;
    values[(index + direction.signum()).rem_euclid(values.len() as i32) as usize]
}

/// Complete runtime configuration passed to a provider session.
///
/// The API key is deliberately non-serializable and redacted from `Debug`.
#[derive(Clone)]
pub struct VoiceRuntimeConfig {
    pub api_key: SecretString,
    pub model: VoiceModel,
    pub voice: VoiceName,
    pub reasoning_effort: ReasoningEffort,
    pub input_mode: VoiceInputMode,
    pub vad_eagerness: VadEagerness,
    pub input_device_name: Option<String>,
    pub output_device_name: Option<String>,
    pub output_volume_percent: u8,
    pub instructions: String,
}

impl std::fmt::Debug for VoiceRuntimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceRuntimeConfig")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("voice", &self.voice)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("input_mode", &self.input_mode)
            .field("vad_eagerness", &self.vad_eagerness)
            .field("input_device_name", &self.input_device_name)
            .field("output_device_name", &self.output_device_name)
            .field("output_volume_percent", &self.output_volume_percent)
            .field("instructions", &self.instructions)
            .finish()
    }
}

impl VoiceRuntimeConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.api_key.expose_secret().trim().is_empty() {
            return Err("OpenAI API key must not be empty".to_owned());
        }

        if self.output_volume_percent > 100 {
            return Err("voice output volume must be between 0 and 100".to_owned());
        }

        if self.instructions.trim().is_empty() {
            return Err("voice instructions must not be empty".to_owned());
        }

        Ok(())
    }
}
