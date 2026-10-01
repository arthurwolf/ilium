//! Raw decoder frames to dot intensities and cell colors: tone curve,
//! render styles, and nearest-neighbour resampling while a resize catches up.

use super::command::PixelFormat;
use super::settings::{RenderStyle, VideoSettings};

/// Brightness, contrast, gamma and invert as one 256-entry lookup table.
pub struct Tone {
    table: [f32; 256],
}

impl Tone {
    pub fn new(settings: &VideoSettings) -> Self {
        let gamma = settings.gamma_percent as f32 / 100.0;
        let contrast = ((100 + settings.contrast) as f32 / 100.0).max(0.05);
        let brightness = settings.brightness as f32 / 200.0;
        let mut table = [0.0; 256];
        for (level, entry) in table.iter_mut().enumerate() {
            let mut value = (level as f32 / 255.0).powf(1.0 / gamma.max(0.05));
            value = (value - 0.5) * contrast + 0.5 + brightness;
            value = value.clamp(0.0, 1.0);
            *entry = if settings.invert { 1.0 - value } else { value };
        }
        Self { table }
    }

    pub fn apply(&self, level: u8) -> f32 {
        self.table[usize::from(level)]
    }
}

/// A converted frame, independent of the terminal size it will be drawn at.
#[derive(Debug, PartialEq)]
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub dots: Vec<f32>,
    /// One color per cell (`width / 2 * height / 4`); empty unless colored.
    pub cell_colors: Vec<[u8; 3]>,
}

fn luma_of(red: u8, green: u8, blue: u8) -> u8 {
    let value = 0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue);
    value.round().clamp(0.0, 255.0) as u8
}

/// Pen-and-ink look: strong lines where the picture has edges plus a faint
/// tone so large flat areas do not vanish.
fn ink(plane: &[f32], width: usize, height: usize) -> Vec<f32> {
    let at = |x: isize, y: isize| -> f32 {
        let column = x.clamp(0, width as isize - 1) as usize;
        let row = y.clamp(0, height as isize - 1) as usize;
        plane[row * width + column]
    };
    let mut out = vec![0.0; plane.len()];
    for y in 0..height {
        for x in 0..width {
            let (cx, cy) = (x as isize, y as isize);
            let gx = at(cx + 1, cy - 1) + 2.0 * at(cx + 1, cy) + at(cx + 1, cy + 1)
                - at(cx - 1, cy - 1)
                - 2.0 * at(cx - 1, cy)
                - at(cx - 1, cy + 1);
            let gy = at(cx - 1, cy + 1) + 2.0 * at(cx, cy + 1) + at(cx + 1, cy + 1)
                - at(cx - 1, cy - 1)
                - 2.0 * at(cx, cy - 1)
                - at(cx + 1, cy - 1);
            let edge = (gx * gx + gy * gy).sqrt() * 0.5;
            out[y * width + x] = (0.22 * plane[y * width + x] + edge * 1.8).clamp(0.0, 1.0);
        }
    }
    out
}

/// Mean color of each 2x4 dot cell, lifted toward full brightness: how many
/// dots are lit already carries the luminance, so the ink itself keeps its hue.
fn cell_colors(data: &[u8], width: usize, height: usize) -> Vec<[u8; 3]> {
    let (columns, rows) = (width / 2, height / 4);
    let mut colors = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let mut sum = [0u32; 3];
            for dy in 0..4 {
                for dx in 0..2 {
                    let base = ((row * 4 + dy) * width + column * 2 + dx) * 3;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(data[base + channel]);
                    }
                }
            }
            let mean = sum.map(|total| (total / 8) as f32);
            let peak = mean.iter().copied().fold(0.0_f32, f32::max);
            let lift = if peak < 1.0 {
                1.0
            } else {
                (255.0 / peak).min(3.0)
            };
            let blend = |value: f32| {
                let lifted = (value * lift).min(255.0);
                (0.3 * value + 0.7 * lifted).round().clamp(0.0, 255.0) as u8
            };
            let floor = |value: u8| value.max(24);
            colors.push([
                floor(blend(mean[0])),
                floor(blend(mean[1])),
                floor(blend(mean[2])),
            ]);
        }
    }
    colors
}

/// Convert one raw frame. `None` when the buffer does not match the size
/// (a truncated read) or the size is not a whole number of cells.
pub fn decode_frame(
    data: &[u8],
    width: usize,
    height: usize,
    format: PixelFormat,
    style: RenderStyle,
    tone: &Tone,
) -> Option<Decoded> {
    let count = width.checked_mul(height)?;
    if count == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(4)
        || data.len() != count.checked_mul(format.bytes_per_dot())?
    {
        return None;
    }
    let levels: Vec<u8> = match format {
        PixelFormat::Gray => data.to_vec(),
        PixelFormat::Rgb24 => data
            .chunks_exact(3)
            .map(|pixel| luma_of(pixel[0], pixel[1], pixel[2]))
            .collect(),
    };
    let mut dots: Vec<f32> = levels.iter().map(|level| tone.apply(*level)).collect();
    if style == RenderStyle::MonoInk {
        dots = ink(&dots, width, height);
    }
    let cell_colors = if style == RenderStyle::Colored && format == PixelFormat::Rgb24 {
        cell_colors(data, width, height)
    } else {
        Vec::new()
    };
    Some(Decoded {
        width,
        height,
        dots,
        cell_colors,
    })
}

impl Decoded {
    /// Copy into a destination of `dest_width x dest_height` dots, resampling
    /// by nearest neighbour when the sizes differ.
    pub fn resampled_dots(&self, dest_width: usize, dest_height: usize) -> Vec<f32> {
        if (dest_width, dest_height) == (self.width, self.height) {
            return self.dots.clone();
        }
        let mut out = Vec::with_capacity(dest_width * dest_height);
        for y in 0..dest_height {
            let source_y = (y * self.height / dest_height.max(1)).min(self.height - 1);
            for x in 0..dest_width {
                let source_x = (x * self.width / dest_width.max(1)).min(self.width - 1);
                out.push(self.dots[source_y * self.width + source_x]);
            }
        }
        out
    }

    pub fn resampled_colors(&self, dest_columns: usize, dest_rows: usize) -> Vec<[u8; 3]> {
        let (columns, rows) = (self.width / 2, self.height / 4);
        if self.cell_colors.is_empty() || columns == 0 || rows == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(dest_columns * dest_rows);
        for y in 0..dest_rows {
            let source_y = (y * rows / dest_rows.max(1)).min(rows - 1);
            for x in 0..dest_columns {
                let source_x = (x * columns / dest_columns.max(1)).min(columns - 1);
                out.push(self.cell_colors[source_y * columns + source_x]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral() -> Tone {
        Tone::new(&VideoSettings {
            contrast: 0,
            ..VideoSettings::default()
        })
    }

    fn gradient(width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .map(|index| ((index % width) * 255 / (width - 1)) as u8)
            .collect()
    }

    #[test]
    fn neutral_tone_is_the_identity() {
        let tone = neutral();
        for level in [0u8, 1, 64, 128, 200, 255] {
            assert!((tone.apply(level) - f32::from(level) / 255.0).abs() < 1e-5);
        }
    }

    #[test]
    fn tone_controls_move_levels_in_the_right_direction() {
        let base = neutral().apply(100);
        let with = |change: fn(&mut VideoSettings)| {
            let mut settings = VideoSettings {
                contrast: 0,
                ..VideoSettings::default()
            };
            change(&mut settings);
            Tone::new(&settings).apply(100)
        };
        assert!(with(|s| s.brightness = 50) > base);
        assert!(with(|s| s.brightness = -50) < base);
        assert!(with(|s| s.gamma_percent = 200) > base);
        assert!(with(|s| s.gamma_percent = 60) < base);
        assert!(
            with(|s| s.contrast = 60) < base,
            "contrast pushes darks darker"
        );
        assert!((with(|s| s.invert = true) - (1.0 - base)).abs() < 1e-5);
        let flat = |level| {
            Tone::new(&VideoSettings {
                contrast: -100,
                ..VideoSettings::default()
            })
            .apply(level)
        };
        assert!(
            (flat(0) - flat(255)).abs() < 0.1,
            "minimum contrast is nearly flat"
        );
    }

    #[test]
    fn gray_gradient_becomes_a_rising_intensity_ramp() {
        let data = gradient(32, 16);
        let decoded = decode_frame(
            &data,
            32,
            16,
            PixelFormat::Gray,
            RenderStyle::Dithered,
            &neutral(),
        )
        .unwrap();
        assert!(decoded.cell_colors.is_empty());
        for row in decoded.dots.chunks(32) {
            assert!(row.windows(2).all(|pair| pair[1] >= pair[0]));
            assert!(row[0] < 0.01 && row[31] > 0.99);
        }
    }

    #[test]
    fn dithering_a_gradient_lights_more_dots_toward_the_bright_side() {
        use crate::raster::DitherMode;
        let data = gradient(60, 40);
        let decoded = decode_frame(
            &data,
            60,
            40,
            PixelFormat::Gray,
            RenderStyle::Dithered,
            &neutral(),
        )
        .unwrap();
        let mut raster = crate::raster::Raster::default();
        raster.resize(60, 40);
        raster.dots.copy_from_slice(&decoded.dots);
        let mut lit_per_third = [0usize; 3];
        for y in 0..40 {
            for x in 0..60 {
                let value = raster.dots[y * 60 + x];
                if value > crate::raster::threshold(x, y, DitherMode::Ordered) {
                    lit_per_third[x / 20] += 1;
                }
            }
        }
        assert!(lit_per_third[0] < lit_per_third[1] && lit_per_third[1] < lit_per_third[2]);
        assert!(lit_per_third[0] > 0 && lit_per_third[2] < 20 * 40 + 1);
    }

    #[test]
    fn rgb_frames_use_luma_and_average_cell_colors() {
        // 2x4 dots = one cell; left half red, right half blue.
        let mut data = Vec::new();
        for _ in 0..4 {
            data.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        }
        let decoded = decode_frame(
            &data,
            2,
            4,
            PixelFormat::Rgb24,
            RenderStyle::Colored,
            &neutral(),
        )
        .unwrap();
        assert_eq!(decoded.cell_colors.len(), 1);
        let [red, green, blue] = decoded.cell_colors[0];
        assert!(
            red > 100 && blue > 100 && green < 60,
            "{red} {green} {blue}"
        );
        assert!(
            decoded.dots[0] > decoded.dots[1],
            "red is brighter than blue in luma"
        );
    }

    #[test]
    fn dark_cells_keep_a_visible_minimum_color() {
        let data = vec![0u8; 2 * 4 * 3];
        let decoded = decode_frame(
            &data,
            2,
            4,
            PixelFormat::Rgb24,
            RenderStyle::Colored,
            &neutral(),
        )
        .unwrap();
        assert!(decoded.cell_colors[0].iter().all(|channel| *channel >= 24));
    }

    #[test]
    fn mono_ink_lights_edges_more_than_flat_areas() {
        // Left half black, right half white with a hard vertical edge.
        let mut data = vec![0u8; 32 * 16];
        for row in 0..16 {
            for column in 16..32 {
                data[row * 32 + column] = 255;
            }
        }
        let decoded = decode_frame(
            &data,
            32,
            16,
            PixelFormat::Gray,
            RenderStyle::MonoInk,
            &neutral(),
        )
        .unwrap();
        let at = |x: usize, y: usize| decoded.dots[y * 32 + x];
        assert!(at(15, 8) > 0.9 && at(16, 8) > 0.9, "edge is inked");
        assert!(at(4, 8) < 0.05, "black flat area stays empty");
        assert!(
            at(28, 8) > 0.1 && at(28, 8) < 0.4,
            "white flat area keeps a faint tone"
        );
    }

    #[test]
    fn malformed_buffers_are_rejected() {
        let tone = neutral();
        assert!(decode_frame(
            &[0; 10],
            4,
            4,
            PixelFormat::Gray,
            RenderStyle::Dithered,
            &tone
        )
        .is_none());
        assert!(decode_frame(
            &[0; 15],
            3,
            5,
            PixelFormat::Gray,
            RenderStyle::Dithered,
            &tone
        )
        .is_none());
        assert!(decode_frame(&[], 0, 0, PixelFormat::Gray, RenderStyle::Dithered, &tone).is_none());
    }

    #[test]
    fn resampling_scales_dots_and_colors() {
        let decoded = Decoded {
            width: 4,
            height: 8,
            dots: (0..32).map(|index| index as f32 / 31.0).collect(),
            cell_colors: vec![[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]],
        };
        assert_eq!(decoded.resampled_dots(4, 8), decoded.dots);
        let larger = decoded.resampled_dots(8, 16);
        assert_eq!(larger.len(), 128);
        assert_eq!(larger[0], decoded.dots[0]);
        assert_eq!(larger[127], decoded.dots[31]);
        assert_eq!(decoded.resampled_dots(2, 4).len(), 8);
        let colors = decoded.resampled_colors(4, 4);
        assert_eq!(colors.len(), 16);
        assert_eq!(colors[0], [1, 1, 1]);
        assert_eq!(colors[3], [2, 2, 2]);
        assert_eq!(colors[15], [4, 4, 4]);
        assert!(Decoded {
            cell_colors: vec![],
            ..decoded
        }
        .resampled_colors(2, 2)
        .is_empty());
    }
}
