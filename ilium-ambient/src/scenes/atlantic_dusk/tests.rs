use super::settings::{DitherStyle, TimeOfDay};
use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::time::{Duration, Instant};

fn scene_with(settings: &AtlanticDuskSettings) -> AtlanticDuskScene {
    AtlanticDuskScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn pinned(time_of_day: TimeOfDay) -> AtlanticDuskSettings {
    AtlanticDuskSettings {
        time_of_day,
        ..AtlanticDuskSettings::default()
    }
}

fn frame_at(settings: &AtlanticDuskSettings, seconds: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, 100, 40, Duration::from_secs_f64(seconds))
}

fn mean(rendered: &Rendered, rows: std::ops::Range<usize>) -> f32 {
    let width = rendered.raster.width;
    let slice = &rendered.raster.dots[rows.start * width..rows.end * width];
    slice.iter().sum::<f32>() / slice.len() as f32
}

/// Dots at full brightness in the sky rows of a 100x40 frame.
fn bright_sky_dots(rendered: &Rendered) -> usize {
    let width = rendered.raster.width;
    rendered.raster.dots[..width * 67]
        .iter()
        .filter(|dot| **dot >= 0.99)
        .count()
}

#[test]
fn same_time_gives_identical_rasters() {
    let settings = AtlanticDuskSettings::default();
    let first = frame_at(&settings, 37.5);
    let second = frame_at(&settings, 37.5);
    assert_eq!(first.raster.dots, second.raster.dots);
}

#[test]
fn different_times_differ() {
    let settings = AtlanticDuskSettings::default();
    assert_ne!(
        frame_at(&settings, 1.0).raster.dots,
        frame_at(&settings, 9.0).raster.dots
    );
}

#[test]
fn normalized_clamps_out_of_range_values() {
    let defaults = AtlanticDuskSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let low = AtlanticDuskSettings {
        day_length_seconds: 0,
        cloud_coverage: 0,
        wave_scale: 0,
        contrast: 0,
        ..AtlanticDuskSettings::default()
    }
    .normalized();
    assert_eq!(
        (low.day_length_seconds, low.wave_scale, low.contrast),
        (30, 50, 20)
    );
    let high = AtlanticDuskSettings {
        day_length_seconds: 99_999,
        cloud_coverage: 999,
        cloud_speed: 999,
        wave_speed: 999,
        wave_scale: 999,
        star_density: 999,
        contrast: 999,
        seed: 99_999,
        ..AtlanticDuskSettings::default()
    }
    .normalized();
    assert_eq!(
        (
            high.day_length_seconds,
            high.cloud_coverage,
            high.cloud_speed,
            high.wave_speed,
            high.wave_scale,
            high.star_density,
            high.contrast,
            high.seed
        ),
        (600, 100, 300, 300, 200, 80, 100, 999)
    );
}

#[test]
fn controls_round_trip_for_every_row() {
    let settings = AtlanticDuskSettings::default();
    let rows = settings.controls();
    assert!((6..=12).contains(&rows.len()));
    for row in &rows {
        assert!(!row.help.is_empty(), "{} has no help", row.id);
        let mut edited = settings.clone();
        assert_eq!(
            edited.set_control(row.id, row.value.clone()),
            Ok(false),
            "{}",
            row.id
        );
        let changed = match (&row.kind, &row.value) {
            (ControlKind::Slider { min, max, .. }, ControlValue::Number(value)) => {
                ControlValue::Number(if value == min { *max } else { *min })
            }
            (ControlKind::Choice { options }, ControlValue::Index(index)) => {
                ControlValue::Index((index + 1) % options.len())
            }
            (ControlKind::Toggle, ControlValue::Bool(on)) => ControlValue::Bool(!on),
            other => panic!("unexpected control kind {other:?}"),
        };
        assert_eq!(
            edited.set_control(row.id, changed.clone()),
            Ok(true),
            "{}",
            row.id
        );
        let reread = edited
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id)
            .map(|candidate| candidate.value);
        assert_eq!(reread, Some(changed), "{}", row.id);
        assert_eq!(edited, edited.normalized(), "{}", row.id);
    }
}

#[test]
fn unknown_id_and_wrong_type_are_handled() {
    let mut settings = AtlanticDuskSettings::default();
    assert_eq!(
        settings.set_control("nope", ControlValue::Number(1)),
        Ok(false)
    );
    assert!(settings
        .set_control("contrast", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("dither", ControlValue::Index(99))
        .is_err());
    assert_eq!(
        settings.set_control("contrast", ControlValue::Number(5000)),
        Ok(true)
    );
    assert_eq!(settings.contrast, 100);
}

#[test]
fn defaults_are_drawn_and_not_uniform() {
    let rendered = frame_at(&AtlanticDuskSettings::default(), 5.0);
    let first = rendered.raster.dots[0];
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    assert!(rendered
        .raster
        .dots
        .iter()
        .any(|dot| (*dot - first).abs() > 0.05));
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|dot| (0.0..=1.0).contains(dot)));
}

#[test]
fn tiny_sizes_do_not_panic() {
    for time_of_day in [TimeOfDay::Cycle, TimeOfDay::Dusk, TimeOfDay::Night] {
        for dither in [DitherStyle::Host, DitherStyle::Bayer4, DitherStyle::Bayer8] {
            let mut scene = scene_with(&AtlanticDuskSettings {
                time_of_day,
                dither,
                ..AtlanticDuskSettings::default()
            });
            for (width, height) in [(1, 1), (3, 2), (1, 5), (7, 1)] {
                let rendered = render_frame(&mut scene, width, height, Duration::from_secs(4));
                assert_eq!(
                    rendered.raster.dots.len(),
                    usize::from(width) * usize::from(height) * 8
                );
            }
        }
    }
}

#[test]
fn inspired_by_lists_the_devlog_pages() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn scene_reports_a_sane_cadence_and_no_status() {
    let scene = scene_with(&AtlanticDuskSettings::default());
    assert!((1..=30).contains(&scene.frames_per_second()));
    assert!(!scene.uses_cell_colors());
    assert_eq!(scene.status(), None);
}

#[test]
fn night_sky_is_darker_than_day_sky_and_has_stars() {
    let day = frame_at(&pinned(TimeOfDay::Noon), 0.0);
    let night = frame_at(&pinned(TimeOfDay::Night), 0.0);
    assert!(mean(&night, 0..64) < mean(&day, 0..64) * 0.5);
    let stars = bright_sky_dots(&night);
    assert!(stars > 5, "only {stars} star dots");
    let starless = frame_at(
        &AtlanticDuskSettings {
            star_density: 0,
            ..pinned(TimeOfDay::Night)
        },
        0.0,
    );
    assert!(bright_sky_dots(&starless) < stars);
}

#[test]
fn fixed_gradient_dots_do_not_move_between_frames() {
    // Cloud-free noon sky: only the sea lines move.
    let settings = AtlanticDuskSettings {
        cloud_coverage: 0,
        ..pinned(TimeOfDay::Noon)
    };
    let first = frame_at(&settings, 3.0);
    let second = frame_at(&settings, 40.0);
    let width = first.raster.width;
    assert_eq!(
        first.raster.dots[..width * 60],
        second.raster.dots[..width * 60]
    );
    assert_ne!(
        first.raster.dots[width * 100..],
        second.raster.dots[width * 100..]
    );
}

#[test]
fn own_dither_modes_are_binary_and_fixed_in_the_cloud_free_sky() {
    for dither in [DitherStyle::Bayer4, DitherStyle::Bayer8] {
        let settings = AtlanticDuskSettings {
            cloud_coverage: 0,
            dither,
            ..pinned(TimeOfDay::Noon)
        };
        let first = frame_at(&settings, 3.0);
        let second = frame_at(&settings, 60.0);
        assert!(first
            .raster
            .dots
            .iter()
            .all(|dot| *dot == 0.0 || *dot == 1.0));
        let width = first.raster.width;
        assert_eq!(
            first.raster.dots[..width * 60],
            second.raster.dots[..width * 60]
        );
    }
}

#[test]
fn clouds_are_lighter_by_day_and_darker_at_dusk() {
    // The cloud layer is the difference between full and no coverage.
    for (time_of_day, lighter) in [(TimeOfDay::Noon, true), (TimeOfDay::Dusk, false)] {
        let with_clouds = frame_at(
            &AtlanticDuskSettings {
                cloud_coverage: 100,
                ..pinned(time_of_day)
            },
            2.0,
        );
        let without = frame_at(
            &AtlanticDuskSettings {
                cloud_coverage: 0,
                ..pinned(time_of_day)
            },
            2.0,
        );
        let width = with_clouds.raster.width;
        let range = width * 20..width * 40;
        let delta: f32 = with_clouds.raster.dots[range.clone()]
            .iter()
            .zip(&without.raster.dots[range])
            .map(|(cloudy, clear)| cloudy - clear)
            .sum();
        if lighter {
            assert!(delta > 0.0, "day clouds should brighten, delta {delta}");
        } else {
            assert!(delta < 0.0, "dusk clouds should darken, delta {delta}");
        }
    }
}

#[test]
fn sun_is_drawn_at_dusk() {
    let dusk = frame_at(&pinned(TimeOfDay::Dusk), 0.0);
    assert!(
        bright_sky_dots(&dusk) > 40,
        "sun disc missing: {}",
        bright_sky_dots(&dusk)
    );
}

#[test]
fn pinned_time_ignores_the_clock_but_cycle_advances() {
    let still = AtlanticDuskSettings {
        cloud_coverage: 0,
        wave_speed: 0,
        ..pinned(TimeOfDay::Dusk)
    };
    assert_eq!(
        frame_at(&still, 1.0).raster.dots,
        frame_at(&still, 50.0).raster.dots
    );
    let cycling = AtlanticDuskSettings {
        cloud_coverage: 0,
        wave_speed: 0,
        ..AtlanticDuskSettings::default()
    };
    assert_ne!(
        frame_at(&cycling, 1.0).raster.dots,
        frame_at(&cycling, 50.0).raster.dots
    );
}

#[test]
fn seed_changes_the_clouds() {
    let base = AtlanticDuskSettings {
        cloud_coverage: 70,
        ..pinned(TimeOfDay::Noon)
    };
    let other = AtlanticDuskSettings {
        seed: 7,
        ..base.clone()
    };
    assert_ne!(
        frame_at(&base, 0.0).raster.dots,
        frame_at(&other, 0.0).raster.dots
    );
}

#[test]
fn contrast_scales_the_picture() {
    let quiet = frame_at(
        &AtlanticDuskSettings {
            contrast: 20,
            ..pinned(TimeOfDay::Noon)
        },
        0.0,
    );
    let loud = frame_at(
        &AtlanticDuskSettings {
            contrast: 100,
            ..pinned(TimeOfDay::Noon)
        },
        0.0,
    );
    assert!(mean(&quiet, 0..160) < mean(&loud, 0..160));
}

#[test]
fn full_day_cycle_stays_in_range_at_many_times() {
    let mut scene = scene_with(&AtlanticDuskSettings::default());
    for step in 0..48 {
        let rendered = render_frame(&mut scene, 40, 20, Duration::from_secs(step * 5));
        assert!(rendered
            .raster
            .dots
            .iter()
            .all(|dot| dot.is_finite() && (0.0..=1.0).contains(dot)));
    }
}

#[test]
fn one_frame_at_default_size_is_cheap() {
    let mut scene = scene_with(&AtlanticDuskSettings::default());
    let started = Instant::now();
    for step in 0..10 {
        let _ = render_frame(&mut scene, 100, 40, Duration::from_secs(step));
    }
    // Generous bound so debug builds and loaded machines pass; release is ~1 ms.
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
#[ignore = "prints the picture for visual inspection"]
fn print_pictures() {
    let mut lines = Vec::new();
    for time_of_day in [
        TimeOfDay::Dawn,
        TimeOfDay::Noon,
        TimeOfDay::Dusk,
        TimeOfDay::Night,
    ] {
        let settings = AtlanticDuskSettings {
            time_of_day,
            ..AtlanticDuskSettings::default()
        };
        let mut scene = scene_with(&settings);
        let rendered = render_frame(&mut scene, 80, 24, Duration::from_secs(20));
        lines.push(format!("== {time_of_day:?}"));
        lines.extend(rendered.braille_lines(100, DitherMode::Ordered));
    }
    println!("{}", lines.join("\n"));
}
