use super::settings::{ClockPosition, HourFormat};
use super::*;
use crate::control::{Control, ControlKind, ControlValue};
use crate::debug::render_frame;
use std::time::{Duration, Instant};

fn scene_with(settings: &CubeClockSettings) -> CubeClockScene {
    CubeClockScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn dots(settings: &CubeClockSettings, width: u16, height: u16, time: f64) -> Vec<f32> {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, width, height, Duration::from_secs_f64(time))
        .raster
        .dots
}

#[test]
fn same_time_gives_identical_raster() {
    let settings = CubeClockSettings::default();
    assert_eq!(dots(&settings, 60, 30, 12.3), dots(&settings, 60, 30, 12.3));
}

#[test]
fn different_times_differ() {
    let settings = CubeClockSettings::default();
    assert_ne!(dots(&settings, 60, 30, 1.0), dots(&settings, 60, 30, 4.0));
}

#[test]
fn defaults_survive_normalization_and_wild_values_are_clamped() {
    let defaults = CubeClockSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let clean = CubeClockSettings {
        cube_size: -5,
        rotation_rate: 9999,
        face_shading: 500,
        brightness: 0,
        utc_offset_hours: 99,
        ..CubeClockSettings::default()
    }
    .normalized();
    assert_eq!(
        (
            clean.cube_size,
            clean.rotation_rate,
            clean.face_shading,
            clean.brightness,
            clean.utc_offset_hours
        ),
        (20, 300, 100, 30, 14)
    );
}

#[test]
fn control_count_and_kinds_fit_the_contract() {
    let rows = CubeClockSettings::default().controls();
    assert!((6..=12).contains(&rows.len()));
    for row in &rows {
        assert!(!row.help.is_empty(), "{} needs help text", row.id);
    }
}

fn changed_value(row: &Control) -> ControlValue {
    row.stepped(1).expect("every row here is steppable")
}

#[test]
fn every_control_round_trips_through_set_control() {
    let base = CubeClockSettings::default();
    for row in base.controls() {
        let mut settings = base.clone();
        assert_eq!(
            settings.set_control(row.id, row.value.clone()),
            Ok(false),
            "{} unchanged",
            row.id
        );
        let mut next = changed_value(&row);
        if next == row.value {
            next = row.stepped(-1).expect("steppable");
        }
        assert_eq!(
            settings.set_control(row.id, next.clone()),
            Ok(true),
            "{}",
            row.id
        );
        let after = settings
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id)
            .expect("row persists");
        assert_eq!(after.value, next, "{}", row.id);
        assert_eq!(settings.normalized(), settings);
    }
}

#[test]
fn out_of_range_edits_clamp_and_wrong_types_report_errors() {
    let mut settings = CubeClockSettings::default();
    assert_eq!(
        settings.set_control("cube_size", ControlValue::Number(1000)),
        Ok(true)
    );
    assert_eq!(settings.cube_size, 60);
    assert!(settings
        .set_control("cube_size", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("hour_format", ControlValue::Index(9))
        .is_err());
}

#[test]
fn unknown_control_id_is_ignored() {
    let mut settings = CubeClockSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert_eq!(settings, CubeClockSettings::default());
}

#[test]
fn sliders_use_integer_ranges_that_contain_their_defaults() {
    for row in CubeClockSettings::default().controls() {
        if let (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) =
            (&row.kind, &row.value)
        {
            assert!(min <= value && value <= max, "{}", row.id);
        }
    }
}

#[test]
fn default_raster_is_non_empty_dim_and_not_uniform() {
    let picture = dots(&CubeClockSettings::default(), 100, 40, 3.0);
    let lit = picture.iter().filter(|dot| **dot > 0.0).count();
    assert!(lit > 100, "lit {lit}");
    assert!(lit < picture.len() / 3, "background art must stay sparse");
    let peak = picture.iter().copied().fold(0.0, f32::max);
    assert!(peak <= 0.6, "quiet by default, peak {peak}");
    assert!(picture.iter().all(|dot| (0.0..=1.0).contains(dot)));
}

#[test]
fn tiny_sizes_do_not_panic() {
    for settings in [
        CubeClockSettings::default(),
        CubeClockSettings {
            clock_position: ClockPosition::Inside,
            show_seconds: true,
            hour_format: HourFormat::Twelve,
            ..CubeClockSettings::default()
        },
    ] {
        for (width, height) in [(0, 0), (1, 1), (3, 2), (1, 40), (200, 1)] {
            let mut scene = scene_with(&settings);
            render_frame(&mut scene, width, height, Duration::from_secs(5));
        }
    }
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY.iter().all(|url| url.starts_with("https://")));
}

#[test]
fn frozen_rotation_without_tick_is_static() {
    let settings = CubeClockSettings {
        rotation_rate: 0,
        second_tick: false,
        dust: false,
        minute_ring: false,
        clock_position: ClockPosition::Off,
        ..CubeClockSettings::default()
    };
    assert_eq!(dots(&settings, 60, 30, 1.0), dots(&settings, 60, 30, 9.0));
}

#[test]
fn wireframe_mode_is_dimmer_than_shaded() {
    let base = CubeClockSettings {
        dust: false,
        minute_ring: false,
        clock_position: ClockPosition::Off,
        ..CubeClockSettings::default()
    };
    let wire = CubeClockSettings {
        face_shading: 0,
        ..base.clone()
    };
    let sum = |picture: Vec<f32>| picture.iter().sum::<f32>();
    assert!(sum(dots(&base, 100, 40, 2.0)) > sum(dots(&wire, 100, 40, 2.0)));
}

#[test]
fn brightness_scales_the_picture() {
    let dim = CubeClockSettings {
        brightness: 30,
        ..CubeClockSettings::default()
    };
    let bright = CubeClockSettings {
        brightness: 150,
        ..CubeClockSettings::default()
    };
    let sum = |picture: Vec<f32>| picture.iter().sum::<f32>();
    assert!(sum(dots(&bright, 60, 30, 2.0)) > 2.0 * sum(dots(&dim, 60, 30, 2.0)));
}

#[test]
fn hidden_readout_removes_the_digits_area() {
    let with_clock = CubeClockSettings::default();
    let without = CubeClockSettings {
        clock_position: ClockPosition::Off,
        ..with_clock.clone()
    };
    assert!(
        dots(&with_clock, 100, 40, 2.0).iter().sum::<f32>()
            > dots(&without, 100, 40, 2.0).iter().sum::<f32>()
    );
}

#[test]
fn clock_time_applies_zone_offsets_and_wraps_midnight() {
    // 1970-01-02 00:30:15.25 UTC
    let unix = 86_400.0 + 30.0 * 60.0 + 15.25;
    let utc = ClockTime::from_unix(unix, 0);
    assert_eq!((utc.hour, utc.minute, utc.second), (0, 30, 15));
    assert!((utc.fraction - 0.25).abs() < 1e-4);
    let east = ClockTime::from_unix(unix, 2);
    assert_eq!((east.hour, east.minute), (2, 30));
    let west = ClockTime::from_unix(unix, -1);
    assert_eq!((west.hour, west.minute), (23, 30));
}

#[test]
fn readout_formats_twelve_and_twenty_four_hours() {
    let clock = ClockTime {
        hour: 15,
        minute: 7,
        second: 9,
        fraction: 0.0,
    };
    let scene = scene_with(&CubeClockSettings::default());
    assert_eq!(scene.readout(&clock), (vec![1, 5, font::COLON, 0, 7], 0));
    let twelve = scene_with(&CubeClockSettings {
        hour_format: HourFormat::Twelve,
        show_seconds: true,
        ..CubeClockSettings::default()
    });
    let morning = ClockTime {
        hour: 0,
        minute: 5,
        second: 45,
        fraction: 0.0,
    };
    assert_eq!(
        twelve.readout(&morning),
        (vec![1, 2, font::COLON, 0, 5, font::COLON, 4, 5], 1)
    );
    let afternoon = ClockTime { hour: 15, ..clock };
    assert_eq!(
        twelve.readout(&afternoon),
        (vec![3, font::COLON, 0, 7, font::COLON, 0, 9], 2)
    );
}

#[test]
fn the_second_tick_moves_the_cube_only_within_a_second() {
    let scene = scene_with(&CubeClockSettings::default());
    let early = ClockTime {
        hour: 1,
        minute: 2,
        second: 3,
        fraction: 0.0,
    };
    let late = ClockTime {
        fraction: 0.6,
        ..early
    };
    let next = ClockTime {
        second: 4,
        fraction: 0.0,
        ..early
    };
    let (yaw_early, ..) = scene.angles(0.0, &early);
    let (yaw_late, ..) = scene.angles(0.0, &late);
    let (yaw_next, ..) = scene.angles(0.0, &next);
    assert!(yaw_late != yaw_early);
    // The eased tick has fully landed by the end of the second's first quarter.
    assert!((yaw_late - yaw_next).abs() < 1e-3);
}

#[test]
fn ring_marks_the_current_second() {
    let settings = CubeClockSettings {
        dust: false,
        face_shading: 0,
        cube_size: 20,
        clock_position: ClockPosition::Off,
        ..CubeClockSettings::default()
    };
    let with_ring = dots(&settings, 100, 40, 2.0);
    let no_ring = dots(
        &CubeClockSettings {
            minute_ring: false,
            ..settings
        },
        100,
        40,
        2.0,
    );
    let extra = with_ring
        .iter()
        .zip(&no_ring)
        .filter(|(a, b)| a > b)
        .count();
    assert!(extra >= 60, "{extra}");
}

#[test]
fn glyph_table_draws_recognisable_digits() {
    let lit = |glyph| {
        (0..5)
            .flat_map(|row| (0..3).map(move |column| (column, row)))
            .filter(|(column, row)| font::pixel(glyph, *column, *row))
            .count()
    };
    assert_eq!(lit(8), 13);
    assert_eq!(lit(1), 8);
    assert_eq!(lit(font::COLON), 2);
    assert!(!font::pixel(99, 0, 0));
}

#[test]
fn a_full_size_frame_is_cheap() {
    let mut scene = scene_with(&CubeClockSettings {
        show_seconds: true,
        ..CubeClockSettings::default()
    });
    render_frame(&mut scene, 100, 40, Duration::from_secs(1));
    let start = Instant::now();
    for step in 0..20 {
        render_frame(&mut scene, 100, 40, Duration::from_millis(step * 70));
    }
    let per_frame = start.elapsed() / 20;
    // Generous bound so debug builds on a busy machine stay green.
    assert!(per_frame < Duration::from_millis(60), "{per_frame:?}");
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    use crate::raster::DitherMode;
    for settings in [
        CubeClockSettings::default(),
        CubeClockSettings {
            clock_position: ClockPosition::Inside,
            ..CubeClockSettings::default()
        },
    ] {
        for time in [0.5, 6.0] {
            let mut scene = scene_with(&settings);
            let picture = render_frame(&mut scene, 100, 40, Duration::from_secs_f64(time));
            for line in picture.braille_lines(200, DitherMode::Ordered) {
                println!("{line}");
            }
            println!("----");
        }
    }
}
