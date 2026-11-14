//! Scenes supplied from outside this crate (gap G9b): a plugin's scene renderer, hosted by
//! the client. The engine only knows the factory contract; how the frames are produced (a
//! WebAssembly call, a test double) is the factory's business.
//!
//! An external scene gets the shared look for free, like every built-in one (CLAUDE.md,
//! "Background animations: the shared look is mandatory"): it is built with the current
//! palette, it is always wrapped in `PaletteScene`, and colour, dithering and display stay
//! the host's shared rows. An external scene therefore declares only its own controls.
//!
//! PALETTE (future plugin contract): the factory receives the current palette in
//! `SceneEnv::palette` when the scene is built and later changes through
//! `Scene::set_palette`; a plugin scene with natural colours must follow it.

use std::collections::BTreeMap;

use crate::control::{Control, ControlValue};
use crate::scene::{PaletteScene, Scene, SceneEnv};

/// Longest id of an external scene, in bytes.
pub const MAXIMUM_EXTERNAL_SCENE_ID_BYTES: usize = 256;

/// Builds the scenes of one external source, for example one plugin scene contribution.
pub trait ExternalSceneFactory: Send + Sync {
    /// The global id of the scene, `<plugin-id>.<scene-id>`; unique among external scenes.
    fn id(&self) -> &str;
    /// The name shown in the background animation list.
    fn title(&self) -> &str;
    /// The scene's own settings rows, never colour, dither or display rows (those are the
    /// shared look's).
    fn controls(&self) -> Vec<Control> {
        Vec::new()
    }
    /// Applies one edit of a scene control: `Ok(true)` when something changed, `Ok(false)`
    /// for an unknown id or an unchanged value, `Err` for invalid input to show the user.
    fn set_control(&self, _id: &str, _value: ControlValue) -> Result<bool, String> {
        Ok(false)
    }
    /// Builds a fresh scene. `env.palette` holds the shared palette to follow.
    fn create(&self, env: &SceneEnv) -> Box<dyn Scene>;
}

/// Why an external scene was not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExternalSceneError {
    /// The id is empty, too long, or has characters other than `a-z 0-9 . -`.
    #[error("invalid external scene id {0:?}")]
    InvalidId(String),
    /// Another factory already uses the id.
    #[error("an external scene with id {0} is already registered")]
    Duplicate(String),
}

/// The external scenes the host knows, by id, in id order.
#[derive(Default)]
pub struct ExternalScenes {
    factories: BTreeMap<String, Box<dyn ExternalSceneFactory>>,
}

impl std::fmt::Debug for ExternalScenes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalScenes")
            .field("ids", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Whether `id` is a valid external scene id.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAXIMUM_EXTERNAL_SCENE_ID_BYTES
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
        && !id.starts_with('.')
        && !id.ends_with('.')
}

impl ExternalScenes {
    /// Registers a factory. Its id must be valid and new.
    pub fn register(
        &mut self,
        factory: Box<dyn ExternalSceneFactory>,
    ) -> Result<(), ExternalSceneError> {
        let id = factory.id().to_owned();
        if !is_valid_id(&id) {
            return Err(ExternalSceneError::InvalidId(id));
        }
        if self.factories.contains_key(&id) {
            return Err(ExternalSceneError::Duplicate(id));
        }
        self.factories.insert(id, factory);
        Ok(())
    }

    /// Forgets the factory with this id (its plugin went away). Returns whether it was known.
    pub fn unregister(&mut self, id: &str) -> bool {
        self.factories.remove(id).is_some()
    }

    /// Forgets every factory whose id starts with `<plugin>.`. Returns how many went.
    pub fn unregister_plugin(&mut self, plugin: &str) -> usize {
        let prefix = format!("{plugin}.");
        let before = self.factories.len();
        self.factories.retain(|id, _| !id.starts_with(&prefix));
        before - self.factories.len()
    }

    /// `(id, title)` of every external scene, for the background animation list.
    pub fn entries(&self) -> Vec<(String, String)> {
        self.factories
            .iter()
            .map(|(id, factory)| (id.clone(), factory.title().to_owned()))
            .collect()
    }

    /// The scene controls of one external scene; empty for an unknown id.
    pub fn controls(&self, id: &str) -> Vec<Control> {
        self.factories
            .get(id)
            .map(|factory| factory.controls())
            .unwrap_or_default()
    }

    /// Applies one control edit to an external scene; `Ok(false)` for an unknown id.
    pub fn set_control(
        &self,
        id: &str,
        control: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        match self.factories.get(id) {
            Some(factory) => factory.set_control(control, value),
            None => Ok(false),
        }
    }

    /// Builds the external scene `id`, wrapped in `PaletteScene` like every built-in scene so
    /// the shared look applies to it. `None` for an unknown id.
    pub fn create_scene(&self, id: &str, env: &SceneEnv) -> Option<Box<dyn Scene>> {
        let factory = self.factories.get(id)?;
        Some(Box::new(PaletteScene::new(
            factory.create(env),
            env.palette.clone(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use crate::scene::Frame;
    use crate::style::ScenePalette;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};

    /// A plugin-like scene: one red cell colour, and it records the palette it was given.
    struct RedScene {
        seen_palette: Arc<Mutex<Option<ScenePalette>>>,
    }

    impl Scene for RedScene {
        fn render(&mut self, frame: &mut Frame<'_>) {
            frame.cell_colors.clear();
            frame.cell_colors.resize(4, [200, 40, 40]);
        }
        fn uses_cell_colors(&self) -> bool {
            true
        }
        fn set_palette(&mut self, palette: &ScenePalette) {
            if let Ok(mut seen) = self.seen_palette.lock() {
                *seen = Some(palette.clone());
            }
        }
    }

    struct RedFactory {
        id: String,
        seen_palette: Arc<Mutex<Option<ScenePalette>>>,
        speed: Mutex<i32>,
    }

    impl ExternalSceneFactory for RedFactory {
        fn id(&self) -> &str {
            &self.id
        }
        fn title(&self) -> &str {
            "Red"
        }
        fn set_control(&self, id: &str, value: ControlValue) -> Result<bool, String> {
            match (id, value) {
                ("speed", ControlValue::Number(speed)) => {
                    let mut current = self.speed.lock().map_err(|_| "poisoned".to_owned())?;
                    let changed = *current != speed;
                    *current = speed;
                    Ok(changed)
                }
                _ => Ok(false),
            }
        }
        fn create(&self, env: &SceneEnv) -> Box<dyn Scene> {
            if let Ok(mut seen) = self.seen_palette.lock() {
                *seen = Some(env.palette.clone());
            }
            Box::new(RedScene {
                seen_palette: Arc::clone(&self.seen_palette),
            })
        }
    }

    fn factory(id: &str) -> (Box<RedFactory>, Arc<Mutex<Option<ScenePalette>>>) {
        let seen = Arc::new(Mutex::new(None));
        (
            Box::new(RedFactory {
                id: id.to_owned(),
                seen_palette: Arc::clone(&seen),
                speed: Mutex::new(1),
            }),
            seen,
        )
    }

    fn env(palette: ScenePalette) -> SceneEnv {
        let directory = std::env::temp_dir().join("ilium-ambient-external-scene-test");
        let mut env = SceneEnv::for_test(directory, crate::resources::test_resources());
        env.palette = palette;
        env
    }

    fn render(scene: &mut dyn Scene) -> Vec<[u8; 3]> {
        let mut raster = Raster::default();
        raster.resize(4, 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 2,
            height: 2,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        });
        colors
    }

    #[test]
    fn an_external_scene_is_built_with_the_palette_and_recoloured_onto_it() {
        let mut scenes = ExternalScenes::default();
        let (red, seen) = factory("example.weather.rain");
        scenes.register(red).unwrap();
        let palette = ScenePalette {
            stops: vec![[0, 0, 255], [0, 255, 0]],
            reverse: false,
            shift_percent: 0,
        };
        let mut scene = scenes
            .create_scene("example.weather.rain", &env(palette.clone()))
            .unwrap();
        assert_eq!(seen.lock().unwrap().as_ref(), Some(&palette));
        // The scene drew red; the shared look moved it onto the blue-green palette.
        let colors = render(scene.as_mut());
        assert_eq!(colors.len(), 4);
        assert!(colors.iter().all(|color| color[0] < 200), "{colors:?}");
    }

    #[test]
    fn without_a_palette_an_external_scene_keeps_its_own_colours() {
        let mut scenes = ExternalScenes::default();
        let (red, _) = factory("example.weather.rain");
        scenes.register(red).unwrap();
        let mut scene = scenes
            .create_scene("example.weather.rain", &env(ScenePalette::default()))
            .unwrap();
        assert!(render(scene.as_mut())
            .iter()
            .all(|color| *color == [200, 40, 40]));
        assert!(scenes
            .create_scene("example.weather.snow", &env(ScenePalette::default()))
            .is_none());
    }

    #[test]
    fn ids_are_validated_unique_and_forgotten_with_their_plugin() {
        let mut scenes = ExternalScenes::default();
        scenes.register(factory("example.weather.rain").0).unwrap();
        scenes.register(factory("example.weather.snow").0).unwrap();
        scenes.register(factory("other.plugin.sky").0).unwrap();
        assert_eq!(
            scenes.register(factory("example.weather.rain").0),
            Err(ExternalSceneError::Duplicate("example.weather.rain".into()))
        );
        for bad in ["", "Example.Rain", "a b", ".lead", "trail."] {
            assert_eq!(
                scenes.register(factory(bad).0),
                Err(ExternalSceneError::InvalidId(bad.into()))
            );
        }
        assert_eq!(
            scenes.entries(),
            vec![
                ("example.weather.rain".to_owned(), "Red".to_owned()),
                ("example.weather.snow".to_owned(), "Red".to_owned()),
                ("other.plugin.sky".to_owned(), "Red".to_owned()),
            ]
        );
        assert_eq!(scenes.unregister_plugin("example.weather"), 2);
        assert!(scenes.unregister("other.plugin.sky"));
        assert!(!scenes.unregister("other.plugin.sky"));
        assert!(scenes.entries().is_empty());
    }

    #[test]
    fn scene_controls_round_trip_through_the_factory() {
        let mut scenes = ExternalScenes::default();
        scenes.register(factory("example.weather.rain").0).unwrap();
        assert!(scenes.controls("example.weather.rain").is_empty());
        assert_eq!(
            scenes.set_control("example.weather.rain", "speed", ControlValue::Number(3)),
            Ok(true)
        );
        assert_eq!(
            scenes.set_control("example.weather.rain", "speed", ControlValue::Number(3)),
            Ok(false)
        );
        assert_eq!(
            scenes.set_control("example.missing", "speed", ControlValue::Number(3)),
            Ok(false)
        );
    }
}
