//! Orthographic hatch geometry only. The host retains palette/dither ownership.
//! The sampled maximum is bilinearly reconstructed; no opaque surface or
//! hidden-line removal is claimed. Keep one Renderer in the owning Scene.
use super::model::{finite, Body, Prepared, MAX_BODIES};
use crate::Raster;

const GRID: usize = 256;
const SIDE: usize = GRID + 1;
const TILE: usize = 8;
const TILES: usize = SIDE.div_ceil(TILE);
pub const MAX_DOTS: usize = 1_048_576;
pub const MAX_DIMENSION: usize = 2048;

#[derive(Debug, Clone, Copy)]
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
    pub tile_tests: usize,
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
    minima: Vec<f32>,
    softness: Option<f32>,
    stats: RenderStats,
}

impl Renderer {
    #[cfg(test)]
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    fn prepare(&mut self, input: &[Body], softness: f32) {
        self.next.clear();
        self.next
            .extend(input.iter().filter_map(|b| Prepared::new(*b)));
        self.stats.invalid_bodies = input.len() - self.next.len();
        self.next
            .sort_unstable_by(|a, b| geometry_order(a, b).then(b.height.total_cmp(&a.height)));
        self.next.dedup_by(|a, b| a.key == b.key);
        self.next
            .sort_unstable_by(|a, b| b.height.total_cmp(&a.height).then(geometry_order(a, b)));
        self.stats.unique_bodies = self.next.len();
        if self.softness == Some(softness) && self.next == self.bodies {
            return;
        }
        std::mem::swap(&mut self.next, &mut self.bodies);
        self.softness = Some(softness);
        self.heights.resize(SIDE * SIDE, 0.0);
        self.heights.fill(0.0);
        self.minima.resize(TILES * TILES, 0.0);
        self.minima.fill(0.0);
        self.stats.rebuilt = true;
        for body in &self.bodies {
            let r = body.key[4];
            let lo: [usize; 2] = std::array::from_fn(|i| {
                (((body.key[i].min(body.key[i + 2]) - r - 1e-6).max(0.0) * GRID as f32).floor()
                    as usize
                    / TILE)
                    .min(TILES - 1)
            });
            let hi: [usize; 2] = std::array::from_fn(|i| {
                (((body.key[i].max(body.key[i + 2]) + r + 1e-6).min(1.0) * GRID as f32).ceil()
                    as usize
                    / TILE)
                    .min(TILES - 1)
            });
            for ty in lo[1]..=hi[1] {
                for tx in lo[0]..=hi[0] {
                    self.stats.tile_tests += 1;
                    let x0 = tx * TILE;
                    let y0 = ty * TILE;
                    let x1 = ((tx + 1) * TILE).min(SIDE);
                    let y1 = ((ty + 1) * TILE).min(SIDE);
                    let center = [
                        (x0 + x1 - 1) as f32 / (2 * GRID) as f32,
                        (y0 + y1 - 1) as f32 / (2 * GRID) as f32,
                    ];
                    let half_diagonal =
                        ((x1 - x0 - 1) as f32).hypot((y1 - y0 - 1) as f32) / (2 * GRID) as f32;
                    // Distance to a segment is 1-Lipschitz. Round the lower
                    // distance outward before evaluating the decreasing cap.
                    let distance =
                        (body.distance_squared(center).sqrt() - half_diagonal - 1e-6).max(0.0);
                    if distance >= r {
                        continue;
                    }
                    let upper = body.envelope(distance * distance, softness) + 1e-6;
                    let tile = ty * TILES + tx;
                    if upper <= self.minima[tile] {
                        continue;
                    }
                    let mut minimum = f32::INFINITY;
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let h = &mut self.heights[y * SIDE + x];
                            if *h < body.height {
                                self.stats.body_evaluations += 1;
                                *h = h.max(body.sample(
                                    [x as f32 / GRID as f32, y as f32 / GRID as f32],
                                    softness,
                                ));
                            }
                            minimum = minimum.min(*h);
                        }
                    }
                    self.minima[tile] = minimum;
                }
            }
        }
    }

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
        self.stats = RenderStats::default();
        let Some(area) = raster.width.checked_mul(raster.height) else {
            self.stats.rejected = true;
            return;
        };
        if area != raster.dots.len()
            || area > MAX_DOTS
            || raster.width > MAX_DIMENSION
            || raster.height > MAX_DIMENSION
        {
            self.stats.rejected = true;
            return;
        }
        raster.dots.fill(0.0);
        if bodies.len() > MAX_BODIES {
            self.stats.rejected = true;
            return;
        }
        let o = options.normalized();
        let Some(camera) = Camera::new(raster.width, raster.height, &o) else {
            return;
        };
        if o.line_width == 0.0 {
            return;
        }
        self.prepare(bodies, o.softness);
        let lipschitz = self
            .bodies
            .iter()
            .map(|b| 2.0 * b.height / b.key[4])
            .fold(0.0, f32::max);
        // Bilinear reconstruction <= L*sqrt(2)/(2G); chord interpolation
        // on steps <=1/(2G) adds <= L*sqrt(2)/(4G).
        self.stats.height_error_bound_dots =
            camera.lift * lipschitz * 3.0 * 2.0_f32.sqrt() / (4 * GRID) as f32;
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
                let p = point(lerp(start, end, step as f32 / steps as f32));
                let next = camera.dots(p, self.height(p));
                self.stats.samples += 1;
                stroke(raster, previous, next, o.line_width * 0.5, &mut self.stats);
                previous = next;
            }
        }
    }
}

/// Allocation-owning convenience path. A live scene should retain Renderer.
#[cfg(test)]
pub fn render(raster: &mut Raster, bodies: &[Body], options: &RenderOptions) {
    Renderer::default().render(raster, bodies, options);
}

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
