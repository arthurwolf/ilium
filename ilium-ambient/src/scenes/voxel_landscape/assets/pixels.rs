use super::budget::{zeroed_bytes, ByteBudget, Cancel, Limits, Reservation};
use super::error::{AssetError, Result};
use super::identity::{BlobOrigin, Digest256, SourceBlob};
use image::{codecs::png::PngDecoder, ColorType, ImageDecoder};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{self, BufRead, Cursor, Read, Seek, SeekFrom},
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageExpectations {
    pub dimensions: Option<[u32; 2]>,
    pub rgba_sha256: Option<Digest256>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DecodeInfo {
    pub source_bits_per_channel: u8,
    pub alpha_min: u8,
    pub alpha_max: u8,
    pub zero_alpha_pixels: u64,
    pub partially_transparent_pixels: u64,
    pub opaque_pixels: u64,
    pub omitted_ancillary_chunks: u32,
    /// At most 32 distinct names; count above includes any further names.
    pub omitted_ancillary_kinds: Vec<String>,
    pub color_policy: &'static str,
}

/// Unmodified 8-bit channels (or explicitly normalized 16-bit channels), never
/// gamma-decoded normal maps or alpha-flattened artwork. Arc<PixelImage> is the
/// sharing unit; image storage is not cloned per block or animation frame.
#[derive(Debug)]
pub struct PixelImage {
    dimensions: [u32; 2],
    rgba: Vec<u8>,
    origin: BlobOrigin,
    source_sha256: Digest256,
    rgba_sha256: Digest256,
    info: DecodeInfo,
    _reservation: Reservation,
}
impl PixelImage {
    /// Decode a TGA while retaining original-byte and normalized-RGBA identities.
    pub fn decode_tga(
        blob: &SourceBlob,
        expected: ImageExpectations,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        let decoded = super::tga::decode(blob, expected, limits, budget, cancel)?;
        let mut info = DecodeInfo {
            source_bits_per_channel: decoded.bits,
            alpha_min: 255,
            alpha_max: 0,
            zero_alpha_pixels: 0,
            partially_transparent_pixels: 0,
            opaque_pixels: 0,
            omitted_ancillary_chunks: 0,
            omitted_ancillary_kinds: Vec::new(),
            color_policy: "TGA BGR/origin/palette normalized to RGBA8; source hash retained; no gamma decoding or alpha flattening",
        };
        for (index, pixel) in decoded.rgba.chunks_exact(4).enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            info.alpha_min = info.alpha_min.min(pixel[3]);
            info.alpha_max = info.alpha_max.max(pixel[3]);
            match pixel[3] {
                0 => info.zero_alpha_pixels += 1,
                255 => info.opaque_pixels += 1,
                _ => info.partially_transparent_pixels += 1,
            }
        }
        let rgba_sha256 = Digest256::of_checked(&decoded.rgba, cancel)?;
        Ok(Self {
            dimensions: decoded.dimensions,
            rgba: decoded.rgba,
            origin: blob.origin().clone(),
            source_sha256: blob.digest(),
            rgba_sha256,
            info,
            _reservation: decoded.reservation,
        })
    }
    pub fn decode_png(
        blob: &SourceBlob,
        expected: ImageExpectations,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if !blob.uses_budget(budget) {
            return Err(AssetError::InvalidImage(
                "source and decoder must share the texture-bank byte account".into(),
            ));
        }
        let dimensions = png_dimensions(blob.bytes())?;
        let rgba_len = limits.rgba_bytes(dimensions[0], dimensions[1])?;
        if expected.dimensions.is_some_and(|e| e != dimensions) {
            return Err(AssetError::InvalidImage(
                "decoded source dimensions differ from the pinned inspection".into(),
            ));
        }
        let sanitized = sanitize_png(blob.bytes(), limits, budget, cancel)?;
        // image::Limits.max_alloc is best effort. This allowance plus the
        // product check bounds our buffers, not all upstream allocator behavior.
        let _scratch = budget.reserve(limits.decoder_scratch_bytes, cancel)?;
        let mut codec_limits = image::Limits::default();
        codec_limits.max_image_width = None;
        codec_limits.max_image_height = None;
        codec_limits.max_alloc = Some(limits.decoder_scratch_bytes);
        let input = CheckedCursor {
            inner: Cursor::new(sanitized.bytes.as_slice()),
            cancel,
        };
        let decoder_result = PngDecoder::with_limits(input, codec_limits);
        cancel.check()?;
        let decoder = decoder_result.map_err(image_error)?;
        if decoder.dimensions() != (dimensions[0], dimensions[1]) {
            return Err(AssetError::InvalidImage(
                "PNG dimensions changed during decode".into(),
            ));
        }
        let color = decoder.color_type();
        let (channels, bytes_per_channel, grayscale) = match color {
            ColorType::L8 => (1, 1, true),
            ColorType::La8 => (2, 1, true),
            ColorType::Rgb8 => (3, 1, false),
            ColorType::Rgba8 => (4, 1, false),
            ColorType::L16 => (1, 2, true),
            ColorType::La16 => (2, 2, true),
            ColorType::Rgb16 => (3, 2, false),
            ColorType::Rgba16 => (4, 2, false),
            _ => return Err(AssetError::Unsupported("PNG output color type".into())),
        };
        let raw_len = usize::try_from(decoder.total_bytes()).map_err(|_| AssetError::Allocation)?;
        if raw_len != rgba_len / 4 * channels * bytes_per_channel {
            return Err(AssetError::InvalidImage(
                "PNG decoder output length disagrees with dimensions".into(),
            ));
        }
        let _raw_reservation = budget.reserve(raw_len as u64, cancel)?;
        let mut raw = zeroed_bytes(raw_len)?;
        let decoded = decoder.read_image(&mut raw);
        cancel.check()?;
        decoded.map_err(image_error)?;
        let reservation = budget.reserve(rgba_len as u64, cancel)?;
        let mut rgba = zeroed_bytes(rgba_len)?;
        let mut info = DecodeInfo {
            source_bits_per_channel: (bytes_per_channel * 8) as u8,
            alpha_min: 255, alpha_max: 0,
            zero_alpha_pixels: 0, partially_transparent_pixels: 0, opaque_pixels: 0,
            omitted_ancillary_chunks: sanitized.omitted,
            omitted_ancillary_kinds: sanitized.kinds.iter().cloned().collect(),
            color_policy: "retain raw color channels; color sampler assumes sRGB; data sampler stays linear; 16-bit channels round to nearest 8-bit UNORM; ancillary color/text metadata is not interpreted",
        };
        for (index, target) in rgba.chunks_exact_mut(4).enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            let offset = index * channels * bytes_per_channel;
            let component = |channel: usize| -> u8 {
                let start = offset + channel * bytes_per_channel;
                if bytes_per_channel == 1 {
                    raw[start]
                } else {
                    // ImageDecoder::read_image exposes native-endian u16s.
                    let value = u16::from_ne_bytes([raw[start], raw[start + 1]]);
                    ((u32::from(value) + 128) / 257) as u8
                }
            };
            let rgb = if grayscale {
                [component(0); 3]
            } else {
                [component(0), component(1), component(2)]
            };
            let alpha = if (grayscale && channels == 2) || (!grayscale && channels == 4) {
                component(channels - 1)
            } else {
                255
            };
            target[..3].copy_from_slice(&rgb);
            target[3] = alpha;
            info.alpha_min = info.alpha_min.min(alpha);
            info.alpha_max = info.alpha_max.max(alpha);
            match alpha {
                0 => info.zero_alpha_pixels += 1,
                255 => info.opaque_pixels += 1,
                _ => info.partially_transparent_pixels += 1,
            }
        }
        let rgba_sha256 = Digest256::of_checked(&rgba, cancel)?;
        if let Some(expected) = expected.rgba_sha256 {
            if rgba_sha256 != expected {
                return Err(AssetError::Integrity {
                    expected: expected.to_string(),
                    actual: rgba_sha256.to_string(),
                });
            }
        }
        cancel.check()?;
        Ok(Self {
            dimensions,
            rgba,
            origin: blob.origin().clone(),
            source_sha256: blob.digest(),
            rgba_sha256,
            info,
            _reservation: reservation,
        })
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    pub fn dimensions(&self) -> [u32; 2] {
        self.dimensions
    }
    pub fn bytes(&self) -> &[u8] {
        &self.rgba
    }
    pub fn origin(&self) -> &BlobOrigin {
        &self.origin
    }
    pub fn source_sha256(&self) -> Digest256 {
        self.source_sha256
    }
    pub fn rgba_sha256(&self) -> Digest256 {
        self.rgba_sha256
    }
    pub fn info(&self) -> &DecodeInfo {
        &self.info
    }
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.dimensions[0] || y >= self.dimensions[1] {
            return None;
        }
        let index = (y as usize * self.dimensions[0] as usize + x as usize) * 4;
        let p = &self.rgba[index..index + 4];
        Some([p[0], p[1], p[2], p[3]])
    }
}
fn image_error(error: image::ImageError) -> AssetError {
    AssetError::InvalidImage(super::error::summary(&error.to_string()))
}

fn png_dimensions(bytes: &[u8]) -> Result<[u32; 2]> {
    if bytes.len() < 33
        || &bytes[..8] != PNG_SIGNATURE
        || &bytes[12..16] != b"IHDR"
        || bytes[8..12] != [0, 0, 0, 13]
    {
        return Err(AssetError::InvalidImage(
            "missing PNG signature or leading 13-byte IHDR".into(),
        ));
    }
    Ok([
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
    ])
}

struct SanitizedPng {
    bytes: Vec<u8>,
    omitted: u32,
    kinds: BTreeSet<String>,
    _reservation: Reservation,
}
/// Remove ancillary text/profiles before image's decoder sees them. They are not
/// texture pixels and may contain compressed payloads. Keep PLTE/tRNS and all
/// standard pixel-critical chunks verbatim, including their CRCs for the decoder.
/// The original file remains unmodified and its digest stays in the receipt.
fn sanitize_png(
    bytes: &[u8],
    limits: &Limits,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<SanitizedPng> {
    if bytes.len() as u64 > limits.encoded_bytes {
        return Err(AssetError::Limit {
            resource: "encoded bytes",
            requested: bytes.len() as u64,
            limit: limits.encoded_bytes,
        });
    }
    let dimensions = png_dimensions(bytes)?;
    limits.rgba_bytes(dimensions[0], dimensions[1])?;
    let reservation = budget.reserve(bytes.len() as u64, cancel)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| AssetError::Allocation)?;
    output.extend_from_slice(PNG_SIGNATURE);
    let (mut position, mut count, mut omitted) = (8_usize, 0_usize, 0_u32);
    let mut kinds = BTreeSet::new();
    let mut ended = false;
    while position < bytes.len() {
        cancel.check()?;
        count += 1;
        if count > limits.png_chunks {
            return Err(AssetError::Limit {
                resource: "PNG chunks",
                requested: count as u64,
                limit: limits.png_chunks as u64,
            });
        }
        if bytes.len() - position < 12 {
            return Err(AssetError::InvalidImage("truncated PNG chunk".into()));
        }
        let length = u32::from_be_bytes([
            bytes[position],
            bytes[position + 1],
            bytes[position + 2],
            bytes[position + 3],
        ]) as usize;
        let end = position
            .checked_add(12)
            .and_then(|p| p.checked_add(length))
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| {
                AssetError::InvalidImage("PNG chunk length exceeds source bytes".into())
            })?;
        let kind = &bytes[position + 4..position + 8];
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err(AssetError::InvalidImage(
                "non-alphabetic PNG chunk type".into(),
            ));
        }
        if count == 1 && (kind != b"IHDR" || length != 13) {
            return Err(AssetError::InvalidImage("bad leading PNG chunk".into()));
        }
        if kind == b"acTL" || kind == b"fcTL" || kind == b"fdAT" {
            return Err(AssetError::Unsupported(
                "APNG is not a Java texture-strip animation; normalize it explicitly".into(),
            ));
        }
        if kind == b"IHDR" && count != 1 {
            return Err(AssetError::InvalidImage("duplicate IHDR".into()));
        }
        if kind == b"IHDR"
            || kind == b"PLTE"
            || kind == b"tRNS"
            || kind == b"IDAT"
            || kind == b"IEND"
        {
            output.extend_from_slice(&bytes[position..end]);
        } else if kind[0] & 32 == 0 {
            return Err(AssetError::Unsupported(format!(
                "critical PNG chunk {}",
                String::from_utf8_lossy(kind)
            )));
        } else {
            omitted += 1;
            if kinds.len() < 32 {
                kinds.insert(String::from_utf8_lossy(kind).into_owned());
            }
        }
        position = end;
        if kind == b"IEND" {
            if length != 0 || position != bytes.len() {
                return Err(AssetError::InvalidImage(
                    "invalid IEND or trailing PNG bytes".into(),
                ));
            }
            ended = true;
            break;
        }
    }
    if !ended {
        return Err(AssetError::InvalidImage("missing IEND".into()));
    }
    Ok(SanitizedPng {
        bytes: output,
        omitted,
        kinds,
        _reservation: reservation,
    })
}

/// A decoder can read buffered compressed data between checks; cancellation is
/// cooperative, not a hard wall-time interrupt. Limiting read slices provides
/// checkpoints without ever involving a disk or UI-thread I/O operation here.
struct CheckedCursor<'a> {
    inner: Cursor<&'a [u8]>,
    cancel: Cancel<'a>,
}
impl CheckedCursor<'_> {
    fn check(&self) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            Err(io::Error::other("asset decode cancelled"))
        } else {
            Ok(())
        }
    }
}
impl Read for CheckedCursor<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        let count = out.len().min(64 * 1024);
        self.inner.read(&mut out[..count])
    }
}
impl BufRead for CheckedCursor<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.check()?;
        let bytes = self.inner.fill_buf()?;
        Ok(&bytes[..bytes.len().min(64 * 1024)])
    }
    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}
impl Seek for CheckedCursor<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.inner.seek(position)
    }
}

#[cfg(test)]
pub(crate) fn fixture_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    use image::ImageEncoder;
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    encoded
}
#[cfg(test)]
mod tests {
    use super::super::{identity::OriginKind, review::fixture_origin};
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicBool, Ordering},
    };
    fn crc(bytes: &[u8]) -> u32 {
        let mut c = !0_u32;
        for byte in bytes {
            c ^= u32::from(*byte);
            for _ in 0..8 {
                c = (c >> 1) ^ (0xedb8_8320 & (0_u32.wrapping_sub(c & 1)));
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(bytes);
        let check = crc(&out[start..]);
        out.extend_from_slice(&check.to_be_bytes());
    }
    fn palette_png() -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        chunk(&mut out, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 3, 0, 0, 0]);
        chunk(&mut out, b"PLTE", &[0, 0, 0, 255, 0, 0]);
        chunk(&mut out, b"tRNS", &[255, 0]);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&[0, 0, 1]).unwrap();
        chunk(&mut out, b"IDAT", &z.finish().unwrap());
        chunk(&mut out, b"IEND", &[]);
        out
    }
    fn decode(bytes: Vec<u8>, expected: ImageExpectations) -> Result<PixelImage> {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(512 * 1024 * 1024).unwrap();
        let limits = Limits::default();
        let blob = SourceBlob::new(
            bytes,
            fixture_origin(OriginKind::DiagnosticFixture),
            None,
            &limits,
            &budget,
            cancel,
        )?;
        PixelImage::decode_png(&blob, expected, &limits, &budget, cancel)
    }
    #[test]
    fn palette_transparency_and_opaque_black_are_not_air() {
        let image = decode(palette_png(), ImageExpectations::default()).unwrap();
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(image.pixel(1, 0), Some([255, 0, 0, 0]));
        assert_eq!(
            (image.info().opaque_pixels, image.info().zero_alpha_pixels),
            (1, 1)
        );
        assert_eq!(
            image.rgba_sha256(),
            Digest256::of(&[0, 0, 0, 255, 255, 0, 0, 0])
        );
    }
    #[test]
    fn real_decode_accepts_both_measured_goodvibes_shapes_without_assuming_frames() {
        // Synthetic constant pixels, not downloaded GoodVibes artwork. This
        // exercises full decoded allocation shapes rather than header-only math.
        for [w, h] in [[512_u32, 16384_u32], [1021, 16384]] {
            let data = [21, 67, 91, 153].repeat(w as usize * h as usize);
            let expected = Digest256::of(&data);
            let encoded = fixture_png(w, h, &data);
            drop(data);
            let image = decode(
                encoded,
                ImageExpectations {
                    dimensions: Some([w, h]),
                    rgba_sha256: Some(expected),
                },
            )
            .unwrap();
            assert_eq!(image.dimensions(), [w, h]);
            assert_eq!((image.info().alpha_min, image.info().alpha_max), (153, 153));
            assert_eq!(image.pixel(w - 1, h - 1), Some([21, 67, 91, 153]));
        }
    }
    #[test]
    fn corrupt_truncated_and_oversized_images_fail_before_publication() {
        let image = fixture_png(2, 2, &[255; 16]);
        assert!(decode(image[..20].to_vec(), ImageExpectations::default()).is_err());
        let mut huge = image.clone();
        huge[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
        huge[20..24].copy_from_slice(&100_000_u32.to_be_bytes());
        assert!(matches!(
            decode(huge, ImageExpectations::default()),
            Err(AssetError::Limit {
                resource: "decoded pixels",
                ..
            })
        ));
        assert!(decode(
            image.clone(),
            ImageExpectations {
                dimensions: Some([3, 2]),
                rgba_sha256: None
            }
        )
        .is_err());
        assert!(decode(
            image,
            ImageExpectations {
                dimensions: None,
                rgba_sha256: Some(Digest256::of(b"wrong"))
            }
        )
        .is_err());
    }
    #[test]
    fn unused_compressed_ancillary_data_never_reaches_the_decoder() {
        let original = fixture_png(1, 1, &[3, 5, 7, 255]);
        let mut amended = original[..33].to_vec();
        // Deliberately invalid compressed text. Text metadata is not part of
        // texture decoding, not a reason to decompress an unbounded payload.
        chunk(&mut amended, b"zTXt", b"note\0\0not-a-zlib-stream");
        amended.extend_from_slice(&original[33..]);
        let expected_source = Digest256::of(&amended);
        let image = decode(amended, ImageExpectations::default()).unwrap();
        assert_eq!(image.source_sha256(), expected_source);
        assert_eq!(image.info().omitted_ancillary_chunks, 1);
        assert_eq!(image.pixel(0, 0), Some([3, 5, 7, 255]));
    }
    #[test]
    fn apng_is_not_silently_reinterpreted_as_a_java_strip() {
        let original = fixture_png(1, 1, &[255; 4]);
        let mut apng = original[..33].to_vec();
        chunk(&mut apng, b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
        apng.extend_from_slice(&original[33..]);
        assert!(matches!(
            decode(apng, ImageExpectations::default()),
            Err(AssetError::Unsupported(_))
        ));
    }
    #[test]
    fn decoder_read_bufread_and_seek_share_cancellation() {
        let stop = AtomicBool::new(false);
        let mut reader = CheckedCursor {
            inner: Cursor::new(&b"abc"[..]),
            cancel: Cancel::new(&stop),
        };
        assert_eq!(reader.read(&mut [0; 1]).unwrap(), 1);
        stop.store(true, Ordering::Release);
        assert!(reader.read(&mut [0; 1]).is_err());
        assert!(reader.fill_buf().is_err());
        assert!(reader.seek(SeekFrom::Start(0)).is_err());
    }
    #[test]
    fn identity_error_releases_its_byte_reservation() {
        let stop = AtomicBool::new(false);
        let budget = ByteBudget::new(1024).unwrap();
        let result = SourceBlob::new(
            vec![1, 2, 3],
            fixture_origin(OriginKind::DiagnosticFixture),
            Some(Digest256::of(b"different")),
            &Limits::default(),
            &budget,
            Cancel::new(&stop),
        );
        assert!(matches!(result, Err(AssetError::Integrity { .. })));
        assert_eq!(budget.used(), 0);
    }
}
