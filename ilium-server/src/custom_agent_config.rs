//! Resolve custom agents from the current private configuration at launch time.
//!
//! Environment values remain in memory and are never part of the pane snapshot.

use std::io::Read;
use std::path::Path;

use ilium_core::custom_agent::{
    validate_profiles, CustomAgentIdentity, CustomAgentProfile, CustomAgentSettings,
};
use serde::Deserialize;

#[derive(Default, Deserialize)]
struct Document {
    #[serde(default)]
    custom_agents: CustomAgentSettings,
}

/// Errors deliberately omit the TOML parser's source excerpt and any values.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("custom agent configuration could not be read")]
    Read,
    #[error("custom agent configuration exceeds the 256 KiB configuration limit")]
    TooLarge,
    #[error("custom agent configuration is not valid TOML")]
    Parse,
    #[error("custom agent configuration is invalid: {0}")]
    Invalid(ilium_core::custom_agent::ProfileValidationError),
    #[error("custom agent profile is missing; restore or reassign it before restarting")]
    Missing,
    #[error("custom agent profile now uses a different provider; choose a compatible destination")]
    ProviderChanged,
}

/// Read afresh for every spawn or restart; no client connection is required.
pub fn resolve(
    config_dir: &Path,
    identity: &CustomAgentIdentity,
) -> Result<CustomAgentProfile, ResolveError> {
    let file = match ilium_platform::secure_fs::open_regular_file(&config_dir.join("config.toml")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ResolveError::Missing)
        }
        Err(_) => return Err(ResolveError::Read),
    };
    let mut contents = String::new();
    file.take((crate::config::MAX_CONFIG_BYTES + 1) as u64)
        .read_to_string(&mut contents)
        .map_err(|_| ResolveError::Read)?;
    if contents.len() > crate::config::MAX_CONFIG_BYTES {
        return Err(ResolveError::TooLarge);
    }
    let document: Document = toml::from_str(&contents).map_err(|_| ResolveError::Parse)?;
    validate_profiles(&document.custom_agents.profiles).map_err(ResolveError::Invalid)?;
    let profile = document
        .custom_agents
        .profiles
        .into_iter()
        .find(|profile| profile.id == identity.profile_id)
        .ok_or(ResolveError::Missing)?;
    if profile.base_provider != identity.base_provider {
        return Err(ResolveError::ProviderChanged);
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::BuiltinAgentProvider;

    #[test]
    fn reads_current_values_and_refuses_missing_or_changed_provider() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let identity = CustomAgentIdentity {
            profile_id: "stable".into(),
            base_provider: BuiltinAgentProvider::Claude,
        };
        assert!(matches!(
            resolve(directory.path(), &identity),
            Err(ResolveError::Missing)
        ));
        std::fs::write(&path, "[[custom_agents.profiles]]\nid = 'stable'\nname = 'Work'\nicon = '★'\nbase_provider = 'Claude'\n[[custom_agents.profiles.environment]]\nkey = 'TOKEN'\nvalue = 'sentinel-one'\n").unwrap();
        assert_eq!(
            resolve(directory.path(), &identity).unwrap().environment[0].value,
            "sentinel-one"
        );
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("sentinel-one", "sentinel-two");
        std::fs::write(&path, text).unwrap();
        assert_eq!(
            resolve(directory.path(), &identity).unwrap().environment[0].value,
            "sentinel-two"
        );
        let changed = CustomAgentIdentity {
            base_provider: BuiltinAgentProvider::Codex,
            ..identity
        };
        assert!(matches!(
            resolve(directory.path(), &changed),
            Err(ResolveError::ProviderChanged)
        ));
    }

    #[test]
    fn malformed_profile_configuration_does_not_echo_values() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            "[[custom_agents.profiles]]\nvalue = 'sensitive-sentinel\n",
        )
        .unwrap();
        let identity = CustomAgentIdentity {
            profile_id: "stable".into(),
            base_provider: BuiltinAgentProvider::Claude,
        };
        let error = resolve(directory.path(), &identity).unwrap_err();
        assert!(matches!(&error, ResolveError::Parse));
        assert!(!format!("{error:?} {error}").contains("sensitive-sentinel"));
    }

    #[test]
    fn refuses_reserved_environment_from_a_manually_edited_file() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.toml"), "[[custom_agents.profiles]]\nid = 'stable'\nname = 'Work'\nicon = '★'\nbase_provider = 'Claude'\n[[custom_agents.profiles.environment]]\nkey = 'ILIUM_PANE_ID'\nvalue = 'sensitive-sentinel'\n").unwrap();
        let identity = CustomAgentIdentity {
            profile_id: "stable".into(),
            base_provider: BuiltinAgentProvider::Claude,
        };
        let error = resolve(directory.path(), &identity).unwrap_err();
        assert!(matches!(&error, ResolveError::Invalid(_)));
        assert!(!format!("{error:?} {error}").contains("sensitive-sentinel"));
    }
}
