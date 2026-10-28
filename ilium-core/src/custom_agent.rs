//! Stable identities and validation for user-defined agent profiles.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::BuiltinAgentProvider;

/// An ordered row is retained until validation so duplicate keys are visible.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentEntry {
    pub key: String,
    pub value: String,
}

impl fmt::Debug for EnvironmentEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentEntry")
            .field("key", &self.key)
            .field("value", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomAgentProfile {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub base_provider: BuiltinAgentProvider,
    #[serde(default)]
    pub environment: Vec<EnvironmentEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomAgentSettings {
    #[serde(default)]
    pub profiles: Vec<CustomAgentProfile>,
}

/// Non-secret identity safe for pane projections and snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomAgentIdentity {
    pub profile_id: String,
    pub base_provider: BuiltinAgentProvider,
}

/// Transient launch input; deliberately not serializable into session snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCustomAgentLaunch {
    pub identity: CustomAgentIdentity,
    pub environment: Vec<EnvironmentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileValidationError {
    #[error("Profile ID must be nonempty and contain no control characters")]
    InvalidId,
    #[error("Profile name must not be blank or contain control characters")]
    InvalidName,
    #[error("Profile icon must not be blank or contain control characters")]
    InvalidIcon,
    #[error("Custom profiles support only Claude and Codex")]
    InvalidProvider,
    #[error("Environment row {row}: invalid variable name")]
    InvalidKey { row: usize },
    #[error("Environment row {row}: variable name is reserved by Ilium or the terminal")]
    ReservedKey { row: usize },
    #[error("Environment row {row}: duplicate variable name")]
    DuplicateKey { row: usize },
    #[error("Environment row {row}: value contains a NUL character")]
    InvalidValue { row: usize },
    #[error("Profile IDs must be unique")]
    DuplicateId,
    #[error("Profile names must be unique, ignoring case and surrounding whitespace")]
    DuplicateName,
}

/// The terminal adapter filters these variables even when inherited.
pub fn is_terminal_filtered_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    matches!(
        key.as_str(),
        "TERM_PROGRAM" | "TERM_PROGRAM_VERSION" | "TMUX" | "ZELLIJ" | "NO_COLOR"
    ) || ["WEZTERM_", "KITTY_", "TMUX_", "ZELLIJ_"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

pub fn is_reserved_environment_key(key: &str) -> bool {
    let normalized = key.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "TERM"
            | "ILIUM_PANE_ID"
            | "ILIUM_SESSION_NAME"
            | "ILIUM_SESSION_SOCKET"
            | "ILIUM_WORKTREE"
            | "ILIUM_BRANCH"
    ) || is_terminal_filtered_key(key)
}

impl CustomAgentProfile {
    pub fn launch_input(&self) -> Result<ResolvedCustomAgentLaunch, ProfileValidationError> {
        self.validate()?;
        Ok(ResolvedCustomAgentLaunch {
            identity: CustomAgentIdentity {
                profile_id: self.id.clone(),
                base_provider: self.base_provider,
            },
            environment: self.environment.clone(),
        })
    }

    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        if self.id.trim().is_empty() || self.id.chars().any(char::is_control) {
            return Err(ProfileValidationError::InvalidId);
        }
        if self.name.trim().is_empty() || self.name.chars().any(char::is_control) {
            return Err(ProfileValidationError::InvalidName);
        }
        if self.icon.trim().is_empty() || self.icon.chars().any(char::is_control) {
            return Err(ProfileValidationError::InvalidIcon);
        }
        if !matches!(
            self.base_provider,
            BuiltinAgentProvider::Claude | BuiltinAgentProvider::Codex
        ) {
            return Err(ProfileValidationError::InvalidProvider);
        }
        let mut keys = HashSet::new();
        for (index, entry) in self.environment.iter().enumerate() {
            let row = index + 1;
            if entry.key.is_empty() || entry.key.contains('=') || entry.key.contains('\0') {
                return Err(ProfileValidationError::InvalidKey { row });
            }
            if is_reserved_environment_key(&entry.key) {
                return Err(ProfileValidationError::ReservedKey { row });
            }
            let comparison = if cfg!(windows) {
                entry.key.to_uppercase()
            } else {
                entry.key.clone()
            };
            if !keys.insert(comparison) {
                return Err(ProfileValidationError::DuplicateKey { row });
            }
            if entry.value.contains('\0') {
                return Err(ProfileValidationError::InvalidValue { row });
            }
        }
        Ok(())
    }
}

pub fn validate_profiles(profiles: &[CustomAgentProfile]) -> Result<(), ProfileValidationError> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for profile in profiles {
        profile.validate()?;
        if !ids.insert(&profile.id) {
            return Err(ProfileValidationError::DuplicateId);
        }
        if !names.insert(profile.name.trim().to_lowercase()) {
            return Err(ProfileValidationError::DuplicateName);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> CustomAgentProfile {
        CustomAgentProfile {
            id: "stable-1".into(),
            name: "Work".into(),
            icon: "★".into(),
            base_provider: BuiltinAgentProvider::Claude,
            environment: vec![],
        }
    }

    #[test]
    fn names_ids_and_providers_are_validated() {
        let first = profile();
        let mut second = first.clone();
        second.id = "stable-2".into();
        second.name = "Personal".into();
        assert_eq!(validate_profiles(&[first.clone(), second.clone()]), Ok(()));
        let stable_id = second.id.clone();
        second.name = "Renamed".into();
        assert_eq!(second.id, stable_id);
        second.name = " work ".into();
        assert_eq!(
            validate_profiles(&[first.clone(), second.clone()]),
            Err(ProfileValidationError::DuplicateName)
        );
        second.name = "New".into();
        second.id = first.id.clone();
        assert_eq!(
            validate_profiles(&[first, second]),
            Err(ProfileValidationError::DuplicateId)
        );
        assert_eq!(validate_profiles(&[]), Ok(()));
    }

    #[test]
    fn values_are_literal_redacted_and_rows_validate_before_deduplication() {
        let mut item = profile();
        item.environment.push(EnvironmentEntry {
            key: "TOKEN".into(),
            value: "private = 'value'\n☃".into(),
        });
        item.environment.push(EnvironmentEntry {
            key: "EMPTY".into(),
            value: String::new(),
        });
        assert_eq!(item.validate(), Ok(()));
        assert!(!format!("{item:?}").contains("private"));
        let launch = item.launch_input().unwrap();
        assert_eq!(launch.identity.profile_id, item.id);
        assert_eq!(launch.environment[1].value, "");
        assert!(!format!("{launch:?}").contains("private"));
        item.environment.push(item.environment[0].clone());
        let error = item.validate().unwrap_err();
        assert_eq!(error, ProfileValidationError::DuplicateKey { row: 3 });
        assert!(!error.to_string().contains("private"));
        item.environment[2].key = "TERM".into();
        assert_eq!(
            item.validate(),
            Err(ProfileValidationError::ReservedKey { row: 3 })
        );
        item.environment[2].key = "bad=key".into();
        assert_eq!(
            item.validate(),
            Err(ProfileValidationError::InvalidKey { row: 3 })
        );
    }

    #[test]
    fn rejects_terminal_and_ilium_identity_keys_without_exposing_values() {
        for key in [
            "TERM",
            "term_program",
            "WEZTERM_VERSION",
            "KITTY_WINDOW_ID",
            "TMUX",
            "TMUX_PANE",
            "ZELLIJ_SESSION_NAME",
            "NO_COLOR",
            "ILIUM_PANE_ID",
            "ILIUM_SESSION_NAME",
            "ILIUM_SESSION_SOCKET",
            "ILIUM_WORKTREE",
            "ILIUM_BRANCH",
        ] {
            let mut item = profile();
            item.environment.push(EnvironmentEntry {
                key: key.into(),
                value: "sensitive-sentinel".into(),
            });
            let error = item.validate().unwrap_err();
            assert_eq!(error, ProfileValidationError::ReservedKey { row: 1 });
            assert!(!format!("{error:?}").contains("sensitive-sentinel"));
        }
        let mut item = profile();
        item.environment.push(EnvironmentEntry {
            key: "KEY".into(),
            value: "bad\0value".into(),
        });
        assert_eq!(
            item.validate(),
            Err(ProfileValidationError::InvalidValue { row: 1 })
        );
        item.base_provider = BuiltinAgentProvider::Antigravity;
        assert_eq!(
            item.validate(),
            Err(ProfileValidationError::InvalidProvider)
        );
    }
}
