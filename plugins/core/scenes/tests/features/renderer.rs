//! Renderer lifecycle shared by all scenes (api-scenes.md, COR-011).
use crate::common::*;
use crate::feature_support::*;
use serde_json::json;

/// REQ: COR-011, F-096
#[test]
fn renderer_011_frame_has_the_requested_cell_size() {
    let mut scene = fake_scene("waves", 40, 10);
    let screen = scene.frame(braille_frame(40, 10, 0));
    assert_eq!((screen.columns, screen.rows), (40, 10));
}

/// REQ: COR-011, F-096
#[test]
fn renderer_011_boundary_minimum_and_maximum_sizes_render() {
    for (columns, rows) in [(10u16, 1u16), (11, 2), (499, 199), (500, 200)] {
        let mut scene = fake_scene("waves", columns, rows);
        let screen = scene.frame(braille_frame(columns, rows, 0));
        assert_eq!((screen.columns, screen.rows), (columns, rows));
    }
}

/// REQ: COR-011, F-096
#[test]
fn renderer_011_size_outside_the_harness_range_is_refused_with_render_size() {
    let result =
        std::panic::catch_unwind(|| fake_scene("waves", 9, 1).frame(braille_frame(9, 1, 0)));
    let message = format!(
        "{:?}",
        result
            .err()
            .and_then(|e| e.downcast_ref::<String>().cloned())
    );
    assert!(message.contains("harness.render-size"), "{message}");
}

/// REQ: COR-011
#[test]
fn renderer_011_unknown_scene_id_is_refused_with_not_found() {
    let result = std::panic::catch_unwind(|| {
        fake_scene("no-such-scene", 40, 10).frame(braille_frame(40, 10, 0))
    });
    let message = format!(
        "{:?}",
        result
            .err()
            .and_then(|e| e.downcast_ref::<String>().cloned())
    );
    assert!(message.contains("not-found"), "{message}");
}

/// REQ: COR-011
#[test]
fn renderer_011_same_input_gives_the_same_frame() {
    let first = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500));
    let second = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500));
    assert_eq!(first.text(), second.text());
}

/// REQ: COR-011
#[test]
fn renderer_011_palette_change_keeps_the_tone_cells() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let after = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        before.text(),
        after.text(),
        "the host colours tones; the scene draws the same cells"
    );
}

/// REQ: COR-011
#[test]
fn renderer_011_empty_palette_is_accepted() {
    let mut scene = fake_scene("waves", 40, 10);
    scene.set_palette(Vec::new());
    assert_eq!(scene.frame(braille_frame(40, 10, 0)).columns, 40);
}

/// REQ: COR-011
#[test]
fn renderer_011_settings_object_with_unknown_keys_does_not_break_the_scene() {
    let plain = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 0));
    let mut scene = fake_scene_with("waves", 40, 10, &json!({"unknown": 1}));
    let screen = scene.frame(braille_frame(40, 10, 0));
    assert_eq!((screen.columns, screen.rows), (40, 10));
    assert_eq!(screen.text(), plain.text(), "unknown keys are ignored");
}

/// REQ: COR-011
#[test]
fn renderer_011_spec_helper_matches_the_documented_fields() {
    let value: serde_json::Value =
        serde_json::from_str(&spec("waves", json!({}), &[1, 2], "live")).unwrap();
    assert_eq!(value["scene"], "waves");
    assert_eq!(value["palette"], json!([1, 2]));
}
