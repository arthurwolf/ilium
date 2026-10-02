//! Orthographic hatch geometry only. The host retains palette/dither ownership.
//! The sampled maximum is bilinearly reconstructed; no opaque surface or
//! hidden-line removal is claimed. Keep one Renderer in the owning Scene.
use super::model::{finite, Body, Prepared, MAX_BODIES};
use crate::Raster;
#[path = "envelope_tree.rs"]
// Keep the new implementation leaf independent of parent scene registration.
mod envelope_tree; // Own the exact maximum-query hierarchy inside this renderer.
use envelope_tree::EnvelopeTreeIndex; // Share no geometry state with simulations.

const GRID: usize = 256;
const SIDE: usize = GRID + 1;
const TILE: usize = 8;
const TILES: usize = SIDE.div_ceil(TILE);
pub const MAX_DOTS: usize = 1_048_576;
pub const MAX_DIMENSION: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq)] // Compare normalized presentation values directly for raster reuse.
pub struct RenderOptions {
    /// Angles in degrees; pitch is elevation above the ground, not from zenith.
    pub yaw: f32,
    pub pitch: f32,
    /// 1 fits the whole ground plus fixed height headroom; >1 intentionally crops.
    pub zoom: f32,
    pub hatch_direction: f32,
    /// Perpendicular distance between projected, undeformed lines, in dots.
    pub spacing_in_dots: f32,
    /// Stroke diameter in dots, not Raster::line's radius. Zero disables ink.
    pub line_width: f32,
    pub height_scale: f32,
    pub softness: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            yaw: 45.0,
            pitch: 35.26439,
            zoom: 1.0,
            hatch_direction: 0.0,
            spacing_in_dots: 4.0,
            line_width: 1.0,
            height_scale: 0.25,
            softness: 0.5,
        }
    }
}

impl RenderOptions {
    pub fn normalized(&self) -> Self {
        let d = Self::default();
        Self {
            yaw: finite(self.yaw, d.yaw).rem_euclid(360.0),
            pitch: finite(self.pitch, d.pitch).clamp(5.0, 85.0),
            zoom: finite(self.zoom, d.zoom).clamp(0.25, 4.0),
            hatch_direction: finite(self.hatch_direction, d.hatch_direction).rem_euclid(180.0),
            spacing_in_dots: finite(self.spacing_in_dots, d.spacing_in_dots).clamp(2.0, 24.0),
            line_width: finite(self.line_width, d.line_width).clamp(0.0, 3.0),
            height_scale: finite(self.height_scale, d.height_scale).clamp(0.0, 2.5),
            softness: finite(self.softness, d.softness).clamp(0.0, 1.0),
        }
    }
}

/// Pure, body-independent camera. Coordinates returned/accepted are normalized
/// to the ENTIRE raster, with y downward; callers subtract the screen origin.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    size: [f32; 2],
    right: [f32; 2],
    down: [f32; 2],
    origin: [f32; 2],
    scale: f32,
    lift: f32,
    sine_pitch: f32,
}

impl Camera {
    pub fn new(width: usize, height: usize, options: &RenderOptions) -> Option<Self> {
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || width.checked_mul(height)? > MAX_DOTS
        {
            return None;
        }
        let o = options.normalized();
        let (s, c) = o.yaw.to_radians().sin_cos();
        let (sp, cp) = o.pitch.to_radians().sin_cos();
        let span = s.abs() + c.abs();
        let scale = 0.94
            * (width as f32 / span).min(height as f32 / (sp * span + o.height_scale * cp))
            * o.zoom;
        let lift = scale * cp * o.height_scale;
        Some(Self {
            size: [width as f32, height as f32],
            right: [c, -s],
            down: [sp * s, sp * c],
            origin: [width as f32 * 0.5, height as f32 * 0.5 + lift * 0.5],
            scale,
            lift,
            sine_pitch: sp,
        })
    }

    fn vector(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.scale * dot(self.right, p),
            self.scale * dot(self.down, p),
        ]
    }

    fn precise_dots(&self, p: [f32; 2], height: f32) -> [f64; 2] {
        let v = [f64::from(p[0]) - 0.5, f64::from(p[1]) - 0.5];
        [
            f64::from(self.origin[0])
                + f64::from(self.scale)
                    * (f64::from(self.right[0]) * v[0] + f64::from(self.right[1]) * v[1]),
            f64::from(self.origin[1])
                + f64::from(self.scale)
                    * (f64::from(self.down[0]) * v[0] + f64::from(self.down[1]) * v[1])
                - f64::from(self.lift) * f64::from(height),
        ]
    }

    fn dots(&self, p: [f32; 2], height: f32) -> [f32; 2] {
        self.precise_dots(p, height).map(|v| v as f32)
    }

    /// f64 screen coordinates avoid cancellation for very narrow viewports.
    #[cfg(test)]
    pub fn project(&self, ground: [f32; 2], height: f32) -> Option<[f64; 2]> {
        if !ground
            .into_iter()
            .chain([height])
            .all(|x| x.is_finite() && (0.0..=1.0).contains(&x))
        {
            return None;
        }
        let p = self.precise_dots(ground, height);
        Some([
            p[0] / f64::from(self.size[0]),
            p[1] / f64::from(self.size[1]),
        ])
    }

    /// Ground-plane hit, NOT an intersection with the raised surface. Returns
    /// None outside either the visible viewport or the projected ground square.
    pub fn inverse_ground(&self, screen: [f64; 2]) -> Option<[f32; 2]> {
        if !screen
            .into_iter()
            .all(|x| x.is_finite() && (0.0..=1.0).contains(&x))
        {
            return None;
        }
        let x = (screen[0] * f64::from(self.size[0]) - f64::from(self.origin[0]))
            / f64::from(self.scale);
        let y = (screen[1] * f64::from(self.size[1]) - f64::from(self.origin[1]))
            / f64::from(self.scale);
        let r = self.right.map(f64::from);
        let d = self.down.map(f64::from);
        let det = r[0] * d[1] - r[1] * d[0];
        let p = [
            0.5 + (d[1] * x - r[1] * y) / det,
            0.5 + (r[0] * y - d[0] * x) / det,
        ];
        p.into_iter()
            .all(|v| (-1e-9..=1.000000001).contains(&v))
            .then(|| p.map(|v| v.clamp(0.0, 1.0) as f32))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RenderStats {
    pub rejected: bool,
    pub invalid_bodies: usize,
    pub unique_bodies: usize,
    pub rebuilt: bool,
    pub tile_tests: usize, // Dirty lattice tiles visited; no longer old body/tile pair tests.
    pub node_tests: usize, // Conservative hierarchy bounds evaluated during general repairs.
    pub monotone_update: bool, // Only additions or nondecreasing heights extended the old maximum.
    pub field_changed: bool, // At least one repaired lattice vertex actually changed.
    pub frame_cache_hit: bool, // Completed dot intensities were copied without drawing strokes.
    pub copied_dots: usize, // Number of cached dot values restored into the caller's raster.
    pub body_evaluations: usize,
    pub hatch_lines: usize,
    pub samples: usize,
    pub segments: usize,
    /// Sum of actual Raster::line bounding rectangle areas, not ink coverage.
    pub raster_visits_bound: u64,
    /// Analytic spatial sampling bound in dots, NOT a measured visual error.
    pub height_error_bound_dots: f32,
}

#[derive(Debug, Default)]
pub struct Renderer {
    bodies: Vec<Prepared>,
    next: Vec<Prepared>,
    heights: Vec<f32>,
    scratch: Vec<f32>, // Reusable exact reconstruction target for low-support-work populations.
    dirty: Vec<bool>,  // Fixed lattice-tile repair mask.
    changed: Vec<usize>, // Every added or height-changed canonical body index.
    raw_input: Vec<Body>, // Exact previous input for allocation-free unchanged-frame checks.
    invalid_bodies: usize, // Preserve invalid-record counts on the raw-input fast path.
    tree: EnvelopeTreeIndex, // Reusable complete maximum-query hierarchy.
    cached_dots: Vec<f32>, // One completed raster, independent of caller buffer ownership.
    cached_key: Option<(usize, usize, RenderOptions)>, // Exact normalized dimensions and presentation settings.
    softness: Option<f32>,
    stats: RenderStats,
}

impl Renderer {
    #[cfg(test)]
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    #[cfg(test)] // Preserve the existing private preparation seam used by recovered tests.
    fn prepare(&mut self, input: &[Body], softness: f32) {
        let _ = self.prepare_checked(input, softness, None);
    } // Ordinary tests use the same implementation without cancellation.
    fn prepare_checked(
        &mut self,
        input: &[Body],
        softness: f32,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> bool {
        // Maintain the same lattice maximum without rebuilding unrelated regions.
        self.stats.invalid_bodies = self.invalid_bodies; // Restore population statistics on raw-input cache hits.
        self.stats.unique_bodies = self.bodies.len(); // Canonical contributors are retained independently of caller ordering.
        if self.softness == Some(softness)
            && self.raw_input.len() == input.len()
            && self
                .raw_input
                .iter()
                .zip(input)
                .all(|(a, b)| body_bits(a) == body_bits(b))
        {
            return true;
        } // Compare bits so repeated invalid NaNs do not defeat reuse.
        if stopping(stop) {
            return false;
        } // Cancellation never publishes a partially repaired field.
        self.next.clear(); // Reuse canonicalization storage.
        self.next
            .extend(input.iter().filter_map(|body| Prepared::new(*body))); // Normalize every supplied record under the unchanged model contract.
        self.invalid_bodies = input.len() - self.next.len(); // Invalid records remain explicitly counted.
        self.stats.invalid_bodies = self.invalid_bodies; // Report the current request, not the previous request.
        self.next
            .sort_unstable_by(|a, b| geometry_order(a, b).then(b.height.total_cmp(&a.height))); // Equal geometry is ordered tallest first.
        self.next.dedup_by(|a, b| a.key == b.key); // Only identical center segments and radii may share their maximum-height representative.
        self.stats.unique_bodies = self.next.len(); // No spatial shortlist or population truncation is used.
        self.raw_input.clear(); // Refresh the exact request identity only after normalization.
        self.raw_input.extend_from_slice(input); // Keep at most the accepted 4096 records.
        if self.softness == Some(softness) && self.next == self.bodies {
            return true;
        } // Permutations and changes to dominated exact duplicates preserve the field.
        self.dirty.resize(TILES * TILES, false); // Allocate a fixed lattice-tile mask once.
        self.dirty.fill(false); // Dirty ownership is frame-local, not cumulative.
        self.changed.clear(); // Retain capacity for changed or added canonical indices.
        let mut monotone = self.softness == Some(softness) && self.heights.len() == SIDE * SIDE; // Only an established field with unchanged softness can be extended by maximum.
        if !monotone {
            self.dirty.fill(true);
        } // First construction and softness changes require all lattice vertices to be reconsidered.
        let (mut old_index, mut new_index) = (0, 0); // Merge the two geometry-sorted populations.
        while old_index < self.bodies.len() || new_index < self.next.len() {
            // Each source record participates in at most one merge step.
            let comparison = match (self.bodies.get(old_index), self.next.get(new_index)) {
                // Exhaustion is represented as insertion or removal.
                (Some(old), Some(new)) => geometry_order(old, new), // Heights are deliberately excluded from geometric identity.
                (Some(_), None) => std::cmp::Ordering::Less, // Remaining old records were removed.
                (None, Some(_)) => std::cmp::Ordering::Greater, // Remaining new records were added.
                (None, None) => break,                       // Both populations are exhausted.
            }; // Finish this merge comparison.
            if comparison.is_lt() {
                mark_tiles(self.bodies[old_index], &mut self.dirty);
                monotone = false;
                old_index += 1;
                continue;
            } // Removing a body may expose a previously hidden contributor.
            if comparison.is_gt() {
                mark_tiles(self.next[new_index], &mut self.dirty);
                self.changed.push(new_index);
                new_index += 1;
                continue;
            } // Additions cannot invalidate an established maximum.
            let old_height = self.bodies[old_index].height; // Equal geometry can still change its peak height.
            let new_body = self.next[new_index]; // Read the new representative once.
            if old_height != new_body.height {
                // Unchanged contributors need no dirty region.
                mark_tiles(new_body, &mut self.dirty); // Equal geometry has identical old and new support.
                self.changed.push(new_index); // Increasing heights can be stamped directly.
                monotone &= new_body.height >= old_height; // Decreases must recover the maximum of all remaining contributors.
            } // Finish the equal-geometry change check.
            old_index += 1; // Advance the old canonical population.
            new_index += 1; // Advance the new canonical population.
        } // Finish the bounded population merge.
        std::mem::swap(&mut self.next, &mut self.bodies); // The current field operation owns the complete new population.
        self.heights.resize(SIDE * SIDE, 0.0); // Preserve established lattice values outside dirty regions.
        self.stats.rebuilt = true; // This request performed field maintenance, including incremental updates.
        self.stats.monotone_update = monotone
            && self
                .changed
                .iter()
                .map(|index| support_work(self.bodies[*index]))
                .sum::<usize>()
                <= 1_000_000; // Large monotone populations may use the exact general path instead of excessive stamping.
        self.stats.tile_tests = self.dirty.iter().filter(|dirty| **dirty).count(); // This field now counts dirty lattice tiles, not old body/tile pairs.
        if self.stats.monotone_update {
            // No prior maximum can become invalid on this bounded-support path.
            for &body_index in &self.changed {
                // Stamp every changed or newly added contributor.
                let body = self.bodies[body_index]; // Keep the original Prepared arithmetic.
                let (low, high) = lattice_bounds(body); // Restrict work to outward-rounded compact support.
                for y in low[1]..=high[1] {
                    // Include support-boundary vertices conservatively.
                    if stopping(stop) {
                        return false;
                    } // Bound cancellation work by one support or tile row.
                    for x in low[0]..=high[0] {
                        // No scan of unrelated bodies is required here.
                        let value = &mut self.heights[y * SIDE + x]; // Update the existing maximum in place.
                        if body.height + 0.000004 <= *value {
                            continue;
                        } // Skip only a conservatively dominated peak.
                        self.stats.body_evaluations += 1; // Count the actual analytic evaluation.
                        let updated = value.max(sample_body(
                            body,
                            [x as f32 / GRID as f32, y as f32 / GRID as f32],
                            softness,
                        )); // Increasing a positive height is monotone under the unchanged arithmetic.
                        self.stats.field_changed |= updated != *value; // Shadowed contributors need not invalidate the completed raster.
                        *value = updated; // Publish the exact updated vertex maximum.
                    } // Finish this support row.
                } // Finish this changed body.
            } // Finish all maximum extensions.
        } else if self
            .bodies
            .iter()
            .map(|body| support_work(*body))
            .sum::<usize>()
            <= 1_000_000
        {
            // Small total support is cheaper to stamp than to query through a hierarchy.
            self.scratch.resize(SIDE * SIDE, 0.0); // Retain one bounded reconstruction buffer.
            self.scratch.fill(0.0); // Reconstruct from ground, never from invalidated winners.
            for &body in &self.bodies {
                // Include every contributor, including previously hidden bodies.
                let (low, high) = lattice_bounds(body); // Restrict exact evaluations to compact support.
                for y in low[1]..=high[1] {
                    // Keep row-major writes local.
                    if stopping(stop) {
                        return false;
                    } // Bound cancellation work by one support or tile row.
                    for x in low[0]..=high[0] {
                        // Each support rectangle has a known finite area.
                        if !self.dirty[(y / TILE) * TILES + x / TILE] {
                            continue;
                        } // Unaffected tiles need no reconstruction.
                        let value = &mut self.scratch[y * SIDE + x]; // Accumulate the complete replacement maximum.
                        if body.height + 0.000004 <= *value {
                            continue;
                        } // Skip only a conservatively dominated peak.
                        self.stats.body_evaluations += 1; // Record actual analytic work for release comparisons.
                        *value = value.max(sample_body(
                            body,
                            [x as f32 / GRID as f32, y as f32 / GRID as f32],
                            softness,
                        )); // Spheres avoid unnecessary segment projection without changing their arithmetic result.
                    } // Finish this support row.
                } // Finish this body.
            } // Finish the complete replacement envelope.
            for y in 0..SIDE {
                // Commit only reconstructed tiles.
                if stopping(stop) {
                    return false;
                } // Bound cancellation work by one support or tile row.
                for x in 0..SIDE {
                    // Compare final maxima rather than temporary clearing operations.
                    if !self.dirty[(y / TILE) * TILES + x / TILE] {
                        continue;
                    } // Preserve unrelated vertices byte-for-byte.
                    let index = y * SIDE + x; // Use identical row-major indexing in both buffers.
                    self.stats.field_changed |= self.heights[index] != self.scratch[index]; // A shadowed motion can still reuse completed ink.
                    self.heights[index] = self.scratch[index]; // Publish the exact recovered maximum.
                } // Finish this commit row.
            } // Finish low-support-work reconstruction.
        } else {
            // Larger support populations use conservative hierarchy queries.
            if !self.tree.build(&self.bodies, stop) {
                return false;
            } // Cancel between bounded hierarchy-construction nodes.
            let mut hint = usize::MAX; // A previous query's winner accelerates traversal but never limits admission.
            for tile in 0..TILES * TILES {
                // Process only dirty tiles after a general population change.
                if !self.dirty[tile] {
                    continue;
                } // Unaffected lattice vertices remain byte-for-byte unchanged.
                let (x0, y0) = ((tile % TILES) * TILE, (tile / TILES) * TILE); // Recover this tile's first lattice vertex.
                for y in y0..(y0 + TILE).min(SIDE) {
                    // Boundary tiles may contain fewer than eight vertices.
                    if stopping(stop) {
                        return false;
                    } // Bound cancellation work by one support or tile row.
                    for x in x0..(x0 + TILE).min(SIDE) {
                        // Tile ranges partition the lattice without overlap.
                        let updated = self.tree.sample(
                            &self.bodies,
                            [x as f32 / GRID as f32, y as f32 / GRID as f32],
                            softness,
                            &mut hint,
                            &mut self.stats.body_evaluations,
                            &mut self.stats.node_tests,
                        ); // Query all potentially winning bodies through conservative bounds.
                        let value = &mut self.heights[y * SIDE + x]; // Read the old value only after the independent query.
                        self.stats.field_changed |= updated != *value; // Reuse final ink only when the entire affected lattice is unchanged.
                        *value = updated; // A removed winner cannot leave ghost height behind.
                    } // Finish this tile row.
                } // Finish this tile.
            } // Finish all required repairs.
        } // Finish field maintenance.
        self.softness = Some(softness); // Commit the interpolation field's shape parameter.
        if self.stats.field_changed {
            self.cached_key = None;
        } // Camera-independent field changes invalidate completed raster reuse.
        true // The complete affected lattice is now coherent.
    } // End incremental field preparation.
      // Section boundary.

    fn height(&self, p: [f32; 2]) -> f32 {
        let x = p[0].clamp(0.0, 1.0) * GRID as f32;
        let y = p[1].clamp(0.0, 1.0) * GRID as f32;
        let ix = (x as usize).min(GRID - 1);
        let iy = (y as usize).min(GRID - 1);
        let a = iy * SIDE + ix;
        lerp(
            lerp(self.heights[a], self.heights[a + 1], x - ix as f32),
            lerp(
                self.heights[a + SIDE],
                self.heights[a + SIDE + 1],
                x - ix as f32,
            ),
            y - iy as f32,
        )
    }

    /// Overwrites a correctly sized dot raster. Oversize/malformed rasters or
    /// >4096 records are rejected without resizing or truncating input bodies.
    pub fn render(&mut self, raster: &mut Raster, bodies: &[Body], options: &RenderOptions) {
        let _ = self.render_checked(raster, bodies, options, None);
    } // Preserve the fixed synchronous rendering interface.
    pub(super) fn render_checked(
        &mut self,
        raster: &mut Raster,
        bodies: &[Body],
        options: &RenderOptions,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> bool {
        // Owned workers may discard an interrupted frame safely.
        self.stats = RenderStats::default();
        if stopping(stop) {
            return self.cancel_render();
        } // Do not start an already cancelled job.
        let Some(area) = raster.width.checked_mul(raster.height) else {
            self.stats.rejected = true;
            return true; // Preserve the original guarded no-op or rejection result.
        };
        if area != raster.dots.len()
            || area > MAX_DOTS
            || raster.width > MAX_DIMENSION
            || raster.height > MAX_DIMENSION
        {
            self.stats.rejected = true;
            return true; // Preserve the original guarded no-op or rejection result.
        }
        raster.dots.fill(0.0);
        if bodies.len() > MAX_BODIES {
            self.stats.rejected = true;
            return true; // Preserve the original guarded no-op or rejection result.
        }
        let o = options.normalized();
        let Some(camera) = Camera::new(raster.width, raster.height, &o) else {
            return true; // Preserve the original guarded no-op or rejection result.
        };
        if o.line_width == 0.0 {
            return true; // Preserve the original guarded no-op or rejection result.
        } // Keep zero-width rendering disabled without touching retained geometry.
        if !self.prepare_checked(bodies, o.softness, stop) {
            return self.cancel_render();
        } // Partial field repairs are never reusable or publishable.
        let lipschitz = self
            .bodies
            .iter()
            .map(|b| 2.0 * b.height / b.key[4])
            .fold(0.0, f32::max);
        // Bilinear reconstruction <= L*sqrt(2)/(2G); chord interpolation
        // on steps <=1/(2G) adds <= L*sqrt(2)/(4G).
        self.stats.height_error_bound_dots =
            camera.lift * lipschitz * 3.0 * 2.0_f32.sqrt() / (4 * GRID) as f32;
        let cache_key = (raster.width, raster.height, o); // All controls affecting the rendered dots participate in reuse.
        if self.cached_key == Some(cache_key) && self.cached_dots.len() == area {
            // The field invalidates this key whenever any lattice vertex changes.
            raster.dots.copy_from_slice(&self.cached_dots); // Restore even a cleared or independently overwritten caller buffer.
            self.stats.frame_cache_hit = true; // Do not disguise a copy as newly evaluated geometry.
            self.stats.copied_dots = area; // Account for the actual copy work.
            return true; // No projections, height samples, or Raster::line calls are repeated.
        } // A cache miss keeps the existing full-fidelity stroke pipeline.
        let (s, c) = o.hatch_direction.to_radians().sin_cos();
        let direction = [c, s];
        let normal = [-s, c];
        let projected = camera.vector(direction);
        let separation =
            camera.scale * camera.scale * camera.sine_pitch / projected[0].hypot(projected[1]);
        let spacing = o.spacing_in_dots / separation;
        let extent = (normal[0].abs() + normal[1].abs()) * 0.5;
        let first = (-extent / spacing).ceil() as i32;
        let last = (extent / spacing).floor() as i32;
        for k in first..=last {
            let base = [
                0.5 + normal[0] * k as f32 * spacing,
                0.5 + normal[1] * k as f32 * spacing,
            ];
            let Some((start, end)) = interval(base, direction, &camera, o.line_width * 0.5 + 0.6)
            else {
                continue;
            };
            self.stats.hatch_lines += 1;
            let steps = ((end - start) * (2 * GRID) as f32).ceil().max(1.0) as usize;
            let point = |t: f32| [base[0] + t * direction[0], base[1] + t * direction[1]];
            let p = point(start);
            let mut previous = camera.dots(p, self.height(p));
            self.stats.samples += 1;
            for step in 1..=steps {
                if step % 32 == 0 && stopping(stop) {
                    return self.cancel_render();
                } // Bound cancellation latency during long or steep polylines.
                let p = point(lerp(start, end, step as f32 / steps as f32));
                let next = camera.dots(p, self.height(p));
                self.stats.samples += 1;
                stroke(raster, previous, next, o.line_width * 0.5, &mut self.stats);
                previous = next;
            }
        }
        if stopping(stop) {
            return self.cancel_render();
        } // Do not publish a frame cancelled during its final hatch.
        self.cached_dots.resize(area, 0.0); // Retain at most one accepted raster allocation.
        self.cached_dots.copy_from_slice(&raster.dots); // Cache completed intensities, not externally mutable raster storage.
        self.cached_key = Some(cache_key); // Publish the key only after the complete raster has been drawn.
        true // A completed frame may now be published by its owner.
    } // End complete-frame rendering.
    fn cancel_render(&mut self) -> bool {
        // Roll back cache validity, not the caller's already private scratch raster.
        self.softness = None; // The next request must rebuild a coherent complete field.
        self.raw_input.clear(); // A partial preparation cannot pass the raw-input fast path.
        self.cached_key = None; // Partial ink must never be copied as a completed frame.
        false // The owned worker must discard this result.
    } // End cancellation invalidation.
}

/// Allocation-owning convenience path. A live scene should retain Renderer.
#[cfg(test)]
pub fn render(raster: &mut Raster, bodies: &[Body], options: &RenderOptions) {
    Renderer::default().render(raster, bodies, options);
}
// Section boundary.
fn stopping(stop: Option<&std::sync::atomic::AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
} // Poll cancellation without locks, clocks, or allocation.
fn sample_body(body: Prepared, point: [f32; 2], softness: f32) -> f32 {
    // Specialize only truly identical endpoints.
    if body.key[0] != body.key[2] || body.key[1] != body.key[3] {
        return body.sample(point, softness);
    } // Capsules retain the exact existing segment projection.
    let dx = point[0] - body.key[0]; // A zero-length segment projects with t equal to zero.
    let dy = point[1] - body.key[1]; // Eliminate multiplication by its zero axis.
    body.envelope(dx * dx + dy * dy, softness) // Preserve the original squared-distance and envelope operation order.
} // End exact sphere specialization.
fn body_bits(body: &Body) -> [u32; 6] {
    // Compare all fixed Body fields without requiring a public PartialEq implementation.
    [
        body.from[0].to_bits(),
        body.from[1].to_bits(),
        body.to[0].to_bits(),
        body.to[1].to_bits(),
        body.radius.to_bits(),
        body.height.to_bits(),
    ] // Preserve NaN and signed-zero request identities.
} // End raw request identity.
fn lattice_bounds(body: Prepared) -> ([usize; 2], [usize; 2]) {
    // Return an inclusive, conservative vertex rectangle.
    let low = std::array::from_fn(|i| {
        ((body.key[i].min(body.key[i + 2]) - body.key[4] - 0.000004).max(0.0) * GRID as f32).floor()
            as usize
    }); // Include old support when removing or moving a body.
    let high = std::array::from_fn(|i| {
        (((body.key[i].max(body.key[i + 2]) + body.key[4] + 0.000004).min(1.0) * GRID as f32).ceil()
            as usize)
            .min(GRID)
    }); // Include newly reached boundary vertices.
    (low, high) // Normalized endpoints keep both ranges inside the fixed lattice.
} // End compact-support bounds.
fn support_work(body: Prepared) -> usize {
    let (low, high) = lattice_bounds(body);
    (high[0] - low[0] + 1) * (high[1] - low[1] + 1)
} // Strategy selection counts complete support rectangles; it never truncates work or bodies.
fn mark_tiles(body: Prepared, dirty: &mut [bool]) {
    // Accumulate the union of old and new influence, including removals.
    let (low, high) = lattice_bounds(body); // Use the same conservative support as direct stamping.
    for y in low[1] / TILE..=high[1] / TILE {
        // Cover every intersecting tile row.
        for x in low[0] / TILE..=high[0] / TILE {
            dirty[y * TILES + x] = true;
        } // Mark without per-body or per-tile allocation.
    } // Finish this body's influence union.
} // End dirty-tile admission.

fn geometry_order(a: &Prepared, b: &Prepared) -> std::cmp::Ordering {
    for i in 0..5 {
        let order = a.key[i].total_cmp(&b.key[i]);
        if !order.is_eq() {
            return order;
        }
    }
    std::cmp::Ordering::Equal
}
fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn slab(origin: f32, direction: f32, low: f32, high: f32, span: &mut (f32, f32)) -> bool {
    if direction.abs() < 1e-8 {
        return origin >= low && origin <= high;
    }
    let a = (low - origin) / direction;
    let b = (high - origin) / direction;
    span.0 = span.0.max(a.min(b));
    span.1 = span.1.min(a.max(b));
    span.0 <= span.1
}

fn interval(
    base: [f32; 2],
    direction: [f32; 2],
    camera: &Camera,
    margin: f32,
) -> Option<(f32, f32)> {
    let mut span = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..2 {
        if !slab(base[i], direction[i], 0.0, 1.0, &mut span) {
            return None;
        }
    }
    let p = camera.dots(base, 0.0);
    let d = camera.vector(direction);
    // Height moves y upward only, by at most camera.lift. No possible visible
    // portion is removed by this clipping of undeformed ground coordinates.
    if !slab(p[0], d[0], -margin, camera.size[0] + margin, &mut span)
        || !slab(
            p[1],
            d[1],
            -margin,
            camera.size[1] + camera.lift + margin,
            &mut span,
        )
    {
        return None;
    }
    Some(span)
}

fn stroke(raster: &mut Raster, a: [f32; 2], b: [f32; 2], radius: f32, stats: &mut RenderStats) {
    let mut span = (0.0, 1.0);
    let outer = radius + 0.6;
    let size = [raster.width as f32, raster.height as f32];
    for i in 0..2 {
        if !slab(a[i], b[i] - a[i], -outer, size[i] + outer, &mut span) {
            return;
        }
    }
    let from: [f32; 2] = std::array::from_fn(|i| lerp(a[i], b[i], span.0));
    let to: [f32; 2] = std::array::from_fn(|i| lerp(a[i], b[i], span.1));
    // Short dot-space chunks keep Raster::line's bounding-box scan bounded,
    // even at steep height transitions; no per-dot/segment allocations.
    let pieces = ((to[0] - from[0]).abs().max((to[1] - from[1]).abs()) / 4.0)
        .ceil()
        .max(1.0) as usize;
    let mut previous = from;
    for piece in 1..=pieces {
        let next: [f32; 2] =
            std::array::from_fn(|i| lerp(from[i], to[i], piece as f32 / pieces as f32));
        let u = (previous[0] / size[0], previous[1] / size[1]);
        let v = (next[0] / size[0], next[1] / size[1]);
        // Match Raster::line's normalized-coordinate round trip and bounds.
        let ax = u.0 * size[0];
        let ay = u.1 * size[1];
        let bx = v.0 * size[0];
        let by = v.1 * size[1];
        let left = (ax.min(bx) - outer).max(0.0) as usize;
        let top = (ay.min(by) - outer).max(0.0) as usize;
        let right = ((ax.max(bx) + outer).ceil().max(0.0) as usize).min(raster.width);
        let bottom = ((ay.max(by) + outer).ceil().max(0.0) as usize).min(raster.height);
        stats.raster_visits_bound +=
            (right.saturating_sub(left) * bottom.saturating_sub(top)) as u64;
        stats.segments += 1;
        raster.line(u, v, radius, 1.0);
        previous = next;
    }
}

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod tests;

#[cfg(test)] // Keep C1 regressions separate from the recovered original tests.
#[path = "performance_tests.rs"]
// Load the complete incremental, cache, cancellation, and work-count controls.
mod performance_tests; // Parent scene registration is not required to run these tests.
