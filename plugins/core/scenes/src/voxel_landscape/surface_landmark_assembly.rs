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

// Original bounded adaptation for this fixed 21x21 exterior only. Heights here
// are exclusive; the returned anchor Z is the actual authored floor block.
const PYRAMID_SIDE: i32 = 21;
const PYRAMID_WORK: usize = 21 * 21 * 3;

fn pyramid_dry(sample: &super::terrain_fields::TerrainSample) -> bool {
    (1..=257).contains(&sample.height)
        && sample
            .water_level
            .is_none_or(|water| water <= sample.height)
}

fn assemble_pyramid(
    anchor: [i32; 3],
    seed: u64,
    sample: impl Fn([i32; 2]) -> Option<super::terrain_fields::TerrainSample>,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<LandmarkPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if !(0..=256).contains(&anchor[2])
        || anchor[..2]
            .iter()
            .any(|v| v.unsigned_abs() > (i32::MAX - 512) as u32)
    {
        return Ok(None);
    }
    let mut columns = BTreeMap::new();
    let mut heights = Vec::with_capacity(441);
    let (mut lower, mut upper): (i32, i32) = (1, 246);
    for y in 0..PYRAMID_SIDE {
        for x in 0..PYRAMID_SIDE {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let xy = [anchor[0] + x, anchor[1] + y];
            let Some(ground) = sample(xy).filter(pyramid_dry) else {
                return Ok(None);
            };
            let height = i32::from(ground.height);
            // At most two removed solids ABOVE the floor, six fill blocks BELOW
            // it. The template's existing floor is not counted as earthwork.
            lower = lower.max(height - 3);
            upper = upper.min(height + 6);
            if let Some(water) = ground.water_level {
                lower = lower.max(i32::from(water) - 1);
            }
            heights.push(height);
            columns.insert(xy, ground);
        }
    }
    heights.sort_unstable();
    if heights[440] - heights[0] > 8 {
        return Ok(None);
    }
    for (&[x, y], ground) in &columns {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        for neighbor in [[x - 1, y], [x, y - 1]] {
            if columns
                .get(&neighbor)
                .is_some_and(|other| (i32::from(ground.height) - i32::from(other.height)).abs() > 2)
            {
                return Ok(None);
            }
        }
    }
    // The actual three-wide north doorway must meet unchanged dry ground with
    // at most a one-block step. These read-only approach columns add no writes.
    for x in 9..=11 {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let Some(ground) = sample([anchor[0] + x, anchor[1] - 1]).filter(pyramid_dry) else {
            return Ok(None);
        };
        let height = i32::from(ground.height);
        lower = lower.max(height - 2);
        upper = upper.min(height);
    }
    if lower > upper {
        return Ok(None);
    }
    // The approach interval contains at most three integer floors. Minimize
    // actual cut/fill work; ties prefer the nominal anchor, then the lower floor.
    let Some((work, _, floor)) = (lower..=upper)
        .map(|floor| {
            let work: usize = heights
                .iter()
                .map(|height| ((floor - height).max(0) + (height - floor - 1).max(0)) as usize)
                .sum();
            (work, floor.abs_diff(anchor[2]), floor)
        })
        .min()
    else {
        return Ok(None);
    };
    if work > PYRAMID_WORK {
        return Ok(None);
    }
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let mut kit =
        surface_landmarks::build(LandmarkKind::DesertPyramid, seed, state).map_err(place_error)?;
    let foundation = state("minecraft:sandstone", &[]).map_err(place_error)?;
    let mut cells = BTreeMap::new();
    for cell in kit.template.cells {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if !(0..21).contains(&cell.position[0])
            || !(0..21).contains(&cell.position[1])
            || !(0..=10).contains(&cell.position[2])
        {
            return Err(place_error(PlacementError::Bounds));
        }
        if cells.insert(cell.position, cell.state).is_some() {
            return Err(place_error(PlacementError::Duplicate(cell.position)));
        }
    }
    for y in 0..PYRAMID_SIDE {
        for x in 0..PYRAMID_SIDE {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            if !cells.get(&[x, y, 0]).is_some_and(Option::is_some) {
                return Err(place_error(PlacementError::Bounds));
            }
            let height = i32::from(columns[&[anchor[0] + x, anchor[1] + y]].height);
            for z in height..floor {
                cells.insert([x, y, z - floor], Some(foundation.clone()));
            }
            // Preserve every authored solid AND air cell. Fill only absent air
            // in the bounding prism, preventing dunes outside the inset shell
            // from surviving the cut or later plants from occupying that space.
            for z in 1..=10 {
                cells.entry([x, y, z]).or_insert(None);
            }
        }
    }
    kit.template.cells = cells
        .into_iter()
        .map(|(position, state)| surface_structures::TemplateCell { position, state })
        .collect();
    let anchor = [anchor[0], anchor[1], floor];
    let prepared = match Prepared::prepare(
        &kit.template,
        anchor,
        0,
        rotated_state,
        |p| {
            if !(0..=256).contains(&p[2]) || !columns.contains_key(&[p[0], p[1]]) {
                Habitat::Unknown
            } else {
                Habitat::Replaceable
            }
        },
        cancelled,
    ) {
        Ok(prepared) => prepared,
        Err(PlacementError::Cancelled) => return Err(AssetError::Cancelled),
        Err(
            e @ (PlacementError::InvalidState
            | PlacementError::InvalidSource
            | PlacementError::Budget),
        ) => {
            return Err(place_error(e));
        }
        Err(_) => return Ok(None),
    };
    let mut writes = BTreeMap::new();
    for (position, value) in prepared.cells() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        writes.insert(position, value.cloned());
    }
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    Ok(Some(LandmarkPlacement {
        kind: LandmarkKind::DesertPyramid,
        anchor,
        source: prepared.source().to_owned(),
        writes,
        occupants: Vec::new(),
    }))
}

/// Prepare all source cells before any camera-window projection. `anchor` is
/// the nominal sampled elevation; a desert pyramid returns its fitted floor Z.
pub fn assemble(
    kind: LandmarkKind,
    anchor: [i32; 3],
    seed: u64,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<LandmarkPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if !(0..=256).contains(&anchor[2]) {
        return Ok(None);
    }
    if anchor[0].unsigned_abs() > (i32::MAX - 512) as u32
        || anchor[1].unsigned_abs() > (i32::MAX - 512) as u32
    {
        return Ok(None);
    }
    if kind == LandmarkKind::DesertPyramid {
        return assemble_pyramid(
            anchor,
            seed,
            |[x, y]| Some(fields.sample(x, y, settings.rivers)),
            &cancelled,
        );
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

#[cfg(test)]
mod foundation_tests {
    use super::super::{
        surface_generation::{self, Region, SourceOwner, SurfaceWorld},
        terrain_fields::TerrainSample,
    };
    use super::*;
    use std::cell::Cell;

    fn dry(height: i16) -> TerrainSample {
        let mut ground = TerrainFields::new(71839).sample(0, 0, false);
        ground.height = height;
        ground.water_level = None;
        ground
    }

    #[test]
    fn pyramid_cut_fill_keeps_every_authored_state_and_air_cell() {
        let ground = dry(80);
        let placement = assemble_pyramid(
            [-16, -16, 80],
            19,
            |[x, _]| {
                Some(TerrainSample {
                    height: 80 + ((x + 16) / 4) as i16,
                    ..ground
                })
            },
            &|| false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(placement.anchor, [-16, -16, 82]);
        let kit = surface_landmarks::build(LandmarkKind::DesertPyramid, 19, state).unwrap();
        assert_eq!(placement.source, kit.template.source);
        for cell in kit.template.cells {
            let p = std::array::from_fn(|axis| placement.anchor[axis] + cell.position[axis]);
            assert_eq!(placement.writes.get(&p), Some(&cell.state));
        }
        for y in -16..5 {
            for x in -16..5 {
                let height = 80 + (x + 16) / 4;
                for z in height..82 {
                    assert_eq!(
                        placement.writes[&[x, y, z]].as_ref().unwrap().id().as_str(),
                        "minecraft:sandstone"
                    );
                }
                assert!(placement.writes[&[x, y, 82]].is_some());
                for z in 83..=92 {
                    assert!(placement.writes.contains_key(&[x, y, z]));
                }
            }
        }
        assert_eq!(placement.writes[&[4, 4, 83]], None);
        assert_eq!(placement.writes[&[4, 4, 84]], None);
        assert!(placement
            .writes
            .iter()
            .all(|(p, value)| p[2] >= 82 || value.is_some()));
        assert!(placement
            .writes
            .keys()
            .all(|p| (-16..5).contains(&p[0]) && (-16..5).contains(&p[1])));
        assert!(placement.writes.len() <= 21 * 21 * 11 + PYRAMID_WORK);
    }

    #[test]
    fn pyramid_rejects_unknown_wet_cliffs_relief_access_and_earthwork_overruns() {
        let ground = dry(80);
        for (case, label) in [
            (0, "unknown last column"),
            (1, "wet last column"),
            (2, "four-block local cliff"),
            (3, "ten-block relief"),
            (4, "approach requires excessive cut"),
            (5, "approach requires excessive fill"),
            (6, "volume exceeds budget"),
            (7, "unknown approach"),
            (8, "no supporting ground"),
            (9, "roof exceeds height budget"),
        ] {
            let value = assemble_pyramid(
                [0, 0, 80],
                19,
                |xy| {
                    let mut t = ground;
                    match case {
                        0 if xy == [20, 20] => return None,
                        1 if xy == [20, 20] => t.water_level = Some(81),
                        2 if xy == [20, 20] => t.height = 84,
                        3 => t.height = 80 + (xy[0] / 2) as i16,
                        4 if xy[1] == -1 => t.height = 70,
                        5 if xy[1] == -1 => t.height = 89,
                        6 if xy[1] == -1 => t.height = 86,
                        7 if xy[1] == -1 => return None,
                        8 => t.height = 0,
                        9 => t.height = 249,
                        _ => {}
                    }
                    Some(t)
                },
                &|| false,
            )
            .unwrap();
            assert!(value.is_none(), "{label}");
        }
        let mut at_water = ground;
        at_water.water_level = Some(at_water.height);
        assert!(
            assemble_pyramid([0, 0, 80], 19, |_| Some(at_water), &|| false)
                .unwrap()
                .is_some()
        );
        let high = dry(246);
        let p = assemble_pyramid([0, 0, 246], 19, |_| Some(high), &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(p.writes.keys().map(|p| p[2]).max(), Some(256));
    }

    #[test]
    fn pyramid_cancellation_and_signed_bounds_never_publish_partial_placements() {
        let ground = dry(80);
        let calls = Cell::new(0usize);
        assert!(assemble_pyramid([0, 0, 80], 19, |_| Some(ground), &|| {
            calls.set(calls.get() + 1);
            false
        })
        .unwrap()
        .is_some());
        let total = calls.get();
        for stop in [1, 100, total / 2, total - 1] {
            calls.set(0);
            assert!(matches!(
                assemble_pyramid([0, 0, 80], 19, |_| Some(ground), &|| {
                    calls.set(calls.get() + 1);
                    calls.get() >= stop
                }),
                Err(AssetError::Cancelled)
            ));
        }
        let fields = TerrainFields::new(71839);
        let settings = VoxelLandscapeSettings::default();
        for anchor in [
            [i32::MAX, 0, 80],
            [i32::MIN, 0, 80],
            [0, i32::MAX, 80],
            [0, i32::MIN, 80],
            [0, 0, -1],
            [0, 0, 257],
        ] {
            assert!(assemble(
                LandmarkKind::DesertPyramid,
                anchor,
                19,
                &fields,
                &settings,
                || false
            )
            .unwrap()
            .is_none());
        }
    }

    fn assert_projection(world: &SurfaceWorld, placement: &LandmarkPlacement) {
        let owner = SourceOwner::Structure {
            anchor: placement.anchor,
            source: placement.source.clone(),
        };
        let mut cells = 0;
        let mut solids = 0;
        for (position, expected) in &placement.writes {
            if !world.region.contains(*position) {
                continue;
            }
            cells += 1;
            assert!(!world.fluids.contains_key(position));
            match expected {
                Some(state) => {
                    solids += 1;
                    let actual = world.blocks.get(position).expect("missing pyramid block");
                    assert_eq!(&actual.state, state);
                    assert_eq!(actual.owner, owner);
                }
                None => assert!(
                    !world.blocks.contains_key(position),
                    "air lost at {position:?}"
                ),
            }
        }
        assert!(cells > 0);
        assert_eq!(
            world
                .blocks
                .values()
                .filter(|block| block.owner == owner)
                .count(),
            solids
        );
        let records: Vec<_> = world
            .structures
            .iter()
            .filter(|record| {
                record.source == "minecraft:desert_pyramid" && record.anchor == placement.anchor
            })
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].projected_cells, cells);
    }

    #[test]
    fn natural_pyramid_foundation_survives_whole_split_and_shifted_worlds() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            structures_percent: 100,
            ..Default::default()
        };
        let fields = TerrainFields::new(u64::from(settings.seed));
        let mut eligible = 0;
        let mut assembled = 0;
        // Provisional bounded witness discovery, not fabricated fixed coordinates.
        // The prefilter avoids constructing unrelated mansions/outposts; BOTH
        // production candidate() and the ordinary whole generator must succeed.
        for gy in -256..=256 {
            for gx in -256..=256 {
                let grid = [gx, gy];
                let hash = hash2(71839 ^ 0x6c61_6e64_6d61_726b, i64::from(gx), i64::from(gy));
                if hash % 100 >= 10 {
                    continue;
                }
                let x = gx * 256 + 128 + (((hash >> 8) & 7) as i32 - 4) * 16;
                let y = gy * 256 + 128 + (((hash >> 11) & 7) as i32 - 4) * 16;
                let origin = fields.sample(x, y, settings.rivers);
                let biome = surface_biome_selector::select(71839, [x, y], origin);
                if biome != SurfaceBiome::Desert
                    || kind_for(biome, hash.rotate_left(17)) != Some(LandmarkKind::DesertPyramid)
                {
                    continue;
                }
                eligible += 1;
                let Some(p) = candidate(grid, &fields, &settings, || false).unwrap() else {
                    continue;
                };
                assembled += 1;
                if x >= 0 && y >= 0 {
                    continue;
                }
                let region = Region {
                    minimum: [x, y],
                    maximum: [x + 21, y + 21],
                };
                let world = surface_generation::prepare(region, &settings, || false).unwrap();
                if !world.structures.iter().any(|record| {
                    record.source == "minecraft:desert_pyramid" && record.anchor == p.anchor
                }) {
                    continue;
                }
                assert_eq!(&p.anchor[..2], &[x, y]);
                assert_projection(&world, &p);
                let mut old_delta = 0;
                let mut work = 0;
                let mut minimum = i32::MAX;
                let mut maximum = i32::MIN;
                for yy in y..y + 21 {
                    for xx in x..x + 21 {
                        let t = fields.sample(xx, yy, settings.rivers);
                        let height = i32::from(t.height);
                        assert!(pyramid_dry(&t));
                        minimum = minimum.min(height);
                        maximum = maximum.max(height);
                        old_delta = old_delta.max((height - i32::from(origin.height)).abs());
                        let fill = (p.anchor[2] - height).max(0);
                        let cut = (height - p.anchor[2] - 1).max(0);
                        assert!(fill <= 6 && cut <= 2);
                        work += (fill + cut) as usize;
                        for z in (height - 1).min(p.anchor[2] - 1)..=p.anchor[2] {
                            assert!(
                                world.blocks.contains_key(&[xx, yy, z]),
                                "unsupported floor {xx},{yy},{z}"
                            );
                        }
                    }
                }
                assert!(
                    old_delta > 2,
                    "witness does not exercise the former rejection"
                );
                assert!(maximum - minimum <= 8 && work <= PYRAMID_WORK);
                let seam = (x.div_euclid(16) + 1) * 16;
                for window in [
                    Region {
                        minimum: region.minimum,
                        maximum: [seam, y + 21],
                    },
                    Region {
                        minimum: [seam, y],
                        maximum: region.maximum,
                    },
                    Region {
                        minimum: [x + 4, y + 3],
                        maximum: [x + 25, y + 24],
                    },
                ] {
                    assert_projection(
                        &surface_generation::prepare(window, &settings, || false).unwrap(),
                        &p,
                    );
                }
                let again = candidate(grid, &fields, &settings, || false)
                    .unwrap()
                    .unwrap();
                assert_eq!(again.anchor, p.anchor);
                assert_eq!(again.source, p.source);
                assert_eq!(again.writes, p.writes);
                assert!(matches!(
                    candidate(grid, &fields, &settings, || true),
                    Err(AssetError::Cancelled)
                ));
                let disabled = VoxelLandscapeSettings {
                    structures_percent: 0,
                    ..settings.clone()
                };
                assert!(candidate(grid, &fields, &disabled, || false)
                    .unwrap()
                    .is_none());
                eprintln!("D4.PYRAMID seed=71839 grid={grid:?} anchor={:?} old_delta={old_delta} relief={} work={work} writes={}", p.anchor, maximum - minimum, p.writes.len());
                return;
            }
        }
        panic!("no whole-generator negative-coordinate pyramid witness within radius256: eligible={eligible}, assembled={assembled}");
    }
}
