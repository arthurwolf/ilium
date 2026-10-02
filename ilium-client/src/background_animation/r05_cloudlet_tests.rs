// R05 tests. The reference bodies below are copied from the immutable R04
// scenes.rs; only their function names changed. Do not optimize the oracle.
use super::*;
use crate::background_animation::{AnimationFrame, CloudletSettings, DitherMode};
use std::time::Duration;

fn reference_cloudlets(raster: &mut Raster, settings: &AnimationSettings, time: f32) {
    // AnimationFrame normalizes this control to 2..=10. Specializing the small
    // source loop retains its addition order and exposes a fixed loop bound.
    match settings.cloudlets.form_count {
        2 => reference_cloudlets_with_sources::<2>(raster, settings, time),
        3 => reference_cloudlets_with_sources::<3>(raster, settings, time),
        4 => reference_cloudlets_with_sources::<4>(raster, settings, time),
        5 => reference_cloudlets_with_sources::<5>(raster, settings, time),
        6 => reference_cloudlets_with_sources::<6>(raster, settings, time),
        7 => reference_cloudlets_with_sources::<7>(raster, settings, time),
        8 => reference_cloudlets_with_sources::<8>(raster, settings, time),
        9 => reference_cloudlets_with_sources::<9>(raster, settings, time),
        _ => reference_cloudlets_with_sources::<10>(raster, settings, time),
    }
}

fn reference_cloudlets_with_sources<const SOURCE_COUNT: usize>(
    raster: &mut Raster,
    settings: &AnimationSettings,
    time: f32,
) {
    let controls = settings.cloudlets;
    let aspect = raster.aspect();
    let orbit = f32::from(controls.orbit_radius_percent) / 100.0;
    let radius_scale = f32::from(controls.form_size_percent) / 100.0;
    let cohesion = f32::from(controls.cohesion_percent) / 100.0;
    let cohesion_scale = cohesion.sqrt();
    let field_low = 1.10 / cohesion_scale;
    let field_high = 1.36 / cohesion_scale;
    let mut sources = [(0.0, 0.0, 0.0, 0.0); SOURCE_COUNT];
    for (index, source) in sources.iter_mut().enumerate() {
        let seed = hash(index as i32, 459);
        let phase = index as f32 * TAU / SOURCE_COUNT as f32;
        let radius = (0.065 + seed * 0.025) * radius_scale;
        let squared_radius = radius * radius;
        *source = (
            0.5 + (time * 0.19 + phase).cos() * 0.25 * orbit,
            0.49 + (time * 0.23 + phase * 1.3).sin() * 0.19 * orbit,
            squared_radius,
            squared_radius * (0.18 + cohesion * 0.08),
        );
    }
    raster.field(|u, v| {
        let mut field = 0.0;
        for &(cx, cy, squared_radius, softened_radius) in &sources {
            let distance_squared = ((u - cx) * aspect).powi(2) + (v - cy).powi(2);
            field += squared_radius / (distance_squared + softened_radius);
        }
        smoothstep(field_low, field_high, field) * 0.91
    });
}

pub(in crate::background_animation) fn reference_frame(
    settings: &AnimationSettings,
    width: u16,
    height: u16,
    elapsed: Duration,
) -> AnimationFrame {
    let settings = settings.normalized();
    let mut frame = AnimationFrame::default();
    frame.resize(width, height);
    if width > 0 && height > 0 {
        let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0;
        reference_cloudlets(&mut frame.raster, &settings, seconds as f32);
        frame.pack(settings.density_percent, settings.dither);
    }
    frame
}

fn assert_frame_bits(actual: &AnimationFrame, expected: &AnimationFrame) {
    assert_eq!(
        (actual.width(), actual.height()),
        (expected.width(), expected.height())
    );
    assert_eq!(actual.raster.dots.len(), expected.raster.dots.len());
    for (index, (actual, expected)) in actual
        .raster
        .dots
        .iter()
        .zip(&expected.raster.dots)
        .enumerate()
    {
        assert_eq!(actual.to_bits(), expected.to_bits(), "dot {index}");
    }
    assert_eq!(actual.packed_cells(), expected.packed_cells());
}

#[test]
fn r05_full_resolution_oracle_all_source_counts_and_nonmonotone_times() {
    for (width, height) in [(1, 1), (15, 5), (80, 24)] {
        for form_count in 2..=10 {
            for controls in [
                CloudletSettings {
                    form_count,
                    ..Default::default()
                },
                CloudletSettings {
                    form_count,
                    form_size_percent: 50,
                    cohesion_percent: 25,
                    orbit_radius_percent: 25,
                },
                CloudletSettings {
                    form_count,
                    form_size_percent: 175,
                    cohesion_percent: 200,
                    orbit_radius_percent: 150,
                },
            ] {
                let settings = AnimationSettings {
                    kind: AnimationKind::Cloudlets,
                    cloudlets: controls,
                    ..Default::default()
                };
                let mut actual = AnimationFrame::default();
                for millis in [0, 33, 24000, 7000, 0, 7000] {
                    let elapsed = Duration::from_millis(millis);
                    actual.render(&settings, width, height, elapsed);
                    assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
                }
            }
        }
    }
    for (width, height) in [(160, 50), (240, 80)] {
        for density_percent in [25, 60, 100] {
            for dither in [DitherMode::Ordered, DitherMode::Stippled] {
                let settings = AnimationSettings {
                    kind: AnimationKind::Cloudlets,
                    density_percent,
                    dither,
                    ..Default::default()
                };
                let mut actual = AnimationFrame::default();
                for millis in [0, 33, 7000] {
                    let elapsed = Duration::from_millis(millis);
                    actual.render(&settings, width, height, elapsed);
                    assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
                }
            }
        }
    }
}

#[test]
fn r05_odd_dot_geometry_and_poisoned_scratch_preserve_r04() {
    for (width, height) in [(0, 0), (0, 7), (9, 0), (1, 1), (31, 17)] {
        let settings = AnimationSettings {
            kind: AnimationKind::Cloudlets,
            ..Default::default()
        };
        let mut actual = Raster::default();
        actual.resize(width, height);
        let mut expected = Raster::default();
        expected.resize(width, height);
        let mut columns = vec![[f32::NAN; 10]; width];
        for time in [0.0, 7.0, -1.0, 0.0] {
            actual.dots.fill(f32::NAN);
            reference_cloudlets(&mut expected, &settings, time);
            cloudlets_prepared(&mut actual, &settings, time, &mut columns);
            assert_eq!(actual.dots.len(), expected.dots.len());
            for (a, b) in actual.dots.iter().zip(&expected.dots) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
    }
}

#[test]
fn r05_scratch_is_reused_but_every_live_frame_rewrites_it() {
    let settings = AnimationSettings {
        kind: AnimationKind::Cloudlets,
        ..Default::default()
    };
    let mut actual = AnimationFrame::default();
    actual.render(&settings, 31, 9, Duration::ZERO);
    let (pointer, capacity) = match &mut actual.scene_cache.prepared {
        PreparedScene::Cloudlets { columns } => {
            columns.fill([f32::NAN; 10]);
            (columns.as_ptr(), columns.capacity())
        }
        _ => panic!("Cloudlets must use its prepared workspace"),
    };
    for millis in [33, 7000, 1, 24000] {
        let elapsed = Duration::from_millis(millis);
        actual.raster.dots.fill(f32::NAN);
        actual.render(&settings, 31, 9, elapsed);
        assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
        assert_eq!(actual.scene_cache.preparations, 1);
        match &actual.scene_cache.prepared {
            PreparedScene::Cloudlets { columns } => {
                assert_eq!(columns.as_ptr(), pointer);
                assert_eq!(columns.capacity(), capacity);
                assert_eq!(columns.len(), 62);
            }
            _ => panic!("workspace lost"),
        }
    }
}

#[test]
fn r05_keys_repack_resize_ready_load_and_scene_switch() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Cloudlets,
        ..Default::default()
    };
    let mut actual = AnimationFrame::default();
    let elapsed = Duration::from_secs(7);
    actual.render(&settings, 31, 9, elapsed);
    let initial_preparations = actual.scene_cache.preparations;
    let initial_renders = actual.geometry_render_count;
    for (density, dither, hue) in [
        (25, DitherMode::Stippled, 30),
        (100, DitherMode::Ordered, 250),
    ] {
        settings.density_percent = density;
        settings.dither = dither;
        settings.hue_degrees = hue;
        actual.render(&settings, 31, 9, elapsed);
        assert_eq!(actual.scene_cache.preparations, initial_preparations);
        assert_eq!(actual.geometry_render_count, initial_renders);
        assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    }
    for field in 0..4 {
        let previous = actual.scene_cache.preparations;
        match field {
            0 => settings.cloudlets.form_count = 10,
            1 => settings.cloudlets.form_size_percent = 175,
            2 => settings.cloudlets.cohesion_percent = 25,
            _ => settings.cloudlets.orbit_radius_percent = 150,
        }
        actual.render(&settings, 31, 9, elapsed);
        assert_eq!(actual.scene_cache.preparations, previous + 1);
        assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    }
    let previous = actual.scene_cache.preparations;
    settings.speed_percent = 300;
    actual.render(&settings, 31, 9, elapsed);
    assert_eq!(actual.scene_cache.preparations, previous);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    actual.load_packed_cells(31, 9, &vec![0xff; 31 * 9]);
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    for (width, height) in [(9, 31), (0, 9), (9, 0), (31, 9)] {
        actual.render(&settings, width, height, elapsed);
        assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
    }
    let other = AnimationSettings {
        kind: AnimationKind::MoonlitWater,
        ..Default::default()
    };
    actual.render(&other, 31, 9, elapsed);
    assert!(matches!(
        &actual.scene_cache.prepared,
        PreparedScene::Moon { .. }
    ));
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
}
