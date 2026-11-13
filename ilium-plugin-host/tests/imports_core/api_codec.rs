//! api-codec.md: `inflate`, `deflate`, `get-image-info`, `decode-image`, `sha256`, `rasterize-text`.

use ilium_plugin_api::generated::host::{Compression, RasterText};
use ilium_plugin_api::generated::types::ErrorCode;
use proptest::prelude::*;

use crate::support::*;

const MAX_INPUT: usize = 16 * MIB;
const ALL: [Compression; 3] = [
    Compression::Zlib,
    Compression::Gzip,
    Compression::DeflateRaw,
];

fn raster(text: &str, size_px: u16, max_width_px: u32) -> RasterText {
    RasterText {
        text: text.to_owned(),
        size_px,
        max_width_px,
    }
}

// ---- sha256 ----------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/codec.sha256
#[test]
fn codec_sha256_of_empty_input_matches_the_published_vector() {
    let mut fixture = fixture_t2(&[]);
    let digest = fixture.codec().sha256(Vec::new());
    assert_eq!(
        hex(&digest),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// REQ: WIT:ilium:host/codec.sha256
#[test]
fn codec_sha256_of_abc_matches_the_published_vector() {
    let mut fixture = fixture_t2(&[]);
    let digest = fixture.codec().sha256(b"abc".to_vec());
    assert_eq!(
        hex(&digest),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// REQ: WIT:ilium:host/codec.sha256
#[test]
fn codec_sha256_accepts_input_beyond_the_16_mib_cap_of_other_calls() {
    let mut fixture = fixture_t2(&[]);
    let digest = fixture.codec().sha256(vec![0u8; MAX_INPUT + 1]);
    assert_eq!(digest.len(), 32);
}

proptest! {
    // REQ: WIT:ilium:host/codec.sha256
    #[test]
    fn codec_sha256_is_32_bytes_and_deterministic(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut fixture = fixture_t2(&[]);
        let first = fixture.codec().sha256(data.clone());
        let second = fixture.codec().sha256(data);
        prop_assert_eq!(first.len(), 32);
        prop_assert_eq!(first, second);
    }
}

// ---- deflate / inflate -----------------------------------------------------------------------

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_known_zlib_stream_of_empty_input() {
    let mut fixture = fixture_t2(&[]);
    let stream = vec![0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
    assert!(expect_ok(fixture.codec().inflate(Compression::Zlib, stream, 1024)).is_empty());
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_known_raw_deflate_stream_of_empty_input() {
    let mut fixture = fixture_t2(&[]);
    assert!(expect_ok(
        fixture
            .codec()
            .inflate(Compression::DeflateRaw, vec![0x03, 0x00], 1024)
    )
    .is_empty());
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_output_exactly_at_max_output_is_returned() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(
        fixture
            .codec()
            .deflate(Compression::Zlib, vec![7u8; 1000], 6),
    );
    let out = expect_ok(fixture.codec().inflate(Compression::Zlib, packed, 1000));
    assert_eq!(out, vec![7u8; 1000]);
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_output_one_byte_over_max_output_quota_exceeded() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(
        fixture
            .codec()
            .deflate(Compression::Zlib, vec![7u8; 1001], 6),
    );
    assert_eq!(
        expect_code(fixture.codec().inflate(Compression::Zlib, packed, 1000)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_corrupt_data_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    for format in ALL {
        let code = expect_code(fixture.codec().inflate(
            format,
            vec![0xde, 0xad, 0xbe, 0xef, 0x01],
            1024,
        ));
        assert_eq!(code, ErrorCode::InvalidArgument, "{format:?}");
    }
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_truncated_stream_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let mut packed = expect_ok(fixture.codec().deflate(
        Compression::Gzip,
        ascii(5000).into_bytes(),
        6,
    ));
    packed.truncate(packed.len() / 2);
    assert_eq!(
        expect_code(fixture.codec().inflate(Compression::Gzip, packed, 100_000)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_rejects_every_truncated_stream_and_trailing_bytes() {
    let mut fixture = fixture_t2(&[]);
    for format in ALL {
        let packed = expect_ok(
            fixture
                .codec()
                .deflate(format, b"bounded stream".to_vec(), 6),
        );
        for end in 0..packed.len() {
            assert_eq!(
                expect_code(fixture.codec().inflate(format, packed[..end].to_vec(), 14)),
                ErrorCode::InvalidArgument,
                "{format:?} truncated at {end}"
            );
        }
        let mut trailing = packed;
        trailing.push(0);
        assert_eq!(
            expect_code(fixture.codec().inflate(format, trailing, 14)),
            ErrorCode::InvalidArgument,
            "{format:?} trailing byte"
        );
    }
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_zero_limit_accepts_empty_stream_and_rejects_output() {
    let mut fixture = fixture_t2(&[]);
    for format in ALL {
        let empty = expect_ok(fixture.codec().deflate(format, Vec::new(), 6));
        assert!(expect_ok(fixture.codec().inflate(format, empty, 0)).is_empty());
        let nonempty = expect_ok(fixture.codec().deflate(format, vec![1], 6));
        assert_eq!(
            expect_code(fixture.codec().inflate(format, nonempty, 0)),
            ErrorCode::QuotaExceeded
        );
    }
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_validates_checksums_even_at_the_output_limit() {
    let mut fixture = fixture_t2(&[]);
    for format in [Compression::Zlib, Compression::Gzip] {
        let mut packed = expect_ok(fixture.codec().deflate(format, vec![42; 8192], 6));
        let last = packed.len() - 1;
        packed[last] ^= 1;
        assert_eq!(
            expect_code(fixture.codec().inflate(format, packed, 8192)),
            ErrorCode::InvalidArgument,
            "{format:?} corrupt trailer"
        );
    }
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_rejects_a_nonfinal_empty_deflate_block() {
    let mut fixture = fixture_t2(&[]);
    // A complete empty stored block with BFINAL clear requires a following block.
    let raw = vec![0x00, 0x00, 0x00, 0xff, 0xff];
    assert_eq!(
        expect_code(fixture.codec().inflate(Compression::DeflateRaw, raw, 0)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_input_over_16_mib_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let code = expect_code(fixture.codec().inflate(
        Compression::Zlib,
        vec![0u8; MAX_INPUT + 1],
        1024,
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_max_output_above_64_mib_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(fixture.codec().deflate(Compression::Zlib, vec![1u8; 10], 6));
    let code = expect_code(
        fixture
            .codec()
            .inflate(Compression::Zlib, packed, 64 * MIB as u32 + 1),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/codec.inflate
#[test]
fn codec_inflate_max_output_of_exactly_64_mib_is_accepted() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(fixture.codec().deflate(Compression::Zlib, vec![1u8; 10], 6));
    assert_eq!(
        expect_ok(
            fixture
                .codec()
                .inflate(Compression::Zlib, packed, 64 * MIB as u32)
        )
        .len(),
        10
    );
}

/// REQ: WIT:ilium:host/codec.deflate
#[test]
fn codec_deflate_levels_0_to_9_are_accepted() {
    let mut fixture = fixture_t2(&[]);
    for level in 0..=9u8 {
        expect_ok(
            fixture
                .codec()
                .deflate(Compression::Zlib, ascii(300).into_bytes(), level),
        );
    }
}

/// REQ: WIT:ilium:host/codec.deflate
#[test]
fn codec_deflate_level_10_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(fixture.codec().deflate(Compression::Zlib, vec![1], 10)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.deflate
#[test]
fn codec_deflate_input_over_16_mib_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let code = expect_code(
        fixture
            .codec()
            .deflate(Compression::Zlib, vec![0u8; MAX_INPUT + 1], 1),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/codec.deflate
#[test]
fn codec_deflate_zlib_output_starts_with_the_zlib_header() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(
        fixture
            .codec()
            .deflate(Compression::Zlib, b"hello".to_vec(), 6),
    );
    assert_eq!(packed[0] & 0x0f, 8, "deflate method");
}

/// REQ: WIT:ilium:host/codec.deflate
#[test]
fn codec_deflate_gzip_output_starts_with_the_gzip_magic() {
    let mut fixture = fixture_t2(&[]);
    let packed = expect_ok(
        fixture
            .codec()
            .deflate(Compression::Gzip, b"hello".to_vec(), 6),
    );
    assert_eq!(&packed[..2], &[0x1f, 0x8b]);
}

proptest! {
    // REQ: WIT:ilium:host/codec.deflate, WIT:ilium:host/codec.inflate
    #[test]
    fn codec_deflate_then_inflate_returns_the_input(
        data in proptest::collection::vec(any::<u8>(), 0..4096),
        level in 0u8..=9,
        which in 0usize..3,
    ) {
        let mut fixture = fixture_t2(&[]);
        let packed = expect_ok(fixture.codec().deflate(ALL[which], data.clone(), level));
        let back = expect_ok(fixture.codec().inflate(ALL[which], packed, 1 << 20));
        prop_assert_eq!(back, data);
    }
}

// ---- images ----------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/codec.get-image-info
#[test]
fn codec_image_info_reports_width_height_and_one_frame_for_a_bmp() {
    let mut fixture = fixture_t2(&[]);
    let info = expect_ok(fixture.codec().get_image_info(bmp(3, 2, 0, 0, 255)));
    assert_eq!((info.width, info.height, info.frames), (3, 2, 1));
}

/// REQ: WIT:ilium:host/codec.get-image-info
/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_animated_gif_reports_and_decodes_each_frame() {
    // Two identical 1x1 frames, with a global black/white palette.
    let header = [
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff,
    ];
    let frame = [
        0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
    ];
    let mut data = header.to_vec();
    data.extend_from_slice(&frame);
    data.extend_from_slice(&frame);
    data.push(0x3b);
    let mut fixture = fixture_t2(&[]);
    let info = expect_ok(fixture.codec().get_image_info(data.clone()));
    assert_eq!((info.width, info.height, info.frames), (1, 1, 2));
    for index in 0..2 {
        let image = expect_ok(fixture.codec().decode_image(data.clone(), index, 1));
        assert_eq!((image.width, image.height, image.rgba.len()), (1, 1, 4));
    }
    assert_eq!(
        expect_code(fixture.codec().decode_image(data, 2, 1)),
        ErrorCode::InvalidArgument
    );
}

/// Supported still-image formats must round-trip their dimensions and RGBA shape.
#[test]
fn codec_common_still_formats_decode_to_rgba() {
    for format in [
        image::ImageFormat::Png,
        image::ImageFormat::Jpeg,
        image::ImageFormat::WebP,
    ] {
        let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            3,
            2,
            image::Rgb([40, 80, 120]),
        ));
        let mut encoded = std::io::Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, format)
            .expect("encode test image");
        let bytes = encoded.into_inner();
        let mut fixture = fixture_t2(&[]);
        let metadata = expect_ok(fixture.codec().get_image_info(bytes.clone()));
        assert_eq!(
            (metadata.width, metadata.height, metadata.frames),
            (3, 2, 1)
        );
        let decoded = expect_ok(fixture.codec().decode_image(bytes, 0, 6));
        assert_eq!(
            (decoded.width, decoded.height, decoded.rgba.len()),
            (3, 2, 24)
        );
    }
}

/// A GIF frame cannot allocate beyond the declared logical canvas.
#[test]
fn codec_gif_frame_larger_than_canvas_is_rejected() {
    let data = vec![
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255, 0x2c,
        0, 0, 0, 0, 2, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3b,
    ];
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(fixture.codec().decode_image(data, 0, 1)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.get-image-info
#[test]
fn codec_image_info_unknown_format_unsupported() {
    let mut fixture = fixture_t2(&[]);
    let tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    assert_eq!(
        expect_code(fixture.codec().get_image_info(tiff)),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/codec.get-image-info
#[test]
fn codec_image_info_corrupt_bmp_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let mut data = bmp(4, 4, 1, 2, 3);
    data.truncate(20);
    assert_eq!(
        expect_code(fixture.codec().get_image_info(data)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.get-image-info
#[test]
fn codec_image_info_input_over_16_mib_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(fixture.codec().get_image_info(vec![0u8; MAX_INPUT + 1])),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_rgba_length_is_width_times_height_times_4() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(
        fixture
            .codec()
            .decode_image(bmp(5, 3, 0, 0, 255), 0, 1_000_000),
    );
    assert_eq!((image.width, image.height), (5, 3));
    assert_eq!(image.rgba.len(), 5 * 3 * 4);
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_solid_red_bmp_decodes_to_red_opaque_pixels() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(
        fixture
            .codec()
            .decode_image(bmp(2, 2, 0, 0, 255), 0, 1_000_000),
    );
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_frame_one_of_a_single_frame_image_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(
            fixture
                .codec()
                .decode_image(bmp(2, 2, 0, 0, 0), 1, 1_000_000)
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_more_pixels_than_max_pixels_quota_exceeded() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(fixture.codec().decode_image(bmp(4, 4, 0, 0, 0), 0, 15)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_exactly_max_pixels_is_accepted() {
    let mut fixture = fixture_t2(&[]);
    expect_ok(fixture.codec().decode_image(bmp(4, 4, 0, 0, 0), 0, 16));
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_max_pixels_above_67108864_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(
            fixture
                .codec()
                .decode_image(bmp(1, 1, 0, 0, 0), 0, 67_108_865)
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_max_pixels_of_exactly_67108864_is_accepted() {
    let mut fixture = fixture_t2(&[]);
    expect_ok(
        fixture
            .codec()
            .decode_image(bmp(1, 1, 0, 0, 0), 0, 67_108_864),
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_unknown_format_unsupported() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(
            fixture
                .codec()
                .decode_image(vec![b'I', b'I', 42, 0, 0, 0, 0, 0], 0, 100)
        ),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_corrupt_data_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    let mut data = bmp(4, 4, 1, 2, 3);
    data.truncate(30);
    assert_eq!(
        expect_code(fixture.codec().decode_image(data, 0, 100)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.decode-image
#[test]
fn codec_decode_image_input_over_16_mib_invalid_argument() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(
        expect_code(
            fixture
                .codec()
                .decode_image(vec![0u8; MAX_INPUT + 1], 0, 100)
        ),
        ErrorCode::InvalidArgument
    );
}

proptest! {
    // REQ: WIT:ilium:host/codec.decode-image
    #[test]
    fn codec_decode_image_rgba_length_invariant(width in 1u32..24, height in 1u32..24, blue: u8, green: u8, red: u8) {
        let mut fixture = fixture_t2(&[]);
        let image = expect_ok(fixture.codec().decode_image(bmp(width, height, blue, green, red), 0, 1 << 20));
        prop_assert_eq!(image.rgba.len() as u32, width * height * 4);
        prop_assert_eq!(&image.rgba[..4], &[red, green, blue, 255][..]);
    }
}

// ---- rasterize-text --------------------------------------------------------------------------

/// Text must contain visible white ink, not just a correctly sized empty buffer.
#[test]
fn codec_rasterize_text_draws_white_ink() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(fixture.codec().rasterize_text(raster("Hello", 24, 400)));
    let ink: Vec<_> = image
        .rgba
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .collect();
    assert!(!ink.is_empty());
    assert!(ink.iter().all(|pixel| pixel[..3] == [255, 255, 255]));
}

/// Empty input still has a valid transparent raster.
#[test]
fn codec_rasterize_empty_text_is_transparent() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(fixture.codec().rasterize_text(raster("", 16, 400)));
    assert!(image.width > 0 && image.height > 0);
    assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0));
}

/// A request whose raster exceeds the session memory ceiling is refused before allocation.
#[test]
fn codec_rasterize_text_rejects_oversized_output() {
    let mut fixture = fixture_t2(&[]);
    let text = "W".repeat(4096);
    assert_eq!(
        expect_code(fixture.codec().rasterize_text(raster(&text, 512, 16_384))),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/codec.rasterize-text
#[test]
fn codec_rasterize_text_returns_rgba_of_matching_length() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(fixture.codec().rasterize_text(raster("Hello", 16, 400)));
    assert!(image.width >= 1 && image.height >= 1);
    assert_eq!(image.rgba.len() as u32, image.width * image.height * 4);
}

/// REQ: WIT:ilium:host/codec.rasterize-text
#[test]
fn codec_rasterize_text_width_never_exceeds_max_width() {
    let mut fixture = fixture_t2(&[]);
    let image = expect_ok(fixture.codec().rasterize_text(raster(&ascii(500), 16, 100)));
    assert!(image.width <= 100);
}

/// REQ: WIT:ilium:host/codec.rasterize-text
#[test]
fn codec_rasterize_text_size_boundaries_1_and_512_accepted_0_and_513_refused() {
    let mut fixture = fixture_t2(&[]);
    expect_ok(fixture.codec().rasterize_text(raster("a", 1, 64)));
    expect_ok(fixture.codec().rasterize_text(raster("a", 512, 1024)));
    assert_eq!(
        expect_code(fixture.codec().rasterize_text(raster("a", 0, 64))),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        expect_code(fixture.codec().rasterize_text(raster("a", 513, 1024))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.rasterize-text
#[test]
fn codec_rasterize_text_max_width_boundaries_1_and_16384_accepted_0_and_16385_refused() {
    let mut fixture = fixture_t2(&[]);
    expect_ok(fixture.codec().rasterize_text(raster("a", 8, 1)));
    expect_ok(fixture.codec().rasterize_text(raster("a", 8, 16_384)));
    assert_eq!(
        expect_code(fixture.codec().rasterize_text(raster("a", 8, 0))),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        expect_code(fixture.codec().rasterize_text(raster("a", 8, 16_385))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/codec.rasterize-text
#[test]
fn codec_rasterize_text_64_kib_accepted_and_one_byte_more_refused() {
    let mut fixture = fixture_t2(&[]);
    expect_ok(
        fixture
            .codec()
            .rasterize_text(raster(&ascii(64 * KIB), 8, 256)),
    );
    assert_eq!(
        expect_code(
            fixture
                .codec()
                .rasterize_text(raster(&ascii(64 * KIB + 1), 8, 256))
        ),
        ErrorCode::InvalidArgument
    );
}
