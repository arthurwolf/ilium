//! FakeScene: drives one scene renderer (reference-test-harness.md "Scenes").

use ilium_plugin_sdk_core::anim::FrameInput;
use ilium_plugin_sdk_core::SceneRenderer;

use crate::host::Screen;

/// A scene of a given size on the fake clock.
#[derive(Debug)]
pub struct FakeScene {
    _private: (),
}

impl FakeScene {
    /// A scene of `columns` by `rows` (DEV-560).
    pub fn new<R: SceneRenderer>(_scene_id: &str, _columns: u16, _rows: u16) -> FakeScene {
        unimplemented!("DEV-560")
    }

    /// Calls `render` once and reads the surface as a `Screen` (DEV-560, DEV-561).
    pub fn frame(&mut self, _input: FrameInput) -> Screen {
        unimplemented!("DEV-560")
    }

    /// Passes the palette to the renderer before the next frame (DEV-562).
    pub fn set_palette(&mut self, _palette_rgb: Vec<u32>) {
        unimplemented!("DEV-562")
    }

    /// Passes a settings object (JSON text) to the renderer before `new` runs (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn with_settings(self, _settings_json: &str) -> FakeScene {
        unimplemented!("DEV-560")
    }

    /// Answers `location.current` with this position (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn with_location(self, _latitude: f64, _longitude: f64) -> FakeScene {
        unimplemented!("DEV-560")
    }

    /// Calls the renderer's `reconfigure` with a settings object; `true` when it accepted it in place (lane hook, see manual_gaps_fakes.md).
    pub fn reconfigure(&mut self, _settings_json: &str) -> bool {
        unimplemented!("DEV-560")
    }

    /// The frame rate the renderer asks for (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn frames_per_second(&self) -> u32 {
        unimplemented!("DEV-560")
    }

    /// Whether the renderer asks for the pointer position (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn wants_pointer(&self) -> bool {
        unimplemented!("DEV-560")
    }

    /// Whether the renderer asks for the occupancy map (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn wants_occupancy(&self) -> bool {
        unimplemented!("DEV-560")
    }

    /// Whether the renderer writes per-cell colours (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn uses_cell_colors(&self) -> bool {
        unimplemented!("DEV-560")
    }

    /// The status text the renderer reported after the frames so far (lane hook, see manual_gaps_fakes.md; DEV-560).
    pub fn status(&self) -> Option<String> {
        unimplemented!("DEV-560")
    }
}
