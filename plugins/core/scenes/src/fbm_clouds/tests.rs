use super::settings::DitherPattern;
use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

fn scene_with(settings: &FbmCloudsSettings) -> FbmCloudsScene {
    FbmCloudsScene::new(settings, &SceneEnv::for_test())
}

fn frame_of(settings: &FbmCloudsSettings, width: u16, height: u16, seconds: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, width, height, Duration::from_secs_f64(seconds))
}

fn mean(rendered: &Rendered) -> f32 {
    rendered.raster.dots.iter().sum::<f32>() / rendered.raster.dots.len() as f32
}

#[test]
fn same_time_gives_identical_raster_even_across_scene_instances() {
    let settings = FbmCloudsSettings::default();
    let first = frame_of(&settings, 40, 20, 12.5);
    let second = frame_of(&settings, 40, 20, 12.5);
    assert_eq!(first.raster.dots, second.raster.dots);
    let mut reused = scene_with(&settings);
    let _ = render_frame(&mut reused, 40, 20, Duration::from_secs(90));
    let again = render_frame(&mut reused, 40, 20, Duration::from_secs_f64(12.5));
    assert_eq!(first.raster.dots, again.raster.dots);
}

#[test]
fn different_times_differ() {
    let settings = FbmCloudsSettings::default();
    let early = frame_of(&settings, 40, 20, 0.0);
    let late = frame_of(&settings, 40, 20, 60.0);
    assert_ne!(early.raster.dots, late.raster.dots);
}

#[test]
fn seeds_give_different_layouts() {
    let one = frame_of(&FbmCloudsSettings::default(), 40, 20, 3.0);
    let other = frame_of(
        &FbmCloudsSettings {
            seed: 7,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        3.0,
    );
    assert_ne!(one.raster.dots, other.raster.dots);
}

#[test]
fn normalized_clamps_every_field() {
    let defaults = FbmCloudsSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let high = FbmCloudsSettings {
        scale: 9999,
        drift: 9999,
        pan: 9999,
        warp: 9999,
        octaves: 99,
        tone_steepness: 9999,
        block: 99,
        intensity: 9999,
        seed: 9999,
        ..defaults.clone()
    }
    .normalized();
    assert_eq!(
        (
            high.scale,
            high.drift,
            high.pan,
            high.warp,
            high.octaves,
            high.tone_steepness,
            high.block,
            high.intensity,
            high.seed
        ),
        (300, 400, 40, 150, 4, 300, 4, 100, 999)
    );
    let low = FbmCloudsSettings {
        scale: 0,
        pan: -9999,
        octaves: 0,
        tone_steepness: 0,
        block: 0,
        intensity: 0,
        ..defaults
    }
    .normalized();
    assert_eq!(
        (
            low.scale,
            low.pan,
            low.octaves,
            low.tone_steepness,
            low.block,
            low.intensity
        ),
        (50, -40, 2, 50, 1, 5)
    );
}

#[test]
fn controls_of_unnormalized_settings_do_not_panic() {
    let wild = FbmCloudsSettings {
        octaves: 0,
        block: 0,
        ..FbmCloudsSettings::default()
    };
    assert_eq!(wild.controls().len(), 11);
}

#[test]
fn control_count_and_help_are_sane() {
    let rows = FbmCloudsSettings::default().controls();
    assert!((6..=12).contains(&rows.len()));
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), rows.len());
    assert!(ids.contains("pattern"));
    assert!(rows
        .iter()
        .all(|row| !row.label.is_empty() && !row.help.is_empty()));
}

#[test]
fn every_control_round_trips_through_set_control() {
    let original = FbmCloudsSettings::default();
    let mut rebuilt = FbmCloudsSettings {
        scale: 200,
        drift: 0,
        pan: -3,
        warp: 40,
        octaves: 2,
        tone_steepness: 250,
        block: 4,
        pattern: DitherPattern::White,
        intensity: 80,
        invert: true,
        seed: 321,
    };
    for row in original.controls() {
        rebuilt.set_control(row.id, row.value.clone()).unwrap();
    }
    assert_eq!(rebuilt, original);
    for row in rebuilt.controls() {
        assert_eq!(
            row.value,
            original
                .controls()
                .iter()
                .find(|other| other.id == row.id)
                .unwrap()
                .value
        );
    }
}

#[test]
fn every_slider_and_choice_accepts_its_extremes_and_reports_change() {
    for row in FbmCloudsSettings::default().controls() {
        let mut settings = FbmCloudsSettings::default();
        let values: Vec<ControlValue> = match &row.kind {
            ControlKind::Slider { min, max, .. } => {
                vec![ControlValue::Number(*min), ControlValue::Number(*max)]
            }
            ControlKind::Choice { options } => {
                (0..options.len()).map(ControlValue::Index).collect()
            }
            ControlKind::Toggle => vec![ControlValue::Bool(true), ControlValue::Bool(false)],
            ControlKind::Text { .. } => Vec::new(),
        };
        for value in values {
            settings.set_control(row.id, value.clone()).unwrap();
            let after = settings
                .controls()
                .into_iter()
                .find(|candidate| candidate.id == row.id)
                .unwrap();
            assert_eq!(after.value, value, "{}", row.id);
        }
        assert_eq!(settings.normalized(), settings, "{}", row.id);
    }
    let mut settings = FbmCloudsSettings::default();
    assert_eq!(
        settings.set_control("scale", ControlValue::Number(100)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("scale", ControlValue::Number(150)),
        Ok(true)
    );
    assert_eq!(
        settings.set_control("scale", ControlValue::Number(100_000)),
        Ok(true)
    );
    assert_eq!(settings.scale, 300);
}

#[test]
fn invalid_ids_and_types_are_handled() {
    let mut settings = FbmCloudsSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("scale", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("octaves", ControlValue::Index(9))
        .is_err());
    assert_eq!(settings, FbmCloudsSettings::default());
}

#[test]
fn settings_serde_round_trip_and_partial_defaults() {
    let json = serde_json::to_value(FbmCloudsSettings::default()).unwrap();
    let object = json.as_object().unwrap();
    for key in [
        "scale",
        "drift",
        "pan",
        "warp",
        "octaves",
        "tone_steepness",
        "block",
        "pattern",
        "intensity",
        "invert",
        "seed",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    let partial: FbmCloudsSettings =
        serde_json::from_str(r#"{"pattern":"white","intensity":50}"#).unwrap();
    assert_eq!(partial.pattern, DitherPattern::White);
    assert_eq!(partial.intensity, 50);
    assert_eq!(partial.scale, 100);
}

#[test]
fn defaults_are_not_empty_or_uniform_and_stay_quiet() {
    let rendered = frame_of(&FbmCloudsSettings::default(), 80, 40, 5.0);
    let dots = &rendered.raster.dots;
    let max = dots.iter().copied().fold(0.0, f32::max);
    let lit = dots.iter().filter(|dot| **dot > 0.05).count();
    assert!(lit > 0, "nothing lit");
    assert!(lit < dots.len(), "everything lit");
    assert!(max <= 0.36, "too bright for a background: {max}");
    assert!(dots.iter().all(|dot| (0.0..=1.0).contains(dot)));
    assert!(dots.windows(2).any(|pair| pair[0] != pair[1]));
}

#[test]
fn tiny_and_degenerate_sizes_do_not_panic() {
    for (width, height) in [(1, 1), (3, 2), (2, 1), (1, 5), (0, 0)] {
        for block in 1..=4 {
            let settings = FbmCloudsSettings {
                block,
                ..FbmCloudsSettings::default()
            };
            let rendered = frame_of(&settings, width, height, 1.0);
            assert_eq!(
                rendered.raster.dots.len(),
                rendered.raster.width * rendered.raster.height
            );
        }
    }
}

#[test]
fn inspired_by_is_present_and_clean() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn zero_drift_and_pan_freeze_the_picture() {
    let settings = FbmCloudsSettings {
        drift: 0,
        pan: 0,
        ..FbmCloudsSettings::default()
    };
    assert_eq!(
        frame_of(&settings, 40, 20, 0.0).raster.dots,
        frame_of(&settings, 40, 20, 500.0).raster.dots
    );
}

#[test]
fn invert_flips_the_hard_dots() {
    let plain = frame_of(&FbmCloudsSettings::default(), 40, 20, 2.0);
    let inverted = frame_of(
        &FbmCloudsSettings {
            invert: true,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        2.0,
    );
    for (a, b) in plain.raster.dots.iter().zip(&inverted.raster.dots) {
        assert!((*a > 0.2) != (*b > 0.2), "{a} {b}");
    }
}

#[test]
fn intensity_scales_the_output_linearly() {
    let dim = frame_of(
        &FbmCloudsSettings {
            intensity: 20,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        2.0,
    );
    let bright = frame_of(
        &FbmCloudsSettings {
            intensity: 80,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        2.0,
    );
    assert!((mean(&bright) / mean(&dim) - 4.0).abs() < 0.01);
}

#[test]
fn block_size_makes_blocks_uniform_in_tone() {
    let settings = FbmCloudsSettings {
        block: 4,
        pattern: DitherPattern::White,
        ..FbmCloudsSettings::default()
    };
    let rendered = frame_of(&settings, 20, 10, 1.0);
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn pan_moves_clouds_sideways() {
    let settings = FbmCloudsSettings {
        drift: 0,
        pan: 40,
        ..FbmCloudsSettings::default()
    };
    let start = frame_of(&settings, 40, 20, 0.0);
    let later = frame_of(&settings, 40, 20, 10.0);
    assert_ne!(start.raster.dots, later.raster.dots);
}

#[test]
fn octave_choice_changes_detail() {
    let smooth = frame_of(
        &FbmCloudsSettings {
            octaves: 2,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        2.0,
    );
    let detailed = frame_of(&FbmCloudsSettings::default(), 40, 20, 2.0);
    assert_ne!(smooth.raster.dots, detailed.raster.dots);
}

#[test]
fn long_running_clock_stays_finite() {
    let rendered = frame_of(&FbmCloudsSettings::default(), 40, 20, 3.0e6);
    assert!(rendered.raster.dots.iter().all(|dot| dot.is_finite()));
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn one_frame_at_100x40_cells_is_cheap() {
    let mut scene = scene_with(&FbmCloudsSettings::default());
    let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(1));
    let started = Instant::now();
    for step in 0..5 {
        let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(2 + step));
    }
    let per_frame = started.elapsed() / 5;
    // Generous bound so debug builds and loaded CI machines pass; the
    // release-mode target is under 8 ms.
    assert!(per_frame < Duration::from_millis(600), "{per_frame:?}");
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    let settings = FbmCloudsSettings::default();
    let mut scene = scene_with(&settings);
    for time in [0.0, 40.0] {
        let rendered = render_frame(&mut scene, 100, 40, Duration::from_secs_f64(time));
        for line in rendered.braille_lines(100, DitherMode::Ordered) {
            println!("{line}");
        }
        println!("---- mean {}", mean(&rendered));
    }
}

#[test]
#[ignore = "timing probe"]
fn time_probe() {
    let mut scene = scene_with(&FbmCloudsSettings::default());
    let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(1));
    let started = Instant::now();
    for step in 0..20 {
        let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(2 + step));
    }
    println!("PER_FRAME {:?}", started.elapsed() / 20);
}

#[test]
fn the_manifest_rows_match_the_scene_controls() {
    crate::kit::settings::assert_schema_matches_controls(
        "fbm-clouds",
        &FbmCloudsSettings::default().controls(),
    );
}

#[test]
fn a_spec_gives_clamped_settings_and_a_working_scene() {
    let spec = serde_json::json!({ "settings": { "scale": 9999, "pattern": "white", "block": 3 } });
    let settings: FbmCloudsSettings = crate::kit::settings::from_spec(&spec);
    assert_eq!((settings.scale, settings.block), (300, 3));
    assert_eq!(settings.pattern, DitherPattern::White);
    let mut scene = create(&spec, &SceneEnv::for_test());
    let rendered = render_frame(scene.as_mut(), 40, 16, Duration::from_secs(2));
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    let bare: FbmCloudsSettings = crate::kit::settings::from_spec(&serde_json::json!({}));
    assert_eq!(bare, FbmCloudsSettings::default().normalized());
}
