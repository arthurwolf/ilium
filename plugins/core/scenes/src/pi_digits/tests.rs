use super::*;
use crate::kit::control::{ControlKind, ControlValue};
use crate::kit::debug::render_frame;
use crate::kit::settings::assert_schema_matches_controls;
use std::time::Duration;

fn ready(settings: PiSettings) -> PiScene {
    let mut scene = PiScene::new(settings, &SceneEnv::for_test());
    // One frame step spends 1.5 million spigot steps; keep going until the digits are ready.
    let mut guard = 0;
    while scene.generator.is_some() {
        scene.advance_digits();
        guard += 1;
        assert!(guard < 10_000, "the digits must become ready");
    }
    scene
}

#[test]
fn manifest_rows_match_the_scene_rows() {
    let settings = PiSettings::default();
    assert_schema_matches_controls("pi-digits", &settings.controls());
}

#[test]
fn controls_round_trip_with_independent_digit_angles_and_clamped_bounds() {
    let mut settings = PiSettings {
        mode: 1,
        digit_colors: true,
        ..Default::default()
    };
    let mut ids = std::collections::HashSet::new();
    for control in settings.controls() {
        assert!(ids.insert(control.id));
        assert!(settings
            .set_control(control.id, control.stepped(1).unwrap())
            .unwrap());
    }
    let previous = settings.digit_angles;
    settings
        .set_control("digit_3_angle", ControlValue::Number(999))
        .unwrap();
    assert_eq!(settings.digit_angles[3], 360);
    for (digit, angle) in previous.iter().enumerate() {
        if digit != 3 {
            assert_eq!(*angle, settings.digit_angles[digit]);
        }
    }
    settings
        .set_control("digits_limit", ControlValue::Number(i32::MAX))
        .unwrap();
    assert_eq!(settings.digits_limit, 20000);
    assert!(!settings
        .set_control("font_size", ControlValue::Text("wrong".into()))
        .unwrap());
    assert!(!settings
        .set_control("unknown", ControlValue::Number(5))
        .unwrap());
    assert!(settings
        .controls()
        .iter()
        .any(|control| matches!(control.kind, ControlKind::Choice { .. })));
}

#[test]
fn the_spec_settings_reach_the_scene() {
    let spec = serde_json::json!({
        "scene": "pi-digits",
        "settings": {"mode": "font-braille", "digits_limit": 64, "font_size": 16, "digit_colors": true,
                     "colour_angle": 90, "digit_2_angle": 200}
    });
    let settings: PiSettings = crate::scenes2::settings_from_spec(&spec);
    assert_eq!(settings.mode, 1);
    assert_eq!(settings.digits_limit, 64);
    assert_eq!(settings.font_size, 16);
    assert!(settings.digit_colors);
    assert_eq!(settings.colour_angle, 90);
    assert_eq!(settings.digit_angles[2], 200);
}

#[test]
fn font_braille_scroll_and_zero_ink_preserve_real_digit_content() {
    let mut scene = ready(PiSettings {
        mode: 1,
        digits_limit: 100,
        font_size: 16,
        ..Default::default()
    });
    let rendered = render_frame(&mut scene, 40, 12, Duration::ZERO);
    assert!(rendered.raster.dots.iter().any(|value| *value > 0.0));
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    assert_eq!(rendered.cell_colors.len(), 480);
    assert_eq!(scene.native_glyph(0, 0), None);

    scene.apply_settings(PiSettings {
        mode: 0,
        digits_limit: 100,
        scroll_speed: 10,
        ..Default::default()
    });
    render_frame(&mut scene, 40, 12, Duration::from_secs(1));
    assert_eq!(scene.native_glyph(0, 0), Some('.'));
    assert_eq!(scene.native_glyph(1, 0), Some('1'));

    scene.settings.ink_level = 0;
    let hidden = render_frame(&mut scene, 40, 12, Duration::ZERO);
    assert!(hidden.raster.dots.iter().all(|value| *value == 0.0));
    assert_eq!(scene.native_glyph(0, 0), None);
    assert!(hidden.cell_colors.iter().all(|value| *value == [0; 3]));
}

#[test]
fn digits_are_prepared_over_several_frames_and_nothing_is_drawn_meanwhile() {
    let mut scene = PiScene::new(
        PiSettings {
            digits_limit: 2000,
            ..Default::default()
        },
        &SceneEnv::for_test(),
    );
    let first = render_frame(&mut scene, 20, 4, Duration::ZERO);
    assert!(scene
        .status()
        .unwrap()
        .starts_with("Preparing exact Pi digits"));
    assert!(first.cell_colors.iter().all(|value| *value == [0; 3]));
    assert_eq!(scene.native_glyph(0, 0), None);
    let mut frames = 1;
    while scene.status().is_some() {
        render_frame(&mut scene, 20, 4, Duration::ZERO);
        frames += 1;
        assert!(frames < 10_000);
    }
    assert!(frames > 1, "2000 digits need more than one frame of work");
    assert_eq!(scene.native_glyph(0, 0), Some('3'));
}

#[test]
fn a_smaller_limit_reuses_the_digits_already_held() {
    let mut scene = ready(PiSettings {
        digits_limit: 200,
        ..Default::default()
    });
    scene.apply_settings(PiSettings {
        digits_limit: 50,
        ..Default::default()
    });
    assert!(scene.generator.is_none());
    assert!(scene.status().is_none());
    scene.apply_settings(PiSettings {
        digits_limit: 300,
        ..Default::default()
    });
    assert!(scene.generator.is_some());
}

#[test]
fn empty_tiny_targets_and_native_out_of_bounds_are_safe() {
    let mut scene = ready(PiSettings {
        mode: 1,
        digits_limit: 10,
        font_size: 48,
        ..Default::default()
    });
    for (width, height) in [(0u16, 0u16), (1, 1), (2, 3)] {
        let rendered = render_frame(&mut scene, width, height, Duration::ZERO);
        assert_eq!(
            rendered.cell_colors.len(),
            usize::from(width) * usize::from(height)
        );
        assert!(rendered.raster.dots.iter().all(|value| *value == 0.0));
        assert_eq!(scene.native_glyph(width, height), None);
    }
}

#[test]
fn exact_prefix_composes_native_cells_with_decimal() {
    let mut scene = ready(PiSettings {
        digits_limit: 100,
        ..Default::default()
    });
    let rendered = render_frame(&mut scene, 20, 4, Duration::ZERO);
    let text: String = (0..20).filter_map(|x| scene.native_glyph(x, 0)).collect();
    assert_eq!(text, "3.141592653589793238");
    assert!(rendered.raster.dots.iter().all(|value| *value == 0.0));
}

#[test]
fn real_font_atlas_has_ink_for_ten_digits_and_decimal_at_bounded_sizes() {
    for size in [8, 16, 48] {
        let atlas = atlas::build(size).unwrap();
        assert_eq!(atlas.glyphs.len(), 11);
        assert!(atlas.width <= 64 && atlas.height <= 80);
        for glyph in atlas.glyphs {
            assert!(glyph.iter().any(|value| *value > 0.0));
        }
    }
}
