//! Read the existing global polling configuration and OpenAI credential.
//!
//! This type deliberately has no `Debug`, serialization or IPC implementation.
//! The server must reload it when the global configuration changes.
use std::path::Path;

use ilium_core::polling_detection::PollingSettings;
use serde::Deserialize;
use thiserror::Error;

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawConfig {
    polling_detection: PollingSettings,
    inference: RawInference,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawInference {
    openai: RawOpenAi,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawOpenAi {
    api_key: String,
}

pub struct PollingConfig {
    pub settings: PollingSettings,
    openai_key: String,
}

impl PollingConfig {
    pub fn has_openai_key(&self) -> bool {
        !self.openai_key.trim().is_empty()
    }
    pub fn openai_key(&self) -> &str {
        &self.openai_key
    }
}

#[derive(Debug, Error)]
pub enum PollingConfigError {
    #[error("could not read global polling configuration: {0}")]
    Read(#[from] std::io::Error),
    #[error("global polling configuration is not valid TOML")]
    Parse,
    #[error("invalid polling settings: {0}")]
    Invalid(&'static str),
}

fn read_contents(directory: &Path) -> Result<String, PollingConfigError> {
    match crate::config::read_contents(&directory.join("config.toml")) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn load(directory: &Path) -> Result<PollingConfig, PollingConfigError> {
    let contents = read_contents(directory)?;
    let raw: RawConfig = toml::from_str(&contents).map_err(|_| PollingConfigError::Parse)?;
    raw.polling_detection
        .validate()
        .map_err(PollingConfigError::Invalid)?;
    Ok(PollingConfig {
        settings: raw.polling_detection,
        openai_key: raw.inference.openai.api_key,
    })
}

/// Settings-only loading for followers; credential ownership stays with the leader.
pub fn load_settings(directory: &Path) -> Result<PollingSettings, PollingConfigError> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct SettingsOnly {
        polling_detection: PollingSettings,
    }
    let raw: SettingsOnly =
        toml::from_str(&read_contents(directory)?).map_err(|_| PollingConfigError::Parse)?;
    raw.polling_detection
        .validate()
        .map_err(PollingConfigError::Invalid)?;
    Ok(raw.polling_detection)
}
