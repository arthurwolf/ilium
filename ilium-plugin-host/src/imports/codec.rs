//! Host implementation of `ilium:host/codec` (reference-wit.md section 5.10).
//!
//! Pure functions on byte buffers: no grant, no state. Every call is bounded by the input cap, the
//! caller's `max-output` / `max-pixels` budget and the host-wide ceilings below.

use ilium_plugin_api::generated::host::codec::{
    Compression, DecodedImage, HostCodec, ImageInfo, RasterText,
};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};

/// Largest input accepted by `inflate`, `deflate`, `image-info` and `decode-image`: 16 MiB.
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Host ceiling for `inflate`'s `max-output`: 64 MiB.
const MAX_INFLATE_OUTPUT_BYTES: u32 = 67_108_864;
/// Host ceiling for `decode-image`'s `max-pixels` and for a rasterized text image.
const MAX_DECODE_PIXELS: u32 = 67_108_864;
/// Highest deflate level.
const MAX_DEFLATE_LEVEL: u8 = 9;
/// Largest text `rasterize-text` accepts: 64 KiB.
const MAX_RASTER_TEXT_BYTES: usize = 65_536;
/// Accepted `size-px` range of `rasterize-text`.
const RASTER_SIZE_RANGE: std::ops::RangeInclusive<u16> = 1..=512;
/// Accepted `max-width-px` range of `rasterize-text`.
const RASTER_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 1..=16_384;
/// Glyph grid of the built-in bitmap font.
const GLYPH_COLUMNS: usize = 5;
const GLYPH_ROWS: usize = 7;
/// Cell advance (glyph plus one blank column) and line height in unscaled pixels.
const CELL_ADVANCE: u32 = 6;
const LINE_HEIGHT: u32 = 8;

/// Built-in 5x7 column font for U+0020..=U+005F; bit 0 of a column byte is the top row.
/// Lower-case letters reuse the upper-case glyphs; other characters draw as a hollow box.
const FONT_5X7: [[u8; GLYPH_COLUMNS]; 64] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], [0x00, 0x00, 0x5F, 0x00, 0x00],
    [0x00, 0x07, 0x00, 0x07, 0x00], [0x14, 0x7F, 0x14, 0x7F, 0x14],
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50], [0x00, 0x05, 0x03, 0x00, 0x00],
    [0x00, 0x1C, 0x22, 0x41, 0x00], [0x00, 0x41, 0x22, 0x1C, 0x00],
    [0x14, 0x08, 0x3E, 0x08, 0x14], [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00], [0x08, 0x08, 0x08, 0x08, 0x08],
    [0x00, 0x60, 0x60, 0x00, 0x00], [0x20, 0x10, 0x08, 0x04, 0x02],
    [0x3E, 0x51, 0x49, 0x45, 0x3E], [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46], [0x21, 0x41, 0x45, 0x4B, 0x31],
    [0x18, 0x14, 0x12, 0x7F, 0x10], [0x27, 0x45, 0x45, 0x45, 0x39],
    [0x3C, 0x4A, 0x49, 0x49, 0x30], [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36], [0x06, 0x49, 0x49, 0x29, 0x1E],
    [0x00, 0x36, 0x36, 0x00, 0x00], [0x00, 0x56, 0x36, 0x00, 0x00],
    [0x08, 0x14, 0x22, 0x41, 0x00], [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08], [0x02, 0x01, 0x51, 0x09, 0x06],
    [0x32, 0x49, 0x79, 0x41, 0x3E], [0x7E, 0x11, 0x11, 0x11, 0x7E],
    [0x7F, 0x49, 0x49, 0x49, 0x36], [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x22, 0x1C], [0x7F, 0x49, 0x49, 0x49, 0x41],
    [0x7F, 0x09, 0x09, 0x09, 0x01], [0x3E, 0x41, 0x49, 0x49, 0x7A],
    [0x7F, 0x08, 0x08, 0x08, 0x7F], [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01], [0x7F, 0x08, 0x14, 0x22, 0x41],
    [0x7F, 0x40, 0x40, 0x40, 0x40], [0x7F, 0x02, 0x0C, 0x02, 0x7F],
    [0x7F, 0x04, 0x08, 0x10, 0x7F], [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06], [0x3E, 0x41, 0x51, 0x21, 0x5E],
    [0x7F, 0x09, 0x19, 0x29, 0x46], [0x46, 0x49, 0x49, 0x49, 0x31],
    [0x01, 0x01, 0x7F, 0x01, 0x01], [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F], [0x3F, 0x40, 0x38, 0x40, 0x3F],
    [0x63, 0x14, 0x08, 0x14, 0x63], [0x07, 0x08, 0x70, 0x08, 0x07],
    [0x61, 0x51, 0x49, 0x45, 0x43], [0x00, 0x7F, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20], [0x00, 0x41, 0x41, 0x7F, 0x00],
    [0x04, 0x02, 0x01, 0x02, 0x04], [0x40, 0x40, 0x40, 0x40, 0x40],
];
/// Glyph drawn for characters outside the built-in font.
const MISSING_GLYPH: [u8; GLYPH_COLUMNS] = [0x7F, 0x41, 0x41, 0x41, 0x7F];

/// Host side of the `codec` interface.
#[derive(Debug)]
pub struct CodecImport;

fn invalid(message: &str) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

fn check_input_size(len: usize) -> Result<(), PluginError> {
    if len > MAX_INPUT_BYTES {
        return Err(invalid("input is larger than 16 MiB"));
    }
    Ok(())
}

/// Reads at most `max_output + 1` bytes so an over-budget stream is detected without unbounded growth.
fn read_bounded<R: Read>(mut reader: R, max_output: u32) -> Result<Vec<u8>, PluginError> {
    let mut output = Vec::new();
    let limit = u64::from(max_output) + 1;
    reader
        .by_ref()
        .take(limit)
        .read_to_end(&mut output)
        .map_err(|error| invalid(&format!("corrupt compressed data: {error}")))?;
    if output.len() as u64 > u64::from(max_output) {
        return Err(PluginError::new(
            ErrorCode::QuotaExceeded,
            "decompressed output is larger than max-output",
        ));
    }
    Ok(output)
}

fn image_error(error: &image::ImageError) -> PluginError {
    match error {
        image::ImageError::Unsupported(_) => {
            PluginError::new(ErrorCode::Unsupported, "unsupported image format")
        }
        other => invalid(&format!("corrupt image data: {other}")),
    }
}

fn guess_format(data: &[u8]) -> Result<image::ImageFormat, PluginError> {
    image::guess_format(data)
        .map_err(|_| PluginError::new(ErrorCode::Unsupported, "unknown image format"))
}

impl HostCodec for CodecImport {
    fn inflate(
        &mut self,
        format: Compression,
        data: Vec<u8>,
        max_output: u32,
    ) -> Result<Vec<u8>, PluginError> {
        check_input_size(data.len())?;
        if max_output > MAX_INFLATE_OUTPUT_BYTES {
            return Err(invalid("max-output is above the 64 MiB host ceiling"));
        }
        let cursor = Cursor::new(data);
        match format {
            Compression::Zlib => read_bounded(flate2::read::ZlibDecoder::new(cursor), max_output),
            Compression::Gzip => read_bounded(flate2::read::GzDecoder::new(cursor), max_output),
            Compression::DeflateRaw => {
                read_bounded(flate2::read::DeflateDecoder::new(cursor), max_output)
            }
        }
    }

    fn deflate(
        &mut self,
        format: Compression,
        data: Vec<u8>,
        level: u8,
    ) -> Result<Vec<u8>, PluginError> {
        check_input_size(data.len())?;
        if level > MAX_DEFLATE_LEVEL {
            return Err(invalid("level must be 0 to 9"));
        }
        let level = flate2::Compression::new(u32::from(level));
        let failed = |error: std::io::Error| invalid(&format!("deflate failed: {error}"));
        match format {
            Compression::Zlib => {
                let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), level);
                encoder.write_all(&data).map_err(failed)?;
                encoder.finish().map_err(failed)
            }
            Compression::Gzip => {
                let mut encoder = flate2::write::GzEncoder::new(Vec::new(), level);
                encoder.write_all(&data).map_err(failed)?;
                encoder.finish().map_err(failed)
            }
            Compression::DeflateRaw => {
                let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), level);
                encoder.write_all(&data).map_err(failed)?;
                encoder.finish().map_err(failed)
            }
        }
    }

    fn image_info(&mut self, data: Vec<u8>) -> Result<ImageInfo, PluginError> {
        check_input_size(data.len())?;
        let format = guess_format(&data)?;
        let reader = image::ImageReader::with_format(Cursor::new(&data), format);
        let (width, height) = reader.into_dimensions().map_err(|e| image_error(&e))?;
        let frames = if format == image::ImageFormat::Gif {
            use image::AnimationDecoder;
            let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&data))
                .map_err(|e| image_error(&e))?;
            let count = decoder.into_frames().take_while(Result::is_ok).count();
            u32::try_from(count).unwrap_or(u32::MAX).max(1)
        } else {
            1
        };
        Ok(ImageInfo {
            width,
            height,
            frames,
        })
    }

    fn decode_image(
        &mut self,
        data: Vec<u8>,
        frame: u32,
        max_pixels: u32,
    ) -> Result<DecodedImage, PluginError> {
        check_input_size(data.len())?;
        if max_pixels > MAX_DECODE_PIXELS {
            return Err(invalid("max-pixels is above the 67,108,864 host ceiling"));
        }
        let format = guess_format(&data)?;
        let (width, height) = image::ImageReader::with_format(Cursor::new(&data), format)
            .into_dimensions()
            .map_err(|e| image_error(&e))?;
        if u64::from(width) * u64::from(height) > u64::from(max_pixels) {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "image has more pixels than max-pixels",
            ));
        }
        let buffer = if format == image::ImageFormat::Gif {
            use image::AnimationDecoder;
            let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&data))
                .map_err(|e| image_error(&e))?;
            let index = usize::try_from(frame).map_err(|_| invalid("frame is out of range"))?;
            match decoder.into_frames().nth(index) {
                Some(Ok(decoded)) => decoded.into_buffer(),
                Some(Err(error)) => return Err(image_error(&error)),
                None => return Err(invalid("frame is out of range")),
            }
        } else {
            if frame != 0 {
                return Err(invalid("frame is out of range"));
            }
            image::ImageReader::with_format(Cursor::new(&data), format)
                .decode()
                .map_err(|e| image_error(&e))?
                .to_rgba8()
        };
        Ok(DecodedImage {
            width: buffer.width(),
            height: buffer.height(),
            rgba: buffer.into_raw(),
        })
    }

    fn sha256(&mut self, data: Vec<u8>) -> Vec<u8> {
        Sha256::digest(&data).to_vec()
    }

    fn rasterize_text(&mut self, request: RasterText) -> Result<DecodedImage, PluginError> {
        if request.text.len() > MAX_RASTER_TEXT_BYTES {
            return Err(invalid("text is larger than 64 KiB"));
        }
        if !RASTER_SIZE_RANGE.contains(&request.size_px) {
            return Err(invalid("size-px must be 1 to 512"));
        }
        if !RASTER_WIDTH_RANGE.contains(&request.max_width_px) {
            return Err(invalid("max-width-px must be 1 to 16384"));
        }
        let scale = (u32::from(request.size_px) / LINE_HEIGHT).max(1);
        let cell_width = CELL_ADVANCE * scale;
        let line_height = LINE_HEIGHT * scale;
        let columns_per_line = ((request.max_width_px / cell_width) as usize).max(1);
        let lines = wrap_text(&request.text, columns_per_line);
        let widest = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);
        let width = (widest as u32 * cell_width)
            .min(request.max_width_px)
            .max(1);
        let height = (lines.len() as u32 * line_height).max(line_height);
        if u64::from(width) * u64::from(height) > u64::from(MAX_DECODE_PIXELS) {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "rasterized text would exceed the pixel budget",
            ));
        }
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        // White ink: RGB is always 255, the alpha channel carries the ink tone.
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[..3].copy_from_slice(&[255, 255, 255]);
        }
        for (line_index, line) in lines.iter().enumerate() {
            for (column_index, character) in line.chars().enumerate() {
                let glyph = glyph_for(character);
                let origin_x = column_index as u32 * cell_width;
                let origin_y = line_index as u32 * line_height;
                draw_glyph(&mut rgba, width, height, glyph, origin_x, origin_y, scale);
            }
        }
        Ok(DecodedImage { width, height, rgba })
    }
}

fn glyph_for(character: char) -> &'static [u8; GLYPH_COLUMNS] {
    let upper = character.to_ascii_uppercase();
    let code = upper as u32;
    if (0x20..=0x5F).contains(&code) {
        &FONT_5X7[(code - 0x20) as usize]
    } else {
        &MISSING_GLYPH
    }
}

fn draw_glyph(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    glyph: &[u8; GLYPH_COLUMNS],
    origin_x: u32,
    origin_y: u32,
    scale: u32,
) {
    for (column, bits) in glyph.iter().enumerate() {
        for row in 0..GLYPH_ROWS {
            if bits >> row & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let x = origin_x + column as u32 * scale + dx;
                    let y = origin_y + row as u32 * scale + dy;
                    if x < width && y < height {
                        let offset = (y as usize * width as usize + x as usize) * 4;
                        rgba[offset + 3] = 255;
                    }
                }
            }
        }
    }
}

/// Splits text into lines of at most `columns` characters, breaking at spaces where possible.
fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_len = 0usize;
        for word in paragraph.split(' ') {
            let mut word_chars: Vec<char> = word.chars().collect();
            // A word longer than a whole line is broken at the line width.
            while word_chars.len() > columns {
                if current_len > 0 {
                    lines.push(std::mem::take(&mut current));
                    current_len = 0;
                }
                let rest = word_chars.split_off(columns);
                lines.push(word_chars.iter().collect());
                word_chars = rest;
            }
            let needed = word_chars.len() + usize::from(current_len > 0);
            if current_len > 0 && current_len + needed > columns {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
            }
            if current_len > 0 {
                current.push(' ');
                current_len += 1;
            }
            current.extend(word_chars.iter());
            current_len += word_chars.len();
        }
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::CodecImport;
    use ilium_plugin_api::generated::host::codec::{
        Compression, DecodedImage, HostCodec, ImageInfo, RasterText,
    };
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    const ONE_MIB: usize = 1024 * 1024;
    const MAX_CODEC_INPUT_BYTES: usize = 16 * ONE_MIB;
    const MAX_INFLATE_OUTPUT_BYTES: u32 = 67_108_864;
    const MAX_DECODE_PIXELS: u32 = 67_108_864;
    const MAX_RASTER_TEXT_BYTES: usize = 65_536;

    const HELLO: &[u8] = b"hello";
    /// zlib.compress(b"hello", 6)
    const ZLIB_HELLO: &[u8] = &[
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];
    /// Raw deflate payload of b"hello", without zlib header and trailer.
    const RAW_DEFLATE_HELLO: &[u8] = &[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00];
    /// zlib.compress(b"", 6)
    const ZLIB_EMPTY: &[u8] = &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
    const SHA256_EMPTY: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    const SHA256_HELLO: [u8; 32] = [
        0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9, 0xe2,
        0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62, 0x93, 0x8b,
        0x98, 0x24,
    ];
    /// A 1 x 1 RGBA PNG. Its single pixel is RGBA [0x00, 0x00, 0xff, 0x7f].
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0x60, 0xf8, 0x5f, 0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    /// Asserts that a call returned an error with the expected code. An Ok result fails the test.
    fn assert_error_code<T: std::fmt::Debug>(
        result: Result<T, PluginError>,
        expected_code: ErrorCode,
    ) {
        match result {
            Ok(value) => panic!("expected {expected_code:?}, but the call returned Ok({value:?})"),
            Err(error) => assert_eq!(
                error.code, expected_code,
                "unexpected error message: {}",
                error.message
            ),
        }
    }

    fn png_padded_to(total_length: usize) -> Vec<u8> {
        let mut bytes = PNG_1X1.to_vec();
        bytes.resize(total_length, 0);
        bytes
    }

    /// codec is always granted: CodecImport answers without any grant or manifest entry.
    /// REQ: CAP-011
    #[test]
    fn codec_import_runs_without_any_grant() {
        let mut codec = CodecImport;
        assert_eq!(codec.sha256(HELLO.to_vec()), SHA256_HELLO.to_vec());
        let plain = codec
            .inflate(Compression::Zlib, ZLIB_HELLO.to_vec(), 16)
            .expect("codec must inflate with no grant");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, inflate: a zlib stream returns its plain bytes.
    #[test]
    fn inflate_zlib_stream_returns_plain_bytes() {
        let mut codec = CodecImport;
        let plain = codec
            .inflate(Compression::Zlib, ZLIB_HELLO.to_vec(), 16)
            .expect("a valid zlib stream must inflate");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, inflate: raw deflate data (no header) is accepted with the deflate-raw format.
    #[test]
    fn inflate_raw_deflate_returns_plain_bytes() {
        let mut codec = CodecImport;
        let plain = codec
            .inflate(Compression::DeflateRaw, RAW_DEFLATE_HELLO.to_vec(), 16)
            .expect("a valid raw deflate stream must inflate");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, inflate: an empty zlib stream returns zero bytes (empty output boundary).
    #[test]
    fn inflate_empty_zlib_stream_returns_empty_output() {
        let mut codec = CodecImport;
        let plain = codec
            .inflate(Compression::Zlib, ZLIB_EMPTY.to_vec(), 0)
            .expect("an empty zlib stream must inflate with max-output 0");
        assert!(plain.is_empty());
    }

    /// api-codec.md, inflate: output exactly equal to max-output is accepted (boundary).
    #[test]
    fn inflate_output_equal_to_max_output_succeeds() {
        let mut codec = CodecImport;
        let plain = codec
            .inflate(Compression::Zlib, ZLIB_HELLO.to_vec(), HELLO.len() as u32)
            .expect("output of exactly max-output bytes must be accepted");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, inflate: output longer than max-output is refused with quota-exceeded.
    #[test]
    fn inflate_output_one_byte_over_max_output_is_quota_exceeded() {
        let mut codec = CodecImport;
        let result = codec.inflate(
            Compression::Zlib,
            ZLIB_HELLO.to_vec(),
            (HELLO.len() - 1) as u32,
        );
        assert_error_code(result, ErrorCode::QuotaExceeded);
    }

    /// api-codec.md, inflate: max-output 0 refuses any non-empty output with quota-exceeded.
    #[test]
    fn inflate_max_output_zero_refuses_non_empty_output() {
        let mut codec = CodecImport;
        let result = codec.inflate(Compression::Zlib, ZLIB_HELLO.to_vec(), 0);
        assert_error_code(result, ErrorCode::QuotaExceeded);
    }

    /// api-codec.md, inflate: max-output of 64 MiB is the highest accepted value (boundary).
    #[test]
    fn inflate_max_output_at_64_mib_is_accepted() {
        let mut codec = CodecImport;
        let plain = codec
            .inflate(
                Compression::Zlib,
                ZLIB_HELLO.to_vec(),
                MAX_INFLATE_OUTPUT_BYTES,
            )
            .expect("max-output of exactly 64 MiB must be accepted");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, inflate: max-output above 64 MiB is refused with invalid-argument.
    #[test]
    fn inflate_max_output_above_64_mib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.inflate(
            Compression::Zlib,
            ZLIB_HELLO.to_vec(),
            MAX_INFLATE_OUTPUT_BYTES + 1,
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, inflate: corrupt compressed data is invalid-argument.
    #[test]
    fn inflate_corrupt_data_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.inflate(Compression::Zlib, vec![0x78, 0x9c, 0xff, 0xff], 16);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, inflate: a stream cut short before its end is corrupt data (invalid-argument).
    #[test]
    fn inflate_truncated_zlib_stream_is_invalid_argument() {
        let mut codec = CodecImport;
        let truncated = ZLIB_HELLO[..ZLIB_HELLO.len() - 4].to_vec();
        let result = codec.inflate(Compression::Zlib, truncated, 16);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, inflate: empty input is not a compressed stream and is invalid-argument.
    #[test]
    fn inflate_empty_input_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.inflate(Compression::Zlib, Vec::new(), 16);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, inflate: zlib bytes passed as gzip are corrupt for that format (invalid-argument).
    #[test]
    fn inflate_zlib_bytes_labelled_gzip_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.inflate(Compression::Gzip, ZLIB_HELLO.to_vec(), 16);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, Limits: input over 16 MiB is refused with invalid-argument.
    #[test]
    fn inflate_input_over_16_mib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.inflate(Compression::Zlib, vec![0u8; MAX_CODEC_INPUT_BYTES + 1], 16);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, deflate: zlib level 6 of b"hello" produces the reference zlib stream.
    #[test]
    fn deflate_zlib_level_6_matches_reference_bytes() {
        let mut codec = CodecImport;
        let packed = codec
            .deflate(Compression::Zlib, HELLO.to_vec(), 6)
            .expect("level 6 must deflate");
        assert_eq!(packed, ZLIB_HELLO);
    }

    /// api-codec.md, deflate: empty input deflates to the empty zlib stream.
    #[test]
    fn deflate_empty_input_produces_empty_zlib_stream() {
        let mut codec = CodecImport;
        let packed = codec
            .deflate(Compression::Zlib, Vec::new(), 6)
            .expect("empty input must deflate");
        assert_eq!(packed, ZLIB_EMPTY);
    }

    /// api-codec.md, deflate: level 0 (stored, lowest boundary) round-trips through inflate.
    #[test]
    fn deflate_level_0_round_trips() {
        let mut codec = CodecImport;
        let packed = codec
            .deflate(Compression::Zlib, HELLO.to_vec(), 0)
            .expect("level 0 must deflate");
        let plain = codec
            .inflate(Compression::Zlib, packed, HELLO.len() as u32)
            .expect("level 0 output must inflate back");
        assert_eq!(plain, HELLO);
    }

    /// api-codec.md, deflate: level 9 (highest boundary) is accepted.
    #[test]
    fn deflate_level_9_is_accepted() {
        let mut codec = CodecImport;
        let packed = codec
            .deflate(Compression::Zlib, HELLO.to_vec(), 9)
            .expect("level 9 must deflate");
        assert!(!packed.is_empty());
    }

    /// api-codec.md, deflate: level 10 is above the 0 to 9 range and is invalid-argument.
    #[test]
    fn deflate_level_10_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.deflate(Compression::Zlib, HELLO.to_vec(), 10);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, deflate: level 255 (u8 maximum) is invalid-argument.
    #[test]
    fn deflate_level_255_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.deflate(Compression::Zlib, HELLO.to_vec(), 255);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, deflate: the gzip format produces gzip output, which starts with the magic 1f 8b.
    #[test]
    fn deflate_gzip_output_starts_with_gzip_magic() {
        let mut codec = CodecImport;
        let packed = codec
            .deflate(Compression::Gzip, HELLO.to_vec(), 6)
            .expect("gzip must deflate");
        assert_eq!(&packed[..2], &[0x1f, 0x8b]);
    }

    /// api-codec.md, deflate: input over 16 MiB is refused with invalid-argument.
    #[test]
    fn deflate_input_over_16_mib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.deflate(Compression::Zlib, vec![0u8; MAX_CODEC_INPUT_BYTES + 1], 6);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, sha256: the digest of empty input is the known 32-byte SHA-256 value.
    #[test]
    fn sha256_of_empty_input_is_known_digest() {
        let mut codec = CodecImport;
        let digest = codec.sha256(Vec::new());
        assert_eq!(digest.len(), 32);
        assert_eq!(digest, SHA256_EMPTY.to_vec());
    }

    /// api-codec.md, sha256: the digest of b"hello" is the known 32-byte SHA-256 value.
    #[test]
    fn sha256_of_hello_is_known_digest() {
        let mut codec = CodecImport;
        let digest = codec.sha256(HELLO.to_vec());
        assert_eq!(digest, SHA256_HELLO.to_vec());
    }

    /// api-codec.md, image-info: a PNG header gives width, height and one frame.
    #[test]
    fn image_info_png_reports_size_and_one_frame() {
        let mut codec = CodecImport;
        let info = codec
            .image_info(PNG_1X1.to_vec())
            .expect("a valid PNG header must be read");
        assert_eq!(
            info,
            ImageInfo {
                width: 1,
                height: 1,
                frames: 1,
            }
        );
    }

    /// api-codec.md, image-info: data that matches no supported format is unsupported.
    #[test]
    fn image_info_unknown_format_is_unsupported() {
        let mut codec = CodecImport;
        let result = codec.image_info(b"not an image at all".to_vec());
        assert_error_code(result, ErrorCode::Unsupported);
    }

    /// api-codec.md, image-info: a PNG signature with its header cut off is corrupt (invalid-argument).
    #[test]
    fn image_info_truncated_png_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.image_info(PNG_SIGNATURE.to_vec());
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, Limits: input over 16 MiB is refused with invalid-argument before any parsing.
    #[test]
    fn image_info_input_over_16_mib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.image_info(png_padded_to(MAX_CODEC_INPUT_BYTES + 1));
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, decode-image: frame 0 of the 1 x 1 PNG decodes to its RGBA pixel.
    #[test]
    fn decode_image_png_frame_0_returns_rgba_pixels() {
        let mut codec = CodecImport;
        let image = codec
            .decode_image(PNG_1X1.to_vec(), 0, 1)
            .expect("the 1 x 1 PNG must decode within a 1 pixel budget");
        assert_eq!(
            image,
            DecodedImage {
                width: 1,
                height: 1,
                rgba: vec![0x00, 0x00, 0xff, 0x7f],
            }
        );
    }

    /// api-codec.md, decode-image: a frame at or above the frame count is invalid-argument.
    #[test]
    fn decode_image_frame_out_of_range_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.decode_image(PNG_1X1.to_vec(), 1, MAX_DECODE_PIXELS);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, decode-image: an image with more pixels than max-pixels is quota-exceeded.
    #[test]
    fn decode_image_image_over_max_pixels_is_quota_exceeded() {
        let mut codec = CodecImport;
        let result = codec.decode_image(PNG_1X1.to_vec(), 0, 0);
        assert_error_code(result, ErrorCode::QuotaExceeded);
    }

    /// api-codec.md, decode-image: max-pixels above 67,108,864 is invalid-argument.
    #[test]
    fn decode_image_max_pixels_above_host_cap_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.decode_image(PNG_1X1.to_vec(), 0, MAX_DECODE_PIXELS + 1);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, decode-image: max-pixels exactly at the 67,108,864 cap is accepted (boundary).
    #[test]
    fn decode_image_max_pixels_at_host_cap_is_accepted() {
        let mut codec = CodecImport;
        let image = codec
            .decode_image(PNG_1X1.to_vec(), 0, MAX_DECODE_PIXELS)
            .expect("max-pixels of exactly 67,108,864 must be accepted");
        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.rgba.len(), 4);
    }

    /// api-codec.md, decode-image: corrupt picture data is invalid-argument.
    #[test]
    fn decode_image_corrupt_png_is_invalid_argument() {
        let mut codec = CodecImport;
        let mut corrupt = PNG_1X1.to_vec();
        corrupt[40] ^= 0xff;
        corrupt[41] ^= 0xff;
        let result = codec.decode_image(corrupt, 0, MAX_DECODE_PIXELS);
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, decode-image: data that matches no supported format is unsupported.
    #[test]
    fn decode_image_unknown_format_is_unsupported() {
        let mut codec = CodecImport;
        let result = codec.decode_image(b"not an image at all".to_vec(), 0, MAX_DECODE_PIXELS);
        assert_error_code(result, ErrorCode::Unsupported);
    }

    /// api-codec.md, Limits: input over 16 MiB is refused with invalid-argument.
    #[test]
    fn decode_image_input_over_16_mib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.decode_image(
            png_padded_to(MAX_CODEC_INPUT_BYTES + 1),
            0,
            MAX_DECODE_PIXELS,
        );
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, rasterize-text: output is white pixels whose alpha is the ink tone, sized within max-width.
    #[test]
    fn rasterize_text_returns_white_ink_pixels_within_max_width() {
        let mut codec = CodecImport;
        let image = codec
            .rasterize_text(RasterText {
                text: "Hi".to_string(),
                size_px: 16,
                max_width_px: 200,
            })
            .expect("a short text must rasterize");
        assert!(image.width >= 1 && image.width <= 200);
        assert!(image.height >= 1);
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
        for pixel in image.rgba.chunks_exact(4) {
            assert_eq!(&pixel[..3], &[255, 255, 255], "RGB must be white");
        }
        assert!(
            image.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "visible text must have some non-zero ink tone"
        );
    }

    /// api-codec.md, rasterize-text: text wraps at word boundaries to max-width, so it gets taller.
    #[test]
    fn rasterize_text_wraps_at_word_boundaries_to_max_width() {
        let mut codec = CodecImport;
        let text = "alpha beta gamma delta epsilon".to_string();
        let single_line = codec
            .rasterize_text(RasterText {
                text: text.clone(),
                size_px: 16,
                max_width_px: 16_384,
            })
            .expect("single-line rasterize must succeed");
        let wrapped = codec
            .rasterize_text(RasterText {
                text,
                size_px: 16,
                max_width_px: 60,
            })
            .expect("wrapped rasterize must succeed");
        assert!(wrapped.width <= 60);
        assert!(wrapped.height > single_line.height);
    }

    /// api-codec.md, rasterize-text: empty text is not refused and yields a consistent image.
    #[test]
    fn rasterize_text_empty_text_is_accepted() {
        let mut codec = CodecImport;
        let image = codec
            .rasterize_text(RasterText {
                text: String::new(),
                size_px: 16,
                max_width_px: 100,
            })
            .expect("empty text must not be refused");
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
    }

    /// api-codec.md, rasterize-text: text of exactly 64 KiB is accepted (boundary).
    #[test]
    fn rasterize_text_64_kib_text_is_accepted() {
        let mut codec = CodecImport;
        let image = codec
            .rasterize_text(RasterText {
                text: "a".repeat(MAX_RASTER_TEXT_BYTES),
                size_px: 16,
                max_width_px: 16_384,
            })
            .expect("text of exactly 64 KiB must be accepted");
        assert!(image.width >= 1 && image.height >= 1);
    }

    /// api-codec.md, rasterize-text: text over 64 KiB is invalid-argument.
    #[test]
    fn rasterize_text_over_64_kib_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.rasterize_text(RasterText {
            text: "a".repeat(MAX_RASTER_TEXT_BYTES + 1),
            size_px: 16,
            max_width_px: 16_384,
        });
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, rasterize-text: size_px 0 is below the 1 to 512 range and is invalid-argument.
    #[test]
    fn rasterize_text_size_px_zero_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.rasterize_text(RasterText {
            text: "A".to_string(),
            size_px: 0,
            max_width_px: 100,
        });
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, rasterize-text: size_px 1 and 512 are the accepted boundaries.
    #[test]
    fn rasterize_text_size_px_boundaries_are_accepted() {
        let mut codec = CodecImport;
        for size_px in [1u16, 512u16] {
            let result = codec.rasterize_text(RasterText {
                text: "A".to_string(),
                size_px,
                max_width_px: 16_384,
            });
            assert!(result.is_ok(), "size_px {size_px} must be accepted");
        }
    }

    /// api-codec.md, rasterize-text: size_px 513 is above the 1 to 512 range and is invalid-argument.
    #[test]
    fn rasterize_text_size_px_513_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.rasterize_text(RasterText {
            text: "A".to_string(),
            size_px: 513,
            max_width_px: 100,
        });
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, rasterize-text: max_width_px 0 is below the 1 to 16,384 range and is invalid-argument.
    #[test]
    fn rasterize_text_max_width_zero_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.rasterize_text(RasterText {
            text: "A".to_string(),
            size_px: 16,
            max_width_px: 0,
        });
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// api-codec.md, rasterize-text: max_width_px 1 and 16,384 are the accepted boundaries.
    #[test]
    fn rasterize_text_max_width_boundaries_are_accepted() {
        let mut codec = CodecImport;
        for max_width_px in [1u32, 16_384u32] {
            let result = codec.rasterize_text(RasterText {
                text: "A".to_string(),
                size_px: 16,
                max_width_px,
            });
            assert!(
                result.is_ok(),
                "max_width_px {max_width_px} must be accepted"
            );
        }
    }

    /// api-codec.md, rasterize-text: max_width_px 16,385 is above the range and is invalid-argument.
    #[test]
    fn rasterize_text_max_width_above_16384_is_invalid_argument() {
        let mut codec = CodecImport;
        let result = codec.rasterize_text(RasterText {
            text: "A".to_string(),
            size_px: 16,
            max_width_px: 16_385,
        });
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// Hostile inputs to every decoding function must return a defined codec error code, never panic.
    /// REQ: SEC-340
    #[test]
    fn hostile_inputs_return_defined_error_codes_without_panic() {
        let mut codec = CodecImport;
        let defined_codes = [
            ErrorCode::InvalidArgument,
            ErrorCode::QuotaExceeded,
            ErrorCode::Unsupported,
        ];
        let hostile_inputs: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![0x00],
            vec![0xff; 4],
            b"GIF89a".to_vec(),
            PNG_SIGNATURE.to_vec(),
            [PNG_SIGNATURE, &[0xff; 32][..]].concat(),
            vec![0x78; 64],
        ];
        for input in hostile_inputs {
            let inflate_result = codec.inflate(Compression::Zlib, input.clone(), 1024);
            let info_result = codec.image_info(input.clone());
            let decode_result = codec.decode_image(input.clone(), 0, MAX_DECODE_PIXELS);
            for (operation, code) in [
                ("inflate", inflate_result.err().map(|error| error.code)),
                ("image-info", info_result.err().map(|error| error.code)),
                ("decode-image", decode_result.err().map(|error| error.code)),
            ] {
                let code = code
                    .unwrap_or_else(|| panic!("{operation} must refuse hostile input {input:?}"));
                assert!(
                    defined_codes.contains(&code),
                    "{operation} returned {code:?} for input {input:?}, which codec does not raise"
                );
            }
        }
    }
}
