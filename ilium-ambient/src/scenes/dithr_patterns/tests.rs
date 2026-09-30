use super::settings::{BayerSize, DitherKind, Pattern, RenderBackend};
use super::*;
use crate::control::{Control, ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::gpu::test_support::{scripted_runner, AvailabilityGuard};
use crate::gpu::GpuUnavailable;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const ALL_PATTERNS: [Pattern; 10] = [
    Pattern::Caustics,
    Pattern::Cellular,
    Pattern::Halftone,
    Pattern::Starfield,
    Pattern::Moire,
    Pattern::Smoke,
    Pattern::Spiral,
    Pattern::Tunnel,
    Pattern::Plasma,
    Pattern::Ripples,
];

const ALL_DITHERS: [DitherKind; 4] = [
    DitherKind::OrderedBayer,
    DitherKind::FloydSteinberg,
    DitherKind::RandomNoise,
    DitherKind::Threshold,
];

fn scene_with(settings: &DithrPatternsSettings) -> DithrPatternsScene {
    DithrPatternsScene::new(settings, &SceneEnv::for_test(std::env::temp_dir()))
}

fn seconds(value: f64) -> Duration {
    Duration::from_secs_f64(value)
}

fn frame_at(settings: &DithrPatternsSettings, width: u16, height: u16, time: f64) -> Rendered {
    render_frame(&mut scene_with(settings), width, height, seconds(time))
}

fn lit(rendered: &Rendered) -> usize {
    rendered
        .raster
        .dots
        .iter()
        .filter(|dot| **dot > 0.0)
        .count()
}

fn with_pattern(pattern: Pattern) -> DithrPatternsSettings {
    DithrPatternsSettings {
        pattern,
        ..DithrPatternsSettings::default()
    }
}

#[test]
fn defaults_survive_normalization_and_wild_values_are_clamped() {
    let defaults = DithrPatternsSettings::default();
    assert_eq!(defaults.normalized(), defaults);
    let wild = DithrPatternsSettings {
        density: 900,
        complexity: 0,
        scale: 1,
        dither_amount: 500,
        loop_seconds: 0,
        contrast: 0,
        seed: 100_000,
        ..DithrPatternsSettings::default()
    };
    let clean = wild.normalized();
    assert_eq!(
        (
            clean.density,
            clean.complexity,
            clean.scale,
            clean.dither_amount,
            clean.loop_seconds,
            clean.contrast,
            clean.seed
        ),
        (100, 1, 20, 100, 4, 10, 999)
    );
}

#[test]
fn defaults_are_gentle_and_use_the_software_backend() {
    let scene = scene_with(&DithrPatternsSettings::default());
    assert_eq!(scene.frames_per_second(), 8);
    assert_eq!(scene.status(), None);
    assert!(!scene.uses_cell_colors());
    assert_eq!(SLOW_MOTION, 0.5);
}

#[test]
fn serde_uses_snake_case_keys_and_missing_keys_default() {
    let json = serde_json::to_value(DithrPatternsSettings::default()).unwrap();
    let object = json.as_object().unwrap();
    for key in [
        "pattern",
        "dither",
        "density",
        "complexity",
        "scale",
        "dither_amount",
        "loop_seconds",
        "contrast",
        "matrix",
        "seed",
        "invert",
        "render_backend",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    let partial: DithrPatternsSettings = serde_json::from_str(
        r#"{"pattern":"tunnel","dither":"floyd_steinberg","matrix":"bayer4"}"#,
    )
    .unwrap();
    assert_eq!(partial.pattern, Pattern::Tunnel);
    assert_eq!(partial.dither, DitherKind::FloydSteinberg);
    assert_eq!(partial.matrix, BayerSize::Bayer4);
    assert_eq!(partial.density, 54);
    let again: DithrPatternsSettings = serde_json::from_value(json).unwrap();
    assert_eq!(again, DithrPatternsSettings::default());
}

#[test]
fn controls_are_unique_documented_and_within_the_row_budget() {
    let rows = DithrPatternsSettings::default().controls();
    assert!((6..=12).contains(&rows.len()));
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), rows.len());
    assert!(rows
        .iter()
        .all(|row| !row.label.is_empty() && !row.help.is_empty()));
    let backend = rows.iter().find(|row| row.id == "render_backend").unwrap();
    assert_eq!(
        backend.kind,
        ControlKind::Choice {
            options: vec!["Software (slow-mo)", "GPU"]
        }
    );
}

#[test]
fn every_control_round_trips_through_set_control() {
    // The GPU option is selectable only while a GPU is available.
    let _guard = AvailabilityGuard::ready();
    let original = DithrPatternsSettings {
        seed: 3,
        ..DithrPatternsSettings::default()
    };
    for row in original.controls() {
        // Move each row one step, apply it to a fresh copy, and read it back.
        let stepped = row.stepped(1).unwrap();
        let mut changed = original.clone();
        assert_eq!(
            changed.set_control(row.id, stepped.clone()),
            Ok(true),
            "{}",
            row.id
        );
        let readback: Control = changed
            .controls()
            .into_iter()
            .find(|candidate| candidate.id == row.id)
            .unwrap();
        assert_eq!(readback.value, stepped, "{}", row.id);
        assert_eq!(
            changed.set_control(row.id, stepped),
            Ok(false),
            "unchanged {}",
            row.id
        );
        // Every other row is untouched.
        for other in changed.controls().iter().filter(|other| other.id != row.id) {
            let before = original
                .controls()
                .into_iter()
                .find(|candidate| candidate.id == other.id)
                .unwrap();
            assert_eq!(other.value, before.value, "{} moved {}", row.id, other.id);
        }
    }
}

#[test]
fn set_control_rejects_unknown_ids_and_bad_values() {
    let mut settings = DithrPatternsSettings::default();
    assert_eq!(
        settings.set_control("does_not_exist", ControlValue::Number(1)),
        Ok(false)
    );
    assert_eq!(
        settings.set_control("density", ControlValue::Number(9999)),
        Ok(true)
    );
    assert_eq!(settings.density, 100);
    assert_eq!(
        settings.set_control("density", ControlValue::Number(-5)),
        Ok(true)
    );
    assert_eq!(settings.density, 0);
    assert!(settings
        .set_control("density", ControlValue::Bool(true))
        .is_err());
    assert!(settings
        .set_control("pattern", ControlValue::Index(99))
        .is_err());
    assert!(settings
        .set_control("invert", ControlValue::Number(1))
        .is_err());
    assert_eq!(settings.pattern, Pattern::Caustics);
}

#[test]
fn same_time_gives_an_identical_raster_and_fresh_scenes_agree() {
    for pattern in ALL_PATTERNS {
        for dither in ALL_DITHERS {
            let settings = DithrPatternsSettings {
                pattern,
                dither,
                ..DithrPatternsSettings::default()
            };
            let mut scene = scene_with(&settings);
            let first = render_frame(&mut scene, 30, 12, seconds(4.2));
            let other = render_frame(&mut scene, 30, 12, seconds(9.0));
            let again = render_frame(&mut scene, 30, 12, seconds(4.2));
            let fresh = frame_at(&settings, 30, 12, 4.2);
            assert_eq!(
                first.raster.dots, again.raster.dots,
                "{pattern:?} {dither:?}"
            );
            assert_eq!(
                first.raster.dots, fresh.raster.dots,
                "{pattern:?} {dither:?}"
            );
            assert_ne!(
                first.raster.dots, other.raster.dots,
                "{pattern:?} {dither:?}"
            );
        }
    }
}

#[test]
fn different_times_differ_for_every_pattern() {
    for pattern in ALL_PATTERNS {
        let settings = with_pattern(pattern);
        let early = frame_at(&settings, 40, 16, 1.0);
        let later = frame_at(&settings, 40, 16, 5.0);
        assert_ne!(early.raster.dots, later.raster.dots, "{pattern:?}");
    }
}

#[test]
fn defaults_draw_something_that_is_not_uniform() {
    for pattern in ALL_PATTERNS {
        let rendered = frame_at(&with_pattern(pattern), 60, 24, 2.0);
        let total = rendered.raster.dots.len();
        let on = lit(&rendered);
        assert!(on > total / 200, "{pattern:?} nearly empty: {on}/{total}");
        assert!(on < total * 9 / 10, "{pattern:?} nearly full: {on}/{total}");
    }
}

#[test]
fn dot_intensities_stay_within_the_contrast_cap() {
    let settings = DithrPatternsSettings::default();
    let cap = settings.contrast as f32 / 100.0;
    for pattern in ALL_PATTERNS {
        let rendered = frame_at(&with_pattern(pattern), 40, 16, 3.3);
        assert!(rendered
            .raster
            .dots
            .iter()
            .all(|dot| (0.0..=cap + 1e-6).contains(dot)));
    }
}

#[test]
fn animation_loops_exactly_after_loop_seconds() {
    // Internal clock runs at half speed, so one loop takes twice as long.
    let loop_seconds = 12.0;
    let real_loop = loop_seconds / SLOW_MOTION;
    for pattern in ALL_PATTERNS {
        for dither in [DitherKind::OrderedBayer, DitherKind::Threshold] {
            let settings = DithrPatternsSettings {
                pattern,
                dither,
                loop_seconds: loop_seconds as u32,
                ..DithrPatternsSettings::default()
            };
            let start = frame_at(&settings, 40, 16, 3.0);
            let wrapped = frame_at(&settings, 40, 16, 3.0 + real_loop);
            let differing = start
                .raster
                .dots
                .iter()
                .zip(&wrapped.raster.dots)
                .filter(|(a, b)| (**a - **b).abs() > 1e-6)
                .count();
            // f32 phase rounding may flip a handful of threshold-edge dots.
            assert!(
                differing <= start.raster.dots.len() / 100,
                "{pattern:?} {dither:?}: {differing} dots differ"
            );
        }
    }
}

#[test]
fn works_at_tiny_and_degenerate_sizes() {
    for pattern in ALL_PATTERNS {
        for dither in ALL_DITHERS {
            let settings = DithrPatternsSettings {
                pattern,
                dither,
                ..DithrPatternsSettings::default()
            };
            for (width, height) in [(1, 1), (3, 2), (1, 9), (17, 1), (2, 2)] {
                let rendered = frame_at(&settings, width, height, 1.5);
                assert_eq!(
                    rendered.raster.dots.len(),
                    usize::from(width) * usize::from(height) * 8
                );
                assert!(rendered.raster.dots.iter().all(|dot| dot.is_finite()));
            }
        }
    }
    // A zero-sized frame is a no-op rather than a panic.
    let mut scene = scene_with(&DithrPatternsSettings::default());
    let rendered = render_frame(&mut scene, 0, 0, seconds(1.0));
    assert!(rendered.raster.dots.is_empty());
}

#[test]
fn resizing_between_frames_is_safe() {
    let mut scene = scene_with(&DithrPatternsSettings::default());
    for (width, height) in [(80, 30), (5, 4), (120, 50), (80, 30)] {
        let rendered = render_frame(&mut scene, width, height, seconds(2.0));
        assert_eq!(
            rendered.raster.dots.len(),
            usize::from(width) * usize::from(height) * 8
        );
    }
}

#[test]
fn inspired_by_lists_the_source() {
    assert!(!INSPIRED_BY.is_empty());
    assert!(INSPIRED_BY.iter().all(|url| url.starts_with("https://")));
    assert!(INSPIRED_BY.contains(&"https://www.dithr.app/"));
}

#[test]
fn gpu_backend_reports_status_and_matches_software() {
    let software = DithrPatternsSettings::default();
    let gpu = DithrPatternsSettings {
        render_backend: RenderBackend::Gpu,
        ..software.clone()
    };
    let _guard = AvailabilityGuard::unavailable(GpuUnavailable::NotCompiled);
    let gpu_scene = scene_with(&gpu);
    assert_eq!(
        gpu_scene.status().as_deref(),
        Some("GPU support is not compiled into this build.")
    );
    assert_eq!(
        frame_at(&software, 40, 16, 2.0).raster.dots,
        frame_at(&gpu, 40, 16, 2.0).raster.dots
    );
}

#[test]
fn density_and_invert_change_coverage_as_described() {
    let base = with_pattern(Pattern::Plasma);
    let sparse = frame_at(
        &DithrPatternsSettings {
            density: 20,
            ..base.clone()
        },
        50,
        20,
        2.0,
    );
    let dense = frame_at(
        &DithrPatternsSettings {
            density: 80,
            ..base.clone()
        },
        50,
        20,
        2.0,
    );
    assert!(lit(&sparse) < lit(&dense));
    let normal = frame_at(&base, 50, 20, 2.0);
    let inverted = frame_at(
        &DithrPatternsSettings {
            invert: true,
            ..base
        },
        50,
        20,
        2.0,
    );
    // Inverting swaps bright and dark regions: total coverage mirrors.
    let total = normal.raster.dots.len();
    let sum = lit(&normal) + lit(&inverted);
    assert!(sum.abs_diff(total) < total / 8, "{sum} vs {total}");
}

#[test]
fn contrast_scales_the_brightest_dot() {
    let bright = frame_at(
        &DithrPatternsSettings {
            contrast: 100,
            ..DithrPatternsSettings::default()
        },
        40,
        16,
        2.0,
    );
    let dim = frame_at(
        &DithrPatternsSettings {
            contrast: 20,
            ..DithrPatternsSettings::default()
        },
        40,
        16,
        2.0,
    );
    let peak = |rendered: &Rendered| rendered.raster.dots.iter().copied().fold(0.0f32, f32::max);
    assert!(peak(&bright) > 0.9);
    assert!(peak(&dim) <= 0.2 + 1e-6);
    assert_eq!(lit(&bright), lit(&dim));
}

#[test]
fn dither_algorithms_produce_different_textures() {
    let mut seen = BTreeSet::new();
    for dither in ALL_DITHERS {
        let settings = DithrPatternsSettings {
            dither,
            ..DithrPatternsSettings::default()
        };
        let rendered = frame_at(&settings, 50, 20, 2.0);
        let bits: Vec<bool> = rendered.raster.dots.iter().map(|dot| *dot > 0.0).collect();
        seen.insert(bits);
    }
    assert_eq!(seen.len(), ALL_DITHERS.len());
}

#[test]
fn zero_dither_amount_is_a_plain_cut_regardless_of_algorithm() {
    let frames: Vec<Vec<f32>> = [
        DitherKind::OrderedBayer,
        DitherKind::RandomNoise,
        DitherKind::Threshold,
    ]
    .into_iter()
    .map(|dither| {
        let settings = DithrPatternsSettings {
            dither,
            dither_amount: 0,
            ..DithrPatternsSettings::default()
        };
        frame_at(&settings, 40, 16, 2.0).raster.dots
    })
    .collect();
    assert_eq!(frames[0], frames[1]);
    assert_eq!(frames[1], frames[2]);
}

#[test]
fn bayer_matrix_is_a_permutation_of_its_thresholds() {
    for (matrix, size) in [(BayerSize::Bayer4, 4usize), (BayerSize::Bayer8, 8)] {
        let table = dither::BayerTable::new(matrix);
        let mut ranks: Vec<usize> = (0..size * size)
            .map(|index| {
                (table.threshold(index % size, index / size) * (size * size) as f32) as usize
            })
            .collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (0..size * size).collect::<Vec<_>>());
    }
}

#[test]
fn floyd_steinberg_preserves_average_brightness() {
    let width = 64;
    let height = 32;
    let field = vec![0.3f32; width * height];
    let mut output = vec![0.0; width * height];
    let mut carry = vec![0.0; width + 2];
    let mut next = vec![0.0; width + 2];
    let bayer = dither::BayerTable::new(BayerSize::Bayer8);
    let pass = dither::DitherPass {
        kind: DitherKind::FloydSteinberg,
        amount: 1.0,
        bayer: &bayer,
        seed: 1,
        time: 0.0,
    };
    dither::dither_field(&pass, &field, width, &mut output, &mut carry, &mut next);
    let mean = output.iter().sum::<f32>() / output.len() as f32;
    assert!((mean - 0.3).abs() < 0.03, "{mean}");
}

#[test]
fn seed_changes_the_layout() {
    for pattern in [
        Pattern::Caustics,
        Pattern::Cellular,
        Pattern::Starfield,
        Pattern::Ripples,
    ] {
        let one = frame_at(
            &DithrPatternsSettings {
                pattern,
                seed: 1,
                ..DithrPatternsSettings::default()
            },
            40,
            16,
            2.0,
        );
        let two = frame_at(
            &DithrPatternsSettings {
                pattern,
                seed: 2,
                ..DithrPatternsSettings::default()
            },
            40,
            16,
            2.0,
        );
        assert_ne!(one.raster.dots, two.raster.dots, "{pattern:?}");
    }
}

#[test]
fn complexity_adds_stars() {
    assert!(patterns::make_stars(7, 8).len() > patterns::make_stars(7, 1).len());
    assert_eq!(patterns::make_stars(7, 5).len(), 20 + 12 * 5);
}

#[test]
fn lattice_noise_is_bounded_and_scrolls_seamlessly() {
    let noise = noise::LatticeNoise::new(9);
    for step in 0..400 {
        let x = step as f32 * 0.137 - 20.0;
        let y = step as f32 * 0.291 - 30.0;
        let value = noise.fbm(x, y, 4);
        assert!((0.0..=1.0).contains(&value), "{value}");
        let wrapped = noise.fbm_wrapped_y(x, y, 4, 8);
        let shifted = noise.fbm_wrapped_y(x, y + 8.0, 4, 8);
        assert!((wrapped - shifted).abs() < 1e-3, "{wrapped} vs {shifted}");
    }
}

#[test]
fn a_frame_at_terminal_size_is_cheap() {
    let settings = DithrPatternsSettings::default();
    for pattern in ALL_PATTERNS {
        let mut scene = scene_with(&DithrPatternsSettings {
            pattern,
            ..settings.clone()
        });
        render_frame(&mut scene, 100, 40, seconds(0.0));
        let started = Instant::now();
        let frames = 5;
        for index in 1..=frames {
            render_frame(&mut scene, 100, 40, seconds(f64::from(index)));
        }
        let per_frame = started.elapsed() / frames;
        // Release target is under 8 ms; unoptimised builds get a wide margin.
        let budget = if cfg!(debug_assertions) {
            Duration::from_millis(400)
        } else {
            Duration::from_millis(8)
        };
        assert!(per_frame < budget, "{pattern:?}: {per_frame:?}");
    }
}

fn backend_row(settings: &DithrPatternsSettings) -> crate::control::Control {
    settings
        .controls()
        .into_iter()
        .find(|row| row.id == "render_backend")
        .unwrap()
}

#[test]
fn gpu_option_is_disabled_while_unavailable_and_enabled_when_ready() {
    let settings = DithrPatternsSettings::default();
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
    let mut settings = DithrPatternsSettings::default();
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
    let settings = DithrPatternsSettings {
        render_backend: RenderBackend::Gpu,
        ..DithrPatternsSettings::default()
    };
    let without = scene_with(&settings);
    assert_eq!(
        without.status().as_deref(),
        Some("No GPU device was provided by the host; using software")
    );

    let mut env = SceneEnv::for_test(std::env::temp_dir());
    env.gpu = Some(scripted_runner(false));
    let mut scene = DithrPatternsScene::new(&settings, &env);
    let rendered = render_frame(&mut scene, 20, 10, Duration::from_secs(1));
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
    assert_eq!(
        scene.status().as_deref(),
        Some("GPU kernel not ported yet; using software")
    );

    env.gpu = Some(scripted_runner(true));
    let mut failing = DithrPatternsScene::new(&settings, &env);
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
    let settings = DithrPatternsSettings {
        render_backend: RenderBackend::Gpu,
        ..DithrPatternsSettings::default()
    };
    let scene = scene_with(&settings);
    assert_eq!(
        scene.status().as_deref(),
        Some("Checking for a usable GPU...")
    );
}
