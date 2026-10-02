//! Elevation grids: measured bodies decoded from embedded PNGs, and fictional
//! worlds generated from a seed. Every grid is equirectangular, row 0 north,
//! column 0 at longitude -180, with elevations in metres relative to the
//! body's zero level.

use super::settings::WorldId;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};

const MANIFEST: &str = include_str!("../../../assets/topography/manifest.json");

#[derive(Debug, Deserialize)]
struct Manifest {
    bodies: Vec<ManifestBody>,
}

#[derive(Debug, Deserialize)]
struct ManifestBody {
    id: String,
    name: String,
    min_m: f32,
    max_m: f32,
    zero_m: f32,
}

#[derive(Debug)]
pub struct Heightfield {
    pub width: usize,
    pub height: usize,
    /// Row-major elevations in metres relative to the zero level.
    pub meters: Vec<f32>,
    pub min_m: f32,
    pub max_m: f32,
    pub name: String,
}

impl Heightfield {
    /// Bilinear sample. Longitude wraps; latitude clamps at the poles.
    pub fn sample(&self, longitude: f64, latitude: f64) -> f32 {
        let x = (longitude + 180.0).rem_euclid(360.0) / 360.0 * self.width as f64 - 0.5;
        let y = ((90.0 - latitude) / 180.0 * self.height as f64 - 0.5)
            .clamp(0.0, (self.height - 1) as f64);
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = (x - x0) as f32;
        let fy = (y - y0) as f32;
        let column = |value: f64| (value as i64).rem_euclid(self.width as i64) as usize;
        let (xa, xb) = (column(x0), column(x0 + 1.0));
        let ya = y0 as usize;
        let yb = (ya + 1).min(self.height - 1);
        let at = |column: usize, row: usize| self.meters[row * self.width + column];
        let top = at(xa, ya) + (at(xb, ya) - at(xa, ya)) * fx;
        let bottom = at(xa, yb) + (at(xb, yb) - at(xa, yb)) * fx;
        top + (bottom - top) * fy
    }

    fn from_meters(width: usize, height: usize, meters: Vec<f32>, name: String) -> Self {
        let min_m = meters.iter().copied().fold(f32::INFINITY, f32::min);
        let max_m = meters.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        Self {
            width,
            height,
            meters,
            min_m,
            max_m,
            name,
        }
    }
}

fn png_bytes(world: WorldId) -> Option<&'static [u8]> {
    Some(match world {
        WorldId::Earth => include_bytes!("../../../assets/topography/earth.png"),
        WorldId::Moon => include_bytes!("../../../assets/topography/moon.png"),
        WorldId::Mars => include_bytes!("../../../assets/topography/mars.png"),
        WorldId::Venus => include_bytes!("../../../assets/topography/venus.png"),
        WorldId::Mercury => include_bytes!("../../../assets/topography/mercury.png"),
        WorldId::Ceres => include_bytes!("../../../assets/topography/ceres.png"),
        _ => return None,
    })
}

fn manifest_id(world: WorldId) -> &'static str {
    match world {
        WorldId::Earth => "earth",
        WorldId::Moon => "moon",
        WorldId::Mars => "mars",
        WorldId::Venus => "venus",
        WorldId::Mercury => "mercury",
        _ => "ceres",
    }
}

/// Decode one measured body. `Err` carries a message for the status line.
pub fn load_real(world: WorldId) -> Result<Heightfield, String> {
    let bytes = png_bytes(world).ok_or("not a measured world")?;
    let manifest: Manifest =
        serde_json::from_str(MANIFEST).map_err(|error| format!("topography manifest: {error}"))?;
    let entry = manifest
        .bodies
        .iter()
        .find(|body| body.id == manifest_id(world))
        .ok_or("topography manifest has no such body")?;
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|error| format!("topography image: {error}"))?
        .to_luma16();
    let (width, height) = (image.width() as usize, image.height() as usize);
    let span = entry.max_m - entry.min_m;
    // Samples carry a 12-bit level in their high bits (see build_assets.py).
    let meters = image
        .as_raw()
        .iter()
        .map(|sample| entry.min_m + f32::from(sample >> 4) / 4095.0 * span - entry.zero_m)
        .collect();
    Ok(Heightfield::from_meters(
        width,
        height,
        meters,
        entry.name.clone(),
    ))
}

/// Deterministic value noise on the unit sphere, so generated worlds have no
/// seam at longitude 180 and no pinching at the poles.
struct SphereNoise {
    seed: u32,
}

impl SphereNoise {
    fn lattice(&self, x: i32, y: i32, z: i32) -> f32 {
        let mut hash = (x as u32).wrapping_mul(0x9e37_79b1)
            ^ (y as u32).wrapping_mul(0x85eb_ca6b)
            ^ (z as u32).wrapping_mul(0xc2b2_ae35)
            ^ self.seed.wrapping_mul(0x27d4_eb2f);
        hash ^= hash >> 15;
        hash = hash.wrapping_mul(0x2c1b_3c6d);
        hash ^= hash >> 12;
        hash = hash.wrapping_mul(0x297a_2d39);
        hash ^= hash >> 15;
        (hash >> 8) as f32 / 16_777_215.0
    }

    fn value(&self, point: [f32; 3]) -> f32 {
        let floor = point.map(f32::floor);
        let fraction = [
            point[0] - floor[0],
            point[1] - floor[1],
            point[2] - floor[2],
        ];
        let smooth = fraction.map(|value| value * value * (3.0 - 2.0 * value));
        let (x, y, z) = (floor[0] as i32, floor[1] as i32, floor[2] as i32);
        let mut total = 0.0;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let weight = if dx == 1 { smooth[0] } else { 1.0 - smooth[0] }
                        * if dy == 1 { smooth[1] } else { 1.0 - smooth[1] }
                        * if dz == 1 { smooth[2] } else { 1.0 - smooth[2] };
                    total += weight * self.lattice(x + dx, y + dy, z + dz);
                }
            }
        }
        total
    }

    /// Fractal sum in 0..1. `ridged` folds each octave into sharp crests.
    fn fractal(&self, point: [f32; 3], octaves: u32, ridged: bool) -> f32 {
        let (mut sum, mut amplitude, mut frequency, mut norm) = (0.0, 1.0, 1.0, 0.0);
        for octave in 0..octaves {
            let offset = octave as f32 * 17.3;
            let scaled = [
                point[0] * frequency + offset,
                point[1] * frequency - offset,
                point[2] * frequency + offset * 0.5,
            ];
            let raw = self.value(scaled);
            let sample = if ridged {
                1.0 - (2.0 * raw - 1.0).abs()
            } else {
                raw
            };
            sum += amplitude * sample;
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        sum / norm
    }
}

const GENERATED_WIDTH: usize = 1024;
const GENERATED_HEIGHT: usize = 512;

fn unit_vector(longitude_deg: f32, latitude_deg: f32) -> [f32; 3] {
    let (longitude, latitude) = (longitude_deg.to_radians(), latitude_deg.to_radians());
    [
        latitude.cos() * longitude.cos(),
        latitude.sin(),
        latitude.cos() * longitude.sin(),
    ]
}

/// Generate a fictional world. Returns `None` when `stop` was raised.
pub fn generate_fictional(world: WorldId, seed: u32, stop: &AtomicBool) -> Option<Heightfield> {
    let salt = world as u32 * 7919 + 1;
    let noise = SphereNoise {
        seed: seed.wrapping_mul(2_654_435_761).wrapping_add(salt),
    };
    let mut meters = vec![0.0_f32; GENERATED_WIDTH * GENERATED_HEIGHT];
    for row in 0..GENERATED_HEIGHT {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let latitude = 90.0 - (row as f32 + 0.5) / GENERATED_HEIGHT as f32 * 180.0;
        for column in 0..GENERATED_WIDTH {
            let longitude = (column as f32 + 0.5) / GENERATED_WIDTH as f32 * 360.0 - 180.0;
            let point = unit_vector(longitude, latitude);
            meters[row * GENERATED_WIDTH + column] = match world {
                WorldId::Aeria => {
                    let base = noise.fractal(point.map(|v| v * 3.4), 6, false);
                    // A steep response keeps most of the surface under water.
                    (base - 0.56) * 16_000.0
                }
                WorldId::Pangaea => {
                    let continent = noise.fractal(point.map(|v| v * 1.15), 3, false);
                    let detail = noise.fractal(point.map(|v| v * 6.0), 5, false);
                    (continent - 0.5) * 14_000.0 + (detail - 0.5) * 3600.0 - 600.0
                }
                WorldId::Ridgeworld => {
                    let belts = noise.fractal(point.map(|v| v * 2.2), 6, true);
                    let sea = noise.fractal(point.map(|v| v * 1.3), 3, false);
                    (belts - 0.62) * 20_000.0 + (sea - 0.5) * 4000.0
                }
                _ => (noise.fractal(point.map(|v| v * 2.0), 5, false) - 0.5) * 3000.0,
            };
        }
    }
    if world == WorldId::Craterlands {
        carve_craters(&noise, &mut meters, stop)?;
    }
    Some(Heightfield::from_meters(
        GENERATED_WIDTH,
        GENERATED_HEIGHT,
        meters,
        world.label().to_owned(),
    ))
}

/// Bowl-shaped craters with raised rims, scattered over the sphere. Each is
/// applied only inside its own bounding box so the cost stays proportional to
/// the area the craters cover.
fn carve_craters(noise: &SphereNoise, meters: &mut [f32], stop: &AtomicBool) -> Option<()> {
    for index in 0..260_u32 {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let draw = |salt: i32| noise.lattice(index as i32, salt, 913);
        let longitude = draw(1) * 360.0 - 180.0;
        let latitude = (draw(2) * 2.0 - 1.0).asin().to_degrees();
        // Many small craters, a few huge basins.
        let radius_deg = 1.2 + 12.0 * draw(3).powi(4) + 3.0 * draw(4);
        let depth = radius_deg * 110.0;
        let center = unit_vector(longitude, latitude);
        let lat_span = radius_deg * 1.4;
        let lon_span = (lat_span / latitude.to_radians().cos().max(0.1)).min(180.0);
        let rows = ((90.0 - latitude - lat_span) / 180.0 * GENERATED_HEIGHT as f32).floor() as i32
            ..=((90.0 - latitude + lat_span) / 180.0 * GENERATED_HEIGHT as f32).ceil() as i32;
        for row in rows {
            if row < 0 || row >= GENERATED_HEIGHT as i32 {
                continue;
            }
            let pixel_latitude = 90.0 - (row as f32 + 0.5) / GENERATED_HEIGHT as f32 * 180.0;
            let first =
                ((longitude - lon_span + 180.0) / 360.0 * GENERATED_WIDTH as f32).floor() as i32;
            let last =
                ((longitude + lon_span + 180.0) / 360.0 * GENERATED_WIDTH as f32).ceil() as i32;
            for raw_column in first..=last {
                let column = raw_column.rem_euclid(GENERATED_WIDTH as i32) as usize;
                let pixel_longitude =
                    (column as f32 + 0.5) / GENERATED_WIDTH as f32 * 360.0 - 180.0;
                let point = unit_vector(pixel_longitude, pixel_latitude);
                let dot = center[0] * point[0] + center[1] * point[1] + center[2] * point[2];
                let distance = dot.clamp(-1.0, 1.0).acos().to_degrees() / radius_deg;
                let profile = if distance < 1.0 {
                    -depth * (1.0 - distance * distance)
                } else if distance < 1.4 {
                    let rim = (distance - 1.0) / 0.4;
                    depth * 0.35 * (1.0 - rim) * (1.0 - rim)
                } else {
                    continue;
                };
                meters[row as usize * GENERATED_WIDTH + column] += profile;
            }
        }
    }
    Some(())
}

/// Load or generate one world. `None` when generation was cancelled.
pub fn load_world(
    world: WorldId,
    seed: u32,
    stop: &AtomicBool,
) -> Option<Result<Heightfield, String>> {
    if world.is_fictional() {
        generate_fictional(world, seed, stop).map(Ok)
    } else {
        Some(load_real(world))
    }
}
