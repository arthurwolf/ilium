//! Tests of the `dithered-waves` scene: the ported behaviour of the former built-in scene
//! (field, lattice, settings clamping) plus the spec parsing and the manifest contract.

use super::field::ThresholdTile;
use super::settings::{
    BAND_SHARPNESS, BIAS, INTENSITY, LEVELS, PIXEL_SIZE, SEED, WAVE_AMPLITUDE, WAVE_FREQUENCY,
    WAVE_SPEED,
};
use super::*;
use crate::ScenesScene;
use ilium_plugin_sdk::generated::anim::renderer_types::FrameInput;
use ilium_plugin_sdk::{SceneField, SceneProgram};
use serde_json::json;
use std::time::{Duration, Instant};

/// Peak grey value of the default picture: 35 percent ink.
const DEFAULT_PEAK: u8 = 89;

fn render_at(
    settings: &DitheredWavesSettings,
    width: usize,
    height: usize,
    seconds: f32,
) -> Vec<u8> {
    let mut scene = DitheredWaves::new(settings);
    let mut pixels = vec![0u8; width * height];
    scene.render(width, height, (seconds * 1000.0) as u64, &mut pixels);
    pixels
}

fn lit(pixels: &[u8]) -> usize {
    pixels.iter().filter(|value| **value > 0).count()
}

fn manifest_scene() -> serde_json::Value {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../manifest.json")).expect("manifest is JSON");
    manifest["contributes"]["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|scene| scene["id"] == DITHERED_WAVES_ID))
        .cloned()
        .expect("dithered-waves is in the manifest")
}

use crate::DITHERED_WAVES_ID;

#[test]
fn rendering_is_a_pure_function_of_time_and_seed() {
    let settings = DitheredWavesSettings::default();
    assert_eq!(
        render_at(&settings, 80, 40, 3.0),
        render_at(&settings, 80, 40, 3.0)
    );
    assert_ne!(
        render_at(&settings, 80, 40, 0.0),
        render_at(&settings, 80, 40, 30.0)
    );
}

#[test]
fn zero_speed_freezes_the_picture() {
    let settings = DitheredWavesSettings {
        wave_speed: 0,
        ..DitheredWavesSettings::default()
    };
    assert_eq!(
        render_at(&settings, 80, 40, 0.0),
        render_at(&settings, 80, 40, 90.0)
    );
}

#[test]
fn different_seeds_give_different_layouts() {
    let one = render_at(&DitheredWavesSettings::default(), 80, 40, 3.0);
    let two = render_at(
        &DitheredWavesSettings {
            seed: 2,
            ..DitheredWavesSettings::default()
        },
        80,
        40,
        3.0,
    );
    assert_ne!(one, two);
}

#[test]
fn normalized_clamps_out_of_range_values() {
    let defaults = DitheredWavesSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = DitheredWavesSettings {
        wave_frequency: 0,
        wave_amplitude: 5000,
        wave_speed: 5000,
        levels: 99,
        pixel_size: 0,
        intensity: 0,
        band_sharpness: 9999,
        bias: -500,
        seed: 1_000_000,
        ..DitheredWavesSettings::default()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.wave_frequency,
            clean.wave_amplitude,
            clean.wave_speed,
            clean.levels,
            clean.pixel_size,
            clean.intensity,
            clean.band_sharpness,
            clean.bias,
            clean.seed
        ),
        (100, 100, 200, 6, 1, 10, 300, -30, 9999)
    );
}

#[test]
fn settings_parse_from_nested_and_bare_objects_and_keep_defaults_for_the_rest() {
    let nested = DitheredWavesSettings::from_spec(&json!({
        "scene": "dithered-waves",
        "settings": {"levels": 4, "pattern": "noise", "ripple": true, "bias": -10}
    }));
    let bare = DitheredWavesSettings::from_spec(&json!({
        "levels": 4, "pattern": "noise", "ripple": true, "bias": -10
    }));
    assert_eq!(nested, bare);
    assert_eq!(nested.levels, 4);
    assert_eq!(nested.pattern, WavePattern::Noise);
    assert!(nested.ripple);
    assert_eq!(nested.bias, -10);
    assert_eq!(nested.wave_frequency, 300);
}

#[test]
fn settings_ignore_unknown_keys_wrong_types_and_clamp_numbers() {
    let parsed = DitheredWavesSettings::from_spec(&json!({
        "settings": {
            "levels": "many", "pattern": "plaid", "ripple": 1, "seed": 123456, "bias": 99,
            "wave_speed": 12.6, "no_such_key": true
        }
    }));
    let defaults = DitheredWavesSettings::default();
    assert_eq!(parsed.levels, defaults.levels);
    assert_eq!(parsed.pattern, defaults.pattern);
    assert_eq!(parsed.ripple, defaults.ripple);
    assert_eq!(parsed.seed, SEED.1);
    assert_eq!(parsed.bias, BIAS.1);
    assert_eq!(parsed.wave_speed, 13);
    assert_eq!(
        DitheredWavesSettings::from_spec(&json!(null)),
        DitheredWavesSettings::default()
    );
}

#[test]
fn default_picture_is_non_empty_not_uniform_and_quiet() {
    let pixels = render_at(&DitheredWavesSettings::default(), 200, 160, 5.0);
    let total = pixels.len();
    let dots_on = lit(&pixels);
    assert!(dots_on > total / 50, "too few dots: {dots_on}/{total}");
    assert!(dots_on < total * 9 / 10, "too many dots: {dots_on}/{total}");
    assert_eq!(pixels.iter().copied().max(), Some(DEFAULT_PEAK));
}

#[test]
fn binary_levels_produce_only_two_values() {
    let pixels = render_at(&DitheredWavesSettings::default(), 80, 80, 1.0);
    assert!(pixels
        .iter()
        .all(|value| *value == 0 || *value == DEFAULT_PEAK));
}

#[test]
fn more_levels_produce_intermediate_values() {
    let settings = DitheredWavesSettings {
        levels: 5,
        ..DitheredWavesSettings::default()
    };
    let pixels = render_at(&settings, 120, 96, 1.0);
    assert!(pixels
        .iter()
        .any(|value| *value > 0 && *value < DEFAULT_PEAK));
}

#[test]
fn every_pattern_and_level_count_renders_bounded_values() {
    for (pattern, _) in WavePattern::ALL {
        for levels in LEVELS.0..=LEVELS.1 {
            let settings = DitheredWavesSettings {
                pattern,
                levels,
                ripple: true,
                ..DitheredWavesSettings::default()
            };
            let pixels = render_at(&settings, 60, 48, 4.0);
            assert!(pixels.iter().all(|value| *value <= DEFAULT_PEAK));
            assert!(lit(&pixels) > 0, "{pattern:?} {levels}");
        }
    }
}

#[test]
fn positive_bias_adds_dots_and_negative_removes_them() {
    let with_bias = |bias| {
        let settings = DitheredWavesSettings {
            bias,
            ..DitheredWavesSettings::default()
        };
        lit(&render_at(&settings, 120, 96, 1.0))
    };
    assert!(with_bias(30) > with_bias(0));
    assert!(with_bias(0) > with_bias(-30));
}

#[test]
fn pixel_size_groups_dots_into_blocks() {
    let settings = DitheredWavesSettings {
        pixel_size: 4,
        ..DitheredWavesSettings::default()
    };
    let (width, height) = (80, 40);
    let pixels = render_at(&settings, width, height, 1.0);
    for block_y in 0..height / 4 {
        for block_x in 0..width / 4 {
            let first = pixels[block_y * 4 * width + block_x * 4];
            for dy in 0..4 {
                for dx in 0..4 {
                    assert_eq!(pixels[(block_y * 4 + dy) * width + block_x * 4 + dx], first);
                }
            }
        }
    }
}

#[test]
fn ripple_changes_the_picture() {
    let plain = render_at(&DitheredWavesSettings::default(), 120, 96, 2.0);
    let rippled = render_at(
        &DitheredWavesSettings {
            ripple: true,
            ..DitheredWavesSettings::default()
        },
        120,
        96,
        2.0,
    );
    assert_ne!(plain, rippled);
}

#[test]
fn bayer_tiles_are_permutations_with_the_standard_layout() {
    for side in [2usize, 4, 8] {
        let tile = ThresholdTile::bayer(side);
        let cells = (side * side) as f32;
        let mut ranks: Vec<u32> = (0..side * side)
            .map(|index| (tile.at(index % side, index / side) * cells - 0.5).round() as u32)
            .collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (0..(side * side) as u32).collect::<Vec<_>>());
    }
    let four = ThresholdTile::bayer(4);
    let expected = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    for (y, row) in expected.iter().enumerate() {
        for (x, rank) in row.iter().enumerate() {
            let value = four.at(x, y) * 16.0 - 0.5;
            assert!((value - *rank as f32).abs() < 1e-4, "({x},{y})");
        }
    }
}

#[test]
fn works_at_tiny_sizes_and_ignores_an_empty_field() {
    for (width, height) in [(1, 1), (3, 2), (1, 7), (9, 1)] {
        for pixel_size in [1, 4] {
            let settings = DitheredWavesSettings {
                pixel_size,
                ..DitheredWavesSettings::default()
            };
            let pixels = render_at(&settings, width, height, 2.0);
            assert_eq!(pixels.len(), width * height);
        }
    }
    assert!(render_at(&DitheredWavesSettings::default(), 0, 0, 1.0).is_empty());
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn full_size_frame_is_cheap() {
    let mut scene = DitheredWaves::new(&DitheredWavesSettings::default());
    let (width, height) = (200, 160);
    let mut pixels = vec![0u8; width * height];
    scene.render(width, height, 1000, &mut pixels);
    let start = Instant::now();
    for step in 0..5 {
        scene.render(width, height, 2000 + step * 1000, &mut pixels);
    }
    let per_frame = start.elapsed() / 5;
    // Generous bound for unoptimised test builds; release is far below 8 ms.
    assert!(per_frame < Duration::from_millis(400), "{per_frame:?}");
}

fn frame_for(field: &SceneField, time_millis: u64) -> FrameInput {
    FrameInput {
        time_millis,
        wall_millis: time_millis,
        now_unix_millis: time_millis,
        width: field.width() as u32,
        height: field.height() as u32,
        palette_rgb: Vec::new(),
    }
}

#[test]
fn the_component_scene_is_chosen_by_the_spec_and_draws_into_the_field() {
    let spec = json!({"scene": DITHERED_WAVES_ID, "settings": {"levels": 3}, "mode": "live"});
    let mut scene = <ScenesScene as SceneProgram>::new(&spec.to_string());
    assert_eq!(SceneProgram::frames_per_second(&scene), FRAMES_PER_SECOND);
    assert!(!SceneProgram::uses_cell_colors(&scene));
    assert!(!SceneProgram::wants_pointer(&scene));
    let mut field = SceneField::new(40, 20);
    let frame = frame_for(&field, 3000);
    SceneProgram::render(&mut scene, &frame, &mut field);
    assert!(field.pixels.iter().any(|value| *value > 0));
    assert!(field.pixels.iter().all(|value| *value <= DEFAULT_PEAK));
    // The field is the same picture the scene draws on its own.
    let direct = render_at(
        &DitheredWavesSettings {
            levels: 3,
            ..DitheredWavesSettings::default()
        },
        field.width(),
        field.height(),
        3.0,
    );
    assert_eq!(field.pixels, direct);
}

#[test]
fn other_scene_ids_keep_the_skeleton_and_leave_the_field_blank() {
    let spec = json!({"scene": "clock"});
    let mut scene = <ScenesScene as SceneProgram>::new(&spec.to_string());
    let mut field = SceneField::new(10, 5);
    let frame = frame_for(&field, 0);
    SceneProgram::render(&mut scene, &frame, &mut field);
    assert!(field.pixels.iter().all(|value| *value == 0));
}

#[test]
fn reconfigure_applies_settings_and_refuses_malformed_text() {
    let spec = json!({"scene": DITHERED_WAVES_ID});
    let mut scene = <ScenesScene as SceneProgram>::new(&spec.to_string());
    assert!(!SceneProgram::reconfigure(&mut scene, "{not json"));
    assert!(!SceneProgram::reconfigure(&mut scene, "[1]"));
    assert!(SceneProgram::reconfigure(
        &mut scene,
        r#"{"wave_speed": 0, "levels": 4}"#
    ));
    let mut field = SceneField::new(20, 10);
    let frame = frame_for(&field, 0);
    SceneProgram::render(&mut scene, &frame, &mut field);
    let first = field.pixels.clone();
    let later = frame_for(&field, 90_000);
    SceneProgram::render(&mut scene, &later, &mut field);
    assert_eq!(first, field.pixels, "speed 0 freezes the picture");
}

#[test]
fn manifest_entry_matches_the_settings_the_scene_reads() {
    let scene = manifest_scene();
    assert_eq!(scene["family"], "procedural");
    let cap = scene["frame_rate_cap"].as_u64().expect("frame_rate_cap");
    assert!(u64::from(FRAMES_PER_SECOND) <= cap);
    let properties = scene["settings_schema"]["properties"]
        .as_object()
        .expect("properties");
    let defaults = DitheredWavesSettings::default();
    let numbers: [(&str, (u32, u32), i64); 9] = [
        (
            "wave_frequency",
            WAVE_FREQUENCY,
            defaults.wave_frequency.into(),
        ),
        (
            "wave_amplitude",
            WAVE_AMPLITUDE,
            defaults.wave_amplitude.into(),
        ),
        ("wave_speed", WAVE_SPEED, defaults.wave_speed.into()),
        ("levels", LEVELS, defaults.levels.into()),
        ("pixel_size", PIXEL_SIZE, defaults.pixel_size.into()),
        ("intensity", INTENSITY, defaults.intensity.into()),
        (
            "band_sharpness",
            BAND_SHARPNESS,
            defaults.band_sharpness.into(),
        ),
        ("seed", SEED, defaults.seed.into()),
        ("bias", (0, 0), defaults.bias.into()),
    ];
    for (key, (minimum, maximum), default) in numbers {
        let property = &properties[key];
        if key == "bias" {
            assert_eq!(property["minimum"], BIAS.0, "{key}");
            assert_eq!(property["maximum"], BIAS.1, "{key}");
        } else {
            assert_eq!(property["minimum"], minimum, "{key}");
            assert_eq!(property["maximum"], maximum, "{key}");
        }
        assert_eq!(property["default"], default, "{key}");
    }
    let pattern = &properties["pattern"];
    let names: Vec<&str> = WavePattern::ALL.iter().map(|(_, name)| *name).collect();
    assert_eq!(pattern["enum"], json!(names));
    assert_eq!(pattern["default"], "bayer4");
    assert_eq!(
        pattern["enum_titles"].as_array().map(Vec::len),
        Some(names.len())
    );
    assert_eq!(properties["ripple"]["default"], defaults.ripple);
    assert_eq!(
        properties.len(),
        11,
        "every schema property is read by the scene"
    );
    // Every property has a title and a help line for its row.
    for (key, property) in properties {
        assert!(
            property["title"]
                .as_str()
                .is_some_and(|text| !text.is_empty()),
            "{key}"
        );
        assert!(
            property["help"]
                .as_str()
                .is_some_and(|text| !text.is_empty()),
            "{key}"
        );
    }
}
