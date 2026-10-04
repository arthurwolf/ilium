//! Scene-local tone-to-coverage conversion for the voxel landscape.
//! Coverage controls Braille shape; adjust() controls foreground RGB.
//! Neither stage reads or compensates for the host's density/dither settings.
use super::settings::VoxelLandscapeSettings;
use crate::{scene::Frame, style::ScenePalette};

fn luminance(raw: [f32; 3]) -> f32 {
    raw[0] * 0.2126 + raw[1] * 0.7152 + raw[2] * 0.0722
}

/// An artistic coverage transfer, not an sRGB decoding/encoding operation.
///
/// Continuous knots: (0, 0), (1/4, 13/64), (1/2, 55/64),
/// (3/4, 61/64), (1, 63/64). All slopes are positive.
/// The steep middle segment separates wood faces from each other and nearby
/// terrain. The highlight shoulder preserves bright-face variation without a
/// hard white plateau.
/// Zero stays zero: there is no minimum-dot floor or phase-dependent rescue.
fn dot_coverage(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let y = value.clamp(0.0, 1.0);
    if y <= 0.25 {
        0.8125 * y
    } else if y <= 0.5 {
        0.203125 + 2.625 * (y - 0.25)
    } else if y <= 0.75 {
        0.859375 + 0.375 * (y - 0.5)
    } else {
        0.953125 + 0.125 * (y - 0.75)
    }
}

pub fn composite(colors: &[[u8; 3]], frame: &mut Frame<'_>, settings: &VoxelLandscapeSettings) {
    composite_pixels(
        colors,
        None,
        frame,
        settings,
        true,
        &ScenePalette::default(),
    );
}

/// Selected artwork retains its original RGB (unless `palette` is provided,
/// which recolours it by brightness). Coverage comes from raster
/// alpha rather than treating an opaque black texel as an uncovered pixel.
pub fn composite_selected(
    colors: &[[u8; 3]],
    covered: &[bool],
    frame: &mut Frame<'_>,
    settings: &VoxelLandscapeSettings,
    palette: &ScenePalette,
) {
    composite_pixels(colors, Some(covered), frame, settings, false, palette);
}

fn composite_pixels(
    colors: &[[u8; 3]],
    covered: Option<&[bool]>,
    frame: &mut Frame<'_>,
    settings: &VoxelLandscapeSettings,
    pastel: bool,
    palette: &ScenePalette,
) {
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    frame.cell_colors.resize(width * height, [0; 3]);
    frame.cell_colors.fill([0; 3]);
    // Also clear an unmatched tail when a diagnostic supplies a short input.
    frame.raster.dots.fill(0.0);
    let dot_width = frame.raster.width;
    let dot_height = frame.raster.height;
    if width == 0 || height == 0 || dot_width == 0 || dot_height == 0 {
        return;
    }
    let mut sums = vec![[0.0_f32; 4]; width * height];
    for (index, (&rgb, dot)) in colors.iter().zip(frame.raster.dots.iter_mut()).enumerate() {
        // Canvas::new uses black for uncovered pixels. Do not turn air into dots.
        if covered.map_or(rgb == [0; 3], |mask| {
            !mask.get(index).copied().unwrap_or(false)
        }) {
            continue;
        }
        let pixel_y = index / dot_width;
        let cell_x = (index % dot_width) / 2;
        let cell_y = pixel_y / 4;
        if pixel_y >= dot_height || cell_x >= width || cell_y >= height {
            continue;
        }
        let raw = rgb.map(|value| f32::from(value) / 255.0);
        let intensity = dot_coverage(luminance(raw));
        *dot = intensity;
        let cell_index = cell_y * width + cell_x;
        let adjusted = if pastel {
            adjust(raw, settings)
        } else {
            adjust_channels(raw, settings, false)
        };
        // Follow the shared palette at the source: each pixel's adjusted colour
        // moves to the palette colour of equal brightness before cell averaging.
        let adjusted = if palette.is_provided() {
            let bytes = adjusted.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8);
            palette.recolor(bytes).map(|value| f32::from(value) / 255.0)
        } else {
            adjusted
        };
        // Keep expected-coverage weighting within the existing 2x4 cell.
        // Darker surfaces now contribute in proportion to their new coverage.
        for (channel, value) in adjusted.into_iter().enumerate() {
            sums[cell_index][channel] += value * intensity;
        }
        sums[cell_index][3] += intensity;
    }
    for (sum, color) in sums.iter().zip(frame.cell_colors.iter_mut()) {
        let weight = sum[3];
        if weight <= 0.0 {
            continue;
        }
        *color =
            std::array::from_fn(|channel| (sum[channel] / weight * 255.0).clamp(0.0, 255.0) as u8);
    }
}

// Preserve the supplied palette, hue, saturation and lightness semantics.
fn adjust(raw: [f32; 3], settings: &VoxelLandscapeSettings) -> [f32; 3] {
    adjust_channels(raw, settings, true)
}

fn adjust_channels(raw: [f32; 3], settings: &VoxelLandscapeSettings, pastel: bool) -> [f32; 3] {
    if !pastel
        && settings.color_mode == 1
        && settings.palette == 0
        && settings.hue_degrees == 180
        && settings.saturation_percent == 100
        && settings.lightness_percent == 100
    {
        return raw.map(|channel| channel.clamp(0.0, 1.0));
    }
    let tint = match settings.palette {
        1 => [0.12, -0.025, 0.06],
        2 => [-0.035, 0.035, 0.12],
        3 => [0.14, 0.05, -0.045],
        _ => [0.0; 3],
    };
    let gray = luminance(raw);
    let saturation = if settings.color_mode == 0 {
        0.0
    } else {
        settings.saturation_percent as f32 / 100.0
    };
    let lightness = settings.lightness_percent as f32 / 100.0;
    let hue = (settings.hue_degrees as f32 - 180.0) / 180.0 * 0.1;
    std::array::from_fn(|channel| {
        let hue_offset = match channel {
            0 => -hue,
            2 => hue,
            _ => 0.0,
        };
        // Tint is desaturated too: saturation=0 is genuinely achromatic.
        let color = gray + (raw[channel] - gray + tint[channel] + hue_offset) * saturation;
        let color = if pastel { color * 0.7 + 0.3 } else { color };
        (color * lightness).clamp(0.0, 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::super::catalog::{Face, Material, ALL_MATERIALS};
    use super::*;
    use crate::raster::{threshold, DitherMode, Raster};
    use std::time::{Duration, SystemTime};

    #[test]
    fn selected_texture_colors_are_not_lifted_or_desaturated_by_default() {
        let settings = VoxelLandscapeSettings::default();
        let raw = [0.12, 0.63, 0.29];
        assert_eq!(adjust_channels(raw, &settings, false), raw);
        assert_ne!(adjust(raw, &settings), raw);
        let mono = VoxelLandscapeSettings {
            color_mode: 0,
            ..settings
        };
        let output = adjust_channels(raw, &mono, false);
        assert_eq!(output[0], output[1]);
        assert_eq!(output[1], output[2]);
    }

    #[test]
    fn selected_palette_recolours_cells_and_none_is_unchanged() {
        let compose_with = |palette: &ScenePalette| {
            let mut raster = Raster::default();
            raster.resize(2, 4);
            let mut cell_colors = vec![[255; 3]];
            let mut frame = Frame {
                raster: &mut raster,
                cell_colors: &mut cell_colors,
                width: 1,
                height: 1,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: SystemTime::UNIX_EPOCH,
            };
            composite_selected(
                &[[200, 40, 20]; 8],
                &[true; 8],
                &mut frame,
                &VoxelLandscapeSettings::default(),
                palette,
            );
            cell_colors[0]
        };
        let none = compose_with(&ScenePalette::default());
        assert!((199..=200).contains(&none[0]));
        let provided = ScenePalette {
            stops: vec![[0, 0, 40], [0, 255, 255]],
            reverse: false,
            shift_percent: 0,
        };
        let changed = compose_with(&provided);
        assert_ne!(changed, none);
        assert!(changed[0] < 20 && changed[2] > 40);
    }

    #[test]
    fn selected_alpha_mask_prevents_stale_or_transparent_texel_colors() {
        let mut raster = Raster::default();
        raster.resize(2, 4);
        let mut cell_colors = vec![[255; 3]];
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut cell_colors,
            width: 1,
            height: 1,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        };
        composite_selected(
            &[[200, 40, 20]; 8],
            &[false; 8],
            &mut frame,
            &VoxelLandscapeSettings::default(),
            &ScenePalette::default(),
        );
        assert_eq!(frame.cell_colors.as_slice(), &[[0; 3]]);
        assert!(frame.raster.dots.iter().all(|&value| value == 0.));
        composite_selected(
            &[[200, 40, 20]; 8],
            &[true; 8],
            &mut frame,
            &VoxelLandscapeSettings::default(),
            &ScenePalette::default(),
        );
        assert!((199..=200).contains(&frame.cell_colors[0][0]));
        assert!((39..=40).contains(&frame.cell_colors[0][1]));
        assert!((19..=20).contains(&frame.cell_colors[0][2]));
    }

    fn legacy_coverage(y: f32) -> f32 {
        ((y - 0.12) * 1.3).clamp(0.0, 0.92)
    }

    fn previous_coverage(y: f32) -> f32 {
        if y <= 0.25 {
            1.5 * y
        } else if y <= 0.5 {
            0.375 + 1.625 * (y - 0.25)
        } else if y <= 0.75 {
            0.78125 + 0.625 * (y - 0.5)
        } else {
            0.9375 + 0.1875 * (y - 0.75)
        }
    }

    fn compose(
        input: &[[u8; 3]],
        width: u16,
        height: u16,
        settings: &VoxelLandscapeSettings,
    ) -> (Raster, Vec<[u8; 3]>) {
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        raster.dots.fill(1.0);
        let mut colors = vec![[255; 3]; usize::from(width) * usize::from(height)];
        composite(
            input,
            &mut Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width,
                height,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: SystemTime::UNIX_EPOCH,
            },
            settings,
        );
        (raster, colors)
    }

    fn lit_mask(raster: &Raster, density: f32, mode: DitherMode) -> Vec<bool> {
        raster
            .dots
            .iter()
            .enumerate()
            .map(|(i, &dot)| dot * density > threshold(i % raster.width, i / raster.width, mode))
            .collect()
    }

    fn uniform_ordered_count(intensity: f32) -> usize {
        (0..8)
            .flat_map(|y| {
                (0..8).map(move |x| intensity * 0.6 > threshold(x, y, DitherMode::Ordered))
            })
            .filter(|&lit| lit)
            .count()
    }

    fn uniform_stippled_count(intensity: f32) -> usize {
        (0..64)
            .flat_map(|y| {
                (0..64).map(move |x| intensity * 0.6 > threshold(x, y, DitherMode::Stippled))
            })
            .filter(|&lit| lit)
            .count()
    }

    #[test]
    fn coverage_is_continuous_bounded_monotone_and_not_a_floor() {
        for (input, expected) in [
            (0.0, 0.0),
            (0.25, 0.203125),
            (0.5, 0.859375),
            (0.75, 0.953125),
            (1.0, 0.984375),
        ] {
            assert_eq!(dot_coverage(input), expected);
        }
        for knot in [0.25, 0.5, 0.75] {
            let delta = 1.0 / 65536.0;
            let low = dot_coverage(knot - delta);
            let high = dot_coverage(knot + delta);
            assert!(high > low && high - low < 4.0 * delta);
        }
        let mut previous = -1.0;
        for step in 0..=4096 {
            let y = step as f32 / 4096.0;
            let value = dot_coverage(y);
            assert!(value.is_finite() && (0.0..=0.984375).contains(&value));
            assert!(value > previous);
            assert!(value >= legacy_coverage(y));
            previous = value;
        }
        assert!(dot_coverage(0.001) < 0.002);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(dot_coverage(invalid), 0.0);
        }
        assert_eq!(dot_coverage(2.0), 0.984375);
    }

    #[test]
    fn dark_roof_faces_have_more_separation_in_both_dither_modes() {
        let roof = Material::DarkPlanks.style().color;
        let tones = [1.0_f32, 0.78, 0.58].map(|shade| {
            // Match render::cube_instance's byte quantization before composite.
            let rgb = roof.map(|channel| (f32::from(channel) * shade) as u8);
            luminance(rgb.map(|channel| f32::from(channel) / 255.0))
        });
        for count in [
            uniform_ordered_count as fn(f32) -> usize,
            uniform_stippled_count,
        ] {
            let old = tones.map(|y| count(previous_coverage(y)));
            let new = tones.map(|y| count(dot_coverage(y)));
            assert!(new[0] > new[1] && new[1] > new[2]);
            assert!(new[0] - new[1] > old[0] - old[1]);
            assert!(new[1] - new[2] > old[1] - old[2]);
        }
        // Bright materials retain three distinct levels, rather than a white slab.
        for &material in ALL_MATERIALS {
            let levels = [1.0_f32, 0.78, 0.58].map(|shade| {
                let rgb = material.style().color.map(|c| (f32::from(c) * shade) as u8);
                uniform_ordered_count(dot_coverage(luminance(rgb.map(|c| f32::from(c) / 255.0))))
            });
            assert!(
                levels[0] > levels[1] && levels[1] > levels[2],
                "{material:?}: {levels:?}"
            );
        }
    }

    #[test]
    fn authored_wood_retains_phase_averaged_face_and_seam_contrast() {
        for seed in [0_u64, 7, 71839] {
            let mut old = [0_usize; 3];
            let mut new = [0_usize; 3];
            for (face_index, (face, shade)) in [
                (Face::Top, 1.0_f32),
                (Face::Left, 0.78),
                (Face::Right, 0.58),
            ]
            .into_iter()
            .enumerate()
            {
                for v in 0_u8..8 {
                    for u in 0_u8..8 {
                        let rgb = Material::DarkPlanks
                            .texture(u, v, face, seed)
                            .map(|c| (f32::from(c) * shade) as u8);
                        let y = luminance(rgb.map(|c| f32::from(c) / 255.0));
                        // Every texel is compared against all 64 ordered phases.
                        old[face_index] += uniform_ordered_count(previous_coverage(y));
                        new[face_index] += uniform_ordered_count(dot_coverage(y));
                    }
                }
            }
            assert!(new[0] > new[1] && new[1] > new[2]);
            assert!(new[0] - new[1] >= old[0] - old[1]);
            assert!(new[1] - new[2] >= old[1] - old[2]);
            let seam = Material::DarkPlanks.texture(1, 0, Face::Top, seed);
            let board = Material::DarkPlanks.texture(1, 1, Face::Top, seed);
            let seam_level = dot_coverage(luminance(seam.map(|c| f32::from(c) / 255.0)));
            let board_level = dot_coverage(luminance(board.map(|c| f32::from(c) / 255.0)));
            assert!(uniform_ordered_count(board_level) > uniform_ordered_count(seam_level));
        }
    }

    #[test]
    fn zero_saturation_removes_material_palette_and_hue_colors() {
        for palette in 0..4 {
            let settings = VoxelLandscapeSettings {
                palette,
                saturation_percent: 0,
                hue_degrees: 360,
                ..Default::default()
            };
            let color = adjust([0.2, 0.6, 0.3], &settings);
            assert_eq!(color[0], color[1]);
            assert_eq!(color[1], color[2]);
        }
    }

    #[test]
    fn monochrome_is_gray_even_with_saturated_tinted_palette() {
        let settings = VoxelLandscapeSettings {
            color_mode: 0,
            palette: 3,
            saturation_percent: 100,
            hue_degrees: 360,
            ..Default::default()
        };
        let color = adjust([0.2, 0.6, 0.3], &settings);
        assert_eq!(color[0], color[1]);
        assert_eq!(color[1], color[2]);
    }

    #[test]
    fn color_brightness_does_not_change_material_tone() {
        let dim = VoxelLandscapeSettings {
            lightness_percent: 5,
            ..Default::default()
        };
        let bright = VoxelLandscapeSettings {
            lightness_percent: 100,
            ..dim.clone()
        };
        let input = vec![[51, 153, 76]; 16];
        let (a, dark) = compose(&input, 2, 1, &dim);
        let (b, light) = compose(&input, 2, 1, &bright);
        assert_eq!(a.dots, b.dots);
        assert!(dark
            .iter()
            .zip(&light)
            .all(|(a, b)| (0..3).all(|i| a[i] < b[i])));
    }

    #[test]
    fn color_controls_preserve_dot_bits_and_composited_mono_is_exactly_gray() {
        let input: Vec<_> = (0..64)
            .map(|i| match i % 4 {
                0 => [0; 3],
                1 => [71, 60, 52],
                2 => [185, 153, 110],
                _ => [122, 176, 205],
            })
            .collect();
        let (reference, _) = compose(&input, 4, 2, &VoxelLandscapeSettings::default());
        for mode in 0..2 {
            for palette in 0..4 {
                for hue in [0, 180, 360] {
                    for saturation in [0, 65, 100] {
                        let settings = VoxelLandscapeSettings {
                            color_mode: mode,
                            palette,
                            hue_degrees: hue,
                            saturation_percent: saturation,
                            ..Default::default()
                        };
                        let (raster, colors) = compose(&input, 4, 2, &settings);
                        assert!(reference
                            .dots
                            .iter()
                            .zip(&raster.dots)
                            .all(|(a, b)| a.to_bits() == b.to_bits()));
                        if mode == 0 || saturation == 0 {
                            assert!(colors.iter().all(|c| c[0] == c[1] && c[1] == c[2]));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn empty_pixels_short_inputs_and_empty_frames_leave_no_stale_dots() {
        let settings = VoxelLandscapeSettings::default();
        let (empty, colors) = compose(&[], 2, 1, &settings);
        assert!(empty.dots.iter().all(|&dot| dot == 0.0));
        assert!(colors.iter().all(|&color| color == [0; 3]));
        let (short, colors) = compose(&[[0; 3], [71, 60, 52]], 2, 1, &settings);
        assert_eq!(short.dots[0], 0.0);
        assert!(short.dots[1] > 0.0);
        assert!(short.dots[2..].iter().all(|&dot| dot == 0.0));
        assert_ne!(colors[0], [0; 3]);
        assert_eq!(colors[1], [0; 3]);
        for (width, height) in [(0, 0), (0, 24), (80, 0)] {
            let (raster, colors) = compose(&[[255; 3]], width, height, &settings);
            assert!(raster.dots.is_empty() && colors.is_empty());
        }
    }

    #[test]
    fn cell_averaging_does_not_mix_neighbor_cells_or_dilute_with_empty_pixels() {
        let settings = VoxelLandscapeSettings {
            saturation_percent: 100,
            ..Default::default()
        };
        let input: Vec<_> = (0..16)
            .map(|i| {
                if i % 4 < 2 {
                    [200, 90, 40]
                } else {
                    [40, 90, 200]
                }
            })
            .collect();
        let (_, colors) = compose(&input, 2, 1, &settings);
        assert!(colors[0][0] > colors[0][2]);
        assert!(colors[1][2] > colors[1][0]);
        let (_, one) = compose(&[[71, 60, 52]], 1, 1, &settings);
        let (_, full) = compose(&[[71, 60, 52]; 8], 1, 1, &settings);
        assert!((0..3).all(|i| one[0][i].abs_diff(full[0][i]) <= 1));
    }

    #[test]
    fn shared_density_still_controls_both_modes_and_old_lit_samples_are_retained() {
        let input: Vec<_> = (0..256)
            .map(|i| {
                if i % 8 == 0 {
                    [0; 3]
                } else {
                    [32 + (i % 224) as u8; 3]
                }
            })
            .collect();
        let (raster, _) = compose(&input, 8, 4, &VoxelLandscapeSettings::default());
        for mode in [DitherMode::Ordered, DitherMode::Stippled] {
            let mut previous = vec![false; input.len()];
            let mut counts = Vec::new();
            // Zero is a mathematical negative control; the UI starts at 25%.
            for density in [0.0, 0.25, 0.6, 1.0] {
                let mask = lit_mask(&raster, density, mode);
                assert!(previous.iter().zip(&mask).all(|(&a, &b)| !a || b));
                assert!(input
                    .iter()
                    .zip(&mask)
                    .all(|(&rgb, &lit)| rgb != [0; 3] || !lit));
                counts.push(mask.iter().filter(|&&lit| lit).count());
                previous = mask;
            }
            assert_eq!(counts[0], 0);
            assert!(counts.windows(2).all(|pair| pair[0] < pair[1]));
            let mask = lit_mask(&raster, 0.6, mode);
            for (i, &rgb) in input.iter().enumerate() {
                let old = legacy_coverage(luminance(rgb.map(|c| f32::from(c) / 255.0)));
                let was_lit = old * 0.6 > threshold(i % raster.width, i / raster.width, mode);
                assert!(!was_lit || mask[i]);
            }
        }
    }
}
