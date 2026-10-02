//! Pinned Java 1.19.3 BIOME_INFO_NOISE, octave zero and seed 2345.
//! Complete dsq/dsr/dfa/dec/ddq/aoc bytecode is retained in
//! native-swamp-noise-001. This port is source-derived, not a game-runtime oracle.

const MASK_48: u64 = (1 << 48) - 1;
const MULTIPLIER: u64 = 25_214_903_917;
const GRADIENT: [[i8; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

struct WorldgenRandom {
    legacy_seed: u64,
}
impl WorldgenRandom {
    fn new() -> Self {
        // The dfa superclass constructor's overridden setSeed returns while
        // its delegated source is null. It does not reseed dec(2345).
        Self {
            legacy_seed: (2345_u64 ^ MULTIPLIER) & MASK_48,
        }
    }
    fn next_bits(&mut self, bits: u32) -> u32 {
        self.legacy_seed = self.legacy_seed.wrapping_mul(MULTIPLIER).wrapping_add(11) & MASK_48;
        (self.legacy_seed >> (48 - bits)) as u32
    }
    fn next_double(&mut self) -> f64 {
        // BitRandomSource(ddq) default, inherited by WorldgenRandom.
        let upper = u64::from(self.next_bits(26));
        let lower = u64::from(self.next_bits(27));
        ((upper << 27) + lower) as f64 * 1.110_223_024_625_156_5e-16
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0);
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next_bits(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31) as i32;
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }
}

/// Immutable single-octave native sampler. One instance can be reused for all
/// samples in a prepared generation; no world seed or global cache is involved.
pub struct SwampNoise {
    permutation: [u8; 256],
}
impl Default for SwampNoise {
    fn default() -> Self {
        Self::new()
    }
}
impl SwampNoise {
    pub fn new() -> Self {
        let mut random = WorldgenRandom::new();
        // SimplexNoise constructor consumes all three offsets even though
        // PerlinSimplexNoise.getValue(..., false) does not use them.
        for _ in 0..3 {
            let _ = random.next_double() * 256.0;
        }
        let mut permutation = [0_u8; 256];
        for (index, slot) in permutation.iter_mut().enumerate() {
            *slot = index as u8;
        }
        for index in 0..256 {
            let next = index + random.next_int((256 - index) as i32) as usize;
            permutation.swap(index, next);
        }
        Self { permutation }
    }
    fn p(&self, index: i32) -> i32 {
        i32::from(self.permutation[(index & 255) as usize])
    }
    fn corner(index: i32, x: f64, y: f64) -> f64 {
        let z = 0.0_f64;
        let mut t = 0.5 - x * x;
        t -= y * y;
        t -= z * z;
        if t < 0.0 {
            return 0.0;
        }
        t *= t;
        let gradient = GRADIENT[index as usize];
        let dot =
            f64::from(gradient[0]) * x + f64::from(gradient[1]) * y + f64::from(gradient[2]) * z;
        t * t * dot
    }
    fn floor(value: f64) -> i32 {
        let truncated = value as i32;
        if value < f64::from(truncated) {
            truncated.wrapping_sub(1)
        } else {
            truncated
        }
    }
    pub fn sample(&self, x: f64, z: f64) -> f64 {
        let root_three = 3.0_f64.sqrt();
        let f2 = 0.5 * (root_three - 1.0);
        let g2 = (3.0 - root_three) / 6.0;
        let skew = (x + z) * f2;
        let i = Self::floor(x + skew);
        let j = Self::floor(z + skew);
        let unskew = f64::from(i.wrapping_add(j)) * g2;
        let x0 = x - (f64::from(i) - unskew);
        let z0 = z - (f64::from(j) - unskew);
        let (i1, j1) = if x0 > z0 { (1, 0) } else { (0, 1) };
        let x1 = (x0 - f64::from(i1)) + g2;
        let z1 = (z0 - f64::from(j1)) + g2;
        let x2 = (x0 - 1.0) + 2.0 * g2;
        let z2 = (z0 - 1.0) + 2.0 * g2;
        let ii = i & 255;
        let jj = j & 255;
        let k0 = self.p(ii + self.p(jj)) % 12;
        let k1 = self.p(ii + i1 + self.p(jj + j1)) % 12;
        let k2 = self.p(ii + 1 + self.p(jj + 1)) % 12;
        let c0 = Self::corner(k0, x0, z0);
        let c1 = Self::corner(k1, x1, z1);
        let c2 = Self::corner(k2, x2, z2);
        70.0 * ((c0 + c1) + c2)
    }
    pub fn grass_rgb(&self, block_x: i32, block_z: i32) -> [u8; 3] {
        let value = self.sample(f64::from(block_x) * 0.0225, f64::from(block_z) * 0.0225);
        let packed = if value < -0.1 {
            5_011_004_u32
        } else {
            6_975_545_u32
        };
        [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8]
    }
}

#[cfg(test)]
#[path = "native_swamp_noise_tests.rs"]
mod tests;
