//! Versioned, allocation-free value noise at integer world coordinates.
//! Period 0 means 1; fBm uses at most eight octaves; zero octaves means zero.
//! Integer hashing wraps deliberately. No process RNG, clocks or float hashes.

pub const MAX_OCTAVES: u8 = 8;

pub fn mix64(mut n: u64) -> u64 {
    n = n.wrapping_add(0x9e37_79b9_7f4a_7c15);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    n ^ (n >> 31)
}

pub fn hash2(seed: u64, x: i64, z: i64) -> u64 {
    let a = mix64((x as u64).wrapping_add(0x632b_e59b_d9b4_e019));
    let b = mix64((z as u64).wrapping_add(0x8515_7af5_d66d_3e2f));
    mix64(seed ^ a ^ b.rotate_left(32))
}

fn lattice(seed: u64, x: i64, z: i64) -> f64 {
    (hash2(seed, x, z) >> 11) as f64 / 9_007_199_254_740_992.0 * 2.0 - 1.0
}

fn fade(t: f64) -> f64 {
    (t * t * t * (t * (t * 6.0 - 15.0) + 10.0)).clamp(0.0, 1.0)
}

/// Smooth lattice interpolation in [-1, 1]. Euclidean splitting preserves
/// negative-coordinate seams and avoids converting large coordinates to floats.
pub fn value2(seed: u64, x: i64, z: i64, period: u32) -> f64 {
    let p = i64::from(period.max(1));
    let (ix, iz) = (x.div_euclid(p), z.div_euclid(p));
    let tx = fade(x.rem_euclid(p) as f64 / p as f64);
    let tz = fade(z.rem_euclid(p) as f64 / p as f64);
    let (jx, jz) = (ix.wrapping_add(1), iz.wrapping_add(1));
    let a = lattice(seed, ix, iz);
    let b = lattice(seed, jx, iz);
    let c = lattice(seed, ix, jz);
    let d = lattice(seed, jx, jz);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    (top + (bottom - top) * tz).clamp(-1.0, 1.0)
}

/// Normalized octave sum in [-1, 1], with halving periods and amplitudes.
/// Each octave has its own seed, including octaves whose period reaches 1.
pub fn fbm2(seed: u64, x: i64, z: i64, period: u32, octaves: u8) -> f64 {
    let (mut sum, mut weight, mut amplitude) = (0.0, 0.0, 1.0);
    let mut period = period.max(1);
    for octave in 0..octaves.min(MAX_OCTAVES) {
        let seed = mix64(seed.wrapping_add(u64::from(octave)));
        sum += amplitude * value2(seed, x, z, period);
        weight += amplitude;
        amplitude *= 0.5;
        period = (period / 2).max(1);
    }
    if weight == 0.0 {
        0.0
    } else {
        (sum / weight).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_hash_vectors_are_frozen() {
        assert_eq!(mix64(0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(mix64(1), 0x910a_2dec_8902_5cc1);
        assert_eq!(hash2(7, -1, 2), 0x2646_67eb_fd50_3a90);
        assert_eq!(hash2(0, 0, 0), 0xfe9e_596d_6ed1_9427);
        assert_eq!(hash2(u64::MAX, i64::MIN, i64::MAX), 0x7262_6bd0_9f4a_5982);
        assert!((value2(7, -8, 24, 16) + 0.07988965060858311).abs() < 1e-14);
        assert!((fbm2(7, -8, 24, 64, 4) - 0.16060722958907878).abs() < 1e-14);
    }

    #[test]
    fn negative_midpoint_matches_four_corner_average() {
        let corners = [(-1, 1), (0, 1), (-1, 2), (0, 2)];
        let mean = corners.iter().map(|&(x, z)| lattice(7, x, z)).sum::<f64>() / 4.0;
        assert!((value2(7, -8, 24, 16) - mean).abs() < 1e-15);
        assert_eq!(value2(7, -16, 32, 16), lattice(7, -1, 2));
    }

    #[test]
    fn samples_have_no_jump_at_positive_or_negative_lattice_boundaries() {
        for seed in [0, 9, u64::MAX] {
            for edge in -8..=8 {
                let x = edge * 64;
                for z in [-65, -1, 0, 23, 64] {
                    let center = value2(seed, x, z, 64);
                    assert!((center - value2(seed, x - 1, z, 64)).abs() < 0.001);
                    assert!((center - value2(seed, x + 1, z, 64)).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn extremes_periods_and_octaves_are_bounded() {
        for x in [i64::MIN, -1, 0, 1, i64::MAX] {
            for z in [i64::MIN, -1, 0, i64::MAX] {
                for p in [0, 1, 3, 64, u32::MAX] {
                    let v = value2(u64::MAX, x, z, p);
                    let f = fbm2(u64::MAX, x, z, p, 255);
                    assert!(v.is_finite() && (-1.0..=1.0).contains(&v));
                    assert!(f.is_finite() && (-1.0..=1.0).contains(&f));
                    assert_eq!(f, fbm2(u64::MAX, x, z, p, MAX_OCTAVES));
                }
            }
        }
        assert_eq!(fbm2(4, -7, 8, 0, 0), 0.0);
        assert_eq!(value2(4, -7, 8, 0), value2(4, -7, 8, 1));
    }

    #[test]
    fn seeds_and_coordinates_are_not_ignored() {
        let a: Vec<_> = (-32..32).map(|x| fbm2(1, x, -17, 64, 4)).collect();
        let b: Vec<_> = (-32..32).map(|x| fbm2(2, x, -17, 64, 4)).collect();
        assert_ne!(a, b);
        assert!(a.windows(2).any(|pair| pair[0] != pair[1]));
    }
}
