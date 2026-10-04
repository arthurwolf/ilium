//! Turning image layers into Braille dot intensities and cell colours.

use super::adjust::Adjustment;
use super::decode::DecodedImage;
use super::motion::View;
use crate::scene::Frame;

/// One image with the part of it that is visible and its blend weight.
pub struct Layer<'a> {
    pub image: &'a DecodedImage,
    pub view: View,
    pub weight: f32,
}

/// Most sub-samples per dot along one axis (box-filtering large sources).
const MAX_SUPERSAMPLE: usize = 3;

fn supersample_count(source_pixels_per_dot: f32) -> usize {
    (source_pixels_per_dot.round().max(1.0) as usize).min(MAX_SUPERSAMPLE)
}

/// Identity of what `render_layers` would draw; equal keys mean an identical
/// frame, so the scene can reuse the previous one.
pub fn layers_key(layers: &[Layer<'_>], width: u16, height: u16) -> Vec<u64> {
    // The scene supplies at most current + outgoing; reserve the complete
    // known key before pushes so no old/new growth buffers coexist.
    let mut key = Vec::with_capacity(2 + 6 * layers.len());
    key.extend_from_slice(&[u64::from(width), u64::from(height)]);
    for layer in layers {
        key.push(layer.image as *const DecodedImage as usize as u64);
        key.push(u64::from(layer.view.x0.to_bits()));
        key.push(u64::from(layer.view.y0.to_bits()));
        key.push(u64::from(layer.view.width.to_bits()));
        key.push(u64::from(layer.view.height.to_bits()));
        key.push(u64::from(layer.weight.to_bits()));
    }
    key
}

struct DotSampler<'a> {
    layers: &'a [Layer<'a>],
    dots_width: f32,
    dots_height: f32,
}

impl DotSampler<'_> {
    /// Raw blended colour of the dot at raster position (`x`, `y`), or `None`
    /// when no layer covers it (letterbox border).
    fn raw(&self, x: usize, y: usize) -> Option<[f32; 3]> {
        let u = (x as f32 + 0.5) / self.dots_width;
        let v = (y as f32 + 0.5) / self.dots_height;
        let mut color = [0.0f32; 3];
        let mut covered = false;
        for layer in self.layers {
            let view = layer.view;
            let image_u = view.x0 + u * view.width;
            let image_v = view.y0 + v * view.height;
            if !(0.0..=1.0).contains(&image_u) || !(0.0..=1.0).contains(&image_v) {
                continue;
            }
            covered = true;
            let step_u = view.width / self.dots_width;
            let step_v = view.height / self.dots_height;
            let count_x = supersample_count(step_u * layer.image.width as f32);
            let count_y = supersample_count(step_v * layer.image.height as f32);
            let mut sum = [0.0f32; 3];
            for sub_y in 0..count_y {
                let offset_v = ((sub_y as f32 + 0.5) / count_y as f32 - 0.5) * step_v;
                for sub_x in 0..count_x {
                    let offset_u = ((sub_x as f32 + 0.5) / count_x as f32 - 0.5) * step_u;
                    let sample = layer.image.sample(
                        (image_u + offset_u).clamp(0.0, 1.0),
                        (image_v + offset_v).clamp(0.0, 1.0),
                    );
                    for channel in 0..3 {
                        sum[channel] += sample[channel];
                    }
                }
            }
            let samples = (count_x * count_y) as f32;
            for channel in 0..3 {
                color[channel] += layer.weight * sum[channel] / samples;
            }
        }
        covered.then_some(color)
    }
}

/// Fill the whole raster and every cell colour from `layers`.
pub fn render_layers(frame: &mut Frame<'_>, layers: &[Layer<'_>], adjustment: &Adjustment) {
    let cells_wide = usize::from(frame.width);
    let cells_high = usize::from(frame.height);
    let dots_width = cells_wide * 2;
    let dots_height = cells_high * 4;
    if frame.raster.width != dots_width
        || frame.raster.height != dots_height
        || frame.cell_colors.len() != cells_wide * cells_high
    {
        return;
    }
    let sampler = DotSampler {
        layers,
        dots_width: dots_width as f32,
        dots_height: dots_height as f32,
    };
    for cell_y in 0..cells_high {
        for cell_x in 0..cells_wide {
            let mut color_sum = [0u32; 3];
            let mut sample_count = 0u32;
            for dot_y in 0..4 {
                for dot_x in 0..2 {
                    let x = cell_x * 2 + dot_x;
                    let y = cell_y * 4 + dot_y;
                    let Some(raw) = sampler.raw(x, y) else {
                        continue;
                    };
                    let adjusted = adjustment.apply(raw);
                    frame.raster.dots[y * dots_width + x] = adjusted.dot_intensity();
                    let bytes = adjusted.color_bytes();
                    for channel in 0..3 {
                        color_sum[channel] += bytes[channel];
                    }
                    sample_count += 1;
                }
            }
            // A cell fully in the letterbox border has no samples: black.
            let average = |sum: u32| sum.checked_div(sample_count).unwrap_or(0) as u8;
            frame.cell_colors[cell_y * cells_wide + cell_x] = [
                average(color_sum[0]),
                average(color_sum[1]),
                average(color_sum[2]),
            ];
        }
    }
}

/// A faint dust field with a soft light band drifting across it, shown while
/// the first image is still loading.
pub fn render_placeholder(frame: &mut Frame<'_>) {
    let cells_wide = usize::from(frame.width);
    let cells_high = usize::from(frame.height);
    let dots_width = cells_wide * 2;
    let dots_height = cells_high * 4;
    frame.cell_colors.fill([70, 84, 104]);
    if frame.raster.width != dots_width || frame.raster.height != dots_height {
        return;
    }
    let seconds = frame.time.as_secs_f32();
    let band = (seconds * 0.12).rem_euclid(1.6) - 0.3;
    for y in 0..dots_height {
        let v = (y as f32 + 0.5) / dots_height as f32;
        for x in 0..dots_width {
            let u = (x as f32 + 0.5) / dots_width as f32;
            let slant = u - band + (v - 0.5) * 0.25;
            let glow = (-(slant / 0.09).powi(2)).exp();
            frame.raster.dots[y * dots_width + x] = (0.05 + 0.45 * glow).clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::motion::{view_rect, Pose};
    use super::super::settings::{FitMode, ImagesSettings};
    use super::*;
    use crate::debug::render_frame;
    use crate::scene::Scene;
    use std::time::Duration;

    struct OneLayer<'a> {
        image: &'a DecodedImage,
        fit: FitMode,
        adjustment: Adjustment,
    }

    impl Scene for OneLayer<'_> {
        fn render(&mut self, frame: &mut Frame<'_>) {
            let screen = f32::from(frame.width) * 2.0 / (f32::from(frame.height) * 4.0);
            let view = view_rect(self.fit, self.image.aspect(), screen, Pose::STILL);
            let layers = [Layer {
                image: self.image,
                view,
                weight: 1.0,
            }];
            render_layers(frame, &layers, &self.adjustment);
        }
    }

    fn neutral() -> Adjustment {
        Adjustment::from_settings(&ImagesSettings {
            preset: super::super::settings::ImagePreset::Vivid,
            brightness_percent: 100,
            contrast_percent: 100,
            saturation_percent: 100,
            intensity_percent: 100,
            opacity_percent: 100,
            ..ImagesSettings::default()
        })
    }

    #[test]
    fn cell_colors_match_the_hand_computed_average() {
        // A flat colour image: every dot of every cell has the same adjusted
        // colour, so the cell colour equals the byte-truncated adjustment.
        let image = DecodedImage::from_rgb(4, 4, vec![[204, 102, 51]; 16]);
        let adjustment = neutral();
        let mut scene = OneLayer {
            image: &image,
            fit: FitMode::Stretch,
            adjustment,
        };
        let rendered = render_frame(&mut scene, 6, 3, Duration::ZERO);
        // channel = ((c - 0.5) * 1.28 + 0.5) with c = 0.8, 0.4, 0.2
        let expect = |c: f32| (((c - 0.5) * 1.28 + 0.5).clamp(0.0, 1.0) * 255.0) as u32 as u8;
        let expected = [
            expect(204.0 / 255.0),
            expect(102.0 / 255.0),
            expect(51.0 / 255.0),
        ];
        assert_eq!(rendered.cell_colors.len(), 18);
        assert!(rendered.cell_colors.iter().all(|color| *color == expected));
        // Intensity = level / 0.16 with level = mean * 0.82.
        let mean =
            ((0.8f32 - 0.5) * 1.28 + 0.5 + (0.4 - 0.5) * 1.28 + 0.5 + (0.2 - 0.5) * 1.28 + 0.5)
                / 3.0;
        let intensity = (mean * 0.82 / 0.16).clamp(0.0, 1.0);
        assert!(rendered
            .raster
            .dots
            .iter()
            .all(|dot| (*dot - intensity).abs() < 1e-3));
    }

    #[test]
    fn sampling_places_image_features_at_the_right_dots() {
        // Left half white, right half black, stretched over the screen.
        let mut pixels = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                pixels.push(if x < 4 { [255, 255, 255] } else { [0, 0, 0] });
            }
        }
        let image = DecodedImage::from_rgb(8, 8, pixels);
        let mut scene = OneLayer {
            image: &image,
            fit: FitMode::Stretch,
            adjustment: neutral(),
        };
        let rendered = render_frame(&mut scene, 10, 4, Duration::ZERO);
        let width = rendered.raster.width;
        for y in 0..rendered.raster.height {
            assert!(rendered.raster.dots[y * width + 2] > 0.9, "left is lit");
            assert!(
                rendered.raster.dots[y * width + width - 3] == 0.0,
                "right is dark"
            );
        }
    }

    #[test]
    fn fit_mode_leaves_borders_and_fill_covers_everything() {
        let image = DecodedImage::from_rgb(4, 4, vec![[255, 255, 255]; 16]);
        let lit_rows = |fit| {
            let mut scene = OneLayer {
                image: &image,
                fit,
                adjustment: neutral(),
            };
            let rendered = render_frame(&mut scene, 40, 10, Duration::ZERO);
            let width = rendered.raster.width;
            let rows: Vec<bool> = (0..rendered.raster.height)
                .map(|y| rendered.raster.dots[y * width + width / 2] > 0.5)
                .collect();
            (rows, rendered)
        };
        let (fill, _) = lit_rows(FitMode::Fill);
        assert!(fill.iter().all(|lit| *lit), "fill covers the whole screen");
        let (fit, rendered) = lit_rows(FitMode::Fit);
        // Square image on an 80 x 40 dot screen: full height, 40 dots wide.
        assert!(fit.iter().all(|lit| *lit));
        let width = rendered.raster.width;
        assert_eq!(rendered.raster.dots[width / 2 - 25], 0.0, "left border");
        assert_eq!(rendered.raster.dots[width / 2 + 25], 0.0, "right border");
        assert_eq!(rendered.cell_colors[0], [0, 0, 0]);
    }

    #[test]
    fn placeholder_is_sparse_and_moves() {
        struct Placeholder;
        impl Scene for Placeholder {
            fn render(&mut self, frame: &mut Frame<'_>) {
                render_placeholder(frame);
            }
        }
        let a = render_frame(&mut Placeholder, 40, 10, Duration::ZERO);
        let b = render_frame(&mut Placeholder, 40, 10, Duration::from_secs(4));
        assert_ne!(a.raster.dots, b.raster.dots);
        assert!(a.raster.dots.iter().all(|dot| *dot <= 0.5 + 1e-3));
        assert!(a.raster.dots.iter().any(|dot| *dot > 0.05));
    }
}
