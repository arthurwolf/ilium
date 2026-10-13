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
}
