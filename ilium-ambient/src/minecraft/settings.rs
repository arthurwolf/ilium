//! Saved-map source controls; resource-pack controls belong to the shared renderer.
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldSource {
    #[default]
    Generated,
    SavedMaps,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedMapsSettings {
    pub source: WorldSource,
    /// Empty selects the official Java saves directory. Never trim an authored path.
    pub saves_folder: String,
}

impl SavedMapsSettings {
    pub fn saves_root(&self) -> Result<PathBuf, String> {
        if let Some(root) = explicit_root(&self.saves_folder)? {
            return Ok(root);
        }
        ilium_platform::minecraft::java_directory()
            .map(|directory| directory.join("saves"))
            .ok_or_else(|| {
                "Set a saved maps folder; the Java installation location is unavailable".into()
            })
    }
}

fn explicit_root(value: &str) -> Result<Option<PathBuf>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 4096 || value.contains('\0') {
        return Err("Saved maps folder must be at most 4096 bytes and contain no NUL".into());
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(
            "Saved maps folder must be an absolute path, or blank for automatic discovery".into(),
        );
    }
    Ok(Some(path))
}

impl SceneSettings for SavedMapsSettings {
    fn normalized(&self) -> Self {
        // Keep authored paths intact. A malformed persisted value produces a
        // visible saves_root error rather than silently redirecting discovery.
        self.clone()
    }
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::choice("world_source", "World source", match self.source {
                WorldSource::Generated => 0,
                WorldSource::SavedMaps => 1,
            }, &["Generated", "Saved maps"], "Read your saved Java worlds without modifying them, or explore generated terrain."),
            Control::text("saved_maps_folder", "Saved maps folder", &self.saves_folder, "Automatic Java saves folder", "Absolute folder containing your worlds. Leave blank to use the official Java saves location for this computer."),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "world_source" => {
                let source = match control::index(&value) {
                    Some(0) => WorldSource::Generated,
                    Some(1) => WorldSource::SavedMaps,
                    _ => return Err("Expected Generated or Saved maps".into()),
                };
                let changed = self.source != source;
                self.source = source;
                Ok(changed)
            }
            "saved_maps_folder" => {
                let folder = control::text(&value).ok_or("Expected a saved maps folder")?;
                explicit_root(folder)?;
                let changed = self.saves_folder != folder;
                if changed {
                    self.saves_folder = folder.to_owned();
                }
                Ok(changed)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_choice_and_folder_survive_typed_settings_reload() {
        let mut settings = SavedMapsSettings::default();
        let folder = std::env::temp_dir()
            .join("saved worlds 雪 ")
            .to_str()
            .unwrap()
            .to_owned();
        assert!(settings
            .set_control("world_source", ControlValue::Index(1))
            .unwrap());
        assert!(settings
            .set_control("saved_maps_folder", ControlValue::Text(folder.clone()))
            .unwrap());
        let reloaded: SavedMapsSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(reloaded, settings);
        assert_eq!(reloaded.source, WorldSource::SavedMaps);
        assert_eq!(reloaded.saves_root().unwrap(), PathBuf::from(folder));
        assert_eq!(reloaded.controls()[0].value, ControlValue::Index(1));
        assert_eq!(
            reloaded.controls()[1].value,
            ControlValue::Text(reloaded.saves_folder.clone())
        );
    }

    #[test]
    fn bad_edits_preserve_settings_and_valid_unchanged_edits_are_noops() {
        let mut settings = SavedMapsSettings::default();
        for (id, value) in [
            ("world_source", ControlValue::Index(2)),
            ("world_source", ControlValue::Text("Saved maps".into())),
            ("saved_maps_folder", ControlValue::Bool(true)),
            (
                "saved_maps_folder",
                ControlValue::Text("relative/folder".into()),
            ),
            ("saved_maps_folder", ControlValue::Text("bad\0path".into())),
            ("saved_maps_folder", ControlValue::Text("x".repeat(4097))),
        ] {
            let before = settings.clone();
            assert!(settings.set_control(id, value).is_err());
            assert_eq!(settings, before);
        }
        assert!(!settings
            .set_control("world_source", ControlValue::Index(0))
            .unwrap());
        assert!(!settings
            .set_control("saved_maps_folder", ControlValue::Text(String::new()))
            .unwrap());
        assert!(!settings
            .set_control("other", ControlValue::Bool(true))
            .unwrap());
    }

    #[test]
    fn default_root_uses_platform_directory_without_creating_it() {
        let settings: SavedMapsSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.source, WorldSource::Generated);
        match ilium_platform::minecraft::java_directory() {
            Some(root) => assert_eq!(settings.saves_root().unwrap(), root.join("saves")),
            None => assert!(settings.saves_root().is_err()),
        }
        let malformed: SavedMapsSettings =
            serde_json::from_str(r#"{"saves_folder":"relative"}"#).unwrap();
        assert!(malformed.saves_root().is_err());
        assert_eq!(malformed.normalized().saves_folder, "relative");
    }
}
