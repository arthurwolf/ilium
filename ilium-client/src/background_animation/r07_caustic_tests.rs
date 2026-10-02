// R07. Scalar oracle copied from immutable R04; preserve its operation order.
use super::*;
use crate::background_animation::{AnimationFrame, DitherMode};
use std::time::Duration;

fn reference_caustic_texture(u: f32, v: f32) -> f32 {
    let sample_x = u * 8.0;
    let sample_y = v * 6.0;
    let cell_x = sample_x.floor() as i32;
    let cell_y = sample_y.floor() as i32;
    let mut nearest = f32::INFINITY;
    let mut second = f32::INFINITY;
    for gy in cell_y - 1..=cell_y + 1 {
        for gx in cell_x - 1..=cell_x + 1 {
            let site_x = gx as f32 + 0.22 + hash(gx.rem_euclid(8), gy.rem_euclid(6)) * 0.56;
            let site_y = gy as f32 + 0.22 + hash(gx.rem_euclid(8) + 88, gy.rem_euclid(6)) * 0.56;
            let distance = (sample_x - site_x).powi(2) + (sample_y - site_y).powi(2);
            if distance < nearest {
                second = nearest;
                nearest = distance;
            } else if distance < second {
                second = distance;
            }
        }
    }
    let separation = (second - nearest) / (nearest.sqrt() + second.sqrt() + 0.0001);
    1.0 - smoothstep(0.015, 0.072, separation)
}

fn reference_preparation(
    raster: &Raster,
    settings: &AnimationSettings,
) -> (Vec<StoneSurface>, Texture) {
    let stones = stone_layout(settings.stone_caustics);
    let mut surfaces = Vec::with_capacity(raster.dots.len());
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / raster.height as f32;
        for x in 0..raster.width {
            let u = (x as f32 + 0.5) / raster.width as f32;
            let mut surface =
                stone_surface(u, v, raster.aspect(), &stones, settings.stone_caustics);
            let seed = hash(x as i32 + 190, y as i32 + 370);
            if surface.height == 0.0
                && seed > 1.0 - f32::from(settings.stone_caustics.sand_percent) * 0.00006
            {
                surface.base = surface.base.max(0.28);
            }
            surfaces.push(surface);
        }
    }
    let texture = Texture::new(raster.width, raster.height, reference_caustic_texture);
    (surfaces, texture)
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
        let (surfaces, texture) = reference_preparation(&frame.raster, &settings);
        let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0;
        stone_caustics(
            &mut frame.raster,
            settings.stone_caustics,
            seconds as f32,
            &surfaces,
            &texture,
        );
        frame.pack(crate::background_animation::PackKey::of(&settings));
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
fn r07_site_coordinates_keep_translated_f32_rounding() {
    let sites = CausticSites::new();
    for (row, site_row) in sites.sites.iter().enumerate() {
        let gy = row as i32 - 1;
        for (column, &(site_x, site_y)) in site_row.iter().enumerate() {
            let gx = column as i32 - 1;
            assert_eq!(
                site_x.to_bits(),
                (gx as f32 + 0.22 + hash(gx.rem_euclid(8), gy.rem_euclid(6)) * 0.56).to_bits(),
            );
            assert_eq!(
                site_y.to_bits(),
                (gy as f32 + 0.22 + hash(gx.rem_euclid(8) + 88, gy.rem_euclid(6)) * 0.56).to_bits(),
            );
        }
    }
}

#[test]
fn r07_texture_matches_every_full_resolution_r04_sample() {
    let sites = CausticSites::new();
    for (width, height) in [(1, 1), (2, 4), (31, 17), (160, 96), (320, 200), (480, 320)] {
        let actual = Texture::new(width, height, |u, v| sites.sample(u, v));
        let expected = Texture::new(width, height, reference_caustic_texture);
        assert_eq!(actual.values.len(), expected.values.len());
        for (index, (a, b)) in actual.values.iter().zip(&expected.values).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "{width}x{height} texture sample {index}"
            );
        }
    }
}

#[test]
fn r07_lattice_boundaries_neighbors_and_fallback_keep_exact_samples() {
    let sites = CausticSites::new();
    let mut us = vec![-0.01, 0.0, 0.37, 1.0, 1.01];
    let mut vs = vec![-0.01, 0.0, 0.59, 1.0, 1.01];
    for cell in 1..8 {
        let u = cell as f32 / 8.0;
        us.extend([u.next_down(), u, u.next_up()]);
    }
    for cell in 1..6 {
        let v = cell as f32 / 6.0;
        vs.extend([v.next_down(), v, v.next_up()]);
    }
    for u in us {
        for &v in &vs {
            assert_eq!(
                sites.sample(u, v).to_bits(),
                reference_caustic_texture(u, v).to_bits(),
                "u={u}, v={v}",
            );
        }
    }
}

#[test]
fn r07_full_frame_oracle_and_all_preparation_keys() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::StoneCaustics,
        ..Default::default()
    };
    let mut actual = AnimationFrame::default();
    for (width, height) in [(1, 1), (31, 9), (80, 24), (160, 50), (240, 80)] {
        for millis in [0, 33, 7000, 0] {
            let elapsed = Duration::from_millis(millis);
            actual.render(&settings, width, height, elapsed);
            assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
        }
    }
    let elapsed = Duration::from_secs(7);
    for revision in 0..4 {
        let previous = actual.scene_cache.preparations;
        match revision {
            0 => settings.stone_caustics.dome_height_percent = 200,
            1 => settings.stone_caustics.caustic_scale_percent = 50,
            2 => settings.stone_caustics.small_stones_percent = 100,
            _ => settings.stone_caustics.sand_percent = 100,
        }
        actual.render(&settings, 31, 9, elapsed);
        assert_eq!(actual.scene_cache.preparations, previous + 1);
        assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    }
    let previous = actual.scene_cache.preparations;
    let renders = actual.geometry_render_count;
    settings.density_percent = 25;
    settings.dither = DitherMode::Stippled;
    settings.hue_degrees = 90;
    actual.render(&settings, 31, 9, elapsed);
    assert_eq!(actual.scene_cache.preparations, previous);
    assert_eq!(actual.geometry_render_count, renders);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    actual.load_packed_cells(31, 9, &vec![0xff; 31 * 9]);
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
    for (width, height) in [(0, 9), (9, 0), (9, 31), (31, 9)] {
        actual.render(&settings, width, height, elapsed);
        assert_frame_bits(&actual, &reference_frame(&settings, width, height, elapsed));
    }
    actual.render(&AnimationSettings::default(), 31, 9, elapsed);
    actual.render(&settings, 31, 9, elapsed);
    assert_frame_bits(&actual, &reference_frame(&settings, 31, 9, elapsed));
}
