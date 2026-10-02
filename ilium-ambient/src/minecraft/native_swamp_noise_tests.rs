//! Source-derived threshold witnesses; native-runtime parity remains unverified.
use super::*;
#[test]
fn pinned_initialization_consumes_three_doubles_before_shuffle() {
    let sampler = SwampNoise::new();
    assert_eq!(
        &sampler.permutation[..16],
        &[64, 175, 124, 148, 10, 239, 244, 91, 138, 73, 228, 171, 27, 134, 77, 122]
    );
    assert_eq!(
        &sampler.permutation[240..],
        &[179, 246, 20, 107, 168, 97, 229, 101, 155, 62, 47, 58, 116, 243, 105, 36]
    );
}
#[test]
fn threshold_witnesses_match_source_derived_arithmetic() {
    let sampler = SwampNoise::new();
    for (x, z, expected, packed) in [
        (-7, -16, -0.10000514387117068, 5_011_004_u32),
        (-49, -14, -0.10024303768748262, 5_011_004),
        (32, 60, -0.10026036895978187, 5_011_004),
        (35, -26, -0.09994144178063298, 6_975_545),
    ] {
        let actual = sampler.sample(f64::from(x) * 0.0225, f64::from(z) * 0.0225);
        assert!(
            (actual - expected).abs() < 1e-12,
            "{x},{z}: {actual} != {expected}"
        );
        assert_eq!(
            sampler.grass_rgb(x, z),
            [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8]
        );
    }
}
