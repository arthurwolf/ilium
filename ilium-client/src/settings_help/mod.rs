//! Per-setting explanations and inert UTF-8 infographics for the Settings UI.

pub mod catalog;
pub mod dialog;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelpTopic {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub specimen: String,
    pub states: String,
    pub frames: Vec<String>,
    pub motion: String,
    pub caveat: String,
}

#[cfg(test)]
mod tests;
