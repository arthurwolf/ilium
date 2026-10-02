//! Continuous fields for the surface Overworld homage.
//!
//! These are authored terrain prescriptions, not Minecraft's climate selector
//! bands. Weather temperature in the canonical biome registry is separate.
//! Absolute integer coordinates and stateless sampling preserve chunk seams.
use super::noise::{fbm2, value2};

pub const SURFACE_SEA_LEVEL: i16 = 63;
pub const SURFACE_MAX_HEIGHT: i16 = 240;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainClimate {
    pub continentalness: f64,
    pub temperature: f64,
    pub humidity: f64,
    pub erosion: f64,
    pub weirdness: f64,
    pub ridge: f64,
    pub river_distance: f64,
    pub detail: f64,
    pub island: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landform {
    Coast,
    Lowland,
    RollingHills,
    Mountain,
    Dunes,
    Mesa,
    Wetland,
    Island,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainSample {
    pub climate: TerrainClimate,
    pub landform: Landform,
    /// Exclusive solid surface height, before authored structures and plants.
    pub height: i16,
    pub uncarved_height: i16,
    pub water_level: Option<i16>,
    pub river: bool,
    pub valley_strength: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct TerrainFields {
    seed: u64,
}

fn smoothstep(lower: f64, upper: f64, value: f64) -> f64 {
    let t = ((value - lower) / (upper - lower)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl TerrainFields {
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub fn climate(&self, x: i32, z: i32) -> TerrainClimate {
        let (x, z) = (i64::from(x), i64::from(z));
        // The warp is smaller than its lattice period. Quantized offsets stay
        // coordinate safe and do not depend on a chunk origin or sampling order.
        let warped_x =
            x + (96.0 * fbm2(self.seed ^ 0x0077_6172_7078, x, z, 1024, 3)).round() as i64;
        let warped_z =
            z + (96.0 * fbm2(self.seed ^ 0x0077_6172_707a, x, z, 1024, 3)).round() as i64;
        let field =
            |salt, period, octaves| fbm2(self.seed ^ salt, warped_x, warped_z, period, octaves);
        let weirdness = field(0x0077_6569_7264, 384, 4);
        // Two non-aligned fields give rivers coherent curved valleys without
        // excluding any repeating row or imposing a global compass direction.
        let river_field = 0.72 * field(0x0072_6976_6572, 512, 4)
            + 0.28 * fbm2(self.seed ^ 0x7269_7665_7232, warped_z, -warped_x, 896, 3);
        TerrainClimate {
            continentalness: field(0x0063_6f6e_7469, 1536, 5),
            temperature: (0.5 + 0.9 * field(0x0074_656d_7065, 1280, 4)).clamp(0.0, 1.0),
            humidity: (0.5 + 0.95 * field(0x0068_756d_6964, 1024, 4)).clamp(0.0, 1.0),
            erosion: field(0x0065_726f_7369, 768, 4),
            weirdness,
            ridge: (1.0 - 2.0 * weirdness.abs()).clamp(0.0, 1.0),
            river_distance: river_field.abs().clamp(0.0, 1.0),
            detail: fbm2(self.seed ^ 0x0064_6574_6169, x, z, 80, 4),
            island: field(0x6973_6c61_6e64, 256, 3),
        }
    }

    pub fn sample(&self, x: i32, z: i32, rivers: bool) -> TerrainSample {
        let climate = self.climate(x, z);
        self.shape(x, z, climate, rivers)
    }

    fn shape(&self, x: i32, z: i32, climate: TerrainClimate, rivers: bool) -> TerrainSample {
        let sea = f64::from(SURFACE_SEA_LEVEL);
        let inland = smoothstep(-0.08, 0.35, climate.continentalness);
        let mountainous = inland * (1.0 - smoothstep(-0.55, 0.38, climate.erosion));
        let mountain_height = 128.0 * mountainous * climate.ridge.powi(2);
        let base_height = sea + 9.0 + 48.0 * climate.continentalness;
        let detail_height = (3.0 + 9.0 * mountainous) * climate.detail;
        let arid = climate.temperature > 0.68 && climate.humidity < 0.35;
        let wet = climate.humidity > 0.70 && climate.continentalness < 0.18;
        let island = climate.continentalness < -0.25 && climate.island > 0.38;
        let landform = if island {
            Landform::Island
        } else if climate.continentalness < -0.02 {
            Landform::Coast
        } else if mountain_height > 32.0 {
            Landform::Mountain
        } else if arid && climate.erosion < -0.05 {
            Landform::Mesa
        } else if arid {
            Landform::Dunes
        } else if wet {
            Landform::Wetland
        } else if climate.erosion < 0.15 {
            Landform::RollingHills
        } else {
            Landform::Lowland
        };
        // Categories select ecology; continuous weights select geometry. A
        // threshold crossing must not replace a whole height prescription.
        let arid_weight = smoothstep(0.60, 0.76, climate.temperature)
            * (1.0 - smoothstep(0.27, 0.43, climate.humidity));
        let wet_weight = smoothstep(0.60, 0.72, climate.humidity)
            * (1.0 - smoothstep(0.12, 0.22, climate.continentalness))
            * (1.0 - smoothstep(18.0, 34.0, mountain_height));
        let island_weight = (1.0 - smoothstep(-0.33, -0.17, climate.continentalness))
            * smoothstep(0.30, 0.46, climate.island);
        let dune_height = {
            // Integer modulo makes the sine continuous through negative
            // coordinates and keeps its phase accurate at i32 extremes.
            let wind_coordinate = (3 * i64::from(x) + i64::from(z)).rem_euclid(128);
            let phase = std::f64::consts::TAU * wind_coordinate as f64 / 128.0
                + 1.8 * value2(self.seed ^ 0x6475_6e65, i64::from(x), i64::from(z), 160);
            base_height + 7.0 * (phase.sin() + 0.25 * (2.0 * phase).sin()) + detail_height
        };
        let mesa_height = {
            let plateau = base_height + 28.0 * inland + 16.0 * climate.ridge;
            let terraced = sea + ((plateau - sea) / 7.0).floor() * 7.0;
            terraced + 1.5 * climate.detail
        };
        let blend = |a: f64, b: f64, weight: f64| a + (b - a) * weight;
        let dry_height = blend(
            dune_height,
            mesa_height,
            1.0 - smoothstep(-0.13, 0.03, climate.erosion),
        );
        let uncarved = blend(
            base_height + mountain_height + detail_height,
            dry_height,
            arid_weight * (1.0 - smoothstep(20.0, 44.0, mountain_height)),
        );
        let uncarved = blend(
            uncarved,
            // Wetland cores straddle the shared water plane instead of sitting
            // several blocks above it. Coherent detail produces shallow pools
            // and hummocks; continuous climate weights preserve dry fringes.
            sea - 0.8 + 2.0 * climate.detail + 0.4 * inland,
            wet_weight,
        );
        let uncarved = blend(
            uncarved,
            sea + 4.0 + 32.0 * (climate.island - 0.38) + detail_height,
            island_weight,
        );
        let uncarved_height = uncarved.round().clamp(4.0, f64::from(SURFACE_MAX_HEIGHT)) as i16;
        let valley_strength = if rivers && !island {
            1.0 - smoothstep(0.015, 0.075, climate.river_distance)
        } else {
            0.0
        };
        // This stateless field stage has one connected water plane. Independently
        // deriving perched water from each column leaves exposed floating water
        // at banks. Elevated lakes and rivers need a later drainage topology.
        let river_level = SURFACE_SEA_LEVEL;
        let bed = uncarved_height.min(river_level - 3);
        let height = (f64::from(uncarved_height)
            + f64::from(bed - uncarved_height) * valley_strength)
            .round() as i16;
        let river = rivers
            && uncarved_height >= SURFACE_SEA_LEVEL
            && valley_strength > 0.55
            && height < river_level;
        let water_level = if height < SURFACE_SEA_LEVEL {
            Some(SURFACE_SEA_LEVEL)
        } else if river {
            Some(river_level)
        } else {
            None
        };
        TerrainSample {
            climate,
            landform,
            height,
            uncarved_height,
            water_level,
            river,
            valley_strength,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humid_wetland_core_contains_shallow_pools_and_dry_banks() {
        let terrain = TerrainFields::new(71839);
        let mut wet = 0;
        let mut dry = 0;
        for detail in [-0.4, -0.1, 0.3, 0.7] {
            let sample = terrain.shape(
                0,
                0,
                TerrainClimate {
                    continentalness: 0.0,
                    temperature: 0.65,
                    humidity: 0.90,
                    erosion: 0.20,
                    weirdness: 0.40,
                    ridge: 0.20,
                    river_distance: 0.30,
                    detail,
                    island: 0.0,
                },
                false,
            );
            assert_eq!(sample.landform, Landform::Wetland);
            if let Some(level) = sample.water_level {
                assert_eq!(level, SURFACE_SEA_LEVEL);
                assert!((1..=3).contains(&(level - sample.height)));
                wet += 1;
            } else {
                dry += 1;
            }
        }
        assert!(wet >= 2, "wetland core remains mostly dry: {wet} pools");
        assert!(dry >= 1, "wetland core needs above-water banks");
    }

    #[test]
    fn wetland_boundary_does_not_create_an_artificial_cliff() {
        let terrain = TerrainFields::new(1);
        let a = terrain.sample(-2272, -832, false);
        let b = terrain.sample(-2272, -833, false);
        assert!((a.height - b.height).abs() <= 2, "{a:?} {b:?}");
    }

    #[test]
    fn wetland_river_water_has_a_containing_bank() {
        let terrain = TerrainFields::new(1);
        let a = terrain.sample(-992, -3968, true);
        let b = terrain.sample(-992, -3967, true);
        if let Some(level) = a.water_level {
            assert!(
                b.height >= level || b.water_level == Some(level),
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn natural_rivers_cross_historical_band_boundaries() {
        let mut crossings = 0;
        for seed in [1, 7, 31, 173] {
            let terrain = TerrainFields::new(seed);
            for z in (-1536..=1536).step_by(192) {
                for x in (-4096..=4096).step_by(32) {
                    crossings += usize::from(terrain.sample(x, z, true).river);
                }
            }
        }
        assert!(crossings > 0);
    }

    #[test]
    fn disabling_rivers_restores_uncarved_land_but_preserves_coastal_water() {
        let terrain = TerrainFields::new(7);
        let mut inland_river = false;
        let mut coastal_water = false;
        for z in (-2048..2048).step_by(64) {
            for x in (-2048..2048).step_by(64) {
                let enabled = terrain.sample(x, z, true);
                let disabled = terrain.sample(x, z, false);
                assert_eq!(disabled.height, disabled.uncarved_height);
                assert!(!disabled.river);
                if enabled.river && disabled.height > SURFACE_SEA_LEVEL {
                    inland_river = true;
                    assert!(disabled.water_level.is_none());
                }
                coastal_water |= disabled.height < SURFACE_SEA_LEVEL
                    && disabled.water_level == Some(SURFACE_SEA_LEVEL);
            }
        }
        assert!(inland_river && coastal_water);
    }

    #[test]
    fn eroded_and_rugged_prescriptions_have_different_relief() {
        let terrain = TerrainFields::new(0);
        let mut climate = TerrainClimate {
            continentalness: 0.6,
            temperature: 0.5,
            humidity: 0.5,
            erosion: -0.8,
            weirdness: 0.0,
            ridge: 1.0,
            river_distance: 1.0,
            detail: 0.0,
            island: 0.0,
        };
        let rugged = terrain.shape(0, 0, climate, false);
        climate.erosion = 0.8;
        let eroded = terrain.shape(0, 0, climate, false);
        assert_eq!(rugged.landform, Landform::Mountain);
        assert_eq!(eroded.landform, Landform::Lowland);
        assert!(rugged.height - eroded.height >= 80);
    }

    #[test]
    fn sampling_order_and_integer_extremes_do_not_change_fields() {
        for seed in [0, 7, u64::MAX] {
            let terrain = TerrainFields::new(seed);
            for x in [i32::MIN, -17, -16, -1, 0, 15, 16, i32::MAX] {
                for z in [i32::MIN, -17, 0, 16, i32::MAX] {
                    let before = terrain.sample(x, z, true);
                    let _ = terrain.sample(z, x, true);
                    assert_eq!(before, terrain.sample(x, z, true));
                    assert!((4..=SURFACE_MAX_HEIGHT).contains(&before.height));
                    assert!((0.0..=1.0).contains(&before.climate.temperature));
                    assert!((0.0..=1.0).contains(&before.climate.humidity));
                    assert!((-1.0..=1.0).contains(&before.climate.erosion));
                    assert!(before.climate.detail.is_finite());
                    if let Some(level) = before.water_level {
                        assert!(level > before.height);
                    }
                }
            }
        }
    }

    #[test]
    fn adjacent_chunk_edges_have_no_noise_origin_reset() {
        for seed in [1, 7, 173] {
            let terrain = TerrainFields::new(seed);
            for edge in [-1024, -16, 0, 16, 1024] {
                for z in [-531, -17, 0, 71, 333] {
                    let a = terrain.climate(edge - 1, z);
                    let b = terrain.climate(edge, z);
                    let c = terrain.climate(edge + 1, z);
                    for difference in [
                        (a.continentalness - b.continentalness).abs(),
                        (b.continentalness - c.continentalness).abs(),
                        (a.erosion - b.erosion).abs(),
                        (b.erosion - c.erosion).abs(),
                    ] {
                        assert!(difference < 0.08);
                    }
                }
            }
        }
        assert_ne!(
            TerrainFields::new(1).climate(100, 200),
            TerrainFields::new(2).climate(100, 200)
        );
    }
}
