//! A decoded picture in opaque RGB and the incremental box-filter downscale that makes it.
//!
//! The host's `codec.decode-image` returns the whole picture as RGBA at its own size. A scene
//! frame has a deadline of a few milliseconds, so the downscale to the decode target
//! (`DECODE_OVERSAMPLE` times the dot resolution) runs in slices of a bounded number of source
//! pixels per frame; `Downscale::step` returns the finished [`Picture`] once the last slice is
//! done.

/// A picture the host decoded: tightly packed RGBA8, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RgbaImage {
    /// True when the buffer holds exactly `width * height` pixels and neither side is zero.
    pub fn is_consistent(&self) -> bool {
        self.width > 0
            && self.height > 0
            && u64::from(self.width) * u64::from(self.height) * 4 == self.rgba.len() as u64
    }
}

/// A decoded picture in opaque RGB (transparency is composited over black).
#[derive(Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl Picture {
    #[cfg(test)]
    pub fn from_rgb(width: u32, height: u32, pixels: Vec<[u8; 3]>) -> Self {
        debug_assert_eq!(pixels.len() as u64, u64::from(width) * u64::from(height));
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }

    fn texel(&self, x: i64, y: i64) -> [f32; 3] {
        let x = x.clamp(0, i64::from(self.width) - 1) as usize;
        let y = y.clamp(0, i64::from(self.height) - 1) as usize;
        let pixel = self.pixels[y * self.width as usize + x];
        [
            f32::from(pixel[0]) / 255.0,
            f32::from(pixel[1]) / 255.0,
            f32::from(pixel[2]) / 255.0,
        ]
    }

    /// Bilinear sample at normalized coordinates (0..1 over the picture).
    pub fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let (fx, fy) = (x - x0, y - y0);
        let (ix, iy) = (x0 as i64, y0 as i64);
        let a = self.texel(ix, iy);
        let b = self.texel(ix + 1, iy);
        let c = self.texel(ix, iy + 1);
        let d = self.texel(ix + 1, iy + 1);
        let mut out = [0.0; 3];
        for channel in 0..3 {
            let top = a[channel] + (b[channel] - a[channel]) * fx;
            let bottom = c[channel] + (d[channel] - c[channel]) * fx;
            out[channel] = top + (bottom - top) * fy;
        }
        out
    }
}

/// The size a picture of `width` x `height` is reduced to so it fits inside `max_width` x
/// `max_height`: the aspect is kept and a picture is never enlarged.
pub fn fit_inside(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    (
        ((f64::from(width) * scale).round() as u32).clamp(1, max_width.max(1)),
        ((f64::from(height) * scale).round() as u32).clamp(1, max_height.max(1)),
    )
}

/// A box-filter downscale that runs in slices.
pub struct Downscale {
    source: RgbaImage,
    target_width: u32,
    target_height: u32,
    next_row: u32,
    output: Vec<[u8; 3]>,
}

impl Downscale {
    /// Starts reducing `source` to fit inside `max_width` x `max_height`. `None` when the source
    /// buffer does not match its size.
    pub fn new(source: RgbaImage, max_width: u32, max_height: u32) -> Option<Self> {
        if !source.is_consistent() {
            return None;
        }
        let (target_width, target_height) =
            fit_inside(source.width, source.height, max_width, max_height);
        Some(Self {
            output: Vec::with_capacity(target_width as usize * target_height as usize),
            source,
            target_width,
            target_height,
            next_row: 0,
        })
    }

    /// The size of the finished picture.
    #[cfg(test)]
    pub fn target(&self) -> (u32, u32) {
        (self.target_width, self.target_height)
    }

    /// Filters output rows until about `source_pixel_budget` source pixels have been read
    /// (always at least one row); the finished picture once every row is done.
    pub fn step(&mut self, source_pixel_budget: usize) -> Option<Picture> {
        let mut spent = 0usize;
        while self.next_row < self.target_height {
            spent += self.filter_row(self.next_row);
            self.next_row += 1;
            if spent >= source_pixel_budget {
                break;
            }
        }
        if self.next_row < self.target_height {
            return None;
        }
        Some(Picture {
            width: self.target_width,
            height: self.target_height,
            pixels: std::mem::take(&mut self.output),
        })
    }

    /// Appends output row `row`; returns the source pixels it read.
    fn filter_row(&mut self, row: u32) -> usize {
        let source_width = self.source.width as u64;
        let source_height = u64::from(self.source.height);
        let target_width = u64::from(self.target_width);
        let target_height = u64::from(self.target_height);
        let y_start = u64::from(row) * source_height / target_height;
        let y_end = ((u64::from(row) + 1) * source_height / target_height)
            .max(y_start + 1)
            .min(source_height);
        let mut read = 0usize;
        for column in 0..target_width {
            let x_start = column * source_width / target_width;
            let x_end = ((column + 1) * source_width / target_width)
                .max(x_start + 1)
                .min(source_width);
            let mut sum = [0u64; 3];
            let mut count = 0u64;
            for y in y_start..y_end {
                let line = (y * source_width) as usize * 4;
                for x in x_start..x_end {
                    let at = line + x as usize * 4;
                    let alpha = u64::from(self.source.rgba[at + 3]);
                    // Transparency is composited over black.
                    for channel in 0..3 {
                        sum[channel] += u64::from(self.source.rgba[at + channel]) * alpha / 255;
                    }
                    count += 1;
                }
            }
            read += count as usize;
            let count = count.max(1);
            self.output.push([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ]);
        }
        read
    }
}

/// Reduces `source` in one go (tests and small pictures).
#[cfg(test)]
pub fn downscale_now(source: RgbaImage, max_width: u32, max_height: u32) -> Option<Picture> {
    let mut work = Downscale::new(source, max_width, max_height)?;
    loop {
        if let Some(picture) = work.step(usize::MAX) {
            return Some(picture);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage {
            width,
            height,
            rgba: rgba
                .iter()
                .copied()
                .cycle()
                .take(width as usize * height as usize * 4)
                .collect(),
        }
    }

    #[test]
    fn fit_inside_keeps_the_aspect_and_never_enlarges() {
        assert_eq!(fit_inside(100, 50, 200, 200), (100, 50));
        assert_eq!(fit_inside(4000, 2000, 1000, 1000), (1000, 500));
        assert_eq!(fit_inside(2000, 4000, 1000, 1000), (500, 1000));
        assert_eq!(fit_inside(10, 10, 1, 1), (1, 1));
        assert_eq!(fit_inside(0, 5, 10, 10), (1, 1));
    }

    #[test]
    fn a_picture_that_fits_is_copied_unchanged() {
        let picture = downscale_now(solid(4, 3, [10, 20, 30, 255]), 16, 16).unwrap();
        assert_eq!((picture.width, picture.height), (4, 3));
        assert!(picture.pixels.iter().all(|pixel| *pixel == [10, 20, 30]));
    }

    #[test]
    fn a_solid_picture_stays_solid_when_reduced() {
        let picture = downscale_now(solid(64, 32, [200, 100, 50, 255]), 8, 8).unwrap();
        assert_eq!((picture.width, picture.height), (8, 4));
        assert!(picture.pixels.iter().all(|pixel| *pixel == [200, 100, 50]));
    }

    #[test]
    fn the_box_filter_averages_a_checkerboard() {
        let mut source = solid(4, 4, [0, 0, 0, 255]);
        for y in 0..4usize {
            for x in 0..4usize {
                if (x + y) % 2 == 0 {
                    let at = (y * 4 + x) * 4;
                    source.rgba[at..at + 3].copy_from_slice(&[200, 200, 200]);
                }
            }
        }
        let picture = downscale_now(source, 2, 2).unwrap();
        assert!(picture.pixels.iter().all(|pixel| *pixel == [100, 100, 100]));
    }

    #[test]
    fn transparency_is_composited_over_black() {
        let picture = downscale_now(solid(2, 2, [255, 255, 255, 0]), 2, 2).unwrap();
        assert!(picture.pixels.iter().all(|pixel| *pixel == [0, 0, 0]));
        let half = downscale_now(solid(2, 2, [200, 100, 0, 128]), 2, 2).unwrap();
        assert_eq!(half.pixels[0], [100, 50, 0]);
    }

    #[test]
    fn a_slice_budget_spreads_the_work_over_several_steps() {
        let mut work = Downscale::new(solid(100, 100, [9, 9, 9, 255]), 10, 10).unwrap();
        assert_eq!(work.target(), (10, 10));
        let mut steps = 0;
        let picture = loop {
            steps += 1;
            if let Some(picture) = work.step(1_000) {
                break picture;
            }
            assert!(steps < 100, "the work must finish");
        };
        // One output row reads 10 x 10 x 10 = 1,000 source pixels, so every row is a step.
        assert_eq!(steps, 10);
        assert_eq!(picture.pixels.len(), 100);
        assert!(picture.pixels.iter().all(|pixel| *pixel == [9, 9, 9]));
    }

    #[test]
    fn an_inconsistent_buffer_is_refused() {
        let bad = RgbaImage {
            width: 4,
            height: 4,
            rgba: vec![0; 10],
        };
        assert!(Downscale::new(bad, 4, 4).is_none());
        let empty = RgbaImage {
            width: 0,
            height: 4,
            rgba: Vec::new(),
        };
        assert!(Downscale::new(empty, 4, 4).is_none());
    }

    #[test]
    fn bilinear_sampling_blends_neighbours() {
        let picture = Picture::from_rgb(2, 1, vec![[0, 0, 0], [255, 255, 255]]);
        let middle = picture.sample(0.5, 0.5);
        assert!((middle[0] - 0.5).abs() < 1e-3);
        let left = picture.sample(0.0, 0.5);
        assert!(left[0] < 0.01);
        assert!(picture.aspect() > 1.9);
    }
}
