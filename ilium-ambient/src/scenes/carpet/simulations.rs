//! Bounded, deterministic hidden-body simulations in normalized ground coordinates.
//!
//! Time steps are capped after suspension rather than replaying an unbounded backlog.
//! Coordinates do not depend on the terminal dimensions, so resize preserves state.

use super::model::{Body, Mode};
use std::f32::consts::TAU;

/// Tunables are sanitized at the simulation boundary, including non-finite values.
#[derive(Clone, Debug)]
pub struct SimulationOptions {
    pub hunters_count: u32,
    pub hunters_speed: f32,
    pub hunters_separation: f32,
    pub snake_grid: u32,
    pub snake_step_seconds: f64,
    pub snake_initial_length: u32,
    pub snake_food_count: u32,
    pub life_grid: u32,
    pub life_generation_seconds: f64,
    pub life_density: f32,
    pub life_wrap: bool,
    pub dvd_speed: f32,
    pub orbit_speed: f32,
    pub orbit_scale: f32,
    pub clock_seconds: bool,
    pub clock_24h: bool,
    pub clock_tubes: bool,
    pub radius: f32,
    pub height: f32,
    pub easing_seconds: f64,
}

impl Default for SimulationOptions {
    fn default() -> Self {
        Self {
            hunters_count: 7,
            hunters_speed: 0.22,
            hunters_separation: 0.08,
            snake_grid: 12,
            snake_step_seconds: 0.22,
            snake_initial_length: 5,
            snake_food_count: 1,
            life_grid: 18,
            life_generation_seconds: 1.5,
            life_density: 0.28,
            life_wrap: true,
            dvd_speed: 0.16,
            orbit_speed: 0.35,
            orbit_scale: 1.0,
            clock_seconds: true,
            clock_24h: true,
            clock_tubes: true,
            radius: 0.028,
            height: 0.10,
            easing_seconds: 0.18,
        }
    }
}

fn finite(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn finite64(value: f64, fallback: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn ease(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn lerp(a: [f32; 2], b: [f32; 2], factor: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * factor, a[1] + (b[1] - a[1]) * factor]
}

fn point(bodies: &mut Vec<Body>, position: [f32; 2], radius: f32, height: f32) {
    bodies.push(Body {
        from: position,
        to: position,
        radius,
        height,
    });
}

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
    fn fraction(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }
    fn index(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }
}

/// Seven non-chess modes. Chess has a separate legal-move/feed engine.
pub struct Simulations {
    seed: u64,
    active_mode: Option<Mode>,
    last_time: Option<f64>,
    hunters: Vec<Hunter>,
    hunters_accumulator: f64,
    snake: Option<SnakeState>,
    life: Option<LifeState>,
    dvd_phase: [f64; 2],
    orbit_phase: [f64; 8],
}

impl Simulations {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            active_mode: None,
            last_time: None,
            hunters: Vec::new(),
            hunters_accumulator: 0.0,
            snake: None,
            life: None,
            dvd_phase: [0.17, 0.31],
            orbit_phase: std::array::from_fn(|index| index as f64 * 0.73),
        }
    }

    /// Caller clears `bodies`; output is appended, allowing composition with chess.
    pub fn update(
        &mut self,
        mode: Mode,
        options: &SimulationOptions,
        time: f64,
        civil_seconds: f64,
        pointer: Option<[f32; 2]>,
        bodies: &mut Vec<Body>,
    ) {
        if self.active_mode != Some(mode) {
            self.active_mode = Some(mode);
            self.last_time = None;
            match mode {
                Mode::Hunters => {
                    self.hunters.clear();
                    self.hunters_accumulator = 0.0;
                }
                Mode::Snake => self.snake = None,
                Mode::Life => self.life = None,
                Mode::Dvd => self.dvd_phase = [0.17, 0.31],
                Mode::Orbits => {
                    self.orbit_phase = std::array::from_fn(|index| index as f64 * 0.73);
                }
                _ => {}
            }
        }
        let delta = match (self.last_time, time.is_finite()) {
            (Some(previous), true) if time >= previous => (time - previous).min(2.0),
            _ => 0.0,
        };
        if time.is_finite() {
            self.last_time = Some(time);
        }
        let radius = finite(options.radius, 0.028, 0.001, 0.20);
        let height = finite(options.height, 0.10, 0.0, 0.5);
        match mode {
            Mode::Hunters => self.hunters(options, delta, pointer, radius, height, bodies),
            Mode::Snake => self.snake(options, delta, radius, height, bodies),
            Mode::Life => self.life(options, delta, radius, height, bodies),
            Mode::Dvd => {
                let speed = f64::from(finite(options.dvd_speed, 0.16, 0.0, 4.0));
                self.dvd_phase[0] = (self.dvd_phase[0] + delta * speed).rem_euclid(2.0);
                self.dvd_phase[1] = (self.dvd_phase[1] + delta * speed * 0.713).rem_euclid(2.0);
                let ball_radius = (radius * 1.6).min(0.24);
                let margin = f64::from(ball_radius.max(0.04));
                let span = 1.0 - margin * 2.0;
                point(
                    bodies,
                    [
                        (margin + reflected(self.dvd_phase[0]) * span) as f32,
                        (margin + reflected(self.dvd_phase[1]) * span) as f32,
                    ],
                    ball_radius,
                    height,
                );
            }
            Mode::Orbits => {
                let speed = f64::from(finite(options.orbit_speed, 0.35, 0.0, 8.0));

                let scale = finite(options.orbit_scale, 1.0, 0.1, 1.0);
                point(bodies, [0.5, 0.5], radius * 1.5, height * 1.4);
                // Mercury through Neptune: relative periods, stylized orbital radii
                // and visible sizes, deliberately not an astronomical ephemeris.
                for (index, period) in ORBIT_PERIODS.iter().enumerate() {
                    let distance = (0.07 + index as f32 * 0.046) * scale;
                    self.orbit_phase[index] = (self.orbit_phase[index] + delta * speed / period)
                        .rem_euclid(std::f64::consts::TAU);
                    let angle = self.orbit_phase[index] as f32;
                    point(
                        bodies,
                        [0.5 + distance * angle.cos(), 0.5 + distance * angle.sin()],
                        radius * PLANET_SIZES[index],
                        height * PLANET_SIZES[index],
                    );
                }
            }
            Mode::DigitalClock => digital_clock(options, civil_seconds, radius, height, bodies),
            Mode::AnalogClock => analog_clock(options, civil_seconds, radius, height, bodies),
            Mode::AutoChess | Mode::LiveChess => {}
        }
    }

    fn hunters(
        &mut self,
        options: &SimulationOptions,
        delta: f64,
        pointer: Option<[f32; 2]>,
        radius: f32,
        height: f32,
        bodies: &mut Vec<Body>,
    ) {
        let count = options.hunters_count.clamp(1, 64) as usize;
        if self.hunters.len() != count {
            let mut rng = Rng::new(self.seed ^ 0x4855_4e54);
            self.hunters.clear();
            for _ in 0..count {
                self.hunters.push(Hunter {
                    position: [0.1 + rng.fraction() * 0.8, 0.1 + rng.fraction() * 0.8],
                    velocity: [0.0, 0.0],
                    next_velocity: [0.0, 0.0],
                });
            }
        }
        let target = pointer
            .filter(|point| point.iter().all(|v| v.is_finite()))
            .map(|point| [point[0].clamp(0.0, 1.0), point[1].clamp(0.0, 1.0)])
            .unwrap_or([0.5, 0.5]);
        let speed = finite(options.hunters_speed, 0.22, 0.0, 2.0);
        let separation = finite(options.hunters_separation, 0.08, 0.01, 0.4);
        const STEP: f64 = 1.0 / 60.0;
        self.hunters_accumulator = (self.hunters_accumulator + delta).min(STEP * 16.0);
        for _ in 0..16 {
            if self.hunters_accumulator + 1e-12 < STEP {
                break;
            }
            self.hunters_accumulator -= STEP;
            for index in 0..count {
                let hunter = &self.hunters[index];
                let mut repel = [0.0, 0.0];
                let mut align = [0.0, 0.0];
                let mut center = [0.0, 0.0];
                let mut neighbors = 0;
                for (other_index, other) in self.hunters.iter().enumerate() {
                    if index == other_index {
                        continue;
                    }
                    let diff = [
                        hunter.position[0] - other.position[0],
                        hunter.position[1] - other.position[1],
                    ];
                    let distance2 = diff[0] * diff[0] + diff[1] * diff[1];
                    if distance2 < separation * separation {
                        let scale = 1.0 / distance2.max(0.0001);
                        repel[0] += diff[0] * scale;
                        repel[1] += diff[1] * scale;
                    }
                    if distance2 < 0.04 {
                        align[0] += other.velocity[0];
                        align[1] += other.velocity[1];
                        center[0] += other.position[0];
                        center[1] += other.position[1];
                        neighbors += 1;
                    }
                }
                let pursuit = unit([
                    target[0] - hunter.position[0],
                    target[1] - hunter.position[1],
                ]);
                let repel = unit(repel);
                let mut desired = [pursuit[0] + repel[0] * 0.9, pursuit[1] + repel[1] * 0.9];
                if neighbors > 0 {
                    let neighbors = neighbors as f32;
                    let cohesion = unit([
                        center[0] / neighbors - hunter.position[0],
                        center[1] / neighbors - hunter.position[1],
                    ]);
                    let alignment = unit(align);
                    desired[0] += cohesion[0] * 0.15 + alignment[0] * 0.25;
                    desired[1] += cohesion[1] * 0.15 + alignment[1] * 0.25;
                }
                let desired = unit(desired);
                self.hunters[index].next_velocity = [
                    hunter.velocity[0] + (desired[0] * speed - hunter.velocity[0]) * 0.08,
                    hunter.velocity[1] + (desired[1] * speed - hunter.velocity[1]) * 0.08,
                ];
            }
            for hunter in &mut self.hunters {
                hunter.velocity = hunter.next_velocity;
                for axis in 0..2 {
                    hunter.position[axis] += hunter.velocity[axis] * STEP as f32;
                    let bounded = hunter.position[axis].clamp(radius, 1.0 - radius);
                    if bounded != hunter.position[axis] {
                        hunter.velocity[axis] *= -0.5;
                    }
                    hunter.position[axis] = bounded;
                }
            }
        }
        for hunter in &self.hunters {
            let heading = unit(hunter.velocity);
            let tail = [
                (hunter.position[0] - heading[0] * radius * 1.6).clamp(radius, 1.0 - radius),
                (hunter.position[1] - heading[1] * radius * 1.6).clamp(radius, 1.0 - radius),
            ];
            bodies.push(Body {
                from: hunter.position,
                to: tail,
                radius,
                height,
            });
        }
    }

    fn snake(
        &mut self,
        options: &SimulationOptions,
        delta: f64,
        radius: f32,
        height: f32,
        bodies: &mut Vec<Body>,
    ) {
        let side = (options.snake_grid.clamp(4, 32) as usize) & !1;
        let food_count = options.snake_food_count.clamp(1, 16) as usize;
        let initial_length = (options.snake_initial_length as usize).clamp(2, side * side - 1);
        if self.snake.as_ref().is_none_or(|state| {
            state.side != side
                || state.food_count != food_count
                || state.initial_length != initial_length
        }) {
            self.snake = Some(SnakeState::new(side, initial_length, food_count, self.seed));
        }
        let Some(state) = &mut self.snake else {
            return;
        };
        let interval = finite64(options.snake_step_seconds, 0.22, 0.04, 10.0);
        state.accumulator = (state.accumulator + delta).min(interval * 8.0);
        for _ in 0..8 {
            if state.accumulator + 1e-12 < interval {
                break;
            }
            state.accumulator -= interval;
            state.step();
        }
        let easing = finite64(options.easing_seconds, 0.18, 0.0, 10.0).min(interval);
        let progress = if easing <= 0.0 {
            1.0
        } else {
            ease((state.accumulator / easing) as f32)
        };
        let cell_radius = radius.min(0.38 / side as f32);
        for segment in 0..state.length {
            let position = state.segment_position(segment, progress);
            if segment + 1 < state.length {
                bodies.push(Body {
                    from: position,
                    to: state.segment_position(segment + 1, progress),
                    radius: cell_radius,
                    height,
                });
            } else {
                point(bodies, position, cell_radius, height);
            }
        }
        for food in &state.food {
            point(
                bodies,
                state.coordinate(*food),
                cell_radius * 1.25,
                height * 0.65,
            );
        }
    }

    fn life(
        &mut self,
        options: &SimulationOptions,
        delta: f64,
        radius: f32,
        height: f32,
        bodies: &mut Vec<Body>,
    ) {
        let side = options.life_grid.clamp(4, 32) as usize;
        let density = finite(options.life_density, 0.28, 0.0, 1.0);
        if self.life.as_ref().is_none_or(|state| {
            state.side != side || state.density != density || state.wrap != options.life_wrap
        }) {
            self.life = Some(LifeState::new(
                side,
                density,
                options.life_wrap,
                &mut Rng::new(self.seed ^ 0x4c49_4645),
            ));
        }
        let Some(state) = &mut self.life else {
            return;
        };
        let interval = finite64(options.life_generation_seconds, 1.5, 0.05, 60.0);
        state.accumulator = (state.accumulator + delta).min(interval * 2.0);
        for _ in 0..2 {
            if state.accumulator + 1e-12 < interval {
                break;
            }
            state.accumulator -= interval;
            state.step(options.life_wrap);
        }
        let easing = finite64(options.easing_seconds, 0.18, 0.0, 10.0).min(interval);
        let factor = if easing <= 0.0 {
            1.0
        } else {
            ease((state.accumulator / easing) as f32)
        };
        for (index, alive) in state.cells.iter().enumerate() {
            let previous = state.previous[index];
            let amplitude = (if previous { 1.0 } else { 0.0 })
                + ((if *alive { 1.0 } else { 0.0 }) - (if previous { 1.0 } else { 0.0 })) * factor;
            if amplitude > 0.0 {
                point(
                    bodies,
                    [
                        (index % side) as f32 / side as f32 + 0.5 / side as f32,
                        (index / side) as f32 / side as f32 + 0.5 / side as f32,
                    ],
                    radius.min(0.38 / side as f32),
                    height * amplitude,
                );
            }
        }
    }
}

fn unit(vector: [f32; 2]) -> [f32; 2] {
    let length = (vector[0] * vector[0] + vector[1] * vector[1]).sqrt();
    if length < 1e-7 {
        [0.0, 0.0]
    } else {
        [vector[0] / length, vector[1] / length]
    }
}

struct Hunter {
    position: [f32; 2],
    velocity: [f32; 2],
    next_velocity: [f32; 2],
}

fn reflected(phase: f64) -> f64 {
    let phase = phase.rem_euclid(2.0);
    if phase <= 1.0 {
        phase
    } else {
        2.0 - phase
    }
}

const ORBIT_PERIODS: [f64; 8] = [0.24, 0.615, 1.0, 1.881, 11.86, 29.46, 84.0, 164.8];
const PLANET_SIZES: [f32; 8] = [0.45, 0.75, 0.80, 0.6, 1.1, 0.95, 0.8, 0.8];

/// Clockwise closed Hamiltonian cycle on an even square: reserve column zero
/// for the return path, then snake vertically through every remaining column.
fn hamiltonian_cycle(side: usize) -> Vec<[usize; 2]> {
    let mut cycle = Vec::with_capacity(side * side);
    cycle.push([0, 0]);
    for x in 1..side {
        cycle.push([x, 0]);
    }
    for row in 1..side {
        if row % 2 == 1 {
            for x in (1..side).rev() {
                cycle.push([x, row]);
            }
        } else {
            for x in 1..side {
                cycle.push([x, row]);
            }
        }
    }
    for row in (1..side).rev() {
        cycle.push([0, row]);
    }
    cycle
}

struct SnakeState {
    side: usize,
    cycle: Vec<[usize; 2]>,
    head: usize,
    previous_head: usize,
    length: usize,
    previous_length: usize,
    initial_length: usize,
    food_count: usize,
    food: Vec<usize>,
    occupied: Vec<bool>,
    rng: Rng,
    accumulator: f64,
}

impl SnakeState {
    fn new(side: usize, initial_length: usize, food_count: usize, seed: u64) -> Self {
        let mut state = Self {
            side,
            cycle: hamiltonian_cycle(side),
            head: initial_length - 1,
            previous_head: initial_length - 1,
            length: initial_length,
            previous_length: initial_length,
            initial_length,
            food_count,
            food: Vec::with_capacity(food_count),
            occupied: vec![false; side * side],
            rng: Rng::new(seed ^ 0x534e_414b),
            accumulator: 0.0,
        };
        state.replenish_food();
        state
    }
    fn coordinate(&self, index: usize) -> [f32; 2] {
        let cell = self.cycle[index];
        [
            (cell[0] as f32 + 0.5) / self.side as f32,
            (cell[1] as f32 + 0.5) / self.side as f32,
        ]
    }
    fn segment_position(&self, segment: usize, factor: f32) -> [f32; 2] {
        let size = self.cycle.len();
        let current = (self.head + size - segment) % size;
        let previous = (self.previous_head + size - segment.min(self.previous_length - 1)) % size;
        lerp(self.coordinate(previous), self.coordinate(current), factor)
    }
    fn step(&mut self) {
        let size = self.cycle.len();
        // A completed game starts a new legal game without indexing past the board.
        if self.length == size {
            self.length = self.initial_length;
            self.head = self.initial_length - 1;
            self.previous_head = self.head;
            self.previous_length = self.length;
            self.food.clear();
            self.replenish_food();
            return;
        }
        self.previous_head = self.head;
        self.previous_length = self.length;
        self.head = (self.head + 1) % size;
        if let Some(index) = self.food.iter().position(|food| *food == self.head) {
            self.food.swap_remove(index);
            self.length += 1;
        }
        self.replenish_food();
    }
    fn replenish_food(&mut self) {
        self.occupied.fill(false);
        let size = self.cycle.len();
        for offset in 0..self.length {
            self.occupied[(self.head + size - offset) % size] = true;
        }
        for food in &self.food {
            self.occupied[*food] = true;
        }
        let target = self.food_count.min(size - self.length);
        while self.food.len() < target {
            // One bounded scan from a random offset finds an unoccupied cell;
            // rejection sampling could stall when the snake nearly fills the board.
            let start = self.rng.index(size);
            let Some(index) = (0..size)
                .map(|offset| (start + offset) % size)
                .find(|index| !self.occupied[*index])
            else {
                break;
            };
            self.occupied[index] = true;
            self.food.push(index);
        }
    }
}

struct LifeState {
    side: usize,
    density: f32,
    wrap: bool,
    cells: Vec<bool>,
    previous: Vec<bool>,
    scratch: Vec<bool>,
    accumulator: f64,
}

impl LifeState {
    fn new(side: usize, density: f32, wrap: bool, rng: &mut Rng) -> Self {
        let cells: Vec<_> = (0..side * side).map(|_| rng.fraction() < density).collect();
        Self {
            side,
            density,
            wrap,
            previous: cells.clone(),
            scratch: vec![false; side * side],
            cells,
            accumulator: 0.0,
        }
    }
    fn step(&mut self, wrap: bool) {
        self.previous.copy_from_slice(&self.cells);
        for y in 0..self.side {
            for x in 0..self.side {
                let mut neighbors = 0;
                for dy in -1_isize..=1 {
                    for dx in -1_isize..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as isize + dx;
                        let ny = y as isize + dy;
                        let neighbor = if wrap {
                            Some(
                                (ny.rem_euclid(self.side as isize) as usize) * self.side
                                    + nx.rem_euclid(self.side as isize) as usize,
                            )
                        } else if nx >= 0
                            && ny >= 0
                            && nx < self.side as isize
                            && ny < self.side as isize
                        {
                            Some(ny as usize * self.side + nx as usize)
                        } else {
                            None
                        };
                        if neighbor.is_some_and(|index| self.cells[index]) {
                            neighbors += 1;
                        }
                    }
                }
                let index = y * self.side + x;
                self.scratch[index] = neighbors == 3 || (self.cells[index] && neighbors == 2);
            }
        }
        std::mem::swap(&mut self.cells, &mut self.scratch);
    }
}

const DIGITS: [u16; 10] = [
    0b111_101_101_101_111,
    0b010_110_010_010_111,
    0b111_001_111_100_111,
    0b111_001_111_001_111,
    0b101_101_111_001_001,
    0b111_100_111_001_111,
    0b111_100_111_101_111,
    0b111_001_001_001_001,
    0b111_101_111_101_111,
    0b111_101_111_001_111,
];

fn clock_digits(seconds: f64, is_24h: bool) -> [usize; 6] {
    let whole = seconds.rem_euclid(86_400.0).floor() as u32;
    let mut hours = whole / 3600;
    if !is_24h {
        hours = ((hours + 11) % 12) + 1;
    }
    let minutes = (whole / 60) % 60;
    let seconds = whole % 60;
    [
        (hours / 10) as usize,
        (hours % 10) as usize,
        (minutes / 10) as usize,
        (minutes % 10) as usize,
        (seconds / 10) as usize,
        (seconds % 10) as usize,
    ]
}

fn digital_clock(
    options: &SimulationOptions,
    civil_seconds: f64,
    radius: f32,
    height: f32,
    bodies: &mut Vec<Body>,
) {
    let seconds = if civil_seconds.is_finite() {
        civil_seconds.rem_euclid(86_400.0)
    } else {
        0.0
    };
    let cadence = if options.clock_seconds { 1.0 } else { 60.0 };
    let phase = seconds.rem_euclid(cadence);
    let previous = clock_digits(seconds - phase - 1.0, options.clock_24h);
    let current = clock_digits(seconds, options.clock_24h);
    let duration = finite64(options.easing_seconds, 0.18, 0.0, 0.9);
    let factor = if duration <= 0.0 {
        1.0
    } else {
        ease((phase / duration) as f32)
    };
    let digits = if options.clock_seconds { 6 } else { 4 };
    let columns = if options.clock_seconds { 24 } else { 15 };
    let spacing = 0.78 / columns as f32;
    let pixel_radius = radius.min(spacing * 0.42);
    for digit in 0..digits {
        let origin = (digit * 4 + digit / 2) as f32;
        for row in 0..5 {
            for column in 0..3 {
                let bit = 1 << (14 - row * 3 - column);
                let was = if DIGITS[previous[digit]] & bit != 0 {
                    1.0
                } else {
                    0.0
                };
                let now = if DIGITS[current[digit]] & bit != 0 {
                    1.0
                } else {
                    0.0
                };
                let amplitude = was + (now - was) * factor;
                if amplitude > 0.0 {
                    point(
                        bodies,
                        [
                            0.11 + (origin + column as f32) * spacing,
                            0.34 + row as f32 * 0.075,
                        ],
                        pixel_radius,
                        height * amplitude,
                    );
                }
            }
        }
    }
    for column in if options.clock_seconds {
        &[7.5, 16.5][..]
    } else {
        &[7.5][..]
    } {
        for row in [1.0, 3.0] {
            point(
                bodies,
                [0.11 + column * spacing, 0.34 + row * 0.075],
                pixel_radius,
                height,
            );
        }
    }
}

fn analog_clock(
    options: &SimulationOptions,
    civil_seconds: f64,
    radius: f32,
    height: f32,
    bodies: &mut Vec<Body>,
) {
    let seconds = if civil_seconds.is_finite() {
        civil_seconds.rem_euclid(86_400.0)
    } else {
        0.0
    };
    let hands = [
        ((seconds / 43_200.0).rem_euclid(1.0), 0.22, 1.3),
        ((seconds / 3600.0).rem_euclid(1.0), 0.34, 1.0),
        ((seconds / 60.0).rem_euclid(1.0), 0.40, 0.65),
    ];
    point(bodies, [0.5, 0.5], radius * 1.2, height * 1.25);
    for (index, (turn, length, weight)) in hands.iter().enumerate() {
        if index == 2 && !options.clock_seconds {
            continue;
        }
        let angle = *turn as f32 * TAU - TAU / 4.0;
        let tip = [0.5 + length * angle.cos(), 0.5 + length * angle.sin()];
        point(bodies, tip, radius * weight, height * weight);
        if options.clock_tubes {
            bodies.push(Body {
                from: [0.5, 0.5],
                to: tip,
                radius: radius * weight * 0.4,
                height: height * weight * 0.7,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvd_reflection_handles_multiple_border_crossings() {
        assert!((reflected(3.25) - 0.75).abs() < 1e-6);
        assert!((reflected(-0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn hamiltonian_cycle_is_closed_adjacent_and_unique() {
        for side in [4, 6, 12, 32] {
            let cycle = hamiltonian_cycle(side);
            assert_eq!(cycle.len(), side * side);
            let mut sorted = cycle.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), side * side);
            for index in 0..cycle.len() {
                let a = cycle[index];
                let b = cycle[(index + 1) % cycle.len()];
                assert_eq!(a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]), 1);
            }
        }
    }

    #[test]
    fn life_blinker_uses_conway_rules() {
        let mut state = LifeState::new(5, 0.0, false, &mut Rng::new(1));
        state.cells.fill(false);
        for x in 1..=3 {
            state.cells[2 * 5 + x] = true;
        }
        state.step(false);
        let alive: Vec<_> = state
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, alive)| alive.then_some(i))
            .collect();
        assert_eq!(alive, vec![7, 12, 17]);
    }

    #[test]
    fn clock_modes_depend_only_on_civil_time() {
        for mode in [Mode::DigitalClock, Mode::AnalogClock] {
            let options = SimulationOptions::default();
            let mut a = Simulations::new(4);
            let mut b = Simulations::new(4);
            let mut first = Vec::new();
            let mut second = Vec::new();
            a.update(mode, &options, 0.0, 45296.25, None, &mut first);
            b.update(mode, &options, 999999.0, 45296.25, None, &mut second);
            assert_bodies_equal(&first, &second);
        }
    }

    fn assert_bodies_equal(a: &[Body], b: &[Body]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(a.from, b.from);
            assert_eq!(a.to, b.to);
            assert_eq!(a.height, b.height);
            assert_eq!(a.radius, b.radius);
        }
    }

    const MODES: [Mode; 7] = [
        Mode::Hunters,
        Mode::Snake,
        Mode::Life,
        Mode::Dvd,
        Mode::Orbits,
        Mode::DigitalClock,
        Mode::AnalogClock,
    ];

    #[test]
    fn every_mode_is_deterministic_and_finite_after_suspend_backward_and_bad_inputs() {
        let options = SimulationOptions {
            hunters_count: u32::MAX,
            hunters_speed: f32::NAN,
            hunters_separation: f32::INFINITY,
            snake_grid: u32::MAX,
            snake_food_count: u32::MAX,
            snake_initial_length: u32::MAX,
            snake_step_seconds: f64::NAN,
            life_grid: u32::MAX,
            life_density: f32::INFINITY,
            life_generation_seconds: f64::NAN,
            dvd_speed: f32::INFINITY,
            orbit_speed: f32::NAN,
            orbit_scale: f32::INFINITY,
            radius: f32::NAN,
            height: f32::INFINITY,
            easing_seconds: f64::NAN,
            ..SimulationOptions::default()
        };
        for mode in MODES {
            let mut a = Simulations::new(7);
            let mut b = Simulations::new(7);
            for time in [0.0, 0.016, 0.2, 1e100, -10.0, f64::NAN, 10.0] {
                let mut first = Vec::new();
                let mut second = Vec::new();
                a.update(
                    mode,
                    &options,
                    time,
                    45296.3,
                    Some([f32::NAN, f32::INFINITY]),
                    &mut first,
                );
                b.update(
                    mode,
                    &options,
                    time,
                    45296.3,
                    Some([f32::NAN, f32::INFINITY]),
                    &mut second,
                );
                assert_bodies_equal(&first, &second);
                assert!(!first.is_empty(), "{mode:?}");
                assert!(first.len() <= 1024, "{mode:?}: {}", first.len());
                for body in first {
                    assert!(body
                        .from
                        .into_iter()
                        .chain(body.to)
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)));
                    assert!(body.radius.is_finite() && body.radius > 0.0);
                    assert!(body.height.is_finite() && body.height >= 0.0);
                }
            }
        }
    }

    #[test]
    fn snake_eats_grows_and_restarts_without_ever_colliding() {
        let mut snake = SnakeState::new(4, 3, 4, 7);
        let mut grew = false;
        let mut restarted = false;
        for _ in 0..1024 {
            let before = snake.length;
            snake.step();
            grew |= snake.length > before;
            restarted |= snake.length < before;
            let size = snake.cycle.len();
            let mut cells = vec![false; size];
            for segment in 0..snake.length {
                let index = (snake.head + size - segment) % size;
                assert!(!cells[index]);
                cells[index] = true;
            }
            assert!(snake.food.iter().all(|food| !cells[*food]));
            assert_eq!(snake.food.len(), 4.min(size - snake.length));
        }
        assert!(grew && restarted);
    }

    #[test]
    fn life_torus_wraps_neighbors_and_bounded_world_does_not() {
        let mut bounded = LifeState::new(5, 0.0, false, &mut Rng::new(1));
        bounded.cells[0] = true;
        bounded.cells[4] = true;
        bounded.cells[20] = true;
        let mut wrapped = LifeState::new(5, 0.0, true, &mut Rng::new(1));
        wrapped.cells.copy_from_slice(&bounded.cells);
        bounded.step(false);
        wrapped.step(true);
        assert!(!bounded.cells[24]);
        assert!(wrapped.cells[24]);
    }

    #[test]
    fn hunters_follow_the_pointer_with_fixed_step_frame_partition_independence() {
        let options = SimulationOptions {
            hunters_count: 1,
            ..SimulationOptions::default()
        };
        let mut a = Simulations::new(9);
        let mut b = Simulations::new(9);
        let mut bodies = Vec::new();
        a.update(
            Mode::Hunters,
            &options,
            0.0,
            0.0,
            Some([0.85, 0.85]),
            &mut bodies,
        );
        b.update(
            Mode::Hunters,
            &options,
            0.0,
            0.0,
            Some([0.85, 0.85]),
            &mut Vec::new(),
        );
        let initial = a.hunters[0].position;
        let distance = |p: [f32; 2]| (p[0] - 0.85).hypot(p[1] - 0.85);
        for index in 1..=180 {
            a.update(
                Mode::Hunters,
                &options,
                index as f64 / 60.0,
                0.0,
                Some([0.85, 0.85]),
                &mut Vec::new(),
            );
        }
        for index in 1..=90 {
            b.update(
                Mode::Hunters,
                &options,
                index as f64 / 30.0,
                0.0,
                Some([0.85, 0.85]),
                &mut Vec::new(),
            );
        }
        assert_eq!(a.hunters[0].position, b.hunters[0].position);
        assert!(distance(a.hunters[0].position) < distance(initial));
    }

    #[test]
    fn default_life_is_slow_and_transition_heights_are_eased() {
        let options = SimulationOptions::default();
        let mut simulations = Simulations::new(6);
        simulations.update(Mode::Life, &options, 0.0, 0.0, None, &mut Vec::new());
        let initial = simulations.life.as_ref().unwrap().cells.clone();
        simulations.update(Mode::Life, &options, 1.0, 0.0, None, &mut Vec::new());
        assert_eq!(initial, simulations.life.as_ref().unwrap().cells);
        let mut bodies = Vec::new();
        simulations.update(Mode::Life, &options, 1.59, 0.0, None, &mut bodies);
        assert_ne!(initial, simulations.life.as_ref().unwrap().cells);
        assert!(bodies
            .iter()
            .any(|body| body.height > 0.0 && body.height < options.height));
    }

    #[test]
    fn clock_formats_and_hands_have_real_civil_geometry() {
        assert_eq!(clock_digits(0.0, true), [0, 0, 0, 0, 0, 0]);
        assert_eq!(clock_digits(0.0, false), [1, 2, 0, 0, 0, 0]);
        assert_eq!(clock_digits(86399.0, true), [2, 3, 5, 9, 5, 9]);
        assert_eq!(clock_digits(-1.0, true), [2, 3, 5, 9, 5, 9]);
        let options = SimulationOptions {
            clock_tubes: false,
            ..SimulationOptions::default()
        };
        let mut bodies = Vec::new();
        analog_clock(&options, 3.0 * 3600.0, 0.02, 0.1, &mut bodies);
        assert_eq!(bodies.len(), 4);
        assert!((bodies[1].from[0] - 0.72).abs() < 1e-6);
        assert!((bodies[1].from[1] - 0.5).abs() < 1e-6);
        assert!((bodies[2].from[1] - 0.16).abs() < 1e-6);
        let mut next = Vec::new();
        analog_clock(&options, 3.0 * 3600.0 + 15.0, 0.02, 0.1, &mut next);
        assert!((next[3].from[0] - 0.9).abs() < 1e-6);
        assert!((next[3].from[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dvd_repeated_overshoot_stays_inside_its_radius_and_orbits_include_all_planets() {
        let options = SimulationOptions {
            dvd_speed: 4.0,
            radius: 0.2,
            ..SimulationOptions::default()
        };
        let mut simulations = Simulations::new(1);
        for index in 0..100 {
            let mut bodies = Vec::new();
            simulations.update(
                Mode::Dvd,
                &options,
                index as f64 * 1.7,
                0.0,
                None,
                &mut bodies,
            );
            let body = &bodies[0];
            assert!(body
                .from
                .iter()
                .all(|coordinate| *coordinate >= body.radius - 1e-6
                    && *coordinate <= 1.0 - body.radius + 1e-6));
        }
        let mut bodies = Vec::new();
        simulations.update(Mode::Orbits, &options, 1.0, 0.0, None, &mut bodies);
        assert_eq!(bodies.len(), 9);
        assert_eq!(bodies[0].from, [0.5, 0.5]);
    }

    #[test]
    fn changing_grid_and_mode_resets_only_selected_simulation_safely() {
        let mut simulation = Simulations::new(10);
        let mut options = SimulationOptions::default();
        simulation.update(Mode::Snake, &options, 0.0, 0.0, None, &mut Vec::new());
        options.snake_grid = 7;
        simulation.update(Mode::Snake, &options, 0.1, 0.0, None, &mut Vec::new());
        assert_eq!(simulation.snake.as_ref().unwrap().side, 6);
        simulation.update(Mode::Life, &options, 0.2, 0.0, None, &mut Vec::new());
        assert!(simulation.snake.is_some());
        simulation.update(Mode::Snake, &options, 0.3, 0.0, None, &mut Vec::new());
        assert_eq!(
            simulation.snake.as_ref().unwrap().head,
            options.snake_initial_length as usize - 1
        );
    }
}
