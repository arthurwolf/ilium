//! The dot physics: wind, gravity, drag, collisions with screen text and the
//! pushes that changing text gives to nearby dots.
//!
//! Positions are in cells (`x` columns, `y` rows). A terminal cell is about
//! twice as tall as wide, so velocities and forces are expressed in "cell
//! widths" and divided by `ASPECT` when they move a dot vertically. Dots exist
//! only in empty cells: every move is checked against the occupancy mask.

use super::flow::{self, CellPush, PushKind};
use super::settings::{EdgeMode, WindMode, WindSettings};
use crate::scene::OccupancyMask;

/// Height of a cell relative to its width.
const ASPECT: f32 = 2.0;
/// Largest simulation step in seconds.
///
/// This is a per-step performance and fidelity limit for the particle
/// integrator, not a stability guarantee from a fluid solver. The analytic
/// gust force changes with particle position and time, so shorter substeps
/// evaluate different forces. Longer stalls remain bounded by `MAX_STEPS`
/// rather than trying to catch up indefinitely.
const MAX_STEP: f32 = 1.0 / 20.0;
/// Most steps one render may run; longer gaps are dropped, not replayed.
const MAX_STEPS: usize = 15;
/// Top speed in cell widths per second. Collision sweeps handle multi-cell steps.
const MAX_SPEED: f32 = 45.0;
/// Wind force at 100% strength.
const WIND_FORCE: f32 = 60.0;
/// Drag coefficient at 100% drag.
const DRAG_COEFFICIENT: f32 = 5.0;
/// Gravity acceleration at 100% strength, in cell widths per second squared.
const GRAVITY: f32 = 50.0;
/// Push speed at 100% scroll or appear push, in cell widths per second.
const PUSH_SPEED: f32 = 36.0;
/// Local pointer field, measured in cell widths so it looks circular on screen.
pub(super) const MOUSE_RADIUS: f32 = 6.0;
const MOUSE_FORCE: f32 = 180.0;
/// Reach of the dot-to-dot repulsion, measured in cell widths.
const DIFFUSION_RADIUS: f32 = 3.0;
/// Repulsion acceleration between two touching dots at 100% diffusion.
const DIFFUSION_FORCE: f32 = 60.0;
/// Most neighbours that push one dot per step; bounds the cost in dense piles.
const DIFFUSION_MAX_NEIGHBOURS: usize = 24;
/// Require a high population and dense buckets before paying to build AABBs.
const DIFFUSION_AABB_MIN_DOTS: usize = 20_000;
/// Sparse grids often have few candidate particles, so AABB checks can cost more than they save.
const DIFFUSION_AABB_MIN_DOTS_PER_BUCKET: usize = 8;
/// Teleported dots per second at 100% dispersion.
const DISPERSION_RATE: f32 = 100.0;
/// How far a displaced dot looks for an empty cell before it respawns.
const EJECT_RADIUS: i32 = 12;
/// Cells walked along a push direction when moving a dot out of new text.
const EJECT_LINE: i32 = 4;
/// Field spacing for ordinary gust simulation. Keep finer spacing where the
/// dense-particle fast path is not eligible, including stronger wind settings.
const WIND_FIELD_MAX_SPACING: usize = 2;
/// Dense-path field spacing. Across five terminal resolutions, exploratory
/// sampling found <0.81% local force error at the default wind/gust settings;
/// trajectory error still needs production validation. Particle interpolation
/// research motivates checking accumulated paths, not only local force error:
/// https://doi.org/10.1103/PhysRevE.87.043307
const CELL_WIND_FIELD_SPACING: usize = 4;
/// Bound cache memory for pathological terminal dimensions.
const WIND_FIELD_MAX_DIMENSION: usize = 512;
/// Use screen-cell gust samples only at high dot counts and near the default
/// wind and gust settings. Tests bound the sampled force error to 1.5%.
const CELL_WIND_MIN_DOTS: u32 = 20_000;
const CELL_WIND_MAX_STRENGTH: u32 = 35;
const CELL_WIND_MAX_GUSTS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// -1.0..=1.0, fixed per dot; scales the weight variation.
    pub weight_roll: f32,
}

/// xorshift64*: small, seedable and good enough for scattering dots.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in 0.0..1.0.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub struct Sim {
    settings: WindSettings,
    rng: Rng,
    dots: Vec<Dot>,
    /// Reciprocals aligned with `dots`; weights change only when settings do.
    inverse_masses: Vec<f32>,
    mask: OccupancyMask,
    /// Byte-per-cell copy for cheap collision probes in the particle loop.
    collision_mask: Vec<u8>,
    has_mask: bool,
    mask_has_occupied: bool,
    last_time: Option<f32>,
    pointer: Option<[f32; 2]>,
    /// Fractional teleports owed by the dispersion setting.
    dispersion_owed: f32,
    /// Scratch buffers of the neighbour search, kept to avoid per-step allocation.
    bucket_starts: Vec<u32>,
    bucket_fill: Vec<u32>,
    bucket_items: Vec<u32>,
    dot_buckets: Vec<u32>,
    /// Particle bounds for each occupied neighbor-search bucket.
    bucket_bounds: Vec<[f32; 4]>,
    repulsion: Vec<(f32, f32)>,
    repulsion_active: bool,
    wind_field: Vec<(f32, f32)>,
    wind_field_columns: usize,
    wind_field_rows: usize,
    /// One gust sample per terminal cell for the high-population fast path.
    wind_cell_field: Vec<[f32; 2]>,
    /// Runtime-dispatched scratch state for the dense wrapped gust path.
    simd_state: super::simd::State,
    #[cfg(test)]
    last_simd_path: Option<super::simd::KernelPath>,
}

/// Walk the canonical SoA buffers directly when SIMD state is loaded; SoAx
/// motivates contiguous particle-state traversal but does not establish Wind's
/// frame-time gain: https://arxiv.org/abs/1710.03462
pub(super) enum RenderPositions<'a> {
    Soa(std::iter::Zip<std::slice::Iter<'a, f32>, std::slice::Iter<'a, f32>>),
    Dots(std::slice::Iter<'a, Dot>),
}

impl Iterator for RenderPositions<'_> {
    type Item = (f32, f32);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Soa(positions) => positions.next().map(|(&x, &y)| (x, y)),
            Self::Dots(dots) => dots.next().map(|dot| (dot.x, dot.y)),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Soa(positions) => positions.size_hint(),
            Self::Dots(dots) => dots.size_hint(),
        }
    }
}

impl ExactSizeIterator for RenderPositions<'_> {}

fn wind_field_dimension(screen_dimension: u16, spacing: usize) -> usize {
    (usize::from(screen_dimension.max(1)).div_ceil(spacing) + 1).min(WIND_FIELD_MAX_DIMENSION)
}

fn use_diffusion_aabb_pruning(particle_count: usize, bucket_count: usize) -> bool {
    particle_count >= DIFFUSION_AABB_MIN_DOTS
        && particle_count / bucket_count.max(1) >= DIFFUSION_AABB_MIN_DOTS_PER_BUCKET
}

impl Sim {
    pub fn new(settings: &WindSettings) -> Self {
        let mut sim = Self {
            settings: settings.clone(),
            rng: Rng::new(u64::from(settings.seed) + 1),
            dots: Vec::new(),
            inverse_masses: Vec::new(),
            mask: OccupancyMask::default(),
            collision_mask: Vec::new(),
            has_mask: false,
            mask_has_occupied: false,
            last_time: None,
            pointer: None,
            dispersion_owed: 0.0,
            bucket_starts: Vec::new(),
            bucket_fill: Vec::new(),
            bucket_items: Vec::new(),
            dot_buckets: Vec::new(),
            bucket_bounds: Vec::new(),
            repulsion: Vec::new(),
            repulsion_active: false,
            wind_field: vec![(0.0, 0.0); 4],
            wind_field_columns: 2,
            wind_field_rows: 2,
            wind_cell_field: Vec::new(),
            simd_state: super::simd::State::default(),
            #[cfg(test)]
            last_simd_path: None,
        };
        sim.resize_population();
        sim
    }

    pub fn dots(&self) -> &[Dot] {
        &self.dots
    }

    fn flush_simd_state(&mut self) {
        self.simd_state.flush_to_dots(&mut self.dots);
    }

    fn invalidate_simd_state(&mut self) {
        self.flush_simd_state();
        self.simd_state.invalidate();
    }

    pub fn set_pointer(&mut self, position: Option<[f32; 2]>) {
        let position = position.filter(|position| {
            position
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        });
        // Pointer changes affect forces, not particle coordinates or masks.
        // Keep canonical SIMD state resident; scalar-only modes flush it
        // before their next step.
        self.pointer = position;
    }

    fn mouse_force_at(dot: &Dot, pointer: [f32; 2], width: f32, height: f32) -> (f32, f32) {
        Self::mouse_force_at_components(dot.x, dot.y, dot.weight_roll, pointer, width, height)
    }

    pub(super) fn mouse_force_at_components(
        x: f32,
        y: f32,
        weight_roll: f32,
        pointer: [f32; 2],
        width: f32,
        height: f32,
    ) -> (f32, f32) {
        let dx = x - pointer[0] * width;
        let dy = (y - pointer[1] * height) * ASPECT;
        let distance_squared = dx * dx + dy * dy;
        if distance_squared >= MOUSE_RADIUS * MOUSE_RADIUS {
            return (0.0, 0.0);
        }
        // Most dots are outside this small field. Reject them with squared
        // distance before paying for a square root on the near-pointer path.
        let distance = distance_squared.sqrt();
        // A dot exactly under the pointer needs a finite, deterministic direction.
        let (ux, uy) = if distance > 0.0001 {
            (dx / distance, dy / distance)
        } else {
            let angle = weight_roll * std::f32::consts::PI;
            (angle.cos(), angle.sin())
        };
        let falloff = 1.0 - distance / MOUSE_RADIUS;
        let force = MOUSE_FORCE * falloff * falloff;
        (ux * force, uy * force)
    }

    /// Test helper: stops every dot at one place.
    #[cfg(test)]
    pub fn park_all_for_test(&mut self, x: f32, y: f32) {
        self.invalidate_simd_state();
        for dot in &mut self.dots {
            dot.x = x;
            dot.y = y;
            dot.vx = 0.0;
            dot.vy = 0.0;
        }
    }

    pub fn mask(&self) -> &OccupancyMask {
        &self.mask
    }

    /// Takes changed settings, keeping the dots that exist.
    pub fn reconfigure(&mut self, settings: &WindSettings) {
        if &self.settings == settings {
            return;
        }
        self.invalidate_simd_state();
        let reseed = settings.seed != self.settings.seed;
        let mass_settings_changed = settings.dot_weight != self.settings.dot_weight
            || settings.weight_variation != self.settings.weight_variation;
        self.settings = settings.clone();
        if reseed {
            self.rng = Rng::new(u64::from(settings.seed) + 1);
            self.dots.clear();
            self.inverse_masses.clear();
        } else if mass_settings_changed {
            self.inverse_masses.clear();
        }
        self.resize_population();
        if mass_settings_changed && !reseed {
            // Population growth appends new dots before filling a cleared cache,
            // which would pair those masses with the older dots at the front.
            self.inverse_masses = self
                .dots
                .iter()
                .map(|dot| self.inverse_mass_for_roll(dot.weight_roll))
                .collect();
        }
        if self.has_mask {
            self.configure_wind_field_dimensions();
        }
        if self.has_mask {
            for index in 0..self.dots.len() {
                self.respawn_if_blocked(index);
            }
        }
    }

    fn resize_population(&mut self) {
        let wanted = self.settings.dot_count as usize;
        self.dots.truncate(wanted);
        self.inverse_masses.truncate(wanted);
        while self.dots.len() < wanted {
            let dot = self.new_dot();
            let inverse_mass = self.inverse_mass_for_roll(dot.weight_roll);
            self.dots.push(dot);
            self.inverse_masses.push(inverse_mass);
        }
        while self.inverse_masses.len() < self.dots.len() {
            let index = self.inverse_masses.len();
            let inverse_mass = self.inverse_mass_for_roll(self.dots[index].weight_roll);
            self.inverse_masses.push(inverse_mass);
        }
    }

    fn new_dot(&mut self) -> Dot {
        let (width, height) = (
            f32::from(self.mask.width().max(1)),
            f32::from(self.mask.height().max(1)),
        );
        let mut dot = Dot {
            x: self.rng.unit() * width,
            y: self.rng.unit() * height,
            vx: 0.0,
            vy: 0.0,
            weight_roll: self.rng.unit() * 2.0 - 1.0,
        };
        self.place_in_empty_cell(&mut dot);
        dot
    }

    /// Moves a dot to a random empty cell; leaves it where it is when the
    /// screen has none or no mask has arrived yet.
    fn place_in_empty_cell(&mut self, dot: &mut Dot) {
        if !self.has_mask {
            return;
        }
        let (width, height) = (
            i32::from(self.mask.width()).max(1),
            i32::from(self.mask.height()).max(1),
        );
        for _ in 0..64 {
            let column = (self.rng.next_u64() % width as u64) as i32;
            let row = (self.rng.next_u64() % height as u64) as i32;
            if !self.mask.is_occupied(column, row) {
                dot.x = column as f32 + self.rng.unit();
                dot.y = row as f32 + self.rng.unit();
                return;
            }
        }
    }

    fn mass(&self, dot: &Dot) -> f32 {
        let base = self.settings.dot_weight as f32 / 20.0;
        let spread = self.settings.weight_variation as f32 / 100.0 * 0.8;
        (base * (1.0 + dot.weight_roll * spread)).max(0.05)
    }

    fn inverse_mass_for_roll(&self, weight_roll: f32) -> f32 {
        let base = self.settings.dot_weight as f32 / 20.0;
        let spread = self.settings.weight_variation as f32 / 100.0 * 0.8;
        1.0 / (base * (1.0 + weight_roll * spread)).max(0.05)
    }

    fn wind_angle(&self, time: f32) -> f32 {
        let start = self.settings.wind_angle as f32;
        let turned = match self.settings.wind_mode {
            WindMode::Fixed => 0.0,
            WindMode::Rotating => self.settings.rotation_speed as f32 * time,
        };
        (start + turned).to_radians()
    }

    /// Wind acceleration direction and strength at a place, gusts included.
    fn wind_at(&self, angle: f32, x: f32, y: f32, time: f32) -> (f32, f32) {
        let gust = self.settings.gusts as f32 / 100.0;
        let swell = (0.21 * x + 0.9 * time + (0.13 * y + 0.5 * time).sin()).sin() * 0.5
            + (0.17 * y - 0.6 * time + 0.05 * x).sin() * 0.5;
        let strength =
            self.settings.wind_strength as f32 / 100.0 * WIND_FORCE * (1.0 + gust * 0.8 * swell);
        let veer = gust * 0.5 * (0.11 * x - 0.11 * y + 0.7 * time).sin();
        let direction = angle + veer;
        (direction.cos() * strength, direction.sin() * strength)
    }

    pub(super) fn update_wind_field(&mut self, angle: f32, time: f32) {
        let width = f32::from(self.mask.width().max(1));
        let height = f32::from(self.mask.height().max(1));
        for row in 0..self.wind_field_rows {
            let y = row as f32 * height / (self.wind_field_rows - 1) as f32;
            for column in 0..self.wind_field_columns {
                let x = column as f32 * width / (self.wind_field_columns - 1) as f32;
                self.wind_field[row * self.wind_field_columns + column] =
                    self.wind_at(angle, x, y, time);
            }
        }
        if self.uses_cell_wind_field() {
            self.update_wind_cell_field();
        }
    }

    fn uses_cell_wind_field(&self) -> bool {
        self.settings.dot_count >= CELL_WIND_MIN_DOTS
            && self.settings.wind_strength <= CELL_WIND_MAX_STRENGTH
            && self.settings.gusts <= CELL_WIND_MAX_GUSTS
            && self.mask.width() > 0
            && self.mask.height() > 0
    }

    fn configure_wind_field_dimensions(&mut self) {
        let spacing = if self.uses_cell_wind_field() {
            CELL_WIND_FIELD_SPACING
        } else {
            WIND_FIELD_MAX_SPACING
        };
        let columns = wind_field_dimension(self.mask.width(), spacing);
        let rows = wind_field_dimension(self.mask.height(), spacing);
        if columns != self.wind_field_columns || rows != self.wind_field_rows {
            self.wind_field_columns = columns;
            self.wind_field_rows = rows;
            self.wind_field.resize(columns * rows, (0.0, 0.0));
        }
    }

    fn update_wind_cell_field(&mut self) {
        let width = usize::from(self.mask.width());
        let height = usize::from(self.mask.height());
        self.wind_cell_field.resize(width * height, [0.0; 2]);
        let scale_x = (self.wind_field_columns - 1) as f32 / width.max(1) as f32;
        let scale_y = (self.wind_field_rows - 1) as f32 / height.max(1) as f32;
        for row in 0..height {
            for column in 0..width {
                let x = column as f32 + 0.5;
                let y = row as f32 + 0.5;
                let (force_x, force_y) = self.wind_force_at_in_bounds(x, y, scale_x, scale_y);
                self.wind_cell_field[row * width + column] = [force_x, force_y];
            }
        }
    }

    #[cfg(test)]
    pub(super) fn wind_force_at(&self, x: f32, y: f32) -> (f32, f32) {
        let width = f32::from(self.mask.width().max(1));
        let height = f32::from(self.mask.height().max(1));
        self.wind_force_at_scaled(
            x,
            y,
            (self.wind_field_columns - 1) as f32 / width,
            (self.wind_field_rows - 1) as f32 / height,
        )
    }

    /// Samples the cached field when the caller already has screen-to-grid
    /// scales, avoiding two repeated divisions per dot.
    pub(super) fn wind_force_at_scaled(
        &self,
        x: f32,
        y: f32,
        scale_x: f32,
        scale_y: f32,
    ) -> (f32, f32) {
        let grid_x = (x * scale_x).clamp(0.0, (self.wind_field_columns - 1) as f32);
        let grid_y = (y * scale_y).clamp(0.0, (self.wind_field_rows - 1) as f32);
        let left = grid_x.floor() as usize;
        let top = grid_y.floor() as usize;
        let right = (left + 1).min(self.wind_field_columns - 1);
        let bottom = (top + 1).min(self.wind_field_rows - 1);
        let tx = grid_x - left as f32;
        let ty = grid_y - top as f32;
        let top_left = self.wind_field[top * self.wind_field_columns + left];
        let top_right = self.wind_field[top * self.wind_field_columns + right];
        let bottom_left = self.wind_field[bottom * self.wind_field_columns + left];
        let bottom_right = self.wind_field[bottom * self.wind_field_columns + right];
        let top_force = (
            top_left.0 + (top_right.0 - top_left.0) * tx,
            top_left.1 + (top_right.1 - top_left.1) * tx,
        );
        let bottom_force = (
            bottom_left.0 + (bottom_right.0 - bottom_left.0) * tx,
            bottom_left.1 + (bottom_right.1 - bottom_left.1) * tx,
        );
        (
            top_force.0 + (bottom_force.0 - top_force.0) * ty,
            top_force.1 + (bottom_force.1 - top_force.1) * ty,
        )
    }

    /// Fast sampler for the integrator's wrapped or bounded particle positions.
    /// The caller contract keeps positions inside the mask before each step, so
    /// the hot path can avoid clamp and neighbour-index min operations while
    /// preserving the same bilinear interpolation as `wind_force_at_scaled`.
    #[inline(always)]
    fn wind_force_at_in_bounds(&self, x: f32, y: f32, scale_x: f32, scale_y: f32) -> (f32, f32) {
        debug_assert!(x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0);
        let grid_x = x * scale_x;
        let grid_y = y * scale_y;
        let left = grid_x as usize;
        let top = grid_y as usize;
        debug_assert!(
            left < self.wind_field_columns - 1 && top < self.wind_field_rows - 1,
            "wind sampler requires an in-bounds position"
        );
        let tx = grid_x - left as f32;
        let ty = grid_y - top as f32;
        let top_left = self.wind_field[top * self.wind_field_columns + left];
        let top_right = self.wind_field[top * self.wind_field_columns + left + 1];
        let bottom_left = self.wind_field[(top + 1) * self.wind_field_columns + left];
        let bottom_right = self.wind_field[(top + 1) * self.wind_field_columns + left + 1];
        let top_force = (
            top_left.0 + (top_right.0 - top_left.0) * tx,
            top_left.1 + (top_right.1 - top_left.1) * tx,
        );
        let bottom_force = (
            bottom_left.0 + (bottom_right.0 - bottom_left.0) * tx,
            bottom_left.1 + (bottom_right.1 - bottom_left.1) * tx,
        );
        (
            top_force.0 + (bottom_force.0 - top_force.0) * ty,
            top_force.1 + (bottom_force.1 - top_force.1) * ty,
        )
    }

    fn is_blocked(&self, column: i32, row: i32) -> bool {
        let (width, height) = (i32::from(self.mask.width()), i32::from(self.mask.height()));
        if !self.mask_has_occupied {
            return match self.settings.edge_mode {
                EdgeMode::Wrap if width > 0 && height > 0 => false,
                _ => column < 0 || row < 0 || column >= width || row >= height,
            };
        }
        match self.settings.edge_mode {
            EdgeMode::Wrap if width > 0 && height > 0 => {
                let (column, row) = if (0..width).contains(&column) && (0..height).contains(&row) {
                    (column as usize, row as usize)
                } else {
                    (
                        column.rem_euclid(width) as usize,
                        row.rem_euclid(height) as usize,
                    )
                };
                self.collision_mask[row * width as usize + column] != 0
            }
            _ => {
                if column < 0 || row < 0 || column >= width || row >= height {
                    return true;
                }
                self.collision_mask[row as usize * width as usize + column as usize] != 0
            }
        }
    }

    fn swept_x_blocked(&self, from: i32, to: i32, row: i32) -> bool {
        let direction = (to - from).signum();
        let mut column = from + direction;
        while direction != 0
            && (if direction > 0 {
                column <= to
            } else {
                column >= to
            })
        {
            if self.is_blocked(column, row) {
                return true;
            }
            column += direction;
        }
        false
    }

    fn swept_y_blocked(&self, from: i32, to: i32, column: i32) -> bool {
        let direction = (to - from).signum();
        let mut row = from + direction;
        while direction != 0 && (if direction > 0 { row <= to } else { row >= to }) {
            if self.is_blocked(column, row) {
                return true;
            }
            row += direction;
        }
        false
    }

    fn wrap(&self, dot: &mut Dot) {
        if self.settings.edge_mode != EdgeMode::Wrap {
            return;
        }
        let (width, height) = (f32::from(self.mask.width()), f32::from(self.mask.height()));
        Self::wrap_position(dot, width, height);
    }

    #[inline]
    fn wrap_position(dot: &mut Dot, width: f32, height: f32) {
        if width > 0.0 && !(0.0..width).contains(&dot.x) {
            let wrapped = dot.x.rem_euclid(width);
            dot.x = if wrapped >= width { 0.0 } else { wrapped };
        }
        if height > 0.0 && !(0.0..height).contains(&dot.y) {
            let wrapped = dot.y.rem_euclid(height);
            dot.y = if wrapped >= height { 0.0 } else { wrapped };
        }
    }

    fn cell_of(dot: &Dot) -> (i32, i32) {
        (dot.x.floor() as i32, dot.y.floor() as i32)
    }

    fn respawn_if_blocked(&mut self, index: usize) {
        let mut dot = self.dots[index];
        let (column, row) = Self::cell_of(&dot);
        if self.is_blocked(column, row) {
            self.place_in_empty_cell(&mut dot);
            dot.vx = 0.0;
            dot.vy = 0.0;
            self.dots[index] = dot;
        }
    }

    /// Receives the new screen occupancy. Newly occupied cells push nearby
    /// dots; dots standing in them are moved out first.
    pub fn set_mask(&mut self, mask: &OccupancyMask) {
        if self.has_mask && *mask == self.mask {
            return;
        }
        self.invalidate_simd_state();
        let same_size = self.has_mask
            && mask.width() == self.mask.width()
            && mask.height() == self.mask.height();
        let pushes = if same_size {
            flow::analyze(&self.mask, mask, self.settings.scroll_range as i32)
        } else {
            Vec::new()
        };
        let was_missing = !self.has_mask || !same_size;
        self.mask = mask.clone();
        let mask_width = usize::from(mask.width());
        let mask_height = usize::from(mask.height());
        self.collision_mask
            .resize(mask_width.saturating_mul(mask_height), 0);
        let mut mask_has_occupied = false;
        for row in 0..mask.height() {
            for column in 0..mask.width() {
                let index = usize::from(row) * mask_width + usize::from(column);
                let occupied = mask.is_occupied(i32::from(column), i32::from(row));
                self.collision_mask[index] = u8::from(occupied);
                mask_has_occupied |= occupied;
            }
        }
        self.configure_wind_field_dimensions();
        if self.uses_cell_wind_field() {
            self.wind_cell_field
                .resize(mask_width * mask_height, [0.0, 0.0]);
        } else {
            self.wind_cell_field.clear();
        }
        self.has_mask = true;
        self.mask_has_occupied = mask_has_occupied;
        if was_missing {
            // First mask or a resized screen: scatter every dot into empty space.
            for index in 0..self.dots.len() {
                let mut dot = self.dots[index];
                let (width, height) = (
                    f32::from(mask.width().max(1)),
                    f32::from(mask.height().max(1)),
                );
                dot.x = self.rng.unit() * width;
                dot.y = self.rng.unit() * height;
                self.place_in_empty_cell(&mut dot);
                self.dots[index] = dot;
            }
            return;
        }
        self.apply_pushes(&pushes);
    }

    fn apply_pushes(&mut self, pushes: &[CellPush]) {
        if pushes.is_empty() {
            return;
        }
        let (width, height) = (i32::from(self.mask.width()), i32::from(self.mask.height()));
        let mut grid: Vec<Option<CellPush>> = vec![None; (width * height) as usize];
        for push in pushes {
            grid[(push.row * width + push.column) as usize] = Some(*push);
        }
        let reach = self.settings.push_reach as i32;
        for index in 0..self.dots.len() {
            let dot = self.dots[index];
            let (column, row) = Self::cell_of(&dot);
            let mut best: Option<(f32, CellPush)> = None;
            for neighbour_row in (row - reach)..=(row + reach) {
                for neighbour_column in (column - reach)..=(column + reach) {
                    if neighbour_column < 0
                        || neighbour_row < 0
                        || neighbour_column >= width
                        || neighbour_row >= height
                    {
                        continue;
                    }
                    let Some(push) = grid[(neighbour_row * width + neighbour_column) as usize]
                    else {
                        continue;
                    };
                    let distance = (neighbour_column - column)
                        .abs()
                        .max((neighbour_row - row).abs());
                    let weight = 1.0 / (1.0 + distance as f32);
                    if best.is_none_or(|(held, _)| weight > held) {
                        best = Some((weight, push));
                    }
                }
            }
            let inside = self.mask.is_occupied(column, row);
            match best {
                Some((weight, push)) => self.push_dot(index, push, weight),
                None if inside => self.eject(index, (0.0, 0.0)),
                None => {}
            }
        }
    }

    fn push_dot(&mut self, index: usize, push: CellPush, weight: f32) {
        let mut dot = self.dots[index];
        let heaviness = self.mass(&dot).sqrt().max(0.2);
        let (direction, speed) = match push.kind {
            PushKind::Scroll { dx, dy, distance } => {
                let length = ((dx * dx + dy * dy) as f32).sqrt().max(1.0);
                (
                    (dx as f32 / length, dy as f32 / length),
                    self.settings.scroll_push as f32 / 100.0
                        * PUSH_SPEED
                        * (distance as f32).sqrt(),
                )
            }
            PushKind::Appear => {
                let centre = (push.column as f32 + 0.5, push.row as f32 + 0.5);
                let (mut ox, mut oy) = (dot.x - centre.0, (dot.y - centre.1) * ASPECT);
                let length = (ox * ox + oy * oy).sqrt();
                if length < 1e-3 {
                    let angle = self.rng.unit() * std::f32::consts::TAU;
                    (ox, oy) = (angle.cos(), angle.sin());
                } else {
                    (ox, oy) = (ox / length, oy / length);
                }
                (
                    (ox, oy),
                    self.settings.appear_push as f32 / 100.0 * PUSH_SPEED,
                )
            }
        };
        let speed = speed * weight.sqrt() / heaviness;
        // Never slow a dot that already moves faster along the push.
        let along = dot.vx * direction.0 + dot.vy * direction.1;
        let gain = (speed - along).max(0.0);
        dot.vx += direction.0 * gain;
        dot.vy += direction.1 * gain;
        self.dots[index] = dot;
        if self
            .mask
            .is_occupied(Self::cell_of(&dot).0, Self::cell_of(&dot).1)
        {
            self.eject(index, direction);
        }
    }

    /// Moves a dot out of an occupied cell: along `direction` first (so text
    /// that scrolls carries the dot with it), then to the nearest empty cell,
    /// and as a last resort to a random empty cell.
    fn eject(&mut self, index: usize, direction: (f32, f32)) {
        let mut dot = self.dots[index];
        let (column, row) = Self::cell_of(&dot);
        if direction != (0.0, 0.0) {
            let (step_x, step_y) = (direction.0.round() as i32, direction.1.round() as i32);
            if step_x != 0 || step_y != 0 {
                for step in 1..=EJECT_LINE {
                    let target = (column + step_x * step, row + step_y * step);
                    if !self.is_blocked(target.0, target.1) {
                        self.settle(&mut dot, target);
                        self.dots[index] = dot;
                        return;
                    }
                }
            }
        }
        for radius in 1..=EJECT_RADIUS {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs().max(dy.abs()) != radius {
                        continue;
                    }
                    let target = (column + dx, row + dy);
                    if !self.is_blocked(target.0, target.1) {
                        self.settle(&mut dot, target);
                        self.dots[index] = dot;
                        return;
                    }
                }
            }
        }
        self.place_in_empty_cell(&mut dot);
        self.dots[index] = dot;
    }

    fn settle(&mut self, dot: &mut Dot, cell: (i32, i32)) {
        dot.x = cell.0 as f32 + 0.25 + self.rng.unit() * 0.5;
        dot.y = cell.1 as f32 + 0.25 + self.rng.unit() * 0.5;
        self.wrap(dot);
    }

    /// Advances the physics to animation time `time` (seconds).
    pub fn advance(&mut self, time: f32) {
        self.advance_inner(time, true);
    }

    pub(super) fn advance_for_render(&mut self, time: f32) {
        self.advance_inner(time, false);
    }

    fn advance_inner(&mut self, time: f32, flush_positions: bool) {
        let elapsed = match self.last_time {
            Some(last) if time >= last => time - last,
            _ => 0.0,
        };
        self.last_time = Some(time);
        if !self.has_mask || elapsed <= 0.0 {
            return;
        }
        let steps = ((elapsed / MAX_STEP).ceil() as usize).clamp(1, MAX_STEPS);
        let step = (elapsed / steps as f32).min(MAX_STEP);
        for index in 0..steps {
            let at = time - elapsed + step * (index + 1) as f32;
            self.step(step, at);
        }
        // Public callers keep the established AoS view; the renderer can read
        // the canonical SoA positions directly and skip this per-dot copy.
        if flush_positions {
            self.flush_simd_state();
        }
    }

    pub(super) fn render_positions(&self) -> RenderPositions<'_> {
        match self.simd_state.positions() {
            Some((x, y)) => RenderPositions::Soa(x.iter().zip(y)),
            None => RenderPositions::Dots(self.dots.iter()),
        }
    }

    /// Fills `self.repulsion` with the push each dot gets from its close
    /// neighbours. Dots are binned into square buckets of `DIFFUSION_RADIUS`
    /// (rows scaled by `ASPECT` so buckets look square on screen), so only the
    /// 3x3 buckets around a dot are searched.
    fn compute_repulsion(&mut self) {
        let count = self.dots.len();
        let strength = self.settings.diffusion as f32 / 100.0 * DIFFUSION_FORCE;
        if strength <= 0.0 || count < 2 {
            if self.repulsion_active {
                self.repulsion.fill((0.0, 0.0));
                self.repulsion_active = false;
            }
            return;
        }
        self.repulsion.clear();
        self.repulsion.resize(count, (0.0, 0.0));
        // Uniform-grid bins limit candidate checks to nearby particles; the
        // exact radius test below decides which candidates interact. This is
        // the grid-based neighborhood-search approach evaluated by Groß et
        // al., *Fast and Efficient Nearest Neighbor Search for Particle
        // Simulations*: https://diglib.eg.org/items/22911d76-8dbe-4d98-b082-83c90f529c14
        let columns = (f32::from(self.mask.width()) / DIFFUSION_RADIUS).ceil() as usize + 1;
        let rows = (f32::from(self.mask.height()) * ASPECT / DIFFUSION_RADIUS).ceil() as usize + 1;
        let bucket_of = |dot: &Dot| -> usize {
            let column = ((dot.x / DIFFUSION_RADIUS).max(0.0) as usize).min(columns - 1);
            let row = ((dot.y * ASPECT / DIFFUSION_RADIUS).max(0.0) as usize).min(rows - 1);
            row * columns + column
        };
        // Counting sort of dot indices by bucket.
        let mut starts = std::mem::take(&mut self.bucket_starts);
        let mut fill = std::mem::take(&mut self.bucket_fill);
        let mut items = std::mem::take(&mut self.bucket_items);
        let mut dot_buckets = std::mem::take(&mut self.dot_buckets);
        let bucket_count = columns * rows;
        let use_bounds = use_diffusion_aabb_pruning(count, bucket_count);
        starts.clear();
        starts.resize(bucket_count + 1, 0);
        dot_buckets.clear();
        dot_buckets.resize(count, 0);
        let mut bounds = std::mem::take(&mut self.bucket_bounds);
        bounds.clear();
        if use_bounds {
            bounds.resize(
                bucket_count,
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
            );
        }
        for (index, dot) in self.dots.iter().enumerate() {
            let bucket = bucket_of(dot);
            dot_buckets[index] = bucket as u32;
            starts[bucket + 1] += 1;
            if use_bounds {
                let scaled_y = dot.y * ASPECT;
                let bucket_bounds = &mut bounds[bucket];
                bucket_bounds[0] = bucket_bounds[0].min(dot.x);
                bucket_bounds[1] = bucket_bounds[1].min(scaled_y);
                bucket_bounds[2] = bucket_bounds[2].max(dot.x);
                bucket_bounds[3] = bucket_bounds[3].max(scaled_y);
            }
        }
        for index in 1..starts.len() {
            starts[index] += starts[index - 1];
        }
        items.clear();
        items.resize(count, 0);
        fill.clear();
        fill.extend_from_slice(&starts[..starts.len() - 1]);
        for (index, &bucket_value) in dot_buckets.iter().enumerate() {
            let bucket = bucket_value as usize;
            items[fill[bucket] as usize] = index as u32;
            fill[bucket] += 1;
        }
        let radius_squared = DIFFUSION_RADIUS * DIFFUSION_RADIUS;
        for index in 0..count {
            let dot = self.dots[index];
            let dot_y = dot.y * ASPECT;
            let bucket = dot_buckets[index] as usize;
            let (column, row) = (bucket % columns, bucket / columns);
            let (mut push_x, mut push_y) = (0.0_f32, 0.0_f32);
            let mut seen = 0;
            'search: for neighbour_row in row.saturating_sub(1)..=(row + 1).min(rows - 1) {
                for neighbour_column in column.saturating_sub(1)..=(column + 1).min(columns - 1) {
                    let bucket = neighbour_row * columns + neighbour_column;
                    if starts[bucket] == starts[bucket + 1] {
                        continue;
                    }
                    if use_bounds {
                        // Exact particle bounds let us skip whole buckets
                        // whose contents cannot reach this dot. Takeshita's
                        // CPU AABB pruning preserves the neighbor set while
                        // reducing candidate checks:
                        // https://doi.org/10.3756/artsci.19.1
                        let bucket_bounds = bounds[bucket];
                        let distance_x = if dot.x < bucket_bounds[0] {
                            bucket_bounds[0] - dot.x
                        } else if dot.x > bucket_bounds[2] {
                            dot.x - bucket_bounds[2]
                        } else {
                            0.0
                        };
                        let distance_y = if dot_y < bucket_bounds[1] {
                            bucket_bounds[1] - dot_y
                        } else if dot_y > bucket_bounds[3] {
                            dot_y - bucket_bounds[3]
                        } else {
                            0.0
                        };
                        if distance_x * distance_x + distance_y * distance_y >= radius_squared {
                            continue;
                        }
                    }
                    for slot in starts[bucket]..starts[bucket + 1] {
                        let other_index = items[slot as usize] as usize;
                        if other_index == index {
                            continue;
                        }
                        let other = self.dots[other_index];
                        let dx = dot.x - other.x;
                        let dy = (dot.y - other.y) * ASPECT;
                        let distance_squared = dx * dx + dy * dy;
                        if distance_squared >= radius_squared {
                            continue;
                        }
                        let distance = distance_squared.sqrt();
                        // Dots on the same spot need a finite, deterministic direction.
                        let (ux, uy) = if distance > 0.0001 {
                            (dx / distance, dy / distance)
                        } else {
                            let angle = (dot.weight_roll - other.weight_roll
                                + index as f32 * 0.618)
                                * std::f32::consts::PI;
                            (angle.cos(), angle.sin())
                        };
                        let falloff = 1.0 - distance / DIFFUSION_RADIUS;
                        push_x += ux * falloff * falloff;
                        push_y += uy * falloff * falloff;
                        seen += 1;
                        if seen >= DIFFUSION_MAX_NEIGHBOURS {
                            break 'search;
                        }
                    }
                }
            }
            self.repulsion[index] = (push_x * strength, push_y * strength);
        }
        self.bucket_starts = starts;
        self.bucket_fill = fill;
        self.bucket_items = items;
        self.dot_buckets = dot_buckets;
        self.bucket_bounds = bounds;
        self.repulsion_active = true;
    }

    /// Teleports random dots to random empty cells at the dispersion rate.
    fn disperse(&mut self, dt: f32) {
        if self.settings.dispersion == 0 || self.dots.is_empty() {
            return;
        }
        self.dispersion_owed += self.settings.dispersion as f32 / 100.0 * DISPERSION_RATE * dt;
        // Never owe more than one pass over the dots; a long gap is dropped.
        self.dispersion_owed = self.dispersion_owed.min(self.dots.len() as f32);
        while self.dispersion_owed >= 1.0 {
            self.dispersion_owed -= 1.0;
            let index = (self.rng.next_u64() % self.dots.len() as u64) as usize;
            let mut dot = self.dots[index];
            self.place_in_empty_cell(&mut dot);
            self.dots[index] = dot;
        }
    }

    /// Fast path when gusts are cached. Selecting the collision mode once per
    /// frame removes option/feature branches from every dot.
    fn integrate_gusted<const COLLISIONS: bool, const WRAPS: bool>(
        &mut self,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        scale_x: f32,
        scale_y: f32,
    ) {
        let width = f32::from(self.mask.width());
        let height = f32::from(self.mask.height());
        let cell_wind_field_enabled = self.uses_cell_wind_field();
        let screen_width = usize::from(self.mask.width());
        for index in 0..self.dots.len() {
            let mut dot = self.dots[index];
            let inverse_mass = self.inverse_masses[index];
            let (wind_x, wind_y) = if cell_wind_field_enabled {
                let column = dot.x as usize;
                let row = dot.y as usize;
                let [wind_x, wind_y] = self.wind_cell_field[row * screen_width + column];
                (wind_x, wind_y)
            } else {
                self.wind_force_at_in_bounds(dot.x, dot.y, scale_x, scale_y)
            };
            let ax = (wind_x - drag * dot.vx) * inverse_mass;
            let ay = (wind_y - drag * dot.vy) * inverse_mass + gravity;
            dot.vx = (dot.vx + ax * dt).clamp(-MAX_SPEED, MAX_SPEED);
            dot.vy = (dot.vy + ay * dt).clamp(-MAX_SPEED, MAX_SPEED);
            let next_x = dot.x + dot.vx * dt;
            let next_y = dot.y + dot.vy * dt / ASPECT;
            if COLLISIONS {
                let (column, row) = Self::cell_of(&dot);
                let next_column = next_x.floor() as i32;
                let column_after = if self.swept_x_blocked(column, next_column, row) {
                    dot.vx = -dot.vx * bounce;
                    column
                } else {
                    dot.x = next_x;
                    next_column
                };
                let next_row = next_y.floor() as i32;
                if self.swept_y_blocked(row, next_row, column_after) {
                    dot.vy = -dot.vy * bounce;
                } else {
                    dot.y = next_y;
                }
            } else {
                dot.x = next_x;
                dot.y = next_y;
            }
            if WRAPS {
                Self::wrap_position(&mut dot, width, height);
            }
            let invalid_position = dot.x.is_nan() || dot.y.is_nan();
            if invalid_position {
                dot.x = 0.0;
                dot.y = 0.0;
            }
            self.dots[index] = dot;
            if invalid_position {
                self.respawn_if_blocked(index);
            }
        }
    }

    fn step(&mut self, dt: f32, time: f32) {
        #[cfg(test)]
        {
            self.last_simd_path = None;
        }
        let can_keep_soa = self.settings.gusts > 0
            && self.settings.diffusion == 0
            && self.settings.dispersion == 0
            && self.settings.edge_mode == EdgeMode::Wrap
            && self.uses_cell_wind_field();
        if self.simd_state.dirty && !can_keep_soa {
            self.invalidate_simd_state();
        }
        self.compute_repulsion();
        self.disperse(dt);
        let angle = self.wind_angle(time);
        let gusts_enabled = self.settings.gusts > 0;
        if gusts_enabled {
            self.update_wind_field(angle, time);
        }
        let drag = self.settings.drag as f32 / 100.0 * DRAG_COEFFICIENT;
        let gravity = if self.settings.gravity_enabled {
            self.settings.gravity_strength as f32 / 100.0 * GRAVITY
        } else {
            0.0
        };
        let bounce = self.settings.bounce as f32 / 100.0;
        let collisions = self.mask_has_occupied || self.settings.edge_mode != EdgeMode::Wrap;
        let uniform_wind = (!gusts_enabled).then(|| {
            let strength = self.settings.wind_strength as f32 / 100.0 * WIND_FORCE;
            (angle.cos() * strength, angle.sin() * strength)
        });
        let wind_scale = gusts_enabled.then(|| {
            (
                (self.wind_field_columns - 1) as f32 / f32::from(self.mask.width().max(1)),
                (self.wind_field_rows - 1) as f32 / f32::from(self.mask.height().max(1)),
            )
        });
        let pointer = self.pointer.filter(|_| self.settings.mouse_force);
        let mask_width = f32::from(self.mask.width());
        let mask_height = f32::from(self.mask.height());
        let repulsion_active = self.repulsion_active;
        if gusts_enabled && !repulsion_active && self.settings.dispersion == 0 {
            let (scale_x, scale_y) = wind_scale.expect("gusted wind must have cached scales");
            // Particle-in-cell measurements show SIMD crossover depends on
            // workload density. Keep this route restricted to dense gust
            // scenes and retain scalar fallback; Wind's cell-sorting variant
            // was slower in the matched 20k/50k release experiment.
            // https://arxiv.org/abs/1810.03949
            if self.settings.edge_mode == EdgeMode::Wrap && self.uses_cell_wind_field() {
                let width = usize::from(self.mask.width());
                let height = usize::from(self.mask.height());
                // The kernel does not consult blocked cells when collisions
                // are disabled; avoid copying and widening the full mask in
                // the common empty-screen gust path.
                let collision_mask = if collisions {
                    self.collision_mask.as_slice()
                } else {
                    &[]
                };
                let path = self.simd_state.step_wrapped(
                    &mut self.dots,
                    &self.inverse_masses,
                    &self.wind_cell_field,
                    collision_mask,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    MAX_SPEED,
                    ASPECT,
                    collisions,
                    pointer,
                );
                #[cfg(test)]
                {
                    self.last_simd_path = path;
                }
                if path.is_some() {
                    return;
                }
                // A rejected/unsupported SIMD step leaves the `Dot` mirror as
                // the scalar path's source of truth. Flush and invalidate the
                // cached SoA before any scalar writes.
                self.invalidate_simd_state();
            }
            match (collisions, self.settings.edge_mode) {
                (true, EdgeMode::Wrap) => {
                    self.integrate_gusted::<true, true>(
                        dt, drag, gravity, bounce, scale_x, scale_y,
                    );
                }
                (true, EdgeMode::Bounce) => {
                    self.integrate_gusted::<true, false>(
                        dt, drag, gravity, bounce, scale_x, scale_y,
                    );
                }
                (false, EdgeMode::Wrap) => {
                    self.integrate_gusted::<false, true>(
                        dt, drag, gravity, bounce, scale_x, scale_y,
                    );
                }
                (false, EdgeMode::Bounce) => {
                    unreachable!("bounce edges always enable collision checks")
                }
            }
            return;
        }
        for index in 0..self.dots.len() {
            let mut dot = self.dots[index];
            let inverse_mass = self.inverse_masses[index];
            let (wind_x, wind_y) = match (uniform_wind, wind_scale) {
                (Some(force), _) => force,
                (_, Some((scale_x, scale_y))) => {
                    self.wind_force_at_scaled(dot.x, dot.y, scale_x, scale_y)
                }
                (None, None) => unreachable!("wind must have either uniform or gusted force"),
            };
            let (mouse_x, mouse_y) = pointer.map_or((0.0, 0.0), |pointer| {
                Self::mouse_force_at(&dot, pointer, mask_width, mask_height)
            });
            let (spread_x, spread_y) = if repulsion_active {
                self.repulsion[index]
            } else {
                (0.0, 0.0)
            };
            let ax = (wind_x + mouse_x - drag * dot.vx) * inverse_mass + spread_x;
            let ay = (wind_y + mouse_y - drag * dot.vy) * inverse_mass + gravity + spread_y;
            dot.vx = (dot.vx + ax * dt).clamp(-MAX_SPEED, MAX_SPEED);
            dot.vy = (dot.vy + ay * dt).clamp(-MAX_SPEED, MAX_SPEED);
            let next_x = dot.x + dot.vx * dt;
            let next_y = dot.y + dot.vy * dt / ASPECT;
            if collisions {
                let (column, row) = Self::cell_of(&dot);
                let next_column = next_x.floor() as i32;
                let column_after = if self.swept_x_blocked(column, next_column, row) {
                    dot.vx = -dot.vx * bounce;
                    column
                } else {
                    dot.x = next_x;
                    next_column
                };
                let next_row = next_y.floor() as i32;
                if self.swept_y_blocked(row, next_row, column_after) {
                    dot.vy = -dot.vy * bounce;
                } else {
                    dot.y = next_y;
                }
            } else {
                dot.x = next_x;
                dot.y = next_y;
            }
            self.wrap(&mut dot);
            let invalid_position = dot.x.is_nan() || dot.y.is_nan();
            if invalid_position {
                dot.x = 0.0;
                dot.y = 0.0;
            }
            self.dots[index] = dot;
            if invalid_position {
                self.respawn_if_blocked(index);
            }
        }
    }
}

#[cfg(test)]
mod repulsion_cache_tests {
    use super::super::settings::WindSettings;
    use super::{use_diffusion_aabb_pruning, Dot, Sim, ASPECT, DIFFUSION_FORCE, DIFFUSION_RADIUS};
    use crate::scene::OccupancyMask;

    #[test]
    fn disabled_diffusion_does_not_allocate_per_dot_repulsion() {
        let settings = WindSettings {
            dot_count: 50_000,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.compute_repulsion();

        assert!(!sim.repulsion_active);
        assert!(sim.repulsion.is_empty());
    }

    #[test]
    fn aabb_pruning_is_limited_to_large_dense_populations() {
        assert!(!use_diffusion_aabb_pruning(2_000, 1_925));
        assert!(use_diffusion_aabb_pruning(20_000, 1_925));
        assert!(!use_diffusion_aabb_pruning(50_000, 8_000));
    }

    #[test]
    fn squared_distance_filter_keeps_inside_and_excludes_radius_boundary() {
        let settings = WindSettings {
            dot_count: 2,
            diffusion: 100,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&OccupancyMask::empty(32, 20));
        sim.dots = vec![
            Dot {
                x: 10.0,
                y: 10.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
            Dot {
                x: 12.0,
                y: 10.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
        ];

        sim.compute_repulsion();
        assert!(sim.repulsion[0].0 < 0.0);
        assert!(sim.repulsion[1].0 > 0.0);
        assert!(sim.repulsion[0].0.abs() <= DIFFUSION_FORCE);

        sim.dots[1].x = 13.0;
        sim.compute_repulsion();
        assert_eq!(sim.repulsion, [(0.0, 0.0), (0.0, 0.0)]);
    }

    #[test]
    fn bucket_bounds_preserve_the_brute_force_neighbor_set() {
        let positions = [
            (2.95, 1.45),
            (3.05, 1.55),
            (5.95, 1.4),
            (6.05, 1.6),
            (2.95, 2.9),
            (3.1, 3.1),
            (11.0, 3.0),
            (13.8, 3.2),
            (16.0, 8.0),
            (17.5, 8.5),
            (30.0, 19.0),
            (30.5, 18.0),
        ];
        let settings = WindSettings {
            dot_count: 20_000,
            diffusion: 100,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&OccupancyMask::empty(32, 20));
        let mut dots = positions
            .iter()
            .map(|&(x, y)| Dot {
                x,
                y,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            })
            .collect::<Vec<_>>();
        dots.resize(
            settings.dot_count as usize,
            Dot {
                x: 0.0,
                y: 0.0,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            },
        );
        sim.dots = dots;

        let expected = sim
            .dots
            .iter()
            .enumerate()
            .take(positions.len())
            .map(|(index, dot)| {
                let mut push = (0.0_f32, 0.0_f32);
                for (other_index, other) in sim.dots.iter().enumerate() {
                    if other_index == index {
                        continue;
                    }
                    let dx = dot.x - other.x;
                    let dy = (dot.y - other.y) * ASPECT;
                    let distance = dx.hypot(dy);
                    if distance >= DIFFUSION_RADIUS {
                        continue;
                    }
                    let falloff = 1.0 - distance / DIFFUSION_RADIUS;
                    push.0 += dx / distance * falloff * falloff * DIFFUSION_FORCE;
                    push.1 += dy / distance * falloff * falloff * DIFFUSION_FORCE;
                }
                push
            })
            .collect::<Vec<_>>();

        sim.compute_repulsion();
        for (actual, expected) in sim.repulsion.iter().zip(expected) {
            assert!((actual.0 - expected.0).abs() < 0.0001);
            assert!((actual.1 - expected.1).abs() < 0.0001);
        }
    }
}

#[cfg(test)]
mod gust_field_accuracy_tests {
    use super::super::settings::WindSettings;
    use super::{Sim, WIND_FORCE};
    use crate::scene::OccupancyMask;

    #[test]
    // Local force error is not an accumulated path-error bound. The tradeoff
    // should also be checked on particle trajectories, as in van Hinsberg et
    // al., *Optimal interpolation schemes for particle tracking in turbulence*:
    // https://doi.org/10.1103/PhysRevE.87.043307
    fn cached_gust_field_stays_within_three_percent_of_exact_force() {
        let settings = WindSettings {
            gusts: 100,
            wind_strength: 100,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        let mut maximum_error = 0.0_f32;

        for (width, height) in [(80, 24), (160, 50), (240, 80)] {
            sim.set_mask(&OccupancyMask::empty(width, height));
            let expected_columns = usize::from(width).div_ceil(2) + 1;
            let expected_rows = usize::from(height).div_ceil(2) + 1;
            assert_eq!(
                (sim.wind_field_columns, sim.wind_field_rows),
                (expected_columns, expected_rows)
            );
            assert_eq!(sim.wind_field.len(), expected_columns * expected_rows);
            for time in [0.0, 0.7, 1.4, 2.3] {
                let angle = sim.wind_angle(time);
                sim.update_wind_field(angle, time);
                for row in 0..height {
                    for column in 0..width {
                        for sub_y in 0..3 {
                            for sub_x in 0..3 {
                                let x = f32::from(column) + (sub_x as f32 + 0.5) / 3.0;
                                let y = f32::from(row) + (sub_y as f32 + 0.5) / 3.0;
                                let exact = sim.wind_at(angle, x, y, time);
                                let cached = sim.wind_force_at(x, y);
                                maximum_error = maximum_error
                                    .max((exact.0 - cached[0]).hypot(exact.1 - cached[1]));
                            }
                        }
                    }
                }
            }
        }

        let error_fraction = maximum_error / WIND_FORCE;
        assert!(
            error_fraction <= 0.03,
            "cached gust force error {:.2}% exceeds 3% of base wind force",
            error_fraction * 100.0
        );
    }
}

#[cfg(test)]
mod cell_wind_field_tests {
    use super::super::settings::WindSettings;
    use super::{Sim, WIND_FORCE};
    use crate::scene::OccupancyMask;

    #[test]
    fn cell_field_is_reserved_for_high_dot_default_wind() {
        let ordinary = Sim::new(&WindSettings::default());
        assert!(!ordinary.uses_cell_wind_field());

        let dense_settings = WindSettings {
            dot_count: 20_000,
            ..WindSettings::default()
        };
        let dense = Sim::new(&dense_settings);
        assert!(dense.uses_cell_wind_field());

        let stronger_wind = WindSettings {
            dot_count: 50_000,
            wind_strength: 36,
            ..WindSettings::default()
        };
        assert!(!Sim::new(&stronger_wind).uses_cell_wind_field());

        let stronger_gusts = WindSettings {
            dot_count: 50_000,
            gusts: 31,
            ..WindSettings::default()
        };
        assert!(!Sim::new(&stronger_gusts).uses_cell_wind_field());
    }

    #[test]
    fn field_resolution_tracks_dense_fast_path_settings() {
        let mut sim = Sim::new(&WindSettings {
            dot_count: 20_000,
            ..WindSettings::default()
        });
        sim.set_mask(&OccupancyMask::empty(160, 50));
        assert_eq!((sim.wind_field_columns, sim.wind_field_rows), (41, 14));

        sim.reconfigure(&WindSettings {
            dot_count: 20_000,
            wind_strength: 36,
            ..WindSettings::default()
        });
        assert!(!sim.uses_cell_wind_field());
        assert_eq!((sim.wind_field_columns, sim.wind_field_rows), (81, 26));

        sim.reconfigure(&WindSettings {
            dot_count: 20_000,
            ..WindSettings::default()
        });
        assert!(sim.uses_cell_wind_field());
        assert_eq!((sim.wind_field_columns, sim.wind_field_rows), (41, 14));
    }

    #[test]
    fn cell_cached_default_gust_force_stays_within_one_point_five_percent() {
        let settings = WindSettings {
            dot_count: 50_000,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        let mut maximum_error = 0.0_f32;

        for (width, height) in [(80, 24), (120, 40), (160, 50), (240, 80), (320, 100)] {
            sim.set_mask(&OccupancyMask::empty(width, height));
            let expected = (
                usize::from(width).div_ceil(CELL_WIND_FIELD_SPACING) + 1,
                usize::from(height).div_ceil(CELL_WIND_FIELD_SPACING) + 1,
            );
            assert_eq!((sim.wind_field_columns, sim.wind_field_rows), expected);
            for time in [0.0, 1.4, 4.7] {
                let angle = sim.wind_angle(time);
                sim.update_wind_field(angle, time);
                for row in 0..usize::from(height) {
                    for column in 0..usize::from(width) {
                        let cached = sim.wind_cell_field[row * usize::from(width) + column];
                        for sub_y in [0.1, 0.5, 0.9] {
                            for sub_x in [0.1, 0.5, 0.9] {
                                let exact = sim.wind_force_at_in_bounds(
                                    column as f32 + sub_x,
                                    row as f32 + sub_y,
                                    (sim.wind_field_columns - 1) as f32 / f32::from(width),
                                    (sim.wind_field_rows - 1) as f32 / f32::from(height),
                                );
                                maximum_error = maximum_error
                                    .max((exact.0 - cached[0]).hypot(exact.1 - cached[1]));
                            }
                        }
                    }
                }
            }
        }

        assert!(
            maximum_error <= WIND_FORCE * 0.015,
            "cell cached force error {:.3}% exceeds 1.5% of base wind force",
            maximum_error / WIND_FORCE * 100.0
        );
    }

    #[test]
    // Local force error is not an accumulated path-error bound; particle
    // interpolation research recommends evaluating trajectory drift too:
    // https://doi.org/10.1103/PhysRevE.87.043307
    fn coarser_default_field_keeps_long_run_trajectory_drift_bounded() {
        const SAMPLE_DOTS: usize = 5_000;
        const FRAMES: usize = 300;
        const FPS: f32 = 30.0;
        let settings = WindSettings {
            dot_count: 20_000,
            ..WindSettings::default()
        };
        let mut coarse = Sim::new(&settings);
        let mut fine = Sim::new(&settings);
        let empty = OccupancyMask::empty(160, 50);
        coarse.set_mask(&empty);
        fine.set_mask(&empty);
        coarse.dots.truncate(SAMPLE_DOTS);
        coarse.inverse_masses.truncate(SAMPLE_DOTS);
        fine.dots.truncate(SAMPLE_DOTS);
        fine.inverse_masses.truncate(SAMPLE_DOTS);

        fine.wind_field_columns = wind_field_dimension(160, WIND_FIELD_MAX_SPACING);
        fine.wind_field_rows = wind_field_dimension(50, WIND_FIELD_MAX_SPACING);
        fine.wind_field
            .resize(fine.wind_field_columns * fine.wind_field_rows, (0.0, 0.0));
        assert_eq!(coarse.dots, fine.dots);

        for frame in 1..=FRAMES {
            let time = frame as f32 / FPS;
            coarse.advance(time);
            fine.advance(time);
        }

        let (width, height) = (160.0_f32, 50.0_f32 * ASPECT);
        let mut shifts = coarse
            .dots
            .iter()
            .zip(&fine.dots)
            .map(|(coarse_dot, fine_dot)| {
                let dx = (coarse_dot.x - fine_dot.x).abs().rem_euclid(width);
                let dy = (coarse_dot.y - fine_dot.y).abs().rem_euclid(height);
                dx.min(width - dx).hypot(dy.min(height - dy))
            })
            .collect::<Vec<_>>();
        shifts.sort_by(f32::total_cmp);
        let p99 = shifts[(shifts.len() - 1) * 99 / 100];
        let maximum = shifts[shifts.len() - 1];
        assert!(
            p99 <= 0.75,
            "p99 trajectory drift {p99:.3} cells exceeds 0.75"
        );
        assert!(
            maximum <= 8.0,
            "maximum trajectory drift {maximum:.3} cells exceeds 8"
        );
    }
}

#[cfg(test)]
mod inverse_mass_cache_tests {
    use super::super::settings::WindSettings;
    use super::Sim;

    #[test]
    fn inverse_masses_follow_population_and_weight_settings() {
        let initial = WindSettings {
            dot_count: 8,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&initial);
        assert_eq!(sim.inverse_masses.len(), sim.dots.len());

        let expanded = WindSettings {
            dot_count: 12,
            dot_weight: 70,
            weight_variation: 100,
            ..initial.clone()
        };
        sim.reconfigure(&expanded);
        assert_eq!(sim.inverse_masses.len(), sim.dots.len());
        for (dot, inverse_mass) in sim.dots.iter().zip(&sim.inverse_masses) {
            assert_eq!(*inverse_mass, 1.0 / sim.mass(dot));
        }

        let reduced = WindSettings {
            dot_count: 4,
            dot_weight: 1,
            weight_variation: 100,
            ..expanded
        };
        sim.reconfigure(&reduced);
        assert_eq!(sim.inverse_masses.len(), sim.dots.len());
        for (dot, inverse_mass) in sim.dots.iter().zip(&sim.inverse_masses) {
            assert_eq!(*inverse_mass, 1.0 / sim.mass(dot));
        }
    }
}

#[cfg(test)]
mod collision_mask_cache_tests {
    use super::super::settings::{EdgeMode, WindSettings};
    use super::{Dot, Sim};
    use crate::scene::OccupancyMask;

    #[test]
    fn render_advance_reads_simd_positions_without_flushing_the_dot_mirror() {
        let settings = WindSettings {
            dot_count: 20_000,
            gusts: 100,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&OccupancyMask::empty(160, 50));
        sim.advance_for_render(0.0);
        sim.advance_for_render(1.0 / 30.0);

        let positions = sim.render_positions().collect::<Vec<_>>();
        assert_eq!(positions.len(), settings.dot_count as usize);
        if sim.simd_state.loaded {
            assert!(sim.simd_state.dirty);
            sim.dots[0].x = f32::NAN;
            assert_eq!(sim.render_positions().next(), Some(positions[0]));
        }
        sim.flush_simd_state();
        assert_eq!(
            positions,
            sim.dots
                .iter()
                .map(|dot| (dot.x, dot.y))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn cached_collision_mask_preserves_wrap_and_bounce_boundaries() {
        let mask = OccupancyMask::from_fn(3, 2, |column, row| {
            (column, row) == (0, 0) || (column, row) == (2, 1)
        });

        for edge_mode in [EdgeMode::Wrap, EdgeMode::Bounce] {
            let settings = WindSettings {
                dot_count: 10,
                edge_mode,
                ..WindSettings::default()
            };
            let mut sim = Sim::new(&settings);
            sim.set_mask(&mask);

            assert!(sim.is_blocked(0, 0));
            assert!(!sim.is_blocked(1, 0));
            assert!(sim.is_blocked(2, 1));
            if edge_mode == EdgeMode::Wrap {
                assert!(!sim.is_blocked(-1, 0));
                assert!(sim.is_blocked(-1, -1));
                assert!(sim.is_blocked(3, 2));
            } else {
                assert!(sim.is_blocked(-1, 0));
                assert!(sim.is_blocked(3, 2));
            }

            let updated = OccupancyMask::from_fn(3, 2, |column, row| (column, row) == (1, 0));
            sim.set_mask(&updated);
            assert!(!sim.is_blocked(0, 0));
            assert!(sim.is_blocked(1, 0));
            assert!(!sim.is_blocked(2, 1));
        }
    }

    #[test]
    fn specialized_gusted_integrator_preserves_wrap_and_bounce_edges() {
        let step_at_right_edge = |edge_mode| {
            let settings = WindSettings {
                dot_count: 1,
                wind_strength: 0,
                gusts: 100,
                gravity_enabled: false,
                edge_mode,
                ..WindSettings::default()
            };
            let mut sim = Sim::new(&settings);
            sim.set_mask(&OccupancyMask::empty(8, 4));
            sim.dots[0] = Dot {
                x: 7.9,
                y: 2.0,
                vx: 45.0,
                vy: 0.0,
                weight_roll: 0.0,
            };
            sim.step(0.05, 0.05);
            sim.dots[0]
        };

        let wrapped = step_at_right_edge(EdgeMode::Wrap);
        assert!((0.0..8.0).contains(&wrapped.x));
        assert!(wrapped.x < 7.9, "dot should re-enter from the left edge");

        let bounced = step_at_right_edge(EdgeMode::Bounce);
        assert_eq!(bounced.x, 7.9);
        assert!(bounced.vx < 0.0, "dot should reverse at the right edge");
    }

    #[test]
    fn scalar_gusted_integrator_bounces_before_crossing_an_occupied_cell() {
        let settings = WindSettings {
            dot_count: 1,
            wind_strength: 0,
            gusts: 100,
            gravity_enabled: false,
            edge_mode: EdgeMode::Wrap,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        let mask = OccupancyMask::from_fn(8, 4, |column, row| (column, row) == (2, 1));
        sim.set_mask(&mask);
        sim.dots[0] = Dot {
            x: 1.9,
            y: 1.5,
            vx: 45.0,
            vy: 0.0,
            weight_roll: 0.0,
        };

        sim.integrate_gusted::<true, true>(1.0 / 30.0, 0.0, 0.0, 0.5, 1.0, 1.0);

        assert_eq!(sim.dots[0].x, 1.9, "the dot must not pass through cell 2");
        assert!(
            sim.dots[0].vx < 0.0,
            "the dot must bounce from the occupied cell"
        );
    }

    #[test]
    fn rounded_negative_wrap_never_returns_the_screen_extent() {
        for (width, height) in [(160.0, 50.0), (137.0, 43.0), (1.0, 1.0)] {
            for negative in [-f32::from_bits(1), -2.0_f32.powi(-30)] {
                let mut dot = Dot {
                    x: negative,
                    y: negative,
                    vx: 0.0,
                    vy: 0.0,
                    weight_roll: 0.0,
                };
                Sim::wrap_position(&mut dot, width, height);
                assert!(
                    (0.0..width).contains(&dot.x),
                    "wrapped x={:?} must be inside [0, {width}) for input {negative:?}",
                    dot.x
                );
                assert!(
                    (0.0..height).contains(&dot.y),
                    "wrapped y={:?} must be inside [0, {height}) for input {negative:?}",
                    dot.y
                );
            }
        }
    }

    #[test]
    fn dense_wrapped_gust_step_uses_runtime_simd_when_available() {
        let settings = WindSettings {
            dot_count: 20_000,
            diffusion: 0,
            dispersion: 0,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&OccupancyMask::empty(160, 50));
        sim.step(1.0 / 60.0, 1.0 / 60.0);

        #[cfg(target_arch = "x86_64")]
        let simd_available =
            std::is_x86_feature_detected!("avx2") || std::is_x86_feature_detected!("avx512f");
        #[cfg(not(target_arch = "x86_64"))]
        let simd_available = false;

        assert_eq!(
            sim.last_simd_path.is_some(),
            simd_available,
            "dense wrapped gust integration must dispatch to supported SIMD or retain scalar fallback"
        );
    }

    #[test]
    fn pointer_mode_keeps_persistent_soa_and_runtime_simd_active() {
        let settings = WindSettings {
            dot_count: 20_000,
            diffusion: 0,
            dispersion: 0,
            ..WindSettings::default()
        };
        let mut retained = Sim::new(&settings);
        let mut reloaded = Sim::new(&settings);
        let mask = OccupancyMask::empty(160, 50);
        retained.set_mask(&mask);
        reloaded.set_mask(&mask);
        for sim in [&mut retained, &mut reloaded] {
            sim.advance_for_render(0.0);
            sim.advance_for_render(1.0 / 60.0);
        }

        #[cfg(target_arch = "x86_64")]
        let simd_available =
            std::is_x86_feature_detected!("avx2") || std::is_x86_feature_detected!("avx512f");
        #[cfg(not(target_arch = "x86_64"))]
        let simd_available = false;
        assert_eq!(retained.simd_state.loaded, simd_available);
        if simd_available {
            assert!(retained.simd_state.loaded);
        }

        for (index, pointer) in [Some([0.5, 0.5]), Some([0.2, 0.7]), None]
            .into_iter()
            .enumerate()
        {
            retained.set_pointer(pointer);
            reloaded.set_pointer(pointer);
            reloaded.invalidate_simd_state();
            assert_eq!(retained.simd_state.loaded, simd_available);
            let time = (index as f32 + 2.0) / 60.0;
            retained.advance_for_render(time);
            reloaded.advance_for_render(time);
            assert_eq!(
                retained.render_positions().collect::<Vec<_>>(),
                reloaded.render_positions().collect::<Vec<_>>(),
                "pointer transition {index} diverged"
            );
            assert_eq!(retained.last_simd_path.is_some(), simd_available);
        }
    }

    #[test]
    fn occupancy_changes_refresh_the_persistent_simd_collision_mask() {
        let settings = WindSettings {
            dot_count: 20_000,
            diffusion: 0,
            dispersion: 0,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        let empty = OccupancyMask::empty(160, 50);
        sim.set_mask(&empty);
        sim.step(1.0 / 60.0, 1.0 / 60.0);

        #[cfg(target_arch = "x86_64")]
        let simd_available =
            std::is_x86_feature_detected!("avx2") || std::is_x86_feature_detected!("avx512f");
        #[cfg(not(target_arch = "x86_64"))]
        let simd_available = false;
        assert_eq!(sim.simd_state.loaded, simd_available);
        if !simd_available {
            return;
        }
        assert_eq!(sim.simd_state.blocked[0], 0);

        let occupied = OccupancyMask::from_fn(160, 50, |column, row| (column, row) == (0, 0));
        sim.set_mask(&occupied);
        assert!(!sim.simd_state.loaded);
        sim.step(1.0 / 60.0, 2.0 / 60.0);
        assert!(sim.simd_state.loaded);
        assert_eq!(sim.simd_state.blocked[0], 1);

        sim.set_mask(&empty);
        assert!(!sim.simd_state.loaded);
        sim.step(1.0 / 60.0, 3.0 / 60.0);
        assert!(sim.simd_state.loaded);
        assert_eq!(sim.simd_state.blocked[0], 0);
    }

    #[test]
    fn dispersion_keeps_the_scalar_dot_state_authoritative() {
        let settings = WindSettings {
            dot_count: 20_000,
            dispersion: 100,
            diffusion: 0,
            ..WindSettings::default()
        };
        let mut sim = Sim::new(&settings);
        sim.set_mask(&OccupancyMask::empty(160, 50));
        sim.step(1.0 / 60.0, 1.0 / 60.0);

        assert!(sim.last_simd_path.is_none());
        assert!(!sim.simd_state.loaded);
    }
}
