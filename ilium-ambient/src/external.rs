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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex, RwLock};

use crate::control::{Control, ControlValue};
use crate::scene::{Frame, PaletteScene, Scene, SceneEnv};

/// The edited control values of one external scene, by control id. They live in the project
/// settings (`AmbientSettings::plugin_scene`), never in the factory, so two projects can
/// show the same scene with different settings.
pub type SceneValues = BTreeMap<String, ControlValue>;

static EXTERNAL_SCENES: LazyLock<RwLock<ExternalScenes>> =
    LazyLock::new(|| RwLock::new(ExternalScenes::default()));

/// The external scenes of this process, registered by the plugin host and read by the
/// ambient engine (`AmbientKind::Plugin`).
pub fn external_scenes() -> &'static RwLock<ExternalScenes> {
    &EXTERNAL_SCENES
}

static INTERNED: LazyLock<Mutex<BTreeSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));

/// A `'static` copy of `text`, shared by equal texts. Settings rows hold `&'static str`;
/// plugin scene controls come from manifests, so their texts are interned once and live
/// as long as the process (the set is bounded by the installed scene manifests).
pub fn intern(text: &str) -> &'static str {
    let Ok(mut set) = INTERNED.lock() else {
        return Box::leak(text.to_owned().into_boxed_str());
    };
    if let Some(existing) = set.get(text) {
        return existing;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// Longest id of an external scene, in bytes.
pub const MAXIMUM_EXTERNAL_SCENE_ID_BYTES: usize = 256;

/// Builds the scenes of one external source, for example one plugin scene contribution.
pub trait ExternalSceneFactory: Send + Sync {
    /// The global id of the scene, `<plugin-id>.<scene-id>`; unique among external scenes.
    fn id(&self) -> &str;
    /// The name shown in the background animation list.
    fn title(&self) -> &str;
    /// The scene's own settings rows showing `values` (defaults for ids not in it), never
    /// colour, dither or display rows (those are the shared look's).
    fn controls(&self, _values: &SceneValues) -> Vec<Control> {
        Vec::new()
    }
    /// Applies one edit of a scene control to `values`: `Ok(true)` when something changed,
    /// `Ok(false)` for an unknown id or an unchanged value, `Err` for invalid input to show
    /// the user.
    fn set_control(
        &self,
        _values: &mut SceneValues,
        _id: &str,
        _value: ControlValue,
    ) -> Result<bool, String> {
        Ok(false)
    }
    /// Whether the scene shows the shared observer location, so the settings list offers the
    /// Location row for it.
    fn uses_location(&self) -> bool {
        false
    }
    /// Builds a fresh scene showing `values`. `env.palette` holds the shared palette to follow.
    fn create(&self, env: &SceneEnv, values: &SceneValues) -> Box<dyn Scene>;
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

    /// Whether a scene with this id is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.factories.contains_key(id)
    }

    /// The id the settings select: `wanted` when registered, else the first registered id
    /// (a saved scene whose plugin is disabled falls back instead of showing nothing).
    pub fn resolve(&self, wanted: &str) -> Option<String> {
        if self.factories.contains_key(wanted) {
            return Some(wanted.to_owned());
        }
        self.factories.keys().next().cloned()
    }

    /// Whether the external scene `id` shows the shared observer location.
    pub fn uses_location(&self, id: &str) -> bool {
        self.factories
            .get(id)
            .is_some_and(|factory| factory.uses_location())
    }

    /// The scene controls of one external scene showing `values`; empty for an unknown id.
    pub fn controls(&self, id: &str, values: &SceneValues) -> Vec<Control> {
        self.factories
            .get(id)
            .map(|factory| factory.controls(values))
            .unwrap_or_default()
    }

    /// Applies one control edit to `values` of an external scene; `Ok(false)` for an unknown id.
    pub fn set_control(
        &self,
        id: &str,
        values: &mut SceneValues,
        control: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        match self.factories.get(id) {
            Some(factory) => factory.set_control(values, control, value),
            None => Ok(false),
        }
    }

    /// Builds the external scene `id`, wrapped in `PaletteScene` like every built-in scene so
    /// the shared look applies to it. `None` for an unknown id.
    pub fn create_scene(
        &self,
        id: &str,
        env: &SceneEnv,
        values: &SceneValues,
    ) -> Option<Box<dyn Scene>> {
        let factory = self.factories.get(id)?;
        Some(Box::new(PaletteScene::new(
            factory.create(env, values),
            env.palette.clone(),
        )))
    }
}

/// What `AmbientKind::Plugin` shows while no plugin scene is registered (every scene plugin
/// is disabled or failed to load): nothing is drawn and the status line says why.
pub struct NoPluginScene;

impl Scene for NoPluginScene {
    fn render(&mut self, _frame: &mut Frame<'_>) {}

    fn status(&self) -> Option<String> {
        Some("No plugin scene is available. Enable a scene plugin in Settings > Plugins.".into())
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
    }

    impl ExternalSceneFactory for RedFactory {
        fn id(&self) -> &str {
            &self.id
        }
        fn title(&self) -> &str {
            "Red"
        }
        fn set_control(
            &self,
            values: &mut SceneValues,
            id: &str,
            value: ControlValue,
        ) -> Result<bool, String> {
            match (id, value) {
                ("speed", ControlValue::Number(speed)) => {
                    let changed = values.get("speed") != Some(&ControlValue::Number(speed));
                    values.insert("speed".into(), ControlValue::Number(speed));
                    Ok(changed)
                }
                _ => Ok(false),
            }
        }
        fn create(&self, env: &SceneEnv, _values: &SceneValues) -> Box<dyn Scene> {
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
            .create_scene(
                "example.weather.rain",
                &env(palette.clone()),
                &SceneValues::new(),
            )
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
            .create_scene(
                "example.weather.rain",
                &env(ScenePalette::default()),
                &SceneValues::new(),
            )
            .unwrap();
        assert!(render(scene.as_mut())
            .iter()
            .all(|color| *color == [200, 40, 40]));
        assert!(scenes
            .create_scene(
                "example.weather.snow",
                &env(ScenePalette::default()),
                &SceneValues::new(),
            )
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
        let mut values = SceneValues::new();
        assert!(scenes.controls("example.weather.rain", &values).is_empty());
        assert_eq!(
            scenes.set_control(
                "example.weather.rain",
                &mut values,
                "speed",
                ControlValue::Number(3)
            ),
            Ok(true)
        );
        assert_eq!(
            scenes.set_control(
                "example.weather.rain",
                &mut values,
                "speed",
                ControlValue::Number(3)
            ),
            Ok(false)
        );
        assert_eq!(
            scenes.set_control(
                "example.missing",
                &mut values,
                "speed",
                ControlValue::Number(3)
            ),
            Ok(false)
        );
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn resolve_falls_back_to_the_first_registered_scene() {
        let mut scenes = ExternalScenes::default();
        assert_eq!(scenes.resolve("example.weather.rain"), None);
        scenes.register(factory("example.weather.snow").0).unwrap();
        scenes.register(factory("example.weather.rain").0).unwrap();
        assert_eq!(
            scenes.resolve("example.weather.snow").as_deref(),
            Some("example.weather.snow")
        );
        assert_eq!(
            scenes.resolve("gone.plugin.scene").as_deref(),
            Some("example.weather.rain")
        );
    }

    #[test]
    fn interned_text_is_shared() {
        let first = intern("example interned text");
        let second = intern("example interned text");
        assert!(std::ptr::eq(first, second));
    }
}
