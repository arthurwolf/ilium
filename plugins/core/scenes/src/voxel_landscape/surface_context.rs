//! Static generated-scene atmosphere and globally owned creature vignettes.
//! No clock, player sleep debt, lightning simulation or combat is implied.
use super::{noise::hash2, surface_biomes::SurfaceBiome, surface_raster::DirectionalLight};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneAtmosphere {
    Day,
    Night,
    Thunderstorm,
}
impl SceneAtmosphere {
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Night,
            2 => Self::Thunderstorm,
            _ => Self::Day,
        }
    }
    pub fn light(self) -> DirectionalLight {
        let (ambient, diffuse) = match self {
            Self::Day => (0.35, 0.65),
            Self::Night => (0.18, 0.22),
            Self::Thunderstorm => (0.25, 0.35),
        };
        DirectionalLight {
            ambient,
            diffuse,
            ..Default::default()
        }
    }
    pub fn is_night(self) -> bool {
        self == Self::Night
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextEvent {
    Phantom,
    HorseTrap,
    LightningPig,
}
pub const CONTEXT_GRID: i32 = 64;
/// Sparse deterministic display episodes; frequencies are original scene policy.
pub fn candidate(
    seed: u64,
    grid: [i32; 2],
    atmosphere: SceneAtmosphere,
) -> Option<([i32; 2], ContextEvent)> {
    let (salt, rarity) = match atmosphere {
        SceneAtmosphere::Day => return None,
        SceneAtmosphere::Night => (0x7068_616e_746f_6d73, 32),
        SceneAtmosphere::Thunderstorm => (0x7374_6f72_6d5f_7067, 64),
    };
    let entropy = hash2(seed ^ salt, i64::from(grid[0]), i64::from(grid[1]));
    if !entropy.is_multiple_of(rarity) {
        return None;
    }
    let xy = [
        grid[0]
            .checked_mul(CONTEXT_GRID)?
            .checked_add(16 + ((entropy >> 16) & 31) as i32)?,
        grid[1]
            .checked_mul(CONTEXT_GRID)?
            .checked_add(16 + ((entropy >> 24) & 31) as i32)?,
    ];
    let event = if atmosphere.is_night() {
        ContextEvent::Phantom
    } else if entropy & 0x100 != 0 {
        ContextEvent::HorseTrap
    } else {
        ContextEvent::LightningPig
    };
    Some((xy, event))
}
/// Dry warm-enough precipitation biomes; snow/desert contexts are excluded.
/// This static policy does not claim exact altitude-adjusted native weather.
pub fn storm_eligible(biome: SurfaceBiome) -> bool {
    let descriptor = biome.descriptor();
    descriptor.has_precipitation && descriptor.gameplay_temperature > 0.15
}
/// The heart chooses one global nearby position, independent of the view window.
pub fn creaking_candidate(seed: u64, heart: [i32; 3]) -> Option<[i32; 2]> {
    let entropy = hash2(
        seed ^ 0x6372_6561_6b69_6e67,
        i64::from(heart[0]),
        i64::from(heart[1]),
    );
    let [dx, dy] = [[8, 0], [0, 8], [-8, 0], [0, -8]][(entropy & 3) as usize];
    Some([heart[0].checked_add(dx)?, heart[1].checked_add(dy)?])
}
/// One static riverbank figure per eligible signed owner cell at night.
/// This depicts a walker beside the river; underwater spawning is not generated.
pub const RIVERBANK_GRID: i32 = 16;
pub fn riverbank_candidate(
    seed: u64,
    grid: [i32; 2],
    atmosphere: SceneAtmosphere,
    rivers: bool,
) -> Option<[i32; 2]> {
    if !atmosphere.is_night() || !rivers {
        return None;
    }
    let xy = [
        grid[0].checked_mul(RIVERBANK_GRID)?.checked_add(8)?,
        grid[1].checked_mul(RIVERBANK_GRID)?.checked_add(8)?,
    ];
    let fields = super::terrain_fields::TerrainFields::new(seed);
    let bank = fields.sample(xy[0], xy[1], true);
    if bank.water_level.is_some_and(|level| level > bank.height) {
        return None;
    }
    for distance in 1..=8 {
        for [dx, dy] in [
            [1, 0],
            [-1, 0],
            [0, 1],
            [0, -1],
            [1, 1],
            [-1, 1],
            [1, -1],
            [-1, -1],
        ] {
            let Some(x) = xy[0].checked_add(dx * distance) else {
                continue;
            };
            let Some(y) = xy[1].checked_add(dy * distance) else {
                continue;
            };
            let sample = fields.sample(x, y, true);
            if !matches!(
                super::surface_biome_selector::select(seed, [x, y], sample),
                SurfaceBiome::River | SurfaceBiome::FrozenRiver
            ) {
                continue;
            }
            if sample.water_level.is_some_and(|level| {
                sample.height < level && bank.height >= level && bank.height <= level + 4
            }) {
                return Some(xy);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phases_have_distinct_coherent_lighting_and_day_is_default() {
        assert_eq!(SceneAtmosphere::from_index(0), SceneAtmosphere::Day);
        assert_eq!(SceneAtmosphere::from_index(1), SceneAtmosphere::Night);
        assert_eq!(
            SceneAtmosphere::from_index(2),
            SceneAtmosphere::Thunderstorm
        );
        assert_eq!(
            SceneAtmosphere::from_index(usize::MAX),
            SceneAtmosphere::Day
        );
        let brightness = |phase: SceneAtmosphere| phase.light().ambient + phase.light().diffuse;
        assert!(brightness(SceneAtmosphere::Night) < brightness(SceneAtmosphere::Thunderstorm));
        assert!(brightness(SceneAtmosphere::Thunderstorm) < brightness(SceneAtmosphere::Day));
    }
    #[test]
    fn episodes_are_sparse_signed_global_owners_and_absent_by_day() {
        let mut counts = [0usize; 3];
        for gy in -64..64 {
            for gx in -64..64 {
                assert!(candidate(71839, [gx, gy], SceneAtmosphere::Day).is_none());
                for phase in [SceneAtmosphere::Night, SceneAtmosphere::Thunderstorm] {
                    if let Some((xy, event)) = candidate(71839, [gx, gy], phase) {
                        assert_eq!(xy.map(|v| v.div_euclid(CONTEXT_GRID)), [gx, gy]);
                        assert_eq!(candidate(71839, [gx, gy], phase), Some((xy, event)));
                        match event {
                            ContextEvent::Phantom => {
                                assert!(phase.is_night());
                                counts[0] += 1
                            }
                            ContextEvent::HorseTrap => {
                                assert_eq!(phase, SceneAtmosphere::Thunderstorm);
                                counts[1] += 1
                            }
                            ContextEvent::LightningPig => {
                                assert_eq!(phase, SceneAtmosphere::Thunderstorm);
                                counts[2] += 1
                            }
                        }
                    }
                }
            }
        }
        assert!(counts.iter().all(|&n| (50..800).contains(&n)), "{counts:?}");
        for phase in [SceneAtmosphere::Night, SceneAtmosphere::Thunderstorm] {
            for grid in [[i32::MAX, 0], [i32::MIN, 0], [0, i32::MAX], [0, i32::MIN]] {
                assert!(candidate(71839, grid, phase).is_none());
            }
        }
    }
    #[test]
    fn storms_require_rain_context_and_hearts_keep_one_safe_global_offset() {
        for biome in [
            SurfaceBiome::Desert,
            SurfaceBiome::Badlands,
            SurfaceBiome::Savanna,
            SurfaceBiome::SnowyPlains,
            SurfaceBiome::FrozenPeaks,
        ] {
            assert!(!storm_eligible(biome), "{biome:?}");
        }
        for biome in [
            SurfaceBiome::Plains,
            SurfaceBiome::Forest,
            SurfaceBiome::Jungle,
            SurfaceBiome::Swamp,
        ] {
            assert!(storm_eligible(biome));
        }
        for x in -128..128 {
            let heart = [x, -23, 80];
            let xy = creaking_candidate(71839, heart).unwrap();
            assert_eq!((xy[0] - heart[0]).abs() + (xy[1] - heart[1]).abs(), 8);
            assert_eq!(creaking_candidate(71839, heart), Some(xy));
        }
    }

    #[test]
    fn riverbank_owners_require_night_water_and_checked_signed_coordinates() {
        let grid = [-950, -1021];
        assert_eq!(
            riverbank_candidate(71839, grid, SceneAtmosphere::Night, true),
            Some([-15192, -16328])
        );
        for phase in [SceneAtmosphere::Day, SceneAtmosphere::Thunderstorm] {
            assert_eq!(riverbank_candidate(71839, grid, phase, true), None);
        }
        assert_eq!(
            riverbank_candidate(71839, grid, SceneAtmosphere::Night, false),
            None
        );
        for grid in [[i32::MAX, 0], [i32::MIN, 0], [0, i32::MAX], [0, i32::MIN]] {
            assert_eq!(
                riverbank_candidate(71839, grid, SceneAtmosphere::Night, true),
                None
            );
        }
    }
}
