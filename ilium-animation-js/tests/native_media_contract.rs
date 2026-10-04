#![cfg(feature = "native-host")]
use ilium_animation_js::{
    native_media::{
        ImageHandle, MediaLimits, MeshTriangle, MeshVertex, NativeMedia, Sampling, TextSpan,
        TextStyle,
    },
    package::{Package, PackageLimits},
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use ilium_platform::owned_worker::StopToken;
use image::{ImageBuffer, ImageFormat, Rgba};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};
fn root_quota(bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: bytes,
    })
}
fn bank() -> (NativeMedia, QuotaGroup) {
    let quota = root_quota(512 * 1024 * 1024);
    (
        NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap(),
        quota,
    )
}
fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = ImageBuffer::from_fn(width, height, |x, y| Rgba(pixel(x, y)));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}
fn package(asset: &[u8]) -> Package {
    let files = [
        (
            "entry.mjs",
            b"export function plan(){return {};};".as_slice(),
        ),
        ("assets/image.png", asset),
    ];
    let inventory:Vec<_>=files.iter().map(|(path,bytes)|serde_json::json!({"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})).collect();
    let manifest = serde_json::json!({"api_version":1,"id":"native-media-contract","name":"Native media contract","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":inventory,"assets":inventory.iter().filter(|item| item["path"].as_str().is_some_and(|path| path.starts_with("assets/"))).collect::<Vec<_>>()});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, bytes) in files {
        archive.start_file(path, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.start_file("manifest.json", options).unwrap();
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    Package::from_bytes(
        &archive.finish().unwrap().into_inner(),
        PackageLimits::default(),
    )
    .unwrap()
}
#[test]
fn transparent_samples_resampling_and_source_over_do_not_bleed_invisible_colors() {
    let (mut media, _) = bank();
    let stop = StopToken::default();
    let handle = media
        .decode(
            &png(2, 1, |x, _| {
                if x == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 0]
                }
            }),
            &stop,
        )
        .unwrap();
    let samples = media
        .sample(
            handle,
            &[[0.5, 0.5], [-1., 0.5], [2., 0.5]],
            Sampling::Bilinear,
            &stop,
        )
        .unwrap();
    assert_eq!(
        samples.view(),
        &vec![[0.5, 0., 0., 0.5], [1., 0., 0., 1.], [0., 0., 0., 0.]]
    );
    for filter in [Sampling::Bilinear, Sampling::Triangle] {
        let small = media.resample(handle, 1, 1, filter, &stop).unwrap();
        let resized = media.snapshot(small).unwrap();
        let rgba = &resized.view().rgba;
        assert_eq!(rgba[0], 255);
        assert_eq!(rgba[2], 0);
        assert!(rgba[3].abs_diff(128) <= 1);
    }
    let blit = media
        .blit(
            handle,
            [2, 1],
            [0, 0, 2, 1],
            [0., 0., 1., 1.],
            [0, 255, 0, 255],
            &stop,
        )
        .unwrap();
    assert_eq!(blit.view().rgba, [255, 0, 0, 255, 0, 255, 0, 255]);
    let luma = media.luminance(handle, &stop).unwrap();
    assert!((luma.view()[0] - 0.2126).abs() < 1e-6);
    assert_eq!(luma.view()[1], 0.0);
    let palette = media
        .palette(handle, &[[0, 0, 0], [255, 0, 0]], &stop)
        .unwrap();
    assert_eq!(palette.view(), &[1, 0]);
}
#[test]
fn originals_and_sample_results_remain_charged_after_handle_and_bank_retirement() {
    let (mut media, quota) = bank();
    let stop = StopToken::default();
    let handle = media
        .decode(&png(3, 2, |_, _| [12, 34, 56, 255]), &stop)
        .unwrap();
    let image = media.snapshot(handle).unwrap();
    let original = image.view().rgba.as_ptr();
    let sampled = media
        .sample(handle, &[[0.5, 0.5]], Sampling::Nearest, &stop)
        .unwrap();
    media.close(handle).unwrap();
    assert!(media.snapshot(handle).is_err());
    drop(media);
    assert!(quota.snapshot().worker_bytes > 0);
    assert_eq!(image.view().rgba.as_ptr(), original);
    drop(image);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(sampled);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn dimensions_bomb_encoded_ceiling_budget_and_cancellation_publish_nothing() {
    let (mut media, quota) = bank();
    let baseline = quota.snapshot().worker_bytes;
    let stop = StopToken::default();
    let mut bomb = png(1, 1, |_, _| [1, 2, 3, 255]);
    bomb[16..20].copy_from_slice(&9000_u32.to_be_bytes());
    let mut crc = 0xffff_ffff_u32;
    for byte in &bomb[12..29] {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    bomb[29..33].copy_from_slice(&(!crc).to_be_bytes());
    assert!(media.decode(&bomb, &stop).is_err());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    let mut area_bomb = png(1, 1, |_, _| [1, 2, 3, 255]);
    area_bomb[16..20].copy_from_slice(&4096_u32.to_be_bytes());
    area_bomb[20..24].copy_from_slice(&2048_u32.to_be_bytes());
    let mut crc = 0xffff_ffff_u32;
    for byte in &area_bomb[12..29] {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    area_bomb[29..33].copy_from_slice(&(!crc).to_be_bytes());
    let error = media.decode(&area_bomb, &stop).unwrap_err();
    assert!(error.to_string().contains("dimensions/pixels"), "{error}");
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    let cancelled = StopToken::default();
    cancelled.stop();
    assert!(media.decode(&png(2, 2, |_, _| [0; 4]), &cancelled).is_err());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    let small_quota = root_quota(1024 * 1024);
    let mut constrained = NativeMedia::new(small_quota.clone(), MediaLimits::default()).unwrap();
    let before = small_quota.snapshot().worker_bytes;
    assert!(constrained
        .decode(&png(1, 1, |_, _| [255; 4]), &stop)
        .is_err());
    assert_eq!(small_quota.snapshot().worker_bytes, before);
    let limits = MediaLimits {
        encoded_bytes: 16,
        ..MediaLimits::default()
    };
    let mut bounded = NativeMedia::new(quota.clone(), limits).unwrap();
    assert!(bounded.decode(&bomb, &stop).is_err());
}
#[test]
fn thumbnail_mip_dimensions_handle_cap_and_nonfinite_input_are_explicit() {
    let (mut media, _) = bank();
    let stop = StopToken::default();
    let source = media
        .decode(&png(16, 8, |_, _| [20, 40, 60, 255]), &stop)
        .unwrap();
    let thumb = media.thumbnail(source, 4, 4, &stop).unwrap();
    let thumb = media.snapshot(thumb).unwrap();
    assert_eq!((thumb.view().width, thumb.view().height), (4, 2));
    let levels = media.mipmaps(source, 8, &stop).unwrap();
    let sizes: Vec<_> = levels
        .view()
        .iter()
        .map(|handle| {
            let image = media.snapshot(*handle).unwrap();
            (image.view().width, image.view().height)
        })
        .collect();
    assert_eq!(sizes, [(8, 4), (4, 2), (2, 1), (1, 1)]);
    assert!(media
        .sample(source, &[[f32::NAN, 0.]], Sampling::Bilinear, &stop)
        .is_err());
    let wide = media
        .decode(&png(8192, 1, |_, _| [20, 40, 60, 255]), &stop)
        .unwrap();
    assert!(
        media
            .resample(wide, 1, 8192, Sampling::Triangle, &stop)
            .is_err(),
        "vertical-first native filter intermediate must be admitted before allocation"
    );
    assert!(media.snapshot(ImageHandle::from_id(u64::MAX)).is_err());
    assert!(media.glyph_mask("8", f32::INFINITY, &stop).is_err());
    let quota = root_quota(512 * 1024 * 1024);
    let mut capped = NativeMedia::new(
        quota,
        MediaLimits {
            handles: 1,
            ..MediaLimits::default()
        },
    )
    .unwrap();
    let handle = capped.decode(&png(1, 1, |_, _| [0; 4]), &stop).unwrap();
    assert!(capped.decode(&png(1, 1, |_, _| [0; 4]), &stop).is_err());
    capped.close(handle).unwrap();
    let next = capped.decode(&png(1, 1, |_, _| [0; 4]), &stop).unwrap();
    assert_ne!(next.id(), handle.id());
    assert!(capped.snapshot(handle).is_err());
}
#[test]
fn immutable_asset_inventory_hash_and_read_are_bounded_and_retained() {
    let (media, quota) = bank();
    let asset = png(2, 2, |_, _| [10, 20, 30, 255]);
    let package = package(&asset);
    let stop = StopToken::default();
    let listed = media.asset_list(&package, "assets/", 16, &stop).unwrap();
    assert_eq!(listed.view().len(), 1);
    assert_eq!(listed.view()[0].name, "assets/image.png");
    assert_eq!(
        listed.view()[0].sha256,
        format!("{:x}", Sha256::digest(&asset))
    );
    assert_eq!(
        media
            .asset_hash(&package, "assets/image.png", &stop)
            .unwrap()
            .as_slice(),
        Sha256::digest(&asset).as_slice()
    );
    assert!(media.asset_read(&package, "entry.mjs", 100, &stop).is_err());
    assert!(media
        .asset_read(&package, "assets/image.png", 1, &stop)
        .is_err());
    let read = media
        .asset_read(&package, "assets/image.png", 4096, &stop)
        .unwrap();
    drop(listed);
    drop(media);
    drop(package);
    assert_eq!(read.view(), &asset);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(read);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
fn mesh_depth_and_native_total_tie_rank_are_order_independent() {
    let (mut media, _) = bank();
    let stop = StopToken::default();
    let red = media
        .decode(&png(1, 1, |_, _| [255, 0, 0, 255]), &stop)
        .unwrap();
    let blue = media
        .decode(&png(1, 1, |_, _| [0, 0, 255, 255]), &stop)
        .unwrap();
    let vertices = [
        MeshVertex {
            x: 0.,
            y: 0.,
            depth: 1.,
            uv: [0., 0.],
        },
        MeshVertex {
            x: 8.,
            y: 0.,
            depth: 1.,
            uv: [1., 0.],
        },
        MeshVertex {
            x: 0.,
            y: 8.,
            depth: 1.,
            uv: [0., 1.],
        },
    ];
    let a = MeshTriangle {
        vertices,
        texture: red,
        sort_key: 20,
    };
    let b = MeshTriangle {
        vertices,
        texture: blue,
        sort_key: 10,
    };
    let first = media.mesh(8, 8, &[a, b], &stop).unwrap();
    let reversed = media.mesh(8, 8, &[b, a], &stop).unwrap();
    assert_eq!(first.view().rgb, reversed.view().rgb);
    assert_eq!(first.view().depth, reversed.view().depth);
    assert_eq!(first.view().rgb[0], [0, 0, 255]);
    assert!(media.mesh(8, 8, &[a, a], &stop).is_err());
    let mut canvas = ilium_ambient::voxel_landscape::render::Canvas::new(8, 8);
    let v = vertices.map(|v| {
        ilium_ambient::voxel_landscape::render::Vertex::new(v.x, v.y, v.depth, v.uv[0], v.uv[1])
    });
    canvas.face_owned([v[0], v[1], v[2], v[2]], 10, |_, _| [0, 0, 255]);
    assert_eq!(first.view().rgb, canvas.colors);
    assert_eq!(first.view().depth, canvas.depth);
}
#[test]
fn real_font_coverage_and_unicode_continuations_preserve_native_contract() {
    let (media, _) = bank();
    let stop = StopToken::default();
    let glyph = media.glyph_mask("8", 20., &stop).unwrap();
    assert_eq!((glyph.view().width, glyph.view().height), (18, 28));
    assert!(glyph.view().mask.iter().any(|coverage| *coverage > 0));
    assert!(glyph.view().advance.is_finite() && glyph.view().advance > 0.);
    let style = TextStyle {
        foreground: Some([1, 2, 3]),
        bold: true,
        ..TextStyle::default()
    };
    let cells = media
        .styled_cells(
            &[TextSpan {
                text: "A界e\u{301}",
                style,
            }],
            4,
            &stop,
        )
        .unwrap();
    assert_eq!(cells.view().len(), 4);
    assert_eq!(cells.view()[1].width, 2);
    assert_eq!(cells.view()[2].x, 2);
    assert!(cells.view()[2].continuation);
    assert!(cells.view()[2].glyph.is_empty());
    assert_eq!(cells.view()[3].glyph, "e\u{301}");
    assert_eq!(cells.view()[3].style, style);
    for text in ["\u{301}", "\x1b[31m", "\n"] {
        assert!(media
            .styled_cells(&[TextSpan { text, style }], 8, &stop)
            .is_err());
    }
    assert!(media
        .styled_cells(&[TextSpan { text: "界", style }], 1, &stop)
        .is_err());
}
