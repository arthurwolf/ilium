//! A deliberately small strategy simulation. No I/O, clocks or external assets.
//! The connected graph and late frontier campaign provide a finite ending.
pub(super) const TICK_SECONDS: f64 = 0.5;
const UNIFICATION_TICK: u64 = 800;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Star {
    pub position: (f32, f32),
    pub owner: Option<usize>,
    pub minerals: f32,
    pub energy: f32,
    pub garrison: f32,
}
#[derive(Debug, Clone)]
pub(super) struct Empire {
    pub color: [u8; 3],
    pub credits: f32,
    pub ships: f32,
    pub aggression: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Fleet {
    pub owner: usize,
    pub from: usize,
    pub to: usize,
    pub strength: f32,
    pub progress: f32,
    pub travel_ticks: u32,
    pub campaign: bool,
    returning: bool,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct Relation {
    pub war: bool,
    remaining: u32,
}
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Statistics {
    pub captures: u64,
    pub battles: u64,
    pub wars: u64,
    pub treaties: u64,
    pub eliminations: usize,
}

pub(super) struct Galaxy {
    pub stars: Vec<Star>,
    pub lanes: Vec<(usize, usize)>,
    pub neighbors: Vec<Vec<usize>>,
    pub empires: Vec<Empire>,
    pub relations: Vec<Vec<Relation>>,
    pub fleets: Vec<Fleet>,
    pub tick: u64,
    pub hegemon: Option<usize>,
    pub winner: Option<usize>,
    pub victory_tick: Option<u64>,
    pub stats: Statistics,
    pub last_captures: Vec<(usize, usize, usize)>,
    random: Random,
}

struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
    fn fraction(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }
    fn index(&mut self, length: usize) -> usize {
        (self.next() % length.max(1) as u64) as usize
    }
}

fn distance_squared(first: (f32, f32), second: (f32, f32)) -> f32 {
    (first.0 - second.0).powi(2) + (first.1 - second.1).powi(2)
}

impl Galaxy {
    pub fn new(seed: u64, star_count: usize, empire_count: usize) -> Self {
        let star_count = star_count.clamp(120, 420);
        let empire_count = empire_count.clamp(3, 12);
        let mut random = Random(seed);
        let mut stars: Vec<Star> = Vec::with_capacity(star_count);
        let phase = random.fraction() * std::f32::consts::TAU;
        for index in 0..star_count {
            let mut position = (0.0, 0.0);
            for _ in 0..64 {
                let radius = 0.12 + 0.85 * random.fraction().sqrt();
                let angle = phase
                    + (index % 4) as f32 * std::f32::consts::FRAC_PI_2
                    + radius * 3.2
                    + (random.fraction() - 0.5) * 0.8;
                position = (radius * angle.cos(), radius * angle.sin());
                if stars
                    .iter()
                    .all(|star| distance_squared(position, star.position) > 0.0015)
                {
                    break;
                }
            }
            stars.push(Star {
                position,
                owner: None,
                minerals: 1.0 + random.fraction() * 5.0,
                energy: 0.5 + random.fraction() * 4.0,
                garrison: 1.0 + random.fraction() * 2.0,
            });
        }
        // Prim's tree is the connectivity backbone; short additional lanes
        // supply loops without turning the map into a dense complete graph.
        let mut neighbors = vec![Vec::new(); star_count];
        let mut lanes = Vec::new();
        let mut included = vec![false; star_count];
        let mut nearest = vec![(f32::INFINITY, 0); star_count];
        included[0] = true;
        let mut current = 0;
        for _ in 1..star_count {
            for index in 0..star_count {
                let distance = distance_squared(stars[current].position, stars[index].position);
                if !included[index] && distance < nearest[index].0 {
                    nearest[index] = (distance, current);
                }
            }
            let next = (0..star_count)
                .filter(|index| !included[*index])
                .min_by(|a, b| nearest[*a].0.total_cmp(&nearest[*b].0));
            let Some(next) = next else {
                break;
            };
            Self::connect(&mut neighbors, &mut lanes, nearest[next].1, next);
            included[next] = true;
            current = next;
        }
        for index in 0..star_count {
            let mut closest: Vec<_> = (0..star_count).filter(|other| *other != index).collect();
            closest.sort_by(|a, b| {
                distance_squared(stars[index].position, stars[*a].position)
                    .total_cmp(&distance_squared(stars[index].position, stars[*b].position))
            });
            for other in closest.into_iter().take(2) {
                if distance_squared(stars[index].position, stars[other].position) < 0.05
                    && !neighbors[index].contains(&other)
                {
                    Self::connect(&mut neighbors, &mut lanes, index, other);
                }
            }
        }
        let colors = [
            [104, 186, 222],
            [223, 130, 157],
            [127, 207, 158],
            [220, 183, 109],
            [159, 139, 227],
            [216, 151, 107],
            [109, 208, 202],
            [205, 147, 212],
            [159, 196, 106],
            [128, 154, 223],
            [221, 170, 187],
            [181, 199, 159],
        ];
        let empires = (0..empire_count)
            .map(|index| Empire {
                color: colors[index],
                credits: 10.0,
                ships: 8.0 + random.fraction() * 8.0,
                aggression: 0.25 + random.fraction() * 0.6,
            })
            .collect();
        let mut capitals = vec![random.index(star_count)];
        for _ in 1..empire_count {
            let capital = (0..star_count)
                .filter(|index| !capitals.contains(index))
                .max_by(|a, b| {
                    let nearest_distance = |index: usize| {
                        capitals
                            .iter()
                            .map(|capital| {
                                distance_squared(stars[index].position, stars[*capital].position)
                            })
                            .fold(f32::INFINITY, f32::min)
                    };
                    nearest_distance(*a).total_cmp(&nearest_distance(*b))
                });
            if let Some(capital) = capital {
                capitals.push(capital);
            }
        }
        // Reserve every capital before granting neighboring colonies.
        for (owner, &capital) in capitals.iter().enumerate() {
            stars[capital].owner = Some(owner);
            stars[capital].garrison = 5.0;
        }
        for (owner, capital) in capitals.into_iter().enumerate() {
            if let Some(neighbor) = neighbors[capital]
                .iter()
                .copied()
                .find(|index| stars[*index].owner.is_none())
            {
                stars[neighbor].owner = Some(owner);
                stars[neighbor].garrison = 3.0;
            }
        }
        Self {
            stars,
            lanes,
            neighbors,
            empires,
            relations: vec![
                vec![
                    Relation {
                        war: false,
                        remaining: 0
                    };
                    empire_count
                ];
                empire_count
            ],
            fleets: Vec::new(),
            tick: 0,
            hegemon: None,
            winner: None,
            victory_tick: None,
            stats: Statistics::default(),
            last_captures: Vec::new(),
            random,
        }
    }

    fn connect(
        neighbors: &mut [Vec<usize>],
        lanes: &mut Vec<(usize, usize)>,
        first: usize,
        second: usize,
    ) {
        neighbors[first].push(second);
        neighbors[second].push(first);
        lanes.push((first.min(second), first.max(second)));
    }

    fn counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.empires.len()];
        for star in &self.stars {
            if let Some(owner) = star.owner {
                counts[owner] += 1;
            }
        }
        counts
    }
    pub fn living_empires(&self) -> usize {
        self.counts().iter().filter(|count| **count > 0).count()
    }

    pub fn step(&mut self) {
        self.last_captures.clear();
        self.tick += 1;
        if self.winner.is_some() {
            return;
        }
        self.economy();
        self.diplomacy();
        if self.tick >= UNIFICATION_TICK && self.hegemon.is_none() {
            self.hegemon = self
                .counts()
                .iter()
                .enumerate()
                .max_by_key(|(index, count)| (**count, usize::MAX - index))
                .map(|(index, _)| index);
        }
        self.move_fleets();
        if self.tick.is_multiple_of(6) {
            self.dispatch();
        }
        self.dispatch_campaign();
        let counts = self.counts();
        let living: Vec<_> = counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(index, _)| index)
            .collect();
        if living.len() == 1 && self.stars.iter().all(|star| star.owner.is_some()) {
            self.winner = living.first().copied();
            self.victory_tick = Some(self.tick);
            self.fleets.clear();
        }
    }

    fn economy(&mut self) {
        let mut income = vec![(0.0, 0.0); self.empires.len()];
        for star in &mut self.stars {
            if let Some(owner) = star.owner {
                income[owner].0 += star.minerals;
                income[owner].1 += star.energy;
                star.garrison = (star.garrison + 0.025 * star.minerals).min(60.0);
            }
        }
        for (empire, (minerals, energy)) in self.empires.iter_mut().zip(income) {
            empire.credits = (empire.credits + energy * 0.08).min(500.0);
            let built = (minerals * 0.12).min(empire.credits);
            empire.credits -= built;
            empire.ships = (empire.ships + built).min(240.0);
        }
    }

    fn diplomacy(&mut self) {
        let count = self.empires.len();
        let mut borders = vec![vec![false; count]; count];
        for &(first, second) in &self.lanes {
            if let (Some(a), Some(b)) = (self.stars[first].owner, self.stars[second].owner) {
                if a != b {
                    borders[a][b] = true;
                    borders[b][a] = true;
                }
            }
        }
        for (first, row) in borders.iter().enumerate() {
            for (second, contact) in row.iter().enumerate().skip(first + 1) {
                if !contact {
                    continue;
                }
                // The announced final campaign remains at war: a truce must
                // not invalidate the finite frontier-conquest guarantee.
                if self.hegemon == Some(first) || self.hegemon == Some(second) {
                    if !self.relations[first][second].war {
                        self.stats.wars += 1;
                    }
                    self.relations[first][second].war = true;
                    self.relations[second][first].war = true;
                    continue;
                }
                let relation = &mut self.relations[first][second];
                relation.remaining = relation.remaining.saturating_sub(1);
                if relation.remaining != 0 {
                    continue;
                }
                let was_war = relation.war;
                relation.war = self.random.fraction()
                    < (self.empires[first].aggression + self.empires[second].aggression) * 0.5;
                relation.remaining = 40 + self.random.index(120) as u32;
                if relation.war && !was_war {
                    self.stats.wars += 1;
                }
                if !relation.war && was_war {
                    self.stats.treaties += 1;
                }
                self.relations[second][first] = *relation;
            }
        }
    }

    fn move_fleets(&mut self) {
        let before = self.counts();
        let mut travelling = Vec::with_capacity(self.fleets.len());
        let fleets = std::mem::take(&mut self.fleets);
        for mut fleet in fleets {
            if before[fleet.owner] == 0 {
                continue;
            }
            fleet.progress += 1.0 / fleet.travel_ticks as f32;
            if fleet.progress < 1.0 {
                travelling.push(fleet);
                continue;
            }
            let target = &mut self.stars[fleet.to];
            if target.owner == Some(fleet.owner) {
                self.empires[fleet.owner].ships =
                    (self.empires[fleet.owner].ships + fleet.strength * 0.7).min(240.0);
                continue;
            }
            // A returning fleet whose home was lost disperses; it cannot
            // attack a new owner or resurrect an eliminated empire.
            if fleet.returning {
                continue;
            }
            if target
                .owner
                .is_some_and(|owner| !self.relations[fleet.owner][owner].war)
            {
                // Revalidate diplomacy at arrival and visibly retrace the
                // actual hyperlane rather than conquering during a truce.
                std::mem::swap(&mut fleet.from, &mut fleet.to);
                fleet.progress = 0.0;
                fleet.returning = true;
                travelling.push(fleet);
                continue;
            }
            if target.owner == self.hegemon && self.hegemon.is_some() {
                self.stats.battles += 1;
                continue;
            }
            let defense = target.garrison
                + target
                    .owner
                    .map_or(0.0, |owner| self.empires[owner].ships * 0.15);
            if target.owner.is_some() {
                self.stats.battles += 1;
            }
            if fleet.strength > defense {
                target.owner = Some(fleet.owner);
                target.garrison = ((fleet.strength - defense) * 0.25).clamp(1.0, 60.0);
                self.stats.captures += 1;
                self.last_captures.push((fleet.from, fleet.to, fleet.owner));
            } else {
                target.garrison = (target.garrison - fleet.strength * 0.5).max(0.5);
            }
        }
        let after = self.counts();
        self.stats.eliminations += before
            .iter()
            .zip(&after)
            .filter(|(a, b)| **a > 0 && **b == 0)
            .count();
        travelling.retain(|fleet| after[fleet.owner] > 0);
        self.fleets = travelling;
    }

    fn dispatch(&mut self) {
        let counts = self.counts();
        for (owner, count) in counts.iter().enumerate() {
            if *count == 0
                || self.empires[owner].ships < 5.0
                || self
                    .fleets
                    .iter()
                    .filter(|fleet| fleet.owner == owner)
                    .count()
                    >= 3
            {
                continue;
            }
            let mut targets = Vec::new();
            for (from, star) in self.stars.iter().enumerate() {
                if star.owner != Some(owner) {
                    continue;
                }
                for &to in &self.neighbors[from] {
                    let other = self.stars[to].owner;
                    if other == Some(owner)
                        || self
                            .fleets
                            .iter()
                            .any(|fleet| fleet.owner == owner && fleet.to == to)
                    {
                        continue;
                    }
                    if other.is_some_and(|other| !self.relations[owner][other].war) {
                        continue;
                    }
                    if self.hegemon.is_some_and(|dominant| other == Some(dominant)) {
                        continue;
                    }
                    targets.push((from, to));
                }
            }
            if targets.is_empty() {
                continue;
            }
            let (from, to) = targets[self.random.index(targets.len())];
            let strength = (self.empires[owner].ships * 0.65).max(4.0);
            self.empires[owner].ships -= strength;
            self.launch(owner, from, to, strength, false);
        }
    }

    fn launch(&mut self, owner: usize, from: usize, to: usize, strength: f32, campaign: bool) {
        let distance = distance_squared(self.stars[from].position, self.stars[to].position).sqrt();
        let travel_ticks = (6.0 + distance * 24.0).ceil() as u32;
        self.fleets.push(Fleet {
            owner,
            from,
            to,
            strength,
            progress: 0.0,
            travel_ticks,
            campaign,
            returning: false,
        });
    }

    fn dispatch_campaign(&mut self) {
        let Some(owner) = self.hegemon else {
            return;
        };
        if self.fleets.iter().any(|fleet| fleet.campaign) {
            return;
        }
        // Connectivity guarantees a frontier unless all stars belong to the
        // hegemon. Its campaign strength exceeds every bounded defense, while
        // hegemon ownership cannot be lost. Each arrival shrinks the finite
        // set of remaining systems; no mass transfer or disconnected capture.
        let frontier = self
            .stars
            .iter()
            .enumerate()
            .filter(|(_, star)| star.owner == Some(owner))
            .find_map(|(from, _)| {
                self.neighbors[from]
                    .iter()
                    .copied()
                    .find(|to| self.stars[*to].owner != Some(owner))
                    .map(|to| (from, to))
            });
        if let Some((from, to)) = frontier {
            if let Some(other) = self.stars[to].owner {
                if !self.relations[owner][other].war {
                    self.stats.wars += 1;
                }
                self.relations[owner][other].war = true;
                self.relations[other][owner].war = true;
            }
            self.launch(owner, from, to, 160.0, true);
        }
    }
}

#[cfg(test)]
#[path = "simulation_tests.rs"]
mod tests;
