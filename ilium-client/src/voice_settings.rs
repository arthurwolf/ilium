//! Voice-control settings rows and multiline prompt editor state.

use ratatui_textarea::TextArea;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceSettingField {
    ApiKey,
}

impl VoiceSettingField {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "OpenAI API key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceRow {
    Enabled,
    ApiKey,
    Model,
    Voice,
    ReasoningEffort,
    InputMode,
    VadEagerness,
    InputDevice,
    OutputDevice,
    OutputVolume,
    CustomPrompt,
    Reconnect,
}

impl VoiceRow {
    pub const ALL: [Self; 12] = [
        Self::Enabled,
        Self::ApiKey,
        Self::Model,
        Self::Voice,
        Self::ReasoningEffort,
        Self::InputMode,
        Self::VadEagerness,
        Self::InputDevice,
        Self::OutputDevice,
        Self::OutputVolume,
        Self::CustomPrompt,
        Self::Reconnect,
    ];
}

/// Full-screen-safe multiline editor for the additive voice system prompt.
#[derive(Debug, Clone)]
pub struct VoicePromptEditorState {
    pub textarea: TextArea<'static>,
}

impl VoicePromptEditorState {
    pub fn new(prompt: &str) -> Self {
        let lines = if prompt.is_empty() {
            vec![String::new()]
        } else {
            prompt.lines().map(str::to_owned).collect()
        };
        Self {
            textarea: TextArea::from(lines),
        }
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }
}
