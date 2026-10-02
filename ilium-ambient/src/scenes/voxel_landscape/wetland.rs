//! Production-only wetland occupancy derived from the frozen terrain kernel.
//! Ecology is classified once from the unmodified column; this pure decoration
//! is then shared by both terrain attachment and feature habitat sampling.
use super::{
    ecology::{Biome, Ecology},
    noise::value2,
    terrain::{self, Column},
};

const BASIN_SALT: u64 = 0x7765_746c_616e_6473;
const BASIN_WATER_LEVEL: i16 = terrain::SEA_LEVEL + 3;

pub fn occupy(seed: u64, x: i32, y: i32, raw: Column, ecology: Ecology) -> Column {
    if !matches!(ecology.biome, Biome::Swamp | Biome::MangroveSwamp)
        || raw.water_level.is_some()
        || raw.river
        || raw.ravine
        || raw.cave.is_some()
        || raw.height > BASIN_WATER_LEVEL
        || value2(seed ^ BASIN_SALT, i64::from(x), i64::from(y), 48) <= -0.12
    {
        return raw;
    }

    // A single absolute water plane intersects the low wetland. Erode its bed
    // by one block while leaving higher and noise-excluded mud as dry islands.
    // The derived surface is the new bed so the world skinning step places mud
    // on the bed rather than copying a former, now-submerged skin depth.
    let mut occupied = raw;
    occupied.height -= 1;
    occupied.surface_height = occupied.height;
    occupied.water_level = Some(BASIN_WATER_LEVEL);
    occupied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangrove_witness_has_shallow_pool_and_dry_island_without_changing_kernel() {
        let kernel = terrain::Terrain::new(7);
        let mut wet = 0;
        let mut dry = 0;
        for y in -174701..-174637 {
            for x in -439664..-439600 {
                let raw = kernel.sample(x, y);
                let ecology = super::super::ecology::sample(7, x, y, raw);
                let derived = occupy(7, x, y, raw, ecology);
                assert_eq!(kernel.sample(x, y), raw);
                if ecology.biome == Biome::MangroveSwamp {
                    if let Some(level) = derived.water_level {
                        wet += 1;
                        assert_eq!(level, BASIN_WATER_LEVEL);
                        assert!(level > derived.height);
                        assert_eq!(derived.block(level - 1), Some(terrain::Material::Water));
                    } else {
                        dry += 1;
                    }
                }
            }
        }
        assert!(wet > 0 && dry > 0, "wetland must contain water and islands");
    }

    #[test]
    fn caves_ravines_existing_water_and_other_biomes_keep_original_occupancy() {
        let kernel = terrain::Terrain::new(7);
        for &(x, y) in &[(-439632, -174669), (89489, -177042), (146131, 178553)] {
            let raw = kernel.sample(x, y);
            let ecology = super::super::ecology::sample(7, x, y, raw);
            let mut carved = raw;
            carved.ravine = true;
            assert_eq!(occupy(7, x, y, carved, ecology), carved);
            carved = raw;
            carved.cave = Some(terrain::AirSpan { bottom: 5, top: 8 });
            assert_eq!(occupy(7, x, y, carved, ecology), carved);
            carved = raw;
            carved.water_level = Some(terrain::SEA_LEVEL);
            assert_eq!(occupy(7, x, y, carved, ecology), carved);
            if !matches!(ecology.biome, Biome::Swamp | Biome::MangroveSwamp) {
                assert_eq!(occupy(7, x, y, raw, ecology), raw);
            }
        }
    }

    #[test]
    fn negative_and_large_coordinates_are_absolute_and_bounded() {
        let kernel = terrain::Terrain::new(7);
        for (x, y) in [
            (-1_000_000_000, 1_000_000_000),
            (1_000_000_000, -1_000_000_000),
            (-439632, -174669),
        ] {
            let raw = kernel.sample(x, y);
            let ecology = super::super::ecology::sample(7, x, y, raw);
            let first = occupy(7, x, y, raw, ecology);
            assert_eq!(first, occupy(7, x, y, raw, ecology));
            assert!(first.water_level.is_none_or(|level| level > first.height));
            assert!(first.height > 0);
        }
    }
}
