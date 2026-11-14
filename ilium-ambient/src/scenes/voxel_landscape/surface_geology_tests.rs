//! B1-specific policy and candidate tests. Flat worlds below are explicitly synthetic.
use super::super::assets::budget::Cancel;
use super::super::surface_viewport::{
    visible_tiles, MAX_VIEWPORT_TILES, SOURCE_Z_MAX, SOURCE_Z_MIN,
};
use super::geology_public_tests::{
    bare_settings, centers, natural_fossil_witness, owned, square, TARGETS,
};
use super::*;
use std::{
    cell::Cell,
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use surface_geology::{
    desert_fossil, terrain_materials, ExposedFossil, FOSSIL_MAX_CELLS, FOSSIL_SOURCE,
};

fn copy_fossil(fossil: &ExposedFossil) -> ExposedFossil {
    ExposedFossil {
        anchor: fossil.anchor,
        minimum: fossil.minimum,
        maximum: fossil.maximum,
        cells: fossil.cells.clone(),
    }
}
fn flat_sample(height: i16) -> TerrainSample {
    let mut sample = TerrainFields::new(71839).sample(0, 0, false);
    sample.height = height;
    sample.uncarved_height = height;
    sample.water_level = None;
    sample.river = false;
    sample
}
fn flat_fossil(grid: [i32; 2]) -> Option<ExposedFossil> {
    let sample = flat_sample(80);
    desert_fossil(71839, grid, |_| (sample, SurfaceBiome::Desert), || false).unwrap()
}
fn flat_world(region: Region) -> SurfaceWorld {
    let mut world = SurfaceWorld {
        region,
        seed: 71839,
        columns: BTreeMap::new(),
        biomes: BTreeMap::new(),
        blocks: BTreeMap::new(),
        fluids: BTreeMap::new(),
        trees: Vec::new(),
        flora: Vec::new(),
        structures: Vec::new(),
        entities: Vec::new(),
        source_limitations: Vec::new(),
    };
    let sample = flat_sample(80);
    for y in region.minimum[1]..region.maximum[1] {
        for x in region.minimum[0]..region.maximum[0] {
            world.columns.insert([x, y], sample);
            world.biomes.insert([x, y], SurfaceBiome::Desert);
            world.blocks.insert(
                [x, y, 79],
                SurfaceBlock {
                    state: plain("minecraft:sand").unwrap(),
                    owner: SourceOwner::Terrain {
                        biome: SurfaceBiome::Desert,
                    },
                },
            );
        }
    }
    world
}
fn positions(fossil: &ExposedFossil) -> BTreeSet<[i32; 3]> {
    fossil
        .cells
        .iter()
        .map(|(p, _)| add_global(fossil.anchor, *p).unwrap())
        .collect()
}
fn assert_connected(points: &BTreeSet<[i32; 3]>) {
    let start = *points.first().expect("nonempty skeleton");
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(p) = queue.pop_front() {
        for axis in 0..3 {
            for step in [-1, 1] {
                let mut next = p;
                next[axis] += step;
                if points.contains(&next) && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
    }
    assert_eq!(&seen, points, "disconnected exposed bone fragment");
}
fn cross_fossil(edge: i32) -> ([i32; 2], ExposedFossil) {
    let middle = edge.div_euclid(64);
    // Sparse jitter/orientation may need more than 257 rows to truly cross.
    // Keep strict crossing and the actual global grid-to-candidate contract.
    for gy in middle - 4096..=middle + 4096 {
        for gx in middle - 1..=middle + 1 {
            let Some(fossil) = flat_fossil([gx, gy]) else {
                continue;
            };
            if fossil.minimum[0] < edge && fossil.maximum[0] > edge {
                return ([gx, gy], fossil);
            }
        }
    }
    panic!("bounded synthetic seam discovery failed at {edge}");
}

#[test]
fn actual_material_habitat_and_fauna_support_agree() {
    let settings = bare_settings(71839);
    let fields = TerrainFields::new(71839);
    let mut seen = BTreeSet::new();
    let mut checked_flora = BTreeSet::new();
    let mut witness = None;
    for (index, _) in TARGETS.iter().enumerate() {
        for &center in &centers(71839)[index] {
            let world = prepare(square(center, 16), &settings, || false).unwrap();
            let mut dry_soil = None;
            for (&xy, sample) in &world.columns {
                let biome = world.biomes[&xy];
                let feet = [xy[0], xy[1], i32::from(sample.height)];
                let ground = [xy[0], xy[1], feet[2] - 1];
                let Some(block) = world.blocks.get(&ground) else {
                    continue;
                };
                if block.owner != (SourceOwner::Terrain { biome }) {
                    continue;
                }
                let id = block.state.id().as_str();
                assert_eq!(id, terrain_materials(world.seed, biome, xy, ground[2]).top);
                let expected = match id {
                    "minecraft:sand" | "minecraft:red_sand" => HabitatCell::Sand,
                    "minecraft:grass_block"
                    | "minecraft:coarse_dirt"
                    | "minecraft:podzol"
                    | "minecraft:mycelium"
                    | "minecraft:mud" => HabitatCell::Soil,
                    _ => HabitatCell::Solid,
                };
                assert_eq!(habitat(&fields, &settings, ground), expected);
                let flooded = sample
                    .water_level
                    .is_some_and(|level| level > sample.height);
                assert_eq!(
                    habitat(&fields, &settings, feet),
                    if flooded {
                        HabitatCell::Water
                    } else {
                        HabitatCell::Air
                    }
                );
                assert_eq!(
                    dry_surface_support(&world, feet),
                    !flooded && !world.fluids.contains_key(&feet)
                );
                if !flooded && checked_flora.insert(id.to_owned()) {
                    let mut placement = FloraPlacement::new(4).unwrap();
                    let admitted = placement.admit_cancellable(
                        flower_candidate(feet, "minecraft:dandelion"),
                        |p| habitat(&fields, &settings, p),
                        || false,
                    );
                    assert_eq!(
                        admitted.is_ok(),
                        expected == HabitatCell::Soil,
                        "flora support on {id}"
                    );
                    assert_eq!(
                        placement.cells().count(),
                        usize::from(expected == HabitatCell::Soil)
                    );
                }
                seen.insert(id.to_owned());
                if expected == HabitatCell::Soil && !flooded {
                    dry_soil = Some(feet);
                }
            }
            if witness.is_none() {
                if let Some(feet) = dry_soil {
                    witness = Some((world, feet));
                }
            }
        }
    }
    for required in [
        "red_sand",
        "sand",
        "coarse_dirt",
        "podzol",
        "calcite",
        "gravel",
    ] {
        assert!(
            seen.contains(&format!("minecraft:{required}")),
            "missing actual support witness: {required}"
        );
    }
    let (mut world, feet) = witness.unwrap();
    let xy = [feet[0], feet[1]];
    let floor = [feet[0], feet[1], feet[2] - 1];
    let original = world.blocks[&floor].clone();
    for owner in [
        SourceOwner::Structure {
            anchor: feet,
            source: "fixture:protected".into(),
        },
        SourceOwner::TreeDecoration {
            anchor: feet,
            configuration: "minecraft:mega_pine",
        },
        SourceOwner::Saved {
            java_position: feet,
        },
        SourceOwner::Terrain {
            biome: SurfaceBiome::Desert,
        },
    ] {
        world.blocks.get_mut(&floor).unwrap().owner = owner;
        assert!(
            !dry_surface_support(&world, feet),
            "foreign support owner accepted"
        );
    }
    world.blocks.insert(floor, original.clone());
    let column = world.columns.remove(&xy).unwrap();
    assert!(!dry_surface_support(&world, feet));
    world.columns.insert(xy, column);
    world.columns.get_mut(&xy).unwrap().water_level = Some(column.height + 1);
    assert!(!dry_surface_support(&world, feet));
    world.columns.insert(xy, column);
    world.blocks.get_mut(&floor).unwrap().state = plain("minecraft:stone").unwrap();
    assert!(!dry_surface_support(&world, feet));
    world.blocks.insert(floor, original.clone());
    world.blocks.insert(feet, original);
    assert!(!dry_surface_support(
        &world,
        [feet[0], feet[1], feet[2] + 1]
    ));
    let model = surface_entities::model(
        Species::Cow,
        AtlasLayout::Bedrock,
        fauna_climate(SurfaceBiome::Desert),
    );
    let mut flat = flat_world(square([0, 0], 8));
    assert!(entity_clearance(&flat, [0, 0, 80], &model));
    flat.blocks.insert(
        [0, 0, 80],
        SurfaceBlock {
            state: plain("minecraft:bone_block").unwrap(),
            owner: SourceOwner::Geology {
                anchor: [0, 0, 80],
                source: FOSSIL_SOURCE,
            },
        },
    );
    assert!(!entity_clearance(&flat, [0, 0, 80], &model));
}

#[test]
fn signed_material_queries_keep_strata_and_unrelated_palettes() {
    for seed in [31, 71839] {
        for xy in [
            [-1, 0],
            [-17, -16],
            [1_000_000_000, -1_000_000_000],
            [-1_000_000_000, 1_000_000_000],
        ] {
            for biome in [
                SurfaceBiome::Badlands,
                SurfaceBiome::ErodedBadlands,
                SurfaceBiome::WoodedBadlands,
            ] {
                for ground in [80, 119, 180] {
                    for z in ground - 60..ground - 3 {
                        assert_eq!(
                            terrain_materials(seed, biome, xy, ground).at(z),
                            terrain_materials(seed, SurfaceBiome::Badlands, [0, 0], 240).at(z)
                        );
                    }
                }
            }
            for &biome in SurfaceBiome::all() {
                if matches!(
                    biome,
                    SurfaceBiome::Badlands
                        | SurfaceBiome::ErodedBadlands
                        | SurfaceBiome::WoodedBadlands
                        | SurfaceBiome::StonyPeaks
                        | SurfaceBiome::WindsweptGravellyHills
                        | SurfaceBiome::Forest
                        | SurfaceBiome::DappledForest
                        | SurfaceBiome::OldGrowthBirchForest
                        | SurfaceBiome::OldGrowthPineTaiga
                        | SurfaceBiome::OldGrowthSpruceTaiga
                        | SurfaceBiome::WindsweptForest
                        | SurfaceBiome::Taiga
                ) {
                    continue;
                }
                let (top, below) = match biome {
                    SurfaceBiome::Desert | SurfaceBiome::Beach => ("sand", "sandstone"),
                    SurfaceBiome::River => ("sand", "gravel"),
                    SurfaceBiome::FrozenRiver => ("snow_block", "gravel"),
                    SurfaceBiome::SnowyBeach => ("snow_block", "sand"),
                    SurfaceBiome::StonyShore => ("stone", "stone"),
                    SurfaceBiome::FrozenPeaks => ("snow_block", "packed_ice"),
                    SurfaceBiome::JaggedPeaks => ("snow_block", "stone"),
                    SurfaceBiome::MushroomFields => ("mycelium", "dirt"),
                    SurfaceBiome::Swamp | SurfaceBiome::MangroveSwamp => ("mud", "mud"),
                    SurfaceBiome::SnowyPlains
                    | SurfaceBiome::SnowySlopes
                    | SurfaceBiome::IceSpikes
                    | SurfaceBiome::SnowyTaiga
                    | SurfaceBiome::Grove => ("snow_block", "dirt"),
                    _ => ("grass_block", "dirt"),
                };
                let materials = terrain_materials(seed, biome, xy, 120);
                assert_eq!(materials.at(120), format!("minecraft:{top}"));
                assert_eq!(materials.at(117), format!("minecraft:{below}"));
                assert_eq!(materials.at(116), "minecraft:stone");
            }
        }
    }
}

#[test]
fn fossil_variants_are_connected_oriented_ribs_with_supported_feet() {
    let mut variants = BTreeSet::new();
    let mut silhouettes = BTreeSet::new();
    for gy in -16..=16 {
        for gx in -16..=16 {
            let Some(fossil) = flat_fossil([gx, gy]) else {
                continue;
            };
            assert!((30..=FOSSIL_MAX_CELLS).contains(&fossil.cells.len()));
            assert_connected(&positions(&fossil));
            let axes: BTreeSet<_> = fossil
                .cells
                .iter()
                .map(|(_, axis)| axis.java_value())
                .collect();
            assert_eq!(axes, BTreeSet::from(["x", "y", "z"]));
            let mut dimensions = [
                fossil.maximum[0] - fossil.minimum[0],
                fossil.maximum[1] - fossil.minimum[1],
            ];
            dimensions.sort();
            assert!(matches!(dimensions, [5 | 7, 9 | 13]));
            assert!(fossil
                .cells
                .iter()
                .all(|(p, _)| p.iter().all(|v| v.unsigned_abs() <= 16)));
            let footprint = (dimensions[0] * dimensions[1]) as usize;
            let occupied_xy: BTreeSet<_> = fossil.cells.iter().map(|(p, _)| [p[0], p[1]]).collect();
            assert!(
                occupied_xy.len() < footprint,
                "solid slab is not a rib silhouette"
            );
            silhouettes.insert(fossil.cells.iter().map(|(p, _)| *p).collect::<Vec<_>>());
            variants.insert(
                fossil
                    .cells
                    .iter()
                    .map(|(p, a)| (*p, a.java_value()))
                    .collect::<Vec<_>>(),
            );
        }
    }
    // Half-turns share silhouettes; the branch kernel preserves each rotated junction axis.
    assert_eq!(silhouettes.len(), 8);
    assert_eq!(variants.len(), 16);
    let (grid, flat) = cross_fossil(0);
    let base = flat_sample(80);
    let foot = flat.cells.iter().find(|(p, _)| p[2] == 0).unwrap().0;
    let foot = add_global(flat.anchor, foot).unwrap();
    let slope = |xy: [i32; 2]| {
        let mut s = base;
        s.height = if xy == foot[..2] { 80 } else { 81 };
        (s, SurfaceBiome::Desert)
    };
    let fitted = desert_fossil(71839, grid, slope, || false)
        .unwrap()
        .unwrap();
    assert_connected(&positions(&fitted));
    assert_eq!(fitted.anchor[2], flat.anchor[2] + 1);
    assert!(fitted.cells.iter().any(|(p, _)| p[2] == -1));
    for (p, _) in &fitted.cells {
        let global = add_global(fitted.anchor, *p).unwrap();
        assert!(global[2] >= i32::from(slope([global[0], global[1]]).0.height));
        if p[2] <= 0 && !positions(&fitted).contains(&[global[0], global[1], global[2] - 1]) {
            assert_eq!(global[2], i32::from(slope([global[0], global[1]]).0.height));
        }
    }
}

#[test]
fn whole_fossil_footprint_rejects_wet_wrong_biome_relief_and_cancellation() {
    let (grid, fossil) = cross_fossil(0);
    let occupied: BTreeSet<_> = positions(&fossil).iter().map(|p| [p[0], p[1]]).collect();
    let gap = (fossil.minimum[1]..fossil.maximum[1])
        .flat_map(|y| (fossil.minimum[0]..fossil.maximum[0]).map(move |x| [x, y]))
        .find(|xy| !occupied.contains(xy))
        .unwrap();
    let base = flat_sample(80);
    for case in 0..3 {
        let candidate = desert_fossil(
            71839,
            grid,
            |xy| {
                let mut sample = base;
                let mut biome = SurfaceBiome::Desert;
                if xy == gap {
                    match case {
                        0 => sample.water_level = Some(81),
                        1 => biome = SurfaceBiome::Beach,
                        _ => sample.height = 82,
                    }
                }
                (sample, biome)
            },
            || false,
        )
        .unwrap();
        assert!(
            candidate.is_none(),
            "whole footprint gap admitted case={case}"
        );
    }
    let polls = Cell::new(0);
    desert_fossil(
        71839,
        grid,
        |_| (base, SurfaceBiome::Desert),
        || {
            polls.set(polls.get() + 1);
            false
        },
    )
    .unwrap()
    .unwrap();
    for cutoff in 1..=polls.get() {
        let current = Cell::new(0);
        assert!(matches!(
            desert_fossil(
                71839,
                grid,
                |_| (base, SurfaceBiome::Desert),
                || {
                    current.set(current.get() + 1);
                    current.get() == cutoff
                }
            ),
            Err(AssetError::Cancelled)
        ));
    }
    for axis in 0..2 {
        for extreme in [i32::MIN, i32::MAX] {
            let grid = (0..256)
                .map(|other| {
                    if axis == 0 {
                        [extreme, other]
                    } else {
                        [other, extreme]
                    }
                })
                .find(|g| {
                    hash2(
                        71839 ^ 0x666f_7373_696c_7631,
                        i64::from(g[0]),
                        i64::from(g[1]),
                    )
                    .is_multiple_of(8)
                })
                .unwrap();
            let sampled = Cell::new(0);
            assert!(desert_fossil(
                71839,
                grid,
                |_| {
                    sampled.set(sampled.get() + 1);
                    (base, SurfaceBiome::Desert)
                },
                || false
            )
            .unwrap()
            .is_none());
            assert_eq!(
                sampled.get(),
                0,
                "overflowing owner reached terrain sampling"
            );
        }
    }
    let mut too_high = base;
    too_high.height = SOURCE_Z_MAX as i16;
    assert!(
        desert_fossil(71839, grid, |_| (too_high, SurfaceBiome::Desert), || false)
            .unwrap()
            .is_none()
    );
}

#[test]
fn projection_refusals_and_every_cancellation_checkpoint_write_nothing() {
    let (_, fossil) = cross_fossil(0);
    let region = square([fossil.anchor[0], fossil.anchor[1]], 12);
    let last = *positions(&fossil).last().unwrap();
    for collision in 0..3 {
        let mut world = flat_world(region);
        let mut reserved = BTreeSet::new();
        if collision == 0 {
            reserved.insert([fossil.minimum[0], fossil.minimum[1], 0]);
        } else if collision == 1 {
            world.blocks.insert(
                last,
                SurfaceBlock {
                    state: plain("minecraft:stone").unwrap(),
                    owner: SourceOwner::Structure {
                        anchor: last,
                        source: "fixture:protected".into(),
                    },
                },
            );
        } else {
            world.fluids.insert(
                last,
                FluidCell::new(0, water_tint(SurfaceBiome::Desert)).unwrap(),
            );
        }
        let before = owned(&world, region);
        let ledger = reserved.clone();
        let fluids = world.fluids.clone();
        assert!(
            !project_fossil(&mut world, &mut reserved, copy_fossil(&fossil), &|| false).unwrap()
        );
        assert_eq!(owned(&world, region), before);
        assert_eq!(reserved, ledger);
        assert_eq!(world.fluids, fluids);
    }
    let mut world = flat_world(region);
    let polls = Cell::new(0);
    project_fossil(
        &mut world,
        &mut BTreeSet::new(),
        copy_fossil(&fossil),
        &|| {
            polls.set(polls.get() + 1);
            false
        },
    )
    .unwrap();
    for cutoff in 1..=polls.get() {
        let mut world = flat_world(region);
        let before = owned(&world, region);
        let mut reserved = BTreeSet::new();
        let current = Cell::new(0);
        assert!(matches!(
            project_fossil(&mut world, &mut reserved, copy_fossil(&fossil), &|| {
                current.set(current.get() + 1);
                current.get() == cutoff
            }),
            Err(AssetError::Cancelled)
        ));
        assert_eq!(owned(&world, region), before);
        assert!(reserved.is_empty());
    }
    let mut world = flat_world(region);
    let before = owned(&world, region);
    let mut invalid = copy_fossil(&fossil);
    invalid.cells.resize(FOSSIL_MAX_CELLS + 1, fossil.cells[0]);
    let mut reserved = BTreeSet::new();
    assert!(project_fossil(&mut world, &mut reserved, invalid, &|| false).is_err());
    assert_eq!(owned(&world, region), before);
    assert!(reserved.is_empty());
}

#[test]
fn whole_structure_query_rejects_a_synthetic_overlap_with_a_real_well() {
    let settings = VoxelLandscapeSettings {
        seed: 71839,
        ..Default::default()
    };
    let fields = TerrainFields::new(71839);
    let anchor = [-2374_i32, -12665_i32, 94];
    let well = surface_landmark_assembly::desert_well_candidate(
        [anchor[0].div_euclid(16), anchor[1].div_euclid(16)],
        &fields,
        &settings,
        || false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(well.anchor, anchor);
    let (_, mut fossil) = cross_fossil(0);
    let old = fossil.anchor;
    fossil.anchor = anchor;
    for axis in 0..2 {
        fossil.minimum[axis] += anchor[axis] - old[axis];
        fossil.maximum[axis] += anchor[axis] - old[axis];
    }
    assert!(well.writes.keys().any(|p| fossil.covers(*p)));
    assert!(!fossil_clear_of_structures(&fossil, &fields, &settings, &|| false).unwrap());
    assert!(matches!(
        fossil_clear_of_structures(&fossil, &fields, &settings, &|| true),
        Err(AssetError::Cancelled)
    ));
}

#[test]
fn synthetic_fossils_cross_signed_split_shifted_and_tile_seams() {
    for edge in [0, -16, 1_000_000_000, -1_000_000_000] {
        let (_, fossil) = cross_fossil(edge);
        let region = square([fossil.anchor[0], fossil.anchor[1]], 12);
        let mut whole = flat_world(region);
        let mut full_reserved = BTreeSet::new();
        assert!(project_fossil(
            &mut whole,
            &mut full_reserved,
            copy_fossil(&fossil),
            &|| false
        )
        .unwrap());
        let pieces = [
            Region {
                minimum: region.minimum,
                maximum: [edge, region.maximum[1]],
            },
            Region {
                minimum: [edge, region.minimum[1]],
                maximum: region.maximum,
            },
        ];
        for reverse in [false, true] {
            let mut joined = BTreeMap::new();
            for index in if reverse { [1, 0] } else { [0, 1] } {
                let part = pieces[index];
                let mut world = flat_world(part);
                let mut reserved = BTreeSet::new();
                assert!(
                    project_fossil(&mut world, &mut reserved, copy_fossil(&fossil), &|| false)
                        .unwrap()
                );
                assert_eq!(reserved, full_reserved, "off-window reservation lost");
                joined.extend(owned(&world, part));
            }
            assert_eq!(joined, owned(&whole, region));
        }
        let shifted = Region {
            minimum: [region.minimum[0] + 1, region.minimum[1] + 1],
            maximum: region.maximum.map(|v| v + 1),
        };
        let overlap = Region {
            minimum: shifted.minimum,
            maximum: region.maximum,
        };
        let mut other = flat_world(shifted);
        project_fossil(
            &mut other,
            &mut BTreeSet::new(),
            copy_fossil(&fossil),
            &|| false,
        )
        .unwrap();
        assert_eq!(owned(&other, overlap), owned(&whole, overlap));
        let tiled_region = Region {
            minimum: [edge - 96, fossil.anchor[1] - 95],
            maximum: [edge + 32, fossil.anchor[1] + 33],
        };
        let mut tiled_whole = flat_world(tiled_region);
        project_fossil(
            &mut tiled_whole,
            &mut BTreeSet::new(),
            copy_fossil(&fossil),
            &|| false,
        )
        .unwrap();
        let tiles = visible_tiles(tiled_region, 2.8, [4096, 4096]).unwrap();
        assert!(!tiles.is_empty() && tiles.len() <= MAX_VIEWPORT_TILES);
        assert!(tiles.len() > 1);
        for reverse in [false, true] {
            let mut stitched = BTreeMap::new();
            for offset in 0..tiles.len() {
                let tile_index = if reverse {
                    tiles.len() - offset - 1
                } else {
                    offset
                };
                let (core, expanded) = tiles[tile_index];
                let mut tile = flat_world(expanded);
                project_fossil(
                    &mut tile,
                    &mut BTreeSet::new(),
                    copy_fossil(&fossil),
                    &|| false,
                )
                .unwrap();
                stitched.extend(owned(&tile, core));
            }
            assert_eq!(stitched, owned(&tiled_whole, tiled_region));
        }
        let mut left = flat_world(pieces[0]);
        let before = owned(&left, pieces[0]);
        let blocked = *positions(&fossil)
            .iter()
            .find(|p| !pieces[0].contains(**p))
            .unwrap();
        assert!(!project_fossil(
            &mut left,
            &mut BTreeSet::from([blocked]),
            copy_fossil(&fossil),
            &|| false
        )
        .unwrap());
        assert_eq!(owned(&left, pieces[0]), before);
    }
}

#[test]
fn natural_fossil_equals_whole_candidate_and_survives_projection_and_vegetation() {
    let (seed, grid, xy) = natural_fossil_witness();
    let mut settings = bare_settings(seed);
    let fields = TerrainFields::new(u64::from(seed));
    let fossil = desert_fossil(
        u64::from(seed),
        grid,
        |p| sample_ground(&fields, &settings, p[0], p[1]),
        || false,
    )
    .unwrap()
    .unwrap();
    assert!(fossil_clear_of_structures(&fossil, &fields, &settings, &|| false).unwrap());
    let expected: BTreeMap<_, _> = fossil
        .cells
        .iter()
        .map(|(p, axis)| {
            (
                add_global(fossil.anchor, *p).unwrap(),
                (
                    state(
                        "minecraft:bone_block",
                        [("axis".into(), axis.java_value().into())],
                    )
                    .unwrap(),
                    SourceOwner::Geology {
                        anchor: fossil.anchor,
                        source: FOSSIL_SOURCE,
                    },
                ),
            )
        })
        .collect();
    assert_connected(&expected.keys().copied().collect());
    for y in fossil.minimum[1]..fossil.maximum[1] {
        for x in fossil.minimum[0]..fossil.maximum[0] {
            let (sample, biome) = sample_ground(&fields, &settings, x, y);
            assert_eq!(biome, SurfaceBiome::Desert);
            assert!(sample.water_level.is_none());
            assert!((fossil.anchor[2] - i32::from(sample.height)).abs() <= 1);
        }
    }
    for vegetation in [0, 100] {
        settings.vegetation_percent = vegetation;
        let region = square(xy, 24);
        let whole = prepare(region, &settings, || false).unwrap();
        let actual: BTreeMap<_, _> = owned(&whole, region)
            .into_iter()
            .filter(|(_, (_, owner))| {
                *owner
                    == (SourceOwner::Geology {
                        anchor: fossil.anchor,
                        source: FOSSIL_SOURCE,
                    })
            })
            .collect();
        assert_eq!(actual, expected);
        for edge in [xy[0] - 1, xy[0] + 1] {
            let pieces = [
                Region {
                    minimum: region.minimum,
                    maximum: [edge, region.maximum[1]],
                },
                Region {
                    minimum: [edge, region.minimum[1]],
                    maximum: region.maximum,
                },
            ];
            for reverse in [false, true] {
                let mut joined = BTreeMap::new();
                for index in if reverse { [1, 0] } else { [0, 1] } {
                    let part = prepare(pieces[index], &settings, || false).unwrap();
                    joined.extend(owned(&part, pieces[index]));
                }
                assert_eq!(joined, owned(&whole, region));
            }
        }
        validate_generated_height(&whole).unwrap();
        assert!(whole
            .blocks
            .keys()
            .all(|p| (SOURCE_Z_MIN..SOURCE_Z_MAX).contains(&f64::from(p[2]))));
    }
    settings.vegetation_percent = 0;
    let tiled_region = Region {
        minimum: xy.map(|v| v - 95),
        maximum: xy.map(|v| v + 33),
    };
    let tiles = visible_tiles(tiled_region, 2.8, [4096, 4096]).unwrap();
    assert!(!tiles.is_empty() && tiles.len() <= MAX_VIEWPORT_TILES);
    assert!(tiles.len() > 1);
    assert!(fossil.minimum[0] < xy[0] + 1 && fossil.maximum[0] > xy[0] + 1);
    assert!(fossil.minimum[1] < xy[1] + 1 && fossil.maximum[1] > xy[1] + 1);
    let whole = prepare(tiled_region, &settings, || false).unwrap();
    let whole_owned = owned(&whole, tiled_region);
    for reverse in [false, true] {
        let mut stitched = BTreeMap::new();
        for offset in 0..tiles.len() {
            let tile_index = if reverse {
                tiles.len() - offset - 1
            } else {
                offset
            };
            let (core, expanded) = tiles[tile_index];
            let tile = prepare(expanded, &settings, || false).unwrap();
            stitched.extend(owned(&tile, core));
        }
        assert_eq!(stitched, whole_owned);
    }
    let viewport_world =
        prepare_viewport(tiled_region, 2.8, [4096, 4096], &settings, || false).unwrap();
    assert_eq!(owned(&viewport_world, tiled_region), whole_owned);
    assert_eq!(viewport_world.fluids, whole.fluids);
}

#[test]
fn generated_signed_windows_and_stale_cancelled_requests_publish_consistently() {
    for center in [
        [0, 0],
        [-16, -16],
        [1_000_000_000, 1_000_000_000],
        [-1_000_000_000, -1_000_000_000],
    ] {
        let settings = bare_settings(71839);
        let region = square(center, 6);
        let whole = prepare(region, &settings, || false).unwrap();
        let shifted = square([center[0] + 1, center[1] - 1], 6);
        let overlap = Region {
            minimum: [shifted.minimum[0], region.minimum[1]],
            maximum: [region.maximum[0], shifted.maximum[1]],
        };
        let other = prepare(shifted, &settings, || false).unwrap();
        assert_eq!(owned(&whole, overlap), owned(&other, overlap));
        let retry = prepare(region, &settings, || false).unwrap();
        assert_eq!(owned(&whole, region), owned(&retry, region));
    }
    let (seed, _, xy) = natural_fossil_witness();
    let settings = bare_settings(seed);
    let region = square(xy, 12);
    let polls = Cell::new(0);
    let complete = prepare(region, &settings, || {
        polls.set(polls.get() + 1);
        false
    })
    .unwrap();
    for cutoff in [1, polls.get() / 2, polls.get()] {
        let count = Cell::new(0);
        assert!(matches!(
            prepare(region, &settings, || {
                count.set(count.get() + 1);
                count.get() >= cutoff
            }),
            Err(AssetError::Cancelled)
        ));
    }
    let stop = AtomicBool::new(false);
    let revision = AtomicU64::new(7);
    let cancel = Cancel::for_revision(&stop, &revision, 7);
    let count = Cell::new(0);
    assert!(matches!(
        prepare(region, &settings, || {
            count.set(count.get() + 1);
            if count.get() == polls.get() / 2 {
                revision.store(8, Ordering::Release);
            }
            cancel.is_cancelled()
        }),
        Err(AssetError::Cancelled)
    ));
    assert!(matches!(
        prepare(region, &settings, || cancel.is_cancelled()),
        Err(AssetError::Cancelled)
    ));
    let retry = prepare(region, &settings, || false).unwrap();
    assert_eq!(owned(&retry, region), owned(&complete, region));
}
