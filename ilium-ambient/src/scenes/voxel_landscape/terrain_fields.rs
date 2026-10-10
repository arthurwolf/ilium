//! Continuous fields for the surface Overworld homage.
//!
//! These are authored terrain prescriptions, not Minecraft's climate selector
//! bands. Weather temperature in the canonical biome registry is separate.
//! Absolute integer coordinates and stateless sampling preserve chunk seams.
use super::noise::{fbm2, hash2, value2};

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

    // Authored surface hoodoos: jittered global owners, narrow flat caps and
    // steep sides. Subtype and climate fades vanish at ecology boundaries.
    // Prospective river corridors fade the relief even when rivers are off,
    // preserving the contract that uncarved height is setting-independent.
    fn eroded_spire_relief(
        &self,
        x: i32,
        z: i32,
        climate: TerrainClimate,
        mountain_height: f64,
    ) -> f64 {
        let subtype = super::surface_biome_selector::variant(self.seed, [x, z]);
        let prospective_valley = 1.0 - smoothstep(0.015, 0.075, climate.river_distance);
        let weight = (1.0 - smoothstep(0.18, 0.20, subtype))
            * smoothstep(0.68, 0.69, climate.temperature)
            * (1.0 - smoothstep(0.34, 0.35, climate.humidity))
            * (1.0 - smoothstep(-0.08, -0.05, climate.erosion))
            * smoothstep(-0.02, 0.02, climate.continentalness)
            * (1.0 - smoothstep(24.0, 32.0, mountain_height))
            * (1.0 - smoothstep(0.0, 0.20, prospective_valley));
        if weight <= 0.0 {
            return 0.0;
        }
        let (x, z) = (i64::from(x), i64::from(z));
        let (grid_x, grid_z) = (x.div_euclid(24), z.div_euclid(24));
        let roughness = 0.10 * value2(self.seed ^ 0x7370_6972_6573, x, z, 7);
        let mut relief = 0.0_f64;
        for cell_x in grid_x - 1..=grid_x + 1 {
            for cell_z in grid_z - 1..=grid_z + 1 {
                let key = hash2(self.seed ^ 0x686f_6f64_6f6f, cell_x, cell_z);
                let center_x = cell_x * 24 + 8 + (key % 9) as i64;
                let center_z = cell_z * 24 + 8 + ((key >> 8) % 9) as i64;
                let radius_x = 6.0 + ((key >> 16) % 4) as f64;
                let radius_z = 6.0 + ((key >> 24) % 4) as f64;
                let dx = (x - center_x) as f64 / radius_x;
                let dz = (z - center_z) as f64 / radius_z;
                let distance = (dx * dx + dz * dz).sqrt() + roughness;
                let height = 18.0 + ((key >> 32) % 21) as f64;
                relief = relief.max(height * (1.0 - smoothstep(0.32, 1.0, distance)));
            }
        }
        relief * weight
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

    fn inland_lake_basin(
        &self,
        x: i32,
        z: i32,
        climate: TerrainClimate,
        mountain_height: f64,
        island_weight: f64,
        uncarved_height: i16,
    ) -> Option<(i16, Option<i16>)> {
        let wetland_core =
            climate.humidity > 0.70 && climate.continentalness < 0.18 && mountain_height <= 32.0;
        if climate.continentalness <= 0.08
            || mountain_height >= 48.0
            || island_weight >= 0.85
            || wetland_core
        {
            return None;
        }

        const CELL_SIZE: i64 = 192;
        let (world_x, world_z) = (i64::from(x), i64::from(z));
        let (grid_x, grid_z) = (world_x.div_euclid(CELL_SIZE), world_z.div_euclid(CELL_SIZE));
        let mut nearest = None;
        for cell_x in grid_x - 1..=grid_x + 1 {
            for cell_z in grid_z - 1..=grid_z + 1 {
                let site = hash2(self.seed ^ 0x696e_6c61_6e64_6c6b, cell_x, cell_z);
                if site % 6 != 0 {
                    continue;
                }
                let center_x = cell_x * CELL_SIZE + 48 + ((site >> 8) % 96) as i64;
                let center_z = cell_z * CELL_SIZE + 48 + ((site >> 16) % 96) as i64;
                let radius_x = 20.0 + ((site >> 24) % 21) as f64;
                let radius_z = 17.0 + ((site >> 32) % 20) as f64;
                let dx = (world_x - center_x) as f64 / radius_x;
                let dz = (world_z - center_z) as f64 / radius_z;
                let roughness =
                    0.07 * value2(self.seed ^ 0x6c61_6b65_726f_7567, world_x, world_z, 13);
                let distance = (dx * dx + dz * dz).sqrt() + roughness;
                if distance > 1.08 {
                    continue;
                }
                let water_level = SURFACE_SEA_LEVEL + 4 + ((site >> 40) % 7) as i16;
                if nearest.is_none_or(|(nearest_distance, _)| distance < nearest_distance) {
                    nearest = Some((distance, water_level));
                }
            }
        }
        let (distance, water_level) = nearest?;
        let bank_height = f64::from(water_level - 4) + 5.0 * smoothstep(0.58, 1.08, distance);
        let basin_weight = 1.0 - smoothstep(0.72, 1.08, distance);
        let height = (f64::from(uncarved_height)
            + (bank_height - f64::from(uncarved_height)) * basin_weight)
            .round()
            .clamp(4.0, f64::from(SURFACE_MAX_HEIGHT)) as i16;
        let water = (distance < 0.72 && height < water_level).then_some(water_level);
        Some((height, water))
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
        // Categories select ecology; continuous weights select geometry. A
        // threshold crossing must not replace a whole height prescription.
        let arid_weight = smoothstep(0.60, 0.76, climate.temperature)
            * (1.0 - smoothstep(0.27, 0.43, climate.humidity));
        // Each original wetland core reaches its full shelf before the outer transition begins.
        let wet_weight = smoothstep(0.60, 0.70, climate.humidity)
            * (1.0 - smoothstep(0.18, 0.30, climate.continentalness))
            * (1.0 - smoothstep(32.0, 60.0, mountain_height));
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
            // A second coherent scale supplies banks inside the broad, sometimes entirely wet detail field.
            (sea - 0.4
                + 2.0 * climate.detail
                + 0.4 * inland
                + 1.8
                    * value2(
                        self.seed ^ 0x7765_745f_706f_6f6c,
                        i64::from(x),
                        i64::from(z),
                        24,
                    ))
            .clamp(sea - 3.0, sea + 2.0),
            wet_weight,
        );
        let uncarved = blend(
            uncarved,
            sea + 4.0 + 32.0 * (climate.island - 0.38) + detail_height,
            island_weight,
        );
        let uncarved_height = uncarved.round().clamp(4.0, f64::from(SURFACE_MAX_HEIGHT)) as i16;
        // Classify the completed shelf: partially blended high slopes cannot retain wetland ecology.
        let landform = if island {
            Landform::Island
        } else if climate.continentalness < -0.02 {
            Landform::Coast
        } else if wet && uncarved_height <= SURFACE_SEA_LEVEL + 3 {
            Landform::Wetland
        } else if mountain_height * (1.0 - wet_weight) > 32.0 {
            Landform::Mountain
        } else if arid && climate.erosion < -0.05 {
            Landform::Mesa
        } else if arid {
            Landform::Dunes
        } else if climate.erosion < 0.15 {
            Landform::RollingHills
        } else {
            Landform::Lowland
        };
        let relief = if landform == Landform::Mesa {
            self.eroded_spire_relief(x, z, climate, mountain_height * (1.0 - wet_weight))
        } else {
            0.0
        };
        let uncarved_height = (f64::from(uncarved_height) + relief)
            .round()
            .clamp(4.0, f64::from(SURFACE_MAX_HEIGHT)) as i16;
        let lake = if landform == Landform::Wetland {
            None
        } else {
            self.inland_lake_basin(
                x,
                z,
                climate,
                mountain_height,
                island_weight,
                uncarved_height,
            )
            .map(|(lake_height, lake_water)| {
                // Lake eligibility changes discretely at the wetland climate
                // boundary. Fade its geometry and water into the wetland shelf
                // so that neighboring samples do not form a one-column cliff.
                let lake_weight = 1.0 - wet_weight;
                let height = blend(
                    f64::from(uncarved_height),
                    f64::from(lake_height),
                    lake_weight,
                )
                .round()
                .clamp(4.0, f64::from(SURFACE_MAX_HEIGHT)) as i16;
                let water = lake_water
                    .map(|level| {
                        blend(f64::from(SURFACE_SEA_LEVEL), f64::from(level), lake_weight).round()
                            as i16
                    })
                    .filter(|&level| height < level);
                (height, water)
            })
        };
        let uncarved_height = lake.map_or(uncarved_height, |(height, _)| height);
        let valley_strength = if rivers && !island {
            1.0 - smoothstep(0.015, 0.075, climate.river_distance)
        } else {
            0.0
        };
        // This stateless field stage has one connected water plane. Preserve
        // the authored valley depth here; surface generation fills the water
        // column up to that plane so banks never acquire a floating slab.
        let river_level = SURFACE_SEA_LEVEL;
        let bed = uncarved_height.min(river_level - 3);
        let height = (f64::from(uncarved_height)
            + f64::from(bed - uncarved_height) * valley_strength)
            .round() as i16;
        let river = rivers
            && uncarved_height >= SURFACE_SEA_LEVEL
            && valley_strength > 0.55
            && height < river_level;
        let lake_water = lake.and_then(|(_, water)| water);
        let water_level = if lake_water.is_some() {
            lake_water
        } else if height < SURFACE_SEA_LEVEL {
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
        for probe in 0..256 {
            let x = (probe % 8) as i32 * 8;
            let z = ((probe / 8) % 8) as i32 * 8;
            let detail = [-0.4, -0.1, 0.3, 0.7][probe / 64];
            let sample = terrain.shape(
                x,
                z,
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
    fn blended_wetland_outside_core_does_not_become_an_elevated_lake() {
        let terrain = TerrainFields::new(71839);
        let (x, z) = (-1404, -1466);
        let climate = TerrainClimate {
            continentalness: 0.17,
            temperature: 0.65,
            humidity: 0.90,
            erosion: -0.55,
            weirdness: 0.0,
            ridge: 0.64,
            river_distance: 0.30,
            detail: 0.0,
            island: 0.0,
        };
        let inland = smoothstep(-0.08, 0.35, climate.continentalness);
        let mountainous = inland * (1.0 - smoothstep(-0.55, 0.38, climate.erosion));
        let mountain_height = 128.0 * mountainous * climate.ridge.powi(2);
        assert!((32.0..48.0).contains(&mountain_height));

        let possible_lake =
            terrain.inland_lake_basin(x, z, climate, mountain_height, 0.0, SURFACE_SEA_LEVEL);
        assert!(
            matches!(possible_lake, Some((height, Some(level)))
                if height > SURFACE_SEA_LEVEL + 3 && level > SURFACE_SEA_LEVEL),
            "fixture must exercise the elevated-lake candidate: {possible_lake:?}"
        );

        let sample = terrain.shape(x, z, climate, false);
        assert_eq!(sample.landform, Landform::Wetland, "{sample:?}");
        assert!(
            sample.uncarved_height <= SURFACE_SEA_LEVEL + 3,
            "blended wetland shelf was raised by an inland lake: {sample:?}"
        );
        assert!(
            sample
                .water_level
                .is_none_or(|level| level <= SURFACE_SEA_LEVEL),
            "blended wetland acquired elevated lake water: {sample:?}"
        );
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
    fn inland_lakes_are_elevated_and_independent_of_the_river_toggle() {
        let terrain = TerrainFields::new(71839);
        let mut elevated_water = 0;
        let mut inland_elevated_water = 0;
        for z in (-2048..=2048).step_by(16) {
            for x in (-2048..=2048).step_by(16) {
                let sample = terrain.sample(x, z, false);
                let Some(level) = sample
                    .water_level
                    .filter(|level| *level > SURFACE_SEA_LEVEL)
                else {
                    continue;
                };
                assert!(
                    sample.height < level,
                    "lake surface must cover its basin: {sample:?}"
                );
                assert_eq!(sample, terrain.sample(x, z, false));
                assert_eq!(Some(level), terrain.sample(x, z, true).water_level);
                elevated_water += 1;
                inland_elevated_water += usize::from(sample.climate.continentalness > 0.08);
            }
        }
        assert!(elevated_water > 0, "the inland terrain contains no lakes");
        assert!(
            inland_elevated_water > 0,
            "elevated lakes occur only on the coastal shelf"
        );
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
                if enabled.river
                    && disabled.height > SURFACE_SEA_LEVEL
                    && disabled.water_level.is_none()
                {
                    inland_river = true;
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

#[cfg(test)]
#[path = "terrain_ecology_tests.rs"]
mod ecology_regression_tests;

#[cfg(test)]
mod eroded_morphology_regressions {
    use super::super::{surface_biome_selector, surface_biomes::SurfaceBiome};
    use super::*;

    #[test]
    fn natural_eroded_fixture_contains_a_tall_isolated_spire() {
        let fields = TerrainFields::new(71839);
        let center = [-16256, -16384];
        let mut eroded = 0;
        let mut peaks = 0;
        for dx in (-64..64).step_by(2) {
            for dz in (-64..64).step_by(2) {
                let xy = [center[0] + dx, center[1] + dz];
                let sample = fields.sample(xy[0], xy[1], false);
                if surface_biome_selector::select(71839, xy, sample) != SurfaceBiome::ErodedBadlands
                {
                    continue;
                }
                eroded += 1;
                let ring = [
                    [12, 0],
                    [12, 12],
                    [0, 12],
                    [-12, 12],
                    [-12, 0],
                    [-12, -12],
                    [0, -12],
                    [12, -12],
                ];
                let rim = ring
                    .into_iter()
                    .map(|d| fields.sample(xy[0] + d[0], xy[1] + d[1], false).height)
                    .max()
                    .unwrap();
                peaks += usize::from(i32::from(sample.height) - i32::from(rim) >= 12);
            }
        }
        assert!(
            eroded > 100,
            "fixture must actually contain eroded badlands"
        );
        assert!(peaks > 0, "missing tall isolated terracotta-spire terrain");
    }
    #[test]
    fn spire_uncarved_height_is_independent_of_river_setting() {
        let fields = TerrainFields::new(71839);
        for x in -16320..-16192 {
            for z in (-16448..-16320).step_by(4) {
                let dry = fields.sample(x, z, false);
                let river = fields.sample(x, z, true);
                assert_eq!(dry.uncarved_height, river.uncarved_height);
                assert!(river.height <= dry.height);
            }
        }
    }

    #[test]
    fn spire_fields_remain_bounded_at_signed_coordinate_extremes() {
        for seed in [0, 71839, u64::from(u32::MAX)] {
            let fields = TerrainFields::new(seed);
            for x in [i32::MIN, i32::MIN + 24, -24, 0, 24, i32::MAX - 24, i32::MAX] {
                for z in [i32::MIN, i32::MAX, 0] {
                    let sample = fields.sample(x, z, true);
                    assert!((4..=SURFACE_MAX_HEIGHT).contains(&sample.height));
                    assert!(sample.climate.detail.is_finite());
                }
            }
        }
    }
}
