//! Whole original ruin admission followed by actual terrain/water exposure.
//! Eligibility uses the pinned biome vocabulary; sparse anchors and burial
//! depths are authored homage choices, not native placement algorithms.
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
    surface_ruins::{self, PortalStyle, RuinKind, SurfaceSample},
    surface_structures::{self, Habitat, PlacementError, Prepared},
    terrain_fields::{Landform, TerrainFields, TerrainSample},
};
use std::collections::BTreeMap;

pub struct RuinPlacement {
    pub kind: RuinKind,
    pub prepared: Prepared<BlockState>,
    pub writes: BTreeMap<[i32; 3], Option<BlockState>>,
}

pub const fn source(kind: RuinKind) -> &'static str {
    match kind {
        RuinKind::Portal(PortalStyle::Standard) => "minecraft:ruined_portal",
        RuinKind::Portal(PortalStyle::Mountain) => "minecraft:ruined_portal_mountain",
        RuinKind::Portal(PortalStyle::Desert) => "minecraft:ruined_portal_desert",
        RuinKind::Portal(PortalStyle::Jungle) => "minecraft:ruined_portal_jungle",
        RuinKind::Portal(PortalStyle::Swamp) => "minecraft:ruined_portal_swamp",
        RuinKind::TrailTop => "minecraft:trail_ruins",
        RuinKind::ExposedFossil => "original:exposed_fossil",
        RuinKind::MesaMineEntrance => "minecraft:mineshaft_mesa",
        RuinKind::BeachedShipwreck => "minecraft:shipwreck_beached",
    }
}

fn placement_error(error: PlacementError) -> AssetError {
    if error == PlacementError::Cancelled {
        AssetError::Cancelled
    } else {
        AssetError::InvalidMetadata(format!("surface ruin placement: {error:?}"))
    }
}

pub fn assemble(
    kind: RuinKind,
    anchor: [i32; 3],
    rotation: u8,
    seed: u64,
    sample: impl Fn([i32; 2]) -> Option<SurfaceSample>,
    cancelled: impl Fn() -> bool,
) -> Result<Option<RuinPlacement>> {
    let ruin = surface_ruins::build(kind, seed, |id, properties| {
        BlockState::new(
            ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
            properties
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        )
        .map_err(|_| PlacementError::InvalidState)
    })
    .map_err(placement_error)?;
    let prepared = Prepared::prepare(
        &ruin.landmark.template,
        anchor,
        rotation,
        |block, turns| {
            BlockState::new(
                block.id().clone(),
                surface_structures::rotate_properties(block.properties(), turns)?,
            )
            .map_err(|_| PlacementError::InvalidState)
        },
        |position| {
            if (0..=256).contains(&position[2]) {
                Habitat::Replaceable
            } else {
                Habitat::Unknown
            }
        },
        &cancelled,
    )
    .map_err(placement_error)?;
    let exposed = surface_ruins::exposed_cells(&prepared, ruin.exposure, sample, &cancelled)
        .map_err(placement_error)?;
    if !exposed.iter().any(|(_, state)| state.is_some()) {
        return Ok(None);
    }
    let writes = exposed
        .into_iter()
        .map(|(position, state)| (position, state.cloned()))
        .collect();
    Ok(Some(RuinPlacement {
        kind,
        prepared,
        writes,
    }))
}

pub fn eligible_kinds(biome: SurfaceBiome, sample: TerrainSample) -> Vec<RuinKind> {
    let ids = biome.descriptor().surface_structure_variant_ids;
    let mut kinds = Vec::with_capacity(4);
    for (source, kind) in [
        (
            "minecraft:ruined_portal",
            RuinKind::Portal(PortalStyle::Standard),
        ),
        (
            "minecraft:ruined_portal_mountain",
            RuinKind::Portal(PortalStyle::Mountain),
        ),
        (
            "minecraft:ruined_portal_desert",
            RuinKind::Portal(PortalStyle::Desert),
        ),
        (
            "minecraft:ruined_portal_jungle",
            RuinKind::Portal(PortalStyle::Jungle),
        ),
        (
            "minecraft:ruined_portal_swamp",
            RuinKind::Portal(PortalStyle::Swamp),
        ),
        ("minecraft:trail_ruins", RuinKind::TrailTop),
        ("minecraft:shipwreck_beached", RuinKind::BeachedShipwreck),
    ] {
        if ids.contains(&source) {
            kinds.push(kind);
        }
    }
    if matches!(
        biome,
        SurfaceBiome::Desert | SurfaceBiome::Swamp | SurfaceBiome::MangroveSwamp
    ) {
        kinds.push(RuinKind::ExposedFossil);
    }
    // A mesa entrance requires an actual exposed valley cut, not merely a
    // badlands label. Underground tunnel geometry is never constructed.
    if matches!(
        biome,
        SurfaceBiome::Badlands | SurfaceBiome::ErodedBadlands | SurfaceBiome::WoodedBadlands
    ) && sample.landform == Landform::Mesa
        && sample.valley_strength > 0.55
    {
        kinds.push(RuinKind::MesaMineEntrance);
    }
    kinds
}

pub fn candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<RuinPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if settings.structures_percent == 0 {
        return Ok(None);
    }
    let seed = u64::from(settings.seed);
    let hash = hash2(
        seed ^ 0x7275_696e_7330_3031,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if hash % 100 >= settings.structures_percent.clamp(0, 200) as u64 / 8 {
        return Ok(None);
    }
    let coordinates: Option<Vec<_>> = grid
        .into_iter()
        .enumerate()
        .map(|(axis, value)| {
            value
                .checked_mul(128)?
                .checked_add(48 + ((hash >> (8 + axis * 4)) & 31) as i32)
        })
        .collect();
    let Some(coordinates) = coordinates else {
        return Ok(None);
    };
    let [x, y] = [coordinates[0], coordinates[1]];
    if x.unsigned_abs() > (i32::MAX - 256) as u32 || y.unsigned_abs() > (i32::MAX - 256) as u32 {
        return Ok(None);
    }
    let terrain = fields.sample(x, y, settings.rivers);
    let biome = surface_biome_selector::select(seed, [x, y], terrain);
    let kinds = eligible_kinds(biome, terrain);
    if kinds.is_empty() {
        return Ok(None);
    }
    let kind = kinds[(hash.rotate_left(19) as usize) % kinds.len()];
    let burial = match kind {
        RuinKind::TrailTop => 3,
        RuinKind::ExposedFossil => 2,
        _ => 0,
    };
    assemble(
        kind,
        [x, y, i32::from(terrain.height) - burial],
        ((hash >> 24) & 3) as u8,
        hash,
        |[sx, sy]| {
            let sample = fields.sample(sx, sy, settings.rivers);
            Some(SurfaceSample {
                ground: i32::from(sample.height) - 1,
                water: sample.water_level.map(|height| i32::from(height) - 1),
            })
        },
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_whole_rotated_ruins_keep_exact_owner_and_exposed_seam_states() {
        for kind in RuinKind::ALL {
            for rotation in 0..4 {
                let placement = assemble(
                    kind,
                    [-8, 15, 70],
                    rotation,
                    47,
                    |_| {
                        Some(SurfaceSample {
                            ground: 69,
                            water: None,
                        })
                    },
                    || false,
                )
                .unwrap()
                .unwrap();
                assert_eq!(placement.prepared.anchor(), [-8, 15, 70]);
                assert_eq!(placement.prepared.rotation(), rotation);
                assert!(placement.prepared.source().contains(kind.name()));
                let mut parts = BTreeMap::new();
                for right in [false, true] {
                    parts.extend(
                        placement
                            .writes
                            .iter()
                            .filter(|(position, _)| (position[0] >= 0) == right)
                            .map(|(position, state)| (*position, state.clone())),
                    );
                }
                assert_eq!(parts, placement.writes);
                assert!(parts.keys().all(|position| position[2] >= 69));
            }
        }
    }

    #[test]
    fn unknown_cancelled_and_fully_submerged_ruins_never_publish() {
        for kind in RuinKind::ALL {
            assert!(assemble(kind, [0, 0, 70], 0, 47, |_| None, || false).is_err());
            assert!(assemble(
                kind,
                [0, 0, 70],
                0,
                47,
                |_| Some(SurfaceSample {
                    ground: 69,
                    water: None
                }),
                || true
            )
            .is_err());
            assert!(assemble(
                kind,
                [0, 0, 70],
                0,
                47,
                |_| Some(SurfaceSample {
                    ground: 69,
                    water: Some(100)
                }),
                || false
            )
            .unwrap()
            .is_none());
        }
    }

    #[test]
    fn fossils_are_desert_or_wetland_features_and_mesa_entrances_need_a_cut() {
        let terrain = TerrainFields::new(7).sample(0, 0, false);
        for biome in [
            SurfaceBiome::Desert,
            SurfaceBiome::Swamp,
            SurfaceBiome::MangroveSwamp,
        ] {
            assert!(eligible_kinds(biome, terrain).contains(&RuinKind::ExposedFossil));
        }
        assert!(!eligible_kinds(SurfaceBiome::Badlands, terrain).contains(&RuinKind::ExposedFossil));
        let mesa = TerrainSample {
            landform: Landform::Mesa,
            valley_strength: 0.8,
            ..terrain
        };
        assert!(eligible_kinds(SurfaceBiome::Badlands, mesa).contains(&RuinKind::MesaMineEntrance));
        assert!(!eligible_kinds(
            SurfaceBiome::Badlands,
            TerrainSample {
                valley_strength: 0.1,
                ..mesa
            }
        )
        .contains(&RuinKind::MesaMineEntrance));
    }
}
