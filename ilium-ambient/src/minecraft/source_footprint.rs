//! Bounded source closure for an orthographic saved-world camera line.
//!
//! A known-face check cannot certify unknown chunks. This request includes
//! every chunk that could contain a block whose bounded selected-pack model,
//! fluid or supported built-in reaches any viewport pixel during the line.
//! `support` adds a chunk ring for adjacent-state and blend-radius-two biome
//! lookups. The caller must decode and qualify ALL requested support chunks;
//! rejected or absent chunks reject the candidate route, never become air.
use super::coverage::Line;
use crate::voxel_landscape::assets::{
    budget::{ByteBudget, Cancel, Reservation},
    error::AssetError,
};
use std::collections::BTreeSet;

const COS_30: f64 = 0.8660254037844386;
const MIN_BLOCK_Y: f64 = -64.0;
const MAX_BLOCK_Y: f64 = 319.0;
// Native 1.19.3 model parser constrains coordinates/origins to -16..32 pixels;
// <=45-degree element rotation with optional rescale maps them inside [-7,8]
// blocks, including the 90-degree blockstate application about 0.5. The extra
// block covers floating-point rounding. Every supported built-in and fluid
// recipe must be checked against this same bound before publishing a surface.
pub const LOCAL_MIN: f64 = -8.0;
pub const LOCAL_MAX: f64 = 9.0;
const MAX_ENUMERATED_CHUNKS: u64 = 65_536;
pub const MAX_REQUESTED_CHUNKS: usize = 512;
// Both BTreeSets can retain 512 positions. This includes conservative node,
// pointer and allocator slack for each position in each set.
const CHUNK_SET_CHARGE: u64 = 2 * 512 * 160 + 8192;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid saved source-projection input")]
    Invalid,
    #[error("saved source-projection chunk or work limit")]
    Limit,
    #[error(transparent)]
    Asset(#[from] AssetError),
}

/// One route's complete potential source coordinates. The set is a logical
/// account charged to the same `ByteBudget` that owns decoded chunks and mesh.
pub struct Request {
    render: BTreeSet<[i32; 2]>,
    support: BTreeSet<[i32; 2]>,
    pub enumerated_chunks: u64,
    line: Line,
    camera_height: f64,
    size: [usize; 2],
    scale: f64,
    _reservation: Reservation,
}
impl Request {
    pub fn size(&self) -> [usize; 2] {
        self.size
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn render_chunks(&self) -> &BTreeSet<[i32; 2]> {
        &self.render
    }
    pub fn support_chunks(&self) -> &BTreeSet<[i32; 2]> {
        &self.support
    }
    /// The request is valid for this exact emitted route and viewport. Resize,
    /// effective zoom or projection-height change requires a new request and
    /// a fresh qualification; a stale source cannot certify changed pixels.
    pub fn matches_view(
        &self,
        line: Line,
        camera_height: f64,
        size: [usize; 2],
        scale: f64,
    ) -> bool {
        self.size == size
            && self.camera_height.to_bits() == camera_height.to_bits()
            && self.scale.to_bits() == scale.to_bits()
            && self.line.start.map(f64::to_bits) == line.start.map(f64::to_bits)
            && self.line.end.map(f64::to_bits) == line.end.map(f64::to_bits)
    }
    /// Full saved block-Y interval that can possibly project from this Java
    /// [x,z] column into the swept viewport. None proves no bounded model,
    /// fluid or supported built-in vertex in the column can paint. This is
    /// per-column geometric rejection, never opaque-depth or air inference.
    pub fn column_band(&self, column: [i32; 2]) -> Option<[i32; 2]> {
        let mut horizontal = [f64::INFINITY, f64::NEG_INFINITY];
        let mut block_y = [f64::INFINITY, f64::NEG_INFINITY];
        for camera in [self.line.start, self.line.end] {
            for local_x in [LOCAL_MIN, LOCAL_MAX] {
                for local_z in [LOCAL_MIN, LOCAL_MAX] {
                    let x = f64::from(column[0]) + local_x - camera[0];
                    let z = f64::from(column[1]) + local_z - camera[1];
                    let sx = self.size[0] as f64 * 0.5 + (x - z) * COS_30 * self.scale;
                    horizontal[0] = horizontal[0].min(sx);
                    horizontal[1] = horizontal[1].max(sx);
                    for screen_y in [-1.0, self.size[1] as f64 + 1.0] {
                        for local_y in [LOCAL_MIN, LOCAL_MAX] {
                            let owner_y = self.camera_height + (x + z) * 0.5
                                - (screen_y - self.size[1] as f64 * 0.5) / self.scale
                                - local_y;
                            block_y[0] = block_y[0].min(owner_y);
                            block_y[1] = block_y[1].max(owner_y);
                        }
                    }
                }
            }
        }
        if horizontal[1] < -1.0 || horizontal[0] > self.size[0] as f64 + 1.0 {
            return None;
        }
        let lower = ((block_y[0] - 1.0).floor() as i32).max(MIN_BLOCK_Y as i32);
        let upper = ((block_y[1] + 1.0).ceil() as i32).min(MAX_BLOCK_Y as i32);
        (lower <= upper).then_some([lower, upper])
    }
    /// Conservative per-issued-pose check for an authentic painted owner.
    /// The sweep request admits source for every route position; presentation
    /// also requires this owner's bounded geometry to be able to intersect
    /// the CURRENT viewport. This never substitutes for actual painted pixels.
    pub fn may_project_cell(&self, position: [i32; 3], camera: [f64; 2]) -> bool {
        if camera.iter().any(|value| !value.is_finite())
            || !(MIN_BLOCK_Y as i32..=MAX_BLOCK_Y as i32).contains(&position[1])
        {
            return false;
        }
        let mut minimum = [f64::INFINITY; 2];
        let mut maximum = [f64::NEG_INFINITY; 2];
        for local_x in [LOCAL_MIN, LOCAL_MAX] {
            for local_z in [LOCAL_MIN, LOCAL_MAX] {
                for local_y in [LOCAL_MIN, LOCAL_MAX] {
                    let point = project(
                        [
                            f64::from(position[0]) + local_x,
                            f64::from(position[2]) + local_z,
                            f64::from(position[1]) + local_y,
                        ],
                        camera,
                        self.camera_height,
                        self.size,
                        self.scale,
                    );
                    for axis in 0..2 {
                        minimum[axis] = minimum[axis].min(point[axis]);
                        maximum[axis] = maximum[axis].max(point[axis]);
                    }
                }
            }
        }
        maximum[0] >= -1.0
            && minimum[0] <= self.size[0] as f64 + 1.0
            && maximum[1] >= -1.0
            && minimum[1] <= self.size[1] as f64 + 1.0
    }
}

fn project(
    world: [f64; 3],
    camera: [f64; 2],
    camera_height: f64,
    size: [usize; 2],
    scale: f64,
) -> [f64; 2] {
    let x = world[0] - camera[0];
    let z = world[1] - camera[1];
    let y = world[2] - camera_height;
    [
        size[0] as f64 * 0.5 + (x - z) * COS_30 * scale,
        size[1] as f64 * 0.5 + ((x + z) * 0.5 - y) * scale,
    ]
}

fn inverse(
    screen: [f64; 2],
    y: f64,
    camera: [f64; 2],
    camera_height: f64,
    size: [usize; 2],
    scale: f64,
) -> [f64; 2] {
    let difference = (screen[0] - size[0] as f64 * 0.5) / (COS_30 * scale) + camera[0] - camera[1];
    let sum = 2.0
        * (y + (screen[1] - size[1] as f64 * 0.5) / scale + (camera[0] + camera[1]) * 0.5
            - camera_height);
    [(sum + difference) * 0.5, (sum - difference) * 0.5]
}

fn possible_chunk(
    chunk: [i32; 2],
    line: Line,
    camera_height: f64,
    size: [usize; 2],
    scale: f64,
) -> bool {
    let x0 = f64::from(chunk[0]) * 16.0 + LOCAL_MIN;
    let z0 = f64::from(chunk[1]) * 16.0 + LOCAL_MIN;
    let x1 = f64::from(chunk[0]) * 16.0 + 15.0 + LOCAL_MAX;
    let z1 = f64::from(chunk[1]) * 16.0 + 15.0 + LOCAL_MAX;
    let mut minimum = [f64::INFINITY; 2];
    let mut maximum = [f64::NEG_INFINITY; 2];
    for camera in [line.start, line.end] {
        for x in [x0, x1] {
            for z in [z0, z1] {
                for y in [MIN_BLOCK_Y + LOCAL_MIN, MAX_BLOCK_Y + LOCAL_MAX] {
                    let point = project([x, z, y], camera, camera_height, size, scale);
                    for axis in 0..2 {
                        minimum[axis] = minimum[axis].min(point[axis]);
                        maximum[axis] = maximum[axis].max(point[axis]);
                    }
                }
            }
        }
    }
    maximum[0] >= -1.0
        && minimum[0] <= size[0] as f64 + 1.0
        && maximum[1] >= -1.0
        && minimum[1] <= size[1] as f64 + 1.0
}

/// Project the full -64..319 saved block domain and a proven local-vertex
/// bound. Both camera endpoints suffice because projection is affine in camera
/// X/Z: their bounding union contains every intermediate continuous pose.
/// The inverse corner box is only an iteration bound; each enumerated chunk
/// gets a second conservative projected-prism intersection test.
pub fn request(
    line: Line,
    camera_height: f64,
    size: [usize; 2],
    scale: f64,
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<Request, Error> {
    cancel.check()?;
    let length = line.length();
    if line
        .start
        .iter()
        .chain(line.end.iter())
        .any(|v| !v.is_finite())
        || !length.is_finite()
        || length > 1024.0
        || !camera_height.is_finite()
        || !(-4096.0..=4096.0).contains(&camera_height)
        || !scale.is_finite()
        || !(0.01..=1024.0).contains(&scale)
        || size.iter().any(|side| !(1..=65_536).contains(side))
        || size[0]
            .checked_mul(size[1])
            .is_none_or(|dots| dots > 1_048_576)
    {
        return Err(Error::Invalid);
    }
    let reservation = budget.reserve(CHUNK_SET_CHARGE, cancel)?;
    let mut minimum = [f64::INFINITY; 2];
    let mut maximum = [f64::NEG_INFINITY; 2];
    for camera in [line.start, line.end] {
        for screen_x in [-1.0, size[0] as f64 + 1.0] {
            for screen_y in [-1.0, size[1] as f64 + 1.0] {
                for y in [MIN_BLOCK_Y + LOCAL_MIN, MAX_BLOCK_Y + LOCAL_MAX] {
                    let ground =
                        inverse([screen_x, screen_y], y, camera, camera_height, size, scale);
                    for axis in 0..2 {
                        minimum[axis] = minimum[axis].min(ground[axis]);
                        maximum[axis] = maximum[axis].max(ground[axis]);
                    }
                }
            }
        }
    }
    // Invert the owner-local offset and add one full block for rounding and
    // inclusive raster-edge overlap. Reject float-to-int saturation explicitly.
    let lower = [0, 1].map(|axis| ((minimum[axis] - LOCAL_MAX - 1.0) / 16.0).floor());
    let upper = [0, 1].map(|axis| ((maximum[axis] - LOCAL_MIN + 1.0) / 16.0).ceil());
    if lower
        .into_iter()
        .chain(upper)
        .any(|v| !v.is_finite() || v < f64::from(i32::MIN + 1) || v > f64::from(i32::MAX - 1))
    {
        return Err(Error::Invalid);
    }
    let lower = lower.map(|v| v as i32);
    let upper = upper.map(|v| v as i32);
    let widths = [0, 1].map(|axis| i64::from(upper[axis]) - i64::from(lower[axis]) + 1);
    if widths.iter().any(|width| *width <= 0)
        || widths[0]
            .checked_mul(widths[1])
            .is_none_or(|count| count as u64 > MAX_ENUMERATED_CHUNKS)
    {
        return Err(Error::Limit);
    }
    let mut render = BTreeSet::new();
    let mut support = BTreeSet::new();
    let mut examined = 0_u64;
    for z in lower[1]..=upper[1] {
        for x in lower[0]..=upper[0] {
            cancel.check()?;
            examined += 1;
            let position = [x, z];
            if !possible_chunk(position, line, camera_height, size, scale) {
                continue;
            }
            if render.len() >= MAX_REQUESTED_CHUNKS || !render.insert(position) {
                return Err(Error::Limit);
            }
            // A full chunk ring covers native blend-radius-two lookup (at most
            // seven blocks) and direct/directional liquid/shape neighbor reads.
            for dz in -1_i32..=1 {
                for dx in -1_i32..=1 {
                    let neighbor = [
                        x.checked_add(dx).ok_or(Error::Invalid)?,
                        z.checked_add(dz).ok_or(Error::Invalid)?,
                    ];
                    support.insert(neighbor);
                    if support.len() > MAX_REQUESTED_CHUNKS {
                        return Err(Error::Limit);
                    }
                }
            }
        }
    }
    if render.is_empty() {
        return Err(Error::Invalid);
    }
    cancel.check()?;
    Ok(Request {
        render,
        support,
        enumerated_chunks: examined,
        line,
        camera_height,
        size,
        scale,
        _reservation: reservation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn distant_high_face_enters_source_request_before_binding() {
        let budget = ByteBudget::new(1 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let request = request(
            Line {
                start: [0.0, 0.0],
                end: [64.0, 0.0],
            },
            70.0,
            [160, 96],
            4.2,
            &budget,
            Cancel::new(&stop),
        )
        .unwrap();
        assert!(request.render.contains(&[8, 8]));
        assert!(request.support.contains(&[0, 0]));
        assert!(request.render.len() <= MAX_REQUESTED_CHUNKS);
    }

    #[test]
    fn finite_brute_witnesses_are_never_outside_requested_source() {
        let budget = ByteBudget::new(1 << 20).unwrap();
        let stop = AtomicBool::new(false);
        let line = Line {
            start: [-28.0, 12.0],
            end: [53.0, 31.0],
        };
        let camera_height = 72.0;
        let size = [160, 96];
        let scale = 4.2;
        let request = request(
            line,
            camera_height,
            size,
            scale,
            &budget,
            Cancel::new(&stop),
        )
        .unwrap();
        for camera_fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for x in (-224_i32..=224).step_by(7) {
                for z in (-224_i32..=224).step_by(7) {
                    for y in [-64, 0, 70, 140, 220, 319] {
                        for local in [
                            [LOCAL_MIN, LOCAL_MAX, LOCAL_MIN],
                            [LOCAL_MAX, LOCAL_MIN, LOCAL_MAX],
                            [0.0; 3],
                        ] {
                            let screen = project(
                                [
                                    f64::from(x) + local[0],
                                    f64::from(z) + local[1],
                                    f64::from(y) + local[2],
                                ],
                                line.point(camera_fraction),
                                camera_height,
                                size,
                                scale,
                            );
                            if (0.0..size[0] as f64).contains(&screen[0])
                                && (0.0..size[1] as f64).contains(&screen[1])
                            {
                                assert!(request.render.contains(&[x.div_euclid(16), z.div_euclid(16)]),
                                    "{x},{y},{z} at {camera_fraction} projects inside but is missing");
                                let band = request.column_band([x, z]).unwrap();
                                assert!(band[0] <= y && y <= band[1]);
                            }
                        }
                    }
                }
            }
        }
    }
}
