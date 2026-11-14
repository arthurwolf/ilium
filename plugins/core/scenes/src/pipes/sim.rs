//! Pipe growth simulation on a cubic integer grid.
//!
//! The simulation is a pure function of its seed and of the tick value it is
//! advanced to. One tick is one grid cell of growth, so the wall clock never
//! enters: the scene converts animation time to ticks, and frame rate cannot
//! change what is built. Each pipe has its own phase, so pipes do not step in
//! lock step; events are processed in chronological order across pipes, which
//! keeps occupancy decisions consistent.

use super::settings::JointStyle;

/// Deterministic splitmix64 generator; no external dependency needed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut rng = Self(seed ^ 0x9e37_79b9_7f4a_7c15);
        rng.next_u64();
        rng
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    /// Uniform in 0.0..1.0.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in 0..count (count must be positive).
    pub fn below(&mut self, count: usize) -> usize {
        ((self.unit() * count as f64) as usize).min(count.saturating_sub(1))
    }
}

/// Fraction of the volume that may be occupied before growth stops.
pub const FILL_LIMIT: f64 = 0.55;

/// Brightness of successive pipes: distinct enough to tell neighbours apart
/// once dithered.
const ALBEDOS: [f32; 8] = [1.0, 0.66, 0.86, 0.52, 0.94, 0.74, 0.6, 0.8];

/// The two axes perpendicular to `axis`, ascending.
pub fn perpendicular(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SimParams {
    pub grid: i32,
    pub pipe_count: usize,
    /// 0.0..=1.0
    pub turn_chance: f64,
    /// Pipe radius in grid units.
    pub radius: f32,
    pub joint: JointStyle,
}

/// A finished straight run between two adjacent grid points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub axis: usize,
    /// Coordinates on the two perpendicular axes, see `perpendicular`.
    pub center: [f32; 2],
    pub lo: f32,
    pub hi: f32,
    pub albedo: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ball {
    pub center: [f32; 3],
    pub radius: f32,
    pub albedo: f32,
}

/// The growing tip of a pipe at some tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Head {
    pub seg: Seg,
    /// Center of the round tip cap.
    pub tip: [f32; 3],
}

#[derive(Debug, Clone)]
struct Pipe {
    active: bool,
    /// Grid point the growing segment starts from.
    cell: [i32; 3],
    axis: usize,
    sign: i32,
    albedo: f32,
    /// The segment start was a turn (needed for mitered corners).
    start_turn: bool,
    /// Tick at which the pipe appears.
    birth: f64,
    /// Tick at which the growing segment reaches its far grid point.
    next_event: f64,
}

pub struct Sim {
    params: SimParams,
    rng: Rng,
    occupied: Vec<bool>,
    occupied_count: usize,
    fill_limit: usize,
    pipes: Vec<Pipe>,
    segs: Vec<Seg>,
    balls: Vec<Ball>,
    stopping: bool,
    full_tick: Option<f64>,
    albedo_offset: usize,
    serial: usize,
}

fn direction(axis: usize, sign: i32) -> [i32; 3] {
    let mut step = [0; 3];
    step[axis] = sign;
    step
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

impl Sim {
    pub fn new(params: SimParams, seed: u64) -> Self {
        let cells = (params.grid.max(1) as usize).pow(3);
        let mut sim = Self {
            params,
            rng: Rng::new(seed),
            occupied: vec![false; cells],
            occupied_count: 0,
            fill_limit: ((cells as f64) * FILL_LIMIT).ceil() as usize,
            pipes: Vec::new(),
            segs: Vec::new(),
            balls: Vec::new(),
            stopping: false,
            full_tick: None,
            albedo_offset: 0,
            serial: 0,
        };
        sim.albedo_offset = sim.rng.below(ALBEDOS.len());
        let mut birth = 0.0;
        for _ in 0..params.pipe_count.max(1) {
            if let Some(pipe) = sim.spawn(birth) {
                sim.pipes.push(pipe);
            }
            birth += 0.6 + 1.6 * sim.rng.unit();
        }
        if sim.pipes.is_empty() {
            sim.full_tick = Some(0.0);
        }
        sim
    }

    pub fn segs(&self) -> &[Seg] {
        &self.segs
    }

    pub fn balls(&self) -> &[Ball] {
        &self.balls
    }

    /// Tick at which the structure stopped growing, once it did.
    pub fn full_tick(&self) -> Option<f64> {
        self.full_tick
    }

    #[cfg(test)]
    pub fn occupied_count(&self) -> usize {
        self.occupied_count
    }

    fn index(&self, cell: [i32; 3]) -> usize {
        let grid = self.params.grid as usize;
        (cell[0] as usize * grid + cell[1] as usize) * grid + cell[2] as usize
    }

    fn free(&self, cell: [i32; 3]) -> bool {
        cell.iter().all(|c| (0..self.params.grid).contains(c)) && !self.occupied[self.index(cell)]
    }

    fn mark(&mut self, cell: [i32; 3]) {
        let index = self.index(cell);
        if !self.occupied[index] {
            self.occupied[index] = true;
            self.occupied_count += 1;
        }
    }

    fn free_directions(&self, cell: [i32; 3]) -> Vec<(usize, i32)> {
        let mut options = Vec::with_capacity(6);
        for axis in 0..3 {
            for sign in [-1, 1] {
                if self.free(add(cell, direction(axis, sign))) {
                    options.push((axis, sign));
                }
            }
        }
        options
    }

    /// A free grid point with a free neighbour, or `None` when none is left.
    fn find_spawn(&mut self) -> Option<([i32; 3], (usize, i32))> {
        let grid = self.params.grid;
        for _ in 0..64 {
            let cell = [
                self.rng.below(grid as usize) as i32,
                self.rng.below(grid as usize) as i32,
                self.rng.below(grid as usize) as i32,
            ];
            if !self.free(cell) {
                continue;
            }
            let options = self.free_directions(cell);
            if !options.is_empty() {
                return Some((cell, options[self.rng.below(options.len())]));
            }
        }
        // The volume is crowded: scan from a random start.
        let total = (grid as usize).pow(3);
        let start = self.rng.below(total);
        for offset in 0..total {
            let flat = (start + offset) % total;
            let g = grid as usize;
            let cell = [
                (flat / (g * g)) as i32,
                (flat / g % g) as i32,
                (flat % g) as i32,
            ];
            if !self.free(cell) {
                continue;
            }
            let options = self.free_directions(cell);
            if !options.is_empty() {
                return Some((cell, options[self.rng.below(options.len())]));
            }
        }
        None
    }

    fn joint_radius(&self) -> f32 {
        match self.params.joint {
            JointStyle::Ball => self.params.radius * 1.3,
            JointStyle::Rounded => self.params.radius,
            JointStyle::Bare => 0.0,
        }
    }

    fn spawn(&mut self, birth: f64) -> Option<Pipe> {
        let (cell, (axis, sign)) = self.find_spawn()?;
        self.mark(cell);
        self.mark(add(cell, direction(axis, sign)));
        let albedo = ALBEDOS[(self.serial + self.albedo_offset) % ALBEDOS.len()];
        self.serial += 1;
        let radius = self.joint_radius();
        if radius > 0.0 {
            self.balls.push(Ball {
                center: cell.map(|c| c as f32),
                radius,
                albedo,
            });
        }
        Some(Pipe {
            active: true,
            cell,
            axis,
            sign,
            albedo,
            start_turn: false,
            birth,
            next_event: birth + 1.0,
        })
    }

    /// Advance to `tick`, consuming at most `budget` events. Returns false
    /// when the budget ran out first (the caller should restart).
    pub fn advance_to(&mut self, tick: f64, budget: &mut u32) -> bool {
        loop {
            let next = self
                .pipes
                .iter()
                .enumerate()
                .filter(|(_, pipe)| pipe.active && pipe.next_event <= tick)
                .min_by(|a, b| a.1.next_event.total_cmp(&b.1.next_event))
                .map(|(index, _)| index);
            let Some(index) = next else {
                return true;
            };
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            self.process(index);
        }
    }

    fn choose_next(&mut self, at: [i32; 3], axis: usize, sign: i32) -> Option<(usize, i32)> {
        let straight_free = self.free(add(at, direction(axis, sign)));
        let mut turns = Vec::with_capacity(4);
        for turn_axis in (0..3).filter(|candidate| *candidate != axis) {
            for turn_sign in [-1, 1] {
                if self.free(add(at, direction(turn_axis, turn_sign))) {
                    turns.push((turn_axis, turn_sign));
                }
            }
        }
        if straight_free && (turns.is_empty() || self.rng.unit() >= self.params.turn_chance) {
            return Some((axis, sign));
        }
        if turns.is_empty() {
            return None;
        }
        Some(turns[self.rng.below(turns.len())])
    }

    fn process(&mut self, index: usize) {
        let pipe = self.pipes[index].clone();
        let event = pipe.next_event;
        let start = pipe.cell;
        let end = add(start, direction(pipe.axis, pipe.sign));
        let next = if self.stopping {
            None
        } else {
            self.choose_next(end, pipe.axis, pipe.sign)
        };
        let end_turn = next.is_some_and(|(next_axis, _)| next_axis != pipe.axis);
        self.commit_segment(&pipe, end_turn);
        match next {
            Some((next_axis, next_sign)) => {
                self.mark(add(end, direction(next_axis, next_sign)));
                if end_turn {
                    self.push_ball(end, pipe.albedo);
                }
                let slot = &mut self.pipes[index];
                slot.cell = end;
                slot.axis = next_axis;
                slot.sign = next_sign;
                slot.start_turn = end_turn;
                slot.next_event = event + 1.0;
            }
            None => {
                self.push_ball(end, pipe.albedo);
                self.pipes[index].active = false;
                if !self.stopping {
                    if let Some(replacement) = self.spawn(event) {
                        self.pipes[index] = replacement;
                    }
                }
            }
        }
        if self.occupied_count >= self.fill_limit {
            self.stopping = true;
        }
        if self.full_tick.is_none() && self.pipes.iter().all(|pipe| !pipe.active) {
            self.full_tick = Some(event);
        }
    }

    fn push_ball(&mut self, cell: [i32; 3], albedo: f32) {
        let radius = self.joint_radius();
        if radius > 0.0 {
            self.balls.push(Ball {
                center: cell.map(|c| c as f32),
                radius,
                albedo,
            });
        }
    }

    /// Axial extent of a run: mitered style extends turned ends by a radius so
    /// the flat corner closes.
    fn extent(
        &self,
        from: f32,
        sign: i32,
        length: f32,
        start_turn: bool,
        end_turn: bool,
    ) -> (f32, f32) {
        let miter = if self.params.joint == JointStyle::Bare {
            self.params.radius
        } else {
            0.0
        };
        let sign = sign as f32;
        let back = from - sign * if start_turn { miter } else { 0.0 };
        let front = from + sign * (length + if end_turn { miter } else { 0.0 });
        (back.min(front), back.max(front))
    }

    /// The run a pipe is laying from its current grid point.
    fn make_seg(&self, pipe: &Pipe, length: f32, end_turn: bool) -> Seg {
        let (first, second) = perpendicular(pipe.axis);
        let (lo, hi) = self.extent(
            pipe.cell[pipe.axis] as f32,
            pipe.sign,
            length,
            pipe.start_turn,
            end_turn,
        );
        Seg {
            axis: pipe.axis,
            center: [pipe.cell[first] as f32, pipe.cell[second] as f32],
            lo,
            hi,
            albedo: pipe.albedo,
        }
    }

    fn commit_segment(&mut self, pipe: &Pipe, end_turn: bool) {
        let seg = self.make_seg(pipe, 1.0, end_turn);
        self.segs.push(seg);
    }

    /// Growing tips at `tick`, one per visible active pipe.
    pub fn heads(&self, tick: f64) -> Vec<Head> {
        self.pipes
            .iter()
            .filter(|pipe| pipe.active && tick >= pipe.birth)
            .map(|pipe| {
                let progress = (tick - (pipe.next_event - 1.0)).clamp(0.0, 1.0) as f32;
                let seg = self.make_seg(pipe, progress, false);
                let mut tip = pipe.cell.map(|c| c as f32);
                tip[pipe.axis] += pipe.sign as f32 * progress;
                Head { seg, tip }
            })
            .collect()
    }

    /// Radius of the sphere that caps a growing tip; zero for mitered pipes.
    pub fn cap_radius(&self) -> f32 {
        if self.params.joint == JointStyle::Bare {
            0.0
        } else {
            self.params.radius
        }
    }

    #[cfg(test)]
    fn occupied_cells(&self) -> Vec<[i32; 3]> {
        let grid = self.params.grid;
        let mut cells = Vec::new();
        for x in 0..grid {
            for y in 0..grid {
                for z in 0..grid {
                    if self.occupied[self.index([x, y, z])] {
                        cells.push([x, y, z]);
                    }
                }
            }
        }
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn params(joint: JointStyle) -> SimParams {
        SimParams {
            grid: 8,
            pipe_count: 5,
            turn_chance: 0.4,
            radius: 0.15,
            joint,
        }
    }

    fn run_to_full(sim: &mut Sim) -> f64 {
        let mut budget = 1_000_000;
        assert!(sim.advance_to(10_000.0, &mut budget));
        sim.full_tick().expect("simulation must finish")
    }

    #[test]
    fn rng_is_deterministic_and_uniform_enough() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let first: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let other: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(first, second);
        assert_ne!(first, other);
        let mut rng = Rng::new(1);
        let mean: f64 = (0..10_000).map(|_| rng.unit()).sum::<f64>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        assert!((0..1000).all(|_| rng.below(6) < 6));
    }

    #[test]
    fn pipes_never_overlap_and_stay_inside_the_grid() {
        for seed in 0..12 {
            let mut sim = Sim::new(params(JointStyle::Ball), seed);
            run_to_full(&mut sim);
            let mut visited = BTreeSet::new();
            let mut claimed = 0usize;
            for seg in sim.segs() {
                let (first, second) = perpendicular(seg.axis);
                let mut a = [0i32; 3];
                let mut b = [0i32; 3];
                a[first] = seg.center[0] as i32;
                a[second] = seg.center[1] as i32;
                b[first] = a[first];
                b[second] = a[second];
                a[seg.axis] = seg.lo.round() as i32;
                b[seg.axis] = seg.hi.round() as i32;
                assert_eq!(b[seg.axis] - a[seg.axis], 1, "segments span one cell");
                for cell in [a, b] {
                    assert!(cell.iter().all(|c| (0..8).contains(c)), "{cell:?}");
                    visited.insert(cell);
                }
                claimed += 1;
            }
            assert!(claimed > 20, "seed {seed}: only {claimed} segments");
            // Every visited grid point is occupied; any surplus occupied
            // cell is a reserved target of an unfinished tip, which cannot
            // exist once the structure is full.
            let occupied: BTreeSet<[i32; 3]> = sim.occupied_cells().into_iter().collect();
            assert_eq!(visited, occupied, "seed {seed}");
            // Each spawn claims one point and each run claims its far point:
            // equality proves no run entered an occupied point.
            assert_eq!(occupied.len(), sim.serial + sim.segs().len(), "seed {seed}");
        }
    }

    #[test]
    fn growth_is_a_function_of_the_seed_only() {
        let mut a = Sim::new(params(JointStyle::Ball), 42);
        let mut b = Sim::new(params(JointStyle::Ball), 42);
        let mut budget = 100_000;
        // Different advance schedules, same result.
        for tick in (0..200).map(|step| f64::from(step) * 0.37) {
            assert!(a.advance_to(tick, &mut budget));
        }
        assert!(b.advance_to(73.0, &mut budget));
        assert!(a.advance_to(73.0, &mut budget));
        assert_eq!(a.segs(), b.segs());
        assert_eq!(a.balls(), b.balls());
    }

    #[test]
    fn fills_to_the_limit_and_reports_the_finish() {
        let mut sim = Sim::new(params(JointStyle::Rounded), 3);
        let full = run_to_full(&mut sim);
        assert!(full > 10.0);
        let minimum = (512.0 * FILL_LIMIT) as usize;
        assert!(sim.occupied_count() >= minimum, "{}", sim.occupied_count());
        assert!(sim.heads(full + 5.0).is_empty(), "no tips remain when full");
    }

    #[test]
    fn straight_runs_when_turn_chance_is_zero() {
        let mut settings = params(JointStyle::Ball);
        settings.turn_chance = 0.0;
        settings.pipe_count = 1;
        let mut sim = Sim::new(settings, 5);
        let mut budget = 10_000;
        // Seven steps at most fit a straight run in an 8-cell grid.
        assert!(sim.advance_to(3.0, &mut budget));
        let axes: BTreeSet<usize> = sim.segs().iter().map(|seg| seg.axis).collect();
        assert_eq!(axes.len(), 1, "no turns expected in the first steps");
    }

    #[test]
    fn head_length_follows_the_tick_fraction() {
        let mut sim = Sim::new(params(JointStyle::Ball), 9);
        let mut budget = 1000;
        assert!(sim.advance_to(0.5, &mut budget));
        let first = sim.heads(0.0);
        assert!(first.len() == 1, "only the first pipe is born at tick 0");
        let short = first[0].seg.hi - first[0].seg.lo;
        let later = sim.heads(0.5)[0].seg.hi - sim.heads(0.5)[0].seg.lo;
        assert!(short.abs() < 1e-6);
        assert!((later - 0.5).abs() < 1e-5);
    }

    #[test]
    fn mitered_corners_extend_the_runs() {
        let mut sim = Sim::new(params(JointStyle::Bare), 4);
        let mut budget = 10_000;
        assert!(sim.advance_to(40.0, &mut budget));
        assert!(sim.balls().is_empty());
        assert!(
            sim.segs().iter().any(|seg| seg.hi - seg.lo > 1.05),
            "some run must be extended at a turn"
        );
        assert_eq!(sim.cap_radius(), 0.0);
    }
}
