// R06. Frozen R04 preparation/shader; pond layout and live water stay unchanged.
use super::*;
use crate::background_animation::{AnimationFrame, DitherMode};
use std::time::Duration;

fn reference_prepare_pond(raster: &mut Raster, settings: QuietPondSettings) {
    let aspect = raster.aspect();
    let pads = pond_pads(settings, aspect);
    raster.field(|u, v| reference_pond_light(&pads, aspect, u, v));
}

fn reference_pond_light(pads: &[(f32, f32, f32, f32)], aspect: f32, u: f32, v: f32) -> f32 {
    let mut light: f32 = 0.0;
    for &(cx, cy, radius, angle) in pads {
        let dx = (u - cx) * aspect;
        let dy = (v - cy) * 1.13;
        let distance = dx.hypot(dy) / radius;
        if distance > 1.08 {
            continue;
        }
        let theta = dy.atan2(dx);
        let relative = (theta - angle + PI).rem_euclid(TAU) - PI;
        if relative.abs() < 0.22 && distance > 0.10 {
            continue;
        }
        let edge = (1.0 - (distance - 1.0).abs() / 0.085).max(0.0) * 0.75;
        let body = if distance < 1.0 {
            0.11 + (1.0 - distance) * 0.08
        } else {
            0.0
        };
        let vein = if distance > 0.13 && distance < 0.89 {
            smoothstep(0.994, 1.0, (theta * 9.0 + angle).cos()) * 0.36
        } else {
            0.0
        };
        let leaf = edge.max(body + vein);
        // The front body is opaque even where its own veins are dim. Outside
        // the body, retain the bright rim without erasing uncovered water.
        light = if distance < 1.0 {
            leaf
        } else {
            light.max(leaf)
        };
    }
    light
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
        reference_prepare_pond(&mut frame.raster, settings.quiet_pond);
        let base = frame.raster.dots.clone();
        let columns = water_columns(frame.raster.width, 13.0, 35.0);
        let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0;
        quiet_pond(
            &mut frame.raster,
            settings.quiet_pond,
            seconds as f32,
            &base,
            &columns,
        );
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
fn r06_prepared_base_matches_r04_with_overlap_and_both_layouts() {
    for (width, height) in [
        (0, 0),
        (0, 7),
        (9, 0),
        (2, 4),
        (31, 17),
        (160, 96),
        (320, 200),
    ] {
        for (pad_count, pad_size_percent) in [(3, 50), (7, 100), (64, 175)] {
            for natural_placement in [false, true] {
                let settings = QuietPondSettings {
                    pad_count,
                    pad_size_percent,
                    natural_placement,
                    ..Default::default()
                };
                let mut actual = Raster::default();
                actual.resize(width, height);
                actual.dots.fill(f32::NAN);
                let mut expected = Raster::default();
                expected.resize(width, height);
                reference_prepare_pond(&mut expected, settings);
                prepare_pond(&mut actual, settings);
                assert_eq!(actual.dots.len(), expected.dots.len());
                for (index, (a, b)) in actual.dots.iter().zip(&expected.dots).enumerate() {
                    assert_eq!(
                        a.to_bits(),
                        b.to_bits(),
                        "{settings:?} {width}x{height} dot {index}"
                    );
                }
            }
        }
    }
}

#[test]
fn r06_row_filter_removes_work_without_reordering_remaining_leaves() {
    let settings = QuietPondSettings::default();
    let (width, height) = (320, 200);
    let aspect = width as f32 / height as f32;
    let pads = pond_pads(settings, aspect);
    let mut candidates = Vec::with_capacity(pads.len());
    let pointer = candidates.as_ptr();
    let capacity = candidates.capacity();
    let mut candidate_visits = 0;
    for y in 0..height {
        let v = (y as f32 + 0.5) / height as f32;
        pond_row_candidates(&pads, v, &mut candidates);
        candidate_visits += candidates.len() * width;
        assert_eq!(candidates.as_ptr(), pointer);
        assert_eq!(candidates.capacity(), capacity);
        let mut cursor = 0;
        for candidate in &candidates {
            while &pads[cursor] != candidate {
                cursor += 1;
            }
            cursor += 1;
        }
        for pad in &pads {
            let &(cx, cy, radius, _) = pad;
            if candidates.contains(pad) {
                continue;
            }
            // Check every discarded pad at every real x, using the original
            // shader's exact eligibility expression, not the culling expression.
            for x in 0..width {
                let u = (x as f32 + 0.5) / width as f32;
                let dx = (u - cx) * aspect;
                let dy = (v - cy) * 1.13;
                assert!(dx.hypot(dy) / radius > 1.08);
                assert_eq!(
                    reference_pond_light(&[*pad], aspect, u, v).to_bits(),
                    0.0_f32.to_bits()
                );
            }
        }
    }
    assert!(candidate_visits > 0);
    assert!(candidate_visits < width * height * pads.len());
}

#[test]
fn r06_support_boundaries_notches_and_opaque_overlap_are_unchanged() {
    let pads = [
        (0.4, 0.45, 0.11, 0.0),
        (0.5, 0.50, 0.15, PI),
        (0.6, 0.52, 0.09, TAU * 0.7),
    ];
    let mut candidates = Vec::new();
    for &(cx, cy, radius, _) in &pads {
        for relative_y in [-2.01_f32, -2.0, -1.08, -1.0, 0.0, 1.0, 1.08, 2.0, 2.01] {
            let center = cy + relative_y * radius / 1.13;
            for v in [center.next_down(), center, center.next_up()] {
                pond_row_candidates(&pads, v, &mut candidates);
                for u in [cx - radius, cx, cx + radius, 0.0, 0.5, 1.0] {
                    assert_eq!(
                        pond_light(&candidates, 1.0, u, v).to_bits(),
                        reference_pond_light(&pads, 1.0, u, v).to_bits(),
                    );
                }
            }
        }
    }
}

#[test]
fn r06_frame_oracle_keys_and_transitions() {
    let mut actual = AnimationFrame::default();
    let mut settings = AnimationSettings {
        kind: AnimationKind::QuietPond,
        ..Default::default()
    };
    let elapsed = Duration::from_secs(7);
    for revision in 0..6 {
        let previous = actual.scene_cache.preparations;
        match revision {
            0 => {}
            1 => settings.quiet_pond.pad_count = 64,
            2 => settings.quiet_pond.pad_size_percent = 175,
            3 => settings.quiet_pond.natural_placement = true,
            4 => settings.quiet_pond.ripple_strength_percent = 0,
            _ => settings.quiet_pond.drift_percent = 200,
        }
        actual.render(&settings, 31, 9, elapsed);
        assert_eq!(actual.scene_cache.preparations, previous + 1);
        assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    }
    let previous = actual.scene_cache.preparations;
    for (width, height, millis) in [
        (31, 9, 33),
        (31, 9, 0),
        (9, 31, 7000),
        (0, 9, 0),
        (9, 0, 0),
        (31, 9, 7000),
    ] {
        let elapsed = Duration::from_millis(millis);
        actual.render(&settings, width, height, elapsed);
        assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
    }
    assert!(actual.scene_cache.preparations > previous);
    let previous = actual.scene_cache.preparations;
    let renders = actual.geometry_render_count;
    settings.hue_degrees = 33;
    settings.density_percent = 100;
    settings.dither = DitherMode::Stippled;
    actual.render(&settings, 31, 9, elapsed);
    assert_eq!(actual.scene_cache.preparations, previous);
    assert_eq!(actual.geometry_render_count, renders);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    actual.load_packed_cells(31, 9, &vec![0xff; 31 * 9]);
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    actual.render(&AnimationSettings::default(), 31, 9, elapsed);
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    let large = AnimationSettings {
        kind: AnimationKind::QuietPond,
        ..Default::default()
    };
    actual.render(&large, 240, 80, elapsed);
    assert_frame_bits(&actual, &reference_frame(&large, 240, 80, elapsed));
}
