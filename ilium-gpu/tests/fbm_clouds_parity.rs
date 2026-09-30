//! Parity of the GPU `FbmClouds` kernel against the software scene, on a real
//! adapter. Skips (passes, with a printed note) when no usable GPU exists.
//!
//! Both scenes are built through `AmbientSettings::create_scene` exactly like
//! the host does. The software path runs its clock at half speed (slow-mo) and
//! the GPU path in real time, so the software scene is rendered at twice the
//! `Frame::time` of the GPU scene; both then evaluate the same field.
//!
//! "Lit" is decided against half of the scene brightness: hard-lit dots are
//! `0.85 * brightness` or more and unlit dots at most `0.15 * brightness`, so
//! that threshold is the host-like 0.5 split of the normalised intensity.
#![cfg(feature = "gpu")]

use ilium_ambient::debug::{render_frame, Rendered};
use ilium_ambient::gpu::{gpu_availability, GpuAvailability};
use ilium_ambient::{AmbientKind, AmbientSettings, ControlValue, SceneEnv};
use std::time::{Duration, Instant};

/// Largest accepted fraction of dots whose lit decision differs, per case.
/// Measured worst case is far below this (see the printed `parity` lines); the
/// slack covers other drivers' f32 rounding (fma contraction, `sqrt` accuracy)
/// flipping dots whose tone sits on its dither threshold.
const MAX_DIFFERING_FRACTION: f64 = 0.02;
/// Largest accepted mean absolute intensity difference, normalised by the
/// scene brightness.
const MAX_MEAN_ABSOLUTE_DIFFERENCE: f64 = 0.01;

type ControlChange = (&'static str, ControlValue);

struct Case {
    name: &'static str,
    cells_width: u16,
    cells_height: u16,
    gpu_time_millis: u64,
    changes: Vec<ControlChange>,
}

struct Metrics {
    differing_fraction: f64,
    mean_absolute_difference: f64,
    dot_count: usize,
}

fn wait_for_probe() -> Option<std::sync::Arc<dyn ilium_ambient::gpu::GpuRunner>> {
    ilium_gpu::start_probe();
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        match gpu_availability() {
            GpuAvailability::Ready { adapter } => {
                println!("parity adapter: {adapter}");
                return ilium_gpu::runner();
            }
            GpuAvailability::Unavailable(reason)
                if reason != ilium_ambient::gpu::GpuUnavailable::Checking =>
            {
                println!("SKIP: no usable GPU adapter ({})", reason.summary());
                return None;
            }
            GpuAvailability::Unavailable(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    println!("SKIP: GPU probe did not finish within 60 s");
    None
}

fn settings_for(changes: &[ControlChange], use_gpu: bool) -> Result<AmbientSettings, String> {
    let mut settings = AmbientSettings::default();
    for (id, value) in changes {
        settings.set_control(AmbientKind::FbmClouds, id, value.clone())?;
    }
    if use_gpu {
        settings.set_control(
            AmbientKind::FbmClouds,
            "render_backend",
            ControlValue::Index(1),
        )?;
    }
    Ok(settings)
}

fn brightness_of(changes: &[ControlChange]) -> f32 {
    changes
        .iter()
        .find_map(|(id, value)| match (*id, value) {
            ("brightness", ControlValue::Number(percent)) => Some(*percent as f32 / 100.0),
            _ => None,
        })
        .unwrap_or(0.35)
}

/// Renders the software frame and the (waited-for) GPU frame of one case.
fn render_pair(
    runner: &std::sync::Arc<dyn ilium_ambient::gpu::GpuRunner>,
    case: &Case,
) -> Result<(Rendered, Rendered), String> {
    let mut env = SceneEnv::for_test(std::env::temp_dir());
    let software_settings = settings_for(&case.changes, false)?;
    let mut software_scene = software_settings.create_scene(AmbientKind::FbmClouds, &env);
    let software = render_frame(
        software_scene.as_mut(),
        case.cells_width,
        case.cells_height,
        Duration::from_millis(case.gpu_time_millis * 2),
    );

    env.gpu = Some(runner.clone());
    let gpu_settings = settings_for(&case.changes, true)?;
    let mut gpu_scene = gpu_settings.create_scene(AmbientKind::FbmClouds, &env);
    let gpu_time = Duration::from_millis(case.gpu_time_millis);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let gpu = render_frame(
            gpu_scene.as_mut(),
            case.cells_width,
            case.cells_height,
            gpu_time,
        );
        let status = gpu_scene.status().unwrap_or_default();
        if status.starts_with("Rendering on GPU") {
            return Ok((software, gpu));
        }
        if Instant::now() > deadline {
            return Err(format!(
                "{}: GPU never rendered, status: {status}",
                case.name
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn measure(software: &Rendered, gpu: &Rendered, brightness: f32) -> Metrics {
    let dot_count = software.raster.dots.len();
    let mut differing = 0usize;
    let mut absolute_sum = 0.0f64;
    for (soft_dot, gpu_dot) in software.raster.dots.iter().zip(&gpu.raster.dots) {
        let soft_lit = *soft_dot / brightness > 0.5;
        let gpu_lit = *gpu_dot / brightness > 0.5;
        differing += usize::from(soft_lit != gpu_lit);
        absolute_sum += f64::from((soft_dot - gpu_dot).abs() / brightness);
    }
    Metrics {
        differing_fraction: differing as f64 / dot_count as f64,
        mean_absolute_difference: absolute_sum / dot_count as f64,
        dot_count,
    }
}

fn number(id: &'static str, value: i32) -> ControlChange {
    (id, ControlValue::Number(value))
}

fn index(id: &'static str, value: usize) -> ControlChange {
    (id, ControlValue::Index(value))
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let sizes = [(40u16, 20u16), (100, 40), (7, 3)];
    let times = [0u64, 3_700, 61_250];
    let dithers = [("bayer", 0usize), ("gradient", 1), ("white", 2)];
    for (cells_width, cells_height) in sizes {
        for (dither_name, dither_index) in dithers {
            for gpu_time_millis in times {
                cases.push(Case {
                    name: dither_name,
                    cells_width,
                    cells_height,
                    gpu_time_millis,
                    changes: vec![index("dither", dither_index)],
                });
            }
        }
    }
    let variants: Vec<(&'static str, Vec<ControlChange>)> = vec![
        (
            "block 1, white",
            vec![index("block", 0), index("dither", 2), number("seed", 7)],
        ),
        (
            "block 3, gradient, invert",
            vec![
                index("block", 2),
                index("dither", 1),
                ("invert", ControlValue::Bool(true)),
            ],
        ),
        (
            "block 4, bayer, seed 999",
            vec![index("block", 3), number("seed", 999)],
        ),
        (
            "octaves 2, warp 0, scale 250",
            vec![index("octaves", 0), number("warp", 0), number("scale", 250)],
        ),
        (
            "octaves 3, warp 150, contrast 300, pan -20, drift 400",
            vec![
                index("octaves", 1),
                number("warp", 150),
                number("contrast", 300),
                number("pan", -20),
                number("drift", 400),
            ],
        ),
        (
            "scale 50, contrast 50, brightness 100",
            vec![
                number("scale", 50),
                number("contrast", 50),
                number("brightness", 100),
            ],
        ),
        (
            "white, seed 123, block 2, invert",
            vec![
                index("dither", 2),
                number("seed", 123),
                ("invert", ControlValue::Bool(true)),
            ],
        ),
    ];
    for (variant_index, (name, changes)) in variants.into_iter().enumerate() {
        let (cells_width, cells_height) = sizes[variant_index % sizes.len()];
        cases.push(Case {
            name,
            cells_width,
            cells_height,
            gpu_time_millis: times[variant_index % times.len()] + 1_000,
            changes,
        });
    }
    cases
}

#[test]
fn gpu_fbm_clouds_match_the_software_scene() {
    let Some(runner) = wait_for_probe() else {
        return;
    };
    let mut worst_fraction = 0.0f64;
    let mut worst_mean = 0.0f64;
    let mut total_dots = 0usize;
    let mut weighted_differing = 0.0f64;
    let case_list = cases();
    for case in &case_list {
        let (software, gpu) = match render_pair(&runner, case) {
            Ok(pair) => pair,
            Err(message) => panic!("{message}"),
        };
        assert_eq!(software.raster.dots.len(), gpu.raster.dots.len());
        let brightness = brightness_of(&case.changes);
        let metrics = measure(&software, &gpu, brightness);
        println!(
            "parity {:<52} {:>3}x{:<3} t={:>6}ms dots={:>6} differing={:.4}% mean_abs_diff={:.6}",
            case.name,
            case.cells_width,
            case.cells_height,
            case.gpu_time_millis,
            metrics.dot_count,
            metrics.differing_fraction * 100.0,
            metrics.mean_absolute_difference
        );
        assert!(
            metrics.differing_fraction <= MAX_DIFFERING_FRACTION,
            "{} differs in {:.3}% of dots",
            case.name,
            metrics.differing_fraction * 100.0
        );
        assert!(
            metrics.mean_absolute_difference <= MAX_MEAN_ABSOLUTE_DIFFERENCE,
            "{} mean absolute difference {:.5}",
            case.name,
            metrics.mean_absolute_difference
        );
        assert!(
            software.raster.dots.iter().any(|dot| *dot > 0.0),
            "{} drew nothing",
            case.name
        );
        worst_fraction = worst_fraction.max(metrics.differing_fraction);
        worst_mean = worst_mean.max(metrics.mean_absolute_difference);
        total_dots += metrics.dot_count;
        weighted_differing += metrics.differing_fraction * metrics.dot_count as f64;
    }
    // Sensitivity control: a GPU frame from a different time must NOT match,
    // proving the comparison can fail.
    let shifted = Case {
        name: "control",
        cells_width: 40,
        cells_height: 20,
        gpu_time_millis: 3_700,
        changes: Vec::new(),
    };
    let (software, _) =
        render_pair(&runner, &shifted).unwrap_or_else(|message| panic!("{message}"));
    let other_time = Case {
        gpu_time_millis: 13_700,
        ..shifted
    };
    let (_, gpu_elsewhere) =
        render_pair(&runner, &other_time).unwrap_or_else(|message| panic!("{message}"));
    let control = measure(&software, &gpu_elsewhere, 0.35);
    println!(
        "parity control (10 s apart): differing={:.2}%",
        control.differing_fraction * 100.0
    );
    assert!(control.differing_fraction > 0.05, "control did not differ");
    println!(
        "parity summary: cases={} worst_differing={:.4}% overall_differing={:.4}% worst_mean_abs_diff={:.6}",
        case_list.len(),
        worst_fraction * 100.0,
        weighted_differing / total_dots as f64 * 100.0,
        worst_mean
    );
}

fn ascii_of(rendered: &Rendered, brightness: f32) -> Vec<String> {
    const RAMP: &[u8] = b" .:-=+*#%@";
    let width = rendered.raster.width;
    (0..usize::from(rendered.height))
        .map(|cell_y| {
            (0..usize::from(rendered.width))
                .map(|cell_x| {
                    let mut lit = 0usize;
                    for dy in 0..4 {
                        for dx in 0..2 {
                            let dot =
                                rendered.raster.dots[(cell_y * 4 + dy) * width + cell_x * 2 + dx];
                            lit += usize::from(dot / brightness > 0.5);
                        }
                    }
                    RAMP[lit * (RAMP.len() - 1) / 8] as char
                })
                .collect()
        })
        .collect()
}

/// Prints software | GPU for one frame: `cargo test -p ilium-gpu --features gpu
/// -- --ignored --nocapture ascii`.
#[test]
#[ignore = "prints a picture for a human to compare"]
fn ascii_side_by_side_software_and_gpu() {
    let Some(runner) = wait_for_probe() else {
        return;
    };
    let case = Case {
        name: "default",
        cells_width: 56,
        cells_height: 22,
        gpu_time_millis: 9_000,
        changes: Vec::new(),
    };
    let (software, gpu) = match render_pair(&runner, &case) {
        Ok(pair) => pair,
        Err(message) => panic!("{message}"),
    };
    let left = ascii_of(&software, 0.35);
    let right = ascii_of(&gpu, 0.35);
    println!("{:<56} | GPU", "software");
    for (soft_row, gpu_row) in left.iter().zip(&right) {
        println!("{soft_row} | {gpu_row}");
    }
}
