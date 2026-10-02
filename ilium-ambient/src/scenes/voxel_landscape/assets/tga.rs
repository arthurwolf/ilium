//! TGA envelope validation plus the installed image decoder. Original bytes and
//! BGRA alpha are retained; packet overruns are rejected before upstream decode.
use super::{
    budget::{zeroed_bytes, ByteBudget, Cancel, Limits, Reservation},
    error::{AssetError, Result},
    identity::{Digest256, SourceBlob},
    pixels::ImageExpectations,
};
use image::{codecs::tga::TgaDecoder, ColorType, ImageDecoder};
use std::io::{self, Cursor, Read};
fn bad(message: &str) -> AssetError {
    AssetError::InvalidImage(format!("TGA: {message}"))
}
struct CheckedReader<'a> {
    cursor: Cursor<&'a [u8]>,
    cancel: Cancel<'a>,
}
impl Read for CheckedReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("TGA cancelled"));
        }
        let count = bytes.len().min(65536);
        self.cursor.read(&mut bytes[..count])
    }
}
#[derive(Debug)]
pub(crate) struct DecodedTga {
    pub dimensions: [u32; 2],
    pub rgba: Vec<u8>,
    pub bits: u8,
    pub reservation: Reservation,
}
fn preflight(bytes: &[u8], limits: &Limits, cancel: Cancel<'_>) -> Result<[u32; 2]> {
    cancel.check()?;
    if bytes.len() < 18 {
        return Err(bad("truncated header"));
    }
    let width = u32::from(u16::from_le_bytes([bytes[12], bytes[13]]));
    let height = u32::from(u16::from_le_bytes([bytes[14], bytes[15]]));
    limits.rgba_bytes(width, height)?;
    if bytes[1] > 1
        || ![1, 2, 3, 9, 10, 11].contains(&bytes[2])
        || ![8, 15, 16, 24, 32].contains(&bytes[16])
        || bytes[17] & 0xc0 != 0
    {
        return Err(AssetError::Unsupported(
            "TGA type/depth/interleaving".into(),
        ));
    }
    let indexed = matches!(bytes[2], 1 | 9);
    if indexed && (bytes[1] != 1 || ![8, 16].contains(&bytes[16])) {
        return Err(bad("indexed header disagreement"));
    }
    let palette_count = usize::from(u16::from_le_bytes([bytes[5], bytes[6]]));
    let palette_stride = usize::from(bytes[7]).div_ceil(8);
    if bytes[1] == 1 && ![15, 16, 24, 32].contains(&bytes[7]) {
        return Err(bad("unsupported palette entry"));
    }
    let palette_bytes = if bytes[1] == 1 {
        palette_count * palette_stride
    } else {
        0
    };
    let mut cursor = 18 + usize::from(bytes[0]) + palette_bytes;
    let stride = usize::from(bytes[16]).div_ceil(8);
    let pixels = u64::from(width) * u64::from(height);
    if bytes[2] < 8 {
        let needed =
            u64::try_from(cursor).map_err(|_| AssetError::Allocation)? + pixels * stride as u64;
        if needed > bytes.len() as u64 {
            return Err(bad("truncated uncompressed pixels"));
        }
        return Ok([width, height]);
    }
    let mut remaining = pixels;
    while remaining > 0 {
        cancel.check()?;
        let packet = *bytes
            .get(cursor)
            .ok_or_else(|| bad("truncated RLE packet"))?;
        cursor += 1;
        let count = u64::from(packet & 0x7f) + 1;
        if count > remaining {
            return Err(bad("RLE packet exceeds declared pixel count"));
        }
        let payload = if packet & 0x80 != 0 {
            stride
        } else {
            count as usize * stride
        };
        cursor = cursor
            .checked_add(payload)
            .ok_or_else(|| bad("RLE offset overflow"))?;
        if cursor > bytes.len() {
            return Err(bad("truncated RLE payload"));
        }
        remaining -= count;
    }
    Ok([width, height])
}
pub(crate) fn decode(
    blob: &SourceBlob,
    expected: ImageExpectations,
    limits: &Limits,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<DecodedTga> {
    cancel.check()?;
    if !blob.uses_budget(budget) {
        return Err(bad("source and decoder accounts differ"));
    }
    let dimensions = preflight(blob.bytes(), limits, cancel)?;
    if expected.dimensions.is_some_and(|value| value != dimensions) {
        return Err(bad("pinned dimensions disagree"));
    }
    let count = limits.rgba_bytes(dimensions[0], dimensions[1])?;
    let reservation = budget.reserve(count as u64 + 4096, cancel)?;
    let _scratch = budget.reserve(limits.decoder_scratch_bytes, cancel)?;
    let mut decoder = TgaDecoder::new(CheckedReader {
        cursor: Cursor::new(blob.bytes()),
        cancel,
    })
    .map_err(|e| bad(&super::error::summary(&e.to_string())))?;
    let mut codec_limits = image::Limits::default();
    codec_limits.max_alloc = Some(limits.decoder_scratch_bytes);
    codec_limits.max_image_width = None;
    codec_limits.max_image_height = None;
    decoder
        .set_limits(codec_limits)
        .map_err(|e| bad(&super::error::summary(&e.to_string())))?;
    if decoder.dimensions() != (dimensions[0], dimensions[1]) {
        return Err(bad("decoder dimensions disagree"));
    }
    let color = decoder.color_type();
    let channels = match color {
        ColorType::L8 => 1,
        ColorType::La8 => 2,
        ColorType::Rgb8 => 3,
        ColorType::Rgba8 => 4,
        _ => {
            return Err(AssetError::Unsupported(
                "TGA decoder output color type".into(),
            ))
        }
    };
    let raw_count = count / 4 * channels;
    let _raw_charge = budget.reserve(raw_count as u64, cancel)?;
    let mut raw = zeroed_bytes(raw_count)?;
    let decoded = decoder.read_image(&mut raw);
    cancel.check()?;
    decoded.map_err(|e| bad(&super::error::summary(&e.to_string())))?;
    let mut rgba = zeroed_bytes(count)?;
    for (index, (source, pixel)) in raw
        .chunks_exact(channels)
        .zip(rgba.chunks_exact_mut(4))
        .enumerate()
    {
        if index % 4096 == 0 {
            cancel.check()?;
        }
        let gray = channels <= 2;
        pixel[0] = source[0];
        pixel[1] = source[if gray { 0 } else { 1 }];
        pixel[2] = source[if gray { 0 } else { 2 }];
        pixel[3] = if channels == 2 || channels == 4 {
            source[channels - 1]
        } else {
            255
        };
    }
    let actual = Digest256::of_checked(&rgba, cancel)?;
    if let Some(pin) = expected.rgba_sha256 {
        if actual != pin {
            return Err(AssetError::Integrity {
                expected: pin.to_string(),
                actual: actual.to_string(),
            });
        }
    }
    let depth = if blob.bytes()[1] == 1 {
        blob.bytes()[7]
    } else {
        blob.bytes()[16]
    };
    Ok(DecodedTga {
        dimensions,
        rgba,
        bits: if matches!(depth, 15 | 16) && channels >= 3 {
            5
        } else {
            8
        },
        reservation,
    })
}
#[cfg(test)]
mod tests {
    use super::super::pixels::PixelImage;
    use super::super::{review::fixture_origin, OriginKind};
    use super::*;
    use std::sync::atomic::AtomicBool;
    pub(crate) fn synthetic_tga(right: bool, top: bool, rle: bool) -> Vec<u8> {
        let mut bytes = vec![0; 18];
        bytes[2] = if rle { 10 } else { 2 };
        bytes[12] = 2;
        bytes[14] = 2;
        bytes[16] = 32;
        bytes[17] = 8 | if right { 16 } else { 0 } | if top { 32 } else { 0 };
        let pixels = [
            [255, 0, 0, 255],
            [0, 255, 0, 0],
            [0, 0, 255, 153],
            [100, 120, 140, 255],
        ];
        if rle {
            bytes.push(3);
        }
        for row in 0..2 {
            for column in 0..2 {
                let x = if right { 1 - column } else { column };
                let y = if top { row } else { 1 - row };
                let [red, green, blue, alpha] = pixels[y * 2 + x];
                bytes.extend([blue, green, red, alpha]);
            }
        }
        bytes
    }
    #[test]
    fn rgba_tga_all_four_origins_and_rle_retain_alpha_and_original_hashes() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let budget = ByteBudget::new(128 << 20).unwrap();
        let limits = Limits::default();
        for right in [false, true] {
            for top in [false, true] {
                for rle in [false, true] {
                    let bytes = synthetic_tga(right, top, rle);
                    let hash = Digest256::of(&bytes);
                    let blob = SourceBlob::new(
                        bytes,
                        fixture_origin(OriginKind::DiagnosticFixture),
                        None,
                        &limits,
                        &budget,
                        cancel,
                    )
                    .unwrap();
                    let image = PixelImage::decode_tga(
                        &blob,
                        ImageExpectations::default(),
                        &limits,
                        &budget,
                        cancel,
                    )
                    .unwrap();
                    assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
                    assert_eq!(image.pixel(1, 0), Some([0, 255, 0, 0]));
                    assert_eq!(image.pixel(0, 1), Some([0, 0, 255, 153]));
                    assert_eq!(image.source_sha256(), hash);
                    assert_eq!(image.info().partially_transparent_pixels, 1);
                }
            }
        }
    }
    #[test]
    fn oversized_rle_packet_is_rejected_instead_of_silently_clamped() {
        let mut bytes = synthetic_tga(false, true, true);
        bytes[18] = 127;
        let stop = AtomicBool::new(false);
        assert!(preflight(&bytes, &Limits::default(), Cancel::new(&stop)).is_err());
    }
}
