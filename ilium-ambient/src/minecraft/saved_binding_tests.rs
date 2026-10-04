//! Real decoded/geometry synthetic controls; no authored pack or world data.
use super::*;
use crate::minecraft::{
    chunk, evidence, loader,
    nbt::{Compound, Document, Tag, Text},
    surface, tours,
};

fn fields(values: Vec<(&str, Tag)>) -> Compound {
    values
        .into_iter()
        .map(|(name, tag)| (Text::from(name), tag))
        .collect()
}
pub(crate) fn cells(name: &str, properties: &[(&str, &str)]) -> RenderCells {
    let mut state = fields(vec![("Name", Tag::String(name.into()))]);
    if !properties.is_empty() {
        state.insert(
            "Properties".into(),
            Tag::Compound(
                properties
                    .iter()
                    .map(|(key, value)| (Text::from(*key), Tag::String((*value).into())))
                    .collect(),
            ),
        );
    }
    let sections = (-4..=19)
        .map(|y| {
            Tag::Compound(fields(vec![
                ("Y", Tag::Byte(y)),
                (
                    "block_states",
                    Tag::Compound(fields(vec![(
                        "palette",
                        Tag::List {
                            kind: 10,
                            values: vec![Tag::Compound(state.clone())],
                        },
                    )])),
                ),
            ]))
        })
        .collect();
    let decoded = chunk::decode(
        &Document {
            name: "Synthetic binding fixture".into(),
            root: fields(vec![
                ("DataVersion", Tag::Int(3218)),
                ("xPos", Tag::Int(-1)),
                ("zPos", Tag::Int(2)),
                ("Status", Tag::String("full".into())),
                (
                    "sections",
                    Tag::List {
                        kind: 10,
                        values: sections,
                    },
                ),
            ]),
        },
        [-1, 2],
        chunk::Limits::default(),
        &|| false,
    )
    .unwrap();
    let mut loaded = loader::LoadedWindow::default();
    loaded.chunks.insert([-1, 2], Arc::new(decoded));
    loaded.coverage.chunks.insert([-1, 2]);
    let map = Arc::new(
        PreparedMap::new(
            evidence::Source {
                map: evidence::MapId([9; 16]),
                generation: 2,
            },
            0,
            Arc::new(loaded),
            Vec::new(),
            &mut tours::Budget::new(u64::MAX, &|| false),
        )
        .unwrap(),
    );
    RenderCells {
        map,
        core: surface::Bounds {
            minimum: [-16, 32],
            maximum: [-1, 47],
        },
        heights: [318, 319],
        positions: vec![[-16, 319, 32], [-1, 318, 47]],
        work_used: 0,
        storage_charge: 0,
    }
}
#[test]
fn exact_states_axis_owner_map_and_exclusive_region_survive_without_generated_metadata() {
    let input = cells(
        "minecraft:oak_log",
        &[("axis", "x"), ("waterlogged", "false")],
    );
    let original = input.map.state(input.positions[0]).unwrap();
    let result = prepare(&input, Limits::default(), &|| false).unwrap();
    assert!(Arc::ptr_eq(&result.map, &input.map));
    assert_eq!(result.world.region.minimum, [-16, 32]);
    assert_eq!(result.world.region.maximum, [0, 48]);
    assert_eq!(result.heights, [318, 319]);
    assert_eq!(result.world.blocks.len(), 2);
    for java_position in &input.positions {
        let renderer = [java_position[0], java_position[2], java_position[1]];
        let block = &result.world.blocks[&renderer];
        assert_eq!(block.state.id().as_str(), original.name);
        assert_eq!(block.state.properties(), &original.properties);
        assert!(
            matches!(block.owner,crate::voxel_landscape::surface_generation::SourceOwner::Saved{java_position:actual} if actual==*java_position)
        );
    }
    assert!(
        result.world.columns.is_empty()
            && result.world.biomes.is_empty()
            && result.world.trees.is_empty()
            && result.world.flora.is_empty()
            && result.world.structures.is_empty()
            && result.world.entities.is_empty()
            && result.world.fluids.is_empty()
    );
    assert!(result.storage_charge <= Limits::default().owned_bytes);
    assert_eq!(input.positions.len(), 2);
}
#[test]
fn exact_liquid_and_waterlogged_states_retain_raw_block_and_separate_fluid_cell() {
    for (name, properties, amount, falling, waterlogged) in [
        ("minecraft:water", vec![("level", "0")], 8, false, false),
        ("minecraft:water", vec![("level", "15")], 8, true, false),
        ("minecraft:lava", vec![("level", "8")], 8, true, false),
        (
            "minecraft:bubble_column",
            vec![("drag", "true")],
            8,
            false,
            false,
        ),
        (
            "minecraft:glow_lichen",
            vec![("north", "true"), ("waterlogged", "true")],
            8,
            false,
            true,
        ),
    ] {
        let input = cells(name, &properties);
        let result = prepare(&input, Limits::default(), &|| false).unwrap();
        assert_eq!(result.world.blocks.len(), input.positions.len());
        assert_eq!(result.liquid_cells.len(), input.positions.len());
        for java_position in &input.positions {
            let renderer = [java_position[0], java_position[2], java_position[1]];
            let raw = input.map.state(*java_position).unwrap();
            let retained = &result.world.blocks[&renderer].state;
            assert_eq!(retained.id().as_str(), name);
            assert_eq!(retained.properties(), &raw.properties);
            let liquid = &result.liquid_cells[java_position];
            assert_eq!(liquid.java_position, *java_position);
            assert_eq!(liquid.amount, amount);
            assert_eq!(liquid.falling, falling);
            assert_eq!(liquid.waterlogged, waterlogged);
        }
        assert_eq!(input.map.state(input.positions[0]).unwrap().name, name);
        assert!(Arc::ptr_eq(&result.map, &input.map));
    }
}

#[test]
fn uncaptured_waterlogged_behavior_and_invalid_levels_fail_before_publication() {
    for (name, properties) in [
        (
            "minecraft:oak_slab",
            vec![("type", "bottom"), ("waterlogged", "true")],
        ),
        ("minecraft:water", vec![("level", "16")]),
    ] {
        let input = cells(name, &properties);
        assert!(matches!(
            prepare(&input, Limits::default(), &|| false),
            Err(Error::Fluid(_))
        ));
        assert_eq!(Arc::strong_count(&input.map), 1);
    }
}
#[test]
fn malformed_names_properties_duplicates_missing_cells_and_out_of_band_positions_fail() {
    for (name, properties) in [
        ("minecraft:STONE", vec![]),
        ("minecraft:stone//bad", vec![]),
        ("minecraft:stone", vec![("BadKey", "true")]),
    ] {
        assert!(prepare(&cells(name, &properties), Limits::default(), &|| false).is_err());
    }
    let mut input = cells("minecraft:stone", &[]);
    input.positions.push(input.positions[0]);
    assert!(prepare(&input, Limits::default(), &|| false).is_err());
    input.positions = vec![[0, 319, 32]];
    assert!(prepare(&input, Limits::default(), &|| false).is_err());
    input.positions = vec![[-16, 317, 32]];
    assert!(prepare(&input, Limits::default(), &|| false).is_err());
}
#[test]
fn copy_work_cell_caps_and_cancel_reject_before_whole_output() {
    let input = cells("minecraft:stone", &[]);
    for limits in [
        Limits {
            cells: 1,
            ..Limits::default()
        },
        Limits {
            owned_bytes: 128,
            ..Limits::default()
        },
        Limits {
            work_units: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            prepare(&input, limits, &|| false),
            Err(Error::Limit(_)) | Err(Error::Invalid(_))
        ));
        assert_eq!(Arc::strong_count(&input.map), 1);
    }
    assert!(matches!(
        prepare(&input, Limits::default(), &|| true),
        Err(Error::Cancelled)
    ));
}

#[test]
fn overflowed_exclusive_region_unknown_saved_cell_and_mid_attempt_cancel_do_not_publish() {
    let mut input = cells("minecraft:stone", &[]);
    input.core.maximum[0] = i32::MAX;
    assert!(matches!(
        prepare(&input, Limits::default(), &|| false),
        Err(Error::Invalid(_))
    ));
    input.core.maximum[0] = 15;
    input.positions = vec![[0, 319, 32]];
    assert!(input.map.state(input.positions[0]).is_none());
    assert!(matches!(
        prepare(&input, Limits::default(), &|| false),
        Err(Error::Invalid(_))
    ));
    let input = cells("minecraft:stone", &[]);
    let calls = std::cell::Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 3
    };
    assert!(matches!(
        prepare(&input, Limits::default(), &cancel),
        Err(Error::Cancelled)
    ));
    assert_eq!(Arc::strong_count(&input.map), 1);
    assert_eq!(input.positions.len(), 2);
}

#[test]
fn admitted_121_chunk_default_band_has_separate_measured_state_copy_charge() {
    let fixture = cells("minecraft:stone", &[]);
    let template = fixture.map.loaded().chunks.values().next().unwrap();
    let mut loaded = loader::LoadedWindow::default();
    for x in -5..=5 {
        for z in -5..=5 {
            let mut decoded = (**template).clone();
            decoded.identity.position = [x, z];
            loaded.chunks.insert([x, z], Arc::new(decoded));
            loaded.coverage.chunks.insert([x, z]);
        }
    }
    let map = Arc::new(
        PreparedMap::new(
            fixture.map.source(),
            0,
            Arc::new(loaded),
            Vec::new(),
            &mut tours::Budget::new(u64::MAX, &|| false),
        )
        .unwrap(),
    );
    let input = crate::minecraft::render_cells::prepare(
        map,
        crate::minecraft::render_cells::Limits::default(),
        &|| false,
    )
    .unwrap();
    assert_eq!(input.positions.len(), 121 * 256 * 25);
    let bound = prepare(&input, Limits::default(), &|| false).unwrap();
    assert_eq!(bound.world.blocks.len(), input.positions.len());
    assert!(bound.storage_charge > 16 << 20);
    assert!(bound.storage_charge <= 256 << 20);
    assert!(input.storage_charge <= 16 << 20);
    println!(
        "{}",
        serde_json::json!({"type":"result","fixture":"synthetic uniform stone, actual decoded/admitted 121 chunk depth24 window","cells":input.positions.len(),"position_bytes":input.storage_charge,"state_copy_bytes":bound.storage_charge,"state_copy_work":bound.work_used})
    );
}

use crate::voxel_landscape::{
    assets::{
        animation::{AnimationPlan, MissingAnimation},
        bank::TextureBankBuilder,
        block_state::BlockState,
        budget::{ByteBudget, Cancel},
        compatibility::DefinitionSet,
        identity::{AssetPath, BlobOrigin, Label, OriginKind, ResourceId, SourceBlob},
        layers::{ResourceKey, ResourceKind},
        models::ModelCompiler,
        pixels::{ImageExpectations, PixelImage},
        review::{AssetPhase, FullPackReview, PackScope, SourceEdition},
        texture::{Encoding, Texture},
    },
    surface_fluid::{FluidCell, FluidMesh},
    surface_mesh::{
        AlphaMode, BoundModel, MaterialTable, MeshRegion, PreparedMesh, TextureRenderRule,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::AtomicBool,
};
fn geometry_fixture() -> (PreparedMesh, FluidMesh, ByteBudget) {
    // The real model compiler reserves its configured 64 MiB working ceiling
    // alongside the texture/definition receipts; budget both in this fixture.
    let budget = ByteBudget::new(128 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let cancel = Cancel::new(&stop);
    let asset_limits = crate::voxel_landscape::assets::Limits::default();
    let label = |value| Label::new(value).unwrap();
    let review = FullPackReview {
        pack: ResourceId::parse("synthetic:geometry").unwrap(),
        release: label("Synthetic geometry measurement test"),
        edition: SourceEdition::ArtistManifest,
        scope: PackScope::FullWorld,
        phase: AssetPhase::PrivateTestPlaceholder,
        evidence: label("Synthetic fixture only, never native acceptance"),
        author_credit: label("Original test data"),
        license_record: label("Synthetic fixture"),
        restrictions: vec![],
        known_missing: vec![],
    };
    let origin = BlobOrigin {
        pack: review.pack.clone(),
        release: review.release.clone(),
        layer: label("Synthetic geometry layer"),
        path: AssetPath::parse("synthetic.tga").unwrap(),
        review_digest: review.digest().unwrap(),
        kind: OriginKind::DiagnosticFixture,
    };
    let mut tga = vec![0u8; 18];
    tga[2] = 2;
    tga[12] = 1;
    tga[14] = 1;
    tga[16] = 32;
    tga[17] = 0x28;
    tga.extend([255, 255, 255, 255]);
    let blob = SourceBlob::new(tga, origin.clone(), None, &asset_limits, &budget, cancel).unwrap();
    let image = Arc::new(
        PixelImage::decode_tga(
            &blob,
            ImageExpectations::default(),
            &asset_limits,
            &budget,
            cancel,
        )
        .unwrap(),
    );
    let animation = AnimationPlan::build(
        [1, 1],
        None,
        &MissingAnimation::StaticImage,
        &asset_limits,
        &budget,
        cancel,
    )
    .unwrap();
    let texture =
        Arc::new(Texture::new(image, animation, Encoding::SrgbColor, &budget, cancel).unwrap());
    let texture_id = ResourceId::parse("synthetic:block/material").unwrap();
    let mut builder =
        TextureBankBuilder::new(review, vec![], asset_limits, budget.clone(), cancel).unwrap();
    builder.insert(texture_id.clone(), texture, cancel).unwrap();
    let bank = builder.finish(Vec::new(), cancel).unwrap();
    let mut definitions = DefinitionSet::new(origin, asset_limits, budget.clone()).unwrap();
    let mut faces = serde_json::Map::new();
    for direction in ["down", "up", "north", "south", "west", "east"] {
        faces.insert(
            direction.into(),
            serde_json::json!({"texture":"synthetic:block/material"}),
        );
    }
    definitions
        .insert_model(
            "synthetic:block/overhang",
            &serde_json::json!({"elements":[{"from":[-8,-4,-16],"to":[24,32,20],"faces":faces}]}),
            label("Synthetic overhanging cuboid"),
            cancel,
        )
        .unwrap();
    definitions
        .insert(
            ResourceKey {
                kind: ResourceKind::Blockstate,
                id: ResourceId::parse("synthetic:stone").unwrap(),
            },
            &serde_json::json!({"variants":{"":{"model":"synthetic:block/overhang"}}}),
            label("Synthetic exact variant"),
            cancel,
        )
        .unwrap();
    let state = BlockState::new(
        ResourceId::parse("synthetic:stone").unwrap(),
        Vec::<(String, String)>::new(),
    )
    .unwrap();
    let mut compiler = ModelCompiler::new(&definitions, asset_limits, budget.clone()).unwrap();
    let normalized = compiler
        .compile_state(&state, [-16, 32, 319], 0, cancel)
        .unwrap();
    let policy = MaterialTable {
        medium: None,
        rules: BTreeMap::from([(
            texture_id.clone(),
            TextureRenderRule {
                alpha: AlphaMode::Opaque,
                layer: 0,
                normal_map: None,
                specular_map: None,
            },
        )]),
        tints: BTreeMap::new(),
    };
    let model = Arc::new(BoundModel::bind(&normalized, &bank, &policy, &budget, cancel).unwrap());
    let region = MeshRegion {
        minimum: [-16, 32],
        maximum: [0, 48],
    };
    let mesh = PreparedMesh::build(
        &BTreeMap::from([([-16, 32, 319], model)]),
        region,
        bank.identity(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    let fluid = FluidMesh::build(
        &BTreeMap::from([([-1, 47, 317], FluidCell::new(0, [0.2, 0.3, 0.4]).unwrap())]),
        &BTreeSet::new(),
        region,
        &bank,
        bank.resolve(&texture_id).unwrap(),
        100,
        &budget,
        cancel,
    )
    .unwrap();
    (mesh, fluid, budget)
}

#[test]
fn compiled_model_and_fluid_vertices_provide_actual_java_height_and_ground_framing() {
    let (mesh, fluid, _budget) = geometry_fixture();
    let result = measure(&mesh, Some(&fluid), Limits::default(), &|| false).unwrap();
    assert_eq!(result.ground_bounds, [[-16.5, 31.0], [0.0, 48.0]]);
    assert_eq!(result.vertical_bounds, [317.0, 321.0]);
    assert_eq!(result.model_overhang, 1.0);
    assert_eq!(result.faces, 11);
    let framing = result.framing([160, 80], 4.0, 319.0, 1.0).unwrap();
    assert_eq!(framing.vertical_bounds, [317.0, 321.0]);
    assert_eq!(framing.horizontal_halo, 2.0);
    assert!(framing.radius().unwrap() > 2.0);
}
#[test]
fn every_exposed_model_and_fluid_vertex_is_admitted_and_nonfinite_or_unknown_quads_fail() {
    let (mut mesh, mut fluid, _budget) = geometry_fixture();
    fluid.faces.last_mut().unwrap().quad.points[3][2] = 10.0;
    let result = measure(&mesh, Some(&fluid), Limits::default(), &|| false).unwrap();
    assert_eq!(result.vertical_bounds[1], 327.0);
    fluid.faces.last_mut().unwrap().quad.points[3][0] = f64::NAN;
    assert!(matches!(
        measure(&mesh, Some(&fluid), Limits::default(), &|| false),
        Err(Error::Geometry(_))
    ));
    mesh.faces[0].quad_index = u16::MAX;
    assert!(matches!(
        measure(&mesh, None, Limits::default(), &|| false),
        Err(Error::Geometry(_))
    ));
}
#[test]
fn measured_geometry_caps_cancel_empty_and_invalid_neighbor_halo_are_typed() {
    let (mut mesh, fluid, _budget) = geometry_fixture();
    assert!(matches!(
        measure(
            &mesh,
            Some(&fluid),
            Limits {
                faces: 10,
                ..Limits::default()
            },
            &|| false
        ),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        measure(
            &mesh,
            Some(&fluid),
            Limits {
                work_units: 2,
                ..Limits::default()
            },
            &|| false
        ),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        measure(&mesh, Some(&fluid), Limits::default(), &|| true),
        Err(Error::Cancelled)
    ));
    let measured = measure(&mesh, None, Limits::default(), &|| false).unwrap();
    assert!(measured.framing([160, 80], 4.0, 319.0, -1.0).is_err());
    mesh.faces.clear();
    assert!(matches!(
        measure(&mesh, None, Limits::default(), &|| false),
        Err(Error::EmptyGeometry)
    ));
}
