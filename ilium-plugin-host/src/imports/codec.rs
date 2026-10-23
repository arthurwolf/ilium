//! Host implementation of `ilium:host/codec` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCodec` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::codec::{
    Compression, DecodedImage, HostCodec, ImageInfo, RasterText,
};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `codec` interface.
#[derive(Debug)]
pub struct CodecImport;

impl HostCodec for CodecImport {
    fn inflate(
        &mut self,
        _format: Compression,
        _data: Vec<u8>,
        _max_output: u32,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::codec::inflate")
    }

    fn deflate(
        &mut self,
        _format: Compression,
        _data: Vec<u8>,
        _level: u8,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::codec::deflate")
    }

    fn image_info(&mut self, _data: Vec<u8>) -> Result<ImageInfo, PluginError> {
        unimplemented!("host::imports::codec::image_info")
    }

    fn decode_image(
        &mut self,
        _data: Vec<u8>,
        _frame: u32,
        _max_pixels: u32,
    ) -> Result<DecodedImage, PluginError> {
        unimplemented!("host::imports::codec::decode_image")
    }

    fn sha256(&mut self, _data: Vec<u8>) -> Vec<u8> {
        unimplemented!("host::imports::codec::sha256")
    }

    fn rasterize_text(&mut self, _request: RasterText) -> Result<DecodedImage, PluginError> {
        unimplemented!("host::imports::codec::rasterize_text")
    }
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
