//! Pure surface terrain, not a full voxel simulation or feature catalog.
//! x/z are i32 block coordinates. y is a block's bottom; heights and air
//! intervals are exclusive at the top. Blocks below y=0 are outside the world.
//! Rivers are level-water meandering ribbons, not simulated drainage basins.
use super::noise::{fbm2, hash2, value2};

pub const GENERATOR_VERSION: u32 = 1;
pub const CHUNK_SIDE: i32 = 16;
pub const SEA_LEVEL: i16 = 18;
pub const MAX_HEIGHT: i16 = 80;
const REGION_SIDE: i64 = 96; // Multiple of CHUNK_SIDE; sites never cross regions.

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Taiga,
    Tundra,
    Desert,
    Savanna,
    Swamp,
    Alpine,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Bedrock,
    Stone,
    Sandstone,
    Dirt,
    Grass,
    Sand,
    Snow,
    Mud,
    Water,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    pub temperature: f64,     // 0..=1, including altitude cooling.
    pub moisture: f64,        // 0..=1.
    pub continentalness: f64, // -1..=1; not a distance to the ocean.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirSpan {
    pub bottom: i16,
    pub top: i16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub height: i16, // Exclusive top of the solid envelope; cave gaps are separate.
    pub surface_height: i16, // After river erosion, before ravine carving.
    pub water_level: Option<i16>,
    pub climate: Climate,
    pub biome: Biome,
    pub river: bool,  // Actual water in an inland ribbon, not merely a noise mask.
    pub ravine: bool, // Actual reduction of this column's solid height.
    pub cave: Option<AirSpan>, // Real sidewall grotto; roof remains solid.
}

impl Column {
    /// The authoritative occupancy/material query; a dark solid is not air.
    pub fn block(&self, y: i16) -> Option<Material> {
        use Material::*;
        if y < 0 {
            return None;
        }
        if y >= self.height {
            return self.water_level.filter(|&level| y < level).map(|_| Water);
        }
        if self.cave.is_some_and(|gap| y >= gap.bottom && y < gap.top) {
            return None;
        }
        if y == 0 {
            return Some(Bedrock);
        }
        let sandy =
            self.water_level.is_some() || matches!(self.biome, Biome::Desert | Biome::Beach);
        let depth = self.surface_height - 1 - y;
        Some(if depth == 0 {
            if sandy {
                Sand
            } else {
                match self.biome {
                    Biome::Tundra | Biome::Alpine => Snow,
                    Biome::Swamp => Mud,
                    _ => Grass,
                }
            }
        } else if depth < 4 {
            if sandy {
                Sand
            } else {
                Dirt
            }
        } else if (y as u16 / 4).is_multiple_of(4) {
            Sandstone
        } else {
            Stone
        })
    }

    pub fn is_solid(&self, y: i16) -> bool {
        self.block(y)
            .is_some_and(|material| material != Material::Water)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Terrain {
    seed: u64,
    rivers: bool,
    ravines: bool,
    caves: bool,
}

#[derive(Clone, Copy)]
struct Site {
    x: i64,
    z: i64,
    swap: bool,
    length: i64,
    width: i64,
    depth: i16,
    floor: i16,
}

impl Site {
    fn local(self, x: i64, z: i64) -> (i64, i64) {
        let (dx, dz) = (x - self.x, z - self.z);
        if self.swap {
            (dz, dx)
        } else {
            (dx, dz)
        }
    }
    fn world(self, u: i64, v: i64) -> (i64, i64) {
        if self.swap {
            (self.x + v, self.z + u)
        } else {
            (self.x + u, self.z + v)
        }
    }
    fn cave_at(self, u: i64, v: i64) -> bool {
        u.abs() <= 1 && (-self.width - 4..=-self.width - 1).contains(&v)
    }
    fn affects(self, x: i64, z: i64) -> bool {
        let (u, v) = self.local(x, z);
        (u.abs() <= self.length && v.abs() <= self.width) || self.cave_at(u, v)
    }
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn classify(c: Climate, natural: i16, height: i16, river: bool) -> Biome {
    if natural < SEA_LEVEL {
        Biome::Ocean
    } else if !river && height <= SEA_LEVEL + 1 {
        Biome::Beach
    } else if natural > 58 {
        Biome::Alpine
    } else if c.temperature < 0.20 {
        Biome::Tundra
    } else if c.temperature < 0.35 {
        Biome::Taiga
    } else if c.temperature > 0.62 && c.moisture < 0.40 {
        Biome::Desert
    } else if c.temperature > 0.62 && c.moisture < 0.60 {
        Biome::Savanna
    } else if height <= SEA_LEVEL + 4 && c.moisture > 0.62 {
        Biome::Swamp
    } else if c.moisture > 0.55 {
        Biome::Forest
    } else {
        Biome::Plains
    }
}

impl Terrain {
    pub const fn new(seed: u64) -> Self {
        Self {
            seed,
            rivers: true,
            ravines: true,
            caves: true,
        }
    }

    pub const fn with_features(mut self, rivers: bool, ravines: bool, caves: bool) -> Self {
        self.rivers = rivers;
        self.ravines = ravines;
        self.caves = caves;
        self
    }

    fn river_shape(&self, x: i64, band: i64) -> (f64, f64) {
        let seed = hash2(self.seed ^ 0x0072_6976_6572, band, 0);
        let center = 96.0 + 40.0 * value2(seed, x, 0, 256) + 16.0 * value2(seed ^ 1, x, 0, 64);
        let width = 3.0 + value2(seed ^ 2, x, 0, 128);
        (center, width) // Relative to the band; center 40..152, width 2..4.
    }

    fn base(&self, x: i64, z: i64) -> Column {
        let n = |salt, period, octaves| fbm2(self.seed ^ salt, x, z, period, octaves);
        let continentalness = n(1, 512, 4);
        let ridge = 1.0 - n(2, 160, 3).abs();
        let natural = (f64::from(SEA_LEVEL)
            + 6.0
            + 20.0 * continentalness
            + 28.0 * smooth((continentalness - 0.05) / 0.5) * ridge * ridge
            + 4.0 * n(3, 48, 3))
        .round()
        .clamp(4.0, f64::from(MAX_HEIGHT)) as i16;
        let climate = Climate {
            temperature: (0.5 + 0.68 * n(4, 640, 3) - f64::from((natural - 40).max(0)) * 0.007)
                .clamp(0.0, 1.0),
            moisture: (0.5 + 0.70 * n(5, 480, 3)).clamp(0.0, 1.0),
            continentalness,
        };
        let (center, width) = self.river_shape(x, z.div_euclid(192));
        let distance = (z.rem_euclid(192) as f64 - center).abs();
        let erosion = if self.rivers {
            1.0 - smooth((distance - width) / 14.0)
        } else {
            0.0
        };
        let bed = natural.min(SEA_LEVEL - 3);
        let height = (f64::from(natural) + f64::from(bed - natural) * erosion).round() as i16;
        let river = erosion > 0.0 && natural >= SEA_LEVEL && height < SEA_LEVEL;
        Column {
            height,
            surface_height: height,
            water_level: (height < SEA_LEVEL).then_some(SEA_LEVEL),
            climate,
            biome: classify(climate, natural, height, river),
            river,
            ravine: false,
            cave: None,
        }
    }

    fn site_spec(&self, x: i64, z: i64) -> Option<Site> {
        let (rx, rz) = (x.div_euclid(REGION_SIDE), z.div_euclid(REGION_SIDE));
        let h = hash2(self.seed ^ 0x6772_6f74_746f, rx, rz);
        if !h.is_multiple_of(3) {
            return None;
        }
        Some(Site {
            x: rx * REGION_SIDE + 24 + ((h >> 8) % 48) as i64,
            z: rz * REGION_SIDE + 24 + ((h >> 16) % 48) as i64,
            swap: h & 1 != 0,
            length: 14 + ((h >> 24) % 8) as i64,
            width: 2 + ((h >> 28) % 2) as i64,
            depth: 10 + ((h >> 32) % 8) as i16,
            floor: 0,
        })
    }

    fn prepare_site(&self, mut site: Site) -> Option<Site> {
        // Check every cave column AND its side/back collar, not just the center.
        // Exactly 25 base samples; no recursion, search-until-success or I/O.
        let mut low = MAX_HEIGHT;
        for u in -2..=2 {
            for back in 1..=5 {
                let (x, z) = site.world(u, -site.width - back);
                low = low.min(self.base(x, z).height);
            }
        }
        if low < SEA_LEVEL + 8 {
            return None;
        }
        site.floor = (low - site.depth).max(SEA_LEVEL + 1);
        Some(site)
    }

    fn column(&self, x: i64, z: i64, site: Option<Site>) -> Column {
        let mut column = self.base(x, z);
        if let Some(site) = site {
            let (u, v) = site.local(x, z);
            // A cave needs a small open forecourt even when long ravines are off.
            let length = if self.ravines {
                site.length
            } else {
                site.width + 8
            };
            if (self.ravines || self.caves) && u.abs() <= length && v.abs() <= site.width {
                let strength = (site.length - u.abs() + 1).min(4) as i16;
                let cut = (column.height - site.floor).max(0) * strength / 4;
                column.height -= cut;
                column.ravine = cut > 0;
            }
            if self.caves && site.cave_at(u, v) {
                column.cave = Some(AirSpan {
                    bottom: site.floor,
                    top: site.floor + 3,
                });
            }
        }
        column
    }

    pub fn sample(&self, x: i32, z: i32) -> Column {
        let (x, z) = (i64::from(x), i64::from(z));
        let site = self
            .site_spec(x, z)
            .filter(|site| site.affects(x, z))
            .and_then(|site| self.prepare_site(site));
        self.column(x, z, site)
    }

    /// 256 columns, row-major (z then x); None for an out-of-domain chunk.
    /// A chunk lies in one 96x96 region, so its site is prepared at most once.
    pub fn chunk(&self, cx: i32, cz: i32) -> Option<Vec<Column>> {
        let side = i64::from(CHUNK_SIDE);
        let (x0, z0) = (i64::from(cx) * side, i64::from(cz) * side);
        for origin in [x0, z0] {
            if origin < i64::from(i32::MIN) || origin + side - 1 > i64::from(i32::MAX) {
                return None;
            }
        }
        let site = self
            .site_spec(x0, z0)
            .and_then(|site| self.prepare_site(site));
        let mut columns = Vec::with_capacity((CHUNK_SIDE * CHUNK_SIDE) as usize);
        for dz in 0..side {
            for dx in 0..side {
                columns.push(self.column(x0 + dx, z0 + dz, site));
            }
        }
        Some(columns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_order_and_negative_seams_match_direct_sampling() {
        let terrain = Terrain::new(7);
        for (cx, cz) in [
            (-7, -7),
            (-6, -1),
            (-1, 0),
            (0, -1),
            (5, 6),
            (6, 5),
            (-28, -21),
            (15, 1),
            (21, -22),
        ] {
            let chunk = terrain.chunk(cx, cz).unwrap();
            assert_eq!(chunk.len(), 256);
            for dz in 0..CHUNK_SIDE {
                for dx in 0..CHUNK_SIDE {
                    let expected = terrain.sample(cx * CHUNK_SIDE + dx, cz * CHUNK_SIDE + dz);
                    assert_eq!(chunk[(dz * CHUNK_SIDE + dx) as usize], expected);
                }
            }
            assert_eq!(terrain.chunk(cx, cz).unwrap(), chunk);
        }
        assert_eq!((-1i32).div_euclid(CHUNK_SIDE), -1);
        assert_eq!((-1i32).rem_euclid(CHUNK_SIDE), 15);
    }

    #[test]
    fn domain_edges_are_safe_and_invalid_chunks_are_rejected() {
        let terrain = Terrain::new(u64::MAX);
        for x in [i32::MIN, -1, 0, i32::MAX] {
            for z in [i32::MIN, 0, i32::MAX] {
                let c = terrain.sample(x, z);
                assert!((4..=MAX_HEIGHT).contains(&c.height));
                assert_eq!(c.block(-1), None);
                assert_eq!(c.block(MAX_HEIGHT), None);
            }
        }
        assert!(terrain.chunk(i32::MAX, 0).is_none());
        assert!(terrain.chunk(i32::MIN, 0).is_none());
        assert!(terrain.chunk(i32::MIN / CHUNK_SIDE, 0).is_some());
        assert!(terrain.chunk(i32::MAX / CHUNK_SIDE, 0).is_some());
    }

    #[test]
    fn biome_rules_cover_distinct_climate_and_terrain_cases() {
        let cases = [
            (0.5, 0.5, 17, 17, Biome::Ocean),
            (0.5, 0.5, 19, 19, Biome::Beach),
            (0.5, 0.5, 30, 30, Biome::Plains),
            (0.5, 0.8, 30, 30, Biome::Forest),
            (0.3, 0.5, 30, 30, Biome::Taiga),
            (0.1, 0.5, 30, 30, Biome::Tundra),
            (0.8, 0.2, 30, 30, Biome::Desert),
            (0.8, 0.5, 30, 30, Biome::Savanna),
            (0.5, 0.8, 22, 22, Biome::Swamp),
            (0.5, 0.5, 60, 60, Biome::Alpine),
        ];
        for (temperature, moisture, natural, height, expected) in cases {
            let c = Climate {
                temperature,
                moisture,
                continentalness: 0.0,
            };
            assert_eq!(classify(c, natural, height, false), expected);
        }
    }

    #[test]
    fn river_cores_are_wet_and_do_not_break_at_band_or_chunk_boundaries() {
        for seed in [0, 7, 91] {
            let terrain = Terrain::new(seed);
            for band in -2..=2 {
                let mut previous: Option<(i64, i64)> = None;
                for x in -128..=128 {
                    let (center, width) = terrain.river_shape(x, band);
                    let z = band * 192 + center.round() as i64;
                    let c = terrain.sample(x as i32, z as i32);
                    assert!(c.height <= SEA_LEVEL - 3);
                    assert_eq!(c.block(SEA_LEVEL - 1), Some(Material::Water));
                    assert!(!c.is_solid(SEA_LEVEL - 1));
                    let lo = (center - width).ceil() as i64;
                    let hi = (center + width).floor() as i64;
                    if let Some((old_lo, old_hi)) = previous {
                        assert!(lo.max(old_lo) <= hi.min(old_hi), "4-connected channel");
                    }
                    previous = Some((lo, hi));
                    // No river erosion reaches the artificial 192-block band boundary.
                    assert!(center - width - 14.0 > 0.0);
                    assert!(center + width + 14.0 < 191.0);
                }
            }
        }
    }

    #[test]
    fn generated_caves_have_roofs_floors_closed_backs_and_open_mouths() {
        let terrain = Terrain::new(7);
        // Fixed fixtures cover both orientations and caves crossing chunk boundaries.
        for (x, z, floor) in [(-440, -326, 19), (248, 32, 19), (341, -348, 40)] {
            let site = terrain
                .prepare_site(terrain.site_spec(x, z).unwrap())
                .unwrap();
            assert_eq!((site.x, site.z, site.floor), (x, z, floor));
            let at = |u, v| {
                let (x, z) = site.world(u, v);
                terrain.sample(x as i32, z as i32)
            };
            for u in -1..=1 {
                for back in 1..=4 {
                    let c = at(u, -site.width - back);
                    assert_eq!(
                        c.cave,
                        Some(AirSpan {
                            bottom: floor,
                            top: floor + 3
                        })
                    );
                    assert_eq!(c.height, c.surface_height);
                    assert!(c.height >= floor + 7, "at least four roof blocks");
                    assert!(c.is_solid(floor - 1) && c.is_solid(floor + 3));
                    for y in floor..floor + 3 {
                        assert_eq!(c.block(y), None);
                    }
                }
                // The whole trench in front of the opening is air at mouth height.
                for v in -site.width..=site.width {
                    let c = at(u, v);
                    assert!(c.height <= floor);
                    for y in floor..floor + 3 {
                        assert_eq!(c.block(y), None);
                    }
                }
                for y in floor..floor + 3 {
                    assert!(at(u, -site.width - 5).is_solid(y), "closed back");
                }
            }
            for u in [-2, 2] {
                for back in 1..=4 {
                    for y in floor..floor + 3 {
                        assert!(at(u, -site.width - back).is_solid(y), "closed side");
                    }
                }
            }
            // A real cut must expose geology rather than create a new grass cap.
            let cut = at(0, 0);
            assert!(cut.ravine && cut.height < cut.surface_height);
            assert!(matches!(
                cut.block(cut.height - 1),
                Some(Material::Stone | Material::Sandstone)
            ));
        }
    }

    #[test]
    fn sampling_order_and_seed_are_observable() {
        let terrain = Terrain::new(7);
        let a: Vec<_> = (-32..32).map(|x| terrain.sample(x, -41)).collect();
        let mut b: Vec<_> = (-32..32).rev().map(|x| terrain.sample(x, -41)).collect();
        b.reverse();
        assert_eq!(a, b);
        assert_ne!(
            a,
            (-32..32)
                .map(|x| Terrain::new(8).sample(x, -41))
                .collect::<Vec<_>>()
        );
        let c = terrain.sample(0, 0);
        assert_eq!(c.block(0), Some(Material::Bedrock));
        for c in &a {
            assert!(c.height <= c.surface_height && c.surface_height <= MAX_HEIGHT);
            assert!((0.0..=1.0).contains(&c.climate.temperature));
            assert!((0.0..=1.0).contains(&c.climate.moisture));
            assert!((-1.0..=1.0).contains(&c.climate.continentalness));
            if let Some(water) = c.water_level {
                assert!(water > c.height);
            }
        }
    }
    #[test]
    fn generated_biome_fixtures_and_origin_are_frozen() {
        let terrain = Terrain::new(7);
        let cases = [
            (-4096, -4096, Biome::Plains),
            (-4096, -3904, Biome::Taiga),
            (-4096, -3584, Biome::Ocean),
            (-4096, -3456, Biome::Beach),
            (-4096, -3392, Biome::Desert),
            (-4096, -3136, Biome::Savanna),
            (-4096, -2176, Biome::Forest),
            (-4096, -1024, Biome::Alpine),
            (-4096, 512, Biome::Swamp),
            (-3968, -3968, Biome::Tundra),
        ];
        for (x, z, biome) in cases {
            assert_eq!(terrain.sample(x, z).biome, biome);
        }
        let c = terrain.sample(0, 0);
        assert_eq!((c.height, c.surface_height, c.water_level), (22, 22, None));
        assert!((c.climate.temperature - 0.4974548262077778).abs() < 1e-13);
        assert!((c.climate.moisture - 0.17668762452716896).abs() < 1e-13);
        assert!((c.climate.continentalness + 0.08412564363964165).abs() < 1e-13);
        assert!(!c.river && !c.ravine && c.cave.is_none());
    }

    #[test]
    fn material_layers_and_air_use_exclusive_bounds() {
        let mut c = Terrain::new(7).sample(0, 0);
        assert_eq!(c.block(21), Some(Material::Grass));
        assert_eq!(c.block(20), Some(Material::Dirt));
        assert_eq!(c.block(16), Some(Material::Sandstone));
        assert_eq!(c.block(15), Some(Material::Stone));
        assert_eq!(c.block(0), Some(Material::Bedrock));
        assert_eq!(c.block(22), None);
        c.cave = Some(AirSpan { bottom: 8, top: 11 });
        assert!(c.is_solid(7) && c.is_solid(11));
        for y in 8..11 {
            assert_eq!(c.block(y), None);
        }
        c.cave = None;
        c.height = 15;
        c.water_level = Some(18);
        assert!(c.is_solid(14));
        for y in 15..18 {
            assert_eq!(c.block(y), Some(Material::Water));
        }
        assert_eq!(c.block(18), None);
    }

    #[test]
    fn chunk_boundaries_preserve_cave_occupancy_and_materials() {
        let terrain = Terrain::new(7);
        let mut chunks = std::collections::BTreeMap::new();
        for (x, z) in [(248i32, 32i32), (341, -348)] {
            for dz in -8..=8 {
                for dx in -8..=8 {
                    let (x, z) = (x + dx, z + dz);
                    let (cx, cz) = (x.div_euclid(16), z.div_euclid(16));
                    let i = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
                    let cached = chunks
                        .entry((cx, cz))
                        .or_insert_with(|| terrain.chunk(cx, cz).unwrap())[i];
                    let direct = terrain.sample(x, z);
                    for y in 0..MAX_HEIGHT {
                        assert_eq!(cached.block(y), direct.block(y));
                    }
                }
            }
        }
    }

    #[test]
    fn a_real_mouth_has_clear_fixed_isometric_sightlines() {
        let terrain = Terrain::new(7);
        let mouth = terrain.sample(-443, -326);
        assert_eq!(
            mouth.cave,
            Some(AirSpan {
                bottom: 19,
                top: 22
            })
        );
        // Camera is toward (+x,+y,+z). These nine rays cross a finite part
        // of the mouth, not a fortuitous zero-width voxel-corner gap.
        // Grid crossings are multiples of 1/4; 1/16 midpoint steps visit
        // every crossed voxel interval. Stop above the world's ceiling.
        for across in [0.25, 0.5, 0.75] {
            for up in [1.25, 1.5, 1.75] {
                for step in 0..16 * 61 {
                    let d = (step as f64 + 0.5) / 16.0;
                    let y = (19.0 + up + d).floor() as i16;
                    if y >= MAX_HEIGHT {
                        break;
                    }
                    let x = (-443.0 + 0.75 + d).floor() as i32;
                    let z = (-326.0 + across + d).floor() as i32;
                    assert_eq!(
                        terrain.sample(x, z).block(y),
                        None,
                        "occluded mouth ray at ({x},{y},{z})"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod options_tests {
    use super::*;
    #[test]
    fn disabled_rivers_restore_the_uneroded_land_and_disabled_caves_fill_gaps() {
        let original = Terrain::new(7);
        let dry = original.with_features(false, false, false);
        let mut found = false;
        for z in 30..150 {
            let wet = original.sample(248, z);
            let restored = dry.sample(248, z);
            assert!(!restored.river && !restored.ravine && restored.cave.is_none());
            if wet.river {
                assert!(restored.height >= wet.height);
                found = true;
            }
        }
        assert!(found);
        assert_eq!(
            original
                .with_features(true, true, false)
                .sample(-443, -326)
                .cave,
            None
        );
    }
    #[test]
    fn caves_without_ravines_keep_an_open_local_forecourt() {
        let cave = Terrain::new(7).with_features(true, false, true);
        let mouth = cave.sample(-443, -326);
        let span = mouth.cave.expect("retained mouth");
        for y in span.bottom..span.top {
            assert_eq!(cave.sample(-442, -326).block(y), None);
        }
        assert_eq!(
            cave.sample(-440, -312).height,
            Terrain::new(7)
                .with_features(true, false, false)
                .sample(-440, -312)
                .height
        );
    }
}
