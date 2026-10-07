//! Original bounded surface material rules, exposed fossils and cold geology.
//! Packed-ice spike material and surface freezing are primary26.3 facts;
//! silhouettes, rarity and opening noise are authored homage algorithms.
use super::{
    assets::error::{AssetError, Result},
    noise::{hash2, value2},
    surface_biomes::SurfaceBiome,
    surface_flora::HabitatCell,
    terrain_fields::TerrainSample,
    tree_geometry::{LogAxis, TreeCell, TreeGeometry},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum Exposure {
    Ordinary,
    Terracotta(i32),
    Calcite(i64),
}

/// One global column policy shared by emission, habitat and fauna support.
#[derive(Clone, Copy, Debug)]
pub struct TerrainMaterials {
    pub top: &'static str,
    substrate: &'static str,
    ground: i32,
    exposure: Exposure,
}

fn terracotta(z: i64) -> &'static str {
    match z.rem_euclid(24) {
        0..=3 | 16..=19 => "minecraft:terracotta",
        4..=7 | 20..=22 => "minecraft:orange_terracotta",
        8 => "minecraft:yellow_terracotta",
        9..=10 => "minecraft:brown_terracotta",
        11..=13 => "minecraft:red_terracotta",
        14 => "minecraft:white_terracotta",
        _ => "minecraft:light_gray_terracotta",
    }
}

impl TerrainMaterials {
    pub fn top_habitat(self) -> HabitatCell {
        match self.top {
            "minecraft:sand" | "minecraft:red_sand" => HabitatCell::Sand,
            "minecraft:grass_block"
            | "minecraft:coarse_dirt"
            | "minecraft:podzol"
            | "minecraft:mycelium"
            | "minecraft:mud" => HabitatCell::Soil,
            _ => HabitatCell::Solid,
        }
    }

    /// Select within the existing exposed shell; this never extends its depth.
    pub fn at(self, z: i32) -> &'static str {
        if z == self.ground {
            return self.top;
        }
        match self.exposure {
            Exposure::Terracotta(phase) => {
                if self.top_habitat() == HabitatCell::Soil && z >= self.ground.saturating_sub(3) {
                    return "minecraft:dirt";
                }
                terracotta(i64::from(z) + i64::from(phase))
            }
            Exposure::Calcite(phase) if (phase + i64::from(z)).rem_euclid(53) < 4 => {
                "minecraft:calcite"
            }
            Exposure::Calcite(_) => self.substrate,
            Exposure::Ordinary if z >= self.ground.saturating_sub(3) => self.substrate,
            Exposure::Ordinary => "minecraft:stone",
        }
    }
}

/// Authored strata and low-frequency mosaics, independent of the height kernel.
pub fn terrain_materials(
    seed: u64,
    biome: SurfaceBiome,
    xy: [i32; 2],
    ground: i32,
) -> TerrainMaterials {
    use SurfaceBiome::*;
    let (top, substrate) = match biome {
        Badlands | ErodedBadlands | WoodedBadlands => {
            ("minecraft:red_sand", "minecraft:terracotta")
        }
        Desert | Beach => ("minecraft:sand", "minecraft:sandstone"),
        SnowyBeach => ("minecraft:snow_block", "minecraft:sand"),
        River => ("minecraft:sand", "minecraft:gravel"),
        FrozenRiver => ("minecraft:snow_block", "minecraft:gravel"),
        StonyShore | StonyPeaks | WindsweptGravellyHills => ("minecraft:stone", "minecraft:stone"),
        FrozenPeaks => ("minecraft:snow_block", "minecraft:packed_ice"),
        JaggedPeaks => ("minecraft:snow_block", "minecraft:stone"),
        MushroomFields => ("minecraft:mycelium", "minecraft:dirt"),
        Swamp | MangroveSwamp => ("minecraft:mud", "minecraft:mud"),
        SnowyPlains | SnowySlopes | IceSpikes | SnowyTaiga | Grove => {
            ("minecraft:snow_block", "minecraft:dirt")
        }
        _ => ("minecraft:grass_block", "minecraft:dirt"),
    };
    let mut result = TerrainMaterials {
        top,
        substrate,
        ground,
        exposure: Exposure::Ordinary,
    };
    let noise = |salt, period| value2(seed ^ salt, i64::from(xy[0]), i64::from(xy[1]), period);
    if matches!(biome, Badlands | ErodedBadlands | WoodedBadlands) {
        let phase = (hash2(seed ^ 0x7374_7261_7461, 0, 0) % 24) as i32;
        result.exposure = Exposure::Terracotta(phase);
        if biome == WoodedBadlands && ground >= 100 {
            let soil = noise(0x736f_696c_5f6d_6f73, 48);
            // Sandy openings retain the existing arid flora on wooded mesas.
            if soil >= -0.45 {
                result.top = if soil > 0.35 {
                    "minecraft:coarse_dirt"
                } else {
                    "minecraft:grass_block"
                };
            }
        }
        return result;
    }
    if biome == StonyPeaks {
        let phase = i64::from(xy[0])
            + 2 * i64::from(xy[1])
            + (6.0 * noise(0x0063_616c_6369_7465, 96)).round() as i64;
        result.substrate = if noise(0x726f_636b_5f70_6174, 48) > 0.60 {
            "minecraft:andesite"
        } else {
            "minecraft:stone"
        };
        result.exposure = Exposure::Calcite(phase);
        result.top = if (phase + i64::from(ground)).rem_euclid(53) < 4 {
            "minecraft:calcite"
        } else {
            result.substrate
        };
        return result;
    }
    if biome == WindsweptGravellyHills {
        let patch = noise(0x6772_6176_656c, 32);
        result.top = if patch < -0.50 {
            "minecraft:stone"
        } else if patch > 0.40 {
            "minecraft:grass_block"
        } else {
            "minecraft:gravel"
        };
        result.substrate = if result.top == "minecraft:grass_block" {
            "minecraft:dirt"
        } else {
            result.top
        };
        return result;
    }
    // Mega-conifer biomes retain their tree-owned podzol decorator exclusively.
    if matches!(
        biome,
        Forest
            | DappledForest
            | OldGrowthBirchForest
            | OldGrowthPineTaiga
            | OldGrowthSpruceTaiga
            | WindsweptForest
            | Taiga
    ) {
        let patch = noise(0x736f_696c_5f6d_6f73, 48);
        if matches!(biome, Taiga | OldGrowthPineTaiga | OldGrowthSpruceTaiga) && patch > 0.45 {
            result.top = "minecraft:podzol";
        } else if patch < -0.45 {
            result.top = "minecraft:coarse_dirt";
        }
    }
    result
}

pub const FOSSIL_GRID: i32 = 64;
pub const FOSSIL_REACH: i32 = 16;
pub const FOSSIL_JITTER: i32 = 16;
pub const FOSSIL_MAX_CELLS: usize = 128;
pub const FOSSIL_SOURCE: &str = "ilium:exposed_desert_fossil/v1";

pub struct ExposedFossil {
    pub anchor: [i32; 3],
    pub minimum: [i32; 2],
    /// Exclusive footprint maximum, including the spaces between ribs.
    pub maximum: [i32; 2],
    pub cells: Vec<([i16; 3], LogAxis)>,
}

impl ExposedFossil {
    pub fn covers(&self, position: [i32; 3]) -> bool {
        (0..2)
            .all(|axis| position[axis] >= self.minimum[axis] && position[axis] < self.maximum[axis])
    }
}

/// One rare global owner; no clipping, excavation, RNG stream or pack dependency.
pub fn desert_fossil(
    seed: u64,
    grid: [i32; 2],
    mut sample_column: impl FnMut([i32; 2]) -> (TerrainSample, SurfaceBiome),
    cancelled: impl Fn() -> bool,
) -> Result<Option<ExposedFossil>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let entropy = hash2(
        seed ^ 0x666f_7373_696c_7631,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if !entropy.is_multiple_of(8) {
        return Ok(None);
    }
    let mut xy = [0; 2];
    for (axis, target) in xy.iter_mut().enumerate() {
        let jitter = (entropy.rotate_left(17 + axis as u32 * 19) % 33) as i32 - FOSSIL_JITTER;
        let Some(value) = grid[axis]
            .checked_mul(FOSSIL_GRID)
            .and_then(|v| v.checked_add(jitter))
        else {
            return Ok(None);
        };
        *target = value;
    }
    let half = 4 + ((entropy >> 12) & 1) as i16 * 2;
    let width = 2 + ((entropy >> 13) & 1) as i16;
    let geometry_error = |_| AssetError::InvalidMetadata("exposed fossil geometry failed".into());
    let mut geometry = TreeGeometry::default();
    geometry
        .branch([-half, 0, 2], [half, 0, 2])
        .map_err(geometry_error)?;
    for along in (-half + 1..half).step_by(2) {
        geometry
            .branch([along, -width, 0], [along, -width, 2])
            .map_err(geometry_error)?;
        geometry
            .branch([along, -width, 2], [along, width, 2])
            .map_err(geometry_error)?;
        geometry
            .branch([along, width, 2], [along, width, 0])
            .map_err(geometry_error)?;
    }
    geometry = geometry.rotated((entropy >> 24) as u8);
    let mut minimum = [i32::MAX; 2];
    let mut maximum = [i32::MIN; 2];
    for (position, _) in geometry.cells() {
        for (axis, &offset) in position[..2].iter().enumerate() {
            let Some(value) = xy[axis].checked_add(i32::from(offset)) else {
                return Ok(None);
            };
            if !(i32::MIN + 256..i32::MAX - 256).contains(&value) {
                return Ok(None);
            }
            minimum[axis] = minimum[axis].min(value);
            maximum[axis] = maximum[axis].max(value + 1);
        }
    }
    let mut heights = BTreeMap::new();
    let mut low = i32::MAX;
    let mut high = i32::MIN;
    for y in minimum[1]..maximum[1] {
        for x in minimum[0]..maximum[0] {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let (sample, biome) = sample_column([x, y]);
            let height = i32::from(sample.height);
            if biome != SurfaceBiome::Desert
                || sample
                    .water_level
                    .is_some_and(|water| water > sample.height)
                || terrain_materials(seed, biome, [x, y], height - 1).top != "minecraft:sand"
            {
                return Ok(None);
            }
            low = low.min(height);
            high = high.max(height);
            if high - low > 1 {
                return Ok(None);
            }
            heights.insert([x, y], height);
        }
    }
    let feet: Vec<_> = geometry
        .cells()
        .filter(|(position, _)| position[2] == 0)
        .collect();
    for (foot, _) in feet {
        if heights[&[xy[0] + i32::from(foot[0]), xy[1] + i32::from(foot[1])]] < high {
            geometry
                .branch([foot[0], foot[1], -1], foot)
                .map_err(geometry_error)?;
        }
    }
    let mut cells = Vec::new();
    for (position, cell) in geometry.cells() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let z = high + i32::from(position[2]);
        if position
            .iter()
            .any(|value| i32::from(*value).abs() > FOSSIL_REACH)
            || !(super::surface_viewport::SOURCE_Z_MIN..super::surface_viewport::SOURCE_Z_MAX)
                .contains(&f64::from(z))
            || z < heights[&[
                xy[0] + i32::from(position[0]),
                xy[1] + i32::from(position[1]),
            ]]
        {
            return Ok(None);
        }
        let TreeCell::Log(axis) = cell else {
            return Err(AssetError::InvalidMetadata(
                "exposed fossil contains a non-bone cell".into(),
            ));
        };
        if cells.len() == FOSSIL_MAX_CELLS {
            return Err(AssetError::Limit {
                resource: "surface fossil cells",
                requested: (cells.len() + 1) as u64,
                limit: FOSSIL_MAX_CELLS as u64,
            });
        }
        cells.push((position, axis));
    }
    Ok(Some(ExposedFossil {
        anchor: [xy[0], xy[1], high],
        minimum,
        maximum,
        cells,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeologyCell {
    pub position: [i16; 3],
    pub resource_id: &'static str,
}
pub fn ice_spike(entropy: u64) -> Vec<GeologyCell> {
    let height = 9 + (entropy % 23) as i16;
    let base_radius = 3 + (entropy.rotate_left(11) % 2) as i16;
    let mut cells = Vec::new();
    for z in 0..height {
        let radius = base_radius * (height - 1 - z) / (height - 1);
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    cells.push(GeologyCell {
                        position: [x, y, z],
                        resource_id: "minecraft:packed_ice",
                    });
                }
            }
        }
    }
    cells
}
pub fn freezes_surface(biome: SurfaceBiome, seed: u64, position: [i32; 2]) -> bool {
    let cold = matches!(
        biome,
        SurfaceBiome::FrozenRiver
            | SurfaceBiome::FrozenPeaks
            | SurfaceBiome::Grove
            | SurfaceBiome::IceSpikes
            | SurfaceBiome::JaggedPeaks
            | SurfaceBiome::SnowyBeach
            | SurfaceBiome::SnowyPlains
            | SurfaceBiome::SnowySlopes
            | SurfaceBiome::SnowyTaiga
    );
    cold && value2(
        seed ^ 0x6672_6f73_745f_6963,
        i64::from(position[0]),
        i64::from(position[1]),
        32,
    ) < 0.75
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn river_banks_use_surface_sand_or_snow_over_gravel() {
        let warm = terrain_materials(71839, SurfaceBiome::River, [12, -9], 64);
        assert_eq!(warm.top, "minecraft:sand");
        assert_eq!(warm.at(63), "minecraft:gravel");

        let frozen = terrain_materials(71839, SurfaceBiome::FrozenRiver, [12, -9], 64);
        assert_eq!(frozen.top, "minecraft:snow_block");
        assert_eq!(frozen.at(63), "minecraft:gravel");
    }

    #[test]
    fn cold_rivers_freeze_but_warm_rivers_keep_water() {
        let mut frozen = 0;
        for x in -64..64 {
            for y in -64..64 {
                frozen += usize::from(freezes_surface(SurfaceBiome::FrozenRiver, 71839, [x, y]));
                assert!(!freezes_surface(SurfaceBiome::River, 71839, [x, y]));
            }
        }
        assert!(
            frozen > 12000,
            "Frozen river lacks widespread surfaceice: {frozen}/16384"
        );
    }
    #[test]
    fn ice_spikes_have_aboveground_packed_ice_and_tapered_tops() {
        let mut heights = std::collections::BTreeSet::new();
        for entropy in 0..64 {
            let cells = ice_spike(entropy);
            assert!(!cells.is_empty(), "Ice spike is empty");
            let top = cells.iter().map(|cell| cell.position[2]).max().unwrap();
            heights.insert(top);
            assert!((8..=31).contains(&top));
            assert!(cells
                .iter()
                .all(|cell| cell.resource_id == "minecraft:packed_ice"
                    && cell.position[2] >= 0
                    && cell.position[0].abs() <= 4
                    && cell.position[1].abs() <= 4));
            assert_eq!(
                cells.iter().filter(|cell| cell.position[2] == top).count(),
                1
            );
            assert!(cells.iter().filter(|cell| cell.position[2] == 0).count() > 9);
        }
        assert!(heights.len() > 8, "Spikes all have same height");
    }
}
