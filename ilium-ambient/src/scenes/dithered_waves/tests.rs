use super::settings::{DitherMatrix, RenderBackend};
use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::gpu::test_support::{scripted_runner, AvailabilityGuard};
use crate::gpu::GpuUnavailable;
use std::time::{Duration, Instant};

fn scene_with(settings: &DitheredWavesSettings) -> DitheredWavesScene {
    DitheredWavesScene::new(
        settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    )
}

fn render_at(settings: &DitheredWavesSettings, width: u16, height: u16, time: f64) -> Rendered {
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
    let settings = DitheredWavesSettings::default();
    let first = render_at(&settings, 60, 24, 12.5);
    let second = render_at(&settings, 60, 24, 12.5);
    assert_eq!(first.raster.dots, second.raster.dots);
}

#[test]
fn render_order_does_not_matter() {
    let settings = DitheredWavesSettings::default();
    let mut scene = scene_with(&settings);
    let _ = render_frame(&mut scene, 40, 20, Duration::from_secs(50));
    let after = render_frame(&mut scene, 40, 20, Duration::from_secs(7));
    let fresh = render_at(&settings, 40, 20, 7.0);
    assert_eq!(after.raster.dots, fresh.raster.dots);
}

#[test]
fn differing_times_differ() {
    let settings = DitheredWavesSettings::default();
    let first = render_at(&settings, 60, 24, 0.0);
    let second = render_at(&settings, 60, 24, 30.0);
    assert_ne!(first.raster.dots, second.raster.dots);
}

#[test]
fn zero_speed_freezes_the_waves() {
    let settings = DitheredWavesSettings {
        wave_speed: 0,
        ..DitheredWavesSettings::default()
    };
    let first = render_at(&settings, 40, 20, 0.0);
    let second = render_at(&settings, 40, 20, 90.0);
    assert_eq!(first.raster.dots, second.raster.dots);
}

#[test]
fn different_seeds_give_different_layouts() {
    let one = render_at(&DitheredWavesSettings::default(), 40, 20, 3.0);
    let two = render_at(
        &DitheredWavesSettings {
            seed: 2,
            ..DitheredWavesSettings::default()
        },
        40,
        20,
        3.0,
    );
    assert_ne!(one.raster.dots, two.raster.dots);
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
        brightness: 0,
        contrast: 9999,
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
            clean.brightness,
            clean.contrast,
            clean.bias,
            clean.seed
        ),
        (100, 100, 200, 6, 1, 10, 300, -30, 9999)
    );
}

#[test]
fn controls_count_and_kinds_are_sensible() {
    let rows = DitheredWavesSettings::default().controls();
    assert!((6..=12).contains(&rows.len()));
    let mut ids: Vec<_> = rows.iter().map(|row| row.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), rows.len(), "control ids are unique");
    assert!(rows.iter().all(|row| !row.help.is_empty()));
}

#[test]
fn every_control_round_trips_through_set_control() {
    // The GPU option is selectable only while a GPU is available.
    let _guard = AvailabilityGuard::ready();
    let original = DitheredWavesSettings::default();
    for row in original.controls() {
        // Move each row one step away from its default, then check the
        // rebuilt row shows the new value and can be restored.
        let moved = match &row.kind {
            ControlKind::Slider { min, max, step, .. } => {
                let ControlValue::Number(current) = row.value else {
                    panic!("slider {} without a number", row.id);
                };
                let target = if current + step <= *max {
                    current + step
                } else {
                    (current - step).max(*min)
                };
                ControlValue::Number(target)
            }
            ControlKind::Choice { options } => {
                let ControlValue::Index(current) = row.value else {
                    panic!("choice {} without an index", row.id);
                };
                ControlValue::Index((current + 1) % options.len())
            }
            ControlKind::Toggle => ControlValue::Bool(true),
            ControlKind::Text { .. } => continue,
        };
        let mut settings = original.clone();
        assert_eq!(
            settings.set_control(row.id, moved.clone()),
            Ok(true),
            "{}",
            row.id
        );
        let reread = settings
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id)
            .map(|candidate| candidate.value);
        assert_eq!(reread, Some(moved), "{}", row.id);
        assert_eq!(settings.set_control(row.id, row.value.clone()), Ok(true));
        assert_eq!(settings, original, "{}", row.id);
        assert_eq!(settings.set_control(row.id, row.value.clone()), Ok(false));
    }
}

#[test]
fn slider_edits_are_clamped_and_wrong_types_are_errors() {
    let mut settings = DitheredWavesSettings::default();
    assert_eq!(
        settings.set_control("levels", ControlValue::Number(500)),
        Ok(true)
    );
    assert_eq!(settings.levels, 6);
    assert_eq!(
        settings.set_control("bias", ControlValue::Number(-500)),
        Ok(true)
    );
    assert_eq!(settings.bias, -30);
    assert!(settings
        .set_control("levels", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("dither_matrix", ControlValue::Index(99))
        .is_err());
}

#[test]
fn unknown_control_id_returns_false() {
    let mut settings = DitheredWavesSettings::default();
    assert_eq!(
        settings.set_control("no_such_control", ControlValue::Number(1)),
        Ok(false)
    );
}

#[test]
fn settings_deserialize_with_missing_fields() {
    let parsed: DitheredWavesSettings = serde_json::from_str(r#"{"levels": 4}"#).unwrap();
    assert_eq!(parsed.levels, 4);
    assert_eq!(parsed.wave_frequency, 300);
}

#[test]
fn default_raster_is_non_empty_and_not_uniform() {
    let rendered = render_at(&DitheredWavesSettings::default(), 100, 40, 5.0);
    let total = rendered.raster.dots.len();
    let dots_on = lit(&rendered);
    assert!(dots_on > total / 50, "too few dots: {dots_on}/{total}");
    assert!(dots_on < total * 9 / 10, "too many dots: {dots_on}/{total}");
}

#[test]
fn default_picture_is_quiet() {
    let rendered = render_at(&DitheredWavesSettings::default(), 100, 40, 5.0);
    let peak = rendered.raster.dots.iter().copied().fold(0.0f32, f32::max);
    assert!((peak - 0.35).abs() < 1e-6, "peak {peak}");
}

#[test]
fn works_at_tiny_sizes() {
    for (width, height) in [(1, 1), (3, 2), (1, 7), (9, 1)] {
        for pixel_size in [1, 4] {
            let settings = DitheredWavesSettings {
                pixel_size,
                ..DitheredWavesSettings::default()
            };
            let rendered = render_at(&settings, width, height, 2.0);
            assert_eq!(
                rendered.raster.dots.len(),
                usize::from(width) * usize::from(height) * 8
            );
            assert!(rendered.raster.dots.iter().all(|dot| dot.is_finite()));
        }
    }
    let empty = render_at(&DitheredWavesSettings::default(), 0, 0, 1.0);
    assert!(empty.raster.dots.is_empty());
}

#[test]
fn every_matrix_and_level_count_renders_finite_bounded_values() {
    for dither_matrix in [
        DitherMatrix::Bayer2,
        DitherMatrix::Bayer4,
        DitherMatrix::Bayer8,
        DitherMatrix::Noise,
    ] {
        for levels in 2..=6 {
            let settings = DitheredWavesSettings {
                dither_matrix,
                levels,
                ripple: true,
                ..DitheredWavesSettings::default()
            };
            let rendered = render_at(&settings, 30, 12, 4.0);
            assert!(rendered
                .raster
                .dots
                .iter()
                .all(|dot| (0.0..=0.35).contains(dot)));
            assert!(lit(&rendered) > 0, "{dither_matrix:?} {levels}");
        }
    }
}

#[test]
fn binary_levels_produce_only_two_values() {
    let rendered = render_at(&DitheredWavesSettings::default(), 40, 20, 1.0);
    assert!(rendered
        .raster
        .dots
        .iter()
        .all(|dot| *dot == 0.0 || (*dot - 0.35).abs() < 1e-6));
}

#[test]
fn more_levels_produce_intermediate_values() {
    let settings = DitheredWavesSettings {
        levels: 5,
        ..DitheredWavesSettings::default()
    };
    let rendered = render_at(&settings, 60, 24, 1.0);
    let intermediate = rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.01 && **dot < 0.34)
        .count();
    assert!(intermediate > 0);
}

#[test]
fn positive_bias_adds_dots_and_negative_removes_them() {
    let with_bias = |bias| {
        let settings = DitheredWavesSettings {
            bias,
            ..DitheredWavesSettings::default()
        };
        lit(&render_at(&settings, 60, 24, 1.0))
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
    let rendered = render_at(&settings, 40, 20, 1.0);
    let width = rendered.raster.width;
    for block_y in 0..rendered.raster.height / 4 {
        for block_x in 0..width / 4 {
            let first = rendered.raster.dots[block_y * 4 * width + block_x * 4];
            for dy in 0..4 {
                for dx in 0..4 {
                    let value = rendered.raster.dots[(block_y * 4 + dy) * width + block_x * 4 + dx];
                    assert_eq!(value.to_bits(), first.to_bits());
                }
            }
        }
    }
}

#[test]
fn ripple_changes_the_picture() {
    let plain = render_at(&DitheredWavesSettings::default(), 60, 24, 2.0);
    let rippled = render_at(
        &DitheredWavesSettings {
            ripple: true,
            ..DitheredWavesSettings::default()
        },
        60,
        24,
        2.0,
    );
    assert_ne!(plain.raster.dots, rippled.raster.dots);
}

#[test]
fn bayer_tiles_are_permutations_with_the_standard_layout() {
    for side in [2usize, 4, 8] {
        let tile = field::ThresholdTile::bayer(side);
        let cells = (side * side) as f32;
        let mut ranks: Vec<u32> = (0..side * side)
            .map(|index| (tile.at(index % side, index / side) * cells - 0.5).round() as u32)
            .collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (0..(side * side) as u32).collect::<Vec<_>>());
    }
    let four = field::ThresholdTile::bayer(4);
    let expected = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    for (y, row) in expected.iter().enumerate() {
        for (x, rank) in row.iter().enumerate() {
            let value = four.at(x, y) * 16.0 - 0.5;
            assert!((value - *rank as f32).abs() < 1e-4, "({x},{y})");
        }
    }
}

#[test]
fn gpu_backend_reports_fallback_and_matches_software() {
    let software = DitheredWavesSettings::default();
    let gpu = DitheredWavesSettings {
        render_backend: RenderBackend::Gpu,
        ..software.clone()
    };
    let _guard = AvailabilityGuard::unavailable(GpuUnavailable::NotCompiled);
    assert_eq!(scene_with(&software).status(), None);
    assert_eq!(
        scene_with(&gpu).status().as_deref(),
        Some("GPU support is not compiled into this build.")
    );
    assert_eq!(
        render_at(&software, 40, 20, 3.0).raster.dots,
        render_at(&gpu, 40, 20, 3.0).raster.dots
    );
}

#[test]
fn default_cadence_is_gentle_and_slow_motion() {
    let defaults = DitheredWavesSettings::default();
    assert_eq!(scene_with(&defaults).frames_per_second(), 8);
    assert_eq!(defaults.wave_speed, 50);
    assert_eq!(defaults.render_backend, RenderBackend::Software);
    let rows = defaults.controls();
    let backend = rows.iter().find(|row| row.id == "render_backend");
    assert_eq!(
        backend.map(|row| row.kind.clone()),
        Some(ControlKind::Choice {
            options: vec!["Software (slow-mo)", "GPU"]
        })
    );
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY
        .iter()
        .all(|url| url.starts_with("https://") && !url.contains('?')));
}

#[test]
fn scene_does_not_use_cell_colors() {
    assert!(!scene_with(&DitheredWavesSettings::default()).uses_cell_colors());
}

#[test]
fn full_size_frame_is_cheap() {
    let mut scene = scene_with(&DitheredWavesSettings::default());
    render_frame(&mut scene, 100, 40, Duration::from_secs(1));
    let start = Instant::now();
    for step in 0..5 {
        render_frame(&mut scene, 100, 40, Duration::from_secs(2 + step));
    }
    let per_frame = start.elapsed() / 5;
    // Generous bound for unoptimised test builds; release is far below 8 ms.
    assert!(per_frame < Duration::from_millis(400), "{per_frame:?}");
}

#[test]
#[ignore = "prints the picture for eyeballing"]
fn print_picture() {
    for time in [0.0, 40.0] {
        let rendered = render_at(&DitheredWavesSettings::default(), 100, 40, time);
        let width = rendered.raster.width;
        for y in (0..rendered.raster.height).step_by(2) {
            let line: String = (0..width)
                .map(|x| {
                    if rendered.raster.dots[y * width + x] > 0.1 {
                        '#'
                    } else {
                        ' '
                    }
                })
                .collect();
            println!("{line}");
        }
        println!("{}", "=".repeat(width));
    }
}

fn backend_row(settings: &DitheredWavesSettings) -> crate::control::Control {
    settings
        .controls()
        .into_iter()
        .find(|row| row.id == "render_backend")
        .unwrap()
}

#[test]
fn gpu_option_is_disabled_while_unavailable_and_enabled_when_ready() {
    let settings = DitheredWavesSettings::default();
    {
        let _guard = AvailabilityGuard::unavailable(GpuUnavailable::NoDriver);
        let row = backend_row(&settings);
        assert_eq!(row.disabled_reason(0), None);
        assert_eq!(
            row.disabled_reason(1),
            Some(GpuUnavailable::NoDriver.fix().as_str())
        );
        assert_eq!(row.stepped(1), Some(ControlValue::Index(0)));
    }
    let _guard = AvailabilityGuard::ready();
    let row = backend_row(&settings);
    assert_eq!(row.disabled_reason(1), None);
    assert_eq!(row.stepped(1), Some(ControlValue::Index(1)));
    assert!(row.help_detail.as_deref().unwrap().contains("Test GPU"));
}

#[test]
fn set_control_rejects_gpu_when_unavailable_and_accepts_it_when_ready() {
    let mut settings = DitheredWavesSettings::default();
    {
        let _guard = AvailabilityGuard::unavailable(GpuUnavailable::NoVulkanLoader);
        assert_eq!(
            settings.set_control("render_backend", ControlValue::Index(1)),
            Err(GpuUnavailable::NoVulkanLoader.summary())
        );
        assert_eq!(settings.render_backend, RenderBackend::Software);
        assert_eq!(
            settings.set_control("render_backend", ControlValue::Index(0)),
            Ok(false)
        );
    }
    let _guard = AvailabilityGuard::ready();
    assert_eq!(
        settings.set_control("render_backend", ControlValue::Index(1)),
        Ok(true)
    );
    assert_eq!(settings.render_backend, RenderBackend::Gpu);
}

#[test]
fn ready_gpu_reports_the_seam_status_and_keeps_drawing_software() {
    let _guard = AvailabilityGuard::ready();
    let settings = DitheredWavesSettings {
        render_backend: RenderBackend::Gpu,
        ..DitheredWavesSettings::default()
    };
    let without = scene_with(&settings);
    assert_eq!(
        without.status().as_deref(),
        Some("No GPU device was provided by the host; using software")
    );

    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.gpu = Some(scripted_runner(false));
    let mut scene = DitheredWavesScene::new(&settings, &env);
    let rendered = render_frame(&mut scene, 20, 10, Duration::from_secs(1));
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    assert_eq!(
        scene.status().as_deref(),
        Some("GPU kernel not ported yet; using software")
    );

    env.gpu = Some(scripted_runner(true));
    let mut failing = DitheredWavesScene::new(&settings, &env);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut rendered = render_frame(&mut failing, 20, 10, Duration::from_secs(1));
    while failing.status().as_deref() != Some("GPU error: fake kernel not ported; using software")
        && Instant::now() < deadline
    {
        rendered = render_frame(&mut failing, 20, 10, Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        failing.status().as_deref(),
        Some("GPU error: fake kernel not ported; using software")
    );
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn saved_gpu_choice_renders_software_and_reports_the_summary() {
    let _guard = AvailabilityGuard::unavailable(GpuUnavailable::Checking);
    let settings = DitheredWavesSettings {
        render_backend: RenderBackend::Gpu,
        ..DitheredWavesSettings::default()
    };
    let scene = scene_with(&settings);
    assert_eq!(
        scene.status().as_deref(),
        Some("Checking for a usable GPU...")
    );
}
