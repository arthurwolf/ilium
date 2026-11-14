//! Core plugin `ilium.core.scenes` (manual page core-plugin-scenes.md).
//!
//! One scene component serves every scene the manifest contributes. The host builds one
//! `scene-renderer` per scene, handing the scene id in the spec (`scene`), and the scene draws
//! tone into the Braille field of the frame surface; colour, palette, dithering and display
//! stay with the shared look of the host.
//!
//! `dithered-waves` is ported from the former built-in scene and draws for real (module
//! `dithered_waves`). The other ids of the catalogue (`waves`, `clock`, `stars-overhead`) are
//! still the tests-first skeleton: they follow the palette and draw nothing yet.

pub mod atlantic_dusk;
pub mod box_machine;
pub mod cube_clock;
pub mod dither_water;
pub mod dithered_waves;
pub mod dithr_patterns;
pub mod fbm_clouds;
pub mod galactic_empires;
pub mod hex_expedition;
pub mod kit;
pub mod machine_screen;
pub mod pi_digits;
pub mod pipes;
pub mod quiet;
pub mod registry;
pub mod scenes2;
pub mod solar_system;

// The kit modules at the crate root, so the scenes moved here keep their `crate::raster::...`
// style paths.
pub use kit::{control, debug, dither, raster, scene, style};

use dithered_waves::{DitheredWaves, DitheredWavesSettings};
use kit::raster::Raster;
use kit::scene::{Frame, Scene, SceneEnv, SceneLocation};
use kit::style::ScenePalette;
use ilium_plugin_sdk::generated::anim::renderer_types::FrameInput;
use ilium_plugin_sdk::plugin::NativeSurface;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Lifecycle, NativeRenderer, SceneField, SceneProgram};
use serde_json::Value;
use std::time::{Duration, UNIX_EPOCH};

/// Id of the ported scene of this package.
pub const DITHERED_WAVES_ID: &str = "dithered-waves";

/// The scene side of `ilium.core.scenes` (core-plugin-scenes.md, world `ilium:anim/base@0.1.0`).
/// One value is one scene: the component glue builds it from the spec of a `scene-renderer`.
#[ilium_plugin(side = "scene", exports = [])]
pub struct ScenesScene {
    /// Id of the scene this value draws (`scene` of the spec); empty before a spec arrives.
    scene_id: String,
    /// Shared palette as RGB values; empty means the scene keeps its own greys.
    palette_rgb: Vec<u32>,
    /// What draws: the ported scene, or nothing for the skeleton ids.
    drawing: Drawing,
    /// The spec the scene was built from, kept to rebuild a kit scene when its settings change.
    spec: Value,
    /// Scratch raster of a kit scene, reused between frames.
    raster: Raster,
    /// Scratch cell colours of a kit scene, reused between frames.
    cell_colors: Vec<[u8; 3]>,
}

/// The drawing behind a scene id.
enum Drawing {
    /// Skeleton scenes: the frame is left blank.
    Blank,
    /// The `dithered-waves` scene.
    DitheredWaves(Box<DitheredWaves>),
    /// A scene of the shared scene kit (`registry::create`).
    Kit(Box<dyn Scene>),
}

impl Default for ScenesScene {
    fn default() -> Self {
        Self {
            scene_id: String::new(),
            palette_rgb: Vec::new(),
            drawing: Drawing::Blank,
            spec: Value::Null,
            raster: Raster::default(),
            cell_colors: Vec::new(),
        }
    }
}

/// Frame rate of a scene without a cadence of its own.
const SCENE_FRAMES_PER_SECOND: u32 = 30;

/// The cadence each scene of the catalogue asks for; never above its manifest cap.
fn frames_per_second_of(scene_id: &str) -> u32 {
    match scene_id {
        "waves" => 24,
        "clock" => 2,
        "stars-overhead" => 10,
        DITHERED_WAVES_ID => dithered_waves::FRAMES_PER_SECOND,
        _ => SCENE_FRAMES_PER_SECOND,
    }
}

impl ScenesScene {
    /// The scene a spec describes: `scene` names it, `settings` (or the spec itself) holds
    /// its settings and `palette` the shared palette as RGB values.
    fn from_spec(spec_json: &str) -> Self {
        let spec: Value = serde_json::from_str(spec_json).unwrap_or(Value::Null);
        let scene_id = spec
            .get("scene")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let palette_rgb = spec
            .get("palette")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_u64().map(|rgb| (rgb & 0x00ff_ffff) as u32))
                    .collect()
            })
            .unwrap_or_default();
        let drawing = Self::build_drawing(&scene_id, &spec, &palette_rgb);
        Self {
            scene_id,
            palette_rgb,
            drawing,
            spec,
            raster: Raster::default(),
            cell_colors: Vec::new(),
        }
    }

    /// The drawing of scene `scene_id` for `spec`.
    fn build_drawing(scene_id: &str, spec: &Value, palette_rgb: &[u32]) -> Drawing {
        if scene_id == DITHERED_WAVES_ID {
            return Drawing::DitheredWaves(Box::new(DitheredWaves::from_spec(spec)));
        }
        let env = SceneEnv::new(ScenePalette::from_rgb_words(palette_rgb))
            .with_location(SceneLocation::from_spec(spec));
        match registry::create(scene_id, spec, &env) {
            Some(scene) => Drawing::Kit(scene),
            None => Drawing::Blank,
        }
    }

    /// The frame rate the drawing asks for.
    fn rate(&self) -> u32 {
        match &self.drawing {
            Drawing::Kit(scene) => scene.frames_per_second().clamp(1, 30),
            _ => frames_per_second_of(&self.scene_id),
        }
    }

    /// Colour for cell `index` of `count`: the palette entry matching its position, or white.
    fn tone(&self, index: usize, count: usize) -> u32 {
        if self.palette_rgb.is_empty() || count == 0 {
            return 0x00ff_ffff;
        }
        self.palette_rgb[index * self.palette_rgb.len() / count]
    }

    /// Draws one frame into the pixel field. `frame.time_millis` is the scene clock.
    fn draw(&mut self, frame: &FrameInput, field: &mut SceneField) {
        let width = field.width();
        let height = field.height();
        match &mut self.drawing {
            Drawing::DitheredWaves(waves) => {
                waves.render(width, height, frame.time_millis, &mut field.pixels);
            }
            Drawing::Kit(scene) => {
                Self::draw_kit(
                    scene.as_mut(),
                    &mut self.raster,
                    &mut self.cell_colors,
                    frame,
                    field,
                );
            }
            Drawing::Blank => {}
        }
    }

    /// Runs a kit scene for one frame: the scene draws into a dot raster (and, when it has
    /// natural colours, per-cell colours), which becomes the field's grey pixels and colours.
    fn draw_kit(
        scene: &mut dyn Scene,
        raster: &mut Raster,
        cell_colors: &mut Vec<[u8; 3]>,
        frame: &FrameInput,
        field: &mut SceneField,
    ) {
        let (columns, rows) = (field.columns, field.rows);
        raster.resize(field.width(), field.height());
        let cell_count = usize::from(columns) * usize::from(rows);
        cell_colors.clear();
        cell_colors.resize(cell_count, [255, 255, 255]);
        let time = Duration::from_millis(frame.time_millis);
        {
            let mut kit_frame = Frame {
                raster,
                cell_colors,
                width: columns,
                height: rows,
                time,
                wall: Duration::from_millis(frame.wall_millis),
                now: UNIX_EPOCH + Duration::from_millis(frame.now_unix_millis),
            };
            scene.render(&mut kit_frame);
        }
        for (pixel, dot) in field.pixels.iter_mut().zip(&raster.dots) {
            *pixel = (dot.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        if scene.uses_cell_colors() {
            for (colour, rgb) in field.cell_colors.iter_mut().zip(cell_colors.iter()) {
                *colour = (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
            }
        }
        // The glyph plane: a scene that writes terminal text replaces the Braille dots of
        // those cells (`Scene::native_glyph`).
        for row in 0..rows {
            for column in 0..columns {
                if let Some(character) = scene.native_glyph(column, row) {
                    field.put_glyph(column, row, character);
                }
            }
        }
    }

    /// Applies new settings to the drawing; `false` for text that is not a JSON object.
    fn apply_settings(&mut self, settings_json: &str) -> bool {
        let Ok(settings) = serde_json::from_str::<Value>(settings_json) else {
            return false;
        };
        if !settings.is_object() {
            return false;
        }
        if matches!(self.drawing, Drawing::DitheredWaves(_)) {
            let settings = DitheredWavesSettings::from_spec(&settings);
            self.drawing = Drawing::DitheredWaves(Box::new(DitheredWaves::new(&settings)));
        } else if let Drawing::Kit(scene) = &mut self.drawing {
            if !scene.reconfigure(&settings) {
                if let Some(object) = self.spec.as_object_mut() {
                    object.insert("settings".to_owned(), settings);
                }
                self.drawing = Self::build_drawing(&self.scene_id, &self.spec, &self.palette_rgb);
            }
        }
        true
    }
}

impl Lifecycle for ScenesScene {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

/// The scene contract of the component glue (one value per `scene-renderer`).
///
/// PALETTE (future plugin contract): the palette arrives in the spec, in every frame input
/// and through `set_palette`; a scene with natural colours follows it. The scenes of this
/// package draw tone only, so the shared look colours them.
impl SceneProgram for ScenesScene {
    fn new(spec_json: &str) -> Self {
        Self::from_spec(spec_json)
    }

    fn render(&mut self, frame: &FrameInput, field: &mut SceneField) {
        self.draw(frame, field);
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        if let Drawing::Kit(scene) = &mut self.drawing {
            scene.set_palette(&ScenePalette::from_rgb_words(&palette_rgb));
        }
        self.palette_rgb = palette_rgb;
    }

    fn frames_per_second(&self) -> u32 {
        self.rate()
    }

    fn uses_cell_colors(&self) -> bool {
        matches!(&self.drawing, Drawing::Kit(scene) if scene.uses_cell_colors())
    }

    fn status(&self) -> Option<String> {
        match &self.drawing {
            Drawing::Kit(scene) => scene.status(),
            _ => None,
        }
    }

    fn reconfigure(&mut self, settings_json: &str) -> bool {
        self.apply_settings(settings_json)
    }
}

/// Native (in-process) renderer of the scene side, used by `FakeScene::new::<ScenesScene>`
/// (reference-test-harness.md "Scene harness", test-plan.md section 1.3). A drawing scene
/// fills the Braille field and the harness shows it as Braille cells; a skeleton scene keeps
/// the cells and follows the palette.
impl NativeRenderer for ScenesScene {
    fn new(spec_json: &str) -> Self {
        Self::from_spec(spec_json)
    }

    fn render(&mut self, frame: &FrameInput, surface: &mut NativeSurface) {
        if matches!(self.drawing, Drawing::Blank) {
            let count = surface.cells.len();
            for index in 0..count {
                let colour = self.tone(index, count);
                surface.cells[index].1 = colour;
            }
            return;
        }
        let mut field = SceneField::new(surface.columns, surface.rows);
        self.draw(frame, &mut field);
        surface.cells = field.to_native_cells();
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        if let Drawing::Kit(scene) = &mut self.drawing {
            scene.set_palette(&ScenePalette::from_rgb_words(&palette_rgb));
        }
        self.palette_rgb = palette_rgb;
    }

    fn frames_per_second(&self) -> u32 {
        self.rate()
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::json;

    /// Words that name a shared-look control (colour, palette, brightness, contrast, hue, dithering).
    /// COR-011 forbids a scene package from adding any of these to its own settings.
    const SHARED_LOOK_WORDS: [&str; 5] = ["brightness", "contrast", "hue", "palette", "dither"];

    /// The shipped manifest, read at compile time so the tests check the file that gets installed.
    fn manifest_value() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn scene_entries(manifest: &serde_json::Value) -> Vec<serde_json::Value> {
        manifest["contributes"]["scenes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn settings_property_names(scene: &serde_json::Value) -> Vec<String> {
        scene["settings_schema"]["properties"]
            .as_object()
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn is_shared_look_control(property_name: &str) -> bool {
        let lowered = property_name.to_ascii_lowercase();
        SHARED_LOOK_WORDS.iter().any(|word| lowered.contains(word))
    }

    /// REQ: COR-010
    #[test]
    fn cor_010_catalogue_names_the_scenes_in_order() {
        let manifest = manifest_value();
        let ids: Vec<String> = scene_entries(&manifest)
            .iter()
            .map(|scene| scene["id"].as_str().unwrap_or_default().to_string())
            .collect();
        // The skeleton scenes keep their place; every ported scene follows them.
        assert_eq!(&ids[..3], ["waves", "clock", "stars-overhead"]);
        assert!(ids.iter().any(|id| id == "dithered-waves"));
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "scene ids are unique");
    }

    /// REQ: COR-010
    #[test]
    fn cor_010_every_scene_has_a_title_and_a_family() {
        let manifest = manifest_value();
        for scene in scene_entries(&manifest) {
            let title = scene["title"].as_str().unwrap_or_default();
            let family = scene["family"].as_str().unwrap_or_default();
            assert!(!title.is_empty(), "scene {} has no title", scene["id"]);
            assert!(!family.is_empty(), "scene {} has no family", scene["id"]);
        }
    }

    /// REQ: COR-010
    #[test]
    fn cor_010_scene_package_has_a_scene_side_only() {
        let manifest = manifest_value();
        assert_eq!(manifest["scene"]["entry"], "scene.wasm");
        assert_eq!(manifest["scene"]["world"], "ilium:anim/base@0.1.0");
        assert!(manifest.get("server").is_none(), "no server side expected");
        assert!(manifest.get("client").is_none(), "no client side expected");
    }

    /// REQ: COR-010
    #[test]
    fn cor_010_live_data_scene_declares_its_host_and_observer_grants() {
        let manifest = manifest_value();
        let stars = scene_entries(&manifest)
            .into_iter()
            .find(|scene| scene["id"] == "stars-overhead")
            .expect("stars-overhead is in the catalogue");
        assert_eq!(stars["modes"], json!(["live"]));
        let grants = manifest["capabilities"]["scene"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(grants.contains(&json!("http:services.swpc.noaa.gov")));
        assert!(grants.contains(&json!("location.observer")));
    }

    /// REQ: COR-010
    #[test]
    fn cor_010_scene_grants_cover_media_gpu_observer_and_live_host() {
        let manifest = manifest_value();
        let grants = manifest["capabilities"]["scene"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            grants,
            vec![
                json!("media.video"),
                json!("location.observer"),
                json!("gpu.kernels"),
                json!("http:services.swpc.noaa.gov"),
            ]
        );
    }

    /// REQ: COR-011
    #[test]
    fn cor_011_only_the_ported_scene_declares_scene_controls() {
        let manifest = manifest_value();
        for scene in scene_entries(&manifest) {
            let properties = scene["settings_schema"]["properties"]
                .as_object()
                .expect("settings_schema.properties is an object");
            let is_skeleton = matches!(
                scene["id"].as_str(),
                Some("waves" | "clock" | "stars-overhead")
            );
            if !is_skeleton {
                assert!(
                    !properties.is_empty(),
                    "scene {} has its own controls",
                    scene["id"]
                );
            } else {
                assert!(
                    properties.is_empty(),
                    "scene {} adds scene controls of its own",
                    scene["id"]
                );
            }
        }
    }

    /// REQ: COR-011
    #[test]
    fn cor_011_manifest_carries_no_shared_look_control_names() {
        let manifest = manifest_value();
        for scene in scene_entries(&manifest) {
            for name in settings_property_names(&scene) {
                assert!(
                    !is_shared_look_control(&name),
                    "scene {} declares shared-look control {name}",
                    scene["id"]
                );
            }
        }
        let text = include_str!("../manifest.json").to_ascii_lowercase();
        assert!(!text.contains("\"palette\""));
        assert!(!text.contains("\"brightness\""));
    }

    /// REQ: COR-011
    #[test]
    fn cor_011_failure_case_brightness_control_is_flagged() {
        let scene = json!({
            "id": "custom",
            "settings_schema": {
                "type": "object",
                "properties": { "brightness": { "type": "number" } }
            }
        });
        let names = settings_property_names(&scene);
        assert_eq!(names, vec!["brightness".to_string()]);
        assert!(is_shared_look_control(&names[0]));
    }
}
