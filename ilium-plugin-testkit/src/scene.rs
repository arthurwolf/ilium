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

#[cfg(test)]
mod spec_tests {
    use super::FakeScene;
    use ilium_plugin_sdk_core::anim::FrameInput;
    use ilium_plugin_sdk_core::plugin::NativeSurface;
    use ilium_plugin_sdk_core::SceneRenderer;

    /// Paints every cell with `x`; writes the palette length into the first cell so that palette delivery is observable.
    #[derive(Default)]
    struct MarkingRenderer {
        palette_length: usize,
    }

    impl SceneRenderer for MarkingRenderer {
        fn new(_spec_json: &str) -> Self {
            Self::default()
        }

        fn render(&mut self, _frame: &FrameInput, surface: &mut NativeSurface) {
            for cell in surface.cells.iter_mut() {
                cell.0 = 'x';
            }
            if let Some(first_cell) = surface.cells.first_mut() {
                first_cell.0 = char::from_digit(self.palette_length as u32, 10).unwrap();
            }
        }

        fn set_palette(&mut self, palette_rgb: Vec<u32>) {
            self.palette_length = palette_rgb.len();
        }

        fn frames_per_second(&self) -> u32 {
            30
        }
    }

    /// Manual: `FakeScene::frame` calls `render` once and returns a `Screen` of the scene size
    #[test]
    fn frame_returns_screen_of_scene_size_filled_by_render() {
        let mut scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        let screen = scene.frame(FrameInput::default());
        assert_eq!(screen.columns, 4);
        assert_eq!(screen.rows, 2);
        assert_eq!(screen.cells.len(), 8);
        assert_eq!(screen.cell(0, 0).symbol, "0");
        assert_eq!(screen.cell(1, 3).symbol, "x");
    }

    /// Manual: empty input: a zero-size scene gives a `Screen` with no cells
    #[test]
    fn frame_of_zero_size_scene_has_no_cells() {
        let mut scene = FakeScene::new::<MarkingRenderer>("scene", 0, 0);
        let screen = scene.frame(FrameInput::default());
        assert_eq!(screen.columns, 0);
        assert_eq!(screen.rows, 0);
        assert!(screen.cells.is_empty());
    }

    /// Manual: equal inputs on the fake clock give equal screens
    #[test]
    fn equal_inputs_give_equal_screens() {
        let mut scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        let first_screen = scene.frame(FrameInput::default());
        let second_screen = scene.frame(FrameInput::default());
        assert_eq!(first_screen, second_screen);
    }

    /// Manual: `set_palette` passes the palette to the renderer before the next frame
    #[test]
    fn set_palette_reaches_renderer_before_next_frame() {
        let mut scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        scene.set_palette(vec![0x112233, 0x445566]);
        let screen = scene.frame(FrameInput::default());
        assert_eq!(screen.cell(0, 0).symbol, "2");
    }

    #[test]
    fn frames_per_second_reports_renderer_request() {
        let scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        assert_eq!(scene.frames_per_second(), 30);
    }

    #[test]
    fn reconfigure_is_false_when_renderer_cannot_accept_in_place() {
        let mut scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        assert!(!scene.reconfigure("{}"));
    }

    #[test]
    fn status_is_none_before_renderer_reports_any() {
        let scene = FakeScene::new::<MarkingRenderer>("scene", 4, 2);
        assert_eq!(scene.status(), None);
    }
}
