//! Original geometry/pixel fixtures. These tests cannot establish native selected-pack visual acceptance.
use super::{
    assets::{
        animation::MissingAnimation,
        bank::{TextureBank, TextureBankBuilder, TextureRequirement},
        block_state::BlockState,
        budget::{ByteBudget, Cancel, Limits},
        compatibility::DefinitionSet,
        identity::{Label, OriginKind, ResourceId},
        models::ModelCompiler,
        review::{fixture_origin, fixture_review},
        texture::{fixture_texture, Encoding, LinearRgba},
    },
    surface_mesh::{
        AlphaMode, BoundModel, FaceOwner, MaterialTable, MeshRegion, PreparedMesh,
        TextureRenderRule,
    },
    surface_raster::{
        draw_mesh, DirectionalLight, FlatShader, FragmentShader, RasterFrame, RasterLimits, Vertex,
    },
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, Arc},
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
    let mut builder = TextureBankBuilder::new(
        fixture_review(),
        vec![],
        limits.clone(),
        budget.clone(),
        cancel,
    )
    .unwrap();
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
        limits.clone(),
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
    let region = MeshRegion { minimum: [-1, -1], maximum: [2, 2] };
    let bed = PreparedMesh::build(&BTreeMap::from([([0, 0, 0], ground)]),
        region, bank.identity(), 100, &budget, cancel).unwrap();
    let fluid = FluidMesh::build(
        &BTreeMap::from([([0, 0, 1], FluidCell::new(4, [0.8, 0.9, 1.0]).unwrap())]),
        &BTreeSet::from([[0, 0, 0]]), region, &bank,
        bank.resolve(&ResourceId::parse("test:block/water").unwrap()).unwrap(),
        16, &budget, cancel).unwrap();
    assert_eq!(fluid.faces.len(), 5);
    assert!(fluid.faces[0].quad.points.iter().all(|point| point[2] == 0.5));
    let mut frame = RasterFrame::new([64, 64], RasterLimits::default(), &budget, cancel).unwrap();
    let mut snapshots = Vec::new();
    for time in [Duration::ZERO, Duration::from_millis(50)] {
        draw_mesh(&bed, &bank, [0.5, 0.5, 1.0], 16.0, time,
            DirectionalLight::default(), &mut frame, cancel).unwrap();
        draw_fluid_mesh(&fluid, &bank, [0.5, 0.5, 1.0], 16.0, time,
            DirectionalLight::default(), &mut frame, cancel).unwrap();
        snapshots.push((0..4096).map(|i| frame.pixel(i % 64, i / 64).unwrap())
            .collect::<Vec<_>>());
    }
    assert!(snapshots[0].iter().any(|pixel| {
        let owners: Vec<_> = pixel.contributors.iter().flatten().map(|owner| owner.position).collect();
        owners.contains(&[0, 0, 0]) && owners.contains(&[0, 0, 1])
    }));
    assert!(snapshots[0].iter().zip(&snapshots[1]).any(|(a, b)| a.color != b.color));
}
