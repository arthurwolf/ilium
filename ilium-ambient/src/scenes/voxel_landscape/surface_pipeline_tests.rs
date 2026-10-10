//! Original geometry/pixel fixtures. These tests cannot establish native selected-pack visual acceptance.
use super::{
    assets::{
        animation::{AnimationEvidence, MissingAnimation},
        bank::{TextureBank, TextureBankBuilder, TextureRequirement},
        block_state::BlockState,
        budget::{ByteBudget, Cancel, Limits},
        compatibility::DefinitionSet,
        error::AssetError,
        identity::{Label, OriginKind, ResourceId},
        models::ModelCompiler,
        review::{fixture_origin, fixture_review},
        texture::{fixture_texture, Encoding, LinearRgba},
    },
    surface_fluid::{FluidCell, FluidMesh},
    surface_mesh::{
        AlphaMode, BoundModel, FaceOwner, MaterialTable, MeshRegion, PreparedMesh,
        TextureRenderRule,
    },
    surface_raster::{
        draw_fluid_mesh, draw_mesh, draw_mesh_inner, linear_to_srgb_byte, DirectionalLight,
        FlatShader, FragmentShader, PixelResult, RasterFrame, RasterLimits, Vertex,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
fn owner(layer: u16) -> FaceOwner {
    FaceOwner {
        position: [i32::from(layer), 0, 0],
        part: 0,
        face: 0,
        layer,
    }
}
fn quad(depth: f64) -> [Vertex; 4] {
    [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]].map(|[x, y]| Vertex {
        x,
        y,
        depth,
        uv: [(x / 4.0) as f32, (y / 4.0) as f32],
    })
}
fn rgba(rgb: [f32; 3], alpha: f32) -> LinearRgba {
    LinearRgba::from_straight(rgb, alpha).unwrap()
}
#[test]
fn clipped_tiles_do_not_exhaust_the_global_visible_triangle_budget() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut frame = RasterFrame::new(
        [4, 4],
        RasterLimits {
            triangles: 1,
            ..RasterLimits::default()
        },
        &budget,
        cancel,
    )
    .unwrap();
    let mut outside = quad(1.0);
    for vertex in &mut outside {
        vertex.x += 100.0;
    }
    for _ in 0..2 {
        frame
            .triangle(
                [outside[0], outside[1], outside[2]],
                owner(1),
                AlphaMode::Opaque,
                &FlatShader(rgba([1.0, 0.0, 0.0], 1.0)),
                cancel,
            )
            .unwrap();
    }
    let inside = quad(2.0);
    frame
        .triangle(
            [inside[0], inside[1], inside[2]],
            owner(2),
            AlphaMode::Opaque,
            &FlatShader(rgba([0.0, 1.0, 0.0], 1.0)),
            cancel,
        )
        .unwrap();
    assert_eq!(frame.pixel(2, 1).unwrap().front_owner, Some(owner(2)));
    assert!(frame.sample_tests() <= 16);
}

#[test]
fn wholly_offscreen_quads_keep_finite_submission_work_without_raster_attempts() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut frame = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    let shader = FlatShader(rgba([1.0, 0.0, 0.0], 1.0));
    let mut outside = quad(1.0);
    for vertex in &mut outside {
        vertex.x += 100.0;
    }
    frame
        .quad(outside, owner(1), AlphaMode::Opaque, &shader, cancel)
        .unwrap();
    assert_eq!(frame.submitted_quads(), 1);
    assert_eq!(frame.projected_attempts(), 0);
    assert_eq!(frame.sample_tests(), 0);
    frame
        .quad(quad(2.0), owner(2), AlphaMode::Opaque, &shader, cancel)
        .unwrap();
    assert_eq!(frame.submitted_quads(), 2);
    assert_eq!(frame.projected_attempts(), 2);
    assert_eq!(frame.pixel(2, 1).unwrap().front_owner, Some(owner(2)));
    let mut invalid = outside;
    invalid[3].x = f64::INFINITY;
    assert!(frame
        .quad(invalid, owner(3), AlphaMode::Opaque, &shader, cancel)
        .is_err());
    assert!(!frame.is_valid());
}

#[test]
fn bounding_box_without_a_covered_sample_does_not_use_visible_triangle_budget() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut frame = RasterFrame::new(
        [4, 4],
        RasterLimits {
            triangles: 1,
            ..RasterLimits::default()
        },
        &budget,
        cancel,
    )
    .unwrap();
    let full = quad(1.0);
    frame
        .triangle(
            [full[0], full[1], full[2]],
            owner(1),
            AlphaMode::Opaque,
            &FlatShader(rgba([1.0, 0.0, 0.0], 1.0)),
            cancel,
        )
        .unwrap();
    let tiny = [[0.0, 0.0], [0.2, 0.0], [0.0, 0.2]].map(|[x, y]| Vertex {
        x,
        y,
        depth: 2.0,
        uv: [0.0; 2],
    });
    frame
        .triangle(
            tiny,
            owner(2),
            AlphaMode::Opaque,
            &FlatShader(rgba([0.0, 1.0, 0.0], 1.0)),
            cancel,
        )
        .unwrap();
    assert_eq!(frame.projected_attempts(), 2);
    assert_eq!(frame.visible_triangles(), 1);
    assert_eq!(frame.pixel(0, 0).unwrap().front_owner, Some(owner(1)));
}
#[test]
fn top_left_coverage_prevents_double_blending_on_the_shared_triangle_diagonal() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    let paint = FlatShader(rgba([0.4, 0.6, 0.8], 0.5));
    for reverse in [false, true] {
        raster.clear();
        let mut vertices = quad(1.0);
        if reverse {
            vertices.reverse();
        }
        raster
            .quad(vertices, owner(1), AlphaMode::Blend, &paint, cancel)
            .unwrap();
        for y in 0..4 {
            for x in 0..4 {
                let result = raster.pixel(x, y).unwrap();
                assert_eq!(result.color.alpha(), 0.5);
                assert_eq!(result.contributors.iter().flatten().count(), 1);
            }
        }
    }
}
#[test]
fn translucent_layers_and_riverbed_composite_identically_in_all_submission_orders() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let paints = [
        rgba([0.4, 0.2, 0.1], 1.0),
        rgba([0.1, 0.6, 0.8], 0.3),
        rgba([0.6, 0.1, 0.2], 0.5),
    ];
    let expected = paints[2].over(paints[1].over(paints[0]));
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        for index in order {
            raster
                .quad(
                    quad((index + 1) as f64),
                    owner(index as u16),
                    if index == 0 {
                        AlphaMode::Opaque
                    } else {
                        AlphaMode::Blend
                    },
                    &FlatShader(paints[index]),
                    cancel,
                )
                .unwrap();
        }
        let result = raster.pixel(2, 2).unwrap();
        assert_eq!(result.color, expected);
        assert_eq!(
            result
                .contributors
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            vec![owner(0), owner(1), owner(2)]
        );
    }
}
#[test]
fn opaque_black_is_an_occluder_but_zero_alpha_is_not() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    for order in [[0, 1], [1, 0]] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        for index in order {
            raster
                .quad(
                    quad(index as f64 + 1.0),
                    owner(index),
                    AlphaMode::Opaque,
                    &FlatShader(rgba(
                        if index == 0 {
                            [1.0, 0.0, 0.0]
                        } else {
                            [0.0; 3]
                        },
                        1.0,
                    )),
                    cancel,
                )
                .unwrap();
        }
        raster
            .quad(
                quad(3.0),
                owner(2),
                AlphaMode::Blend,
                &FlatShader(LinearRgba::CLEAR),
                cancel,
            )
            .unwrap();
        let pixel = raster.pixel(1, 1).unwrap();
        assert_eq!(pixel.color, rgba([0.0; 3], 1.0));
        assert_eq!(pixel.front_owner, Some(owner(1)));
        assert_eq!(pixel.contributors.iter().flatten().count(), 1);
    }
}
struct FoliageShader;
impl FragmentShader for FoliageShader {
    fn sample(&self, uv: [f32; 2]) -> super::assets::Result<LinearRgba> {
        Ok(rgba([0.0, 1.0, 0.0], if uv[0] < 0.5 { 0.0 } else { 0.75 }))
    }
}
#[test]
fn cutout_holes_preserve_background_depth_and_covered_leaves_are_opaque() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    raster
        .quad(
            quad(1.0),
            owner(0),
            AlphaMode::Opaque,
            &FlatShader(rgba([0.0, 0.0, 1.0], 1.0)),
            cancel,
        )
        .unwrap();
    raster
        .quad(
            quad(2.0),
            owner(1),
            AlphaMode::Cutout { threshold: 128 },
            &FoliageShader,
            cancel,
        )
        .unwrap();
    assert_eq!(
        raster.pixel(0, 1).unwrap().color,
        rgba([0.0, 0.0, 1.0], 1.0)
    );
    assert_eq!(
        raster.pixel(3, 1).unwrap().color,
        rgba([0.0, 1.0, 0.0], 1.0)
    );
    assert_eq!(raster.pixel(0, 1).unwrap().front_owner, Some(owner(0)));
}

#[test]
fn pinned_native_cutout_thresholds_use_exact_shader_comparisons() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    for (mode, below, admitted) in [
        (AlphaMode::NativeCutout, 0.099_f32, 0.1_f32),
        (AlphaMode::NativeCutoutMipped, 0.499_f32, 0.5_f32),
    ] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        raster
            .quad(
                quad(1.0),
                owner(0),
                AlphaMode::Opaque,
                &FlatShader(rgba([0.0, 0.0, 1.0], 1.0)),
                cancel,
            )
            .unwrap();
        raster
            .quad(
                quad(2.0),
                owner(1),
                mode,
                &FlatShader(rgba([1.0, 0.0, 0.0], below)),
                cancel,
            )
            .unwrap();
        assert_eq!(raster.pixel(1, 1).unwrap().front_owner, Some(owner(0)));
        raster.clear();
        raster
            .quad(
                quad(2.0),
                owner(1),
                mode,
                &FlatShader(rgba([1.0, 0.0, 0.0], admitted)),
                cancel,
            )
            .unwrap();
        let visible = raster.pixel(1, 1).unwrap();
        assert_eq!(visible.front_owner, Some(owner(1)));
        assert_eq!(visible.color.alpha(), 1.0);
    }
}

#[test]
fn native_solid_uses_real_bank_rgb_and_depth_even_with_zero_source_alpha() {
    for alpha in [0_u8, 127] {
        native_bank_alpha_case(AlphaMode::NativeSolid, alpha, true);
    }
}

#[test]
fn native_cutout_layers_use_real_bank_alpha_on_both_sides_of_their_thresholds() {
    for (mode, alpha, visible) in [
        (AlphaMode::NativeCutout, 25, false),
        (AlphaMode::NativeCutout, 26, true),
        (AlphaMode::NativeCutoutMipped, 127, false),
        (AlphaMode::NativeCutoutMipped, 128, true),
    ] {
        native_bank_alpha_case(mode, alpha, visible);
    }
}

// Synthetic constant texel, real bank/compiler/binding/mesh/texture shader.
// FlatShader equality fixtures above cover thresholds between byte values.
fn native_bank_alpha_case(mode: AlphaMode, alpha: u8, visible: bool) {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    let texture_id = ResourceId::parse("test:block/native_solid").unwrap();
    builder
        .insert(
            texture_id.clone(),
            fixture_texture(
                [1, 1],
                &[255, 0, 0, alpha],
                None,
                &MissingAnimation::StaticImage,
                Encoding::SrgbColor,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    let bank = builder
        .finish(
            vec![TextureRequirement::selected_color(texture_id.clone())],
            cancel,
        )
        .unwrap();
    let mut definitions = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    definitions.install_geometry_templates(cancel).unwrap();
    definitions
        .bind_single(
            ResourceId::parse("test:native_solid").unwrap(),
            ResourceId::parse("block/cube_all").unwrap(),
            "all",
            texture_id.clone(),
            Label::new("Synthetic native solid alpha").unwrap(),
            cancel,
        )
        .unwrap();
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let normalized = compiler
        .compile_state(
            &BlockState::new(ResourceId::parse("test:native_solid").unwrap(), []).unwrap(),
            [0; 3],
            7,
            cancel,
        )
        .unwrap();
    let model = Arc::new(
        BoundModel::bind(
            &normalized,
            &bank,
            &MaterialTable {
                medium: None,
                rules: BTreeMap::from([(
                    texture_id,
                    TextureRenderRule {
                        alpha: mode,
                        layer: 0,
                        normal_map: None,
                        specular_map: None,
                    },
                )]),
                tints: BTreeMap::new(),
            },
            &budget,
            cancel,
        )
        .unwrap(),
    );
    let mesh = PreparedMesh::build(
        &BTreeMap::from([([0, 0, 0], model)]),
        MeshRegion {
            minimum: [-1, -1],
            maximum: [2, 2],
        },
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let mut raster = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let background_owner = owner(99);
    let background = quad(-10.0).map(|mut vertex| {
        vertex.x *= 16.0;
        vertex.y *= 16.0;
        vertex
    });
    raster
        .quad(
            background,
            background_owner,
            AlphaMode::Opaque,
            &FlatShader(rgba([0.0, 0.0, 1.0], 1.0)),
            cancel,
        )
        .unwrap();
    draw_mesh_inner(
        &mesh,
        &bank,
        [0.5, 0.5, 0.5],
        16.0,
        Duration::ZERO,
        DirectionalLight::default(),
        &mut raster,
        cancel,
    )
    .unwrap();
    let mut covered = 0;
    for y in 0..64 {
        for x in 0..64 {
            let pixel = raster.pixel(x, y).unwrap();
            let front_owner = pixel.front_owner.unwrap();
            if front_owner.position == [0; 3] {
                covered += 1;
                assert_eq!(front_owner.part, 0);
                assert_eq!(front_owner.layer, 0);
                assert_eq!(pixel.color.alpha(), 1.0);
                assert!(pixel.color.straight()[0] > 0.0);
                assert_eq!(pixel.color.straight()[1], 0.0);
                assert_eq!(pixel.color.straight()[2], 0.0);
            } else {
                assert_eq!(front_owner, background_owner);
                assert_eq!(pixel.color.straight(), [0.0, 0.0, 1.0]);
            }
        }
    }
    assert_eq!(
        covered > 0,
        visible,
        "native mode {mode:?}, texture alpha {alpha} has wrong admission"
    );
    // A closer opaque surface must still win when the native mesh is
    // submitted afterwards, including a Solid texel with zero PNG alpha.
    let foreground_owner = owner(100);
    let foreground = background.map(|mut vertex| {
        vertex.depth = 10.0;
        vertex
    });
    raster
        .quad(
            foreground,
            foreground_owner,
            AlphaMode::Opaque,
            &FlatShader(rgba([0.0, 1.0, 0.0], 1.0)),
            cancel,
        )
        .unwrap();
    draw_mesh_inner(
        &mesh,
        &bank,
        [0.5, 0.5, 0.5],
        16.0,
        Duration::ZERO,
        DirectionalLight::default(),
        &mut raster,
        cancel,
    )
    .unwrap();
    for y in 0..64 {
        for x in 0..64 {
            let pixel = raster.pixel(x, y).unwrap();
            assert_eq!(pixel.front_owner, Some(foreground_owner));
            assert_eq!(pixel.color.straight(), [0.0, 1.0, 0.0]);
        }
    }
}
#[test]
fn coplanar_overlay_layer_and_intersecting_depths_are_resolved_per_pixel() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let base = rgba([0.6, 0.3, 0.1], 1.0);
    let tint = rgba([0.1, 0.8, 0.1], 0.5);
    for order in [[0, 1], [1, 0]] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        for index in order {
            raster
                .quad(
                    quad(1.0),
                    owner(index),
                    if index == 0 {
                        AlphaMode::Opaque
                    } else {
                        AlphaMode::Blend
                    },
                    &FlatShader(if index == 0 { base } else { tint }),
                    cancel,
                )
                .unwrap();
        }
        assert_eq!(raster.pixel(1, 1).unwrap().color, tint.over(base));
    }
    let mut snapshots = Vec::new();
    for reverse in [false, true] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        let mut tilted = quad(1.0);
        tilted[1].depth = 3.0;
        tilted[2].depth = 3.0;
        let order = if reverse { [1, 0] } else { [0, 1] };
        for index in order {
            raster
                .quad(
                    if index == 0 { tilted } else { quad(2.0) },
                    owner(index),
                    AlphaMode::Blend,
                    &FlatShader(if index == 0 {
                        rgba([1.0, 0.0, 0.0], 0.5)
                    } else {
                        rgba([0.0, 0.0, 1.0], 0.5)
                    }),
                    cancel,
                )
                .unwrap();
        }
        assert_eq!(raster.pixel(0, 1).unwrap().front_owner, Some(owner(1)));
        assert_eq!(raster.pixel(3, 1).unwrap().front_owner, Some(owner(0)));
        snapshots.push(
            (0..16)
                .map(|index| raster.pixel(index % 4, index / 4).unwrap().color)
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(snapshots[0], snapshots[1]);
}
#[test]
fn layer_overflow_invalidates_the_candidate_instead_of_publishing_a_truncated_water_stack() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new(
        [4, 4],
        RasterLimits {
            layers: 2,
            ..Default::default()
        },
        &budget,
        cancel,
    )
    .unwrap();
    for index in 0..2 {
        raster
            .quad(
                quad(index as f64),
                owner(index),
                AlphaMode::Blend,
                &FlatShader(rgba([0.2, 0.3, 0.4], 0.3)),
                cancel,
            )
            .unwrap();
    }
    assert!(raster
        .quad(
            quad(3.0),
            owner(3),
            AlphaMode::Blend,
            &FlatShader(rgba([0.2, 0.3, 0.4], 0.3)),
            cancel
        )
        .is_err());
    assert!(!raster.is_valid());
    assert!(raster.pixel(0, 0).is_err());
    raster.clear();
    assert!(raster.is_valid());
    assert_eq!(raster.pixel(0, 0).unwrap().color, LinearRgba::CLEAR);
}
#[test]
fn cancellation_clipping_degenerate_vertices_and_sample_budgets_are_finite() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    let paint = FlatShader(rgba([1.0; 3], 0.5));
    let mut clipped = quad(1.0);
    for vertex in &mut clipped {
        vertex.x -= 2.0;
        vertex.y -= 2.0;
    }
    raster
        .quad(clipped, owner(1), AlphaMode::Blend, &paint, cancel)
        .unwrap();
    assert_eq!(
        (0..16)
            .filter(|index| raster.pixel(index % 4, index / 4).unwrap().color.alpha() > 0.0)
            .count(),
        4
    );
    raster
        .triangle(
            [quad(1.0)[0]; 3],
            owner(1),
            AlphaMode::Blend,
            &paint,
            cancel,
        )
        .unwrap();
    let mut bounded = RasterFrame::new(
        [4, 4],
        RasterLimits {
            sample_tests: 1,
            ..Default::default()
        },
        &budget,
        cancel,
    )
    .unwrap();
    assert!(bounded
        .quad(quad(1.0), owner(1), AlphaMode::Blend, &paint, cancel)
        .is_err());
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(raster
        .quad(quad(1.0), owner(1), AlphaMode::Blend, &paint, cancel)
        .is_err());
    assert!(!raster.is_valid());
}
fn bound_pair(
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> (TextureBank, Arc<BoundModel>, Arc<BoundModel>) {
    let limits = Limits::default();
    let mut builder =
        TextureBankBuilder::new(fixture_review(), vec![], limits, budget.clone(), cancel).unwrap();
    let solid_id = ResourceId::parse("test:block/ground").unwrap();
    let water_id = ResourceId::parse("test:block/water").unwrap();
    let solid = fixture_texture(
        [1, 1],
        &[140, 100, 50, 255],
        None,
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::DiagnosticFixture),
        budget,
    );
    let water = fixture_texture(
        [1, 2],
        &[30, 80, 200, 153, 90, 180, 110, 153],
        Some(r#"{"animation":{"frametime":1}}"#),
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::DiagnosticFixture),
        budget,
    );
    builder.insert(solid_id.clone(), solid, cancel).unwrap();
    builder.insert(water_id.clone(), water, cancel).unwrap();
    let bank = builder
        .finish(
            vec![
                TextureRequirement::selected_color(solid_id.clone()),
                TextureRequirement::selected_color(water_id.clone()),
            ],
            cancel,
        )
        .unwrap();
    let mut defs = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        limits,
        budget.clone(),
    )
    .unwrap();
    defs.install_geometry_templates(cancel).unwrap();
    let reason = Label::new("Original synthetic geometry/pixel fixture").unwrap();
    defs.bind_single(
        ResourceId::parse("test:ground").unwrap(),
        ResourceId::parse("block/cube_all").unwrap(),
        "all",
        solid_id.clone(),
        reason.clone(),
        cancel,
    )
    .unwrap();
    defs.bind_single(
        ResourceId::parse("test:water").unwrap(),
        ResourceId::parse("block/cube_all").unwrap(),
        "all",
        water_id.clone(),
        reason,
        cancel,
    )
    .unwrap();
    let mut compiler = ModelCompiler::new(&defs, limits, budget.clone()).unwrap();
    let ground = compiler
        .compile_state(
            &BlockState::new(ResourceId::parse("test:ground").unwrap(), []).unwrap(),
            [0; 3],
            7,
            cancel,
        )
        .unwrap();
    let water = compiler
        .compile_state(
            &BlockState::new(ResourceId::parse("test:water").unwrap(), []).unwrap(),
            [0; 3],
            7,
            cancel,
        )
        .unwrap();
    let rules = BTreeMap::from([
        (
            solid_id,
            TextureRenderRule {
                alpha: AlphaMode::Opaque,
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        ),
        (
            water_id,
            TextureRenderRule {
                alpha: AlphaMode::Blend,
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        ),
    ]);
    let ground = Arc::new(
        BoundModel::bind(
            &ground,
            &bank,
            &MaterialTable {
                medium: None,
                rules: rules.clone(),
                tints: BTreeMap::new(),
            },
            budget,
            cancel,
        )
        .unwrap(),
    );
    let water = Arc::new(
        BoundModel::bind(
            &water,
            &bank,
            &MaterialTable {
                medium: Some(ResourceId::parse("test:connected_water").unwrap()),
                rules,
                tints: BTreeMap::new(),
            },
            budget,
            cancel,
        )
        .unwrap(),
    );
    (bank, ground, water)
}
#[test]
fn water_occupancy_keeps_the_bed_and_suppresses_internal_same_medium_cube_faces() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, water) = bound_pair(&budget, cancel);
    let instances = BTreeMap::from([([0, 0, 0], ground), ([0, 0, 1], Arc::clone(&water))]);
    let region = MeshRegion {
        minimum: [-1, -1],
        maximum: [3, 3],
    };
    let mesh =
        PreparedMesh::build(&instances, region, bank.identity(), 100, &budget, cancel).unwrap();
    assert_eq!(mesh.faces.len(), 11);
    assert!(mesh.faces.iter().any(|face| face.position == [0, 0, 0]
        && face.model.quads[usize::from(face.quad_index)].normal == [0.0, 0.0, 1.0]));
    let water_instances = BTreeMap::from([([0, 0, 0], Arc::clone(&water)), ([1, 0, 0], water)]);
    let water_mesh = PreparedMesh::build(
        &water_instances,
        region,
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    assert_eq!(water_mesh.faces.len(), 10);
}
#[test]
fn actual_texture_sampler_time_changes_water_without_rebuilding_state_geometry_or_camera() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, water) = bound_pair(&budget, cancel);
    let instances = BTreeMap::from([([0, 0, 0], ground), ([0, 0, 1], water)]);
    let mesh = PreparedMesh::build(
        &instances,
        MeshRegion {
            minimum: [-1, -1],
            maximum: [2, 2],
        },
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let mut output = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let mut snapshots = Vec::new();
    let mut bed_through_water = false;
    for time in [Duration::ZERO, Duration::from_millis(50)] {
        draw_mesh(
            &mesh,
            &bank,
            [0.5, 0.5, 1.0],
            16.0,
            time,
            DirectionalLight::default(),
            &mut output,
            cancel,
        )
        .unwrap();
        let pixels: Vec<_> = (0..4096)
            .map(|i| output.pixel(i % 64, i / 64).unwrap())
            .collect();
        bed_through_water |= pixels.iter().any(|p| {
            p.contributors
                .iter()
                .flatten()
                .any(|o| o.position == [0, 0, 0])
                && p.contributors
                    .iter()
                    .flatten()
                    .any(|o| o.position == [0, 0, 1])
        });
        snapshots.push(pixels.into_iter().map(|p| p.color).collect::<Vec<_>>());
    }
    assert!(bed_through_water);
    assert_ne!(snapshots[0], snapshots[1]);
    assert!(!bank.coverage().required_textures_satisfied);
}
#[test]
fn prepared_window_changes_keep_global_owners_and_exact_pixels_at_large_signed_origins() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, _water) = bound_pair(&budget, cancel);
    for offset in [-1_000_000_000, 0, 1_000_000_000] {
        let position = [offset + 4, offset + 7, -64];
        let instances = BTreeMap::from([(position, Arc::clone(&ground))]);
        let mut snapshots = Vec::new();
        for margin in [1, 70] {
            let mesh = PreparedMesh::build(
                &instances,
                MeshRegion {
                    minimum: [position[0] - margin, position[1] - margin],
                    maximum: [position[0] + margin + 1, position[1] + margin + 1],
                },
                bank.identity(),
                100,
                &budget,
                cancel,
            )
            .unwrap();
            let mut output =
                RasterFrame::new([32, 32], RasterLimits::default(), &budget, cancel).unwrap();
            draw_mesh(
                &mesh,
                &bank,
                [
                    f64::from(position[0]) + 0.37,
                    f64::from(position[1]) + 0.61,
                    -63.5,
                ],
                8.0,
                Duration::ZERO,
                DirectionalLight::default(),
                &mut output,
                cancel,
            )
            .unwrap();
            snapshots.push(
                (0..1024)
                    .map(|i| {
                        let pixel = output.pixel(i % 32, i / 32).unwrap();
                        (pixel.color, pixel.front_owner)
                    })
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(snapshots[0], snapshots[1]);
        assert!(snapshots[0]
            .iter()
            .any(|(_, owner)| owner.is_some_and(|owner| owner.position == position)));
    }
}

#[test]
fn partial_fluid_mesh_animates_at_fixed_camera_and_keeps_bed_as_a_pixel_contributor() {
    use super::{
        surface_fluid::{FluidCell, FluidMesh},
        surface_raster::draw_fluid_mesh,
    };
    use std::collections::BTreeSet;
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, _) = bound_pair(&budget, cancel);
    let region = MeshRegion {
        minimum: [-1, -1],
        maximum: [2, 2],
    };
    let bed = PreparedMesh::build(
        &BTreeMap::from([([0, 0, 0], ground)]),
        region,
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let fluid = FluidMesh::build(
        &BTreeMap::from([([0, 0, 1], FluidCell::new(4, [0.8, 0.9, 1.0]).unwrap())]),
        &BTreeSet::from([[0, 0, 0]]),
        region,
        &bank,
        bank.resolve(&ResourceId::parse("test:block/water").unwrap())
            .unwrap(),
        16,
        &budget,
        cancel,
    )
    .unwrap();
    assert_eq!(fluid.faces.len(), 5);
    assert!(fluid.faces[0]
        .quad
        .points
        .iter()
        .all(|point| point[2] == 0.5));
    let mut frame = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let mut snapshots = Vec::new();
    for time in [Duration::ZERO, Duration::from_millis(50)] {
        draw_mesh(
            &bed,
            &bank,
            [0.5, 0.5, 1.0],
            16.0,
            time,
            DirectionalLight::default(),
            &mut frame,
            cancel,
        )
        .unwrap();
        draw_fluid_mesh(
            &fluid,
            &bank,
            [0.5, 0.5, 1.0],
            16.0,
            time,
            DirectionalLight::default(),
            &mut frame,
            cancel,
        )
        .unwrap();
        snapshots.push(
            (0..4096)
                .map(|i| frame.pixel(i % 64, i / 64).unwrap())
                .collect::<Vec<_>>(),
        );
    }
    assert!(snapshots[0].iter().any(|pixel| {
        let owners: Vec<_> = pixel
            .contributors
            .iter()
            .flatten()
            .map(|owner| owner.position)
            .collect();
        owners.contains(&[0, 0, 0]) && owners.contains(&[0, 0, 1])
    }));
    assert!(snapshots[0]
        .iter()
        .zip(&snapshots[1])
        .any(|(a, b)| a.color != b.color));
}

#[test]
fn native_fluid_uv_does_not_drift_but_selected_texture_frames_still_animate() {
    let first = native_fluid_raster(AlphaMode::NativeBlend, 128, Duration::ZERO);
    let same_frame = native_fluid_raster(AlphaMode::NativeBlend, 128, Duration::from_secs(20));
    let next_frame = native_fluid_raster(AlphaMode::NativeBlend, 128, Duration::from_millis(50));
    assert!(first.iter().any(|pixel| pixel.alpha() > 0.0));
    assert_eq!(
        first, same_frame,
        "native UV must not receive generated ripple/time offsets"
    );
    assert_ne!(
        first, next_frame,
        "selected resource animation must keep advancing"
    );
}

#[test]
fn native_lava_uses_opaque_depth_slot_with_zero_alpha_texels() {
    let raster = native_fluid_raster(AlphaMode::NativeSolid, 0, Duration::ZERO);
    assert!(raster
        .iter()
        .any(|pixel| pixel.straight() == [1.0, 0.0, 0.0]));
    assert!(raster
        .iter()
        .filter(|pixel| pixel.alpha() > 0.0)
        .all(|pixel| pixel.alpha() == 1.0));
}

// Synthetic two-frame resource and supplied material modes exercise the real
// bank, fluid mesh, texture shader and dot raster; not saved-fluid assembly.
fn native_fluid_raster(mode: AlphaMode, alpha: u8, time: Duration) -> Vec<LinearRgba> {
    use super::surface_fluid::{FluidCell, FluidMesh};
    use std::collections::BTreeSet;
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let texture_id = ResourceId::parse("test:block/native_fluid").unwrap();
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    builder
        .insert(
            texture_id.clone(),
            fixture_texture(
                [2, 2],
                &[
                    255, 0, 0, alpha, 0, 0, 255, alpha, 0, 255, 0, alpha, 255, 255, 0, alpha,
                ],
                Some(r#"{"animation":{"width":2,"height":1,"frametime":1,"frames":[0,1]}}"#),
                &MissingAnimation::StaticImage,
                Encoding::SrgbColor,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    let bank = builder
        .finish(
            vec![TextureRequirement::selected_color(texture_id.clone())],
            cancel,
        )
        .unwrap();
    let diffuse = bank.resolve(&texture_id).unwrap();
    let mut mesh = FluidMesh::build(
        &BTreeMap::from([([0, 0, 0], FluidCell::new(0, [1.0; 3]).unwrap())]),
        &BTreeSet::new(),
        MeshRegion {
            minimum: [0, 0],
            maximum: [1, 1],
        },
        &bank,
        diffuse,
        16,
        &budget,
        cancel,
    )
    .unwrap();
    for face in &mut mesh.faces {
        face.quad.uv = [[0.25, 0.25]; 4];
        face.quad.material.alpha = mode;
        face.quad.shade = false;
    }
    let limits = if mode == AlphaMode::NativeSolid {
        // Nine distinct opaque owners cover one sample. Native lava belongs
        // in the depth slot and must not consume bounded translucent layers.
        let top = mesh.faces[0].clone();
        mesh.faces.clear();
        for face in 0..9 {
            let mut copy = top.clone();
            copy.quad.face = face;
            copy.owner.face = face;
            mesh.faces.push(copy);
        }
        RasterLimits {
            layers: 1,
            ..RasterLimits::default()
        }
    } else {
        RasterLimits::default()
    };
    let mut raster = RasterFrame::new([32, 32], limits, &budget, cancel).unwrap();
    super::surface_raster::draw_fluid_mesh(
        &mesh,
        &bank,
        [0.5, 0.5, 0.5],
        8.0,
        time,
        DirectionalLight::default(),
        &mut raster,
        cancel,
    )
    .unwrap();
    (0..32)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .map(|(x, y)| raster.pixel(x, y).unwrap().color)
        .collect()
}

use super::surface_raster::draw_mesh_layer;

#[test]
fn adjacent_tile_cores_match_one_mesh_at_the_shared_boundary() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, _water) = bound_pair(&budget, cancel);
    let instances = BTreeMap::from([
        ([0, 0, 0], Arc::clone(&ground)),
        ([1, 0, 0], Arc::clone(&ground)),
    ]);
    let region = |minimum_x, maximum_x| MeshRegion {
        minimum: [minimum_x, 0],
        maximum: [maximum_x, 1],
    };
    let whole = PreparedMesh::build(
        &instances,
        region(0, 2),
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let left = PreparedMesh::build(
        &instances,
        region(0, 1),
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let right = PreparedMesh::build(
        &instances,
        region(1, 2),
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let mut whole_owners: Vec<_> = whole.faces.iter().map(|face| face.owner).collect();
    let mut tile_owners: Vec<_> = left
        .faces
        .iter()
        .chain(&right.faces)
        .map(|face| face.owner)
        .collect();
    whole_owners.sort();
    tile_owners.sort();
    assert_eq!(whole_owners, tile_owners);

    let render = |meshes: &[&PreparedMesh]| {
        let mut raster =
            RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
        for mesh in meshes {
            draw_mesh_layer(
                mesh,
                &bank,
                [1.0, 0.5, 0.5],
                8.0,
                Duration::ZERO,
                DirectionalLight::default(),
                &mut raster,
                cancel,
            )
            .unwrap();
        }
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .map(|(x, y)| {
                let pixel = raster.pixel(x, y).unwrap();
                (pixel.color, pixel.front_owner)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(render(&[&whole]), render(&[&left, &right]));
}

#[test]
fn mesh_layers_keep_both_tile_owners_in_either_submission_order() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    let texture_id = ResourceId::parse("test:block/native_solid").unwrap();
    builder
        .insert(
            texture_id.clone(),
            fixture_texture(
                [1, 1],
                &[255, 0, 0, 255],
                None,
                &MissingAnimation::StaticImage,
                Encoding::SrgbColor,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    let bank = builder
        .finish(
            vec![TextureRequirement::selected_color(texture_id.clone())],
            cancel,
        )
        .unwrap();
    let mut definitions = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    definitions.install_geometry_templates(cancel).unwrap();
    definitions
        .bind_single(
            ResourceId::parse("test:native_solid").unwrap(),
            ResourceId::parse("block/cube_all").unwrap(),
            "all",
            texture_id.clone(),
            Label::new("Synthetic native solid alpha").unwrap(),
            cancel,
        )
        .unwrap();
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let normalized = compiler
        .compile_state(
            &BlockState::new(ResourceId::parse("test:native_solid").unwrap(), []).unwrap(),
            [0; 3],
            7,
            cancel,
        )
        .unwrap();
    let model = Arc::new(
        BoundModel::bind(
            &normalized,
            &bank,
            &MaterialTable {
                medium: None,
                rules: BTreeMap::from([(
                    texture_id,
                    TextureRenderRule {
                        alpha: AlphaMode::NativeSolid,
                        layer: 0,
                        normal_map: None,
                        specular_map: None,
                    },
                )]),
                tints: BTreeMap::new(),
            },
            &budget,
            cancel,
        )
        .unwrap(),
    );

    let meshes = [[0, 0, 0], [3, 0, 0]].map(|position| {
        PreparedMesh::build(
            &BTreeMap::from([(position, Arc::clone(&model))]),
            MeshRegion {
                minimum: [-1, -1],
                maximum: [5, 2],
            },
            bank.identity(),
            100,
            &budget,
            cancel,
        )
        .unwrap()
    });
    let mut snapshots = Vec::new();
    for order in [[0, 1], [1, 0]] {
        let mut raster =
            RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
        for index in order {
            draw_mesh_layer(
                &meshes[index],
                &bank,
                [1.5, 0.5, 0.5],
                8.0,
                Duration::ZERO,
                DirectionalLight::default(),
                &mut raster,
                cancel,
            )
            .unwrap();
        }
        let pixels: Vec<_> = (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .map(|(x, y)| {
                let pixel = raster.pixel(x, y).unwrap();
                (pixel.color, pixel.front_owner)
            })
            .collect();
        for position in [[0, 0, 0], [3, 0, 0]] {
            assert!(
                pixels
                    .iter()
                    .any(|(_, face)| face.is_some_and(|face| face.position == position)),
                "appending another tile erased the pixels of {position:?}"
            );
        }
        snapshots.push(pixels);
        // Cancellation invalidates the whole composed frame; a later valid
        // tile cannot silently resurrect an incomplete owner/depth table.
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(draw_mesh_layer(
            &meshes[0],
            &bank,
            [1.5, 0.5, 0.5],
            8.0,
            Duration::ZERO,
            DirectionalLight::default(),
            &mut raster,
            Cancel::new(&stop)
        )
        .is_err());
        assert!(!raster.is_valid());
        stop.store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(draw_mesh_layer(
            &meshes[1],
            &bank,
            [1.5, 0.5, 0.5],
            8.0,
            Duration::ZERO,
            DirectionalLight::default(),
            &mut raster,
            Cancel::new(&stop)
        )
        .is_err());
    }
    assert_eq!(
        snapshots[0], snapshots[1],
        "tile order changed composed pixels or owners"
    );
}

// Captured native water ranks/owners at pixel [10,94] in diagnostic201.
// The quad is a synthetic coverage fixture; fragment ranks and ownership are actual.
#[test]
fn captured_occluded_native_water_does_not_exhaust_translucent_layers() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let bed_owner = FaceOwner {
        position: [181, -39, 72],
        part: 0,
        face: 5,
        layer: 1,
    };
    let bed = rgba([0.0430224, 0.067860074, 0.0430224], 1.0);
    let fragments = [
        (-46143624, [166, -53, 58], 5),
        (-45353861, [167, -53, 59], 1),
        (-43143606, [167, -52, 59], 5),
        (-42353861, [168, -52, 60], 1),
        (-40465933, [168, -52, 60], 3),
        (-40137471, [169, -51, 60], 4),
        (-34465682, [170, -50, 62], 3),
        (-34111465, [171, -49, 62], 0),
        (-34137328, [171, -49, 62], 4),
    ];
    // Both normal solids-first and eight retained layers before a later bed.
    for bed_after_eight in [false, true] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        if !bed_after_eight {
            raster
                .quad(
                    quad(-4.140869),
                    bed_owner,
                    AlphaMode::Opaque,
                    &FlatShader(bed),
                    cancel,
                )
                .unwrap();
        }
        for (index, (depth, position, face)) in fragments.into_iter().enumerate() {
            if bed_after_eight && index == 8 {
                raster
                    .quad(
                        quad(-4.140869),
                        bed_owner,
                        AlphaMode::Opaque,
                        &FlatShader(bed),
                        cancel,
                    )
                    .unwrap();
            }
            raster
                .quad(
                    quad(f64::from(depth) / 1_000_000.0),
                    FaceOwner {
                        position,
                        part: 0,
                        face,
                        layer: 3,
                    },
                    AlphaMode::NativeBlend,
                    &FlatShader(rgba([0.03, 0.05, 0.1], 180.0 / 255.0)),
                    cancel,
                )
                .unwrap();
        }
        let result = raster.pixel(2, 2).unwrap();
        assert_eq!(result.color, bed);
        assert_eq!(result.front_owner, Some(bed_owner));
        assert_eq!(
            result
                .contributors
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            vec![bed_owner]
        );
    }
}
#[test]
fn native_visible_overflow_and_generic_hidden_overflow_still_invalidate() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    for (mode, bed_depth) in [(AlphaMode::NativeBlend, -1.0), (AlphaMode::Blend, 20.0)] {
        let mut raster =
            RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
        raster
            .quad(
                quad(bed_depth),
                owner(0),
                AlphaMode::Opaque,
                &FlatShader(rgba([0.2; 3], 1.0)),
                cancel,
            )
            .unwrap();
        for layer in 1..=12 {
            raster
                .quad(
                    quad(f64::from(layer)),
                    owner(layer),
                    mode,
                    &FlatShader(rgba([0.1; 3], 0.5)),
                    cancel,
                )
                .unwrap();
        }
        assert!(matches!(
            raster.quad(
                quad(13.0),
                owner(13),
                mode,
                &FlatShader(rgba([0.1; 3], 0.5)),
                cancel
            ),
            Err(super::assets::error::AssetError::Limit {
                resource: "translucent layers per pixel",
                requested: 13,
                limit: 12
            })
        ));
        assert!(raster.pixel(2, 2).is_err());
    }
}
#[test]
fn saved_world_pixel_composites_nine_translucent_layers_over_opaque_terrain() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    raster
        .quad(
            quad(-1.0),
            owner(0),
            AlphaMode::Opaque,
            &FlatShader(rgba([0.2; 3], 1.0)),
            cancel,
        )
        .unwrap();
    for layer in 1..=9 {
        raster
            .quad(
                quad(f64::from(layer)),
                owner(layer),
                AlphaMode::Blend,
                &FlatShader(rgba([0.1; 3], 0.5)),
                cancel,
            )
            .unwrap();
    }

    let pixel = raster.pixel(2, 2).unwrap();
    assert_eq!(pixel.contributors.iter().flatten().count(), 10);
    assert_eq!(pixel.front_owner, Some(owner(9)));
}
#[test]
fn native_occluded_layers_preserve_visible_colours_and_owners() {
    let budget = ByteBudget::new(1 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut raster = RasterFrame::new([4, 4], RasterLimits::default(), &budget, cancel).unwrap();
    let bed = rgba([0.2, 0.3, 0.1], 1.0);
    let lower = rgba([0.1, 0.4, 0.8], 0.5);
    let upper = rgba([0.7, 0.2, 0.1], 0.3);
    raster
        .quad(
            quad(20.0),
            owner(0),
            AlphaMode::Opaque,
            &FlatShader(bed),
            cancel,
        )
        .unwrap();
    for layer in 1..=16 {
        raster
            .quad(
                quad(f64::from(layer)),
                owner(layer),
                AlphaMode::NativeBlend,
                &FlatShader(lower),
                cancel,
            )
            .unwrap();
    }
    // Reverse visible submission proves depth compositing is retained.
    raster
        .quad(
            quad(22.0),
            owner(22),
            AlphaMode::NativeBlend,
            &FlatShader(upper),
            cancel,
        )
        .unwrap();
    raster
        .quad(
            quad(21.0),
            owner(21),
            AlphaMode::NativeBlend,
            &FlatShader(lower),
            cancel,
        )
        .unwrap();
    let result = raster.pixel(2, 2).unwrap();
    assert_eq!(result.color, upper.over(lower.over(bed)));
    assert_eq!(result.front_owner, Some(owner(22)));
    assert_eq!(
        result
            .contributors
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        vec![owner(0), owner(21), owner(22)]
    );
}

// Synthetic texels with the measured shapes/RGBA of the two uniform selected
// images. The original pack PNGs are not embedded in the repository fixture.
fn uniform_water_bank(
    budget: &ByteBudget,
    cancel: Cancel<'_>,
    dimensions: [u32; 2],
    rgba: [u8; 4],
    metadata: Option<&str>,
) -> (TextureBank, ResourceId) {
    let id = ResourceId::parse("test:block/uniform_water").unwrap();
    let pixel_count = usize::try_from(u64::from(dimensions[0]) * u64::from(dimensions[1])).unwrap();
    let texture = fixture_texture(
        dimensions,
        &rgba.repeat(pixel_count),
        metadata,
        &MissingAnimation::StaticImage,
        Encoding::SrgbColor,
        fixture_origin(OriginKind::DiagnosticFixture),
        budget,
    );
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    builder.insert(id.clone(), texture, cancel).unwrap();
    let bank = builder
        .finish(vec![TextureRequirement::selected_color(id.clone())], cancel)
        .unwrap();
    (bank, id)
}

fn fluid_pixels(
    meshes: &[&FluidMesh],
    bank: &TextureBank,
    budget: &ByteBudget,
    camera: [f64; 3],
    size: [usize; 2],
    time: Duration,
    cancel: Cancel<'_>,
) -> Vec<PixelResult> {
    let mut frame = RasterFrame::new(size, RasterLimits::default(), budget, cancel).unwrap();
    for mesh in meshes {
        draw_fluid_mesh(
            mesh,
            bank,
            camera,
            16.0,
            time,
            DirectionalLight::default(),
            &mut frame,
            cancel,
        )
        .unwrap();
    }
    (0..size[1])
        .flat_map(|y| (0..size[0]).map(move |x| (x, y)))
        .map(|(x, y)| frame.pixel(x, y).unwrap())
        .collect()
}

fn pixel_signature(pixel: &PixelResult) -> (LinearRgba, Option<FaceOwner>, [Option<FaceOwner>; 9]) {
    (pixel.color, pixel.front_owner, pixel.contributors)
}

#[test]
fn measured_uniform_water_shapes_animate_at_fixed_camera_without_replacing_source_or_alpha() {
    for (dimensions, rgba, metadata) in [
        ([16, 16], [101, 255, 255, 179], None),
        ([16, 512], [94, 113, 235, 166], None),
        // An authored timeline containing identical source cells also needs
        // generated motion; a changing rectangle is not changing artwork.
        (
            [16, 512],
            [94, 113, 235, 166],
            Some(r#"{"animation":{"frametime":1}}"#),
        ),
    ] {
        let budget = ByteBudget::new(256 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let (bank, id) = uniform_water_bank(&budget, cancel, dimensions, rgba, metadata);
        let texture = bank.texture(bank.resolve(&id).unwrap()).unwrap();
        assert_eq!(texture.image().dimensions(), dimensions);
        assert!(texture
            .image()
            .bytes()
            .chunks_exact(4)
            .all(|texel| texel == rgba.as_slice()));
        let source_before = (
            texture.fingerprint(),
            texture.image().source_sha256(),
            texture.image().rgba_sha256(),
            texture.image().origin().clone(),
            bank.identity(),
        );
        let source_sample = texture.sample_color([0.37, 0.61], Duration::ZERO).unwrap();
        assert_eq!(
            source_sample,
            texture
                .sample_color([0.37, 0.61], Duration::from_millis(400))
                .unwrap(),
            "fixture source itself changed for {dimensions:?}"
        );
        assert_eq!(source_sample.alpha(), f32::from(rgba[3]) / 255.0);
        let position = [-3, -2, 1];
        let region = MeshRegion {
            minimum: [-3, -2],
            maximum: [-2, -1],
        };
        let cells = BTreeMap::from([(position, FluidCell::new(0, [0.65, 0.85, 0.9]).unwrap())]);
        let mut fluid = FluidMesh::build(
            &cells,
            &BTreeSet::new(),
            region,
            &bank,
            bank.resolve(&id).unwrap(),
            16,
            &budget,
            cancel,
        )
        .unwrap();
        // Side faces can overlap the top in the isometric projection. Keep the
        // original generated top face so alpha and RGB have one source layer.
        fluid.faces.retain(|face| face.quad.face == 0);
        assert_eq!(fluid.faces.len(), 1);
        let camera = [-2.5, -1.5, 1.0];
        let first = fluid_pixels(
            &[&fluid],
            &bank,
            &budget,
            camera,
            [96, 96],
            Duration::ZERO,
            cancel,
        );
        let later = fluid_pixels(
            &[&fluid],
            &bank,
            &budget,
            camera,
            [96, 96],
            Duration::from_millis(400),
            cancel,
        );
        let mut top_pixels = 0;
        let mut changed_bytes = 0;
        for (a, b) in first.iter().zip(&later) {
            if !a
                .front_owner
                .is_some_and(|owner| owner.position == position && owner.face == 0)
                || a.contributors.iter().flatten().count() != 1
            {
                continue;
            }
            top_pixels += 1;
            assert_eq!(a.front_owner, b.front_owner);
            assert_eq!(a.color.alpha(), source_sample.alpha());
            assert_eq!(b.color.alpha(), source_sample.alpha());
            if a.color.straight().map(linear_to_srgb_byte)
                != b.color.straight().map(linear_to_srgb_byte)
            {
                changed_bytes += 1;
            }
        }
        assert!(top_pixels >= 16, "no substantial generated top surface");
        assert!(
            changed_bytes >= 4,
            "uniform {dimensions:?} water stayed visibly static"
        );
        assert_eq!(
            source_before,
            (
                texture.fingerprint(),
                texture.image().source_sha256(),
                texture.image().rgba_sha256(),
                texture.image().origin().clone(),
                bank.identity(),
            )
        );

        let untinted_cells = BTreeMap::from([(position, FluidCell::new(0, [1.0; 3]).unwrap())]);
        let mut untinted = FluidMesh::build(
            &untinted_cells,
            &BTreeSet::new(),
            region,
            &bank,
            bank.resolve(&id).unwrap(),
            16,
            &budget,
            cancel,
        )
        .unwrap();
        untinted.faces.retain(|face| face.quad.face == 0);
        let plain = fluid_pixels(
            &[&untinted],
            &bank,
            &budget,
            camera,
            [96, 96],
            Duration::ZERO,
            cancel,
        );
        let index = first
            .iter()
            .position(|pixel| {
                pixel
                    .front_owner
                    .is_some_and(|owner| owner.position == position && owner.face == 0)
                    && pixel.contributors.iter().flatten().count() == 1
            })
            .unwrap();
        let tinted_rgb = first[index].color.premultiplied();
        let plain_rgb = plain[index].color.premultiplied();
        for (channel, tint) in [0.65, 0.85, 0.9].into_iter().enumerate() {
            assert!((tinted_rgb[channel] - plain_rgb[channel] * tint).abs() < 1e-6);
        }
        assert_eq!(tinted_rgb[3], plain_rgb[3]);
    }
}

#[test]
fn generated_water_phase_matches_across_negative_cells_tiles_camera_and_window() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, id) = uniform_water_bank(&budget, cancel, [16, 16], [101, 255, 255, 179], None);
    let cells = BTreeMap::from([
        ([-3, -2, 1], FluidCell::new(0, [0.7, 0.9, 1.0]).unwrap()),
        ([-2, -2, 1], FluidCell::new(0, [0.7, 0.9, 1.0]).unwrap()),
    ]);
    let build = |minimum_x, maximum_x| {
        FluidMesh::build(
            &cells,
            &BTreeSet::new(),
            MeshRegion {
                minimum: [minimum_x, -2],
                maximum: [maximum_x, -1],
            },
            &bank,
            bank.resolve(&id).unwrap(),
            32,
            &budget,
            cancel,
        )
        .unwrap()
    };
    let whole = build(-3, -1);
    let left = build(-3, -2);
    let right = build(-2, -1);
    assert_eq!(whole.faces.len(), left.faces.len() + right.faces.len());
    let camera = [-2.0, -1.5, 1.0];
    for time in [Duration::ZERO, Duration::from_millis(400)] {
        let together = fluid_pixels(&[&whole], &bank, &budget, camera, [96, 96], time, cancel);
        let split = fluid_pixels(
            &[&left, &right],
            &bank,
            &budget,
            camera,
            [96, 96],
            time,
            cancel,
        );
        assert!(together
            .iter()
            .any(|pixel| pixel.front_owner.is_some_and(|owner| owner.face == 0)));
        for (a, b) in together.iter().zip(&split) {
            assert_eq!(pixel_signature(a), pixel_signature(b));
        }
        let small = fluid_pixels(&[&whole], &bank, &budget, camera, [64, 64], time, cancel);
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(
                    pixel_signature(&small[y * 64 + x]),
                    pixel_signature(&together[(y + 16) * 96 + x + 16]),
                );
            }
        }
        let moved = fluid_pixels(
            &[&whole],
            &bank,
            &budget,
            [camera[0] + 1.0, camera[1] + 1.0, camera[2]],
            [96, 96],
            time,
            cancel,
        );
        for y in 16..96 {
            for x in 0..96 {
                assert_eq!(
                    pixel_signature(&together[y * 96 + x]),
                    pixel_signature(&moved[(y - 16) * 96 + x]),
                );
            }
        }
    }
}

#[test]
fn generated_ripple_keeps_authored_diffuse_frames_and_data_maps_active() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let diffuse_id = ResourceId::parse("test:block/animated_water").unwrap();
    let normal_id = ResourceId::parse("test:block/animated_water_n").unwrap();
    let specular_id = ResourceId::parse("test:block/animated_water_s").unwrap();
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    // Two genuinely different, spatially varied authored frames.
    let diffuse_pixels: Vec<u8> = [
        [30, 80, 200, 153],
        [50, 100, 220, 153],
        [70, 120, 240, 153],
        [90, 140, 210, 153],
        [200, 80, 30, 153],
        [220, 100, 50, 153],
        [240, 120, 70, 153],
        [210, 140, 90, 153],
    ]
    .into_iter()
    .flatten()
    .collect();
    builder
        .insert(
            diffuse_id.clone(),
            fixture_texture(
                [2, 4],
                &diffuse_pixels,
                Some(r#"{"animation":{"frametime":1}}"#),
                &MissingAnimation::StaticImage,
                Encoding::SrgbColor,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    builder
        .insert(
            normal_id.clone(),
            fixture_texture(
                [1, 1],
                &[220, 128, 180, 255],
                None,
                &MissingAnimation::StaticImage,
                Encoding::LinearData,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    builder
        .insert(
            specular_id.clone(),
            fixture_texture(
                [1, 1],
                &[160, 90, 100, 254],
                None,
                &MissingAnimation::StaticImage,
                Encoding::LinearData,
                fixture_origin(OriginKind::DiagnosticFixture),
                &budget,
            ),
            cancel,
        )
        .unwrap();
    let bank = builder
        .finish(
            vec![TextureRequirement::selected_color(diffuse_id.clone())],
            cancel,
        )
        .unwrap();
    let diffuse = bank.resolve(&diffuse_id).unwrap();
    let normal = bank.resolve(&normal_id).unwrap();
    let specular = bank.resolve(&specular_id).unwrap();
    let texture = bank.texture(diffuse).unwrap();
    // Diagnostic fixtures retain parsed Java timing/rectangles, but the
    // provenance API deliberately reserves authored_schedule for real packs.
    assert!(matches!(
        texture.animation().evidence(),
        AnimationEvidence::JavaMetadata { .. }
    ));
    assert!(texture.animation().changing_rects());
    assert_ne!(
        texture.sample_color([0.25, 0.25], Duration::ZERO),
        texture.sample_color([0.25, 0.25], Duration::from_millis(50)),
    );
    let position = [-3, -2, 1];
    let cells = BTreeMap::from([(position, FluidCell::new(0, [0.7, 0.9, 1.0]).unwrap())]);
    let region = MeshRegion {
        minimum: [-3, -2],
        maximum: [-2, -1],
    };
    let build = || {
        FluidMesh::build(
            &cells,
            &BTreeSet::new(),
            region,
            &bank,
            diffuse,
            16,
            &budget,
            cancel,
        )
        .unwrap()
    };
    let mut plain = build();
    let mut mapped = build();
    plain.faces.retain(|face| face.quad.face == 0);
    mapped.faces.retain(|face| face.quad.face == 0);
    assert_eq!(plain.faces.len(), 1);
    assert_eq!(mapped.faces.len(), 1);
    for face in &mut mapped.faces {
        face.quad.material.normal_map = Some(normal);
        face.quad.material.specular_map = Some(specular);
    }
    let camera = [-2.5, -1.5, 1.0];
    let plain_at_zero = fluid_pixels(
        &[&plain],
        &bank,
        &budget,
        camera,
        [96, 96],
        Duration::ZERO,
        cancel,
    );
    let mapped_at_zero = fluid_pixels(
        &[&mapped],
        &bank,
        &budget,
        camera,
        [96, 96],
        Duration::ZERO,
        cancel,
    );
    let mapped_at_next = fluid_pixels(
        &[&mapped],
        &bank,
        &budget,
        camera,
        [96, 96],
        Duration::from_millis(50),
        cancel,
    );
    let top = |pixel: &PixelResult| {
        pixel.front_owner.is_some_and(|owner| owner.face == 0)
            && pixel.contributors.iter().flatten().count() == 1
    };
    let mut map_changed = false;
    let mut frame_changed = false;
    for ((plain, first), next) in plain_at_zero
        .iter()
        .zip(&mapped_at_zero)
        .zip(&mapped_at_next)
    {
        if !top(first) {
            continue;
        }
        assert_eq!(first.color.alpha(), 153.0 / 255.0);
        assert_eq!(next.color.alpha(), first.color.alpha());
        map_changed |= plain.color != first.color;
        frame_changed |= first.color != next.color;
    }
    assert!(
        map_changed,
        "normal/AO/specular maps stopped affecting generated water"
    );
    assert!(
        frame_changed,
        "authored diffuse animation stopped affecting generated water"
    );
}

#[test]
fn native_fluid_modes_keep_exact_unshifted_source_color_and_alpha() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let rgba = [94, 113, 235, 166];
    let (bank, id) = uniform_water_bank(&budget, cancel, [16, 16], rgba, None);
    let handle = bank.resolve(&id).unwrap();
    let texture = bank.texture(handle).unwrap();
    let raw = texture
        .sample_native_color([0.25, 0.25], Duration::ZERO)
        .unwrap();
    let position = [-3, -2, 1];
    let region = MeshRegion {
        minimum: [-3, -2],
        maximum: [-2, -1],
    };
    let cells = BTreeMap::from([(position, FluidCell::new(0, [1.0; 3]).unwrap())]);
    for mode in [AlphaMode::NativeBlend, AlphaMode::NativeSolid] {
        let generated = FluidMesh::build(
            &cells,
            &BTreeSet::new(),
            region,
            &bank,
            handle,
            16,
            &budget,
            cancel,
        )
        .unwrap();
        let mut top = generated
            .faces
            .into_iter()
            .find(|face| face.quad.face == 0)
            .unwrap();
        top.quad.material.alpha = mode;
        top.quad.material.tint = [0.7, 0.8, 0.9];
        top.quad.uv = [[0.25, 0.25]; 4];
        top.quad.shade = true;
        let shade = DirectionalLight::default()
            .factor(top.quad.normal, top.quad.shade)
            .unwrap();
        let native =
            FluidMesh::from_native_faces(vec![top], region, &bank, &budget, cancel).unwrap();
        let expected = LinearRgba::from_straight(
            [
                raw[0] * 0.7 * shade,
                raw[1] * 0.8 * shade,
                raw[2] * 0.9 * shade,
            ],
            if mode == AlphaMode::NativeSolid {
                1.0
            } else {
                raw[3]
            },
        )
        .unwrap();
        let first = fluid_pixels(
            &[&native],
            &bank,
            &budget,
            [-2.5, -1.5, 1.0],
            [96, 96],
            Duration::ZERO,
            cancel,
        );
        let later = fluid_pixels(
            &[&native],
            &bank,
            &budget,
            [-2.5, -1.5, 1.0],
            [96, 96],
            Duration::from_millis(400),
            cancel,
        );
        let mut visible = 0;
        for (a, b) in first.iter().zip(&later) {
            assert_eq!(pixel_signature(a), pixel_signature(b));
            if a.front_owner.is_some_and(|owner| owner.face == 0) {
                visible += 1;
                assert_eq!(a.color, expected);
            }
        }
        assert!(visible >= 16);
    }
}

#[test]
fn generated_water_keeps_bounded_account_cancel_and_failure_semantics() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, id) = uniform_water_bank(&budget, cancel, [16, 16], [101, 255, 255, 179], None);
    let cells = BTreeMap::from([([-3, -2, 1], FluidCell::new(0, [1.0; 3]).unwrap())]);
    let fluid = FluidMesh::build(
        &cells,
        &BTreeSet::new(),
        MeshRegion {
            minimum: [-3, -2],
            maximum: [-2, -1],
        },
        &bank,
        bank.resolve(&id).unwrap(),
        16,
        &budget,
        cancel,
    )
    .unwrap();
    let mut frame = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let used = budget.used();
    let peak = budget.peak();
    draw_fluid_mesh(
        &fluid,
        &bank,
        [-2.5, -1.5, 1.0],
        16.0,
        Duration::ZERO,
        DirectionalLight::default(),
        &mut frame,
        cancel,
    )
    .unwrap();
    assert_eq!(
        budget.used(),
        used,
        "per-fragment effect retained allocated bytes"
    );
    assert_eq!(
        budget.peak(),
        peak,
        "per-fragment effect reserved scratch bytes"
    );
    stop.store(true, Ordering::Release);
    assert!(matches!(
        draw_fluid_mesh(
            &fluid,
            &bank,
            [-2.5, -1.5, 1.0],
            16.0,
            Duration::from_millis(400),
            DirectionalLight::default(),
            &mut frame,
            cancel,
        ),
        Err(AssetError::Cancelled)
    ));
    assert!(!frame.is_valid());
    assert!(frame.pixel(0, 0).is_err());
    stop.store(false, Ordering::Release);
    frame.clear();
    assert!(frame.is_valid());
    let mut bounded = RasterFrame::new(
        [64, 64],
        RasterLimits {
            sample_tests: 1,
            ..RasterLimits::default()
        },
        &budget,
        cancel,
    )
    .unwrap();
    assert!(matches!(
        draw_fluid_mesh(
            &fluid,
            &bank,
            [-2.5, -1.5, 1.0],
            16.0,
            Duration::ZERO,
            DirectionalLight::default(),
            &mut bounded,
            cancel,
        ),
        Err(AssetError::Limit {
            resource: "raster sample tests",
            ..
        })
    ));
    assert!(!bounded.is_valid());
    let available = budget.limit() - budget.used();
    assert!(available > 1024);
    let hold = budget.reserve(available - 1024, cancel).unwrap();
    assert!(matches!(
        RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel),
        Err(AssetError::Limit {
            resource: "working bytes",
            ..
        })
    ));
    drop(hold);
    assert!(RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).is_ok());
    let foreign = ByteBudget::new(16 << 20).unwrap();
    let mut wrong_account =
        RasterFrame::new([64, 64], RasterLimits::default(), &foreign, cancel).unwrap();
    assert!(draw_fluid_mesh(
        &fluid,
        &bank,
        [-2.5, -1.5, 1.0],
        16.0,
        Duration::ZERO,
        DirectionalLight::default(),
        &mut wrong_account,
        cancel,
    )
    .is_err());
    assert!(!wrong_account.is_valid());
}

#[test]
fn nonfluid_opaque_solid_keeps_exact_source_lighting_and_time_invariant_frame() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, ground, _) = bound_pair(&budget, cancel);
    let position = [0, 0, 0];
    let mesh = PreparedMesh::build(
        &BTreeMap::from([(position, ground)]),
        MeshRegion {
            minimum: [-1, -1],
            maximum: [2, 2],
        },
        bank.identity(),
        32,
        &budget,
        cancel,
    )
    .unwrap();
    let light = DirectionalLight::default();
    let mut frame = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let mut snapshots = Vec::new();
    for time in [Duration::ZERO, Duration::from_millis(400)] {
        draw_mesh(
            &mesh,
            &bank,
            [0.5, 0.5, 0.5],
            16.0,
            time,
            light,
            &mut frame,
            cancel,
        )
        .unwrap();
        snapshots.push(
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .map(|(x, y)| frame.pixel(x, y).unwrap())
                .collect::<Vec<_>>(),
        );
    }
    let mut solid_pixels = 0;
    for (first, later) in snapshots[0].iter().zip(&snapshots[1]) {
        assert_eq!(pixel_signature(first), pixel_signature(later));
        let Some(owner) = first.front_owner else {
            assert_eq!(first.color, LinearRgba::CLEAR);
            continue;
        };
        assert_eq!(owner.position, position);
        solid_pixels += 1;
        let face = mesh.faces.iter().find(|face| face.owner == owner).unwrap();
        let quad = &face.model.quads[usize::from(face.quad_index)];
        assert_eq!(quad.material.alpha, AlphaMode::Opaque);
        assert!(quad.material.normal_map.is_none() && quad.material.specular_map.is_none());
        let texture = bank.texture(quad.material.texture).unwrap();
        assert_eq!(texture.image().pixel(0, 0), Some([140, 100, 50, 255]));
        let source = texture.sample_color([0.5, 0.5], Duration::ZERO).unwrap();
        assert_eq!(
            source,
            texture
                .sample_color([0.5, 0.5], Duration::from_millis(400))
                .unwrap()
        );
        let factor = light.factor(quad.normal, quad.shade).unwrap();
        let expected = LinearRgba::from_straight(
            std::array::from_fn(|channel| {
                source.straight()[channel] * quad.material.tint[channel] * factor
            }),
            source.alpha(),
        )
        .unwrap();
        assert_eq!(first.color, expected);
        assert_eq!(first.color.alpha(), 1.0);
    }
    assert!(solid_pixels >= 16, "opaque solid did not reach the raster");
}

// Synthetic counterpart assets exercise real normalization, binding and raster APIs; they are not authentic pack artwork.
fn night_flora_pipeline_witness() -> (super::surface_generation::SurfaceWorld, [i32; 3]) {
    use super::{
        settings::VoxelLandscapeSettings,
        surface_biomes::SurfaceBiome,
        surface_generation::{prepare, Region},
        terrain_fields::TerrainFields,
    };
    let settings = VoxelLandscapeSettings {
        seed: 71839,
        atmosphere: 0,
        vegetation_percent: 100,
        structures_percent: 0,
        ..Default::default()
    };
    let fields = TerrainFields::new(u64::from(settings.seed));
    let mut attempts = 0_usize;
    for grid_y in -128_i32..0 {
        for grid_x in -128_i32..0 {
            let center = [grid_x * 128 + 64, grid_y * 128 + 64];
            let sample = fields.sample(center[0], center[1], settings.rivers);
            if super::surface_biome_selector::select(u64::from(settings.seed), center, sample)
                != SurfaceBiome::PaleGarden
            {
                continue;
            }
            attempts += 1;
            let region = Region {
                minimum: center.map(|value| value - 32),
                maximum: center.map(|value| value + 32),
            };
            let world = prepare(region, &settings, || false).unwrap();
            let position = world
                .blocks
                .iter()
                .find(|(_, block)| block.state.id().as_str() == "minecraft:closed_eyeblossom")
                .map(|(position, _)| *position);
            if let Some(position) = position {
                return (world, position);
            }
            assert!(
                attempts < 16,
                "no final-owned eyeblossom in sixteen Pale Garden candidates"
            );
        }
    }
    panic!("no final-owned eyeblossom within the bounded natural-witness search");
}
fn night_flora_pipeline_texture(
    pixel: [u8; 4],
    origin: super::assets::identity::BlobOrigin,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Arc<super::assets::texture::Texture> {
    use super::assets::{
        animation::AnimationPlan,
        identity::SourceBlob,
        pixels::{fixture_png, ImageExpectations, PixelImage},
        texture::Texture,
    };
    // This fixture is one pixel. Keep its decoder allowance inside the existing
    // 32 MiB test account; production ceilings and budget admission are unchanged.
    let limits = Limits {
        decoder_scratch_bytes: 1 << 20,
        ..Limits::default()
    };
    let blob = SourceBlob::new(
        fixture_png(1, 1, &pixel),
        origin,
        None,
        &limits,
        budget,
        cancel,
    )
    .unwrap();
    let image = Arc::new(
        PixelImage::decode_png(&blob, ImageExpectations::default(), &limits, budget, cancel)
            .unwrap(),
    );
    let plan = AnimationPlan::build(
        [1, 1],
        None,
        &MissingAnimation::StaticImage,
        &limits,
        budget,
        cancel,
    )
    .unwrap();
    Arc::new(Texture::new(image, plan, Encoding::SrgbColor, budget, cancel).unwrap())
}

fn night_flora_pipeline_assets(
    open_pixel: Option<[u8; 4]>,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> (TextureBank, DefinitionSet) {
    use super::assets::{
        identity::AssetPath,
        layers::{ResourceKey, ResourceKind},
        models::Direction,
    };
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        Limits::default(),
        budget.clone(),
        cancel,
    )
    .unwrap();
    let mut definitions = DefinitionSet::new(
        fixture_origin(OriginKind::DiagnosticFixture),
        Limits::default(),
        budget.clone(),
    )
    .unwrap();
    let reason =
        Label::new("Synthetic eyeblossom identity regression; not selected pack artwork").unwrap();
    let mut requirements = Vec::new();
    for (name, from, to, pixel) in [
        (
            "closed_eyeblossom",
            [6, 0, 6],
            [10, 12, 10],
            Some([0, 0, 255, 255]),
        ),
        ("open_eyeblossom", [1, 0, 5], [15, 7, 11], open_pixel),
    ] {
        let block_id = ResourceId::parse(&format!("minecraft:{name}")).unwrap();
        let texture_id = ResourceId::parse(&format!("minecraft:block/{name}")).unwrap();
        let mut faces = serde_json::Map::new();
        for face in Direction::ALL {
            faces.insert(
                face.name().into(),
                serde_json::json!({"texture":"#petal","uv":[2,3,14,15]}),
            );
        }
        let model = serde_json::json!({"textures":{"petal":texture_id.as_str()},"elements":[{"from":from,"to":to,"shade":false,"faces":faces}]});
        definitions
            .insert_model(texture_id.as_str(), &model, reason.clone(), cancel)
            .unwrap();
        definitions
            .insert(
                ResourceKey {
                    kind: ResourceKind::Blockstate,
                    id: block_id,
                },
                &serde_json::json!({"variants":{"":{"model":texture_id.as_str()}}}),
                reason.clone(),
                cancel,
            )
            .unwrap();
        requirements.push(TextureRequirement::selected_color(texture_id.clone()));
        let Some(pixel) = pixel else {
            continue;
        };
        let mut origin = fixture_origin(OriginKind::DiagnosticFixture);
        origin.path = AssetPath::parse(&format!("fixtures/night_flora/{name}.png")).unwrap();
        builder
            .insert(
                texture_id,
                night_flora_pipeline_texture(pixel, origin, budget, cancel),
                cancel,
            )
            .unwrap();
    }
    (builder.finish(requirements, cancel).unwrap(), definitions)
}
#[test]
fn night_eyeblossom_generated_counterpart_keeps_exact_synthetic_geometry_pixels_and_bank() {
    use super::{
        assets::models::Direction, settings::VoxelLandscapeSettings, surface_generation::prepare,
    };
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (day, position) = night_flora_pipeline_witness();
    let day_block = &day.blocks[&position];
    assert!(matches!(
        &day_block.owner,
        super::surface_generation::SourceOwner::Flora { .. }
    ));
    let (bank, definitions) = night_flora_pipeline_assets(Some([255, 0, 0, 255]), &budget, cancel);
    let (other_bank, _) = night_flora_pipeline_assets(Some([0, 255, 0, 255]), &budget, cancel);
    assert_ne!(bank.identity(), other_bank.identity());
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let mut prior_model = None;
    for (atmosphere, name, color, top) in [
        (
            0,
            "closed_eyeblossom",
            [0.0, 0.0, 1.0],
            [
                [0.375, 0.375, 0.75],
                [0.625, 0.375, 0.75],
                [0.625, 0.625, 0.75],
                [0.375, 0.625, 0.75],
            ],
        ),
        (
            1,
            "open_eyeblossom",
            [1.0, 0.0, 0.0],
            [
                [0.0625, 0.3125, 0.4375],
                [0.9375, 0.3125, 0.4375],
                [0.9375, 0.6875, 0.4375],
                [0.0625, 0.6875, 0.4375],
            ],
        ),
        (
            2,
            "closed_eyeblossom",
            [0.0, 0.0, 1.0],
            [
                [0.375, 0.375, 0.75],
                [0.625, 0.375, 0.75],
                [0.625, 0.625, 0.75],
                [0.375, 0.625, 0.75],
            ],
        ),
    ] {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            atmosphere,
            vegetation_percent: 100,
            structures_percent: 0,
            ..Default::default()
        };
        let world = prepare(day.region, &settings, || false).unwrap();
        let block = &world.blocks[&position];
        assert_eq!(block.owner, day_block.owner);
        assert_eq!(block.state.properties(), day_block.state.properties());
        let normalized = compiler
            .compile_state(&block.state, position, world.seed, cancel)
            .unwrap();
        let (_, model) = &normalized.applications[0];
        assert_eq!(
            model
                .quads
                .iter()
                .find(|quad| quad.face == Direction::Up)
                .unwrap()
                .points,
            top
        );
        let resource = ResourceId::parse(&format!("minecraft:block/{name}")).unwrap();
        assert_eq!(model.id, resource);
        assert_eq!(normalized.state, block.state);
        assert_eq!(
            normalized.state_origin.origin.path.as_str(),
            format!("compatibility/Blockstate/minecraft/{name}.json")
        );
        assert!(normalized.state_origin.compatibility.is_some());
        assert_eq!(model.origins.len(), 1);
        assert_eq!(
            model.origins[0].origin.path.as_str(),
            format!("compatibility/Model/minecraft/block/{name}.json")
        );
        assert_eq!(model.quads.len(), 6);
        assert!(model.quads.iter().all(|quad| quad.texture == resource
            && !quad.shade
            && quad.tint_index.is_none()
            && quad.uv
                == [
                    [0.125, 0.1875],
                    [0.875, 0.1875],
                    [0.875, 0.9375],
                    [0.125, 0.9375]
                ]));
        let repeated = compiler
            .compile_state(&block.state, position, world.seed, cancel)
            .unwrap();
        assert!(Arc::ptr_eq(model, &repeated.applications[0].1));
        if atmosphere == 0 {
            prior_model = Some(Arc::clone(model));
        }
        if atmosphere == 1 {
            assert!(!Arc::ptr_eq(prior_model.as_ref().unwrap(), model));
        }
        let texture = bank.texture(bank.resolve(&resource).unwrap()).unwrap();
        assert_eq!(texture.image().dimensions(), [1, 1]);
        assert_eq!(
            texture.image().origin().path.as_str(),
            format!("fixtures/night_flora/{name}.png")
        );
        let materials = MaterialTable {
            medium: None,
            rules: BTreeMap::from([(
                resource,
                TextureRenderRule {
                    alpha: AlphaMode::Cutout { threshold: 128 },
                    layer: 0,
                    normal_map: None,
                    specular_map: None,
                },
            )]),
            tints: BTreeMap::new(),
        };
        let bound =
            Arc::new(BoundModel::bind(&normalized, &bank, &materials, &budget, cancel).unwrap());
        let instances = BTreeMap::from([(position, bound)]);
        let region = MeshRegion {
            minimum: [position[0] - 1, position[1] - 1],
            maximum: [position[0] + 2, position[1] + 2],
        };
        let mesh =
            PreparedMesh::build(&instances, region, bank.identity(), 64, &budget, cancel).unwrap();
        assert!(PreparedMesh::build(
            &instances,
            region,
            other_bank.identity(),
            64,
            &budget,
            cancel
        )
        .is_err());
        let camera = position.map(|value| f64::from(value) + 0.5);
        let mut stale_frame =
            RasterFrame::new([32, 32], RasterLimits::default(), &budget, cancel).unwrap();
        assert!(draw_mesh(
            &mesh,
            &other_bank,
            camera,
            16.0,
            Duration::ZERO,
            DirectionalLight::default(),
            &mut stale_frame,
            cancel
        )
        .is_err());
        drop(stale_frame);
        let mut snapshots = Vec::new();
        for time in [Duration::ZERO, Duration::from_secs(3600)] {
            let mut frame =
                RasterFrame::new([32, 32], RasterLimits::default(), &budget, cancel).unwrap();
            draw_mesh(
                &mesh,
                &bank,
                camera,
                16.0,
                time,
                DirectionalLight::default(),
                &mut frame,
                cancel,
            )
            .unwrap();
            let pixels: Vec<_> = (0..1024)
                .map(|index| {
                    let pixel = frame.pixel(index % 32, index / 32).unwrap();
                    (pixel.color, pixel.front_owner)
                })
                .collect();
            assert!(pixels.iter().any(|(_, owner)| owner.is_some()));
            for (pixel, owner) in &pixels {
                let Some(owner) = owner else {
                    assert_eq!(*pixel, LinearRgba::CLEAR);
                    continue;
                };
                assert_eq!(owner.position, position);
                assert_eq!(owner.part, 0);
                assert_eq!(owner.layer, 0);
                assert_eq!(*pixel, rgba(color, 1.0));
            }
            snapshots.push(pixels);
        }
        assert_eq!(snapshots[0], snapshots[1]);
    }
}
#[test]
fn night_eyeblossom_pipeline_missing_open_image_and_cancel_do_not_publish_closed_substitutes() {
    let budget = ByteBudget::new(32 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let (bank, definitions) = night_flora_pipeline_assets(None, &budget, cancel);
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let materials = MaterialTable {
        medium: None,
        rules: BTreeMap::from([(
            ResourceId::parse("minecraft:block/open_eyeblossom").unwrap(),
            TextureRenderRule {
                alpha: AlphaMode::Cutout { threshold: 128 },
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        )]),
        tints: BTreeMap::new(),
    };
    let state = BlockState::new(
        ResourceId::parse("minecraft:open_eyeblossom").unwrap(),
        [("schedule_tick".into(), "true".into())],
    )
    .unwrap();
    let normalized = compiler
        .compile_state(&state, [-17, -31, 64], 71839, cancel)
        .unwrap();
    let (complete_bank, _) = night_flora_pipeline_assets(Some([255, 0, 0, 255]), &budget, cancel);
    BoundModel::bind(&normalized, &complete_bank, &materials, &budget, cancel).unwrap();
    assert!(bank
        .resolve(&ResourceId::parse("minecraft:block/closed_eyeblossom").unwrap())
        .is_some());
    assert!(bank
        .resolve(&ResourceId::parse("minecraft:block/open_eyeblossom").unwrap())
        .is_none());
    let failure = match BoundModel::bind(&normalized, &bank, &materials, &budget, cancel) {
        Ok(_) => panic!("missing open image bound a substitute"),
        Err(error) => error,
    };
    assert!(!matches!(
        failure,
        super::assets::error::AssetError::Cancelled
            | super::assets::error::AssetError::Allocation
            | super::assets::error::AssetError::Limit { .. }
    ));
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(matches!(
        compiler.compile_state(&state, [-17, -31, 64], 71839, cancel),
        Err(super::assets::error::AssetError::Cancelled)
    ));
    stop.store(false, std::sync::atomic::Ordering::Release);
    let retried = compiler
        .compile_state(&state, [-17, -31, 64], 71839, cancel)
        .unwrap();
    assert!(Arc::ptr_eq(
        &normalized.applications[0].1,
        &retried.applications[0].1
    ));
}
