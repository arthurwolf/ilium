//! Original bounded aboveground ice geometry and cold surface admission.
//! Packed-ice spike material and surface freezing are primary26.3 facts;
//! silhouettes, rarity and opening noise are authored homage algorithms.
use super::{noise::value2, surface_biomes::SurfaceBiome};
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
