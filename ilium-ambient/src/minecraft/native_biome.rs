//! Java 1.19.3 `BiomeManager.getBiome` coordinate selection.
//! Source: pinned client SHA-256 b7228c23dbc8988129561af3918dd469577de842d2eb3c7dabe00316bf9a44d6,
//! captured BiomeManager, LinearCongruentialGenerator and Mth disassembly.
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("native biome coordinate exceeds Java signed block range")]
    Coordinate,
}

/// Guava 31.1 `sha256().hashLong(seed).asLong()`: both long byte operations
/// are little-endian. Zero is a valid real seed; absence must use `Option`.
pub fn hash_zoom_seed(world_seed: i64) -> i64 {
    let digest = Sha256::digest(world_seed.to_le_bytes());
    let mut low = [0_u8; 8];
    low.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(low)
}

fn next(seed: i64, salt: i64) -> i64 {
    seed.wrapping_mul(
        seed.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    )
    .wrapping_add(salt)
}

fn fiddle(seed: i64) -> f64 {
    ((seed >> 24).rem_euclid(1024) as f64 / 1024.0 - 0.5) * 0.9
}

fn fiddled_distance(zoom_seed: i64, quart: [i32; 3], fraction: [f64; 3]) -> f64 {
    let [x, y, z] = quart;
    let mut mixed = zoom_seed;
    for coordinate in [x, y, z, x, y, z] {
        mixed = next(mixed, i64::from(coordinate));
    }
    let x_fiddle = fiddle(mixed);
    mixed = next(mixed, zoom_seed);
    let y_fiddle = fiddle(mixed);
    mixed = next(mixed, zoom_seed);
    let z_fiddle = fiddle(mixed);
    let square = |value: f64| value * value;
    square(fraction[2] + z_fiddle) + square(fraction[1] + y_fiddle) + square(fraction[0] + x_fiddle)
}

/// Returns the exact stored quart coordinate chosen from eight neighboring
/// corners. A caller must still require that quart's qualified decoded chunk
/// and actual biome palette entry; missing data cannot become a default biome.
pub fn choose_quart(world_seed: i64, block: [i32; 3]) -> Result<[i32; 3], Error> {
    let adjusted = [
        block[0].checked_sub(2).ok_or(Error::Coordinate)?,
        block[1].checked_sub(2).ok_or(Error::Coordinate)?,
        block[2].checked_sub(2).ok_or(Error::Coordinate)?,
    ];
    let base = adjusted.map(|coordinate| coordinate >> 2);
    let fraction = adjusted.map(|coordinate| f64::from(coordinate & 3) / 4.0);
    let zoom_seed = hash_zoom_seed(world_seed);
    let mut best = None;
    for index in 0..8_u8 {
        let bits = [4_u8, 2, 1];
        let mut quart = [0_i32; 3];
        let mut offset = [0.0_f64; 3];
        for axis in 0..3 {
            let upper = index & bits[axis] != 0;
            quart[axis] = base[axis]
                .checked_add(i32::from(upper))
                .ok_or(Error::Coordinate)?;
            offset[axis] = fraction[axis] - if upper { 1.0 } else { 0.0 };
        }
        let distance = fiddled_distance(zoom_seed, quart, offset);
        if best.is_none_or(|(_, current)| distance < current) {
            best = Some((quart, distance));
        }
    }
    best.map(|(quart, _)| quart).ok_or(Error::Coordinate)
}

#[cfg(test)]
#[path = "native_biome_tests.rs"]
mod tests;
