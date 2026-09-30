use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

fn scene_with(settings: &MachineScreenSettings) -> MachineScreenScene {
    MachineScreenScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn frame_at(settings: &MachineScreenSettings, width: u16, height: u16, time: f64) -> Rendered {
    let mut scene = scene_with(settings);
    render_frame(&mut scene, width, height, Duration::from_secs_f64(time))
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
fn same_time_gives_identical_raster() {
    let settings = MachineScreenSettings::default();
    let first = frame_at(&settings, 60, 30, 3.3);
    let second = frame_at(&settings, 60, 30, 3.3);
    assert_eq!(first.raster.dots, second.raster.dots);
    // A long-lived scene renders the same picture again: no hidden state.
    let mut scene = scene_with(&settings);
    let early = render_frame(&mut scene, 60, 30, Duration::from_secs(3));
    render_frame(&mut scene, 60, 30, Duration::from_secs(9));
    let again = render_frame(&mut scene, 60, 30, Duration::from_secs(3));
    assert_eq!(early.raster.dots, again.raster.dots);
}

#[test]
fn different_times_differ() {
    let settings = MachineScreenSettings::default();
    let first = frame_at(&settings, 60, 30, 1.0);
    let second = frame_at(&settings, 60, 30, 9.0);
    assert_ne!(first.raster.dots, second.raster.dots);
}

#[test]
fn different_seeds_differ() {
    let first = frame_at(&MachineScreenSettings::default(), 60, 30, 2.0);
    let second = frame_at(
        &MachineScreenSettings {
            seed: 4242,
            ..MachineScreenSettings::default()
        },
        60,
        30,
        2.0,
    );
    assert_ne!(first.raster.dots, second.raster.dots);
}

#[test]
fn normalized_clamps_out_of_range_values() {
    let defaults = MachineScreenSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let low = MachineScreenSettings {
        seed: 0,
        gradient_speed: 0,
        panel_size: 0,
        dither_scale: 0,
        marquee_speed: 0,
        brightness: 0,
        ..defaults.clone()
    }
    .normalized();
    assert_eq!(
        (
            low.gradient_speed,
            low.panel_size,
            low.dither_scale,
            low.marquee_speed,
            low.brightness
        ),
        (10, 30, 1, 0, 5)
    );
    let high = MachineScreenSettings {
        seed: 1_000_000,
        gradient_speed: 9999,
        panel_size: 9999,
        dither_scale: 99,
        marquee_speed: 99,
        brightness: 9999,
        ..defaults
    }
    .normalized();
    assert_eq!(
        (
            high.seed,
            high.gradient_speed,
            high.panel_size,
            high.dither_scale,
            high.marquee_speed,
            high.brightness
        ),
        (9999, 200, 90, 4, 12, 40)
    );
}

#[test]
fn serde_uses_snake_case_and_defaults_missing_keys() {
    let json = serde_json::to_value(MachineScreenSettings::default()).unwrap();
    let object = json.as_object().unwrap();
    for key in [
        "seed",
        "gradient_speed",
        "panel_size",
        "dither_scale",
        "marquee_speed",
        "brightness",
        "show_frame",
        "blinking_dots",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    let partial: MachineScreenSettings = serde_json::from_str(r#"{"panel_size":80}"#).unwrap();
    assert_eq!(partial.panel_size, 80);
    assert_eq!(partial.brightness, 20);
}

#[test]
fn controls_round_trip_for_every_id() {
    let mut settings = MachineScreenSettings::default();
    let rows = settings.controls();
    assert!((6..=12).contains(&rows.len()));
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), rows.len());
    assert!(rows
        .iter()
        .all(|row| !row.label.is_empty() && !row.help.is_empty()));
    for row in rows {
        let changed = match (&row.kind, &row.value) {
            (ControlKind::Slider { min, max, .. }, ControlValue::Number(number)) => {
                let target = if number == min { *max } else { *min };
                let changed = settings
                    .set_control(row.id, ControlValue::Number(target))
                    .unwrap();
                assert!(changed, "{}", row.id);
                ControlValue::Number(target)
            }
            (ControlKind::Toggle, ControlValue::Bool(on)) => {
                assert!(settings
                    .set_control(row.id, ControlValue::Bool(!on))
                    .unwrap());
                ControlValue::Bool(!on)
            }
            other => panic!("unexpected control kind {other:?}"),
        };
        let reread = settings
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id)
            .unwrap();
        assert_eq!(reread.value, changed, "{}", row.id);
        // Setting the same value again reports no change.
        assert!(
            !settings.set_control(row.id, changed).unwrap(),
            "{}",
            row.id
        );
    }
}

#[test]
fn set_control_clamps_and_rejects_wrong_types() {
    let mut settings = MachineScreenSettings::default();
    assert!(settings
        .set_control("panel_size", ControlValue::Number(1000))
        .unwrap());
    assert_eq!(settings.panel_size, 90);
    assert!(settings
        .set_control("brightness", ControlValue::Bool(true))
        .is_err());
}

#[test]
fn invalid_id_returns_false() {
    let mut settings = MachineScreenSettings::default();
    assert_eq!(
        settings.set_control("no_such_control", ControlValue::Number(3)),
        Ok(false)
    );
}

#[test]
fn defaults_render_non_uniform_and_quiet() {
    let rendered = frame_at(&MachineScreenSettings::default(), 100, 40, 2.0);
    let dots = &rendered.raster.dots;
    let total = dots.len();
    let count = lit(&rendered);
    assert!(count > total / 50, "too empty: {count}");
    assert!(count < total / 2, "too full: {count}");
    let max = dots.iter().copied().fold(0.0f32, f32::max);
    assert!(max > 0.0 && max <= 1.0, "not quiet: {max}");
    let mean = dots.iter().sum::<f32>() / total as f32;
    assert!(mean < 0.15, "mean {mean}");
}

#[test]
fn works_at_tiny_and_degenerate_sizes() {
    for (width, height) in [(1, 1), (3, 2), (2, 5), (4, 1), (7, 3)] {
        for settings in [
            MachineScreenSettings::default(),
            MachineScreenSettings {
                panel_size: 90,
                dither_scale: 4,
                ..MachineScreenSettings::default()
            },
            MachineScreenSettings {
                show_frame: false,
                blinking_dots: false,
                panel_size: 30,
                ..MachineScreenSettings::default()
            },
        ] {
            let rendered = frame_at(&settings, width, height, 1.7);
            assert_eq!(
                rendered.raster.dots.len(),
                usize::from(width) * 2 * usize::from(height) * 4
            );
        }
    }
    let mut scene = scene_with(&MachineScreenSettings::default());
    render_frame(&mut scene, 0, 0, Duration::ZERO);
}

#[test]
fn inspired_by_lists_the_source_without_tracking() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn render_overwrites_previous_frame_content() {
    let mut scene = scene_with(&MachineScreenSettings::default());
    let mut rendered = render_frame(&mut scene, 30, 15, Duration::from_secs(1));
    let expected = render_frame(&mut scene, 30, 15, Duration::from_secs(2));
    rendered.raster.dots.fill(0.9);
    let mut cell_colors = std::mem::take(&mut rendered.cell_colors);
    let mut frame = Frame {
        raster: &mut rendered.raster,
        cell_colors: &mut cell_colors,
        width: 30,
        height: 15,
        time: Duration::from_secs(2),
        wall: Duration::from_secs(2),
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    scene.render(&mut frame);
    assert_eq!(rendered.raster.dots, expected.raster.dots);
}

#[test]
fn panel_stays_inside_its_share_of_the_raster() {
    let settings = MachineScreenSettings {
        panel_size: 50,
        ..MachineScreenSettings::default()
    };
    let rendered = frame_at(&settings, 100, 40, 1.0);
    let (width, height) = (rendered.raster.width, rendered.raster.height);
    for y in 0..height {
        for x in 0..width {
            let outside = x < width / 4
                || x >= width - width / 4
                || y < height / 4
                || y >= height - height / 4;
            if outside {
                assert_eq!(rendered.raster.dots[y * width + x], 0.0, "({x},{y})");
            }
        }
    }
}

#[test]
fn marquee_scrolls_with_speed_and_holds_at_zero() {
    let moving = MachineScreenSettings {
        blinking_dots: false,
        ..MachineScreenSettings::default()
    };
    let band_rows = |rendered: &Rendered| {
        let width = rendered.raster.width;
        let scene = scene_with(&moving);
        let layout = scene.layout(width, rendered.raster.height);
        let band = layout.marquee.unwrap();
        (band.y..band.y + band.height)
            .map(|y| {
                rendered.raster.dots[y * width + band.x..y * width + band.x + band.width].to_vec()
            })
            .collect::<Vec<_>>()
    };
    let start = band_rows(&frame_at(&moving, 100, 40, 0.0));
    let later = band_rows(&frame_at(&moving, 100, 40, 2.0));
    assert_ne!(start, later);
    let still = MachineScreenSettings {
        marquee_speed: 0,
        ..moving
    };
    assert_eq!(
        band_rows(&frame_at(&still, 100, 40, 0.0)),
        band_rows(&frame_at(&still, 100, 40, 2.0))
    );
}

#[test]
fn blinking_dots_toggle_changes_only_the_gradient_area() {
    let on = frame_at(&MachineScreenSettings::default(), 100, 40, 0.1);
    let off = frame_at(
        &MachineScreenSettings {
            blinking_dots: false,
            ..MachineScreenSettings::default()
        },
        100,
        40,
        0.1,
    );
    assert_ne!(on.raster.dots, off.raster.dots);
}

#[test]
fn frame_toggle_removes_the_outline() {
    let framed = frame_at(&MachineScreenSettings::default(), 100, 40, 1.0);
    let bare = frame_at(
        &MachineScreenSettings {
            show_frame: false,
            ..MachineScreenSettings::default()
        },
        100,
        40,
        1.0,
    );
    assert!(lit(&framed) != lit(&bare));
}

#[test]
fn dither_scale_makes_coarser_texture() {
    let fine = frame_at(&MachineScreenSettings::default(), 100, 40, 1.0);
    let coarse = frame_at(
        &MachineScreenSettings {
            dither_scale: 4,
            ..MachineScreenSettings::default()
        },
        100,
        40,
        1.0,
    );
    assert_ne!(fine.raster.dots, coarse.raster.dots);
}

#[test]
fn frame_is_cheap_at_default_size() {
    let settings = MachineScreenSettings::default();
    let mut scene = scene_with(&settings);
    render_frame(&mut scene, 100, 40, Duration::ZERO);
    let start = Instant::now();
    for step in 0..50 {
        render_frame(&mut scene, 100, 40, Duration::from_millis(step * 80));
    }
    // Includes allocating the raster each call; generous bound for debug builds.
    assert!(start.elapsed() < Duration::from_millis(50 * 40));
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    let mut scene = scene_with(&MachineScreenSettings::default());
    let rendered = render_frame(&mut scene, 80, 40, Duration::from_secs(5));
    for line in rendered.braille_lines(100, DitherMode::Ordered) {
        println!("{line}");
    }
}
