//! Core plugin `ilium.core.scenes` (manual page core-plugin-scenes.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

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
pub struct ScenesScene;

impl Lifecycle for ScenesScene {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.scenes: scene lifecycle.activate")
    }
}

impl Renderer for ScenesScene {
    fn scene_renderer_new(&mut self, _spec_json: String) -> SceneRendererHandle {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_new")
    }

    fn scene_renderer_readiness(&mut self, _handle: SceneRendererHandle) -> Readiness {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_readiness")
    }

    fn scene_renderer_frames_per_second(&mut self, _handle: SceneRendererHandle) -> u32 {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_frames_per_second")
    }

    fn scene_renderer_wants_pointer(&mut self, _handle: SceneRendererHandle) -> bool {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_wants_pointer")
    }

    fn scene_renderer_wants_occupancy(&mut self, _handle: SceneRendererHandle) -> bool {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_wants_occupancy")
    }

    fn scene_renderer_uses_cell_colors(&mut self, _handle: SceneRendererHandle) -> bool {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_uses_cell_colors")
    }

    fn scene_renderer_render(
        &mut self,
        _handle: SceneRendererHandle,
        _frame: FrameInput,
        _surface: FrameSurfaceHandle,
    ) {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_render")
    }

    fn scene_renderer_set_palette(&mut self, _handle: SceneRendererHandle, _palette_rgb: Vec<u32>) {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_set_palette")
    }

    fn scene_renderer_reconfigure(
        &mut self,
        _handle: SceneRendererHandle,
        _settings_json: String,
    ) -> bool {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_reconfigure")
    }

    fn scene_renderer_status(&mut self, _handle: SceneRendererHandle) -> Option<String> {
        unimplemented!("ilium.core.scenes: scene renderer.scene_renderer_status")
    }
}

/// Native (in-process) renderer of the scene side, used by `FakeScene::new::<ScenesScene>`
/// (reference-test-harness.md "Scene harness", test-plan.md section 1.3).
impl NativeRenderer for ScenesScene {
    fn new(_spec_json: &str) -> Self {
        unimplemented!("ilium.core.scenes: native renderer new")
    }

    fn render(&mut self, _frame: &FrameInput, _surface: &mut NativeSurface) {
        unimplemented!("ilium.core.scenes: native renderer render")
    }

    fn set_palette(&mut self, _palette_rgb: Vec<u32>) {
        unimplemented!("ilium.core.scenes: native renderer set_palette")
    }

    fn frames_per_second(&self) -> u32 {
        unimplemented!("ilium.core.scenes: native renderer frames_per_second")
    }
}
