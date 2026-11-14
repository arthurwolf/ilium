//! Settings and construction of `AmbientKind::Plugin`: one animation row whose Scene choice
//! lists every scene the plugin host registered (`external::external_scenes`). The choice
//! and the edited scene controls are saved in the project settings; the scene's own rows
//! come from the scene's factory, so a plugin scene is configured exactly like a built-in
//! one while colour, dithering and display stay the shared rows.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::control::{self, Control, ControlValue};
use crate::external::{external_scenes, intern, NoPluginScene, SceneValues};
use crate::scene::{Scene, SceneEnv};

/// Id of the row that picks the scene.
pub const SCENE_ROW_ID: &str = "scene";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSceneSettings {
    /// The chosen scene, `<plugin-id>.<scene-id>`. Empty, or an id nobody registers any
    /// more, shows the first registered scene.
    pub scene: String,
    /// Edited scene controls, by scene id then control id. Only edited values are stored.
    pub values: BTreeMap<String, SceneValues>,
}

impl PluginSceneSettings {
    pub fn normalized(&self) -> Self {
        self.clone()
    }

    /// The scene id shown now, or `None` while no plugin scene is registered.
    pub fn selected(&self) -> Option<String> {
        external_scenes().read().ok()?.resolve(&self.scene)
    }

    /// The Scene row followed by the selected scene's own rows.
    pub fn controls(&self) -> Vec<Control> {
        let Ok(scenes) = external_scenes().read() else {
            return Vec::new();
        };
        let entries = scenes.entries();
        let selected = scenes.resolve(&self.scene);
        let index = selected
            .as_ref()
            .and_then(|id| entries.iter().position(|(entry, _)| entry == id))
            .unwrap_or(0);
        let titles: Vec<&'static str> = entries.iter().map(|(_, title)| intern(title)).collect();
        let mut rows = vec![Control::choice(
            SCENE_ROW_ID,
            "Scene",
            index,
            &titles,
            "Which scene a plugin supplies is drawn. The scene's own settings follow; colour, dithering and display stay in the shared rows.",
        )];
        if let Some(id) = selected {
            let empty = SceneValues::new();
            let values = self.values.get(&id).unwrap_or(&empty);
            rows.extend(scenes.controls(&id, values));
        }
        rows
    }

    /// Applies one row edit; `Ok(true)` only when the saved settings changed.
    pub fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let scenes = external_scenes()
            .read()
            .map_err(|_| "the plugin scene registry is unavailable".to_owned())?;
        let selected = scenes.resolve(&self.scene);
        if id == SCENE_ROW_ID {
            let index =
                control::index(&value).ok_or_else(|| "Scene expects an option".to_owned())?;
            let entries = scenes.entries();
            let (chosen, _) = entries
                .get(index)
                .ok_or_else(|| format!("Scene: option {index} does not exist"))?;
            if selected.as_deref() == Some(chosen.as_str()) {
                return Ok(false);
            }
            self.scene = chosen.clone();
            return Ok(true);
        }
        let Some(selected) = selected else {
            return Ok(false);
        };
        let values = self.values.entry(selected.clone()).or_default();
        let changed = scenes.set_control(&selected, values, id, value)?;
        let now_empty = values.is_empty();
        if now_empty {
            self.values.remove(&selected);
        }
        Ok(changed)
    }

    /// Identity of what the scene shows: the scene and its edited values.
    pub fn scene_key(&self) -> String {
        let Some(selected) = self.selected() else {
            return "none".to_owned();
        };
        let values = self
            .values
            .get(&selected)
            .map(|values| serde_json::to_string(values).unwrap_or_default())
            .unwrap_or_default();
        format!("{selected}|{values}")
    }

    /// Builds the selected scene (already wrapped in `PaletteScene` by the registry), or a
    /// scene that says why nothing is drawn.
    pub fn create_scene(&self, env: &SceneEnv) -> Box<dyn Scene> {
        let Ok(scenes) = external_scenes().read() else {
            return Box::new(NoPluginScene);
        };
        let Some(selected) = scenes.resolve(&self.scene) else {
            return Box::new(NoPluginScene);
        };
        let empty = SceneValues::new();
        let values = self.values.get(&selected).unwrap_or(&empty);
        scenes
            .create_scene(&selected, env, values)
            .unwrap_or_else(|| Box::new(NoPluginScene))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::ExternalSceneFactory;
    use crate::scene::Frame;

    struct Blank;

    impl Scene for Blank {
        fn render(&mut self, _frame: &mut Frame<'_>) {}
    }

    struct Factory {
        id: &'static str,
    }

    impl ExternalSceneFactory for Factory {
        fn id(&self) -> &str {
            self.id
        }
        fn title(&self) -> &str {
            "Plugin scene settings test"
        }
        fn controls(&self, values: &SceneValues) -> Vec<Control> {
            let level = match values.get("level") {
                Some(ControlValue::Number(level)) => *level,
                _ => 5,
            };
            vec![Control::slider(
                "level",
                "Level",
                level,
                (0, 10, 1),
                "",
                "Test level.",
            )]
        }
        fn set_control(
            &self,
            values: &mut SceneValues,
            id: &str,
            value: ControlValue,
        ) -> Result<bool, String> {
            match (id, value) {
                ("level", ControlValue::Number(level)) => {
                    let level = level.clamp(0, 10);
                    let current = match values.get("level") {
                        Some(ControlValue::Number(current)) => *current,
                        _ => 5,
                    };
                    if current == level {
                        return Ok(false);
                    }
                    values.insert("level".into(), ControlValue::Number(level));
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        fn create(&self, _env: &SceneEnv, _values: &SceneValues) -> Box<dyn Scene> {
            Box::new(Blank)
        }
    }

    /// The registry is process-global, so every assertion that needs a registered scene
    /// lives in this one test with an id no other test uses.
    #[test]
    fn the_scene_row_lists_registered_scenes_and_persists_edits() {
        let id = "ilium.test.plugin-scene-settings";
        if let Ok(mut scenes) = external_scenes().write() {
            scenes.unregister(id);
            scenes.register(Box::new(Factory { id })).unwrap();
        }
        let mut settings = PluginSceneSettings {
            scene: id.to_owned(),
            ..PluginSceneSettings::default()
        };
        let rows = settings.controls();
        assert_eq!(rows[0].id, SCENE_ROW_ID);
        assert_eq!(rows[1].id, "level");
        assert_eq!(settings.scene_key(), format!("{id}|"));

        assert_eq!(settings.set_control("level", ControlValue::Number(5)), Ok(false));
        assert_eq!(settings.set_control("level", ControlValue::Number(8)), Ok(true));
        assert_eq!(settings.controls()[1].value, ControlValue::Number(8));
        assert_ne!(settings.scene_key(), format!("{id}|"));

        let saved = serde_json::to_string(&settings).unwrap();
        let restored: PluginSceneSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, settings);

        let mut unknown = PluginSceneSettings {
            scene: "gone.plugin.scene".to_owned(),
            ..PluginSceneSettings::default()
        };
        assert!(unknown.selected().is_some(), "falls back to a registered scene");
        assert_eq!(
            unknown.set_control(SCENE_ROW_ID, ControlValue::Index(9999)),
            Err("Scene: option 9999 does not exist".to_owned())
        );
        if let Ok(mut scenes) = external_scenes().write() {
            scenes.unregister(id);
        }
    }
}
