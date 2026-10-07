use super::*;
fn ecology_region(center: [i32; 2]) -> Region {
    Region {
        minimum: center.map(|value| value - 32),
        maximum: center.map(|value| value + 32),
    }
}
fn ecology_final_cover(
    world: &SurfaceWorld,
    named_biome: SurfaceBiome,
) -> BTreeMap<&'static str, usize> {
    let receipts: BTreeMap<_, _> = world
        .trees
        .iter()
        .map(|record| ((record.anchor, record.source), record.projected_cells))
        .collect();
    let mut parts =
        BTreeMap::<([i32; 3], &'static str), (BTreeSet<[i32; 2]>, BTreeSet<[i32; 2]>)>::new();
    for (&position, block) in &world.blocks {
        assert!(
            world.region.contains(position),
            "Published block escaped projection: {position:?}"
        );
        let (anchor, configuration) = match &block.owner {
            SourceOwner::Tree {
                anchor,
                configuration,
            }
            | SourceOwner::TreeDecoration {
                anchor,
                configuration,
            } => (*anchor, *configuration),
            _ => continue,
        };
        assert!(
            receipts
                .get(&(anchor, configuration))
                .is_some_and(|count| *count > 0),
            "Final tree cell lacks a nonempty publication receipt: {position:?} {anchor:?} {configuration}"
        );
        let profile = tree_profiles::profile(configuration)
            .expect("Final tree owner must resolve to a retained source profile");
        if matches!(&block.owner, SourceOwner::TreeDecoration { .. }) {
            assert!(
                tree_decoration_profiles::profile(configuration).is_some(),
                "Decoration lacks its source profile: {configuration}"
            );
            continue;
        }
        let resource = block.state.id().as_str();
        let is_stem = resource == profile.stem.id;
        let is_crown = profile.crowns.iter().any(|crown| crown.id == resource);
        let is_root =
            profile.shape == TreeShape::Mangrove && resource == "minecraft:mangrove_roots";
        assert!(
            is_stem || is_crown || is_root,
            "Tree owner published an unrelated resource: {configuration} {resource}"
        );
        if matches!(
            profile.shape,
            TreeShape::Fallen | TreeShape::Bush | TreeShape::RedMushroom | TreeShape::BrownMushroom
        ) {
            continue;
        }
        let entry = parts.entry((anchor, configuration)).or_default();
        if is_stem {
            entry.0.insert([position[0], position[1]]);
        }
        if is_crown {
            entry.1.insert([position[0], position[1]]);
        }
    }
    let named_xy: BTreeSet<_> = world
        .biomes
        .iter()
        .filter(|(xy, biome)| **biome == named_biome && world.region.contains([xy[0], xy[1], 0]))
        .map(|(xy, _)| *xy)
        .collect();
    let mut leaf_xy = BTreeSet::new();
    let mut stem_xy = BTreeSet::new();
    let mut crown_without_projected_stem_xy = BTreeSet::new();
    let mut paired_owners = 0_usize;
    for (_, (stems, crowns)) in parts {
        if stems.is_empty() {
            crown_without_projected_stem_xy.extend(crowns);
            continue;
        }
        if crowns.is_empty() {
            continue;
        }
        paired_owners += 1;
        stem_xy.extend(stems);
        leaf_xy.extend(crowns);
    }
    BTreeMap::from([
        (
            "total_columns",
            ((world.region.maximum[0] - world.region.minimum[0])
                * (world.region.maximum[1] - world.region.minimum[1])) as usize,
        ),
        ("named_columns", named_xy.len()),
        ("leaf_xy", leaf_xy.len()),
        ("named_leaf_xy", leaf_xy.intersection(&named_xy).count()),
        ("stem_xy", stem_xy.len()),
        ("named_stem_xy", stem_xy.intersection(&named_xy).count()),
        ("paired_owners", paired_owners),
        (
            "crown_without_projected_stem_xy",
            crown_without_projected_stem_xy.len(),
        ),
        (
            "zero_projected_receipts",
            world
                .trees
                .iter()
                .filter(|record| record.projected_cells == 0)
                .count(),
        ),
    ])
}
fn ecology_assert_natural_cover(
    center: [i32; 2],
    biome: SurfaceBiome,
    minimum_named_percent: usize,
    minimum_cover_percent: usize,
    minimum_paired_owners: usize,
) {
    let world = prepare(
        ecology_region(center),
        &VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        },
        || false,
    )
    .unwrap();
    let counts = ecology_final_cover(&world, biome);
    eprintln!(
        "ecology_final_cover center={center:?} biome={} counts={counts:?}",
        biome.id()
    );
    assert_eq!(counts["total_columns"], 4096);
    assert!(
        counts["named_columns"] * 100 >= counts["total_columns"] * minimum_named_percent,
        "Named biome was lost at {center:?}: {counts:?}"
    );
    assert!(
        counts["named_leaf_xy"] * 100 >= counts["named_columns"] * minimum_cover_percent,
        "Insufficient final-owned standing crown at {center:?}: {counts:?}"
    );
    assert!(
        counts["paired_owners"] >= minimum_paired_owners,
        "Insufficient remaining stem/crown owner pairs at {center:?}: {counts:?}"
    );
    assert!(
        counts["named_stem_xy"] >= minimum_paired_owners,
        "Named biome lacks physical stems at {center:?}: {counts:?}"
    );
}
#[test]
fn original_forest_requires_final_owned_canopy() {
    ecology_assert_natural_cover([-15104, -16384], SurfaceBiome::Forest, 65, 25, 12);
}
#[test]
fn original_dark_forest_requires_final_owned_canopy() {
    ecology_assert_natural_cover([-14080, -16384], SurfaceBiome::DarkForest, 75, 25, 10);
}
#[test]
fn original_old_growth_birch_requires_final_owned_canopy() {
    ecology_assert_natural_cover(
        [-14336, -16384],
        SurfaceBiome::OldGrowthBirchForest,
        95,
        18,
        20,
    );
}
#[test]
fn original_old_growth_pine_requires_final_owned_canopy() {
    ecology_assert_natural_cover(
        [-11904, -16128],
        SurfaceBiome::OldGrowthPineTaiga,
        95,
        30,
        18,
    );
}
#[test]
fn original_old_growth_spruce_requires_final_owned_canopy() {
    ecology_assert_natural_cover(
        [-13440, -16384],
        SurfaceBiome::OldGrowthSpruceTaiga,
        75,
        35,
        18,
    );
}
#[test]
fn original_wooded_badlands_requires_final_owned_canopy() {
    ecology_assert_natural_cover([15616, -16384], SurfaceBiome::WoodedBadlands, 70, 10, 8);
}
#[test]
fn original_dry_mangrove_requires_final_owned_canopy() {
    ecology_assert_natural_cover([7424, -16384], SurfaceBiome::MangroveSwamp, 75, 35, 18);
}
#[test]
fn original_windswept_forest_requires_final_owned_canopy() {
    ecology_assert_natural_cover([-3968, -16384], SurfaceBiome::WindsweptForest, 45, 12, 5);
}
#[test]
fn original_dry_mangrove_world_and_retained_cores_have_actual_root_contact() {
    for center in [[7424, -16384], [11008, -14464], [-13056, -15616]] {
        let world = prepare(
            ecology_region(center),
            &VoxelLandscapeSettings {
                seed: 71839,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
        let counts = ecology_final_cover(&world, SurfaceBiome::MangroveSwamp);
        let mut wet_columns = 0_usize;
        let mut dry_columns = 0_usize;
        for (&xy, &biome) in &world.biomes {
            if biome != SurfaceBiome::MangroveSwamp || !world.region.contains([xy[0], xy[1], 0]) {
                continue;
            }
            let sample = world.columns[&xy];
            if sample
                .water_level
                .is_some_and(|level| level > sample.height)
            {
                wet_columns += 1;
            } else {
                dry_columns += 1;
            }
        }
        let mut wet_roots = 0_usize;
        let mut dry_roots = 0_usize;
        let mut bed_contacts = 0_usize;
        let mut bed_owners = BTreeSet::new();
        for (&position, block) in &world.blocks {
            if block.state.id().as_str() != "minecraft:mangrove_roots" {
                continue;
            }
            let SourceOwner::Tree {
                anchor,
                configuration,
            } = &block.owner
            else {
                panic!("Root lost exact tree owner at {position:?}");
            };
            assert_eq!(
                tree_profiles::profile(configuration).unwrap().shape,
                TreeShape::Mangrove
            );
            let sample = world.columns[&[position[0], position[1]]];
            if let Some(fluid) = world.fluids.get(&position) {
                wet_roots += 1;
                assert_eq!(block.state.property("waterlogged"), Some("true"));
                assert_eq!(sample.water_level.map(i32::from), Some(position[2] + 1));
                assert_eq!(fluid.level, 0);
            } else {
                dry_roots += 1;
                assert_eq!(block.state.property("waterlogged"), Some("false"));
            }
            let below = [position[0], position[1], position[2] - 1];
            if position[2] != i32::from(sample.height) {
                continue;
            }
            let Some(support) = world.blocks.get(&below) else {
                continue;
            };
            if !matches!(&support.owner, SourceOwner::Terrain { .. }) {
                continue;
            }
            bed_contacts += 1;
            bed_owners.insert((*anchor, *configuration));
        }
        eprintln!(
            "ecology_root_contact center={center:?} cover={counts:?} wet_columns={wet_columns} dry_columns={dry_columns} wet_roots={wet_roots} dry_roots={dry_roots} bed_contacts={bed_contacts} bed_owners={}",
            bed_owners.len()
        );
        assert_eq!(wet_columns + dry_columns, counts["named_columns"]);
        assert!(
            wet_columns * 20 >= counts["named_columns"]
                && dry_columns * 20 >= counts["named_columns"],
            "Wetland lost its pools or banks at {center:?}"
        );
        assert!(
            wet_roots >= 16 && dry_roots >= 32,
            "Actual root states disappeared at {center:?}: wet={wet_roots} dry={dry_roots}"
        );
        assert!(
            bed_contacts >= 16 && bed_owners.len() >= 8,
            "Root-to-bed contact disappeared at {center:?}: cells={bed_contacts} owners={}",
            bed_owners.len()
        );
    }
}
#[test]
fn natural_vegetation_scalar_changes_features_without_changing_world_identity() {
    let region = ecology_region([-15104, -16384]);
    let make_world = |vegetation_percent| {
        prepare(
            region,
            &VoxelLandscapeSettings {
                seed: 71839,
                vegetation_percent,
                ..Default::default()
            },
            || false,
        )
        .unwrap()
    };
    let zero = make_world(0);
    let half = make_world(50);
    let full = make_world(100);
    let saturated = make_world(200);
    for world in [&zero, &half, &saturated] {
        assert_eq!(world.seed, full.seed);
        assert_eq!(world.columns, full.columns);
        assert_eq!(world.biomes, full.biomes);
    }
    assert!(zero.trees.is_empty() && zero.flora.is_empty());
    assert!(zero.blocks.values().all(|block| !matches!(
        &block.owner,
        SourceOwner::Tree { .. } | SourceOwner::TreeDecoration { .. } | SourceOwner::Flora { .. }
    )));
    let keys = |world: &SurfaceWorld| -> BTreeSet<_> {
        world
            .trees
            .iter()
            .map(|record| (record.anchor, record.source))
            .collect()
    };
    let half_keys = keys(&half);
    let full_keys = keys(&full);
    assert!(
        !half_keys.is_empty()
            && half_keys.len() < full_keys.len()
            && half_keys.is_subset(&full_keys)
    );
    let half_cover = ecology_final_cover(&half, SurfaceBiome::Forest);
    let full_cover = ecology_final_cover(&full, SurfaceBiome::Forest);
    assert!(
        half_cover["leaf_xy"] > 0 && half_cover["leaf_xy"] < full_cover["leaf_xy"],
        "Natural scalar lost its measured coverage effect: half={half_cover:?} full={full_cover:?}"
    );
    let block_snapshot = |world: &SurfaceWorld| -> BTreeMap<_, _> {
        world
            .blocks
            .iter()
            .map(|(position, block)| (*position, (block.state.clone(), block.owner.clone())))
            .collect()
    };
    assert_eq!(block_snapshot(&full), block_snapshot(&saturated));
    assert_eq!(full.fluids, saturated.fluids);
    let record_snapshot = |records: &[FeatureRecord]| -> Vec<_> {
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
            .collect()
    };
    assert_eq!(
        record_snapshot(&full.trees),
        record_snapshot(&saturated.trees)
    );
    assert_eq!(
        record_snapshot(&full.flora),
        record_snapshot(&saturated.flora)
    );
    assert_eq!(
        record_snapshot(&full.structures),
        record_snapshot(&saturated.structures)
    );
    let entity_snapshot = |world: &SurfaceWorld| -> Vec<_> {
        world
            .entities
            .iter()
            .map(|entity| (entity.anchor, entity.species, entity.atlas_status))
            .collect()
    };
    assert_eq!(entity_snapshot(&full), entity_snapshot(&saturated));
    assert_eq!(full.source_limitations, saturated.source_limitations);
    eprintln!(
        "ecology_scalar half_candidate_owners={} full_candidate_owners={} half_cover={half_cover:?} full_cover={full_cover:?}",
        half_keys.len(),
        full_keys.len()
    );
}
// Packet288: fixed-atmosphere tests call the original generator and admission APIs.
pub(super) fn night_flora_settings(atmosphere: usize) -> VoxelLandscapeSettings {
    VoxelLandscapeSettings {
        seed: 71839,
        atmosphere,
        vegetation_percent: 100,
        structures_percent: 0,
        ..Default::default()
    }
}
// No natural Pale Garden coordinate is supplied; discover one without inventing a spawn.
pub(super) fn night_flora_region() -> Region {
    let settings = night_flora_settings(0);
    let fields = TerrainFields::new(u64::from(settings.seed));
    let mut attempted_regions = 0_usize;
    for sample_index in 0_i32..16_384 {
        let center = [
            (sample_index % 128 - 128) * 128 + 64,
            (sample_index / 128 - 128) * 128 + 64,
        ];
        let (_, biome) = sample_ground(&fields, &settings, center[0], center[1]);
        if biome != SurfaceBiome::PaleGarden {
            continue;
        }
        if attempted_regions == 16 {
            break;
        }
        attempted_regions += 1;
        let region = ecology_region(center);
        let world = prepare(region, &settings, || false).expect("natural Day witness search");
        let has_closed = world.blocks.values().any(|block| {
            block.state.id().as_str() == "minecraft:closed_eyeblossom"
                && matches!(block.owner, SourceOwner::Flora { .. })
        });
        let has_other_flora = world.blocks.values().any(|block| {
            block.state.id().as_str() != "minecraft:closed_eyeblossom"
                && matches!(block.owner, SourceOwner::Flora { .. })
        });
        if !has_closed || !has_other_flora {
            continue;
        }
        eprintln!("night_flora_witness region={region:?} attempts={attempted_regions}");
        return region;
    }
    panic!("no mixed natural Pale Garden witness in 16384 samples / {attempted_regions} worlds");
}
// Admission stays a closed-state prescription; Night must not add a second placement slot.
#[test]
fn night_flora_closed_candidate_keeps_provider_and_soil_admission() {
    let anchor = [-6, -10, 80];
    let candidate = flower_candidate(anchor, "minecraft:closed_eyeblossom");
    assert_eq!(candidate.anchor, anchor);
    assert_eq!(candidate.source_prescription, "minecraft:closed_eyeblossom");
    assert_eq!(candidate.support, Support::Soil);
    assert!(!candidate.cactus_clearance);
    assert_eq!(candidate.cells.len(), 1);
    assert_eq!(candidate.cells[0].position, [0, 0, 0]);
    assert_eq!(
        candidate.cells[0].state.resource_id,
        "minecraft:closed_eyeblossom"
    );
    assert_eq!(
        candidate.cells[0].state.properties,
        [("schedule_tick", "true")]
    );
    assert!(
        !ENTRIES
            .iter()
            .any(|entry| entry.id == "minecraft:open_eyeblossom")
    );
    for support in [HabitatCell::Sand, HabitatCell::Soil] {
        let mut placement = FloraPlacement::new(4).unwrap();
        let result = placement.admit(
            flower_candidate(anchor, "minecraft:closed_eyeblossom"),
            |position| {
                if position == [anchor[0], anchor[1], anchor[2] - 1] {
                    return support;
                }
                HabitatCell::Air
            },
        );
        if support == HabitatCell::Sand {
            assert_eq!(result, Err(AdmissionError::Unsupported([-6, -10, 79])));
            assert_eq!(placement.cells().count(), 0);
            continue;
        }
        assert_eq!(result, Ok(1));
        let accepted = placement.cells().next().unwrap();
        assert_eq!(accepted.position, anchor);
        assert_eq!(accepted.state, candidate.cells[0].state);
    }
}
// Uses only original APIs; the intended baseline RED is the explicit Night assertion.
#[test]
fn night_flora_only_explicit_night_changes_the_natural_counterpart() {
    let region = night_flora_region();
    let day = prepare(region, &night_flora_settings(0), || false).unwrap();
    let storm = prepare(region, &night_flora_settings(2), || false).unwrap();
    let night = prepare(region, &night_flora_settings(1), || false).unwrap();
    let records = |items: &[FeatureRecord]| {
        items
            .iter()
            .map(|item| {
                (
                    item.anchor,
                    item.source,
                    item.projected_cells,
                    item.authored_placement,
                )
            })
            .collect::<Vec<_>>()
    };
    for other in [&storm, &night] {
        assert_eq!(other.region, day.region);
        assert_eq!(other.seed, day.seed);
        assert_eq!(other.columns, day.columns);
        assert_eq!(other.biomes, day.biomes);
        assert_eq!(other.fluids, day.fluids);
        assert_eq!(other.blocks.len(), day.blocks.len());
        assert_eq!(records(&other.flora), records(&day.flora));
        assert_eq!(records(&other.trees), records(&day.trees));
        assert_eq!(records(&other.structures), records(&day.structures));
        assert_eq!(other.source_limitations, day.source_limitations);
    }
    let mut flowers = 0_usize;
    let mut unrelated_flora = 0_usize;
    for (&position, expected) in &day.blocks {
        let wet = storm
            .blocks
            .get(&position)
            .expect("Thunderstorm retained block");
        let dark = night.blocks.get(&position).expect("Night retained block");
        assert_eq!(wet.owner, expected.owner);
        assert_eq!(dark.owner, expected.owner);
        assert_eq!(wet.state, expected.state);
        if expected.state.id().as_str() == "minecraft:closed_eyeblossom" {
            flowers += 1;
            assert_eq!(
                dark.state.id().as_str(),
                "minecraft:open_eyeblossom",
                "fixed Night at {position:?}"
            );
            assert_eq!(dark.state.properties(), expected.state.properties());
            assert_eq!(dark.state.property("schedule_tick"), Some("true"));
            assert_eq!(
                expected.owner,
                SourceOwner::Flora {
                    anchor: position,
                    prescription: "minecraft:closed_eyeblossom",
                }
            );
            assert!(day.flora.iter().any(|record| {
                record.anchor == position
                    && record.source == "minecraft:closed_eyeblossom"
                    && record.projected_cells == 1
                    && record.authored_placement
            }));
            continue;
        }
        assert_ne!(dark.state.id().as_str(), "minecraft:open_eyeblossom");
        if expected.state.id().as_str() == "minecraft:creaking_heart"
            && matches!(expected.owner, SourceOwner::TreeDecoration { .. })
            && day.biomes.get(&[position[0], position[1]]) == Some(&SurfaceBiome::PaleGarden)
        {
            let mut properties = expected.state.properties().clone();
            properties.insert("creaking_heart_state".into(), "awake".into());
            let awake = BlockState::new(expected.state.id().clone(), properties).unwrap();
            assert_eq!(dark.state, awake);
            continue;
        }
        assert_eq!(dark.state, expected.state, "unrelated cell at {position:?}");
        unrelated_flora += usize::from(matches!(expected.owner, SourceOwner::Flora { .. }));
    }
    assert!(flowers > 0 && unrelated_flora > 0);
    assert!(
        !night
            .blocks
            .values()
            .any(|block| { block.state.id().as_str() == "minecraft:closed_eyeblossom" })
    );
    assert!(
        !day.blocks
            .values()
            .chain(storm.blocks.values())
            .any(|block| { block.state.id().as_str() == "minecraft:open_eyeblossom" })
    );
}
