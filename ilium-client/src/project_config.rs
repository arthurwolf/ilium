//! Persistent, project-scoped Ilium settings stored in `.ilium/config.yaml`.
//!
//! This deliberately owns a different file from `workspace_file`: workspace
//! snapshots are volatile session recovery state, while this configuration
//! contains durable project metadata that must survive a fresh session.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use ilium_platform::file_lock::ExclusiveFileLock;
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
    #[serde(rename = "show project separators", default)]
    pub show_project_separators: bool,
    #[serde(default, skip_serializing_if = "animation_is_default")]
    pub animation: crate::background_animation::AnimationSettings,
    // `serde_norway::Value`, not `serde_json::Value`: the JSON data model has
    // no representation for YAML-only values (non-finite floats like `.inf`,
    // `.nan`), so round-tripping through it silently rewrote them to `null`
    // and violated the "unknown fields preserved" contract above.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn animation_is_default(settings: &crate::background_animation::AnimationSettings) -> bool {
    *settings == crate::background_animation::AnimationSettings::default()
}

/// Atomically merges the project animation preference with current metadata.
pub fn set_animation(
    cwd: &Path,
    settings: crate::background_animation::AnimationSettings,
) -> anyhow::Result<()> {
    update(cwd, |config| config.animation = settings.normalized())
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

/// Applies a project-config update while preserving fields written by a
/// concurrent naming or settings operation in another attached client.
pub fn update(cwd: &Path, mutate: impl FnOnce(&mut ProjectConfig)) -> anyhow::Result<()> {
    let path = cwd.join(RELATIVE_PATH);
    let Some(parent) = path.parent() else {
        anyhow::bail!("project config path {path:?} has no parent");
    };
    std::fs::create_dir_all(parent)?;

    let lock_path = parent.join(".config.yaml.lock");
    let _lock = ExclusiveFileLock::acquire(&lock_path)?;
    let mut config = load(cwd)?;
    mutate(&mut config);
    save_unlocked(cwd, &config)
}

/// Persists one project-scoped UI setting without replacing project metadata.
pub fn set_show_project_separators(cwd: &Path, enabled: bool) -> anyhow::Result<()> {
    update(cwd, |config| config.show_project_separators = enabled)
}

/// Atomically stores a config snapshot. Callers must hold `.config.yaml.lock`.
fn save_unlocked(cwd: &Path, config: &ProjectConfig) -> anyhow::Result<()> {
    let path = cwd.join(RELATIVE_PATH);
    let Some(parent) = path.parent() else {
        anyhow::bail!("project config path {path:?} has no parent");
    };
    std::fs::create_dir_all(parent)?;

    let yaml = serde_norway::to_string(config)?;
    // Process id alone is not enough to make this path unique: two calls to
    // `save` for the same `cwd` racing within the same process would both
    // target the identical temp file, and since `File::create` truncates
    // rather than failing on an existing file, their writes could interleave
    // and corrupt the temp file's contents before either side gets to
    // rename it -- defeating the atomicity this write-then-rename dance
    // exists for. A process-wide counter (matching `workspace_file::save`'s
    // fix for the identical bug) makes every call's temp path distinct
    // regardless of which thread or task it runs on.
    static SAVE_CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call_id = SAVE_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary_path = parent.join(format!(".config.yaml.tmp-{}-{call_id}", std::process::id()));
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

    #[test]
    fn animation_round_trip_preserves_metadata_and_isolates_projects() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        update(first.path(), |config| {
            config.project_name = Some("Beach".into())
        })
        .unwrap();
        let settings = crate::background_animation::AnimationSettings {
            kind: crate::background_animation::AnimationKind::Kelp,
            speed_percent: 150,
            ..Default::default()
        };
        set_animation(first.path(), settings).unwrap();
        let reloaded = load(first.path()).unwrap();
        assert_eq!(reloaded.animation, settings);
        assert_eq!(reloaded.project_name.as_deref(), Some("Beach"));
        assert_eq!(load(second.path()).unwrap().animation, Default::default());
        update(first.path(), |config| {
            config.project_icon = Some("x".into())
        })
        .unwrap();
        assert_eq!(load(first.path()).unwrap().animation, settings);
    }

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
        update(&cwd, |latest| {
            latest.project_name = Some("Ilium".to_string())
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap(),
            "project name: Ilium\nshow project separators: false\n"
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

        update(&cwd, |_| {}).unwrap();

        let saved = std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("ratio: .inf"), "got: {saved}");
    }

    #[test]
    fn saving_a_name_preserves_unknown_configuration() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(cwd.join(RELATIVE_PATH), "theme: dusk\n").unwrap();

        update(&cwd, |config| {
            config.project_name = Some("Moonlight".to_string())
        })
        .unwrap();

        let saved = std::fs::read_to_string(cwd.join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("theme: dusk"));
        assert!(saved.contains("project name: Moonlight"));
    }

    #[test]
    fn project_subtree_separators_default_off_and_update_without_losing_metadata() {
        let cwd = scratch_dir();
        std::fs::create_dir_all(cwd.join(".ilium")).unwrap();
        std::fs::write(
            cwd.join(RELATIVE_PATH),
            "project name: Ilium\nproject icon: 🧭\ncustom: keep-me\n",
        )
        .unwrap();

        let before = load(&cwd).unwrap();
        assert!(!before.show_project_separators);

        set_show_project_separators(&cwd, true).unwrap();

        let after = load(&cwd).unwrap();
        assert!(after.show_project_separators);
        assert_eq!(after.project_name.as_deref(), Some("Ilium"));
        assert_eq!(after.project_icon.as_deref(), Some("🧭"));
        assert!(std::fs::read_to_string(cwd.join(RELATIVE_PATH))
            .unwrap()
            .contains("custom: keep-me"));
    }

    #[test]
    fn concurrent_project_config_updates_preserve_both_fields() {
        let cwd = scratch_dir();
        let first_cwd = cwd.clone();
        let second_cwd = cwd.clone();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let first_barrier = barrier.clone();
        let second_barrier = barrier.clone();

        let name_writer = std::thread::spawn(move || {
            first_barrier.wait();
            update(&first_cwd, |config| {
                config.project_name = Some("Concurrent name".to_string());
                config.project_icon = Some("🧭".to_string());
            })
            .unwrap();
        });
        let setting_writer = std::thread::spawn(move || {
            second_barrier.wait();
            set_show_project_separators(&second_cwd, true).unwrap();
        });
        barrier.wait();
        name_writer.join().unwrap();
        setting_writer.join().unwrap();

        let config = load(&cwd).unwrap();
        assert_eq!(config.project_name.as_deref(), Some("Concurrent name"));
        assert_eq!(config.project_icon.as_deref(), Some("🧭"));
        assert!(config.show_project_separators);
    }

    #[test]
    fn animation_palette_and_named_parameters_survive_yaml_reload_and_metadata_updates() {
        use crate::background_animation::{AnimationKind, AnimationSettings};

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".ilium")).unwrap();
        std::fs::write(
            project.path().join(RELATIVE_PATH),
            "project name: Pond\ncustom: keep-me\nratio: .inf\nanimation:\n  kind: breathing_mountain\n  speed_percent: 125\n",
        )
        .unwrap();
        let initial = load(project.path()).unwrap();
        assert_eq!(initial.animation.kind, AnimationKind::QuietPond);
        assert_eq!(initial.animation.lightness_percent, 60);
        assert_eq!(initial.animation.speed_percent, 125);

        let mut settings = AnimationSettings {
            lightness_percent: 35,
            hue_degrees: 125,
            saturation_percent: 50,
            ..initial.animation
        };
        for (index, kind) in AnimationKind::ALL.into_iter().enumerate() {
            settings.kind = kind;
            for row in 17..=20 {
                let slider = settings.slider(row).unwrap();
                settings.set_slider_value(
                    row,
                    if index % 2 == 0 {
                        slider.minimum
                    } else {
                        slider.maximum
                    },
                );
            }
        }
        set_animation(project.path(), settings).unwrap();
        update(project.path(), |config| {
            config.project_icon = Some("🧭".into())
        })
        .unwrap();

        let reloaded = load(project.path()).unwrap();
        assert_eq!(reloaded.animation, settings);
        assert_eq!(reloaded.project_name.as_deref(), Some("Pond"));
        assert_eq!(reloaded.project_icon.as_deref(), Some("🧭"));
        let saved = std::fs::read_to_string(project.path().join(RELATIVE_PATH)).unwrap();
        assert!(saved.contains("kind: quiet_pond"));
        assert!(!saved.contains("breathing_mountain"));
        assert!(saved.contains("custom: keep-me"));
        assert!(saved.contains("ratio: .inf"));
    }
}
