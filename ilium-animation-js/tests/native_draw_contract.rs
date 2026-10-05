#![cfg(feature = "native-host")]
use ilium_animation_js::{
    native_draw::{DrawAuthority, DrawBinding, DrawLimits, NativeDraw, PreparedBlit},
    native_media::{ImageHandle, MediaLimits, NativeMedia},
    surface::{
        Blend, ColourSpace, Command, Data, Format, FrameMeta, Mode, NativeRenderer, Planes, Rect,
        Shape, Surface, SurfaceError, TextStyle, Update, VectorOp,
    },
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use ilium_platform::owned_worker::StopToken;
use image::{ImageBuffer, ImageFormat, Rgba};
use std::{collections::BTreeMap, io::Cursor};

fn quota(bytes: usize) -> QuotaGroup {
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
fn binding() -> DrawBinding {
    DrawBinding {
        package_digest: "b".repeat(64),
        instance_id: 13,
        plan_generation: 2,
        authorization_epoch: 7,
    }
}
fn shape(format: Format) -> Shape {
    Shape {
        cell_width: 4,
        cell_height: 2,
        mode: if format == Format::Mask8 {
            Mode::Cells
        } else {
            Mode::Pixels
        },
        format,
        update: Update::Retain,
        cell_rgb: false,
        colour_space: ColourSpace::Srgb,
    }
}
fn value(format: Format) -> Vec<f32> {
    match format {
        Format::Mono1 | Format::Mono8 | Format::Gray32 => vec![1.],
        Format::Rgb8 => vec![255.; 3],
        Format::Rgba8 => vec![255.; 4],
        _ => vec![255.],
    }
}
#[derive(Default)]
struct Authority {
    prepared: BTreeMap<String, PreparedBlit>,
    checks: usize,
    revoke_at: Option<usize>,
}
impl DrawAuthority for Authority {
    fn check_frame(&mut self, actual: &DrawBinding) -> Result<(), SurfaceError> {
        self.checks += 1;
        if actual != &binding() || self.revoke_at.is_some_and(|limit| self.checks >= limit) {
            Err(SurfaceError::Stale)
        } else {
            Ok(())
        }
    }
    fn prepared(&mut self, handle: &str, _: &DrawBinding) -> Result<PreparedBlit, SurfaceError> {
        self.prepared
            .get(handle)
            .cloned()
            .ok_or(SurfaceError::Invalid(
                "unknown authenticated prepared handle",
            ))
    }
}
fn native_surface(
    surface: &mut Surface,
    renderer: &mut impl NativeRenderer,
    commands: Vec<Command>,
) -> Result<(), SurfaceError> {
    let seed = surface.begin(1)?;
    let layout = seed.shape.layout()?;
    let planes = Planes {
        data: seed.data,
        touch: vec![0; layout.samples],
        order: vec![0; layout.samples],
        cell_rgb: seed.cell_rgb,
        colour_touch: seed.shape.cell_rgb.then(|| vec![0; layout.cells]),
        colour_order: seed.shape.cell_rgb.then(|| vec![0; layout.cells]),
    };
    surface.finish(
        FrameMeta {
            wire_version: 1,
            key: seed.key,
            shape: seed.shape,
            presented: true,
            error: None,
            commands,
        },
        planes,
        renderer,
    )?;
    Ok(())
}
fn png() -> Vec<u8> {
    let image = ImageBuffer::from_fn(8, 8, |x, y| {
        Rgba::<u8>(if (x == 0 || x == 2) && y == 0 {
            [0, 0, 255, 0]
        } else if x < 4 {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 255]
        })
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn clipped_lines_fill_triangles_paths_and_ellipses_publish_in_every_format() {
    let quota = quota(64 * 1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    for format in [
        Format::Mask8,
        Format::Mono1,
        Format::Mono8,
        Format::Gray8,
        Format::Gray32,
        Format::Rgb8,
        Format::Rgba8,
    ] {
        let shape = shape(format);
        for (op, points, fill, closed) in [
            (
                VectorOp::Line,
                vec![[-1000., 1.], [1000., 1.]],
                false,
                false,
            ),
            (
                VectorOp::Triangle,
                vec![[0., 0.], [7., 0.], [0., 7.]],
                true,
                true,
            ),
            (
                VectorOp::Path,
                vec![[0., 0.], [7., 0.], [7., 7.], [0., 7.]],
                true,
                true,
            ),
            (VectorOp::Ellipse, vec![[4., 4.], [3., 2.]], true, false),
        ] {
            let mut authority = Authority::default();
            let mut renderer = NativeDraw::new(
                &quota,
                shape,
                DrawLimits::default(),
                binding(),
                &media,
                &mut authority,
                &stop,
            )
            .unwrap();
            let mut surface = Surface::new(13, 1, shape).unwrap();
            native_surface(
                &mut surface,
                &mut renderer,
                vec![Command::Vector {
                    order: 1,
                    op,
                    points,
                    width: 1.,
                    fill,
                    closed,
                    value: value(format),
                    rgb: None,
                    blend: Blend::Overwrite,
                }],
            )
            .unwrap();
            let packed = surface
                .snapshot()
                .pack(|value, _, _| value, |value, _, _| value >= 0.5)
                .unwrap();
            assert!(
                packed.masks.iter().any(|mask| *mask != 0),
                "{format:?} {op:?}"
            );
            assert!(surface.snapshot().owners().iter().all(Option::is_none));
        }
    }
}
#[test]
fn work_and_remaining_sample_bounds_reject_before_publication() {
    let quota = quota(64 * 1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    let shape = shape(Format::Gray8);
    let command = Command::Vector {
        order: 1,
        op: VectorOp::Path,
        points: vec![[0., 0.], [8., 0.], [8., 8.], [0., 8.]],
        width: 1.,
        fill: true,
        closed: true,
        value: vec![255.],
        rgb: None,
        blend: Blend::Overwrite,
    };
    let mut authority = Authority::default();
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    assert!(matches!(
        renderer.render(&command, shape, 1),
        Err(SurfaceError::Capacity)
    ));
    drop(renderer);
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits {
            geometry_work: 1,
            text_bytes: 1024,
        },
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let mut surface = Surface::new(13, 1, shape).unwrap();
    assert!(matches!(
        native_surface(&mut surface, &mut renderer, vec![command]),
        Err(SurfaceError::Capacity)
    ));
    assert_eq!(surface.version(), 0);
    assert!(surface.snapshot().states().iter().all(|state| *state == 1));
}
#[test]
fn styled_unicode_clips_whole_wide_graphemes_without_losing_style() {
    let quota = quota(64 * 1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    let shape = shape(Format::Gray8);
    let mut authority = Authority::default();
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let style = TextStyle {
        rgb: Some([12, 34, 56]),
        background: None,
        bold: true,
        italic: true,
        underline: true,
    };
    let mut surface = Surface::new(13, 1, shape).unwrap();
    native_surface(
        &mut surface,
        &mut renderer,
        vec![
            Command::Text {
                order: 1,
                x: 0,
                y: 0,
                text: "e\u{301}".into(),
                style: style.clone(),
            },
            Command::Text {
                order: 2,
                x: 2,
                y: 0,
                text: "界tail".into(),
                style: style.clone(),
            },
            Command::Text {
                order: 3,
                x: 3,
                y: 1,
                text: "界".into(),
                style: style.clone(),
            },
        ],
    )
    .unwrap();
    let text: Vec<_> = surface.snapshot().text().collect();
    assert_eq!(text.len(), 2);
    assert_eq!(text[0].text, "e\u{301}");
    assert_eq!(text[1].text, "界");
    assert_eq!(text[1].width, 2);
    assert_eq!(text[1].style, style);
}
#[test]
fn authenticated_image_blits_convert_all_formats_and_preserve_exact_dot_tokens() {
    let quota = quota(256 * 1024 * 1024);
    let mut media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    let handle = media.decode(&png(), &stop).unwrap();
    let prepared = PreparedBlit::image(&media, handle, binding(), Some(43)).unwrap();
    for format in [
        Format::Mask8,
        Format::Mono1,
        Format::Mono8,
        Format::Gray8,
        Format::Gray32,
        Format::Rgb8,
        Format::Rgba8,
    ] {
        let shape = shape(format);
        let layout = shape.layout().unwrap();
        let mut authority = Authority {
            prepared: BTreeMap::from([("prepared_image".into(), prepared.clone())]),
            ..Default::default()
        };
        let mut renderer = NativeDraw::new(
            &quota,
            shape,
            DrawLimits::default(),
            binding(),
            &media,
            &mut authority,
            &stop,
        )
        .unwrap();
        let mut surface = Surface::new(13, 1, shape).unwrap();
        native_surface(
            &mut surface,
            &mut renderer,
            vec![Command::Blit {
                order: 1,
                handle: "prepared_image".into(),
                source: Rect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 8,
                },
                target: Rect {
                    x: 0,
                    y: 0,
                    width: layout.width as u32,
                    height: layout.height as u32,
                },
                blend: Blend::Overwrite,
            }],
        )
        .unwrap();
        let packed = surface
            .snapshot()
            .pack(|value, _, _| value, |value, _, _| value >= 0.5)
            .unwrap();
        assert!(packed.masks.iter().any(|mask| *mask != 0), "{format:?}");
        assert!(surface.snapshot().owners()[0].is_none());
        assert!(surface.snapshot().owners()[2].is_none());
        assert_eq!(surface.snapshot().owners()[8].unwrap().evidence_key(), 43);
        assert_eq!(surface.snapshot().owners()[1].unwrap().evidence_key(), 43);
        assert!(surface
            .snapshot()
            .owners()
            .iter()
            .flatten()
            .all(|token| token.evidence_key() == 43));
    }
}
#[test]
fn unknown_handle_foreign_binding_and_mid_render_revocation_roll_back() {
    let quota = quota(256 * 1024 * 1024);
    let mut media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    assert!(PreparedBlit::image(&media, ImageHandle::from_id(999), binding(), Some(43)).is_err());
    let handle = media.decode(&png(), &stop).unwrap();
    let mut foreign = binding();
    foreign.authorization_epoch += 1;
    let prepared = PreparedBlit::image(&media, handle, foreign, Some(43)).unwrap();
    let shape = shape(Format::Gray8);
    let command = Command::Blit {
        order: 1,
        handle: "image".into(),
        source: Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        target: Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        blend: Blend::Overwrite,
    };
    let mut authority = Authority::default();
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    assert!(renderer.render(&command, shape, 64).is_err());
    drop(renderer);
    authority.prepared.insert("image".into(), prepared);
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    assert!(matches!(
        renderer.render(&command, shape, 64),
        Err(SurfaceError::Stale)
    ));
    drop(renderer);
    let prepared = PreparedBlit::image(&media, handle, binding(), Some(43)).unwrap();
    authority.prepared.insert("image".into(), prepared);
    authority.checks = 0;
    authority.revoke_at = Some(3);
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let mut surface = Surface::new(13, 1, shape).unwrap();
    assert!(matches!(
        native_surface(&mut surface, &mut renderer, vec![command]),
        Err(SurfaceError::Stale)
    ));
    assert_eq!(surface.version(), 0);
    assert!(surface.snapshot().owners().iter().all(Option::is_none));
}
#[test]
fn mono1_patch_rows_keep_padding_clear_and_count_refusal_releases_scratch() {
    let quota = quota(1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let baseline = quota.snapshot().worker_bytes;
    let stop = StopToken::default();
    let shape = shape(Format::Mono1);
    let mut authority = Authority::default();
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let command = Command::Vector {
        order: 1,
        op: VectorOp::Line,
        points: vec![[1., 0.], [1., 7.]],
        width: 1.,
        fill: false,
        closed: false,
        value: vec![1.],
        rgb: None,
        blend: Blend::Overwrite,
    };
    let output = renderer.render(&command, shape, 64).unwrap();
    let patch = &output.patches[0];
    let Data::U8(data) = &patch.data else {
        panic!("mono1 byte plane")
    };
    let padding = 8 - patch.rect.width % 8;
    if padding < 8 {
        assert!(data.iter().all(|byte| byte & ((1 << padding) - 1) == 0));
    }
    drop(output);
    drop(renderer);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
    stop.stop();
    assert!(NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop
    )
    .is_err());
}

#[test]
fn prepared_world_blit_reuses_actual_retained_native_raster_without_redrawing() {
    use ilium_ambient::resources::AmbientResources;
    use ilium_animation_js::native_worlds::{
        GeneratedWorldSettings, WorldRenderRequest, WorldService,
    };
    use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig};
    use std::time::{Duration, SystemTime};
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 2,
        worker_bytes: 256 * 1024 * 1024,
    });
    let zero = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 1,
                priority: None,
                resident_bytes_per_thread: 1024,
            },
            io: zero,
            service: zero,
        },
    )
    .unwrap();
    let resources = AmbientResources::new(
        execution
            .client(ClientLimits {
                jobs: 2,
                service_jobs: 0,
                input_bytes: 1024,
                result_bytes: 1024,
            })
            .unwrap(),
    );
    let mut worlds = WorldService::new(resources, quota.clone(), 7).unwrap();
    let handle = worlds
        .insert_generated(GeneratedWorldSettings::default())
        .unwrap();
    let frame = worlds
        .render(
            handle,
            WorldRenderRequest {
                width: 32,
                height: 16,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: SystemTime::UNIX_EPOCH,
                pre_rendered: false,
            },
        )
        .unwrap();
    let stop = StopToken::default();
    let baseline = quota.snapshot().worker_bytes;
    // Generated geometry has no saved-state owner. Host authentication keeps
    // those dots uncredited instead of manufacturing history/source evidence.
    let prepared = PreparedBlit::world(
        frame.clone(),
        binding(),
        &quota,
        [255; 3],
        &stop,
        |actual, owner| {
            assert!(std::ptr::eq(actual, frame.as_ref()));
            assert_ne!(owner, 0);
            Ok(None)
        },
    )
    .unwrap();
    assert!(quota.snapshot().worker_bytes > baseline);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let mut authority = Authority {
        prepared: BTreeMap::from([("world_frame".into(), prepared)]),
        ..Default::default()
    };
    let shape = Shape {
        cell_width: 32,
        cell_height: 16,
        format: Format::Gray32,
        ..shape(Format::Gray32)
    };
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let output = renderer
        .render(
            &Command::Blit {
                order: 1,
                handle: "world_frame".into(),
                source: Rect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                },
                target: Rect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                },
                blend: Blend::Overwrite,
            },
            shape,
            4096,
        )
        .unwrap();
    let Data::F32(data) = &output.patches[0].data else {
        panic!("gray32 native world plane")
    };
    assert_eq!(data, &frame.raster().dots);
    assert!(output.patches[0].owners.iter().all(Option::is_none));
}

#[test]
fn native_srgb_images_convert_to_declared_linear_rgba_without_scaling_alpha_twice() {
    let quota = quota(256 * 1024 * 1024);
    let stop = StopToken::default();
    let mut media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let image = ImageBuffer::from_fn(1, 1, |_, _| Rgba([128_u8, 64, 32, 128]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    let handle = media.decode(&bytes.into_inner(), &stop).unwrap();
    let prepared = PreparedBlit::image(&media, handle, binding(), Some(43)).unwrap();
    let mut authority = Authority {
        prepared: BTreeMap::from([("rgba".into(), prepared)]),
        ..Default::default()
    };
    let shape = Shape {
        colour_space: ColourSpace::Linear,
        ..shape(Format::Rgba8)
    };
    let mut renderer = NativeDraw::new(
        &quota,
        shape,
        DrawLimits::default(),
        binding(),
        &media,
        &mut authority,
        &stop,
    )
    .unwrap();
    let output = renderer
        .render(
            &Command::Blit {
                order: 1,
                handle: "rgba".into(),
                source: Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                target: Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                blend: Blend::Alpha,
            },
            shape,
            1,
        )
        .unwrap();
    let Data::U8(data) = &output.patches[0].data else {
        panic!("rgba bytes")
    };
    assert_eq!(data, &[55, 13, 4, 128]);
}

#[test]
fn scratch_refusal_cannot_allocate_an_unadmitted_renderer() {
    let quota = quota(64 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let before = quota.snapshot().worker_bytes;
    let stop = StopToken::default();
    let mut authority = Authority::default();
    assert!(matches!(
        NativeDraw::new(
            &quota,
            shape(Format::Gray8),
            DrawLimits::default(),
            binding(),
            &media,
            &mut authority,
            &stop
        ),
        Err(SurfaceError::Capacity)
    ));
    assert_eq!(quota.snapshot().worker_bytes, before);
}

#[test]
fn vector_tint_colours_actual_coverage_without_painting_empty_bound_corners() {
    let quota = quota(64 * 1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    for format in [Format::Mask8, Format::Gray32] {
        let mut shape = shape(format);
        shape.cell_rgb = true;
        let mut authority = Authority::default();
        let mut renderer = NativeDraw::new(
            &quota,
            shape,
            DrawLimits::default(),
            binding(),
            &media,
            &mut authority,
            &stop,
        )
        .unwrap();
        let command = Command::Vector {
            order: 1,
            op: VectorOp::Line,
            points: vec![[0., 0.], [7., 7.]],
            width: 1.,
            fill: false,
            closed: false,
            value: value(format),
            rgb: Some([255, 0, 0]),
            blend: Blend::Overwrite,
        };
        let output = renderer.render(&command, shape, 64).unwrap();
        assert!(!output.colours.is_empty());
        assert!(output
            .colours
            .iter()
            .all(|(_, _, rgb)| *rgb == Some([255, 0, 0])));
        assert!(
            !output
                .colours
                .iter()
                .any(|(x, y, _)| (*x, *y) == (3, 0) || (*x, *y) == (0, 1)),
            "empty diagonal bounding-box corners must remain untouched"
        );
        let mut undeclared = shape;
        undeclared.cell_rgb = false;
        let mut undeclared_authority = Authority::default();
        let mut refused = NativeDraw::new(
            &quota,
            undeclared,
            DrawLimits::default(),
            binding(),
            &media,
            &mut undeclared_authority,
            &stop,
        )
        .unwrap();
        assert!(refused.render(&command, undeclared, 64).is_err());
    }
}

#[test]
fn native_text_raster_and_styled_spans_use_real_renderer_in_all_seven_formats() {
    let quota = quota(256 * 1024 * 1024);
    let media = NativeMedia::new(quota.clone(), MediaLimits::default()).unwrap();
    let stop = StopToken::default();
    for format in [
        Format::Mask8,
        Format::Mono1,
        Format::Mono8,
        Format::Gray8,
        Format::Gray32,
        Format::Rgb8,
        Format::Rgba8,
    ] {
        let shape = shape(format);
        let mut authority = Authority::default();
        let mut renderer = NativeDraw::new(
            &quota,
            shape,
            DrawLimits::default(),
            binding(),
            &media,
            &mut authority,
            &stop,
        )
        .unwrap();
        let raster = renderer
            .render(
                &Command::RasterText {
                    order: 1,
                    x: 0,
                    y: 0,
                    text: "A".into(),
                    font: "CascadiaCode-Regular".into(),
                    size_px: 8.,
                    intensity: 1.,
                    rgb: None,
                },
                shape,
                shape.layout().unwrap().samples,
            )
            .unwrap();
        assert_eq!(raster.patches.len(), 1, "{format:?}");
        assert!(raster.patches[0].state.contains(&2), "{format:?}");
        let spans = renderer
            .render(
                &Command::TextSpans {
                    order: 2,
                    x: 0,
                    y: 1,
                    max_cells: 4,
                    spans: vec![ilium_animation_js::surface::NativeSpan {
                        text: "e\u{301}界".into(),
                        style: TextStyle {
                            rgb: Some([255, 0, 0]),
                            background: Some([0, 64, 0]),
                            bold: true,
                            italic: true,
                            underline: true,
                        },
                    }],
                },
                shape,
                shape.layout().unwrap().samples,
            )
            .unwrap();
        assert_eq!(spans.text.len(), 2, "{format:?}");
        assert_eq!(
            (spans.text[0].text.as_str(), spans.text[0].width),
            ("e\u{301}", 1)
        );
        assert_eq!(
            (spans.text[1].text.as_str(), spans.text[1].width),
            ("界", 2)
        );
        assert_eq!(spans.text[1].style.background, Some([0, 64, 0]));
        assert!(spans.text[1].style.underline);
    }
}
