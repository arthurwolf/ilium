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
const MAX_STEP: f32 = 1.0 / 60.0;
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
/// How far a displaced dot looks for an empty cell before it respawns.
const EJECT_RADIUS: i32 = 12;
/// Cells walked along a push direction when moving a dot out of new text.
const EJECT_LINE: i32 = 4;

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
    last_time: Option<f32>,
}

impl Sim {
    pub fn new(settings: &WindSettings) -> Self {
        let mut sim = Self {
            settings: settings.clone(),
            rng: Rng::new(u64::from(settings.seed) + 1),
            dots: Vec::new(),
            mask: OccupancyMask::default(),
            has_mask: false,
            last_time: None,
        };
        sim.resize_population();
        sim
    }

    pub fn dots(&self) -> &[Dot] {
        &self.dots
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

    fn is_blocked(&self, column: i32, row: i32) -> bool {
        let (width, height) = (i32::from(self.mask.width()), i32::from(self.mask.height()));
        match self.settings.edge_mode {
            EdgeMode::Wrap if width > 0 && height > 0 => self
                .mask
                .is_occupied(column.rem_euclid(width), row.rem_euclid(height)),
            _ => self.mask.is_occupied(column, row),
        }
    }

    fn wrap(&self, dot: &mut Dot) {
        if self.settings.edge_mode != EdgeMode::Wrap {
            return;
        }
        let (width, height) = (f32::from(self.mask.width()), f32::from(self.mask.height()));
        if width > 0.0 {
            dot.x = dot.x.rem_euclid(width);
        }
        if height > 0.0 {
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

    fn step(&mut self, dt: f32, time: f32) {
        let angle = self.wind_angle(time);
        let drag = self.settings.drag as f32 / 100.0 * DRAG_COEFFICIENT;
        let gravity = if self.settings.gravity_enabled {
            self.settings.gravity_strength as f32 / 100.0 * GRAVITY
        } else {
            0.0
        };
        let bounce = self.settings.bounce as f32 / 100.0;
        for index in 0..self.dots.len() {
            let mut dot = self.dots[index];
            let mass = self.mass(&dot);
            let (wind_x, wind_y) = self.wind_at(angle, dot.x, dot.y, time);
            let ax = (wind_x - drag * dot.vx) / mass;
            let ay = (wind_y + mass * gravity - drag * dot.vy) / mass;
            dot.vx = (dot.vx + ax * dt).clamp(-MAX_SPEED, MAX_SPEED);
            dot.vy = (dot.vy + ay * dt).clamp(-MAX_SPEED, MAX_SPEED);
            let next_x = dot.x + dot.vx * dt;
            let next_y = dot.y + dot.vy * dt / ASPECT;
            let (_, row) = Self::cell_of(&dot);
            if self.is_blocked(next_x.floor() as i32, row) {
                dot.vx = -dot.vx * bounce;
            } else {
                dot.x = next_x;
            }
            let (column_after, _) = Self::cell_of(&dot);
            if self.is_blocked(column_after, next_y.floor() as i32) {
                dot.vy = -dot.vy * bounce;
            } else {
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
