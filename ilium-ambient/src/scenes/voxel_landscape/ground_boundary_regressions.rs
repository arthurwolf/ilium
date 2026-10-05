//! Synthetic selected-source regressions: real model compilation, PNG import, binding and mesh preparation.
use super::super::assets::{models::Direction, pixels::fixture_png};
use super::super::surface_generation::{SourceOwner, SurfaceBlock};
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
fn resource(name: &str) -> ResourceId {
    ResourceId::parse(name).unwrap()
}
fn state(name: &str) -> BlockState {
    BlockState::new(resource(name), []).unwrap()
}
fn box_model(from: [i32; 3], to: [i32; 3], cull: bool) -> Value {
    let mut faces = serde_json::Map::new();
    for face in Direction::ALL {
        let mut value = serde_json::json!({"texture":"#all"});
        if cull {
            value["cullface"] = serde_json::json!(face.name());
        }
        faces.insert(face.name().into(), value);
    }
    serde_json::json!({"textures":{"all":"test:block/cover"},"elements":[{"from":from,"to":to,"faces":faces}]})
}
fn put_json(members: &mut BTreeMap<AssetPath, Vec<u8>>, path: &str, value: &Value) {
    members.insert(
        AssetPath::parse(path).unwrap(),
        serde_json::to_vec(value).unwrap(),
    );
}
fn fixture_members(cover: Value, cover_pixels: &[u8]) -> BTreeMap<AssetPath, Vec<u8>> {
    assert_eq!(cover_pixels.len(), 8);
    let mut members = BTreeMap::new();
    let mut ground = box_model([0, 0, 0], [16, 16, 16], true);
    ground["textures"]["all"] = serde_json::json!("test:block/ground");
    put_json(
        &mut members,
        "assets/test/models/block/ground.json",
        &ground,
    );
    put_json(
        &mut members,
        "assets/test/blockstates/ground.json",
        &serde_json::json!({"variants":{"":{"model":"test:block/ground"}}}),
    );
    put_json(&mut members, "assets/test/models/block/cover.json", &cover);
    put_json(
        &mut members,
        "assets/test/blockstates/cover.json",
        &serde_json::json!({"variants":{"":{"model":"test:block/cover"}}}),
    );
    members.insert(
        AssetPath::parse("assets/test/textures/block/ground.png").unwrap(),
        fixture_png(1, 1, &[140, 80, 30, 255]),
    );
    members.insert(
        AssetPath::parse("assets/test/textures/block/cover.png").unwrap(),
        fixture_png(2, 1, cover_pixels),
    );
    members
}
fn sources_from_members(
    budget: &ByteBudget,
    members: BTreeMap<AssetPath, Vec<u8>>,
) -> BindingSources {
    let stop = AtomicBool::new(false);
    let pack =
        LayeredPack::fixture_ground_members(members, budget.clone(), Cancel::new(&stop)).unwrap();
    BindingSources {
        packs: vec![pack],
        limits: Limits::default(),
        budget: budget.clone(),
        source_sha256: None,
        profile_id: "synthetic_ground_boundary",
        goodvibes: false,
        plasticator: false,
        exact_plasticator_campfire_source: false,
        aliases: BTreeMap::new(),
        fallback_aliases: BTreeMap::new(),
        fallback_goodvibes: false,
        fallback_unavailable: false,
    }
}
fn fixture_sources(budget: &ByteBudget, cover: Value, cover_pixels: &[u8]) -> BindingSources {
    sources_from_members(budget, fixture_members(cover, cover_pixels))
}
fn filled_world(origin: [i32; 3], extent: [i32; 3]) -> SurfaceWorld {
    let mut blocks = BTreeMap::new();
    for x in 0..extent[0] {
        for y in 0..extent[1] {
            for z in 0..extent[2] {
                let position = [origin[0] + x, origin[1] + y, origin[2] + z];
                blocks.insert(
                    position,
                    SurfaceBlock {
                        state: state("test:ground"),
                        owner: SourceOwner::Geology {
                            anchor: position,
                            source: "synthetic ground boundary regression",
                        },
                    },
                );
            }
        }
    }
    SurfaceWorld {
        region: Region {
            minimum: [origin[0], origin[1]],
            maximum: [origin[0] + extent[0], origin[1] + extent[1]],
        },
        seed: 71839,
        columns: BTreeMap::new(),
        biomes: BTreeMap::new(),
        blocks,
        fluids: BTreeMap::new(),
        trees: Vec::new(),
        flora: Vec::new(),
        structures: Vec::new(),
        entities: Vec::new(),
        source_limitations: vec![
            "Synthetic selected geometry/PNG admission fixture; not private-pack pixels",
        ],
    }
}
fn cover_cell(world: &mut SurfaceWorld, position: [i32; 3]) {
    world.blocks.insert(
        position,
        SurfaceBlock {
            state: state("test:cover"),
            owner: SourceOwner::Geology {
                anchor: position,
                source: "synthetic selected cover",
            },
        },
    );
}
fn prepare_fixture(
    world: SurfaceWorld,
    sources: &BindingSources,
    core: Option<Region>,
    cancel: Cancel<'_>,
) -> Result<PreparedSurface> {
    let surface = prepare_world_from_sources(
        world,
        None,
        sources,
        core,
        None,
        None,
        &VoxelLandscapeSettings::default(),
        cancel,
    )?
    .ok_or_else(|| {
        AssetError::InvalidMetadata(
            "binding fixture unexpectedly returned collection-only output".into(),
        )
    })?;
    assert!(
        surface.model_substitutions.is_empty(),
        "selected fixture geometry was substituted"
    );
    Ok(surface)
}
fn has_face(surface: &PreparedSurface, position: [i32; 3], normal: [f32; 3]) -> bool {
    surface.mesh.faces.iter().any(|face| {
        face.position == position && face.model.quads[usize::from(face.quad_index)].normal == normal
    })
}
fn model_at(surface: &PreparedSurface, position: [i32; 3]) -> &BoundModel {
    let model = surface
        .mesh
        .faces
        .iter()
        .find(|face| face.position == position)
        .map(|face| face.model.as_ref())
        .expect("expected a published model at the exact global cell");
    assert!(model
        .origins
        .iter()
        .all(|origin| origin.origin.kind == OriginKind::SelectedPack
            && origin.compatibility.is_none()));
    model
}
#[test]
fn generated_ground_cutout_selected_png_keeps_fully_surrounded_substrate() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([0, 0, 0], [16, 16, 16], true),
        &[30, 190, 50, 0, 30, 190, 50, 255],
    );
    let mut world = filled_world([-2, -2, 0], [5, 5, 1]);
    cover_cell(&mut world, [0, 0, 1]);
    for step in [[0, 0, 1], [-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0]] {
        assert!(world.blocks.contains_key(&step));
    }
    let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
    let cover = model_at(&surface, [0, 0, 1]);
    assert!(cover.opaque_boundaries.iter().all(|opaque| !opaque));
    assert!(cover
        .quads
        .iter()
        .all(|quad| quad.material.alpha == AlphaMode::Cutout { threshold: 128 }));
    assert!(
        has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]),
        "fully surrounded ground beneath selected cutout was rejected before PreparedMesh"
    );
    assert!(surface.skipped_states.is_empty());
    assert!(budget.peak() <= budget.limit());
}
#[test]
fn generated_ground_selected_geometry_and_alpha_override_occupancy_without_id_rules() {
    let stop = AtomicBool::new(false);
    for (from, to, alpha, retained) in [
        ([0, 0, 0], [16, 16, 16], 255, false),
        ([4, 0, 4], [12, 16, 12], 255, true),
        ([0, 0, 0], [16, 16, 16], 0, true),
    ] {
        let budget = ByteBudget::new(256 << 20).unwrap();
        let sources = fixture_sources(
            &budget,
            box_model(from, to, true),
            &[70, 180, 30, alpha, 70, 180, 30, 255],
        );
        let mut world = filled_world([-2, -2, 0], [5, 5, 1]);
        cover_cell(&mut world, [0, 0, 1]);
        let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
        assert_eq!(
            has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]),
            retained,
            "selected geometry {from:?}..{to:?} and alpha {alpha} chose wrong ground admission"
        );
        assert_eq!(
            model_at(&surface, [0, 0, 1]).state.id(),
            &resource("test:cover")
        );
        assert!(surface.skipped_states.is_empty());
    }
}
#[test]
fn generated_ground_survives_six_selected_cutout_layers_in_vertical_and_horizontal_chains() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([0, 0, 0], [16, 16, 16], true),
        &[70, 180, 30, 0, 70, 180, 30, 255],
    );
    let mut vertical = filled_world([-2, -2, 0], [5, 5, 7]);
    for z in 1..=6 {
        cover_cell(&mut vertical, [0, 0, z]);
    }
    let surface = prepare_fixture(vertical, &sources, None, Cancel::new(&stop)).unwrap();
    assert!(has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]));
    for z in 1..=6 {
        assert!(surface
            .mesh
            .faces
            .iter()
            .any(|face| face.position == [0, 0, z]));
    }
    drop(surface);
    let mut horizontal = filled_world([-2, -2, 0], [9, 5, 5]);
    for x in 1..=6 {
        cover_cell(&mut horizontal, [x, 0, 2]);
    }
    let surface = prepare_fixture(horizontal, &sources, None, Cancel::new(&stop)).unwrap();
    assert!(has_face(&surface, [0, 0, 2], [1.0, 0.0, 0.0]));
    assert!(surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_shared_collection_imports_buried_only_selected_texture() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut members = fixture_members(
        box_model([0, 0, 0], [16, 16, 16], true),
        &[30, 190, 50, 0, 30, 190, 50, 255],
    );
    let mut ground = box_model([0, 0, 0], [16, 16, 16], true);
    ground["textures"]["all"] = serde_json::json!("test:block/underlay");
    put_json(
        &mut members,
        "assets/test/models/block/underlay.json",
        &ground,
    );
    put_json(
        &mut members,
        "assets/test/blockstates/underlay.json",
        &serde_json::json!({"variants":{"":{"model":"test:block/underlay"}}}),
    );
    members.insert(
        AssetPath::parse("assets/test/textures/block/underlay.png").unwrap(),
        fixture_png(1, 1, &[90, 60, 20, 255]),
    );
    let sources = sources_from_members(&budget, members);
    let make_world = || {
        let mut world = filled_world([-2, -2, 0], [5, 5, 1]);
        cover_cell(&mut world, [0, 0, 1]);
        world.blocks.get_mut(&[0, 0, 0]).unwrap().state = state("test:underlay");
        world
    };
    let core = Some(Region {
        minimum: [-1, -1],
        maximum: [2, 2],
    });
    let mut collected = BTreeMap::new();
    assert!(prepare_world_from_sources(
        make_world(),
        None,
        &sources,
        core,
        None,
        Some(&mut collected),
        &VoxelLandscapeSettings::default(),
        cancel
    )
    .unwrap()
    .is_none());
    assert!(
        collected.contains_key(&resource("test:block/underlay")),
        "buried reachable substrate texture was lost before shared bank import"
    );
    let requests: Vec<_> = collected.into_values().collect();
    let imports = Arc::new(
        TextureImporter::new(&sources.packs, Limits::default(), budget.clone())
            .unwrap()
            .import(&requests, cancel)
            .unwrap(),
    );
    let surface = prepare_world_from_sources(
        make_world(),
        None,
        &sources,
        core,
        Some(Arc::clone(&imports)),
        None,
        &VoxelLandscapeSettings::default(),
        cancel,
    )
    .unwrap()
    .unwrap();
    assert!(Arc::ptr_eq(&imports, &surface.imports));
    assert!(has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]));
    assert_eq!(
        model_at(&surface, [0, 0, 0]).state.id(),
        &resource("test:underlay")
    );
    assert!(surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_true_opaque_cover_keeps_interior_faces_culled() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([0, 0, 0], [16, 16, 16], true),
        &[70, 180, 30, 255, 70, 180, 30, 255],
    );
    let world = filled_world([-4, -4, 0], [9, 9, 9]);
    let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
    assert!(!surface
        .mesh
        .faces
        .iter()
        .any(|face| face.position == [0, 0, 4]));
    assert!(has_face(&surface, [0, 0, 8], [0.0, 0.0, 1.0]));
    assert!(!has_face(&surface, [0, 0, 8], [0.0, 0.0, -1.0]));
    assert!(surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_selected_blend_chain_preserves_geometry_and_medium_policy() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let mut members = fixture_members(
        box_model([0; 3], [16; 3], true),
        &[10, 70, 190, 128, 10, 70, 190, 128],
    );
    let old_path = AssetPath::parse("assets/test/textures/block/cover.png").unwrap();
    let bytes = members.remove(&old_path).unwrap();
    members.insert(
        AssetPath::parse("assets/test/textures/block/glass_cover.png").unwrap(),
        bytes,
    );
    let mut cover = box_model([0; 3], [16; 3], true);
    cover["textures"]["all"] = serde_json::json!("test:block/glass_cover");
    put_json(&mut members, "assets/test/models/block/cover.json", &cover);
    let sources = sources_from_members(&budget, members);
    let mut world = filled_world([-2, -2, 0], [5, 5, 5]);
    for z in 1..=4 {
        cover_cell(&mut world, [0, 0, z]);
    }
    let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
    assert!(has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]));
    let bound = model_at(&surface, [0, 0, 1]);
    assert!(bound
        .quads
        .iter()
        .all(|quad| quad.material.alpha == AlphaMode::Blend));
    assert!(bound.medium.is_none());
    assert!(surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_clipped_ring_admits_a_transparent_chain_without_local_exposure() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[70, 180, 30, 0, 70, 180, 30, 255],
    );
    let mut world = filled_world([-2, -2, 0], [11, 5, 5]);
    for x in 1..=8 {
        cover_cell(&mut world, [x, 0, 2]);
    }
    let core = Region {
        minimum: [-1, -1],
        maximum: [3, 2],
    };
    assert!(exposed(&world, [8, 0, 2]));
    assert!(!needed_for_core_binding(Some(core), [8, 0, 2]));
    for x in 1..=3 {
        assert!(!exposed(&world, [x, 0, 2]));
    }
    let surface = prepare_fixture(world, &sources, Some(core), Cancel::new(&stop)).unwrap();
    assert!(
        has_face(&surface, [0, 0, 2], [1.0, 0.0, 0.0]),
        "clipped transparent ingress ring lost the enclosed core substrate"
    );
    assert!(surface
        .mesh
        .faces
        .iter()
        .all(|face| core.contains(face.position)));
    assert_eq!(
        model_at(&surface, [0, 0, 2]).state.id(),
        &resource("test:ground")
    );
    assert!(surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_selected_overhang_is_not_hidden_by_an_occupancy_shell() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([-8, 0, 0], [24, 16, 16], false),
        &[70, 180, 30, 255, 70, 180, 30, 255],
    );
    let mut world = filled_world([-2, -2, 0], [5, 5, 5]);
    cover_cell(&mut world, [0, 0, 2]);
    assert!(!exposed(&world, [0, 0, 2]));
    let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
    let cover = model_at(&surface, [0, 0, 2]);
    assert!(cover
        .quads
        .iter()
        .flat_map(|quad| quad.points)
        .flatten()
        .any(|coordinate| !(0.0..=1.0).contains(&coordinate)));
    assert!(cover.quads.iter().all(|quad| quad.cull_face.is_none()));
}
fn owners(surface: &PreparedSurface) -> BTreeSet<FaceOwner> {
    surface.mesh.faces.iter().map(|face| face.owner).collect()
}
fn seam_world(offset: i32) -> SurfaceWorld {
    let mut world = filled_world([offset - 3, offset - 2, 64], [6, 5, 5]);
    for x in [offset - 1, offset] {
        for z in 65..=68 {
            cover_cell(&mut world, [x, offset, z]);
        }
    }
    world
}
#[test]
fn generated_ground_signed_core_halo_seams_match_whole_preparation() {
    let stop = AtomicBool::new(false);
    for offset in [-1_000_000_000, -1, 1_000_000_000] {
        let budget = ByteBudget::new(256 << 20).unwrap();
        let sources = fixture_sources(
            &budget,
            box_model([0; 3], [16; 3], true),
            &[10, 160, 30, 0, 10, 160, 30, 0],
        );
        let whole =
            prepare_fixture(seam_world(offset), &sources, None, Cancel::new(&stop)).unwrap();
        let left = prepare_fixture(
            seam_world(offset),
            &sources,
            Some(Region {
                minimum: [offset - 3, offset - 2],
                maximum: [offset, offset + 3],
            }),
            Cancel::new(&stop),
        )
        .unwrap();
        let right = prepare_fixture(
            seam_world(offset),
            &sources,
            Some(Region {
                minimum: [offset, offset - 2],
                maximum: [offset + 3, offset + 3],
            }),
            Cancel::new(&stop),
        )
        .unwrap();
        assert!([&whole, &left, &right]
            .iter()
            .all(|surface| surface.skipped_states.is_empty()));
        let left_owners = owners(&left);
        let right_owners = owners(&right);
        assert!(left_owners.is_disjoint(&right_owners));
        assert_eq!(
            owners(&whole),
            left_owners.union(&right_owners).copied().collect()
        );
        assert_eq!(whole.bank_epoch, left.bank_epoch);
        assert_eq!(whole.bank_epoch, right.bank_epoch);
        for (surface, x) in [(&left, offset - 1), (&right, offset)] {
            assert!(has_face(surface, [x, offset, 64], [0.0, 0.0, 1.0]));
        }
        let render = |surfaces: &[&PreparedSurface]| {
            let mut frame = RasterFrame::new(
                [64, 64],
                RasterLimits::default(),
                &budget,
                Cancel::new(&stop),
            )
            .unwrap();
            for surface in surfaces {
                surface_raster::draw_mesh_layer(
                    &surface.mesh,
                    surface.bank(),
                    [f64::from(offset), f64::from(offset) + 0.5, 66.5],
                    8.0,
                    Duration::ZERO,
                    surface_raster::DirectionalLight::default(),
                    &mut frame,
                    Cancel::new(&stop),
                )
                .unwrap();
            }
            (0..4096)
                .map(|index| {
                    let pixel = frame.pixel(index % 64, index / 64).unwrap();
                    (pixel.color, pixel.front_owner)
                })
                .collect::<Vec<_>>()
        };
        let reference = render(&[&whole]);
        assert!(reference
            .iter()
            .any(|(_, owner)| owner.is_some_and(|owner| owner.position[0] < offset)));
        assert!(reference
            .iter()
            .any(|(_, owner)| owner.is_some_and(|owner| owner.position[0] >= offset)));
        assert_eq!(reference, render(&[&left, &right]));
        assert_eq!(reference, render(&[&right, &left]));
    }
}
#[test]
fn generated_ground_selected_transparent_pixels_reach_raster_substrate_owner() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[30, 190, 50, 0, 30, 190, 50, 0],
    );
    let mut world = filled_world([-2, -2, 0], [5, 5, 1]);
    cover_cell(&mut world, [0, 0, 1]);
    let surface = prepare_fixture(world, &sources, None, Cancel::new(&stop)).unwrap();
    assert!(model_at(&surface, [0, 0, 1])
        .quads
        .iter()
        .all(|quad| quad.material.alpha == AlphaMode::Cutout { threshold: 128 }));
    assert!(surface.skipped_states.is_empty());
    let mut frame = RasterFrame::new(
        [64, 64],
        RasterLimits::default(),
        &budget,
        Cancel::new(&stop),
    )
    .unwrap();
    surface_raster::draw_mesh(
        &surface.mesh,
        surface.bank(),
        [0.5, 0.5, 1.0],
        16.0,
        Duration::ZERO,
        surface_raster::DirectionalLight::default(),
        &mut frame,
        Cancel::new(&stop),
    )
    .unwrap();
    let mut substrate_pixels = 0;
    for index in 0..4096 {
        let pixel = frame.pixel(index % 64, index / 64).unwrap();
        if pixel
            .front_owner
            .is_none_or(|owner| owner.position != [0, 0, 0])
        {
            continue;
        }
        substrate_pixels += 1;
        assert_eq!(pixel.color.alpha(), 1.0);
        assert!(pixel.color.straight()[0] > pixel.color.straight()[1]);
    }
    assert!(
        substrate_pixels > 0,
        "selected alpha holes rendered no owner from the formerly rejected substrate"
    );
}
#[test]
fn generated_ground_weighted_state_geometry_and_selected_provenance_match_direct_compiler() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let mut members = fixture_members(
        box_model([2, 0, 4], [14, 16, 12], true),
        &[15, 190, 40, 0, 15, 190, 40, 255],
    );
    put_json(
        &mut members,
        "assets/test/models/block/cover_alt.json",
        &box_model([5, 0, 1], [11, 12, 15], true),
    );
    put_json(
        &mut members,
        "assets/test/blockstates/cover.json",
        &serde_json::json!({"variants":{"shape=weighted":[{"model":"test:block/cover","weight":1,"y":90,"uvlock":true},{"model":"test:block/cover_alt","weight":3,"x":90,"y":180}]}}),
    );
    let member_digests: BTreeMap<AssetPath, Digest256> = members
        .iter()
        .map(|(path, bytes)| {
            (
                path.clone(),
                Digest256::try_from(format!("{:x}", Sha256::digest(bytes))).unwrap(),
            )
        })
        .collect();
    let expected_texture_digest = *member_digests
        .get(&AssetPath::parse("assets/test/textures/block/cover.png").unwrap())
        .unwrap();
    let sources = sources_from_members(&budget, members);
    let cover_state = BlockState::new(
        resource("test:cover"),
        [("shape".into(), "weighted".into())],
    )
    .unwrap();
    let mut world = filled_world([-33, -2, 64], [66, 5, 1]);
    let seed = world.seed;
    let positions: Vec<_> = (-32..32).map(|x| [x, 0, 65]).collect();
    for &position in &positions {
        world.blocks.insert(
            position,
            SurfaceBlock {
                state: cover_state.clone(),
                owner: SourceOwner::Geology {
                    anchor: position,
                    source: "synthetic weighted selected cover",
                },
            },
        );
    }
    let surface = prepare_fixture(world, &sources, None, cancel).unwrap();
    let definitions = DefinitionSources::new(&sources.packs[..1], &[], None).unwrap();
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let mut choices = BTreeSet::new();
    for position in positions {
        let normalized = compiler
            .compile_state(&cover_state, position, seed, cancel)
            .unwrap();
        assert_eq!(normalized.applications.len(), 1);
        let application = &normalized.applications[0].0;
        let expected = if application.model == resource("test:block/cover") {
            (0, 1, true, 1)
        } else {
            (1, 2, false, 3)
        };
        assert_eq!(
            (
                application.x_turns,
                application.y_turns,
                application.uvlock,
                application.weight
            ),
            expected
        );
        choices.insert(application.model.clone());
        let rules = BTreeMap::from([(
            resource("test:block/cover"),
            TextureRenderRule {
                alpha: AlphaMode::Cutout { threshold: 128 },
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        )]);
        let direct = BoundModel::bind(
            &normalized,
            surface.bank(),
            &MaterialTable {
                medium: None,
                rules,
                tints: BTreeMap::from([(0, [1.0; 3])]),
            },
            &budget,
            cancel,
        )
        .unwrap();
        let actual = model_at(&surface, position);
        assert_eq!(actual.state, cover_state);
        assert_eq!(actual.bank, direct.bank);
        assert_eq!(actual.opaque_boundaries, direct.opaque_boundaries);
        assert_eq!(
            (actual.medium.as_ref(), actual.medium_boundaries),
            (direct.medium.as_ref(), direct.medium_boundaries)
        );
        assert_eq!(
            serde_json::to_vec(&actual.origins).unwrap(),
            serde_json::to_vec(&direct.origins).unwrap()
        );
        assert!(
            !actual.origins.is_empty()
                && actual
                    .origins
                    .iter()
                    .all(|origin| origin.origin.kind == OriginKind::SelectedPack
                        && origin.compatibility.is_none())
        );
        for origin in &actual.origins {
            assert_eq!(
                member_digests.get(&origin.origin.path),
                Some(&origin.sha256)
            );
        }
        assert_eq!(actual.quads.len(), direct.quads.len());
        for face in surface
            .mesh
            .faces
            .iter()
            .filter(|face| face.position == position)
        {
            let quad = &actual.quads[usize::from(face.quad_index)];
            assert_eq!(
                face.owner,
                FaceOwner {
                    position,
                    part: quad.part,
                    face: quad.face,
                    layer: quad.material.layer
                }
            );
        }
        for (actual_quad, expected_quad) in actual.quads.iter().zip(&direct.quads) {
            assert_eq!(
                (actual_quad.points, actual_quad.uv, actual_quad.normal),
                (expected_quad.points, expected_quad.uv, expected_quad.normal)
            );
            assert_eq!(
                (
                    actual_quad.material,
                    actual_quad.cull_face,
                    actual_quad.shade,
                    actual_quad.part,
                    actual_quad.face
                ),
                (
                    expected_quad.material,
                    expected_quad.cull_face,
                    expected_quad.shade,
                    expected_quad.part,
                    expected_quad.face
                )
            );
        }
    }
    assert_eq!(
        choices,
        BTreeSet::from([
            resource("test:block/cover"),
            resource("test:block/cover_alt")
        ])
    );
    let imported = surface
        .imports
        .records
        .iter()
        .find(|record| record.resource == resource("test:block/cover"))
        .unwrap();
    assert_eq!(imported.source_sha256, Some(expected_texture_digest));
    assert_eq!(
        imported.source.as_ref().unwrap().path.as_str(),
        "assets/test/textures/block/cover.png"
    );
    assert_eq!(
        imported.source.as_ref().unwrap().pack,
        sources.packs[0].review().pack
    );
    assert!(surface.model_substitutions.is_empty() && surface.skipped_states.is_empty());
}
#[test]
fn generated_ground_cancellation_and_revision_drop_incomplete_candidates() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(true);
    let sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[20, 190, 40, 0, 20, 190, 40, 255],
    );
    let baseline = budget.used();
    assert!(matches!(
        prepare_fixture(
            filled_world([-2, -2, 0], [5, 5, 3]),
            &sources,
            None,
            Cancel::new(&stop)
        ),
        Err(AssetError::Cancelled)
    ));
    assert_eq!(budget.used(), baseline);
    stop.store(false, Ordering::Release);
    let revision = AtomicU64::new(12);
    assert!(matches!(
        prepare_fixture(
            filled_world([-2, -2, 0], [5, 5, 3]),
            &sources,
            None,
            Cancel::for_revision(&stop, &revision, 11)
        ),
        Err(AssetError::Cancelled)
    ));
    assert_eq!(budget.used(), baseline);
    drop(sources);
    assert_eq!(budget.used(), 0);
    let stop = Arc::new(AtomicBool::new(false));
    let mut sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[20, 190, 40, 0, 20, 190, 40, 255],
    );
    sources.packs[0].fixture_cancel_on_read(Arc::clone(&stop), 6);
    let baseline = budget.used();
    let mut world = filled_world([-2, -2, 0], [5, 5, 5]);
    for z in 1..=4 {
        cover_cell(&mut world, [0, 0, z]);
    }
    assert!(matches!(
        prepare_fixture(world, &sources, None, Cancel::new(stop.as_ref())),
        Err(AssetError::Cancelled)
    ));
    assert!(stop.load(Ordering::Acquire));
    assert_eq!(budget.used(), baseline);
    drop(sources);
    assert_eq!(budget.used(), 0);
}
#[test]
fn generated_ground_tight_account_failure_is_fatal_and_releases_reservations() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[20, 190, 40, 0, 20, 190, 40, 255],
    );
    let source_charge = budget.used();
    let blocker = budget
        .reserve(budget.limit() - source_charge - 64 * 1024, cancel)
        .unwrap();
    let blocked_baseline = budget.used();
    let result = prepare_fixture(filled_world([-2, -2, 0], [5, 5, 5]), &sources, None, cancel);
    assert!(matches!(
        result,
        Err(AssetError::Limit {
            resource: "working bytes",
            ..
        })
    ));
    assert_eq!(budget.used(), blocked_baseline);
    assert!(budget.peak() <= budget.limit());
    drop(blocker);
    assert_eq!(budget.used(), source_charge);
    let mut world = filled_world([-2, -2, 0], [5, 5, 1]);
    cover_cell(&mut world, [0, 0, 1]);
    let surface = prepare_fixture(world, &sources, None, cancel).unwrap();
    assert!(has_face(&surface, [0, 0, 0], [0.0, 0.0, 1.0]));
    drop(surface);
    assert_eq!(budget.used(), source_charge);
    drop(sources);
    assert_eq!(budget.used(), 0);
}
#[test]
fn generated_ground_opaque_shell_does_not_bind_every_buried_cell() {
    let budget = ByteBudget::new(256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let sources = fixture_sources(
        &budget,
        box_model([0; 3], [16; 3], true),
        &[70, 180, 30, 255, 70, 180, 30, 255],
    );
    let world = filled_world([-4, -4, 0], [9, 9, 9]);
    let definitions = DefinitionSources::new(&sources.packs[..1], &[], None).unwrap();
    let mut compiler = ModelCompiler::new(&definitions, Limits::default(), budget.clone()).unwrap();
    let mut normalized = Vec::new();
    let mut cache = BTreeMap::new();
    for (&position, block) in &world.blocks {
        let selected = compiler
            .compile_state(&block.state, position, world.seed, cancel)
            .unwrap();
        normalized.push((
            position,
            retain_normalized(selected, false, &mut cache).unwrap(),
        ));
    }
    let request = request(
        resource("test:block/ground"),
        &sources.packs[0].review().pack,
        &BTreeMap::new(),
        false,
        None,
        false,
    )
    .unwrap();
    let imports = TextureImporter::new(&sources.packs, Limits::default(), budget.clone())
        .unwrap()
        .import(&[request], cancel)
        .unwrap();
    let rules = BTreeMap::from([(
        resource("test:block/ground"),
        TextureRenderRule {
            alpha: AlphaMode::Opaque,
            layer: 0,
            normal_map: None,
            specular_map: None,
        },
    )]);
    let table = MaterialTable {
        medium: None,
        rules,
        tints: BTreeMap::from([(0, [1.0; 3])]),
    };
    let mut attempted = BTreeSet::new();
    let GeneratedInstances {
        instances,
        published,
        _retained_charge: charge,
    } = generated_instances(&world, None, &normalized, &budget, cancel, |position| {
        assert!(
            attempted.insert(position),
            "a position was bound repeatedly"
        );
        let index = normalized
            .binary_search_by_key(&position, |(key, _)| *key)
            .unwrap();
        BoundModel::bind(&normalized[index].1, &imports.bank, &table, &budget, cancel)
            .map(|model| Some(Arc::new(model)))
    })
    .unwrap();
    assert!(
        attempted.len() < world.blocks.len(),
        "all 729 candidates were blindly bound"
    );
    assert!(!attempted.contains(&[0, 0, 4]));
    assert!(!instances.contains_key(&[0, 0, 4]));
    assert!(published.binary_search(&[0, 0, 4]).is_err());
    assert!(instances.contains_key(&[0, 0, 7]));
    assert!(published.binary_search(&[0, 0, 7]).is_err());
    assert!(published.binary_search(&[0, 0, 8]).is_ok());
    assert!(charge.belongs_to(&budget));
    assert!(budget.peak() <= budget.limit());
    drop((instances, published, charge));
    let baseline = budget.used();
    let blocker = budget
        .reserve(budget.limit() - baseline - 1024, cancel)
        .unwrap();
    let blocked_baseline = budget.used();
    let mut bound_during_refusal = false;
    let result = generated_instances(&world, None, &normalized, &budget, cancel, |_| {
        bound_during_refusal = true;
        Ok(None)
    });
    assert!(matches!(
        result,
        Err(AssetError::Limit {
            resource: "working bytes",
            ..
        })
    ));
    assert!(!bound_during_refusal);
    assert_eq!(budget.used(), blocked_baseline);
    drop(blocker);
    for change_revision in [false, true] {
        stop.store(false, Ordering::Release);
        let revision = AtomicU64::new(12);
        let cancel = Cancel::for_revision(&stop, &revision, 12);
        let mut attempts = 0usize;
        let result = generated_instances(&world, None, &normalized, &budget, cancel, |position| {
            let index = normalized
                .binary_search_by_key(&position, |(key, _)| *key)
                .unwrap();
            let model = Arc::new(BoundModel::bind(
                &normalized[index].1,
                &imports.bank,
                &table,
                &budget,
                cancel,
            )?);
            attempts += 1;
            if attempts == 2 && change_revision {
                revision.store(13, Ordering::Release);
            }
            if attempts == 2 && !change_revision {
                stop.store(true, Ordering::Release);
            }
            Ok(Some(model))
        });
        assert!(matches!(result, Err(AssetError::Cancelled)));
        assert_eq!(attempts, 2);
        assert_eq!(budget.used(), baseline);
    }
}
