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
/// Wind is advected with the semi-implicit velocity update below. A 20 FPS
/// frame is therefore a stable visual integration interval; replaying three
/// 1/60-second substeps only repeats the same force evaluation. Longer stalls
/// remain bounded by `MAX_STEPS` rather than trying to catch up indefinitely.
/// This follows the stable large-step direction described by Stam's *Stable
/// Fluids*: https://graphics.stanford.edu/courses/cs468-05-fall/Papers/p121-stam.pdf
const MAX_STEP: f32 = 1.0 / 20.0;
/// Most steps one render may run; longer gaps are dropped, not replayed.
const MAX_STEPS: usize = 15;
/// Top speed in cell widths per second. At `MAX_STEP` this stays below one cell per step.
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
const MOUSE_RADIUS: f32 = 6.0;
const MOUSE_FORCE: f32 = 180.0;
/// Reach of the dot-to-dot repulsion, measured in cell widths.
const DIFFUSION_RADIUS: f32 = 3.0;
/// Repulsion acceleration between two touching dots at 100% diffusion.
const DIFFUSION_FORCE: f32 = 60.0;
/// Most neighbours that push one dot per step; bounds the cost in dense piles.
const DIFFUSION_MAX_NEIGHBOURS: usize = 24;
/// Teleported dots per second at 100% dispersion.
const DISPERSION_RATE: f32 = 100.0;
/// How far a displaced dot looks for an empty cell before it respawns.
const EJECT_RADIUS: i32 = 12;
/// Cells walked along a push direction when moving a dot out of new text.
const EJECT_LINE: i32 = 4;
/// Resolution of the Eulerian wind field used for spatial gust sampling.
///
/// The field is deliberately much smaller than a terminal viewport. Gusts are
/// a smooth visual force, so bilinear sampling preserves their appearance
/// while replacing per-particle trigonometric work with a bounded grid pass.
/// This applies the grid-based spatial organization used for particle
/// simulation in Groß et al., *Fast and Efficient Nearest Neighbor Search for
/// Particle Simulations*: https://diglib.eg.org/items/22911d76-8dbe-4d98-b082-83c90f529c14
/// and the coarse-grid/support-neighbour approach surveyed in *SPH Techniques
/// for the Physics Based Simulation of Fluids and Solids*:
/// https://diglib.eg.org/bitstream/handle/10.2312/egt20191035/001-041.pdf
const WIND_FIELD_COLUMNS: usize = 16;
const WIND_FIELD_ROWS: usize = 8;

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
    mask: OccupancyMask,
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
    repulsion: Vec<(f32, f32)>,
    repulsion_active: bool,
    wind_field: Vec<(f32, f32)>,
}

impl Sim {
    pub fn new(settings: &WindSettings) -> Self {
        let mut sim = Self {
            settings: settings.clone(),
            rng: Rng::new(u64::from(settings.seed) + 1),
            dots: Vec::new(),
            mask: OccupancyMask::default(),
            has_mask: false,
            mask_has_occupied: false,
            last_time: None,
            pointer: None,
            dispersion_owed: 0.0,
            bucket_starts: Vec::new(),
            bucket_fill: Vec::new(),
            bucket_items: Vec::new(),
            dot_buckets: Vec::new(),
            repulsion: Vec::new(),
            repulsion_active: false,
            wind_field: vec![(0.0, 0.0); WIND_FIELD_COLUMNS * WIND_FIELD_ROWS],
        };
        sim.resize_population();
        sim
    }

    pub fn dots(&self) -> &[Dot] {
        &self.dots
    }

    pub fn set_pointer(&mut self, position: Option<[f32; 2]>) {
        self.pointer = position.filter(|position| {
            position
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        });
    }

    fn mouse_force_at(dot: &Dot, pointer: [f32; 2], width: f32, height: f32) -> (f32, f32) {
        let dx = dot.x - pointer[0] * width;
        let dy = (dot.y - pointer[1] * height) * ASPECT;
        let distance = dx.hypot(dy);
        if distance >= MOUSE_RADIUS {
            return (0.0, 0.0);
        }
        // A dot exactly under the pointer needs a finite, deterministic direction.
        let (ux, uy) = if distance > 0.0001 {
            (dx / distance, dy / distance)
        } else {
            let angle = dot.weight_roll * std::f32::consts::PI;
            (angle.cos(), angle.sin())
        };
        let falloff = 1.0 - distance / MOUSE_RADIUS;
        let force = MOUSE_FORCE * falloff * falloff;
        (ux * force, uy * force)
    }

    /// Test helper: stops every dot at one place.
    #[cfg(test)]
    pub fn park_all_for_test(&mut self, x: f32, y: f32) {
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
        let reseed = settings.seed != self.settings.seed;
        self.settings = settings.clone();
        if reseed {
            self.rng = Rng::new(u64::from(settings.seed) + 1);
            self.dots.clear();
        }
        self.resize_population();
        if self.has_mask {
            for index in 0..self.dots.len() {
                self.respawn_if_blocked(index);
            }
        }
    }

    fn resize_population(&mut self) {
        let wanted = self.settings.dot_count as usize;
        self.dots.truncate(wanted);
        while self.dots.len() < wanted {
            let dot = self.new_dot();
            self.dots.push(dot);
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
        for row in 0..WIND_FIELD_ROWS {
            let y = row as f32 * height / (WIND_FIELD_ROWS - 1) as f32;
            for column in 0..WIND_FIELD_COLUMNS {
                let x = column as f32 * width / (WIND_FIELD_COLUMNS - 1) as f32;
                self.wind_field[row * WIND_FIELD_COLUMNS + column] =
                    self.wind_at(angle, x, y, time);
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
            (WIND_FIELD_COLUMNS - 1) as f32 / width,
            (WIND_FIELD_ROWS - 1) as f32 / height,
        )
    }

    /// Samples the cached field when the caller already has screen-to-grid
    /// scales. The per-particle loop uses this form to avoid repeating two
    /// divisions for every dot. This follows the hybrid layout and optimized
    /// interpolation direction in Hentschel et al., *Packet-Oriented
    /// Streamline Tracing on Modern SIMD Architectures*:
    /// https://diglib.eg.org/items/1b468676-83d7-4097-98ce-19b4da41f4ce
    pub(super) fn wind_force_at_scaled(
        &self,
        x: f32,
        y: f32,
        scale_x: f32,
        scale_y: f32,
    ) -> (f32, f32) {
        let grid_x = (x * scale_x).clamp(0.0, (WIND_FIELD_COLUMNS - 1) as f32);
        let grid_y = (y * scale_y).clamp(0.0, (WIND_FIELD_ROWS - 1) as f32);
        let left = grid_x.floor() as usize;
        let top = grid_y.floor() as usize;
        let right = (left + 1).min(WIND_FIELD_COLUMNS - 1);
        let bottom = (top + 1).min(WIND_FIELD_ROWS - 1);
        let tx = grid_x - left as f32;
        let ty = grid_y - top as f32;
        let top_left = self.wind_field[top * WIND_FIELD_COLUMNS + left];
        let top_right = self.wind_field[top * WIND_FIELD_COLUMNS + right];
        let bottom_left = self.wind_field[bottom * WIND_FIELD_COLUMNS + left];
        let bottom_right = self.wind_field[bottom * WIND_FIELD_COLUMNS + right];
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
            left < WIND_FIELD_COLUMNS - 1 && top < WIND_FIELD_ROWS - 1,
            "wind sampler requires an in-bounds position"
        );
        let tx = grid_x - left as f32;
        let ty = grid_y - top as f32;
        let top_left = self.wind_field[top * WIND_FIELD_COLUMNS + left];
        let top_right = self.wind_field[top * WIND_FIELD_COLUMNS + left + 1];
        let bottom_left = self.wind_field[(top + 1) * WIND_FIELD_COLUMNS + left];
        let bottom_right = self.wind_field[(top + 1) * WIND_FIELD_COLUMNS + left + 1];
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
                if (0..width).contains(&column) && (0..height).contains(&row) {
                    self.mask.is_occupied(column, row)
                } else {
                    self.mask
                        .is_occupied(column.rem_euclid(width), row.rem_euclid(height))
                }
            }
            _ => self.mask.is_occupied(column, row),
        }
    }

    fn wrap(&self, dot: &mut Dot) {
        if self.settings.edge_mode != EdgeMode::Wrap {
            return;
        }
        let (width, height) = (f32::from(self.mask.width()), f32::from(self.mask.height()));
        if width > 0.0 && !(0.0..width).contains(&dot.x) {
            dot.x = dot.x.rem_euclid(width);
        }
        if height > 0.0 && !(0.0..height).contains(&dot.y) {
            dot.y = dot.y.rem_euclid(height);
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
        self.has_mask = true;
        self.mask_has_occupied = (0..mask.height()).any(|row| {
            (0..mask.width()).any(|column| mask.is_occupied(i32::from(column), i32::from(row)))
        });
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
    }

    /// Fills `self.repulsion` with the push each dot gets from its close
    /// neighbours. Dots are binned into square buckets of `DIFFUSION_RADIUS`
    /// (rows scaled by `ASPECT` so buckets look square on screen), so only the
    /// 3x3 buckets around a dot are searched.
    fn compute_repulsion(&mut self) {
        let count = self.dots.len();
        let strength = self.settings.diffusion as f32 / 100.0 * DIFFUSION_FORCE;
        if strength <= 0.0 || count < 2 {
            if self.repulsion.len() != count {
                self.repulsion.resize(count, (0.0, 0.0));
            }
            if self.repulsion_active {
                self.repulsion.fill((0.0, 0.0));
                self.repulsion_active = false;
            }
            return;
        }
        self.repulsion.clear();
        self.repulsion.resize(count, (0.0, 0.0));
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
        starts.clear();
        starts.resize(columns * rows + 1, 0);
        dot_buckets.clear();
        dot_buckets.resize(count, 0);
        for (index, dot) in self.dots.iter().enumerate() {
            let bucket = bucket_of(dot);
            dot_buckets[index] = bucket as u32;
            starts[bucket + 1] += 1;
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
        for index in 0..count {
            let dot = self.dots[index];
            let bucket = dot_buckets[index] as usize;
            let (column, row) = (bucket % columns, bucket / columns);
            let (mut push_x, mut push_y) = (0.0_f32, 0.0_f32);
            let mut seen = 0;
            'search: for neighbour_row in row.saturating_sub(1)..=(row + 1).min(rows - 1) {
                for neighbour_column in column.saturating_sub(1)..=(column + 1).min(columns - 1) {
                    let bucket = neighbour_row * columns + neighbour_column;
                    for slot in starts[bucket]..starts[bucket + 1] {
                        let other_index = items[slot as usize] as usize;
                        if other_index == index {
                            continue;
                        }
                        let other = self.dots[other_index];
                        let dx = dot.x - other.x;
                        let dy = (dot.y - other.y) * ASPECT;
                        let distance = dx.hypot(dy);
                        if distance >= DIFFUSION_RADIUS {
                            continue;
                        }
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

    /// Fast path for the common visual workload: gusts are cached, and no
    /// pointer or diffusion force is active. Selecting the collision mode once
    /// per frame removes three option/feature branches from every dot.
    fn integrate_gusted<const COLLISIONS: bool>(
        &mut self,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        mass_base: f32,
        mass_spread: f32,
        scale_x: f32,
        scale_y: f32,
    ) {
        for index in 0..self.dots.len() {
            let mut dot = self.dots[index];
            let mass = (mass_base * (1.0 + dot.weight_roll * mass_spread)).max(0.05);
            let inverse_mass = 1.0 / mass;
            let (wind_x, wind_y) = self.wind_force_at_in_bounds(dot.x, dot.y, scale_x, scale_y);
            let ax = (wind_x - drag * dot.vx) * inverse_mass;
            let ay = (wind_y + mass * gravity - drag * dot.vy) * inverse_mass;
            dot.vx = (dot.vx + ax * dt).clamp(-MAX_SPEED, MAX_SPEED);
            dot.vy = (dot.vy + ay * dt).clamp(-MAX_SPEED, MAX_SPEED);
            let next_x = dot.x + dot.vx * dt;
            let next_y = dot.y + dot.vy * dt / ASPECT;
            if COLLISIONS {
                let (column, row) = Self::cell_of(&dot);
                let next_column = next_x.floor() as i32;
                if next_column != column && self.is_blocked(next_column, row) {
                    dot.vx = -dot.vx * bounce;
                } else {
                    dot.x = next_x;
                }
                let column_after = Self::cell_of(&dot).0;
                let next_row = next_y.floor() as i32;
                if next_row != row && self.is_blocked(column_after, next_row) {
                    dot.vy = -dot.vy * bounce;
                } else {
                    dot.y = next_y;
                }
            } else {
                dot.x = next_x;
                dot.y = next_y;
            }
            self.wrap(&mut dot);
            self.dots[index] = dot;
            if self.dots[index].x.is_nan() || self.dots[index].y.is_nan() {
                self.dots[index].x = 0.0;
                self.dots[index].y = 0.0;
                self.respawn_if_blocked(index);
            }
        }
    }

    fn step(&mut self, dt: f32, time: f32) {
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
        let mass_base = self.settings.dot_weight as f32 / 20.0;
        let mass_spread = self.settings.weight_variation as f32 / 100.0 * 0.8;
        let collisions = self.mask_has_occupied || self.settings.edge_mode != EdgeMode::Wrap;
        let uniform_wind = (!gusts_enabled).then(|| {
            let strength = self.settings.wind_strength as f32 / 100.0 * WIND_FORCE;
            (angle.cos() * strength, angle.sin() * strength)
        });
        let wind_scale = gusts_enabled.then(|| {
            (
                (WIND_FIELD_COLUMNS - 1) as f32 / f32::from(self.mask.width().max(1)),
                (WIND_FIELD_ROWS - 1) as f32 / f32::from(self.mask.height().max(1)),
            )
        });
        let pointer = self.pointer.filter(|_| self.settings.mouse_force);
        let mask_width = f32::from(self.mask.width());
        let mask_height = f32::from(self.mask.height());
        let repulsion_active = self.repulsion_active;
        if gusts_enabled && pointer.is_none() && !repulsion_active {
            let (scale_x, scale_y) = wind_scale.expect("gusted wind must have cached scales");
            if collisions {
                self.integrate_gusted::<true>(
                    dt,
                    drag,
                    gravity,
                    bounce,
                    mass_base,
                    mass_spread,
                    scale_x,
                    scale_y,
                );
            } else {
                self.integrate_gusted::<false>(
                    dt,
                    drag,
                    gravity,
                    bounce,
                    mass_base,
                    mass_spread,
                    scale_x,
                    scale_y,
                );
            }
            return;
        }
        for index in 0..self.dots.len() {
            let mut dot = self.dots[index];
            let mass = (mass_base * (1.0 + dot.weight_roll * mass_spread)).max(0.05);
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
            // Reuse one reciprocal for both force components. This keeps the
            // equation unchanged while avoiding a second per-dot division.
            let inverse_mass = 1.0 / mass;
            let ax = (wind_x + mouse_x - drag * dot.vx) * inverse_mass + spread_x;
            let ay = (wind_y + mouse_y + mass * gravity - drag * dot.vy) * inverse_mass + spread_y;
            dot.vx = (dot.vx + ax * dt).clamp(-MAX_SPEED, MAX_SPEED);
            dot.vy = (dot.vy + ay * dt).clamp(-MAX_SPEED, MAX_SPEED);
            let next_x = dot.x + dot.vx * dt;
            let next_y = dot.y + dot.vy * dt / ASPECT;
            if collisions {
                let (column, row) = Self::cell_of(&dot);
                let next_column = next_x.floor() as i32;
                if next_column != column && self.is_blocked(next_column, row) {
                    dot.vx = -dot.vx * bounce;
                } else {
                    dot.x = next_x;
                }
                let column_after = Self::cell_of(&dot).0;
                let next_row = next_y.floor() as i32;
                if next_row != row && self.is_blocked(column_after, next_row) {
                    dot.vy = -dot.vy * bounce;
                } else {
                    dot.y = next_y;
                }
            } else {
                dot.x = next_x;
                dot.y = next_y;
            }
            self.wrap(&mut dot);
            self.dots[index] = dot;
            if self.dots[index].x.is_nan() || self.dots[index].y.is_nan() {
                self.dots[index].x = 0.0;
                self.dots[index].y = 0.0;
                self.respawn_if_blocked(index);
            }
        }
    }
}
