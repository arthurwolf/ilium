//! Deterministic pseudo-random numbers (SplitMix64) for draws and bootstraps.
//!
//! The simulation must be reproducible for a given seed on every platform, so
//! the generator is implemented here instead of pulling a dependency whose
//! stream could change between versions.

/// SplitMix64 generator.
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    /// Creates a generator from a seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    /// Uniform integer in `0..bound`; returns 0 when `bound` is 0.
    pub fn next_below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        // The modulo bias is below 2^-40 for the bounds used here.
        (self.next_u64() % bound as u64) as usize
    }

    /// Uniform float in `[0, 1)`.
    pub fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut first = SeededRng::new(12_345);
        let mut second = SeededRng::new(12_345);
        for _ in 0..100 {
            assert_eq!(first.next_u64(), second.next_u64());
        }
        let mut other = SeededRng::new(12_346);
        assert_ne!(SeededRng::new(12_345).next_u64(), other.next_u64());
    }

    #[test]
    fn bounded_values_stay_in_range() {
        let mut rng = SeededRng::new(7);
        for _ in 0..1_000 {
            assert!(rng.next_below(17) < 17);
            let unit = rng.next_unit();
            assert!((0.0..1.0).contains(&unit));
        }
        assert_eq!(rng.next_below(0), 0);
    }
}
