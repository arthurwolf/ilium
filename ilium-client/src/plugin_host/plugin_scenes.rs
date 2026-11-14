//! Plugin scenes (gap G9b): the scenes a plugin's scene component contributes, offered to the
//! ambient engine as [`ExternalSceneFactory`] entries of the process-wide
//! `ilium_ambient::external::external_scenes` registry, where `AmbientKind::Plugin` finds them.
//!
//! A plugin declares its scenes in `contributes.scenes` of its manifest (with the scene's own
//! settings in `settings_schema`, see `scene_controls`) and ships one
//! `ilium:anim/base` component that exports `ilium:exports/renderer@0.1.0`. At client start
//! every enabled plugin with such a component is compiled once and each of its scenes is
//! registered as `<plugin-id>.<scene-id>`. Building a scene instantiates the component in a
//! store of its own ([`SceneGuest`]); each frame the guest writes grey pixels (and, when it
//! asks for them, cell colours) into the host frame surface, and [`PluginScene`] copies the
//! presented frame into the Braille raster. Colour, dithering and display stay the shared
//! look's: [`ExternalScenes::create_scene`] wraps every external scene in `PaletteScene`.
//!
//! PALETTE (future plugin contract): the renderer receives the current palette in its spec
//! (`palette`) and in every `frame-input`, and later changes through `set-palette`; a plugin
//! scene with natural colours must follow it.
//!
//! A scene may also write the glyph plane (`write-glyphs`): the character it names replaces the
//! Braille dots of its cell, and [`Scene::native_glyph`] hands it to the client host.
//!
//! What scenes cannot use yet: host imports other than the frame surface (a scene's `log`,
//! `http`, `media` ... calls trap with `host.engine-error`) and owner ids.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_ambient::control::{Control, ControlValue};
use ilium_ambient::external::{external_scenes, ExternalSceneFactory, ExternalScenes, SceneValues};
use ilium_ambient::scene::SceneReadiness;
use ilium_ambient::style::ScenePalette;
use ilium_ambient::{Frame, GeoLocation, OccupancyMask, Scene, SceneEnv};
use ilium_plugin_api::generated::anim::surface::{OccupancyGrid, ScenePointer};
use ilium_plugin_api::generated::anim::Readiness;
use ilium_plugin_host::calls::ResolvedLimits;
use ilium_plugin_host::engine::{CompiledComponent, PluginEngine};
use ilium_plugin_host::imports::env::ImportEnv;
use ilium_plugin_host::imports::generated_dispatch::ImportSet;
use ilium_plugin_host::imports::http::HttpImport;
use ilium_plugin_host::runtime::CataloguePlugin;
use ilium_plugin_host::scene_guest::{
    PresentedFrame, SceneFrameInput, SceneGuest, SceneTraits, PIXELS_PER_COLUMN, PIXELS_PER_ROW,
};
use ilium_plugin_host::store_data::GuestIdentity;
use serde_json::{json, Value};

use super::scene_controls::SceneControlSchema;

/// How often (in rendered frames) a scene is asked for its status line.
const STATUS_EVERY_FRAMES: u32 = 30;

/// One scene a plugin contributes, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneDeclaration {
    /// The scene id inside the plugin.
    pub(crate) id: String,
    /// The name shown in the animation list.
    pub(crate) title: String,
    /// The highest frame rate the manifest allows (`frame_rate_cap`), 1..=30.
    pub(crate) frame_rate_cap: u32,
    /// The scene's own settings rows, from `settings_schema`.
    pub(crate) controls: SceneControlSchema,
}

/// The scenes and the scene-side grants a manifest declares. Scenes without an id are skipped.
pub(crate) fn declared_scenes(manifest: &str) -> (Vec<SceneDeclaration>, Vec<String>) {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return (Vec::new(), Vec::new());
    };
    let scenes = manifest
        .pointer("/contributes/scenes")
        .and_then(Value::as_array)
        .map(|scenes| {
            scenes
                .iter()
                .filter_map(|scene| {
                    let id = scene.get("id")?.as_str()?.to_owned();
                    let title = scene
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_owned();
                    let frame_rate_cap = scene
                        .get("frame_rate_cap")
                        .and_then(Value::as_u64)
                        .map_or(30, |cap| cap.clamp(1, 30) as u32);
                    let controls = scene
                        .get("settings_schema")
                        .map(SceneControlSchema::from_schema)
                        .unwrap_or_default();
                    Some(SceneDeclaration {
                        id,
                        title,
                        frame_rate_cap,
                        controls,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let granted = manifest
        .pointer("/capabilities/scene")
        .and_then(Value::as_array)
        .map(|grants| {
            grants
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    (scenes, granted)
}

/// Compiles the scene component of every enabled plugin of `catalogue` (a catalogue of the
/// `scene` side: `component` is the plugin's scene component) and registers its
/// scenes in `scenes`. Returns how many scenes were registered; failures are logged and skip
/// only the plugin or scene concerned.
pub(crate) fn register_catalogue(
    scenes: &mut ExternalScenes,
    engine: &Arc<PluginEngine>,
    catalogue: &[CataloguePlugin],
    disabled: &BTreeSet<String>,
) -> usize {
    let mut registered = 0;
    for plugin in catalogue {
        let Some(component) = plugin.component.as_deref() else {
            continue;
        };
        if disabled.contains(&plugin.id) {
            continue;
        }
        let (declared, granted) = declared_scenes(&plugin.manifest);
        if declared.is_empty() {
            continue;
        }
        let compiled = match engine.compile(component) {
            Ok(compiled) => compiled,
            Err(error) => {
                tracing::warn!(plugin = %plugin.id, "scene component did not compile: {error}");
                continue;
            }
        };
        for declaration in declared {
            let factory = PluginSceneFactory {
                id: format!("{}.{}", plugin.id, declaration.id),
                plugin: plugin.id.clone(),
                declaration,
                engine: Arc::clone(engine),
                compiled: compiled.clone(),
                granted: granted.clone(),
            };
            match scenes.register(Box::new(factory)) {
                Ok(()) => registered += 1,
                Err(error) => {
                    tracing::warn!(plugin = %plugin.id, "plugin scene not registered: {error}")
                }
            }
        }
    }
    registered
}

/// Registers the scenes of `catalogue` in the process-wide registry.
pub(crate) fn register_bundled(
    engine: &Arc<PluginEngine>,
    catalogue: &[CataloguePlugin],
    disabled: &BTreeSet<String>,
) -> usize {
    match external_scenes().write() {
        Ok(mut scenes) => register_catalogue(&mut scenes, engine, catalogue, disabled),
        Err(_) => {
            tracing::error!("plugin scene registry is poisoned; no plugin scenes registered");
            0
        }
    }
}

/// Follows a plugin being enabled or disabled while the client runs: its scenes leave the
/// registry at once, or are compiled and registered again. Returns how many scenes were
/// registered (0 when disabling).
pub(crate) fn set_plugin_enabled(
    engine: &Arc<PluginEngine>,
    catalogue: &[CataloguePlugin],
    plugin: &str,
    enabled: bool,
) -> usize {
    let Ok(mut scenes) = external_scenes().write() else {
        tracing::error!("plugin scene registry is poisoned; plugin scenes not updated");
        return 0;
    };
    // Whatever the plugin had registered goes first, so a repeated enable cannot duplicate.
    scenes.unregister_plugin(plugin);
    if !enabled {
        return 0;
    }
    let own: Vec<CataloguePlugin> = catalogue
        .iter()
        .filter(|candidate| candidate.id == plugin)
        .cloned()
        .collect();
    register_catalogue(&mut scenes, engine, &own, &BTreeSet::new())
}

/// A scene of a plugin's scene component.
pub(crate) struct PluginSceneFactory {
    id: String,
    plugin: String,
    declaration: SceneDeclaration,
    engine: Arc<PluginEngine>,
    compiled: CompiledComponent,
    granted: Vec<String>,
}

impl PluginSceneFactory {
    /// Instantiates the component for one scene. A component that fails to start becomes a
    /// scene that draws nothing and says why in its status line.
    fn build(
        &self,
        palette: &ScenePalette,
        location: &GeoLocation,
        values: &SceneValues,
    ) -> PluginScene {
        let palette_rgb = palette_rgb(palette);
        let spec = json!({
            "scene": self.declaration.id,
            "settings": self.declaration.controls.settings_json(values),
            "palette": palette_rgb,
            "location": {"latitude": location.latitude, "longitude": location.longitude},
            "mode": "live",
        })
        .to_string();
        let identity = GuestIdentity {
            plugin: self.plugin.clone(),
            side: ilium_plugin_api::Side::Scene,
            granted: self.granted.clone(),
        };
        // A scene draws inside a render frame, so its http import never waits for an answer
        // (`take-response` reports a running fetch as not ready) and reaches only the hosts
        // the scene side of the manifest grants.
        let imports = ImportSet {
            http: Some(Box::new(
                HttpImport::new(
                    ImportEnv::unrestricted()
                        .with_identity(&self.plugin, 1)
                        .with_grants(self.granted.clone()),
                )
                .without_waiting(),
            )),
            ..ImportSet::default()
        };
        let guest = SceneGuest::instantiate(
            Arc::clone(&self.engine),
            &self.compiled,
            ResolvedLimits(Default::default()),
            imports,
            identity,
            &spec,
        );
        PluginScene::new(guest, self.declaration.frame_rate_cap, palette_rgb)
    }
}

impl ExternalSceneFactory for PluginSceneFactory {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.declaration.title
    }

    fn controls(&self, values: &SceneValues) -> Vec<Control> {
        self.declaration.controls.controls(values)
    }

    fn set_control(
        &self,
        values: &mut SceneValues,
        id: &str,
        value: ControlValue,
    ) -> Result<bool, String> {
        self.declaration.controls.set_control(values, id, value)
    }

    fn create(&self, env: &SceneEnv, values: &SceneValues) -> Box<dyn Scene> {
        Box::new(self.build(&env.palette, &env.location, values))
    }
}

fn palette_rgb(palette: &ScenePalette) -> Vec<u32> {
    palette
        .stops
        .iter()
        .map(|[red, green, blue]| {
            (u32::from(*red) << 16) | (u32::from(*green) << 8) | u32::from(*blue)
        })
        .collect()
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A running plugin scene: the guest, its last presented frame and why it stopped, if it did.
pub(crate) struct PluginScene {
    guest: Option<SceneGuest>,
    failure: Option<String>,
    traits: SceneTraits,
    frame_rate_cap: u32,
    palette_rgb: Vec<u32>,
    last: Option<PresentedFrame>,
    /// The glyph plane of `last`, by (column, row).
    glyphs: HashMap<(u16, u16), char>,
    status: Option<String>,
    frames_since_status: u32,
    pointer: Option<[f32; 2]>,
    occupancy: Option<OccupancyGrid>,
}

impl PluginScene {
    fn new(
        guest: Result<SceneGuest, ilium_plugin_host::error::HostError>,
        frame_rate_cap: u32,
        palette_rgb: Vec<u32>,
    ) -> Self {
        let (guest, failure) = match guest {
            Ok(guest) => (Some(guest), None),
            Err(error) => (None, Some(format!("Plugin scene did not start: {error}"))),
        };
        let traits = guest.as_ref().map_or(
            SceneTraits {
                frames_per_second: 1,
                wants_pointer: false,
                wants_occupancy: false,
                uses_cell_colors: false,
            },
            SceneGuest::traits,
        );
        Self {
            guest,
            failure,
            traits,
            frame_rate_cap,
            palette_rgb,
            last: None,
            glyphs: HashMap::new(),
            status: None,
            frames_since_status: 0,
            pointer: None,
            occupancy: None,
        }
    }

    /// Stops calling a guest that trapped; the scene keeps its last frame and says why.
    fn fail(&mut self, trap: &ilium_plugin_host::calls::Trap) {
        tracing::warn!("plugin scene stopped: {trap}");
        self.failure = Some(format!("Plugin scene stopped ({})", trap.code.as_str()));
        self.guest = None;
    }

    /// The pointer in cells and sub-cell pixels of a `columns` x `rows` field.
    fn scene_pointer(&self, columns: u16, rows: u16) -> Option<ScenePointer> {
        let [x, y] = self.pointer?;
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || columns == 0 || rows == 0 {
            return None;
        }
        let pixel_x = (x * (f32::from(columns) * PIXELS_PER_COLUMN as f32)) as u32;
        let pixel_y = (y * (f32::from(rows) * PIXELS_PER_ROW as f32)) as u32;
        let pixel_x = pixel_x.min(u32::from(columns) * PIXELS_PER_COLUMN - 1);
        let pixel_y = pixel_y.min(u32::from(rows) * PIXELS_PER_ROW - 1);
        // Bounded by the field above, so the cell numbers fit a u16 and sub-pixels a u8.
        Some(ScenePointer {
            column: (pixel_x / PIXELS_PER_COLUMN) as u16,
            row: (pixel_y / PIXELS_PER_ROW) as u16,
            sub_x: (pixel_x % PIXELS_PER_COLUMN) as u8,
            sub_y: (pixel_y % PIXELS_PER_ROW) as u8,
            pressed: false,
        })
    }

    /// Paints the last presented frame when it fits the field; a frame of another size is
    /// not stretched (the next presented frame replaces it).
    fn paint(&self, frame: &mut Frame<'_>) {
        let Some(last) = &self.last else {
            return;
        };
        if last.width as usize != frame.raster.width
            || last.height as usize != frame.raster.height
            || last.pixels.len() != frame.raster.dots.len()
        {
            return;
        }
        for (dot, pixel) in frame.raster.dots.iter_mut().zip(&last.pixels) {
            *dot = f32::from(*pixel) / 255.0;
        }
        if let Some(colors) = &last.cell_colors {
            frame.cell_colors.clear();
            frame.cell_colors.extend(colors.iter().map(|color| {
                // Each channel is masked to eight bits.
                [
                    ((color >> 16) & 0xff) as u8,
                    ((color >> 8) & 0xff) as u8,
                    (color & 0xff) as u8,
                ]
            }));
        }
    }
}

impl Scene for PluginScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let pointer = self
            .traits
            .wants_pointer
            .then(|| self.scene_pointer(frame.width, frame.height))
            .flatten();
        let occupancy = self
            .traits
            .wants_occupancy
            .then(|| self.occupancy.clone())
            .flatten();
        if let Some(guest) = self.guest.as_mut() {
            if let Ok(mut surface) = guest.surface().lock() {
                surface.set_pointer(pointer);
                surface.set_occupancy(occupancy);
            }
            let input = SceneFrameInput {
                time_millis: millis(frame.time),
                wall_millis: millis(frame.wall),
                now_unix_millis: frame
                    .now
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, millis),
                columns: u32::from(frame.width),
                rows: u32::from(frame.height),
                palette_rgb: self.palette_rgb.clone(),
            };
            match guest.render(&input) {
                Ok(Some(presented)) => {
                    self.glyphs = presented
                        .glyphs
                        .iter()
                        .map(|(column, row, character)| ((*column, *row), *character))
                        .collect();
                    self.last = Some(presented);
                }
                // No present: the previous frame stays.
                Ok(None) => {}
                Err(trap) => self.fail(&trap),
            }
            self.frames_since_status += 1;
            if self.frames_since_status >= STATUS_EVERY_FRAMES {
                self.frames_since_status = 0;
                if let Some(guest) = self.guest.as_mut() {
                    match guest.status() {
                        Ok(status) => self.status = status,
                        Err(trap) => self.fail(&trap),
                    }
                }
            }
        }
        self.paint(frame);
    }

    fn readiness(&mut self) -> SceneReadiness {
        if let Some(failure) = &self.failure {
            return SceneReadiness::Unavailable(failure.clone());
        }
        let Some(guest) = self.guest.as_mut() else {
            return SceneReadiness::Unavailable("Plugin scene is not running".to_owned());
        };
        match guest.readiness() {
            Ok(Readiness::Ready) => SceneReadiness::Ready,
            Ok(Readiness::Preparing) => SceneReadiness::Preparing,
            Ok(Readiness::Failed) => {
                SceneReadiness::Unavailable("The plugin scene could not prepare".to_owned())
            }
            Err(trap) => {
                self.fail(&trap);
                SceneReadiness::Unavailable(self.failure.clone().unwrap_or_default())
            }
        }
    }

    fn pointer(&mut self, position: Option<[f32; 2]>) {
        self.pointer = position;
    }

    fn wants_pointer(&self) -> bool {
        self.traits.wants_pointer
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette_rgb = palette_rgb(palette);
        if let Some(guest) = self.guest.as_mut() {
            if let Err(trap) = guest.set_palette(&self.palette_rgb) {
                self.fail(&trap);
            }
        }
    }

    fn wants_occupancy(&self) -> bool {
        self.traits.wants_occupancy
    }

    fn occupancy(&mut self, mask: &OccupancyMask) {
        let (columns, rows) = (mask.width(), mask.height());
        // Two planes, row by row: the occupied cells, then the cells holding a foreground
        // character anchor (`FieldOccupancy` documents the layout for the guest).
        let positions = || {
            (0..i32::from(rows))
                .flat_map(|row| (0..i32::from(columns)).map(move |column| (column, row)))
        };
        let cells = positions()
            .map(|(column, row)| mask.is_occupied(column, row))
            .chain(positions().map(|(column, row)| mask.is_character(column, row)))
            .collect();
        self.occupancy = Some(OccupancyGrid {
            columns,
            rows,
            cells,
        });
    }

    fn uses_cell_colors(&self) -> bool {
        self.traits.uses_cell_colors
    }

    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        self.glyphs.get(&(x, y)).copied()
    }

    fn frames_per_second(&self) -> u32 {
        self.traits
            .frames_per_second
            .min(self.frame_rate_cap)
            .clamp(1, 30)
    }

    fn status(&self) -> Option<String> {
        self.failure.clone().or_else(|| self.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ambient::Raster;
    use ilium_plugin_api::ProcessKind;
    use ilium_plugin_host::engine::EngineConfig;
    use ilium_plugin_host::testing::wat::scene_component_text;
    use ilium_plugin_host::testing::wat_component;
    use std::borrow::Cow;

    const MANIFEST: &str = r#"{
        "id": "test.scenes",
        "capabilities": { "scene": [] },
        "contributes": { "scenes": [
            { "id": "grey", "title": "Grey field", "frame_rate_cap": 5 },
            { "id": "second", "frame_rate_cap": 400 },
            { "title": "no id" }
        ] }
    }"#;

    fn engine() -> Arc<PluginEngine> {
        Arc::new(
            PluginEngine::new(EngineConfig {
                epoch_tick_ms: 10,
                cache_dir: None,
                process: ProcessKind::Client,
            })
            .expect("real engine"),
        )
    }

    fn catalogue(ink: u8) -> Vec<CataloguePlugin> {
        vec![CataloguePlugin {
            id: "test.scenes".to_owned(),
            manifest: MANIFEST.to_owned(),
            component: Some(Cow::Owned(wat_component(&scene_component_text(ink)))),
        }]
    }

    fn draw(scene: &mut dyn Scene, columns: u16, rows: u16) -> (Raster, Vec<[u8; 3]>) {
        let mut raster = Raster::default();
        raster.resize(usize::from(columns) * 2, usize::from(rows) * 4);
        let mut colors = vec![[0; 3]; usize::from(columns) * usize::from(rows)];
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: columns,
            height: rows,
            time: Duration::from_millis(40),
            wall: Duration::from_millis(40),
            now: SystemTime::now(),
        });
        (raster, colors)
    }

    #[test]
    fn manifests_declare_scenes_with_capped_frame_rates() {
        let (scenes, granted) = declared_scenes(MANIFEST);
        assert_eq!(
            scenes,
            vec![
                SceneDeclaration {
                    id: "grey".to_owned(),
                    title: "Grey field".to_owned(),
                    frame_rate_cap: 5,
                    controls: SceneControlSchema::default(),
                },
                SceneDeclaration {
                    id: "second".to_owned(),
                    title: "second".to_owned(),
                    frame_rate_cap: 30,
                    controls: SceneControlSchema::default(),
                },
            ]
        );
        assert!(granted.is_empty());
        assert_eq!(declared_scenes("not json"), (Vec::new(), Vec::new()));
    }

    #[test]
    fn every_declared_scene_of_an_enabled_plugin_is_registered() {
        let mut scenes = ExternalScenes::default();
        let engine = engine();
        let count = register_catalogue(&mut scenes, &engine, &catalogue(1), &BTreeSet::new());
        assert_eq!(count, 2);
        assert_eq!(
            scenes.entries(),
            vec![
                ("test.scenes.grey".to_owned(), "Grey field".to_owned()),
                ("test.scenes.second".to_owned(), "second".to_owned()),
            ]
        );
        assert_eq!(scenes.unregister_plugin("test.scenes"), 2);
        let disabled = BTreeSet::from(["test.scenes".to_owned()]);
        assert_eq!(
            register_catalogue(&mut scenes, &engine, &catalogue(1), &disabled),
            0
        );
    }

    #[test]
    fn a_plugin_scene_paints_the_guests_pixels_into_the_braille_raster() {
        let engine = engine();
        let compiled = engine
            .compile(&wat_component(&scene_component_text(255)))
            .expect("component");
        let factory = PluginSceneFactory {
            id: "test.scenes.grey".to_owned(),
            plugin: "test.scenes".to_owned(),
            declaration: SceneDeclaration {
                id: "grey".to_owned(),
                title: "Grey field".to_owned(),
                frame_rate_cap: 5,
                controls: SceneControlSchema::default(),
            },
            engine,
            compiled,
            granted: Vec::new(),
        };
        let mut scene = factory.build(
            &ScenePalette::default(),
            &GeoLocation::default(),
            &SceneValues::new(),
        );
        assert_eq!(scene.status(), None);
        assert_eq!(scene.readiness(), SceneReadiness::Ready);
        // The guest asks for 7 frames per second, the manifest allows 5.
        assert_eq!(scene.frames_per_second(), 5);
        assert!(!scene.uses_cell_colors());
        let (raster, _) = draw(&mut scene, 3, 2);
        assert_eq!((raster.width, raster.height), (6, 8));
        assert!(raster
            .dots
            .iter()
            .all(|dot| (*dot - 1.0).abs() < f32::EPSILON));
    }

    #[test]
    fn a_component_that_does_not_start_is_an_unavailable_scene_not_a_crash() {
        let engine = engine();
        let compiled = engine
            .compile(&wat_component("(component)"))
            .expect("component");
        let factory = PluginSceneFactory {
            id: "test.scenes.none".to_owned(),
            plugin: "test.scenes".to_owned(),
            declaration: SceneDeclaration {
                id: "none".to_owned(),
                title: "None".to_owned(),
                frame_rate_cap: 30,
                controls: SceneControlSchema::default(),
            },
            engine,
            compiled,
            granted: Vec::new(),
        };
        let mut scene = factory.build(
            &ScenePalette::default(),
            &GeoLocation::default(),
            &SceneValues::new(),
        );
        assert!(scene.status().is_some());
        assert!(matches!(scene.readiness(), SceneReadiness::Unavailable(_)));
        let (raster, _) = draw(&mut scene, 2, 1);
        assert!(raster.dots.iter().all(|dot| *dot == 0.0));
    }

    #[test]
    fn the_pointer_maps_to_cells_and_sub_cell_pixels() {
        let mut scene = PluginScene::new(
            Err(ilium_plugin_host::error::HostError::EngineError(
                "none".into(),
            )),
            30,
            Vec::new(),
        );
        scene.pointer(Some([0.5, 0.99]));
        let pointer = scene.scene_pointer(10, 5).expect("inside the field");
        assert_eq!((pointer.column, pointer.sub_x), (5, 0));
        assert_eq!((pointer.row, pointer.sub_y), (4, 3));
        scene.pointer(Some([1.5, 0.0]));
        assert_eq!(scene.scene_pointer(10, 5), None);
    }

    #[test]
    fn palettes_are_sent_as_rgb_words() {
        let palette = ScenePalette {
            stops: vec![[0x12, 0x34, 0x56], [255, 0, 1]],
            ..ScenePalette::default()
        };
        assert_eq!(palette_rgb(&palette), vec![0x0012_3456, 0x00ff_0001]);
    }
}
