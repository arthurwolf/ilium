//! Raw decoder frames to dot intensities and cell colors: the placement of the decoded picture
//! on the dot canvas (fit, fill, stretch), sharpening, render styles, and nearest-neighbour
//! resampling while a resize catches up.
//!
//! The host decodes to packed `rgb8` frames of exactly the size the scene asks for. The scene
//! asks for the size that makes the chosen fit a plain copy: the whole picture scaled to fit
//! (letterbox), a picture that covers the canvas (crop) or the canvas itself (stretch).

use super::settings::{FitMode, RenderStyle};

/// Largest frame the host decodes, per side.
pub const MAX_FRAME: (u32, u32) = (3_840, 2_160);

/// The size to ask the host for: `canvas` is the dot canvas (2 columns x 4 rows per cell),
/// `source` the pixel size of the video.
pub fn request_size(source: (u32, u32), canvas: (u32, u32), fit: FitMode) -> (u32, u32) {
    let (canvas_width, canvas_height) = (canvas.0.max(1), canvas.1.max(1));
    let (source_width, source_height) = (source.0.max(1), source.1.max(1));
    let size = match fit {
        FitMode::Stretch => (canvas_width, canvas_height),
        FitMode::Fit => {
            let width = (u64::from(canvas_height) * u64::from(source_width)
                / u64::from(source_height))
            .clamp(1, u64::from(canvas_width));
            let height = (u64::from(canvas_width) * u64::from(source_height)
                / u64::from(source_width))
            .clamp(1, u64::from(canvas_height));
            (width as u32, height as u32)
        }
        FitMode::Fill => {
            // Scale by the larger ratio so the picture covers the canvas on both sides.
            let by_width = u64::from(canvas_width) * u64::from(source_height);
            let by_height = u64::from(canvas_height) * u64::from(source_width);
            if by_width >= by_height {
                let height = by_width.div_ceil(u64::from(source_width));
                (canvas_width, height.max(u64::from(canvas_height)) as u32)
            } else {
                let width = by_height.div_ceil(u64::from(source_height));
                (width.max(u64::from(canvas_width)) as u32, canvas_height)
            }
        }
    };
    (size.0.clamp(1, MAX_FRAME.0), size.1.clamp(1, MAX_FRAME.1))
}

/// Places `frame` (`rgb8`, `frame_size`) centred on a black canvas of `canvas` pixels: a
/// larger frame is cropped, a smaller one is bordered.
pub fn place_on_canvas(frame: &[u8], frame_size: (u32, u32), canvas: (u32, u32)) -> Vec<u8> {
    let (frame_width, frame_height) = (frame_size.0 as usize, frame_size.1 as usize);
    let (canvas_width, canvas_height) = (canvas.0 as usize, canvas.1 as usize);
    if frame.len() != frame_width * frame_height * 3 {
        return Vec::new();
    }
    if frame_size == canvas {
        return frame.to_vec();
    }
    let mut out = vec![0u8; canvas_width * canvas_height * 3];
    let copy_width = frame_width.min(canvas_width);
    let copy_height = frame_height.min(canvas_height);
    let source_left = (frame_width - copy_width) / 2;
    let source_top = (frame_height - copy_height) / 2;
    let target_left = (canvas_width - copy_width) / 2;
    let target_top = (canvas_height - copy_height) / 2;
    for row in 0..copy_height {
        let from = ((source_top + row) * frame_width + source_left) * 3;
        let to = ((target_top + row) * canvas_width + target_left) * 3;
        out[to..to + copy_width * 3].copy_from_slice(&frame[from..from + copy_width * 3]);
    }
    out
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

fn luma_of(red: u8, green: u8, blue: u8) -> f32 {
    let value = 0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue);
    value.clamp(0.0, 255.0) / 255.0
}

/// Unsharp mask with a 3x3 box blur: `amount` 0 leaves the plane unchanged.
fn sharpen(plane: &[f32], width: usize, height: usize, amount: f32) -> Vec<f32> {
    if amount <= 0.0 || width < 5 || height < 5 {
        return plane.to_vec();
    }
    let at = |x: isize, y: isize| -> f32 {
        let column = x.clamp(0, width as isize - 1) as usize;
        let row = y.clamp(0, height as isize - 1) as usize;
        plane[row * width + column]
    };
    let mut out = vec![0.0; plane.len()];
    for y in 0..height {
        for x in 0..width {
            let (cx, cy) = (x as isize, y as isize);
            let mut sum = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    sum += at(cx + dx, cy + dy);
                }
            }
            let centre = plane[y * width + x];
            out[y * width + x] = (centre + amount * (centre - sum / 9.0)).clamp(0.0, 1.0);
        }
    }
    out
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

/// Converts one `rgb8` canvas. `None` when the buffer does not match the size (a truncated
/// read) or the size is not a whole number of cells. `detail` is the sharpening, 0..=100.
pub fn decode_frame(
    data: &[u8],
    width: usize,
    height: usize,
    style: RenderStyle,
    detail: u16,
) -> Option<Decoded> {
    let count = width.checked_mul(height)?;
    if count == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(4)
        || data.len() != count.checked_mul(3)?
    {
        return None;
    }
    let plane: Vec<f32> = data
        .chunks_exact(3)
        .map(|pixel| luma_of(pixel[0], pixel[1], pixel[2]))
        .collect();
    let amount = f32::from(detail) / 100.0 * 1.5;
    let mut dots = sharpen(&plane, width, height, amount);
    if style == RenderStyle::MonoInk {
        dots = ink(&dots, width, height);
    }
    let cell_colors = if style == RenderStyle::Colored {
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

    fn gradient_rgb(width: usize, height: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(width * height * 3);
        for _ in 0..height {
            for x in 0..width {
                let level = (x * 255 / (width - 1)) as u8;
                data.extend_from_slice(&[level, level, level]);
            }
        }
        data
    }

    #[test]
    fn request_sizes_follow_the_fit() {
        // A 16:9 video on a 200x100 canvas (2:1).
        assert_eq!(
            request_size((1920, 1080), (200, 100), FitMode::Stretch),
            (200, 100)
        );
        assert_eq!(
            request_size((1920, 1080), (200, 100), FitMode::Fit),
            (177, 100)
        );
        let (fill_width, fill_height) = request_size((1920, 1080), (200, 100), FitMode::Fill);
        assert_eq!(fill_width, 200);
        assert!((100..=113).contains(&fill_height));
        // A narrow canvas letterboxes top and bottom.
        assert_eq!(
            request_size((1920, 1080), (100, 100), FitMode::Fit),
            (100, 56)
        );
        // Degenerate input never yields a zero side.
        assert_eq!(request_size((0, 0), (0, 0), FitMode::Fit), (1, 1));
    }

    #[test]
    fn placing_borders_a_small_frame_and_crops_a_large_one() {
        let small = vec![200u8; 2 * 2 * 3];
        let bordered = place_on_canvas(&small, (2, 2), (4, 4));
        assert_eq!(bordered.len(), 4 * 4 * 3);
        assert_eq!(bordered[0], 0);
        assert_eq!(bordered[(4 + 1) * 3], 200);
        let mut large = vec![0u8; 6 * 6 * 3];
        // Mark the centre pixel (3, 3) of a 6x6 frame.
        large[(3 * 6 + 3) * 3] = 99;
        let cropped = place_on_canvas(&large, (6, 6), (4, 4));
        assert_eq!(cropped.len(), 4 * 4 * 3);
        assert_eq!(cropped[(2 * 4 + 2) * 3], 99);
        assert!(place_on_canvas(&[0; 5], (2, 2), (4, 4)).is_empty());
        assert_eq!(place_on_canvas(&small, (2, 2), (2, 2)), small);
    }

    #[test]
    fn gray_gradient_becomes_a_rising_intensity_ramp() {
        let data = gradient_rgb(32, 16);
        let decoded = decode_frame(&data, 32, 16, RenderStyle::Dithered, 0).unwrap();
        assert!(decoded.cell_colors.is_empty());
        for row in decoded.dots.chunks(32) {
            assert!(row.windows(2).all(|pair| pair[1] >= pair[0]));
            assert!(row[0] < 0.01 && row[31] > 0.99);
        }
    }

    #[test]
    fn detail_sharpens_an_edge_and_zero_leaves_it() {
        let mut data = vec![0u8; 16 * 8 * 3];
        for row in 0..8 {
            for column in 8..16 {
                let base = (row * 16 + column) * 3;
                data[base..base + 3].copy_from_slice(&[128, 128, 128]);
            }
        }
        let plain = decode_frame(&data, 16, 8, RenderStyle::Dithered, 0).unwrap();
        let sharp = decode_frame(&data, 16, 8, RenderStyle::Dithered, 100).unwrap();
        let at = |frame: &Decoded, x: usize| frame.dots[4 * 16 + x];
        assert!((at(&plain, 8) - 128.0 / 255.0).abs() < 1e-3);
        assert!(at(&sharp, 8) > at(&plain, 8), "bright side of the edge");
        assert!(at(&sharp, 7) <= at(&plain, 7), "dark side of the edge");
    }

    #[test]
    fn rgb_frames_use_luma_and_average_cell_colors() {
        // 2x4 dots = one cell; left half red, right half blue.
        let mut data = Vec::new();
        for _ in 0..4 {
            data.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        }
        let decoded = decode_frame(&data, 2, 4, RenderStyle::Colored, 0).unwrap();
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
        let decoded = decode_frame(&data, 2, 4, RenderStyle::Colored, 0).unwrap();
        assert!(decoded.cell_colors[0].iter().all(|channel| *channel >= 24));
    }

    #[test]
    fn mono_ink_lights_edges_more_than_flat_areas() {
        // Left half black, right half white with a hard vertical edge.
        let mut data = vec![0u8; 32 * 16 * 3];
        for row in 0..16 {
            for column in 16..32 {
                let base = (row * 32 + column) * 3;
                data[base..base + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let decoded = decode_frame(&data, 32, 16, RenderStyle::MonoInk, 0).unwrap();
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
        assert!(decode_frame(&[0; 10], 4, 4, RenderStyle::Dithered, 0).is_none());
        assert!(decode_frame(&[0; 45], 3, 5, RenderStyle::Dithered, 0).is_none());
        assert!(decode_frame(&[], 0, 0, RenderStyle::Dithered, 0).is_none());
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
