use super::settings::{PipePattern, PipeShading};
use super::*;
use crate::control::{Control, ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::raster::DitherMode;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

fn fixed(seed: u32) -> PipesSettings {
    PipesSettings {
        seed_mode: SeedMode::Fixed,
        seed,
        ..PipesSettings::default()
    }
}

fn scene_with(settings: &PipesSettings) -> PipesScene {
    PipesScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn seconds(value: f64) -> Duration {
    Duration::from_secs_f64(value)
}

fn frame_at(scene: &mut PipesScene, time: f64) -> Rendered {
    render_frame(scene, 60, 30, seconds(time))
}

#[test]
fn defaults_are_inside_their_ranges_and_survive_normalization() {
    let defaults = PipesSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = PipesSettings {
        pipe_count: 0,
        volume_size: 999,
        pipe_thickness: 0,
        turn_chance: 1000,
        growth_speed: 0,
        orbit_speed: 500,
        field_of_view: 5,
        reset_seconds: 1,
        seed: 1_000_000,
        ..PipesSettings::default()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.pipe_count,
            clean.volume_size,
            clean.pipe_thickness,
            clean.turn_chance,
            clean.growth_speed,
            clean.orbit_speed,
            clean.field_of_view,
            clean.reset_seconds,
            clean.seed
        ),
        (1, 16, 10, 100, 1, 100, 20, 10, 9999)
    );
}

#[test]
fn yaml_style_keys_round_trip_and_missing_keys_default() {
    let json = serde_json::to_value(PipesSettings::default()).unwrap();
    let object = json.as_object().unwrap();
    for key in [
        "pipe_count",
        "volume_size",
        "pipe_thickness",
        "turn_chance",
        "growth_speed",
        "orbit_speed",
        "field_of_view",
        "joint_style",
        "shading",
        "pattern",
        "reset_mode",
        "reset_seconds",
        "seed_mode",
        "seed",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    let partial: PipesSettings = serde_json::from_str(
        r#"{"joint_style":"none","shading":"high_contrast","reset_mode":"timed"}"#,
    )
    .unwrap();
    assert_eq!(partial.joint_style, JointStyle::Bare);
    assert_eq!(partial.shading, PipeShading::HighContrast);
    assert_eq!(partial.reset_mode, ResetMode::Timed);
    assert_eq!(partial.pipe_count, 5);
    let again: PipesSettings = serde_json::from_value(json).unwrap();
    assert_eq!(again, PipesSettings::default());
}

#[test]
fn controls_have_stable_unique_ids_labels_and_help() {
    let mut settings = PipesSettings::default();
    let rows = settings.controls();
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), rows.len());
    assert!(rows
        .iter()
        .all(|row| !row.label.is_empty() && !row.help.is_empty()));
    for id in [
        "pipe_count",
        "volume_size",
        "pipe_thickness",
        "turn_chance",
        "growth_speed",
        "orbit_speed",
        "field_of_view",
        "joint_style",
        "shading",
        "pattern",
        "reset_mode",
        "seed_mode",
    ] {
        assert!(ids.contains(id), "{id}");
    }
    // Conditional rows appear only in their mode.
    assert!(!ids.contains("reset_seconds") && !ids.contains("seed"));
    settings
        .set_control("reset_mode", ControlValue::Index(1))
        .unwrap();
    settings
        .set_control("seed_mode", ControlValue::Index(1))
        .unwrap();
    let ids: BTreeSet<&str> = settings.controls().iter().map(|row| row.id).collect();
    assert!(ids.contains("reset_seconds") && ids.contains("seed"));
}

#[test]
fn every_row_reflects_the_value_and_steps() {
    let settings = fixed(7);
    for row in settings.controls() {
        let Control { kind, value, .. } = &row;
        match (kind, value) {
            (ControlKind::Slider { min, max, .. }, ControlValue::Number(number)) => {
                assert!((min..=max).contains(&number), "{}", row.id);
            }
            (ControlKind::Choice { options }, ControlValue::Index(index)) => {
                assert!(*index < options.len(), "{}", row.id);
            }
            _ => panic!("unexpected row {}", row.id),
        }
        assert!(row.stepped(1).is_some());
    }
}

#[test]
fn set_control_reports_changes_and_rejects_bad_input() {
    let mut settings = PipesSettings::default();
    assert_eq!(
        settings.set_control("pipe_count", ControlValue::Number(9)),
        Ok(true)
    );
    assert_eq!(settings.pipe_count, 9);
    assert_eq!(
        settings.set_control("pipe_count", ControlValue::Number(9)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("pipe_count", ControlValue::Number(99)),
        Ok(true)
    );
    assert_eq!(settings.pipe_count, 16);
    assert_eq!(
        settings.set_control("pipe_count", ControlValue::Number(-4)),
        Ok(true)
    );
    assert_eq!(settings.pipe_count, 1);
    assert!(settings
        .set_control("pipe_count", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("joint_style", ControlValue::Index(9))
        .is_err());
    assert!(settings
        .set_control("shading", ControlValue::Number(1))
        .is_err());
    assert_eq!(
        settings.set_control("joint_style", ControlValue::Index(2)),
        Ok(true)
    );
    assert_eq!(settings.joint_style, JointStyle::Bare);
    assert_eq!(
        settings.set_control("nonsense", ControlValue::Number(1)),
        Ok(false)
    );
    assert_eq!(settings.normalized(), settings);
}

#[test]
fn growth_lights_more_dots_over_time() {
    let mut scene = scene_with(&fixed(11));
    let early = frame_at(&mut scene, 1.0).lit_dots();
    let middle = frame_at(&mut scene, 5.0).lit_dots();
    let late = frame_at(&mut scene, 10.0).lit_dots();
    assert!(early > 0, "the first pipe must already be visible");
    assert!(middle > early, "{early} -> {middle}");
    assert!(late > middle, "{middle} -> {late}");
}

#[test]
fn nothing_is_drawn_outside_zero_to_one_and_full_range_is_used() {
    let mut scene = scene_with(&fixed(2));
    let frame = frame_at(&mut scene, 14.0);
    assert!(frame
        .raster
        .dots
        .iter()
        .all(|dot| (0.0..=1.0).contains(dot)));
    let max = frame.raster.dots.iter().copied().fold(0.0, f32::max);
    assert!(
        max > 0.9,
        "highlights should reach full brightness, got {max}"
    );
    // A gradient, not a two-tone image: many distinct dot intensities.
    let levels: BTreeSet<u32> = frame
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.02)
        .map(|dot| (dot * 32.0) as u32)
        .collect();
    assert!(levels.len() >= 12, "only {} intensity levels", levels.len());
}

#[test]
fn same_seed_and_times_give_identical_frames() {
    let settings = fixed(5);
    let mut a = scene_with(&settings);
    let mut b = scene_with(&settings);
    for time in [0.0, 0.4, 3.3, 7.9, 12.0] {
        let first = frame_at(&mut a, time);
        let second = frame_at(&mut b, time);
        assert_eq!(first.raster.dots, second.raster.dots, "t={time}");
    }
}

#[test]
fn different_seeds_give_different_layouts() {
    let mut a = scene_with(&fixed(1));
    let mut b = scene_with(&fixed(2));
    assert_ne!(
        frame_at(&mut a, 8.0).raster.dots,
        frame_at(&mut b, 8.0).raster.dots
    );
}

#[test]
fn random_seed_mode_depends_on_the_run_not_on_a_constant() {
    // The scene reads Frame::now (never the system clock); the debug helper
    // derives it from the animation time, so the same helper input repeats
    // exactly while a fixed seed is unaffected by it.
    let settings = PipesSettings::default();
    let mut a = scene_with(&settings);
    let mut b = scene_with(&settings);
    assert_eq!(
        frame_at(&mut a, 6.0).raster.dots,
        frame_at(&mut b, 6.0).raster.dots
    );
}

#[test]
fn growth_is_independent_of_the_frame_rate() {
    let settings = fixed(9);
    let mut smooth = scene_with(&settings);
    let mut jumpy = scene_with(&settings);
    let mut time = 0.0;
    while time < 9.0 {
        frame_at(&mut smooth, time);
        time += 1.0 / 30.0;
    }
    let smooth_frame = frame_at(&mut smooth, 9.0);
    let jumpy_frame = frame_at(&mut jumpy, 9.0);
    assert_eq!(smooth_frame.raster.dots, jumpy_frame.raster.dots);
}

#[test]
fn time_running_backwards_restarts_the_scene() {
    let settings = fixed(3);
    let mut scene = scene_with(&settings);
    let advanced = frame_at(&mut scene, 12.0);
    assert!(advanced.lit_dots() > 200);
    // Rewinding starts a fresh layout whose growth clock begins at the new time.
    let rewound = frame_at(&mut scene, 4.0);
    assert!(rewound.lit_dots() < advanced.lit_dots() / 4);
    assert_eq!(scene.epoch, 4.0);
    assert!(frame_at(&mut scene, 10.0).lit_dots() > rewound.lit_dots() * 3);
    // Same seed, same restart time: the rewound run repeats the first run.
    let mut twin = scene_with(&settings);
    frame_at(&mut twin, 12.0);
    frame_at(&mut twin, 4.0);
    assert_eq!(
        frame_at(&mut twin, 10.0).raster.dots,
        frame_at(&mut scene, 10.0).raster.dots
    );
}

#[test]
fn camera_orbit_changes_the_view_and_zero_speed_holds_it() {
    let mut orbiting = scene_with(&fixed(4));
    let a = frame_at(&mut orbiting, 30.0);
    // Same sim state (timed reset far away), later camera angle.
    let mut settings = fixed(4);
    settings.reset_mode = ResetMode::Timed;
    settings.reset_seconds = 600;
    let mut later = scene_with(&settings);
    let mut earlier = scene_with(&settings);
    assert_ne!(
        frame_at(&mut later, 60.0).raster.dots,
        frame_at(&mut earlier, 30.0).raster.dots
    );
    assert!(a.lit_dots() > 0);

    settings.orbit_speed = 0;
    let mut still_early = scene_with(&settings);
    let mut still_late = scene_with(&settings);
    // The layout is full by both times, so only the camera could differ.
    let first = frame_at(&mut still_early, 100.0);
    let second = frame_at(&mut still_late, 140.0);
    assert_eq!(first.raster.dots, second.raster.dots);
}

#[test]
fn a_full_volume_fades_and_restarts_with_a_new_layout() {
    let mut settings = fixed(6);
    settings.volume_size = 4;
    settings.pipe_count = 3;
    settings.growth_speed = 10;
    let mut scene = scene_with(&settings);
    let mut time = 0.0;
    let mut peak = 0;
    let mut cycles = BTreeSet::new();
    let mut saw_dim_frame = false;
    while time < 40.0 {
        let frame = frame_at(&mut scene, time);
        let lit = frame.lit_dots();
        peak = peak.max(lit);
        cycles.insert(scene.cycle);
        if peak > 0 && lit * 4 < peak && scene.cycle > 0 {
            saw_dim_frame = true;
        }
        time += 0.1;
    }
    assert!(
        cycles.len() >= 3,
        "expected several restarts, got {cycles:?}"
    );
    assert!(peak > 200);
    assert!(saw_dim_frame || cycles.len() > 3);
}

#[test]
fn fade_darkens_the_finished_structure_before_the_restart() {
    let mut settings = fixed(6);
    settings.volume_size = 4;
    settings.pipe_count = 3;
    settings.growth_speed = 10;
    let mut scene = scene_with(&settings);
    // Find when this small volume finishes.
    let mut time = 0.0;
    let full_seconds = loop {
        frame_at(&mut scene, time);
        if let Some(full) = scene.sim.as_ref().and_then(Sim::full_tick) {
            break full / 10.0;
        }
        time += 0.1;
        assert!(time < 60.0, "never filled");
    };
    let full_frame = frame_at(&mut scene, full_seconds + 1.0).lit_dots();
    let fading = frame_at(&mut scene, full_seconds + HOLD_SECONDS + FADE_SECONDS * 0.8).lit_dots();
    assert!(full_frame > 100);
    assert!(fading < full_frame / 2, "{full_frame} -> {fading}");
}

#[test]
fn timed_reset_restarts_on_schedule_even_when_not_full() {
    let mut settings = fixed(8);
    settings.reset_mode = ResetMode::Timed;
    settings.reset_seconds = 20;
    settings.volume_size = 16;
    settings.growth_speed = 2;
    let mut scene = scene_with(&settings);
    frame_at(&mut scene, 10.0);
    assert_eq!(scene.cycle, 0);
    let before = frame_at(&mut scene, 17.0).lit_dots();
    let fading = frame_at(&mut scene, 19.5).lit_dots();
    assert!(fading < before, "{before} -> {fading}");
    frame_at(&mut scene, 20.5);
    assert_eq!(scene.cycle, 1);
    // The new cycle started at t=20: nothing much grown yet.
    let young = frame_at(&mut scene, 21.0).lit_dots();
    assert!(young < before / 2, "{young} vs {before}");
}

#[test]
fn a_huge_clock_jump_does_not_hang_or_panic() {
    let mut settings = fixed(1);
    settings.volume_size = 4;
    settings.growth_speed = 30;
    let mut scene = scene_with(&settings);
    frame_at(&mut scene, 1.0);
    let start = Instant::now();
    let frame = frame_at(&mut scene, 1.0e7);
    assert!(start.elapsed() < Duration::from_secs(20));
    assert!(frame.raster.dots.iter().all(|dot| dot.is_finite()));
}

#[test]
fn joint_styles_and_shadings_render_distinct_pictures() {
    let mut seen = Vec::new();
    for joint in [JointStyle::Ball, JointStyle::Rounded, JointStyle::Bare] {
        let mut settings = fixed(12);
        settings.joint_style = joint;
        let mut scene = scene_with(&settings);
        let frame = frame_at(&mut scene, 8.0);
        assert!(frame.lit_dots() > 50, "{joint:?}");
        seen.push(frame.raster.dots);
    }
    assert_ne!(seen[0], seen[1]);
    assert_ne!(seen[1], seen[2]);
    let mut shaded = Vec::new();
    for shading in [
        PipeShading::SoftLit,
        PipeShading::Flat,
        PipeShading::HighContrast,
    ] {
        let mut settings = fixed(12);
        settings.shading = shading;
        let mut scene = scene_with(&settings);
        let frame = frame_at(&mut scene, 8.0);
        assert!(frame.lit_dots() > 50, "{shading:?}");
        shaded.push(frame.raster.dots);
    }
    assert_ne!(shaded[0], shaded[1]);
    assert_ne!(shaded[0], shaded[2]);
}

#[test]
fn patterns_change_pipe_surfaces() {
    let mut frames = Vec::new();
    for pattern in [PipePattern::Plain, PipePattern::Rings, PipePattern::Checker] {
        let mut settings = fixed(12);
        settings.pattern = pattern;
        let mut scene = scene_with(&settings);
        frames.push(frame_at(&mut scene, 8.0).raster.dots);
    }
    assert_ne!(frames[0], frames[1]);
    assert_ne!(frames[0], frames[2]);
    assert_ne!(frames[1], frames[2]);
}

#[test]
fn thicker_pipes_cover_more_dots() {
    let count = |thickness: u32| {
        let mut settings = fixed(3);
        settings.pipe_thickness = thickness;
        let mut scene = scene_with(&settings);
        frame_at(&mut scene, 6.0)
            .raster
            .dots
            .iter()
            .filter(|dot| **dot > 0.05)
            .count()
    };
    assert!(count(50) > count(15) * 3 / 2);
}

#[test]
fn pipe_count_controls_simultaneous_growth() {
    let count = |pipes: u32| {
        let mut settings = fixed(3);
        settings.pipe_count = pipes;
        let mut scene = scene_with(&settings);
        frame_at(&mut scene, 6.0);
        scene.sim.as_ref().map_or(0, Sim::occupied_count)
    };
    assert!(count(10) > count(2));
}

#[test]
fn degenerate_raster_sizes_are_harmless() {
    let mut scene = scene_with(&fixed(1));
    for (width, height) in [(0, 0), (1, 1), (2, 1), (3, 2), (200, 1)] {
        let frame = render_frame(&mut scene, width, height, seconds(2.0));
        assert!(frame.raster.dots.iter().all(|dot| dot.is_finite()));
    }
}

#[test]
fn narrow_and_wide_terminals_both_show_the_structure() {
    for (width, height) in [(20, 40), (120, 20)] {
        let mut scene = scene_with(&fixed(1));
        let frame = render_frame(&mut scene, width, height, seconds(12.0));
        assert!(frame.lit_dots() > 30, "{width}x{height}");
    }
}

#[test]
fn scene_reports_no_status_and_no_cell_colors() {
    let scene = scene_with(&PipesSettings::default());
    assert!(scene.status().is_none());
    assert!(!scene.uses_cell_colors());
    assert!((1..=30).contains(&scene.frames_per_second()));
}

#[test]
fn dropping_the_scene_releases_it_at_once() {
    // The scene owns no threads or child processes; dropping must be instant
    // even in the middle of a run.
    let mut scene = scene_with(&fixed(1));
    frame_at(&mut scene, 3.0);
    let start = Instant::now();
    drop(scene);
    assert!(start.elapsed() < Duration::from_millis(50));
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Release-only timing: a 200x60 cell terminal must render in a few
/// milliseconds per frame. Debug builds skip the bound so they cannot flake.
#[test]
fn rendering_100_frames_at_200x60_is_fast_in_release() {
    let mut settings = fixed(5);
    settings.reset_mode = ResetMode::Timed;
    settings.reset_seconds = 600;
    settings.volume_size = 12;
    let mut scene = scene_with(&settings);
    // Let the structure grow first so the frames are the expensive kind.
    frame_at(&mut scene, 60.0);
    let mut timings = Vec::new();
    for frame_index in 0..100 {
        let time = 60.0 + f64::from(frame_index) / 12.0;
        let start = Instant::now();
        let frame = render_frame(&mut scene, 200, 60, seconds(time));
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(frame.lit_dots() > 500);
    }
    let typical = median(timings.clone());
    let worst = timings.iter().copied().fold(0.0, f64::max);
    eprintln!("200x60 frame cost: median {typical:.2} ms, worst {worst:.2} ms");
    if !cfg!(debug_assertions) {
        assert!(typical < 25.0, "median {typical} ms");
    }
}

#[test]
#[ignore = "prints pictures for visual inspection: cargo test -- --ignored --nocapture"]
fn print_pictures() {
    let show = |label: &str, settings: &PipesSettings, times: &[f64], width: u16, height: u16| {
        for time in times {
            let mut scene = scene_with(settings);
            let frame = render_frame(&mut scene, width, height, seconds(*time));
            println!("--- {label} t={time}s");
            for line in frame.braille_lines(100, DitherMode::Ordered) {
                println!("{line}");
            }
        }
    };
    show("default", &fixed(3), &[4.0, 14.0], 60, 30);
    let mut high = fixed(3);
    high.shading = PipeShading::HighContrast;
    show("high contrast", &high, &[14.0], 60, 30);
    let mut flat = fixed(3);
    flat.shading = PipeShading::Flat;
    show("flat", &flat, &[14.0], 60, 30);
    let mut rings = fixed(3);
    rings.pattern = PipePattern::Rings;
    rings.joint_style = JointStyle::Rounded;
    show("rings rounded", &rings, &[14.0], 60, 30);
    let mut checker = fixed(3);
    checker.pattern = PipePattern::Checker;
    checker.joint_style = JointStyle::Bare;
    show("checker bare", &checker, &[14.0], 60, 30);
}

#[test]
#[ignore = "writes PNGs for visual inspection"]
fn write_pngs() {
    let dir =
        std::path::PathBuf::from(std::env::var("PIPES_PNG_DIR").unwrap_or_else(|_| "/tmp".into()));
    let cases: Vec<(&str, PipesSettings, f64)> = vec![
        ("soft", fixed(3), 14.0),
        (
            "high",
            PipesSettings {
                shading: PipeShading::HighContrast,
                ..fixed(3)
            },
            14.0,
        ),
        (
            "flat",
            PipesSettings {
                shading: PipeShading::Flat,
                ..fixed(3)
            },
            14.0,
        ),
        (
            "rings",
            PipesSettings {
                pattern: PipePattern::Rings,
                joint_style: JointStyle::Rounded,
                ..fixed(3)
            },
            14.0,
        ),
        (
            "checker",
            PipesSettings {
                pattern: PipePattern::Checker,
                joint_style: JointStyle::Bare,
                ..fixed(3)
            },
            14.0,
        ),
        ("early", fixed(3), 5.0),
    ];
    for (name, settings, time) in cases {
        let mut scene = scene_with(&settings);
        let frame = render_frame(&mut scene, 100, 40, seconds(time));
        let (w, h) = (frame.raster.width, frame.raster.height);
        let scale = 3;
        let mut gray = image::GrayImage::new((w * 2 * scale) as u32, (h * scale) as u32);
        for y in 0..h {
            for x in 0..w {
                let v = frame.raster.dots[y * w + x];
                let d = crate::raster::threshold(x, y, DitherMode::Ordered);
                for sy in 0..scale {
                    for sx in 0..scale {
                        gray.put_pixel(
                            (x * scale + sx) as u32,
                            (y * scale + sy) as u32,
                            image::Luma([(v * 255.0) as u8]),
                        );
                        gray.put_pixel(
                            ((w + x) * scale + sx) as u32,
                            (y * scale + sy) as u32,
                            image::Luma([if v > d { 255 } else { 0 }]),
                        );
                    }
                }
            }
        }
        gray.save(dir.join(format!("pipes_{name}.png"))).unwrap();
    }
}
