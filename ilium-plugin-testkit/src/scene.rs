//! FakeScene: drives one scene renderer (reference-test-harness.md "Scenes").

use ilium_plugin_sdk_core::anim::FrameInput;
use ilium_plugin_sdk_core::plugin::NativeSurface;
use ilium_plugin_sdk_core::types::Color;
use ilium_plugin_sdk_core::SceneRenderer;

use crate::host::{Screen, ScreenCell};

/// Start of the fake clock in unix milliseconds (same instant as `FakeHost`).
const START_MS: u64 = 1_893_456_000_000;

/// A renderer with its type erased.
trait ErasedRenderer {
    fn render(&mut self, frame: &FrameInput, surface: &mut NativeSurface);
    fn set_palette(&mut self, palette_rgb: Vec<u32>);
    fn frames_per_second(&self) -> u32;
}

struct Wrapped<R: SceneRenderer>(R);

impl<R: SceneRenderer> ErasedRenderer for Wrapped<R> {
    fn render(&mut self, frame: &FrameInput, surface: &mut NativeSurface) {
        self.0.render(frame, surface);
    }
    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.0.set_palette(palette_rgb);
    }
    fn frames_per_second(&self) -> u32 {
        self.0.frames_per_second()
    }
}

/// Builds a renderer from a spec JSON.
type RendererFactory = Box<dyn Fn(&str) -> Box<dyn ErasedRenderer>>;

/// One scene of a given size on the fake clock (DEV-560).
pub struct FakeScene {
    renderer: Box<dyn ErasedRenderer>,
    factory: RendererFactory,
    scene_id: String,
    columns: u16,
    rows: u16,
    settings_json: String,
    location: Option<(f64, f64)>,
    palette_rgb: Vec<u32>,
}

impl std::fmt::Debug for FakeScene {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeScene")
            .field("scene_id", &self.scene_id)
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .finish()
    }
}

impl FakeScene {
    /// A scene of the given size on the fake clock (DEV-560).
    pub fn new<R: SceneRenderer + 'static>(scene_id: &str, columns: u16, rows: u16) -> FakeScene {
        let factory: RendererFactory =
            Box::new(|spec_json| Box::new(Wrapped(R::new(spec_json))) as Box<dyn ErasedRenderer>);
        let renderer = factory("{}");
        FakeScene {
            renderer,
            factory,
            scene_id: scene_id.to_string(),
            columns,
            rows,
            settings_json: "{}".to_string(),
            location: None,
            palette_rgb: Vec::new(),
        }
    }

    /// The spec JSON handed to the renderer: the settings object plus the location when set.
    fn spec_json(&self) -> String {
        let mut spec: serde_json::Value = serde_json::from_str(&self.settings_json)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
        if let (Some((latitude, longitude)), Some(object)) = (self.location, spec.as_object_mut()) {
            object.insert("latitude".to_string(), serde_json::json!(latitude));
            object.insert("longitude".to_string(), serde_json::json!(longitude));
        }
        spec.to_string()
    }

    fn rebuild(&mut self) {
        let spec = self.spec_json();
        self.renderer = (self.factory)(&spec);
        if !self.palette_rgb.is_empty() {
            self.renderer.set_palette(self.palette_rgb.clone());
        }
    }

    /// Draws one frame: calls `render` once and reads the surface as a `Screen` of the scene size (DEV-560, DEV-561).
    pub fn frame(&mut self, input: FrameInput) -> Screen {
        let mut frame = input;
        if frame.width == 0 {
            frame.width = u32::from(self.columns);
        }
        if frame.height == 0 {
            frame.height = u32::from(self.rows);
        }
        if frame.wall_millis == 0 {
            frame.wall_millis = frame.time_millis;
        }
        if frame.now_unix_millis == 0 {
            frame.now_unix_millis = START_MS + frame.time_millis;
        }
        if frame.palette_rgb.is_empty() {
            frame.palette_rgb = self.palette_rgb.clone();
        }
        let cell_count = usize::from(self.columns) * usize::from(self.rows);
        let mut surface = NativeSurface {
            columns: self.columns,
            rows: self.rows,
            cells: vec![(' ', 0, 0); cell_count],
        };
        self.renderer.render(&frame, &mut surface);
        let mut screen = Screen::blank(self.columns, self.rows);
        for (target, source) in screen.cells.iter_mut().zip(surface.cells.iter()) {
            *target = ScreenCell {
                symbol: source.0.to_string(),
                foreground: Color::Rgb(source.1),
                background: Color::Rgb(source.2),
                ..ScreenCell::default()
            };
        }
        screen
    }

    /// Passes the palette to the renderer before the next frame (DEV-562).
    pub fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.palette_rgb = palette_rgb.clone();
        self.renderer.set_palette(palette_rgb);
    }

    /// Rebuilds the renderer with these settings.
    pub fn with_settings(mut self, settings_json: &str) -> FakeScene {
        self.settings_json = settings_json.to_string();
        self.rebuild();
        self
    }

    /// Rebuilds the renderer with this observer location.
    pub fn with_location(mut self, latitude: f64, longitude: f64) -> FakeScene {
        self.location = Some((latitude, longitude));
        self.rebuild();
        self
    }

    /// Reconfigures in place; the native renderer contract has no such call, so this is always false.
    pub fn reconfigure(&mut self, _settings_json: &str) -> bool {
        false
    }

    /// The frame rate the renderer asks for.
    pub fn frames_per_second(&self) -> u32 {
        self.renderer.frames_per_second()
    }

    /// Whether the renderer wants pointer input; the native contract has no such call.
    pub fn wants_pointer(&self) -> bool {
        false
    }

    /// Whether the renderer wants occupancy input; the native contract has no such call.
    pub fn wants_occupancy(&self) -> bool {
        false
    }

    /// Whether the renderer draws its own cell colours; the native contract has no such call.
    pub fn uses_cell_colors(&self) -> bool {
        false
    }

    /// The status line the renderer reports; none before it reports any.
    pub fn status(&self) -> Option<String> {
        None
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
