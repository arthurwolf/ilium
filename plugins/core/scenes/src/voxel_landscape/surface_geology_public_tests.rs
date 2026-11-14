//! Public-output regressions using only APIs present before geology B1.
//! Discovery is bounded and provisional until executed; no coordinate is a claimed receipt.
use super::*;
use std::sync::OnceLock;

pub(super) const TARGETS: [SurfaceBiome; 8] = [
    SurfaceBiome::Badlands,
    SurfaceBiome::ErodedBadlands,
    SurfaceBiome::WoodedBadlands,
    SurfaceBiome::StonyPeaks,
    SurfaceBiome::WindsweptGravellyHills,
    SurfaceBiome::Forest,
    SurfaceBiome::Taiga,
    SurfaceBiome::Desert,
];
pub(super) type Catalog = [Vec<[i32; 2]>; 8];
pub(super) type OwnedCells = BTreeMap<[i32; 3], (BlockState, SourceOwner)>;

pub(super) fn bare_settings(seed: u32) -> VoxelLandscapeSettings {
    VoxelLandscapeSettings {
        seed,
        rivers: false,
        vegetation_percent: 0,
        ..Default::default()
    }
}
pub(super) fn square(center: [i32; 2], radius: i32) -> Region {
    Region {
        minimum: center.map(|v| v - radius),
        maximum: center.map(|v| v + radius),
    }
}
pub(super) fn owned(world: &SurfaceWorld, region: Region) -> OwnedCells {
    world
        .blocks
        .iter()
        .filter(|(p, _)| region.contains(**p))
        .map(|(p, b)| (*p, (b.state.clone(), b.owner.clone())))
        .collect()
}
pub(super) fn centers(seed: u32) -> &'static Catalog {
    static FIRST: OnceLock<Catalog> = OnceLock::new();
    static SECOND: OnceLock<Catalog> = OnceLock::new();
    let slot = match seed {
        71839 => &FIRST,
        31 => &SECOND,
        _ => panic!("unlisted discovery seed"),
    };
    slot.get_or_init(|| {
        let fields = TerrainFields::new(u64::from(seed));
        let settings = bare_settings(seed);
        let mut found: Catalog = std::array::from_fn(|_| Vec::new());
        for gy in -128..=128 {
            for gx in -128..=128 {
                let xy = [gx * 128, gy * 128];
                let (sample, biome) = sample_ground(&fields, &settings, xy[0], xy[1]);
                let Some(index) = TARGETS.iter().position(|b| *b == biome) else {
                    continue;
                };
                if found[index].len() == 8 || sample.water_level.is_some() {
                    continue;
                }
                if found[index]
                    .iter()
                    .any(|p| (p[0] - xy[0]).abs() < 256 && (p[1] - xy[1]).abs() < 256)
                {
                    continue;
                }
                found[index].push(xy);
            }
            if found.iter().all(|row| row.len() == 8) {
                break;
            }
        }
        for (biome, row) in TARGETS.iter().zip(&found) {
            assert!(
                !row.is_empty(),
                "bounded discovery found no {} for seed={seed}",
                biome.id()
            );
            eprintln!(
                "geology_discovery seed={seed} biome={} centers={row:?}",
                biome.id()
            );
        }
        found
    })
}

#[test]
fn emitted_badlands_require_colored_horizontal_strata_and_wooded_soil() {
    for seed in [71839, 31] {
        let settings = bare_settings(seed);
        let mut by_height = BTreeMap::new();
        let mut low_sand = 0;
        let mut high_soils = BTreeSet::new();
        for (index, &biome) in TARGETS[..3].iter().enumerate() {
            let mut colors = BTreeSet::new();
            let mut heights = BTreeSet::new();
            for &center in &centers(seed)[index] {
                let world = prepare(square(center, 24), &settings, || false).unwrap();
                for (&position, block) in &world.blocks {
                    if block.owner != (SourceOwner::Terrain { biome }) {
                        continue;
                    }
                    let ground = i32::from(world.columns[&[position[0], position[1]]].height) - 1;
                    let id = block.state.id().as_str();
                    heights.insert(ground);
                    if position[2] == ground {
                        if ground < 100 && id == "minecraft:red_sand" {
                            low_sand += 1;
                        }
                        if biome == SurfaceBiome::WoodedBadlands
                            && ground >= 100
                            && matches!(id, "minecraft:grass_block" | "minecraft:coarse_dirt")
                        {
                            high_soils.insert(id.to_owned());
                        }
                        continue;
                    }
                    if biome == SurfaceBiome::WoodedBadlands && id == "minecraft:dirt" {
                        continue;
                    }
                    assert!(
                        id.ends_with("terracotta"),
                        "stone cliff at seed={seed} {position:?}: {id}"
                    );
                    colors.insert(id.to_owned());
                    if let Some(previous) = by_height.insert(position[2], id.to_owned()) {
                        assert_eq!(
                            previous, id,
                            "band changed horizontally at seed={seed} {position:?}"
                        );
                    }
                }
            }
            assert!(
                colors.len() >= 3,
                "{} seed={seed}: colors={colors:?}",
                biome.id()
            );
            assert!(heights.len() >= 2, "varied-height witness required");
        }
        assert!(low_sand > 0, "seed={seed} lacks emitted sandy lowlands");
        assert_eq!(
            high_soils,
            BTreeSet::from([
                "minecraft:grass_block".into(),
                "minecraft:coarse_dirt".into()
            ])
        );
    }
}

fn rocky_counts(seed: u32, index: usize) -> (BTreeMap<String, usize>, usize) {
    let biome = TARGETS[index];
    let mut counts = BTreeMap::new();
    let mut adjacent = 0;
    let sought = if index == 3 {
        "minecraft:calcite"
    } else {
        "minecraft:gravel"
    };
    for &center in &centers(seed)[index] {
        let world = prepare(square(center, 24), &bare_settings(seed), || false).unwrap();
        let mut tops = BTreeMap::new();
        for (&xy, sample) in &world.columns {
            let p = [xy[0], xy[1], i32::from(sample.height) - 1];
            let Some(block) = world.blocks.get(&p) else {
                continue;
            };
            if block.owner != (SourceOwner::Terrain { biome }) {
                continue;
            }
            let id = block.state.id().as_str();
            *counts.entry(id.to_owned()).or_insert(0) += 1;
            tops.insert(xy, id);
        }
        adjacent += tops
            .iter()
            .filter(|(xy, id)| {
                **id == sought
                    && [[xy[0] + 1, xy[1]], [xy[0], xy[1] + 1]]
                        .iter()
                        .any(|p| tops.get(p).copied() == Some(sought))
            })
            .count();
    }
    eprintln!(
        "rocky_receipt seed={seed} biome={} counts={counts:?} adjacent={adjacent}",
        biome.id()
    );
    (counts, adjacent)
}
#[test]
fn emitted_stony_peaks_require_calcite_seams_among_stone() {
    for seed in [71839, 31] {
        let (counts, adjacent) = rocky_counts(seed, 3);
        assert!(counts.get("minecraft:calcite").copied().unwrap_or(0) >= 16);
        assert!(counts.get("minecraft:stone").copied().unwrap_or(0) >= 16);
        assert!(adjacent >= 8, "calcite must form adjacent surface cells");
    }
}
#[test]
fn emitted_gravelly_hills_require_gravel_majority_and_other_patches() {
    for seed in [71839, 31] {
        let (counts, adjacent) = rocky_counts(seed, 4);
        let gravel = counts.get("minecraft:gravel").copied().unwrap_or(0);
        assert!(
            gravel * 2 > counts.values().sum::<usize>(),
            "seed={seed}: {counts:?}"
        );
        assert!(counts.get("minecraft:stone").copied().unwrap_or(0) > 0);
        assert!(counts.get("minecraft:grass_block").copied().unwrap_or(0) > 0);
        assert!(adjacent * 4 >= gravel, "gravel must form coherent patches");
    }
}

#[test]
fn actual_mega_conifers_replace_ground_with_owned_podzol() {
    for (center, source) in [
        ([-11904, -16128], "minecraft:mega_pine"),
        ([-13440, -16384], "minecraft:mega_spruce"),
    ] {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        let world = prepare(square(center, 32), &settings, || false).unwrap();
        assert_eq!(
            owned(&world, world.region).len(),
            world.blocks.len(),
            "source escaped its requested region"
        );
        let mut changed = 0;
        for (&p, block) in &world.blocks {
            if block.state.id().as_str() != "minecraft:podzol" {
                continue;
            }
            let SourceOwner::TreeDecoration {
                anchor,
                configuration,
            } = &block.owner
            else {
                continue;
            };
            if *configuration != source {
                continue;
            }
            assert_eq!(p[2], i32::from(world.columns[&[p[0], p[1]]].height) - 1);
            assert_eq!(block.state.property("snowy"), Some("false"));
            assert!(world
                .trees
                .iter()
                .any(|r| r.anchor == *anchor && r.source == source && r.projected_cells > 0));
            changed += 1;
        }
        assert!(
            changed > 0,
            "actual ground-height adapter emitted no {source} podzol"
        );
    }
}

pub(super) fn natural_fossil_witness() -> (u32, [i32; 2], [i32; 2]) {
    static WITNESS: OnceLock<(u32, [i32; 2], [i32; 2])> = OnceLock::new();
    *WITNESS.get_or_init(|| {
        // Exact policy census407 supplies a finite eligibility shortlist only.
        // Broad square relief rejects valid narrow footprints. Neither the
        // census nor this shortlist proves publication: prepare must emit the
        // actual owner, and callers still verify its whole skeleton/footprint.
        // Only original public APIs are used, so this test also runs on RED.
        const CANDIDATES: [(u32, [i32; 2], [i32; 2]); 10] = [
            (71839, [132, -207], [8457, -13262]),
            (71839, [43, 19], [2741, 1210]),
            (71839, [-80, 187], [-5128, 11974]),
            (31, [-114, -248], [-7290, -15883]),
            (31, [107, -129], [6836, -8243]),
            (31, [-108, -113], [-6901, -7224]),
            (31, [-195, -97], [-12478, -6197]),
            (31, [111, 64], [7089, 4112]),
            (31, [-181, 84], [-11599, 5383]),
            (31, [209, 233], [13363, 14899]),
        ];
        for (index, (seed, grid, xy)) in CANDIDATES.into_iter().enumerate() {
            let h = hash2(u64::from(seed) ^ 0x666f_7373_696c_7631, i64::from(grid[0]), i64::from(grid[1]));
            assert!(h.is_multiple_of(8), "shortlist lost its actual global owner gate");
            assert_eq!(xy, [grid[0] * 64 + (h.rotate_left(17) % 33) as i32 - 16,
                grid[1] * 64 + (h.rotate_left(36) % 33) as i32 - 16]);
            let settings = bare_settings(seed);
            let fields = TerrainFields::new(u64::from(seed));
            let (center, biome) = sample_ground(&fields, &settings, xy[0], xy[1]);
            assert_eq!(biome, SurfaceBiome::Desert);
            assert!(center.water_level.is_none());
            let world = prepare(square(xy, 16), &settings, || false).unwrap();
            let published = world.blocks.values().any(|b| matches!(b.owner,
                SourceOwner::Geology { anchor, source: "ilium:exposed_desert_fossil/v1" }
                    if anchor[..2] == xy));
            eprintln!("natural_fossil_candidate seed={seed} grid={grid:?} center={xy:?} preparation={} emitted_owner={published}", index + 1);
            if !published { continue; }
            eprintln!("natural_fossil_receipt seed={seed} grid={grid:?} center={xy:?} preparations={}", index + 1);
            return (seed, grid, xy);
        }
        panic!("bounded natural fossil discovery found no emitted owner in ten independently audited eligible candidates");
    })
}
#[test]
fn prepare_emits_a_natural_exposed_fossil_owner() {
    let (seed, _, xy) = natural_fossil_witness();
    let world = prepare(square(xy, 16), &bare_settings(seed), || false).unwrap();
    let bones: Vec<_> = world.blocks.iter().filter(|(_, b)| matches!(b.owner,
        SourceOwner::Geology { anchor, source: "ilium:exposed_desert_fossil/v1" } if anchor[..2] == xy)).collect();
    assert!(bones.len() >= 30);
    for (&p, block) in bones {
        assert_eq!(block.state.id().as_str(), "minecraft:bone_block");
        assert!(matches!(
            block.state.property("axis"),
            Some("x" | "y" | "z")
        ));
        assert_eq!(world.biomes[&[p[0], p[1]]], SurfaceBiome::Desert);
        assert!(p[2] >= i32::from(world.columns[&[p[0], p[1]]].height));
        assert!(!world.fluids.contains_key(&p));
    }
}

#[test]
fn emitted_columns_keep_original_fields_and_cold_geology() {
    for (center, ice_source) in [([-11776, -15616], true), ([8960, -15872], false)] {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        let world = prepare(square(center, 32), &settings, || false).unwrap();
        let fields = TerrainFields::new(71839);
        let mut wet = 0;
        for (&xy, sample) in &world.columns {
            assert_eq!(*sample, fields.sample(xy[0], xy[1], true));
            if sample
                .water_level
                .is_some_and(|level| level > sample.height)
            {
                assert_eq!(
                    habitat(&fields, &settings, [xy[0], xy[1], i32::from(sample.height)]),
                    HabitatCell::Water
                );
                wet += 1;
            }
        }
        if !ice_source {
            assert!(wet > 2500, "water habitat control was vacuous");
        }
        let count = world
            .blocks
            .iter()
            .filter(|(_, b)| {
                if ice_source {
                    matches!(
                        b.owner,
                        SourceOwner::Geology {
                            source: "minecraft:ice_spike",
                            ..
                        }
                    )
                } else {
                    b.state.id().as_str() == "minecraft:ice"
                }
            })
            .count();
        assert!(count > if ice_source { 100 } else { 2500 });
        assert!(world.fluids.keys().all(|p| !world.blocks.contains_key(p)));
    }
    for seed in [71839, 31] {
        for index in [0, 7] {
            let world = prepare(
                square(centers(seed)[index][0], 16),
                &bare_settings(seed),
                || false,
            )
            .unwrap();
            let fields = TerrainFields::new(u64::from(seed));
            for (&xy, sample) in &world.columns {
                assert_eq!(*sample, fields.sample(xy[0], xy[1], false));
            }
        }
    }
}
