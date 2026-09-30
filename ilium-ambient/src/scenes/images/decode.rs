//! Bounded image decoding and the small LRU of decoded images.

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::collections::VecDeque;
use std::io::Cursor;

/// Hard limits against hostile or absurdly large files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_width: u32,
    pub max_height: u32,
    /// Maximum width * height of the *source* image.
    pub max_pixels: u64,
    /// Maximum bytes of the encoded file.
    pub max_file_bytes: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_width: 16_384,
            max_height: 16_384,
            // 8000 x 8000; RGBA8 of that is 256 MB, the decoder's peak.
            max_pixels: 64_000_000,
            max_file_bytes: 48 * 1024 * 1024,
        }
    }
}

/// A decoded picture in opaque RGB (transparency is composited over black).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl DecodedImage {
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

    /// Bilinear sample at normalized coordinates (0..1 over the image).
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

/// Decode `bytes`, honour the EXIF orientation and downscale to fit inside
/// `max_width` x `max_height` (aspect kept, never upscaled).
pub fn decode_image(
    bytes: &[u8],
    limits: &DecodeLimits,
    max_width: u32,
    max_height: u32,
) -> Result<DecodedImage, String> {
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(format!(
            "file is larger than {} MB",
            limits.max_file_bytes / (1024 * 1024)
        ));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    reader.limits({
        let mut decoder_limits = Limits::default();
        decoder_limits.max_image_width = Some(limits.max_width);
        decoder_limits.max_image_height = Some(limits.max_height);
        decoder_limits.max_alloc = Some(limits.max_pixels.saturating_mul(6).min(1 << 30));
        decoder_limits
    });
    let mut decoder = reader.into_decoder().map_err(|error| {
        let text = error.to_string();
        if text.contains("limit") {
            format!("image is too large ({text})")
        } else {
            format!("not a readable image ({text})")
        }
    })?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > limits.max_width
        || height > limits.max_height
        || u64::from(width) * u64::from(height) > limits.max_pixels
    {
        return Err(format!("image is too large ({width} x {height} pixels)"));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("cannot decode image ({error})"))?;
    image.apply_orientation(orientation);
    let (width, height) = (image.width(), image.height());
    if width > max_width || height > max_height {
        image = image.resize(
            max_width.max(1),
            max_height.max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let rgba = image.to_rgba8();
    let pixels = rgba
        .pixels()
        .map(|pixel| {
            let alpha = u32::from(pixel.0[3]);
            let over_black = |channel: u8| ((u32::from(channel) * alpha + 127) / 255) as u8;
            [
                over_black(pixel.0[0]),
                over_black(pixel.0[1]),
                over_black(pixel.0[2]),
            ]
        })
        .collect();
    Ok(DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        pixels,
    })
}

/// Least-recently-used map with a small fixed capacity.
#[derive(Debug)]
pub struct Lru<V> {
    capacity: usize,
    entries: VecDeque<(usize, V)>,
}

impl<V> Lru<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn contains(&self, key: usize) -> bool {
        self.entries.iter().any(|(entry_key, _)| *entry_key == key)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Look up and mark as most recently used.
    pub fn get(&mut self, key: usize) -> Option<&V> {
        let position = self
            .entries
            .iter()
            .position(|(entry_key, _)| *entry_key == key)?;
        let entry = self.entries.remove(position)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, value)| value)
    }

    /// Insert as most recently used, evicting the oldest entry when full.
    pub fn insert(&mut self, key: usize, value: V) {
        self.entries.retain(|(entry_key, _)| *entry_key != key);
        self.entries.push_back((key, value));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use image::{ImageBuffer, ImageFormat, Rgb, Rgba};
    use std::io::Cursor;

    /// PNG bytes of a `width` x `height` image coloured by `paint(x, y)`.
    pub fn png_bytes(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| Rgb(paint(x, y)));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    pub fn png_rgba_bytes(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |_, _| Rgba(pixel));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    pub fn jpeg_bytes(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| Rgb(paint(x, y)));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .expect("encode jpeg");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn decodes_png_and_jpeg_fixtures() {
        let limits = DecodeLimits::default();
        let png = decode_image(
            &png_bytes(8, 6, |x, _| [x as u8 * 30, 0, 200]),
            &limits,
            100,
            100,
        )
        .expect("png");
        assert_eq!((png.width, png.height), (8, 6));
        assert_eq!(png.pixels[1], [30, 0, 200]);
        let jpeg = decode_image(
            &jpeg_bytes(16, 16, |_, _| [200, 100, 50]),
            &limits,
            100,
            100,
        )
        .expect("jpeg");
        assert_eq!((jpeg.width, jpeg.height), (16, 16));
        let pixel = jpeg.pixels[0];
        assert!(pixel[0].abs_diff(200) < 12 && pixel[1].abs_diff(100) < 12);
    }

    #[test]
    fn large_images_are_downscaled_keeping_the_aspect() {
        let bytes = png_bytes(400, 200, |_, _| [10, 20, 30]);
        let image = decode_image(&bytes, &DecodeLimits::default(), 100, 100).expect("decode");
        assert_eq!((image.width, image.height), (100, 50));
        let small = decode_image(&bytes, &DecodeLimits::default(), 1000, 1000).expect("decode");
        assert_eq!((small.width, small.height), (400, 200), "never upscaled");
    }

    #[test]
    fn hostile_dimensions_are_refused_before_decoding() {
        let limits = DecodeLimits {
            max_width: 64,
            max_height: 64,
            max_pixels: 1000,
            max_file_bytes: 1 << 20,
        };
        let wide = png_bytes(128, 4, |_, _| [1, 2, 3]);
        assert!(decode_image(&wide, &limits, 32, 32)
            .expect_err("too wide")
            .contains("too large"));
        let many = png_bytes(60, 60, |_, _| [1, 2, 3]);
        assert!(
            decode_image(&many, &limits, 32, 32).is_err(),
            "3600 px > 1000 px"
        );
    }

    #[test]
    fn oversized_files_and_garbage_are_refused() {
        let limits = DecodeLimits {
            max_file_bytes: 10,
            ..DecodeLimits::default()
        };
        assert!(decode_image(&png_bytes(8, 8, |_, _| [0; 3]), &limits, 8, 8)
            .expect_err("big file")
            .contains("larger"));
        let error = decode_image(b"definitely not an image", &DecodeLimits::default(), 8, 8)
            .expect_err("garbage");
        assert!(!error.is_empty());
    }

    #[test]
    fn a_huge_but_compressible_png_is_refused_by_the_default_limits() {
        // 20000 x 20 stays tiny to encode yet exceeds the 16384 width limit.
        let bytes = png_bytes(20_000, 20, |_, _| [9, 9, 9]);
        let error = decode_image(&bytes, &DecodeLimits::default(), 512, 512).expect_err("refused");
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn transparency_is_composited_over_black() {
        let bytes = png_rgba_bytes(2, 2, [200, 100, 50, 128]);
        let image = decode_image(&bytes, &DecodeLimits::default(), 8, 8).expect("decode");
        assert_eq!(image.pixels[0], [100, 50, 25]);
    }

    #[test]
    fn bilinear_sampling_interpolates_and_clamps() {
        let image = DecodedImage::from_rgb(2, 1, vec![[0, 0, 0], [255, 255, 255]]);
        let middle = image.sample(0.5, 0.5);
        assert!((middle[0] - 0.5).abs() < 1e-5);
        assert!(image.sample(0.0, 0.5)[0] < 1e-5, "clamps at the left edge");
        assert!(image.sample(1.0, 0.5)[0] > 0.999);
        assert!((image.sample(0.25, 0.5)[0] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn lru_evicts_the_least_recently_used() {
        let mut lru = Lru::new(2);
        lru.insert(1, "a");
        lru.insert(2, "b");
        assert_eq!(lru.get(1), Some(&"a"));
        lru.insert(3, "c");
        assert!(lru.contains(1) && lru.contains(3) && !lru.contains(2));
        lru.insert(3, "c2");
        assert_eq!(lru.len(), 2);
        assert_eq!(lru.get(3), Some(&"c2"));
        lru.clear();
        assert!(lru.is_empty());
    }
}
