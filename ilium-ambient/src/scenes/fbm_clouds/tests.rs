use super::settings::{DitherPattern, RenderBackend};
use super::*;
use crate::control::{ControlKind, ControlValue};
use crate::debug::{render_frame, Rendered};
use crate::gpu::test_support::{scripted_runner, AvailabilityGuard};
use crate::gpu::{GpuJob, GpuKernel, GpuRunner, GpuUnavailable};
use crate::raster::DitherMode;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn scene_with(settings: &FbmCloudsSettings) -> FbmCloudsScene {
    FbmCloudsScene::new(
        settings,
        &SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources()),
    )
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
        contrast: 9999,
        block: 99,
        brightness: 9999,
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
            high.contrast,
            high.block,
            high.brightness,
            high.seed
        ),
        (300, 400, 40, 150, 4, 300, 4, 100, 999)
    );
    let low = FbmCloudsSettings {
        scale: 0,
        pan: -9999,
        octaves: 0,
        contrast: 0,
        block: 0,
        brightness: 0,
        ..defaults
    }
    .normalized();
    assert_eq!(
        (
            low.scale,
            low.pan,
            low.octaves,
            low.contrast,
            low.block,
            low.brightness
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
    assert_eq!(wild.controls().len(), 12);
}

#[test]
fn control_count_and_help_are_sane() {
    let rows = FbmCloudsSettings::default().controls();
    assert!((6..=12).contains(&rows.len()));
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), rows.len());
    assert!(ids.contains("render_backend"));
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
        contrast: 250,
        block: 4,
        dither: DitherPattern::White,
        brightness: 80,
        invert: true,
        seed: 321,
        render_backend: RenderBackend::Gpu,
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
    // The GPU option is selectable only while a GPU is available.
    let _guard = AvailabilityGuard::ready();
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
        "contrast",
        "block",
        "dither",
        "brightness",
        "invert",
        "seed",
        "render_backend",
    ] {
        assert!(object.contains_key(key), "{key}");
    }
    let partial: FbmCloudsSettings =
        serde_json::from_str(r#"{"render_backend":"gpu","dither":"white"}"#).unwrap();
    assert_eq!(partial.render_backend, RenderBackend::Gpu);
    assert_eq!(partial.dither, DitherPattern::White);
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
fn gpu_backend_reports_status_and_matches_software_shape() {
    let software = scene_with(&FbmCloudsSettings::default());
    assert_eq!(software.status(), None);
    assert_eq!(software.frames_per_second(), 8);
    assert!(!software.uses_cell_colors());
    let _guard = AvailabilityGuard::unavailable(GpuUnavailable::NotCompiled);
    let gpu = scene_with(&FbmCloudsSettings {
        render_backend: RenderBackend::Gpu,
        ..FbmCloudsSettings::default()
    });
    assert_eq!(
        gpu.status().as_deref(),
        Some("GPU support is not compiled into this build.")
    );
    let rendered = frame_of(
        &FbmCloudsSettings {
            render_backend: RenderBackend::Gpu,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        4.0,
    );
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn software_runs_in_slow_motion_relative_to_gpu_clock() {
    // Software at time 2t equals the GPU-clock scene at time t (scale 0.5 vs 1.0).
    let software = frame_of(&FbmCloudsSettings::default(), 40, 20, 20.0);
    let gpu = frame_of(
        &FbmCloudsSettings {
            render_backend: RenderBackend::Gpu,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        10.0,
    );
    assert_eq!(software.raster.dots, gpu.raster.dots);
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
fn brightness_scales_the_output_linearly() {
    let dim = frame_of(
        &FbmCloudsSettings {
            brightness: 20,
            ..FbmCloudsSettings::default()
        },
        40,
        20,
        2.0,
    );
    let bright = frame_of(
        &FbmCloudsSettings {
            brightness: 80,
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
        dither: DitherPattern::White,
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

fn backend_row(settings: &FbmCloudsSettings) -> crate::control::Control {
    settings
        .controls()
        .into_iter()
        .find(|row| row.id == "render_backend")
        .unwrap()
}

#[test]
fn gpu_option_is_disabled_while_unavailable_and_enabled_when_ready() {
    let settings = FbmCloudsSettings::default();
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
    let mut settings = FbmCloudsSettings::default();
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

/// CPU stand-in for a device: fills every dot with `fill`. Jobs at least
/// `hold_from_width` wide wait for `gate` (closed gate = error).
struct FakeRunner {
    fill: f32,
    hold_from_width: u32,
    gate: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    jobs: std::sync::Mutex<Vec<GpuJob>>,
}

impl GpuRunner for FakeRunner {
    fn adapter_name(&self) -> String {
        "Fake".to_owned()
    }

    fn run(&self, job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push(job.clone());
        }
        if job.width >= self.hold_from_width {
            let gate = self.gate.lock().map_err(|_| "gate poisoned".to_owned())?;
            // Bounded: a scene dropped while a later job waits would otherwise
            // deadlock its worker join (the sender outlives the scene).
            gate.recv_timeout(Duration::from_secs(10))
                .map_err(|_| "gate closed".to_owned())?;
        }
        out.fill(self.fill);
        Ok(())
    }
}

fn fake_runner(fill: f32, hold_from_width: u32) -> (Arc<FakeRunner>, std::sync::mpsc::Sender<()>) {
    let (release, gate) = std::sync::mpsc::channel();
    let runner = Arc::new(FakeRunner {
        fill,
        hold_from_width,
        gate: std::sync::Mutex::new(gate),
        jobs: std::sync::Mutex::new(Vec::new()),
    });
    (runner, release)
}

fn gpu_settings() -> FbmCloudsSettings {
    FbmCloudsSettings {
        render_backend: RenderBackend::Gpu,
        ..FbmCloudsSettings::default()
    }
}

fn render_until_status(
    scene: &mut FbmCloudsScene,
    width: u16,
    height: u16,
    wanted: &str,
) -> Rendered {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rendered = render_frame(scene, width, height, Duration::from_secs(1));
        if scene.status().as_deref() == Some(wanted) || Instant::now() > deadline {
            return rendered;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn gpu_frames_replace_software_once_they_arrive() {
    let _guard = AvailabilityGuard::ready();
    let without = scene_with(&gpu_settings());
    assert_eq!(
        without.status().as_deref(),
        Some("No GPU device was provided by the host; using software")
    );

    // Until the first GPU frame arrives the software picture is drawn.
    let (runner, release) = fake_runner(0.25, 1);
    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.gpu = Some(runner.clone());
    let mut scene = FbmCloudsScene::new(&gpu_settings(), &env);
    let before = render_frame(&mut scene, 20, 10, Duration::from_secs(1));
    assert_eq!(
        scene.status().as_deref(),
        Some("Starting the GPU renderer; using software until the first frame")
    );
    let expected_software = frame_of(&gpu_settings(), 20, 10, 1.0);
    assert_eq!(before.raster.dots, expected_software.raster.dots);
    assert!(before.raster.dots.iter().any(|dot| *dot > 0.0));

    assert!(release.send(()).is_ok());
    let after = render_until_status(&mut scene, 20, 10, "Rendering on GPU (Fake)");
    assert_eq!(scene.status().as_deref(), Some("Rendering on GPU (Fake)"));
    assert!(after.raster.dots.iter().all(|dot| *dot == 0.25));
    assert!(!scene.uses_cell_colors());
    let jobs = runner
        .jobs
        .lock()
        .map(|jobs| jobs.clone())
        .unwrap_or_default();
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|job| {
        job.kernel == GpuKernel::FbmClouds
            && (job.width, job.height) == (40, 40)
            && job.uniforms.len() == gpu::UNIFORM_COUNT
    }));
}

#[test]
fn a_gpu_frame_of_another_size_keeps_software_on_screen() {
    let _guard = AvailabilityGuard::ready();
    // Jobs from 60 dots wide (30 cells) on are held back by the gate, so the
    // only finished frame stays the 40x40 one.
    let (runner, release) = fake_runner(0.25, 60);
    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.gpu = Some(runner);
    let mut scene = FbmCloudsScene::new(&gpu_settings(), &env);
    let small = render_until_status(&mut scene, 20, 10, "Rendering on GPU (Fake)");
    assert!(small.raster.dots.iter().all(|dot| *dot == 0.25));

    let large = render_frame(&mut scene, 30, 12, Duration::from_secs(1));
    let software = frame_of(&gpu_settings(), 30, 12, 1.0);
    assert_eq!(large.raster.dots, software.raster.dots);
    assert_eq!(
        scene.status().as_deref(),
        Some("Starting the GPU renderer; using software until the first frame")
    );
    drop(release);
}

#[test]
fn failing_gpu_keeps_drawing_software_and_reports_the_error() {
    let _guard = AvailabilityGuard::ready();
    let mut env = SceneEnv::for_test(std::env::temp_dir(), crate::resources::test_resources());
    env.gpu = Some(scripted_runner(true));
    let mut failing = FbmCloudsScene::new(&gpu_settings(), &env);
    let wanted = "GPU error: fake kernel not ported; using software";
    let rendered = render_until_status(&mut failing, 20, 10, wanted);
    assert_eq!(failing.status().as_deref(), Some(wanted));
    assert!(rendered.raster.dots.iter().any(|dot| *dot > 0.0));
}

#[test]
fn gpu_job_uniforms_follow_the_settings() {
    use gpu::{
        build_job, UNIFORM_COUNT, U_BLOCK, U_BRIGHTNESS, U_CONTRAST, U_DITHER, U_FLIP,
        U_INVERSE_HEIGHT, U_INVERT, U_OCTAVES, U_PAN, U_PHASE, U_SCALE, U_SEED, U_WARP,
    };
    let settings = FbmCloudsSettings {
        scale: 200,
        drift: 200,
        pan: -20,
        warp: 50,
        octaves: 3,
        contrast: 150,
        block: 3,
        dither: DitherPattern::White,
        brightness: 60,
        invert: true,
        seed: 42,
        render_backend: RenderBackend::Gpu,
    };
    let job = build_job(&settings, 80, 40, 8.0);
    assert_eq!(job.kernel, GpuKernel::FbmClouds);
    assert_eq!((job.width, job.height), (80, 40));
    assert_eq!(job.uniforms.len(), UNIFORM_COUNT);
    let values = &job.uniforms;
    assert_eq!(values[U_BLOCK], 3.0);
    assert_eq!(values[U_OCTAVES], 3.0);
    assert_eq!(values[U_WARP], 0.5);
    // GPU clock is real time: 8 s * 0.1875 * 2.0
    assert!((values[U_PHASE] - 3.0).abs() < 1e-6);
    assert!((values[U_PAN] - (8.0 * -20.0 * 0.0025)).abs() < 1e-6);
    assert!((values[U_SCALE] - 6.0).abs() < 1e-6);
    assert_eq!(values[U_CONTRAST], 1.5);
    assert_eq!(values[U_BRIGHTNESS], 0.6);
    assert_eq!(values[U_INVERT], 1.0);
    assert_eq!(values[U_DITHER], 2.0);
    assert_eq!(values[U_SEED], 42.0);
    assert!((values[U_FLIP] - -(1.0 + 0.0625 * 2.0)).abs() < 1e-6);
    assert!((values[U_INVERSE_HEIGHT] - 0.025).abs() < 1e-6);
    assert!(values[13..].iter().all(|value| *value == 0.0));
    // Deterministic.
    assert_eq!(job, build_job(&settings, 80, 40, 8.0));
    // Every dither choice maps to its own index.
    let index_of = |dither| {
        build_job(
            &FbmCloudsSettings {
                dither,
                ..settings.clone()
            },
            80,
            40,
            0.0,
        )
        .uniforms[U_DITHER]
    };
    assert_eq!(index_of(DitherPattern::Bayer), 0.0);
    assert_eq!(index_of(DitherPattern::Gradient), 1.0);
}

#[test]
fn gpu_job_uniforms_are_clamped_to_the_documented_ranges() {
    use gpu::{build_job, U_BLOCK, U_BRIGHTNESS, U_CONTRAST, U_OCTAVES, U_SCALE, U_SEED, U_WARP};
    let wild = FbmCloudsSettings {
        scale: 9_999,
        drift: 9_999,
        pan: 999,
        warp: 9_999,
        octaves: 99,
        contrast: 9_999,
        block: 99,
        brightness: 0,
        seed: 100_000,
        ..FbmCloudsSettings::default()
    };
    let values = build_job(&wild, 10, 10, 1.0).uniforms;
    assert_eq!(values[U_SCALE], 9.0);
    assert_eq!(values[U_WARP], 1.5);
    assert_eq!(values[U_OCTAVES], 4.0);
    assert_eq!(values[U_CONTRAST], 3.0);
    assert_eq!(values[U_BLOCK], 4.0);
    assert_eq!(values[U_BRIGHTNESS], 0.05);
    assert_eq!(values[U_SEED], 999.0);
    // A zero-sized raster never divides by zero.
    let empty = build_job(&FbmCloudsSettings::default(), 0, 0, 0.0);
    assert!(empty.uniforms.iter().all(|value| value.is_finite()));
}

#[test]
fn saved_gpu_choice_renders_software_and_reports_the_summary() {
    let _guard = AvailabilityGuard::unavailable(GpuUnavailable::Checking);
    let settings = FbmCloudsSettings {
        render_backend: RenderBackend::Gpu,
        ..FbmCloudsSettings::default()
    };
    let scene = scene_with(&settings);
    assert_eq!(
        scene.status().as_deref(),
        Some("Checking for a usable GPU...")
    );
}
