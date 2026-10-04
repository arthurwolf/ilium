use super::settings::DitherSize;
use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use std::time::Duration;

fn scene_with(settings: &DitherWaterSettings) -> DitherWaterScene {
    DitherWaterScene::new(
        settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    )
}

fn frame_at(settings: &DitherWaterSettings, cols: u16, rows: u16, time: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, cols, rows, Duration::from_secs_f64(time))
}

fn lit(rendered: &Rendered) -> usize {
    rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.0)
        .count()
}

#[test]
fn same_time_gives_identical_raster_and_colors() {
    let settings = DitherWaterSettings::default();
    let first = frame_at(&settings, 40, 20, 12.5);
    let second = frame_at(&settings, 40, 20, 12.5);
    assert_eq!(first.raster.dots, second.raster.dots);
    assert_eq!(first.cell_colors, second.cell_colors);
}

#[test]
fn render_does_not_depend_on_previous_frames() {
    let settings = DitherWaterSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 40, 20, Duration::from_secs(3));
    let later = render_frame(&mut scene, 40, 20, Duration::from_secs(9));
    let fresh = frame_at(&settings, 40, 20, 9.0);
    assert_eq!(later.raster.dots, fresh.raster.dots);
}

#[test]
fn different_times_differ() {
    let settings = DitherWaterSettings::default();
    let first = frame_at(&settings, 40, 20, 1.0);
    let second = frame_at(&settings, 40, 20, 4.0);
    assert_ne!(first.raster.dots, second.raster.dots);
}

#[test]
fn different_seeds_differ() {
    let first = frame_at(&DitherWaterSettings::default(), 40, 20, 2.0);
    let other = DitherWaterSettings {
        seed: 500,
        ..DitherWaterSettings::default()
    };
    assert_ne!(first.raster.dots, frame_at(&other, 40, 20, 2.0).raster.dots);
}

#[test]
fn normalized_clamps_out_of_range_values() {
    let defaults = DitherWaterSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = DitherWaterSettings {
        wave_scale: 0,
        flow_speed: 99_999,
        sharpness: 0,
        density: -500,
        brightness: 1000,
        perspective: 1000,
        seed: 1_000_000,
        ..DitherWaterSettings::default()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.wave_scale,
            clean.flow_speed,
            clean.sharpness,
            clean.density,
            clean.brightness,
            clean.perspective,
            clean.seed
        ),
        (50, 300, 1, -30, 80, 100, 999)
    );
    let low = DitherWaterSettings {
        density: 500,
        brightness: 0,
        ..DitherWaterSettings::default()
    };
    assert_eq!(
        (low.normalized().density, low.normalized().brightness),
        (30, 15)
    );
}

#[test]
fn settings_survive_a_json_round_trip_and_partial_files() {
    let settings = DitherWaterSettings {
        dither: DitherSize::Bayer8,
        stepped: true,
        ..DitherWaterSettings::default()
    };
    let text = serde_json::to_string(&settings).expect("serialize");
    let back: DitherWaterSettings = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, settings);
    let partial: DitherWaterSettings =
        serde_json::from_str(r#"{"sharpness": 5}"#).expect("partial");
    assert_eq!(partial.sharpness, 5);
    assert_eq!(partial.brightness, 45);
}

fn alternative_value(control: &Control) -> ControlValue {
    match (&control.kind, &control.value) {
        (ControlKind::Slider { min, max, .. }, ControlValue::Number(number)) => {
            ControlValue::Number(if number == min { *max } else { *min })
        }
        (ControlKind::Choice { options }, ControlValue::Index(index)) => {
            ControlValue::Index((index + 1) % options.len())
        }
        (ControlKind::Toggle, ControlValue::Bool(on)) => ControlValue::Bool(!on),
        _ => panic!("unexpected control kind"),
    }
}

use crate::control::Control;

#[test]
fn controls_have_sane_shape() {
    let controls = DitherWaterSettings::default().controls();
    assert!((6..=12).contains(&controls.len()));
    let ids: std::collections::BTreeSet<_> = controls.iter().map(|control| control.id).collect();
    assert_eq!(ids.len(), controls.len());
    for control in &controls {
        assert!(!control.help.is_empty() && !control.label.is_empty());
        if let (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&control.kind, &control.value)
        {
            assert!((min..=max).contains(&value), "{} out of range", control.id);
        }
    }
}

#[test]
fn every_control_round_trips_through_set_control() {
    let defaults = DitherWaterSettings::default();
    for control in defaults.controls() {
        let mut settings = defaults.clone();
        let target = alternative_value(&control);
        assert_eq!(
            settings.set_control(control.id, target.clone()),
            Ok(true),
            "{}",
            control.id
        );
        let after = settings
            .controls()
            .into_iter()
            .find(|row| row.id == control.id)
            .expect("row remains");
        assert_eq!(after.value, target, "{}", control.id);
        assert_eq!(settings.set_control(control.id, target), Ok(false));
        assert_eq!(settings.normalized(), settings);
    }
}

#[test]
fn set_control_rejects_unknown_ids_and_clamps_or_errors_on_bad_values() {
    let mut settings = DitherWaterSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("sharpness", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("dither", ControlValue::Index(9))
        .is_err());
    assert_eq!(
        settings.set_control("brightness", ControlValue::Number(10_000)),
        Ok(true)
    );
    assert_eq!(settings.brightness, 80);
}

#[test]
fn defaults_draw_a_non_uniform_quiet_picture() {
    let rendered = frame_at(&DitherWaterSettings::default(), 100, 40, 5.0);
    let total = rendered.raster.dots.len();
    let on = lit(&rendered);
    assert!(on > total / 20, "too empty: {on}/{total}");
    assert!(on < total * 3 / 4, "too full: {on}/{total}");
    let peak = rendered.raster.dots.iter().copied().fold(0.0, f32::max);
    assert!((peak - 1.0).abs() < 1e-6, "peak {peak}");
    // Quiet: tint colours never exceed the pale-cyan end of the palette.
    assert!(rendered
        .cell_colors
        .iter()
        .all(|c| c[2] <= 230 && c[0] <= 141));
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|dot| (0.0..=1.0).contains(dot)));
}

#[test]
fn raster_is_strictly_one_bit() {
    let rendered = frame_at(&DitherWaterSettings::default(), 30, 10, 2.0);
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|dot| *dot == 0.0 || (*dot - 1.0).abs() < 1e-6));
}

#[test]
fn tiny_sizes_do_not_panic() {
    for (cols, rows) in [(1, 1), (3, 2), (1, 5), (7, 1)] {
        for ripples in [true, false] {
            for tint in [true, false] {
                let settings = DitherWaterSettings {
                    ripples,
                    tint,
                    ..DitherWaterSettings::default()
                };
                let rendered = frame_at(&settings, cols, rows, 3.0);
                assert_eq!(
                    rendered.raster.dots.len(),
                    usize::from(cols) * 2 * usize::from(rows) * 4
                );
            }
        }
    }
    let mut scene = scene_with(&DitherWaterSettings::default());
    let empty = render_frame(&mut scene, 0, 0, Duration::ZERO);
    assert!(empty.raster.dots.is_empty());
}

#[test]
fn survives_huge_and_zero_times() {
    let settings = DitherWaterSettings::default();
    for seconds in [0.0, 1.0e6, 3.0e8] {
        let rendered = frame_at(&settings, 30, 10, seconds);
        assert!(lit(&rendered) > 0, "empty at t={seconds}");
    }
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn tint_controls_cell_colors() {
    let tinted = DitherWaterSettings::default();
    assert!(scene_with(&tinted).uses_cell_colors());
    let rendered = frame_at(&tinted, 30, 10, 2.0);
    assert_eq!(rendered.cell_colors.len(), 300);
    assert!(rendered
        .cell_colors
        .iter()
        .all(|c| c[2] >= c[0] && c[2] > 60));
    let distinct: std::collections::BTreeSet<_> = rendered.cell_colors.iter().collect();
    assert!(distinct.len() > 3);
    let plain = DitherWaterSettings {
        tint: false,
        ..tinted
    };
    assert!(!scene_with(&plain).uses_cell_colors());
}

#[test]
fn density_bias_adds_and_removes_dots() {
    let count = |density: i32| {
        let settings = DitherWaterSettings {
            density,
            ripples: false,
            ..DitherWaterSettings::default()
        };
        lit(&frame_at(&settings, 40, 20, 3.0))
    };
    assert!(count(30) > count(0));
    assert!(count(0) > count(-30));
}

#[test]
fn sharper_caustics_light_fewer_dots() {
    let count = |sharpness: u32| {
        let settings = DitherWaterSettings {
            sharpness,
            ripples: false,
            ..DitherWaterSettings::default()
        };
        lit(&frame_at(&settings, 40, 20, 3.0))
    };
    assert!(count(1) > count(6));
}

#[test]
fn brightness_dims_dots_below_reference_and_lifts_tint_above_it() {
    let peak = |settings: &DitherWaterSettings| {
        frame_at(settings, 20, 10, 1.0)
            .raster
            .dots
            .iter()
            .copied()
            .fold(0.0, f32::max)
    };
    let dim = DitherWaterSettings {
        brightness: 20,
        ..DitherWaterSettings::default()
    };
    assert!((peak(&dim) - 20.0 / 45.0).abs() < 1e-6);
    let bright = DitherWaterSettings {
        brightness: 80,
        ..DitherWaterSettings::default()
    };
    assert!((peak(&bright) - 1.0).abs() < 1e-6);
    let sum = |settings: &DitherWaterSettings| -> u32 {
        frame_at(settings, 20, 10, 1.0)
            .cell_colors
            .iter()
            .map(|c| u32::from(c[2]))
            .sum()
    };
    assert!(sum(&bright) > sum(&DitherWaterSettings::default()));
}

#[test]
fn ripples_change_the_picture_only_when_enabled() {
    let on = DitherWaterSettings::default();
    let off = DitherWaterSettings {
        ripples: false,
        ..on.clone()
    };
    // Later than the first ring's spawn, so at least one ring is alive.
    assert_ne!(
        frame_at(&on, 40, 20, 4.0).raster.dots,
        frame_at(&off, 40, 20, 4.0).raster.dots
    );
    let flipped = DitherWaterSettings {
        ripples: false,
        ..off.clone()
    };
    assert_eq!(
        frame_at(&off, 40, 20, 4.0).raster.dots,
        frame_at(&flipped, 40, 20, 4.0).raster.dots
    );
}

#[test]
fn stepped_motion_holds_frames_within_a_step() {
    let settings = DitherWaterSettings {
        stepped: true,
        ..DitherWaterSettings::default()
    };
    let first = frame_at(&settings, 30, 10, 1.01);
    let same_step = frame_at(&settings, 30, 10, 1.10);
    let next_step = frame_at(&settings, 30, 10, 1.20);
    assert_eq!(first.raster.dots, same_step.raster.dots);
    assert_ne!(first.raster.dots, next_step.raster.dots);
}

#[test]
fn bayer_matrices_are_permutations() {
    for side in [2usize, 4, 8] {
        let mut ranks = bayer_ranks(side);
        ranks.sort_unstable();
        let expected: Vec<u32> = (0..(side * side) as u32).collect();
        assert_eq!(ranks, expected);
    }
    assert_eq!(bayer_ranks(2), vec![0, 2, 3, 1]);
}

#[test]
fn every_dither_size_renders_something() {
    for dither in DitherSize::ALL {
        let settings = DitherWaterSettings {
            dither,
            ..DitherWaterSettings::default()
        };
        let rendered = frame_at(&settings, 30, 10, 2.0);
        assert!(lit(&rendered) > 0 && lit(&rendered) < rendered.raster.dots.len());
    }
}

#[test]
fn one_full_size_frame_is_cheap() {
    let settings = DitherWaterSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(1));
    let start = std::time::Instant::now();
    let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(2));
    // Generous bound so debug builds on a busy machine pass; release is a
    // few milliseconds.
    assert!(start.elapsed() < Duration::from_millis(400));
}

#[test]
fn reports_no_status_and_a_valid_frame_rate() {
    let scene = scene_with(&DitherWaterSettings::default());
    assert!(scene.status().is_none());
    assert!((1..=30).contains(&scene.frames_per_second()));
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    let mut scene = scene_with(&DitherWaterSettings::default());
    for time in [2.0, 6.0] {
        let rendered = render_frame(&mut scene, 100, 40, Duration::from_secs_f64(time));
        // 60 is the host's default density.
        for line in rendered.braille_lines(60, crate::raster::DitherMode::Ordered) {
            println!("{line}");
        }
        println!("----");
    }
}

#[test]
fn provided_palette_changes_tint_colors_and_none_keeps_them() {
    let settings = DitherWaterSettings::default();
    let plain = frame_at(&settings, 30, 10, 2.0);
    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.palette = ScenePalette {
        stops: vec![[200, 20, 20], [250, 200, 40]],
        reverse: false,
        shift_percent: 0,
    };
    let mut scene = DitherWaterScene::new(&settings, &env);
    let tinted = render_frame(&mut scene, 30, 10, Duration::from_secs_f64(2.0));
    assert_ne!(plain.cell_colors, tinted.cell_colors);
    scene.set_palette(&ScenePalette::default());
    let reset = render_frame(&mut scene, 30, 10, Duration::from_secs_f64(2.0));
    assert_eq!(plain.cell_colors, reset.cell_colors);
    assert!(scene.follows_palette());
}
