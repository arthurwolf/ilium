//! Persistent, project-scoped Ilium settings stored in `.ilium/config.yaml`.
//!
//! This deliberately owns a different file from `workspace_file`: workspace
//! snapshots are volatile session recovery state, while this configuration
//! contains durable project metadata that must survive a fresh session.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_norway::Value;

const RELATIVE_PATH: &str = ".ilium/config.yaml";

/// Configuration values Ilium owns, plus unknown fields preserved across
/// reads and writes so later settings are never erased by name inference.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ProjectConfig {
    #[serde(
        rename = "project name",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project_name: Option<String>,
    #[serde(
        rename = "project icon",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project_icon: Option<String>,
    // `serde_norway::Value`, not `serde_json::Value`: the JSON data model has
    // no representation for YAML-only values (non-finite floats like `.inf`,
    // `.nan`), so round-tripping through it silently rewrote them to `null`
    // and violated the "unknown fields preserved" contract above.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl ProjectConfig {
    /// Starts a configuration with only the durable project-name field set.
    #[cfg(test)]
    pub fn with_project_name(project_name: impl Into<String>) -> Self {
        Self {
            project_name: Some(project_name.into()),
            project_icon: None,
            extra: BTreeMap::new(),
        }
    }
}

/// Reads the project configuration. An absent file is a clean, empty config.
pub fn load(cwd: &Path) -> anyhow::Result<ProjectConfig> {
    let path = cwd.join(RELATIVE_PATH);
    // Read directly instead of checking `path.exists()` first: a separate
    // exists-then-read pair races against concurrent deletion/rename of the
    // file and would surface as a spurious error instead of the documented
    // "absent file is a clean, empty config" behavior.
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectConfig::default());
        }
        Err(error) => return Err(error.into()),
    };
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
    // Written in a closure so a failure partway through (create/write/sync)
    // falls through to the cleanup below instead of leaking the temp file
    // in `.ilium/` forever.
    let write_result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::File::create(&temporary_path)?;
        file.write_all(yaml.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&temporary_path, path) {
        // Rename failed after the temp file was fully written; clean it up
        // so a failed save doesn't leave a stray file behind in `.ilium/`.
        let _ = std::fs::remove_file(&temporary_path);
        return Err(err.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("ilium-project-config-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn save_and_load_use_the_requested_project_name_property() {
        let cwd = scratch_dir();
        let config = ProjectConfig {
            project_name: Some("Ilium".to_string()),
            project_icon: None,
            extra: BTreeMap::new(),
        };

        save(&cwd, &config).unwrap();
        assert_eq!(
            std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap(),
            "project name: Ilium\n"
        );
        assert_eq!(load(&cwd).unwrap().project_name.as_deref(), Some("Ilium"));
    }

    #[test]
    fn saving_a_name_preserves_a_non_finite_float_in_unknown_configuration() {
        // Regression test: `extra` used to be `BTreeMap<String, serde_json::Value>`,
        // and JSON has no representation for non-finite floats, so re-saving
        // this file used to silently rewrite `ratio: .inf` to `ratio: null`.
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(cwd.join(RELATIVE_PATH), "ratio: .inf\n").unwrap();

        let config = load(&cwd).unwrap();
        save(&cwd, &config).unwrap();

        let saved = std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("ratio: .inf"), "got: {saved}");
    }

    #[test]
    fn saving_a_name_preserves_unknown_configuration() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(cwd.join(RELATIVE_PATH), "theme: dusk\n").unwrap();

        let mut config = load(&cwd).unwrap();
        config.project_name = Some("Moonlight".to_string());
        save(&cwd, &config).unwrap();

        let saved = std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("theme: dusk"));
        assert!(saved.contains("project name: Moonlight"));
    }
}
