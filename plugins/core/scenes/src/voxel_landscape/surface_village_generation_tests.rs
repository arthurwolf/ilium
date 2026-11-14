//! Actual generated-world projections; native pack pixels remain a separate gate.
use super::super::surface_structures::{Habitat, PlacementError, Prepared, Template, TemplateCell};
use super::super::surface_village_assembly::{self as villages, VillagePlacement};
use super::super::village_kit::MarkerKind;
use super::*;
use std::cell::Cell;

// Frozen current181 grids and original centers; the assembly module separately
// requires >=6 residences, varied home forms, work and farms at each center.
const WITNESSES: [([i32; 2], [i32; 3]); 5] = [
    ([-4, -46], [-896, -11600, 67]),
    ([44, -48], [11440, -12144, 104]),
    ([1, -48], [416, -12160, 80]),
    ([-45, -48], [-11344, -12128, 99]),
    ([9, -48], [2480, -12224, 98]),
];
fn settings() -> VoxelLandscapeSettings {
    VoxelLandscapeSettings {
        seed: 71839,
        vegetation_percent: 100,
        structures_percent: 100,
        ..Default::default()
    }
}
fn natural_village(grid: [i32; 2]) -> VillagePlacement {
    villages::candidate(grid, &TerrainFields::new(71839), &settings(), || false)
        .unwrap()
        .expect("frozen natural locality must remain admitted")
}
fn window(center: [i32; 3], radius: i32) -> Region {
    Region {
        minimum: [center[0] - radius, center[1] - radius],
        maximum: [center[0] + radius, center[1] + radius],
    }
}
fn owned_blocks(
    world: &SurfaceWorld,
    region: Region,
) -> BTreeMap<[i32; 3], (BlockState, SourceOwner)> {
    world
        .blocks
        .iter()
        .filter(|(p, _)| region.contains(**p))
        .map(|(&p, block)| (p, (block.state.clone(), block.owner.clone())))
        .collect()
}
// Only source-proven model fields are compared: no invented Model equality or
// mesh serialization API. These are semantic/model-bound checks, not pixel QA.
type EntitySnapshot = (
    [i32; 3],
    String,
    &'static str,
    String,
    Option<[[u64; 3]; 2]>,
    Vec<String>,
);
fn entities(world: &SurfaceWorld, region: Region, village_only: bool) -> Vec<EntitySnapshot> {
    let mut values: Vec<_> = world
        .entities
        .iter()
        .filter(|entity| {
            region.contains(entity.anchor)
                && (!village_only || entity.atlas_status.starts_with("Village kit marker;"))
        })
        .map(|entity| {
            let bounds = entity.model.bounds().map(|(minimum, maximum)| {
                [
                    std::array::from_fn(|axis| f64::from(minimum[axis]).to_bits()),
                    std::array::from_fn(|axis| f64::from(maximum[axis]).to_bits()),
                ]
            });
            (
                entity.anchor,
                format!("{:?}", entity.species),
                entity.atlas_status,
                format!("{:?}", entity.model.species),
                bounds,
                entity
                    .model
                    .parts
                    .iter()
                    .map(|part| part.texture_semantic.to_string())
                    .collect(),
            )
        })
        .collect();
    values.sort();
    values
}
fn assert_receipts(world: &SurfaceWorld, region: Region) {
    for (position, block) in &world.blocks {
        if !region.contains(*position) {
            continue;
        }
        let (anchor, source, records) = match &block.owner {
            SourceOwner::Tree {
                anchor,
                configuration,
            }
            | SourceOwner::TreeDecoration {
                anchor,
                configuration,
            } => (*anchor, Some(*configuration), &world.trees),
            SourceOwner::Flora {
                anchor,
                prescription,
            } => (*anchor, Some(*prescription), &world.flora),
            SourceOwner::Structure { anchor, .. } => (*anchor, None, &world.structures),
            _ => continue,
        };
        assert!(
            records.iter().any(|record| record.anchor == anchor
                && record.projected_cells > 0
                && record.authored_placement
                && source.is_none_or(|source| record.source == source)),
            "final cell {position:?} lost source receipt {:?}",
            block.owner
        );
    }
}
fn assert_projection(expected: &SurfaceWorld, actual: &SurfaceWorld, overlap: Region) {
    assert_eq!(actual.seed, expected.seed);
    assert_eq!(actual.source_limitations, expected.source_limitations);
    assert_eq!(
        owned_blocks(actual, overlap),
        owned_blocks(expected, overlap)
    );
    let fluids = |world: &SurfaceWorld| -> BTreeMap<_, _> {
        world
            .fluids
            .iter()
            .filter(|(p, _)| overlap.contains(**p))
            .map(|(&p, &cell)| (p, cell))
            .collect()
    };
    let columns = |world: &SurfaceWorld| -> BTreeMap<_, _> {
        world
            .columns
            .iter()
            .filter(|(xy, _)| overlap.contains([xy[0], xy[1], 0]))
            .map(|(&xy, &sample)| (xy, sample))
            .collect()
    };
    let biomes = |world: &SurfaceWorld| -> BTreeMap<_, _> {
        world
            .biomes
            .iter()
            .filter(|(xy, _)| overlap.contains([xy[0], xy[1], 0]))
            .map(|(&xy, &biome)| (xy, biome))
            .collect()
    };
    assert_eq!(fluids(actual), fluids(expected));
    assert_eq!(columns(actual), columns(expected));
    assert_eq!(biomes(actual), biomes(expected));
    // Raw projected_cells totals vary by window; final owners were compared above.
    assert_receipts(actual, overlap);
    assert_receipts(expected, overlap);
    assert_eq!(
        entities(actual, overlap, true),
        entities(expected, overlap, true)
    );
}
fn marker_species(kind: MarkerKind) -> Species {
    match kind {
        MarkerKind::Resident(_) => Species::Villager,
        MarkerKind::Guardian => Species::IronGolem,
        MarkerKind::Animal(id) => Species::from_id(id).expect("retained animal marker identity"),
    }
}
fn assert_village_projection(world: &SurfaceWorld, village: &VillagePlacement) {
    let mut projected = 0;
    for (position, write) in &village.writes {
        if !world.region.contains(*position) {
            continue;
        }
        projected += 1;
        match &write.state {
            None => {
                assert!(
                    !world.blocks.contains_key(position),
                    "owned air filled at {position:?}"
                );
                assert!(
                    !world.fluids.contains_key(position),
                    "owned air flooded at {position:?}"
                );
            }
            Some(state) if state.id().as_str() == "minecraft:water" => {
                assert!(!world.blocks.contains_key(position));
                assert_eq!(
                    world
                        .fluids
                        .get(position)
                        .expect("whole farm/plaza water")
                        .level,
                    0
                );
            }
            Some(state) => {
                let block = world
                    .blocks
                    .get(position)
                    .expect("whole village solid was clipped");
                assert_eq!(&block.state, state);
                assert_eq!(
                    block.owner,
                    SourceOwner::Structure {
                        anchor: village.center,
                        source: write.piece_source.clone()
                    }
                );
                assert!(!world.fluids.contains_key(position));
            }
        }
    }
    assert!(projected > 0);
    let records: Vec<_> = world
        .structures
        .iter()
        .filter(|record| {
            record.anchor == village.center && record.source == villages::source(village.style)
        })
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].projected_cells, projected);
    assert!(records[0].authored_placement);
    let mut wanted: Vec<_> = village
        .markers
        .iter()
        .filter(|marker| world.region.contains(marker.position))
        .map(|marker| {
            (
                marker.position,
                format!("{:?}", marker_species(marker.kind)),
            )
        })
        .collect();
    let mut actual: Vec<_> = world
        .entities
        .iter()
        .filter(|entity| entity.atlas_status.starts_with("Village kit marker;"))
        .map(|entity| (entity.anchor, format!("{:?}", entity.species)))
        .collect();
    wanted.sort();
    actual.sort();
    assert_eq!(
        wanted, actual,
        "missing, additional or duplicated village markers"
    );
    assert!(actual.windows(2).all(|pair| pair[0].0 != pair[1].0));
}

#[test]
fn village_generation_all_five_keep_full_ownership_across_reverse_split_and_shifted_windows() {
    let settings = settings();
    for witness in WITNESSES {
        let village = natural_village(witness.0);
        let region = window(village.center, 64);
        let whole = prepare(region, &settings, || false).unwrap();
        assert_village_projection(&whole, &village);
        let split = village.center[0] + 3;
        let left = Region {
            minimum: region.minimum,
            maximum: [split, region.maximum[1]],
        };
        let right = Region {
            minimum: [split, region.minimum[1]],
            maximum: region.maximum,
        };
        for order in [[left, right], [right, left]] {
            for part in order {
                let partial = prepare(part, &settings, || false).unwrap();
                assert_projection(&whole, &partial, part);
                assert_village_projection(&partial, &village);
            }
        }
        let shifted_region = Region {
            minimum: [region.minimum[0] + 7, region.minimum[1] + 11],
            maximum: [region.maximum[0] + 7, region.maximum[1] + 11],
        };
        let shifted = prepare(shifted_region, &settings, || false).unwrap();
        assert_projection(
            &whole,
            &shifted,
            Region {
                minimum: shifted_region.minimum,
                maximum: region.maximum,
            },
        );
        assert_village_projection(&shifted, &village);
        eprintln!(
            "village_generation_projection style={} grid={:?} center={:?} whole_blocks={} trees={} flora={} village_markers={} orders=2 shifted_overlap=121x117",
            village.style.name(),
            witness.0,
            village.center,
            whole.blocks.len(),
            whole.trees.len(),
            whole.flora.len(),
            village.markers.len()
        );
    }
}

#[test]
fn village_generation_appearance_changes_preserve_the_complete_same_window_snapshot() {
    let settings = settings();
    let village = natural_village(WITNESSES[4].0);
    let region = window(village.center, 24);
    let first = prepare(region, &settings, || false).unwrap();
    let appearance = VoxelLandscapeSettings {
        zoom_percent: 400,
        detail: 0,
        pan_speed_percent: 0,
        pan_direction: 3,
        color_mode: 0,
        palette: 3,
        hue_degrees: 17,
        saturation_percent: 0,
        lightness_percent: 5,
        pack_profile: 7,
        ..settings.clone()
    };
    let second = prepare(region, &appearance, || false).unwrap();
    assert_projection(&first, &second, region);
    assert_eq!(
        entities(&first, region, false),
        entities(&second, region, false)
    );
    let records = |records: &[FeatureRecord]| {
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
    };
    assert_eq!(records(&first.trees), records(&second.trees));
    assert_eq!(records(&first.flora), records(&second.flora));
    assert_eq!(records(&first.structures), records(&second.structures));
}

#[test]
fn village_generation_structure_zero_preserves_terrain_and_ecology_controls() {
    let settings = settings();
    let fields = TerrainFields::new(71839);
    let witness = WITNESSES[4];
    let village = natural_village(witness.0);
    let region = window(village.center, 24);
    let enabled = prepare(region, &settings, || false).unwrap();
    let disabled_settings = VoxelLandscapeSettings {
        structures_percent: 0,
        ..settings.clone()
    };
    assert!(
        villages::candidate(witness.0, &fields, &disabled_settings, || false)
            .unwrap()
            .is_none()
    );
    let disabled = prepare(region, &disabled_settings, || false).unwrap();
    assert_eq!(enabled.columns, disabled.columns);
    assert_eq!(enabled.biomes, disabled.biomes);
    assert!(!disabled.blocks.values().any(|block| matches!(&block.owner,
        SourceOwner::Structure { source, .. } if source.starts_with("homage:village/"))));
    assert!(entities(&disabled, region, true).is_empty());
    assert!(!disabled
        .structures
        .iter()
        .any(|record| record.source.starts_with("minecraft:village_")));
    let bare_settings = VoxelLandscapeSettings {
        vegetation_percent: 0,
        ..disabled_settings
    };
    let bare = prepare(region, &bare_settings, || false).unwrap();
    assert_eq!(bare.columns, disabled.columns);
    assert_eq!(bare.biomes, disabled.biomes);
    assert!(bare.trees.is_empty() && bare.flora.is_empty());
    assert!(!bare.blocks.values().any(|block| matches!(
        block.owner,
        SourceOwner::Tree { .. } | SourceOwner::TreeDecoration { .. } | SourceOwner::Flora { .. }
    )));
}

#[test]
fn village_generation_full_prepared_air_conflict_outside_the_halo_rejects_the_owner() {
    let settings = settings();
    let fields = TerrainFields::new(71839);
    let village = natural_village(WITNESSES[4].0);
    let blocked = *village
        .writes
        .iter()
        .find(|(_, write)| write.state.is_none())
        .unwrap()
        .0;
    let visible = [blocked[0] + 512, blocked[1], blocked[2]];
    let anchor = [blocked[0] + 256, blocked[1], blocked[2]];
    let region = Region {
        minimum: [visible[0], visible[1]],
        maximum: [visible[0] + 1, visible[1] + 1],
    };
    let mut evaluated = BTreeSet::new();
    let mut reserved = BTreeSet::new();
    let first = region.minimum.map(|value| (value - 192).div_euclid(256));
    let last = region.maximum.map(|value| (value + 192).div_euclid(256));
    for gy in first[1]..=last[1] {
        for gx in first[0]..=last[0] {
            evaluated.insert([gx, gy]);
            if let Some(other) =
                villages::candidate([gx, gy], &fields, &settings, || false).unwrap()
            {
                reserved.extend(other.writes.keys().copied());
            }
        }
    }
    assert!(!evaluated.contains(&WITNESSES[4].0));
    let template = Template {
        source: "homage:test/whole_landmark".into(),
        cells: vec![
            TemplateCell {
                position: [-256, 0, 0],
                state: None,
            },
            TemplateCell {
                position: [256, 0, 0],
                state: Some(plain("minecraft:stone").unwrap()),
            },
        ],
    };
    let prepared = Prepared::prepare(
        &template,
        anchor,
        0,
        |state, _| Ok(state.clone()),
        |_| Habitat::Replaceable,
        || false,
    )
    .unwrap();
    let visible_cells: Vec<_> = prepared
        .project(region.minimum.map(i64::from), region.maximum.map(i64::from))
        .unwrap()
        .collect();
    assert_eq!(visible_cells.len(), 1);
    assert!(visible_cells
        .iter()
        .all(|(position, _)| !reserved.contains(position)));
    assert!(!reserved.contains(&blocked));
    reserve_village_footprint_owners(
        prepared.cells().map(|(position, _)| position),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| false,
    )
    .unwrap();
    assert!(village
        .writes
        .keys()
        .all(|position| reserved.contains(position)));
    assert!(!reserved.contains(&visible));
    let admission = Prepared::prepare(
        &template,
        anchor,
        0,
        |state, _| Ok(state.clone()),
        |position| {
            if reserved.contains(&position) {
                Habitat::Protected
            } else {
                Habitat::Replaceable
            }
        },
        || false,
    );
    assert!(matches!(admission, Err(PlacementError::Protected(position)) if position == blocked));
    // This exercises the actual helper and atomic Prepared admission using its
    // maximum legal span; it is not a fabricated native landmark-family receipt.
}

#[test]
fn village_generation_owner_lookup_caches_some_none_and_validates_before_reservation() {
    let settings = settings();
    let fields = TerrainFields::new(71839);
    let village = natural_village(WITNESSES[4].0);
    let positions: Vec<_> = village.writes.keys().take(3).copied().collect();
    let mut evaluated = BTreeSet::new();
    let mut reserved = BTreeSet::new();
    let calls = Cell::new(0_usize);
    reserve_village_footprint_owners(
        positions.iter().copied(),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| {
            calls.set(calls.get() + 1);
            false
        },
    )
    .unwrap();
    let total = calls.get();
    assert!(total > village.writes.len());
    for stop in [1, 2, total - 1] {
        let (mut aborted_grids, mut aborted_cells) = (BTreeSet::new(), BTreeSet::new());
        calls.set(0);
        assert!(
            matches!(
                reserve_village_footprint_owners(
                    positions.iter().copied(),
                    &mut aborted_grids,
                    &mut aborted_cells,
                    &fields,
                    &settings,
                    &|| {
                        calls.set(calls.get() + 1);
                        calls.get() == stop
                    }
                ),
                Err(AssetError::Cancelled)
            ),
            "helper checkpoint {stop}/{total}"
        );
        if stop <= 2 {
            assert!(aborted_grids.is_empty() && aborted_cells.is_empty());
        }
    }
    // Mid-call helper ledgers are private to an aborted world; retry starts fresh.
    let (mut retry_grids, mut retry_cells) = (BTreeSet::new(), BTreeSet::new());
    reserve_village_footprint_owners(
        positions.iter().copied(),
        &mut retry_grids,
        &mut retry_cells,
        &fields,
        &settings,
        &|| false,
    )
    .unwrap();
    assert_eq!(retry_grids, evaluated);
    assert_eq!(retry_cells, reserved);
    assert!(evaluated.contains(&WITNESSES[4].0));
    assert_eq!(reserved, village.writes.keys().copied().collect());
    let polls = Cell::new(0);
    reserve_village_footprint_owners(
        positions.iter().copied(),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| {
            polls.set(polls.get() + 1);
            polls.get() > 32
        },
    )
    .unwrap();
    assert!(polls.get() <= 32, "cached owner was assembled again");
    assert_eq!(hash2(71839 ^ 0x0076_696c_6c61_6765, 0, 0) % 100, 83);
    assert!(villages::candidate([0, 0], &fields, &settings, || false)
        .unwrap()
        .is_none());
    polls.set(0);
    reserve_village_footprint_owners(
        [[128, 128, 80]].into_iter(),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| {
            polls.set(polls.get() + 1);
            false
        },
    )
    .unwrap();
    let first_none_polls = polls.get();
    polls.set(0);
    reserve_village_footprint_owners(
        [[128, 128, 80]].into_iter(),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| {
            polls.set(polls.get() + 1);
            false
        },
    )
    .unwrap();
    assert!(
        polls.get() < first_none_polls,
        "cached None candidate was reevaluated"
    );
    assert!(evaluated.contains(&[0, 0]));
    let previous_grids = evaluated.clone();
    let previous_cells = reserved.clone();
    for positions in [
        Vec::new(),
        vec![[0, 0, 80]; 65_537],
        vec![[i32::MIN, 0, 80], [i32::MAX, 0, 80]],
        vec![[0, 0, 80], [513, 0, 80]],
    ] {
        assert!(matches!(
            reserve_village_footprint_owners(
                positions.into_iter(),
                &mut evaluated,
                &mut reserved,
                &fields,
                &settings,
                &|| false
            ),
            Err(AssetError::InvalidMetadata(_))
        ));
        assert_eq!(evaluated, previous_grids);
        assert_eq!(reserved, previous_cells);
    }
    let nine: Vec<[i32; 3]> = [-512, -256, 0]
        .into_iter()
        .flat_map(|x| [-512, -256, 0].into_iter().map(move |y| [x, y, 80]))
        .collect();
    evaluated.extend(
        nine.iter()
            .map(|position| [position[0].div_euclid(256), position[1].div_euclid(256)]),
    );
    reserve_village_footprint_owners(
        nine.into_iter(),
        &mut evaluated,
        &mut reserved,
        &fields,
        &settings,
        &|| false,
    )
    .unwrap();
    assert_eq!(reserved, previous_cells);
}

#[test]
fn village_generation_cancellation_returns_no_world_and_fresh_retry_is_identical() {
    let settings = settings();
    let region = window(WITNESSES[4].1, 16);
    let polls = Cell::new(0_usize);
    let clean = prepare(region, &settings, || {
        polls.set(polls.get() + 1);
        false
    })
    .unwrap();
    let total = polls.get();
    assert!(total > 100);
    for stop in [1, total / 3, total - 1, total] {
        polls.set(0);
        assert!(
            matches!(
                prepare(region, &settings, || {
                    polls.set(polls.get() + 1);
                    polls.get() == stop
                }),
                Err(AssetError::Cancelled)
            ),
            "missed checkpoint {stop}/{total}"
        );
    }
    let retry = prepare(region, &settings, || false).unwrap();
    assert_projection(&clean, &retry, region);
    assert_eq!(
        entities(&clean, region, false),
        entities(&retry, region, false)
    );
}
