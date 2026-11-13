//! reference-test-harness.md "Scenes": DEV-560 to DEV-562.

use std::cell::RefCell;

use ilium_plugin_sdk_core::anim::FrameInput;
use ilium_plugin_sdk_core::plugin::NativeSurface;
use ilium_plugin_sdk_core::types::Color;
use ilium_plugin_sdk_core::SceneRenderer;
use ilium_plugin_testkit::FakeScene;
use proptest::prelude::*;

#[derive(Default, Debug, Clone)]
struct Seen {
    renders: u32,
    frames: Vec<FrameInput>,
    palettes: Vec<Vec<u32>>,
    /// Number of `render` calls that happened after the last palette change.
    renders_after_palette: u32,
    /// The palette the renderer held at each render call.
    palette_at_render: Vec<Vec<u32>>,
}

thread_local! {
    static SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

fn seen() -> Seen {
    SEEN.with(|state| state.borrow().clone())
}

fn seen_reset() {
    SEEN.with(|state| *state.borrow_mut() = Seen::default());
}

/// Fills every cell with the last digit of `time_millis`; foreground is the first palette colour.
struct Digits {
    palette: Vec<u32>,
}

impl SceneRenderer for Digits {
    fn new(_spec_json: &str) -> Self {
        Digits {
            palette: Vec::new(),
        }
    }

    fn render(&mut self, frame: &FrameInput, surface: &mut NativeSurface) {
        SEEN.with(|state| {
            let mut state = state.borrow_mut();
            state.renders += 1;
            state.renders_after_palette += 1;
            state.frames.push(frame.clone());
            state.palette_at_render.push(self.palette.clone());
        });
        let digit = char::from_digit((frame.time_millis % 10) as u32, 10).unwrap_or('?');
        let foreground = self.palette.first().copied().unwrap_or(0xFFFFFF);
        for cell in surface.cells.iter_mut() {
            *cell = (digit, foreground, 0x000000);
        }
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        SEEN.with(|state| {
            let mut state = state.borrow_mut();
            state.palettes.push(palette_rgb.clone());
            state.renders_after_palette = 0;
        });
        self.palette = palette_rgb;
    }

    fn frames_per_second(&self) -> u32 {
        30
    }
}

fn input_at(time_millis: u64) -> FrameInput {
    FrameInput {
        time_millis,
        ..FrameInput::default()
    }
}

// ---- DEV-560 ---------------------------------------------------------------------------------

/// REQ: DEV-560
#[test]
fn dev_560_frame_returns_a_screen_of_the_scene_size() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 12, 3);
    let screen = scene.frame(FrameInput::default());
    assert_eq!((screen.columns, screen.rows), (12, 3));
    assert_eq!(screen.cells.len(), 36);
}

/// REQ: DEV-560
#[test]
fn dev_560_frame_calls_render_exactly_once() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 2);
    scene.frame(FrameInput::default());
    assert_eq!(seen().renders, 1);
    scene.frame(FrameInput::default());
    assert_eq!(seen().renders, 2);
}

/// REQ: DEV-560
#[test]
fn dev_560_the_surface_cells_become_the_screen_cells() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 2);
    let screen = scene.frame(input_at(7));
    for row in 0..2u16 {
        for column in 0..4u16 {
            let cell = screen.cell(row, column);
            assert_eq!(cell.symbol, "7");
            assert_eq!(cell.foreground, Color::Rgb(0xFFFFFF));
            assert_eq!(cell.background, Color::Rgb(0x000000));
        }
    }
}

/// REQ: DEV-560
#[test]
fn dev_560_the_screen_text_has_one_line_per_row() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 3, 2);
    let screen = scene.frame(input_at(5));
    assert_eq!(screen.text(), "555\n555");
}

/// REQ: DEV-560
#[test]
fn dev_560_the_smallest_scene_is_one_cell() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 1, 1);
    let screen = scene.frame(input_at(3));
    assert_eq!(screen.text(), "3");
}

/// REQ: DEV-560
#[test]
fn dev_560_the_renderer_receives_the_given_frame_input() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 2);
    let input = FrameInput {
        time_millis: 40,
        wall_millis: 41,
        now_unix_millis: 42,
        width: 4,
        height: 2,
        palette_rgb: vec![1, 2],
    };
    scene.frame(input.clone());
    assert_eq!(seen().frames, vec![input]);
}

/// REQ: DEV-560
#[test]
fn dev_560_scenes_of_different_sizes_give_different_screens() {
    seen_reset();
    let small = FakeScene::new::<Digits>("example.scene", 2, 1).frame(input_at(1));
    let large = FakeScene::new::<Digits>("example.scene", 5, 4).frame(input_at(1));
    assert_ne!((small.columns, small.rows), (large.columns, large.rows));
}

proptest! {
    // REQ: DEV-560
    #[test]
    fn dev_560_property_the_screen_has_columns_times_rows_cells(columns in 1u16..80, rows in 1u16..40) {
        seen_reset();
        let mut scene = FakeScene::new::<Digits>("example.scene", columns, rows);
        let screen = scene.frame(FrameInput::default());
        prop_assert_eq!((screen.columns, screen.rows), (columns, rows));
        prop_assert_eq!(screen.cells.len(), columns as usize * rows as usize);
    }
}

// ---- DEV-561 ---------------------------------------------------------------------------------

/// REQ: DEV-561
#[test]
fn dev_561_the_default_frame_time_is_zero_fake_ms_after_creation() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 2);
    scene.frame(FrameInput::default());
    assert_eq!(seen().frames[0].time_millis, 0);
}

/// REQ: DEV-561
#[test]
fn dev_561_two_frames_with_equal_inputs_give_equal_screens() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 6, 2);
    let first = scene.frame(input_at(123));
    let second = scene.frame(input_at(123));
    assert_eq!(first, second);
}

/// REQ: DEV-561
#[test]
fn dev_561_two_scenes_drawing_the_same_input_agree() {
    seen_reset();
    let first = FakeScene::new::<Digits>("example.scene", 6, 2).frame(input_at(9));
    let second = FakeScene::new::<Digits>("example.scene", 6, 2).frame(input_at(9));
    assert_eq!(first, second);
}

/// REQ: DEV-561
#[test]
fn dev_561_a_default_frame_does_not_depend_on_the_real_wall_clock() {
    seen_reset();
    let first = FakeScene::new::<Digits>("example.scene", 4, 1).frame(FrameInput::default());
    std::thread::sleep(std::time::Duration::from_millis(30));
    let second = FakeScene::new::<Digits>("example.scene", 4, 1).frame(FrameInput::default());
    assert_eq!(first, second);
    let frames = seen().frames;
    assert_eq!(frames[0].time_millis, frames[1].time_millis);
    assert_eq!(frames[0].now_unix_millis, frames[1].now_unix_millis);
}

/// REQ: DEV-561
#[test]
fn dev_561_different_times_give_different_screens() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    assert_ne!(scene.frame(input_at(1)), scene.frame(input_at(2)));
}

proptest! {
    // REQ: DEV-561
    #[test]
    fn dev_561_property_equal_inputs_give_equal_screens(time in any::<u64>(), columns in 1u16..40, rows in 1u16..20) {
        seen_reset();
        let mut scene = FakeScene::new::<Digits>("example.scene", columns, rows);
        let first = scene.frame(input_at(time));
        let second = scene.frame(input_at(time));
        prop_assert_eq!(first, second);
    }
}

// ---- DEV-562 ---------------------------------------------------------------------------------

/// REQ: DEV-562
#[test]
fn dev_562_the_palette_reaches_the_renderer() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    scene.set_palette(vec![0xFF0000, 0x00FF00]);
    assert_eq!(seen().palettes, vec![vec![0xFF0000, 0x00FF00]]);
}

/// REQ: DEV-562
#[test]
fn dev_562_the_next_frame_is_drawn_with_the_new_palette() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    scene.set_palette(vec![0xFF0000]);
    let screen = scene.frame(input_at(1));
    assert_eq!(screen.cell(0, 0).foreground, Color::Rgb(0xFF0000));
    assert_eq!(seen().palette_at_render[0], vec![0xFF0000]);
}

/// REQ: DEV-562
#[test]
fn dev_562_a_frame_before_the_call_keeps_the_old_palette() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    let before = scene.frame(input_at(1));
    scene.set_palette(vec![0x00FF00]);
    let after = scene.frame(input_at(1));
    assert_eq!(before.cell(0, 0).foreground, Color::Rgb(0xFFFFFF));
    assert_eq!(after.cell(0, 0).foreground, Color::Rgb(0x00FF00));
}

/// REQ: DEV-562
#[test]
fn dev_562_set_palette_does_not_draw_a_frame() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    scene.set_palette(vec![0x123456]);
    assert_eq!(seen().renders, 0);
}

/// REQ: DEV-562
#[test]
fn dev_562_the_last_palette_wins() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    scene.set_palette(vec![0x111111]);
    scene.set_palette(vec![0x222222]);
    let screen = scene.frame(input_at(1));
    assert_eq!(screen.cell(0, 0).foreground, Color::Rgb(0x222222));
}

/// REQ: DEV-562
#[test]
fn dev_562_an_empty_palette_is_passed_on_as_empty() {
    seen_reset();
    let mut scene = FakeScene::new::<Digits>("example.scene", 4, 1);
    scene.set_palette(vec![0xABCDEF]);
    scene.set_palette(Vec::new());
    scene.frame(input_at(1));
    assert_eq!(seen().palette_at_render[0], Vec::<u32>::new());
}
