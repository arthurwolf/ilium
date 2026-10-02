//! Global admission and projection for the root-owned exterior landmark kit.
//! The sparse grid and terrain adaptation are original Ilium choices. Native
//! structure-set salts, processors and buried portions are not inferred here.
use super::{
    assets::{
        block_state::BlockState,
        error::{AssetError, Result},
        identity::ResourceId,
    },
    noise::hash2,
    settings::VoxelLandscapeSettings,
    surface_biome_selector,
    surface_biomes::SurfaceBiome,
    surface_landmarks::{self, LandmarkKind},
    surface_structures::{self, Habitat, PlacementError, Prepared},
    surface_village_assembly,
    terrain_fields::TerrainFields,
};
use std::collections::BTreeMap;

pub struct LandmarkPlacement {
    pub kind: LandmarkKind,
    pub anchor: [i32; 3],
    pub source: String,
    pub writes: BTreeMap<[i32; 3], Option<BlockState>>,
    pub occupants: Vec<(&'static str, [i32; 3])>,
}

fn state(id: &str, properties: &[(&str, &str)]) -> std::result::Result<BlockState, PlacementError> {
    BlockState::new(
        ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
        properties
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    )
    .map_err(|_| PlacementError::InvalidState)
}

fn rotated_state(block: &BlockState, turns: u8) -> std::result::Result<BlockState, PlacementError> {
    let properties = surface_structures::rotate_properties(block.properties(), turns)?;
    BlockState::new(block.id().clone(), properties).map_err(|_| PlacementError::InvalidState)
}

fn place_error(error: PlacementError) -> AssetError {
    AssetError::InvalidMetadata(format!("surface landmark placement: {error:?}"))
}

fn dry_ground(fields: &TerrainFields, settings: &VoxelLandscapeSettings, x: i32, y: i32) -> bool {
    let sample = fields.sample(x, y, settings.rivers);
    sample
        .water_level
        .is_none_or(|level| level <= sample.height)
}

fn admissible_ground(
    kind: LandmarkKind,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    anchor: [i32; 3],
    position: [i32; 3],
) -> bool {
    let sample = fields.sample(position[0], position[1], settings.rivers);
    if (i32::from(sample.height) - anchor[2]).abs() > 2 {
        return false;
    }
    if kind != LandmarkKind::SwampHut && !dry_ground(fields, settings, position[0], position[1]) {
        return false;
    }
    if kind == LandmarkKind::DesertWell {
        let biome = surface_biome_selector::select(
            u64::from(settings.seed),
            [position[0], position[1]],
            sample,
        );
        if biome != SurfaceBiome::Desert {
            return false;
        }
    }
    true
}

/// Prepare all source cells before any camera-window projection. `anchor` is
/// the kit's local [0,0,0] at the sampled ground elevation.
pub fn assemble(
    kind: LandmarkKind,
    anchor: [i32; 3],
    seed: u64,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<LandmarkPlacement>> {
    if !(0..=256).contains(&anchor[2]) {
        return Ok(None);
    }
    if anchor[0].unsigned_abs() > (i32::MAX - 512) as u32
        || anchor[1].unsigned_abs() > (i32::MAX - 512) as u32
    {
        return Ok(None);
    }
    let mut kit = surface_landmarks::build(kind, seed, state).map_err(place_error)?;
    for local in &kit.entrances {
        let Some(x) = anchor[0].checked_add(local[0]) else {
            return Ok(None);
        };
        let Some(y) = anchor[1].checked_add(local[1]) else {
            return Ok(None);
        };
        if !admissible_ground(kind, fields, settings, anchor, [x, y, anchor[2]]) {
            return Ok(None);
        }
    }
    // Cache the complete footprint before mutation. Sampling per source cell
    // would reevaluate the same terrain column thousands of times for a mansion.
    let mut columns = BTreeMap::new();
    for cell in &kit.template.cells {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let xy = [anchor[0] + cell.position[0], anchor[1] + cell.position[1]];
        if columns.contains_key(&xy) {
            continue;
        }
        if !admissible_ground(kind, fields, settings, anchor, [xy[0], xy[1], anchor[2]]) {
            return Ok(None);
        }
        columns.insert(xy, fields.sample(xy[0], xy[1], settings.rivers));
    }
    let foundation = state(
        match kind {
            LandmarkKind::DesertWell | LandmarkKind::DesertPyramid => "minecraft:sandstone",
            LandmarkKind::IglooTop => "minecraft:snow_block",
            _ => "minecraft:cobblestone",
        },
        &[],
    )
    .map_err(place_error)?;
    let supports: Vec<_> = kit
        .template
        .cells
        .iter()
        .filter(|c| c.position[2] == 0 && c.state.is_some())
        .flat_map(|c| {
            let sample = columns[&[anchor[0] + c.position[0], anchor[1] + c.position[1]]];
            ((i32::from(sample.height))..anchor[2])
                .map(move |z| [c.position[0], c.position[1], z - anchor[2]])
        })
        .collect();
    for position in supports {
        kit.template.cells.push(surface_structures::TemplateCell {
            position,
            state: Some(foundation.clone()),
        });
    }
    // Every callback sees the same immutable fields/settings snapshot.
    let prepared = Prepared::prepare(
        &kit.template,
        anchor,
        0,
        rotated_state,
        |position| {
            if position[2] < 0 || position[2] > 256 {
                return Habitat::Unknown;
            }
            if !columns.contains_key(&[position[0], position[1]]) {
                return Habitat::Protected;
            }
            Habitat::Replaceable
        },
        &cancelled,
    );
    let prepared = match prepared {
        Ok(value) => value,
        Err(PlacementError::Cancelled) => return Err(AssetError::Cancelled),
        Err(
            problem @ (PlacementError::InvalidState
            | PlacementError::InvalidSource
            | PlacementError::Budget),
        ) => return Err(place_error(problem)),
        Err(_) => return Ok(None),
    };
    let mut occupants = Vec::new();
    for (id, local) in kit.occupants {
        let Some(x) = anchor[0].checked_add(local[0]) else {
            return Ok(None);
        };
        let Some(y) = anchor[1].checked_add(local[1]) else {
            return Ok(None);
        };
        let Some(z) = anchor[2].checked_add(local[2]) else {
            return Ok(None);
        };
        occupants.push((id, [x, y, z]));
    }
    let writes = prepared
        .project([i64::from(i32::MIN); 2], [i64::from(i32::MAX) + 1; 2])
        .map_err(place_error)?
        .map(|(position, value)| (position, value.cloned()))
        .collect();
    Ok(Some(LandmarkPlacement {
        kind,
        anchor,
        source: prepared.source().to_owned(),
        writes,
        occupants,
    }))
}

fn kind_for(biome: SurfaceBiome, hash: u64) -> Option<LandmarkKind> {
    // Wells have their own chunk-scale placed-feature admission below;
    // structure presets use complete pinned biome eligibility.
    let ids = biome.descriptor().surface_structure_variant_ids;
    let eligible: Vec<_> = LandmarkKind::ALL
        .into_iter()
        .filter(|kind| *kind != LandmarkKind::DesertWell && ids.contains(&source(*kind)))
        .collect();
    if eligible.is_empty() {
        return None;
    }
    Some(eligible[(hash % eligible.len() as u64) as usize])
}

/// Separate authored chunk-scale admission for the pinned rare placed feature.
pub fn desert_well_anchor(seed: u64, grid: [i32; 2], structures_percent: i32) -> Option<[i32; 2]> {
    let density = structures_percent.clamp(0, 200) as u64;
    let hash = hash2(
        seed ^ 0x6465_7365_7274_7765,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    // Default100 gives1/1000 admission per16-block cell, independently of
    // sparse structures. This is an authored hash, not a native RNG/salt claim.
    if hash % 100_000 >= density {
        return None;
    }
    Some([
        grid[0]
            .checked_mul(16)?
            .checked_add(((hash >> 20) & 15) as i32)?,
        grid[1]
            .checked_mul(16)?
            .checked_add(((hash >> 28) & 15) as i32)?,
    ])
}

pub fn desert_well_candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<LandmarkPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let seed = u64::from(settings.seed);
    let Some([x, y]) = desert_well_anchor(seed, grid, settings.structures_percent) else {
        return Ok(None);
    };
    let sample = fields.sample(x, y, settings.rivers);
    if surface_biome_selector::select(seed, [x, y], sample) != SurfaceBiome::Desert
        || !dry_ground(fields, settings, x, y)
    {
        return Ok(None);
    }
    assemble(
        LandmarkKind::DesertWell,
        [x, y, i32::from(sample.height)],
        hash2(
            seed ^ 0x6465_7365_7274_7765,
            i64::from(grid[0]),
            i64::from(grid[1]),
        ),
        fields,
        settings,
        cancelled,
    )
}

/// One deterministic 256-block grid cell. The seed and complete footprint are
/// independent of the requested render region and selected resource pack.
pub fn candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<LandmarkPlacement>> {
    if settings.structures_percent == 0 {
        return Ok(None);
    }
    let seed = u64::from(settings.seed);
    let hash = hash2(
        seed ^ 0x6c61_6e64_6d61_726b,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if hash % 100 >= u64::from(settings.structures_percent.clamp(0, 200) as u32) / 10 {
        return Ok(None);
    }
    let Some(base_x) = grid[0].checked_mul(256).and_then(|v| v.checked_add(128)) else {
        return Ok(None);
    };
    let Some(base_y) = grid[1].checked_mul(256).and_then(|v| v.checked_add(128)) else {
        return Ok(None);
    };
    let Some(x) = base_x.checked_add(((hash >> 8 & 7) as i32 - 4) * 16) else {
        return Ok(None);
    };
    let Some(y) = base_y.checked_add(((hash >> 11 & 7) as i32 - 4) * 16) else {
        return Ok(None);
    };
    let sample = fields.sample(x, y, settings.rivers);
    let biome = surface_biome_selector::select(seed, [x, y], sample);
    let Some(kind) = kind_for(biome, hash.rotate_left(17)) else {
        return Ok(None);
    };
    if kind != LandmarkKind::SwampHut && sample.water_level.is_some_and(|v| v > sample.height) {
        return Ok(None);
    }
    // Source outposts exclude village centers within ten chunks. This
    // conservative square check applies to Ilium's own village candidates.
    if kind == LandmarkKind::PillagerOutpost {
        for gy in (y.div_euclid(256) - 1)..=(y.div_euclid(256) + 1) {
            for gx in (x.div_euclid(256) - 1)..=(x.div_euclid(256) + 1) {
                if cancelled() {
                    return Err(AssetError::Cancelled);
                }
                if let Some(village) =
                    surface_village_assembly::candidate([gx, gy], fields, settings, &cancelled)?
                {
                    if (village.center[0] - x)
                        .abs()
                        .max((village.center[1] - y).abs())
                        <= 160
                    {
                        return Ok(None);
                    }
                }
            }
        }
    }
    assemble(
        kind,
        [x, y, i32::from(sample.height)],
        hash,
        fields,
        settings,
        cancelled,
    )
}

pub const fn source(kind: LandmarkKind) -> &'static str {
    match kind {
        LandmarkKind::DesertWell => "minecraft:desert_well",
        LandmarkKind::DesertPyramid => "minecraft:desert_pyramid",
        LandmarkKind::JunglePyramid => "minecraft:jungle_pyramid",
        LandmarkKind::SwampHut => "minecraft:swamp_hut",
        LandmarkKind::IglooTop => "minecraft:igloo",
        LandmarkKind::PillagerOutpost => "minecraft:pillager_outpost",
        LandmarkKind::WoodlandMansion => "minecraft:mansion",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desert_wells_have_chunk_scale_rarity_without_landmark_gate() {
        let mut count = 0;
        for y in -100..100 {
            for x in -250..250 {
                if let Some(position) = desert_well_anchor(71839, [x, y], 100) {
                    assert_eq!(position.map(|v| v.div_euclid(16)), [x, y]);
                    count += 1;
                }
            }
        }
        assert!(
            (60..=140).contains(&count),
            "100000 chunk attempts yielded {count} wells"
        );
    }
    #[test]
    fn well_admission_density_is_monotone_and_disabled_at_zero() {
        let mut half = 0;
        let mut full = 0;
        for x in -50000..50000 {
            let grid = [x, -1000];
            assert!(desert_well_anchor(71839, grid, 0).is_none());
            if desert_well_anchor(71839, grid, 50).is_some() {
                half += 1;
                assert!(desert_well_anchor(71839, grid, 100).is_some());
            }
            full += usize::from(desert_well_anchor(71839, grid, 100).is_some());
        }
        assert!(half > 20 && full > half);
        assert!(desert_well_anchor(71839, [i32::MAX, i32::MIN], 200).is_none());
    }
    #[test]
    fn all_43_biomes_use_exact_structure_eligibility_including_mountain_outposts() {
        for &biome in SurfaceBiome::all() {
            let mut observed = std::collections::BTreeSet::new();
            for hash in 0..2000 {
                if let Some(kind) = kind_for(biome, hash) {
                    if kind == LandmarkKind::DesertWell {
                        assert_eq!(biome, SurfaceBiome::Desert);
                    } else {
                        assert!(biome
                            .descriptor()
                            .surface_structure_variant_ids
                            .contains(&source(kind)));
                    }
                    observed.insert(source(kind));
                }
            }
            for kind in LandmarkKind::ALL {
                if kind != LandmarkKind::DesertWell
                    && biome
                        .descriptor()
                        .surface_structure_variant_ids
                        .contains(&source(kind))
                {
                    assert!(
                        observed.contains(source(kind)),
                        "{} missing {}",
                        biome.id(),
                        source(kind)
                    );
                }
            }
        }
        assert_eq!(kind_for(SurfaceBiome::SparseJungle, 1), None);
        assert!(matches!(
            kind_for(SurfaceBiome::SnowySlopes, 0),
            Some(LandmarkKind::IglooTop)
        ));
    }
    #[test]
    fn all_source_kits_prepare_as_whole_bounded_exteriors() {
        for kind in LandmarkKind::ALL {
            let kit = surface_landmarks::build(kind, 17, state).unwrap();
            let prepared = Prepared::prepare(
                &kit.template,
                [100, 100, 70],
                0,
                rotated_state,
                |_| Habitat::Replaceable,
                || false,
            )
            .unwrap();
            let complete: Vec<_> = prepared.project([0, 0], [1000, 1000]).unwrap().collect();
            let left = prepared.project([0, 0], [110, 1000]).unwrap().count();
            let right = prepared.project([110, 0], [1000, 1000]).unwrap().count();
            assert_eq!(left + right, complete.len(), "{} seam", kind.name());
            assert!(
                complete.iter().any(|(_, state)| state.is_some()),
                "{} empty",
                kind.name()
            );
            assert!(complete.iter().all(|(position, _)| {
                (0..3).all(|axis| {
                    position[axis] >= [100, 100, 70][axis] + kit.bounds[0][axis]
                        && position[axis] < [100, 100, 70][axis] + kit.bounds[1][axis]
                })
            }));
        }
    }
}
