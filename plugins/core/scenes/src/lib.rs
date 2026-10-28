//! Core plugin `ilium.core.scenes` (manual page core-plugin-scenes.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! a minimal real implementation.

use ilium_plugin_sdk::generated::anim::renderer_types::FrameInput;
use ilium_plugin_sdk::generated::anim::renderer_types::Readiness;
use ilium_plugin_sdk::generated::anim::surface::FrameSurfaceHandle;
use ilium_plugin_sdk::generated::exports::renderer::SceneRendererHandle;
use ilium_plugin_sdk::plugin::NativeSurface;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Lifecycle, NativeRenderer, Renderer};

/// The scene side of `ilium.core.scenes` (core-plugin-scenes.md, world `ilium:anim/base@0.1.0`).
#[ilium_plugin(side = "scene", exports = [])]
#[derive(Default)]
pub struct ScenesScene {
    /// Shared palette as RGB values; empty means the scene keeps its own greys.
    palette_rgb: Vec<u32>,
    /// Next renderer handle number handed out by `scene_renderer_new`.
    next_handle: u32,
}

/// Frame rate every scene of this package asks for.
const SCENE_FRAMES_PER_SECOND: u32 = 30;

impl ScenesScene {
    /// Colour for cell `index` of `count`: the palette entry matching its position, or white.
    fn tone(&self, index: usize, count: usize) -> u32 {
        if self.palette_rgb.is_empty() || count == 0 {
            return 0x00ff_ffff;
        }
        self.palette_rgb[index * self.palette_rgb.len() / count]
    }
}

impl Lifecycle for ScenesScene {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Renderer for ScenesScene {
    fn scene_renderer_new(&mut self, _spec_json: String) -> SceneRendererHandle {
        self.next_handle += 1;
        SceneRendererHandle(self.next_handle)
    }

    fn scene_renderer_readiness(&mut self, _handle: SceneRendererHandle) -> Readiness {
        Readiness::Ready
    }

    fn scene_renderer_frames_per_second(&mut self, _handle: SceneRendererHandle) -> u32 {
        SCENE_FRAMES_PER_SECOND
    }

    fn scene_renderer_wants_pointer(&mut self, _handle: SceneRendererHandle) -> bool {
        false
    }

    fn scene_renderer_wants_occupancy(&mut self, _handle: SceneRendererHandle) -> bool {
        false
    }

    fn scene_renderer_uses_cell_colors(&mut self, _handle: SceneRendererHandle) -> bool {
        true
    }

    fn scene_renderer_render(
        &mut self,
        _handle: SceneRendererHandle,
        _frame: FrameInput,
        _surface: FrameSurfaceHandle,
    ) {
        // Pixel drawing goes through the host surface handle, which has no SDK call yet.
    }

    fn scene_renderer_set_palette(&mut self, _handle: SceneRendererHandle, palette_rgb: Vec<u32>) {
        self.palette_rgb = palette_rgb;
    }

    fn scene_renderer_reconfigure(
        &mut self,
        _handle: SceneRendererHandle,
        _settings_json: String,
    ) -> bool {
        true
    }

    fn scene_renderer_status(&mut self, _handle: SceneRendererHandle) -> Option<String> {
        None
    }
}

/// Native (in-process) renderer of the scene side, used by `FakeScene::new::<ScenesScene>`
/// (reference-test-harness.md "Scene harness", test-plan.md section 1.3).
impl NativeRenderer for ScenesScene {
    fn new(_spec_json: &str) -> Self {
        Self {
            palette_rgb: Vec::new(),
            next_handle: 0,
        }
    }

    fn render(&mut self, _frame: &FrameInput, surface: &mut NativeSurface) {
        let count = surface.cells.len();
        for index in 0..count {
            let colour = self.tone(index, count);
            surface.cells[index].1 = colour;
        }
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.palette_rgb = palette_rgb;
    }

    fn frames_per_second(&self) -> u32 {
        SCENE_FRAMES_PER_SECOND
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
    fn cor_010_catalogue_names_the_three_scenes_in_order() {
        let manifest = manifest_value();
        let ids: Vec<String> = scene_entries(&manifest)
            .iter()
            .map(|scene| scene["id"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(ids, vec!["waves", "clock", "stars-overhead"]);
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
    fn cor_011_every_scene_settings_schema_has_no_properties() {
        let manifest = manifest_value();
        for scene in scene_entries(&manifest) {
            let properties = scene["settings_schema"]["properties"]
                .as_object()
                .expect("settings_schema.properties is an object");
            assert!(
                properties.is_empty(),
                "scene {} adds scene controls of its own",
                scene["id"]
            );
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
