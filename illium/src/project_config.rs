//! Persistent, project-scoped Illium settings stored in `.illium/config.yaml`.
//!
//! This deliberately owns a different file from `workspace_file`: workspace
//! snapshots are volatile session recovery state, while this configuration
//! contains durable project metadata that must survive a fresh session.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const RELATIVE_PATH: &str = ".illium/config.yaml";

/// Configuration values Illium owns, plus unknown fields preserved across
/// reads and writes so later settings are never erased by name inference.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ProjectConfig {
    #[serde(
        rename = "project name",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project_name: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl ProjectConfig {
    /// Starts a configuration with only the durable project-name field set.
    #[cfg(test)]
    pub fn with_project_name(project_name: impl Into<String>) -> Self {
        Self {
            project_name: Some(project_name.into()),
            extra: BTreeMap::new(),
        }
    }
}

/// Reads the project configuration. An absent file is a clean, empty config.
pub fn load(cwd: &Path) -> anyhow::Result<ProjectConfig> {
    let path = cwd.join(RELATIVE_PATH);
    if !path.exists() {
        return Ok(ProjectConfig::default());
    }
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_norway::from_str(&contents)?)
}

/// Atomically stores project configuration without ever touching session state.
pub fn save(cwd: &Path, config: &ProjectConfig) -> anyhow::Result<()> {
    let path = cwd.join(RELATIVE_PATH);
    let Some(parent) = path.parent() else {
        anyhow::bail!("project config path {path:?} has no parent");
    };
    std::fs::create_dir_all(parent)?;

    let yaml = serde_norway::to_string(config)?;
    let temporary_path = parent.join(format!(".config.yaml.tmp-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&temporary_path)?;
        file.write_all(yaml.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(temporary_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("illium-project-config-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn save_and_load_use_the_requested_project_name_property() {
        let cwd = scratch_dir();
        let config = ProjectConfig {
            project_name: Some("Illium".to_string()),
            extra: BTreeMap::new(),
        };

        save(&cwd, &config).unwrap();
        assert_eq!(
            std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap(),
            "project name: Illium\n"
        );
        assert_eq!(load(&cwd).unwrap().project_name.as_deref(), Some("Illium"));
    }

    #[test]
    fn saving_a_name_preserves_unknown_configuration() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".illium")).unwrap();
        std::fs::write(cwd.join(RELATIVE_PATH), "theme: dusk\n").unwrap();

        let mut config = load(&cwd).unwrap();
        config.project_name = Some("Moonlight".to_string());
        save(&cwd, &config).unwrap();

        let saved = std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("theme: dusk"));
        assert!(saved.contains("project name: Moonlight"));
    }
}
