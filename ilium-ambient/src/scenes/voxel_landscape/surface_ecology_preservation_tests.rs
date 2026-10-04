use super::*;
#[test]
fn ecology_every_biome_source_and_all_132_growth_pairs_remain_reachable() {
    let azalea_biomes = [
        SurfaceBiome::BambooJungle,
        SurfaceBiome::BirchForest,
        SurfaceBiome::DappledForest,
        SurfaceBiome::DarkForest,
        SurfaceBiome::FlowerForest,
        SurfaceBiome::Forest,
        SurfaceBiome::Jungle,
        SurfaceBiome::OldGrowthBirchForest,
        SurfaceBiome::SparseJungle,
    ];
    let expected_profiles: BTreeSet<_> = tree_profiles::TREE_PROFILES
        .iter()
        .map(|profile| profile.id)
        .collect();
    assert_eq!(expected_profiles.len(), 44);
    let fallen_ids: BTreeSet<_> = tree_profiles::TREE_PROFILES
        .iter()
        .filter(|profile| profile.shape == TreeShape::Fallen)
        .map(|profile| profile.id)
        .collect();
    assert_eq!(
        fallen_ids,
        BTreeSet::from([
            "minecraft:fallen_birch_tree",
            "minecraft:fallen_jungle_tree",
            "minecraft:fallen_oak_tree",
            "minecraft:fallen_poplar_tree",
            "minecraft:fallen_spruce_tree",
            "minecraft:fallen_super_birch_tree"
        ])
    );
    let mut all_reached = BTreeSet::<(&'static str, u8)>::new();
    let mut table_profiles = BTreeSet::from([SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID]);
    let mut total_expected = 0_usize;
    for &biome in SurfaceBiome::all() {
        let mut expected = BTreeSet::<(&'static str, u8)>::new();
        for &source in biome.descriptor().natural_tree_configuration_ids {
            assert!(
                tree_configuration_weight(biome, source) > 0,
                "{} lost positive weight in {}",
                source,
                biome.id()
            );
            table_profiles.insert(source);
            for age in 0_u8..3 {
                expected.insert((source, age));
            }
        }
        if azalea_biomes.contains(&biome) {
            for age in 0_u8..3 {
                expected.insert((SURFACE_AZALEA_INDICATOR_CONFIGURATION_ID, age));
            }
        }
        total_expected += expected.len();
        if expected.is_empty() {
            assert_eq!(tree_configuration(biome, hash2(71839, 0, 0)), None);
            assert_eq!(tree_cover_percent(biome), 0);
            continue;
        }
        let mut reached = BTreeSet::new();
        let mut last_index = 0_i64;
        for index in 0_i64..65_536 {
            for infill in [false, true] {
                if infill && !tree_infill_enabled(biome) {
                    continue;
                }
                let salt = if infill {
                    0x7472_6565_5f66_696c
                } else {
                    0x7472_6565
                };
                let entropy = hash2(71839 ^ salt, index % 256 - 128, index / 256 - 128);
                if entropy % 100 >= tree_cover_percent(biome) {
                    continue;
                }
                let source =
                    tree_configuration(biome, entropy).expect("eligible source-bearing biome");
                let growth = tree_growth(entropy);
                let age = match growth {
                    Growth::Young => 0,
                    Growth::Mature => 1,
                    Growth::Old => 2,
                };
                let pair = (source, age);
                assert!(
                    expected.contains(&pair),
                    "unexpected {pair:?} in {}",
                    biome.id()
                );
                reached.insert(pair);
                if !all_reached.insert(pair) {
                    continue;
                }
                let profile = tree_profiles::profile(source).expect("retained selected profile");
                let geometry =
                    tree_forms::build(profile, growth, entropy).expect("selected geometry");
                let cells = geometry.cells().count();
                assert!(
                    cells > 2 && cells <= TreeGeometry::CELL_LIMIT,
                    "{pair:?} invalid cell count {cells}"
                );
                assert_eq!(
                    tree_forms::bind_states(profile, &geometry, entropy)
                        .unwrap()
                        .len(),
                    cells
                );
                let column = |_| TreeTerrainColumn {
                    solid_top: 80,
                    water_top: None,
                };
                assert!(
                    tree_terrain_admits([0, 0, 80], profile.shape, &geometry, column, || false)
                        .unwrap(),
                    "{pair:?} failed suitable ground"
                );
                let polls = std::cell::Cell::new(0_usize);
                let sampled = std::cell::Cell::new(0_usize);
                let result = tree_terrain_admits(
                    [0, 0, 80],
                    profile.shape,
                    &geometry,
                    |_| {
                        sampled.set(sampled.get() + 1);
                        TreeTerrainColumn {
                            solid_top: 80,
                            water_top: None,
                        }
                    },
                    || {
                        polls.set(polls.get() + 1);
                        polls.get() == cells
                    },
                );
                assert!(
                    matches!(result, Err(AssetError::Cancelled)),
                    "{pair:?} ignored late cancellation"
                );
                assert_eq!(
                    polls.get(),
                    cells,
                    "{pair:?} failed before the intended cancellation point"
                );
                assert!(sampled.get() > 0, "{pair:?} did no support work");
                assert!(
                    tree_terrain_admits([0, 0, 80], profile.shape, &geometry, column, || false)
                        .unwrap(),
                    "{pair:?} retry changed support"
                );
            }
            last_index = index;
            if reached == expected {
                break;
            }
        }
        assert_eq!(
            reached,
            expected,
            "{} missing source/growth after grid index {last_index}",
            biome.id()
        );
        println!(
            "ecology_reachability biome={} pairs={} last_index={last_index}",
            biome.id(),
            reached.len()
        );
    }
    assert_eq!(table_profiles, expected_profiles);
    assert_eq!(total_expected, 348);
    let expected_pairs: BTreeSet<_> = expected_profiles
        .iter()
        .flat_map(|&source| (0_u8..3).map(move |age| (source, age)))
        .collect();
    assert_eq!(all_reached, expected_pairs);
    assert_eq!(all_reached.len(), 132);
    println!(
        "ecology_reachability global_pairs={} per_biome_pairs={total_expected}",
        all_reached.len()
    );
}
#[test]
fn ecology_primary_and_infill_cancel_after_selection_then_retry_cleanly() {
    let region = Region {
        minimum: [-15120, -16400],
        maximum: [-15088, -16368],
    };
    let settings = VoxelLandscapeSettings {
        seed: 71839,
        vegetation_percent: 100,
        structures_percent: 0,
        ..Default::default()
    };
    let fields = TerrainFields::new(71839);
    let clean = prepare(region, &settings, || false).expect("uncancelled natural control");
    let witnesses = [
        (
            false,
            [-1163_i32, -1262],
            [-15112, -16396, 89],
            4_689_086_865_667_730_548_u64,
            "minecraft:birch_bees_0002_leaf_litter",
        ),
        (
            true,
            [-1163_i32, -1261],
            [-15118, -16392, 88],
            4_038_303_347_568_564_364_u64,
            "minecraft:oak_bees_0002_leaf_litter",
        ),
    ];
    for (infill, grid, anchor, expected_entropy, expected_source) in witnesses {
        let salt = if infill {
            0x7472_6565_5f66_696c
        } else {
            0x7472_6565
        };
        let entropy = hash2(71839 ^ salt, i64::from(grid[0]), i64::from(grid[1]));
        assert_eq!(entropy, expected_entropy);
        let xy = if infill {
            [
                grid[0] * 13 + (entropy.rotate_left(29) % 3) as i32 - 1,
                grid[1] * 13 + (entropy.rotate_left(43) % 3) as i32 - 1,
            ]
        } else {
            [
                grid[0] * 13 + 6 + (entropy.rotate_left(29) % 9) as i32 - 4,
                grid[1] * 13 + 6 + (entropy.rotate_left(43) % 9) as i32 - 4,
            ]
        };
        assert_eq!(xy, [anchor[0], anchor[1]]);
        let (sample, biome) = sample_ground(&fields, &settings, xy[0], xy[1]);
        assert_eq!(i32::from(sample.height), anchor[2]);
        assert_eq!(tree_configuration(biome, entropy), Some(expected_source));
        assert!(entropy % 100 < tree_cover_percent(biome));
        assert!(!infill || tree_infill_enabled(biome));
        assert!(clean.trees.iter().any(|record| record.anchor == anchor
            && record.source == expected_source
            && record.projected_cells > 0));
        assert!(clean.blocks.values().any(|block| matches!(&block.owner, SourceOwner::Tree { anchor: owner_anchor, configuration } if *owner_anchor == anchor && *configuration == expected_source) && tree_block_is_wood(block)));
        let selected = std::cell::Cell::new(false);
        let after_selection_polls = std::cell::Cell::new(0_usize);
        let result = prepare_with_tree_configuration(
            region,
            &settings,
            |chosen_biome, chosen_entropy| {
                let source = tree_configuration(chosen_biome, chosen_entropy);
                if chosen_entropy == entropy {
                    assert_eq!(source, Some(expected_source));
                    selected.set(true);
                }
                source
            },
            || {
                if !selected.get() {
                    return false;
                }
                after_selection_polls.set(after_selection_polls.get() + 1);
                true
            },
        );
        assert!(
            selected.get(),
            "infill={infill} target never reached selection"
        );
        assert!(
            matches!(result, Err(AssetError::Cancelled)),
            "infill={infill} published despite cancellation"
        );
        assert_eq!(after_selection_polls.get(), 1);
        let retry =
            prepare(region, &settings, || false).expect("clean retry after tree cancellation");
        assert_eq!(retry.columns, clean.columns);
        assert_eq!(retry.biomes, clean.biomes);
        assert_eq!(retry.fluids, clean.fluids);
        assert!(retry
            .blocks
            .iter()
            .map(|(position, block)| (position, &block.state, &block.owner))
            .eq(clean.blocks.iter().map(|(position, block)| (
                position,
                &block.state,
                &block.owner
            ))));
        assert!(retry
            .trees
            .iter()
            .map(|record| (
                record.anchor,
                record.source,
                record.projected_cells,
                record.authored_placement
            ))
            .eq(clean.trees.iter().map(|record| (
                record.anchor,
                record.source,
                record.projected_cells,
                record.authored_placement
            ))));
        assert!(retry
            .flora
            .iter()
            .map(|record| (
                record.anchor,
                record.source,
                record.projected_cells,
                record.authored_placement
            ))
            .eq(clean.flora.iter().map(|record| (
                record.anchor,
                record.source,
                record.projected_cells,
                record.authored_placement
            ))));
        assert!(retry
            .entities
            .iter()
            .map(|entity| (entity.species, entity.anchor, entity.atlas_status))
            .eq(clean.entities.iter().map(|entity| (
                entity.species,
                entity.anchor,
                entity.atlas_status
            ))));
        println!("ecology_cancellation infill={infill} anchor={anchor:?} source={expected_source} support_polls={} retry_blocks={}", after_selection_polls.get(), retry.blocks.len());
    }
}
// B2 projection and overlap regressions; this fragment belongs in the child test module.
fn ecology_assert_projection_eq(expected: &SurfaceWorld, actual: &SurfaceWorld, overlap: Region) {
    let within_xy = |xy: [i32; 2]| overlap.contains([xy[0], xy[1], 0]);
    let expected_blocks: BTreeMap<_, _> = expected
        .blocks
        .iter()
        .filter(|(position, _)| overlap.contains(**position))
        .map(|(&position, block)| (position, (block.state.clone(), block.owner.clone())))
        .collect();
    let actual_blocks: BTreeMap<_, _> = actual
        .blocks
        .iter()
        .filter(|(position, _)| overlap.contains(**position))
        .map(|(&position, block)| (position, (block.state.clone(), block.owner.clone())))
        .collect();
    assert_eq!(
        actual_blocks.len(),
        expected_blocks.len(),
        "final block cardinality differs in {overlap:?}"
    );
    for (position, wanted) in &expected_blocks {
        assert_eq!(
            actual_blocks.get(position),
            Some(wanted),
            "final state or owner differs at {position:?}"
        );
    }
    let expected_fluids: BTreeMap<_, _> = expected
        .fluids
        .iter()
        .filter(|(position, _)| overlap.contains(**position))
        .map(|(&position, &cell)| (position, cell))
        .collect();
    let actual_fluids: BTreeMap<_, _> = actual
        .fluids
        .iter()
        .filter(|(position, _)| overlap.contains(**position))
        .map(|(&position, &cell)| (position, cell))
        .collect();
    assert_eq!(
        actual_fluids, expected_fluids,
        "fluid publication differs in {overlap:?}"
    );
    let expected_columns: BTreeMap<_, _> = expected
        .columns
        .iter()
        .filter(|(xy, _)| within_xy(**xy))
        .map(|(&xy, &sample)| (xy, sample))
        .collect();
    let actual_columns: BTreeMap<_, _> = actual
        .columns
        .iter()
        .filter(|(xy, _)| within_xy(**xy))
        .map(|(&xy, &sample)| (xy, sample))
        .collect();
    let area = ((overlap.maximum[0] - overlap.minimum[0])
        * (overlap.maximum[1] - overlap.minimum[1])) as usize;
    assert_eq!(
        expected_columns.len(),
        area,
        "reference has missing visible columns"
    );
    assert_eq!(
        actual_columns, expected_columns,
        "complete terrain samples differ in {overlap:?}"
    );
    let expected_biomes: BTreeMap<_, _> = expected
        .biomes
        .iter()
        .filter(|(xy, _)| within_xy(**xy))
        .map(|(&xy, &biome)| (xy, biome))
        .collect();
    let actual_biomes: BTreeMap<_, _> = actual
        .biomes
        .iter()
        .filter(|(xy, _)| within_xy(**xy))
        .map(|(&xy, &biome)| (xy, biome))
        .collect();
    assert_eq!(
        expected_biomes.len(),
        area,
        "reference has missing visible biome labels"
    );
    assert_eq!(
        actual_biomes, expected_biomes,
        "biome labels differ in {overlap:?}"
    );
}
// Three supplied natural footprints cover positive-X woodland, negative-X cold forest and wet mangrove ground.
#[test]
fn ecology_natural_worlds_preserve_final_publication_across_split_reversed_and_shifted_windows() {
    let settings = VoxelLandscapeSettings {
        seed: 71839,
        vegetation_percent: 100,
        ..Default::default()
    };
    for (center, expected_biome) in [
        ([15616, -16384], SurfaceBiome::WoodedBadlands),
        ([-13440, -16384], SurfaceBiome::OldGrowthSpruceTaiga),
        ([7424, -16384], SurfaceBiome::MangroveSwamp),
    ] {
        let region = Region {
            minimum: [center[0] - 32, center[1] - 32],
            maximum: [center[0] + 32, center[1] + 32],
        };
        let whole = prepare(region, &settings, || false).unwrap();
        assert_eq!(
            whole.biomes.get(&center),
            Some(&expected_biome),
            "natural witness center changed at {center:?}"
        );
        assert!(
            whole.trees.iter().any(|record| record.projected_cells > 0),
            "no tree actually projected at {center:?}"
        );
        assert!(
            whole
                .blocks
                .values()
                .any(|block| matches!(&block.owner, SourceOwner::Tree { .. })),
            "no final tree-owned cells at {center:?}"
        );
        if expected_biome == SurfaceBiome::MangroveSwamp {
            assert!(
                !whole.fluids.is_empty(),
                "mangrove seam became a dry-ground comparison"
            );
        }
        let split_x = if expected_biome == SurfaceBiome::WoodedBadlands {
            15635
        } else {
            center[0] + 3
        };
        let left = Region {
            minimum: region.minimum,
            maximum: [split_x, region.maximum[1]],
        };
        let right = Region {
            minimum: [split_x, region.minimum[1]],
            maximum: region.maximum,
        };
        assert_eq!(
            (left.maximum[0] - left.minimum[0]) + (right.maximum[0] - right.minimum[0]),
            64
        );
        for ordered_parts in [[left, right], [right, left]] {
            for part in ordered_parts {
                let partial = prepare(part, &settings, || false).unwrap();
                ecology_assert_projection_eq(&whole, &partial, part);
            }
        }
        let shifted_region = Region {
            minimum: [region.minimum[0] + 7, region.minimum[1] + 11],
            maximum: [region.maximum[0] + 7, region.maximum[1] + 11],
        };
        let shifted = prepare(shifted_region, &settings, || false).unwrap();
        let overlap = Region {
            minimum: shifted_region.minimum,
            maximum: region.maximum,
        };
        ecology_assert_projection_eq(&whole, &shifted, overlap);
        eprintln!("ecology_seam center={center:?} biome={} whole_columns=4096 shifted_overlap_columns={} block_cells={} fluid_cells={} request_orders=2", expected_biome.id(), 57 * 53, whole.blocks.len(), whole.fluids.len());
    }
}
// Source-derived fixed overlap: the primary stem and later infill crown physically meet at this exact cell.
#[test]
fn ecology_later_natural_crown_cannot_erase_an_earlier_stem() {
    let settings = VoxelLandscapeSettings {
        seed: 71839,
        vegetation_percent: 100,
        ..Default::default()
    };
    let region = Region {
        minimum: [-13472, -16416],
        maximum: [-13408, -16352],
    };
    let fields = TerrainFields::new(u64::from(settings.seed));
    let position = [-13460, -16405, 93];
    let earlier_anchor = [-13460, -16404, 88];
    let later_anchor = [-13456, -16407, 88];
    let witnesses = [
        (
            false,
            [-1036, -1262],
            earlier_anchor,
            8_447_049_914_129_620_602_u64,
            [0_i16, -1, 5],
            TreeCell::Log(LogAxis::Vertical),
        ),
        (
            true,
            [-1035, -1262],
            later_anchor,
            6_101_139_857_992_908_054_u64,
            [-4_i16, 2, 5],
            TreeCell::Leaf,
        ),
    ];
    let whole = prepare(region, &settings, || false).unwrap();
    let mut expected_wood: Option<BlockState> = None;
    for (infill, grid, anchor, expected_entropy, offset, expected_cell) in witnesses {
        let salt = if infill {
            0x7472_6565_5f66_696c
        } else {
            0x7472_6565
        };
        let entropy = hash2(
            u64::from(settings.seed) ^ salt,
            i64::from(grid[0]),
            i64::from(grid[1]),
        );
        assert_eq!(entropy, expected_entropy);
        let xy = if infill {
            [
                grid[0] * 13 + (entropy.rotate_left(29) % 3) as i32 - 1,
                grid[1] * 13 + (entropy.rotate_left(43) % 3) as i32 - 1,
            ]
        } else {
            [
                grid[0] * 13 + 6 + (entropy.rotate_left(29) % 9) as i32 - 4,
                grid[1] * 13 + 6 + (entropy.rotate_left(43) % 9) as i32 - 4,
            ]
        };
        assert_eq!(xy, [anchor[0], anchor[1]]);
        let (sample, biome) = sample_ground(&fields, &settings, xy[0], xy[1]);
        assert_eq!(i32::from(sample.height), anchor[2]);
        assert_eq!(biome, SurfaceBiome::OldGrowthSpruceTaiga);
        assert!(entropy % 100 < tree_cover_percent(biome));
        assert!(!infill || tree_infill_enabled(biome));
        assert_eq!(
            tree_configuration(biome, entropy),
            Some("minecraft:mega_spruce")
        );
        assert_eq!(tree_growth(entropy), Growth::Mature);
        let profile = tree_profiles::profile("minecraft:mega_spruce").unwrap();
        let geometry = tree_forms::build(profile, tree_growth(entropy), entropy).unwrap();
        assert_eq!(
            geometry
                .cells()
                .find_map(|(at, cell)| (at == offset).then_some(cell)),
            Some(expected_cell)
        );
        assert_eq!(add_global(anchor, offset).unwrap(), position);
        assert!(tree_terrain_admits(
            anchor,
            profile.shape,
            &geometry,
            |xy| {
                let natural = fields.sample(xy[0], xy[1], settings.rivers);
                TreeTerrainColumn {
                    solid_top: i32::from(natural.height),
                    water_top: natural.water_level.map(i32::from),
                }
            },
            || false
        )
        .unwrap());
        assert!(
            whole.trees.iter().any(|record| record.anchor == anchor
                && record.source == profile.id
                && record.projected_cells > 0),
            "overlap candidate never published: {anchor:?}"
        );
        let bound = tree_forms::bind_states(profile, &geometry, entropy)
            .unwrap()
            .into_iter()
            .find(|cell| cell.position == offset)
            .unwrap();
        if infill {
            assert!(profile
                .crowns
                .iter()
                .any(|resource| resource.id == bound.resource_id));
            continue;
        }
        expected_wood = Some(
            state(
                bound.resource_id,
                bound
                    .properties
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value.to_owned())),
            )
            .unwrap(),
        );
    }
    let final_block = whole
        .blocks
        .get(&position)
        .expect("physical overlap cell must be present");
    assert_eq!(
        final_block.state,
        expected_wood.expect("earlier wood state must be bound"),
        "later natural crown erased earlier wood at {position:?}"
    );
    assert_eq!(
        final_block.owner,
        SourceOwner::Tree {
            anchor: earlier_anchor,
            configuration: "minecraft:mega_spruce"
        }
    );
    assert!(
        whole.blocks.values().any(|block| block.owner
            == SourceOwner::Tree {
                anchor: later_anchor,
                configuration: "minecraft:mega_spruce"
            }),
        "later candidate has no surviving physical cells"
    );
    eprintln!("ecology_wood_guard position={position:?} earlier={earlier_anchor:?} later={later_anchor:?} actual_final_resource={}", final_block.state.id().as_str());
}
// Classifier coverage supplements the real write-branch oracle with every protected role and an ownership counterexample.
#[test]
fn ecology_wood_priority_recognizes_stems_roots_and_hearts_only_with_generated_ownership() {
    for (resource, owner, expected) in [
        (
            "minecraft:spruce_log",
            SourceOwner::Tree {
                anchor: [0, 0, 80],
                configuration: "minecraft:mega_spruce",
            },
            true,
        ),
        (
            "minecraft:mangrove_roots",
            SourceOwner::Tree {
                anchor: [0, 0, 63],
                configuration: "minecraft:mangrove",
            },
            true,
        ),
        (
            "minecraft:creaking_heart",
            SourceOwner::TreeDecoration {
                anchor: [0, 0, 80],
                configuration: "minecraft:pale_oak_creaking",
            },
            true,
        ),
        (
            "minecraft:spruce_leaves",
            SourceOwner::Tree {
                anchor: [0, 0, 80],
                configuration: "minecraft:mega_spruce",
            },
            false,
        ),
        (
            "minecraft:vine",
            SourceOwner::TreeDecoration {
                anchor: [0, 0, 80],
                configuration: "minecraft:mangrove",
            },
            false,
        ),
        (
            "minecraft:spruce_log",
            SourceOwner::Saved {
                java_position: [0, 80, 0],
            },
            false,
        ),
        (
            "minecraft:spruce_log",
            SourceOwner::Terrain {
                biome: SurfaceBiome::OldGrowthSpruceTaiga,
            },
            false,
        ),
    ] {
        let block = SurfaceBlock {
            state: plain(resource).unwrap(),
            owner,
        };
        assert_eq!(
            tree_block_is_wood(&block),
            expected,
            "incorrect wood priority for {resource}"
        );
    }
}
// Packet288 uses the existing generator and projection oracle; no native clock or art is asserted.
fn night_flora_assert_same_world(expected: &SurfaceWorld, actual: &SurfaceWorld) {
    assert_eq!(actual.region, expected.region);
    assert_eq!(actual.seed, expected.seed);
    assert_eq!(actual.blocks.len(), expected.blocks.len());
    assert_eq!(actual.fluids, expected.fluids);
    ecology_assert_projection_eq(expected, actual, expected.region);
    assert_eq!(actual.columns, expected.columns);
    assert_eq!(actual.biomes, expected.biomes);
    let record_snapshot = |world: &SurfaceWorld| {
        [&world.trees, &world.flora, &world.structures].map(|records| {
            records
                .iter()
                .map(|record| {
                    (
                        record.anchor,
                        record.source,
                        record.projected_cells,
                        record.authored_placement,
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(record_snapshot(actual), record_snapshot(expected));
    let entity_snapshot = |world: &SurfaceWorld| {
        world
            .entities
            .iter()
            .map(|entity| {
                (
                    entity.species,
                    entity.anchor,
                    entity.atlas_status,
                    entity.model.bounds(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(entity_snapshot(actual), entity_snapshot(expected));
    assert_eq!(actual.source_limitations, expected.source_limitations);
}
#[test]
fn night_flora_original_api_negative_core_and_halo_projections_keep_exact_counterparts() {
    use super::ecology_regression_tests::{night_flora_region, night_flora_settings};
    let region = night_flora_region();
    let day = prepare(region, &night_flora_settings(0), || false).unwrap();
    let flower_position = day
        .blocks
        .iter()
        .find_map(|(&position, block)| {
            (block.state.id().as_str() == "minecraft:closed_eyeblossom"
                && matches!(&block.owner, SourceOwner::Flora { .. }))
            .then_some(position)
        })
        .expect("negative witness region must publish a natural closed eyeblossom");
    assert!(flower_position[0] < 0 && flower_position[1] < 0);
    let settings = night_flora_settings(1);
    let whole = prepare(region, &settings, || false).unwrap();
    let flower = &whole.blocks[&flower_position];
    assert_eq!(flower.state.id().as_str(), "minecraft:open_eyeblossom");
    assert_eq!(
        flower.state.properties(),
        day.blocks[&flower_position].state.properties()
    );
    assert_eq!(flower.owner, day.blocks[&flower_position].owner);
    for axis in [0_usize, 1_usize] {
        let split = flower_position[axis];
        assert!(split > region.minimum[axis] && split < region.maximum[axis]);
        let mut lower = region;
        lower.maximum[axis] = split;
        let mut upper = region;
        upper.minimum[axis] = split;
        assert!(upper.contains(flower_position) && !lower.contains(flower_position));
        for ordered_parts in [[lower, upper], [upper, lower]] {
            for part in ordered_parts {
                let partial = prepare(part, &settings, || false).unwrap();
                ecology_assert_projection_eq(&whole, &partial, part);
                let expanded_region = Region {
                    minimum: part.minimum.map(|value| value - 8),
                    maximum: part.maximum.map(|value| value + 8),
                };
                let expanded = prepare(expanded_region, &settings, || false).unwrap();
                ecology_assert_projection_eq(&partial, &expanded, part);
            }
        }
    }
    let shifted_region = Region {
        minimum: [flower_position[0] - 32, flower_position[1] - 32],
        maximum: [flower_position[0] + 32, flower_position[1] + 32],
    };
    assert_ne!(shifted_region, region);
    assert!(shifted_region.maximum.iter().all(|value| *value < 0));
    let shifted = prepare(shifted_region, &settings, || false).unwrap();
    let overlap = Region {
        minimum: [
            region.minimum[0].max(shifted_region.minimum[0]),
            region.minimum[1].max(shifted_region.minimum[1]),
        ],
        maximum: [
            region.maximum[0].min(shifted_region.maximum[0]),
            region.maximum[1].min(shifted_region.maximum[1]),
        ],
    };
    assert!(overlap.contains(flower_position));
    ecology_assert_projection_eq(&whole, &shifted, overlap);
    let stitched = prepare_viewport(region, 2.8, [4096, 4096], &settings, || false).unwrap();
    ecology_assert_projection_eq(&whole, &stitched, region);
    assert_eq!(stitched.blocks[&flower_position].state, flower.state);
}
#[test]
fn night_flora_original_api_cancelled_and_invalid_preparations_retry_identically() {
    use super::ecology_regression_tests::{night_flora_region, night_flora_settings};
    let region = night_flora_region();
    let settings = night_flora_settings(1);
    let calls = std::cell::Cell::new(0_usize);
    let selections = std::cell::Cell::new(0_usize);
    let clean = prepare_with_tree_configuration(
        region,
        &settings,
        |biome, entropy| {
            selections.set(selections.get() + 1);
            tree_configuration(biome, entropy)
        },
        || {
            calls.set(calls.get() + 1);
            false
        },
    )
    .unwrap();
    assert!(clean
        .blocks
        .values()
        .any(|block| block.state.id().as_str() == "minecraft:open_eyeblossom"));
    let final_checkpoint = calls.get();
    let completed_selections = selections.get();
    assert!(completed_selections > 0);
    assert!(final_checkpoint > 1);
    for stop_at in [1_usize, final_checkpoint] {
        let attempt_calls = std::cell::Cell::new(0_usize);
        let attempt_selections = std::cell::Cell::new(0_usize);
        let result = prepare_with_tree_configuration(
            region,
            &settings,
            |biome, entropy| {
                attempt_selections.set(attempt_selections.get() + 1);
                tree_configuration(biome, entropy)
            },
            || {
                attempt_calls.set(attempt_calls.get() + 1);
                attempt_calls.get() == stop_at
            },
        );
        assert!(matches!(result, Err(AssetError::Cancelled)));
        assert_eq!(attempt_calls.get(), stop_at);
        let expected_selections = if stop_at == 1 {
            0
        } else {
            completed_selections
        };
        assert_eq!(attempt_selections.get(), expected_selections);
        let retry = prepare(region, &settings, || false).unwrap();
        night_flora_assert_same_world(&clean, &retry);
    }
    let invalid_region = Region {
        minimum: region.minimum,
        maximum: [region.minimum[0] + 129, region.maximum[1]],
    };
    let invalid_polls = std::cell::Cell::new(0_usize);
    let invalid = prepare(invalid_region, &settings, || {
        invalid_polls.set(invalid_polls.get() + 1);
        false
    });
    assert!(matches!(invalid, Err(AssetError::InvalidMetadata(_))));
    assert_eq!(invalid_polls.get(), 0);
    for atmosphere in [0_usize, 2_usize] {
        let other_settings = night_flora_settings(atmosphere);
        let other = prepare(region, &other_settings, || false).unwrap();
        assert!(other
            .blocks
            .values()
            .any(|block| block.state.id().as_str() == "minecraft:closed_eyeblossom"));
        assert!(!other
            .blocks
            .values()
            .any(|block| block.state.id().as_str() == "minecraft:open_eyeblossom"));
        let repeated_other = prepare(region, &other_settings, || false).unwrap();
        night_flora_assert_same_world(&other, &repeated_other);
        let night_again = prepare(region, &settings, || false).unwrap();
        night_flora_assert_same_world(&clean, &night_again);
    }
}
