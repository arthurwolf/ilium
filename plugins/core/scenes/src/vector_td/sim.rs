//! The tower defense rules: monsters walk the path, towers shoot, money is
//! earned and spent, waves launch and levels end. Fixed time steps and a
//! seeded generator make a game a pure function of its inputs.

use super::maps::Level;
use super::model::{self, MonsterKind, TowerKind};
use std::collections::VecDeque;

/// Simulation time step in seconds.
pub const STEP: f32 = 1.0 / 30.0;
/// Seconds the map takes to draw itself before the first wave.
pub const INTRO_SECONDS: f32 = 7.0;
/// Seconds before the first wave after the intro.
const FIRST_WAVE_DELAY: f32 = 5.0;
/// Seconds between automatic waves.
pub const WAVE_GAP: f32 = 17.0;
/// Seconds the end-of-level and defeat banners stay.
pub const OUTRO_SECONDS: f32 = 6.5;
/// Shortest time between two launched waves.
const MIN_WAVE_SPACING: f32 = 1.5;

/// Deterministic splitmix64 generator.
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
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

/// What a drawn thing is, for the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Tower(TowerKind),
    Monster(MonsterKind),
    Money,
    Danger,
    Frost,
}

#[derive(Debug, Clone)]
pub struct Monster {
    pub id: u32,
    pub kind: MonsterKind,
    pub path: usize,
    /// Distance walked along the path (ground) or the straight line (air).
    pub distance: f32,
    pub pos: (f32, f32),
    pub heading: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub wave: u32,
    pub bounty: f32,
    pub slow_factor: f32,
    pub slow_until: f32,
    /// Seconds since a hit, for the hit flash.
    pub hurt: f32,
    pub spin: f32,
}

#[derive(Debug, Clone)]
pub struct Tower {
    pub kind: TowerKind,
    pub cell: (i32, i32),
    pub level: u8,
    pub cooldown: f32,
    pub aim: f32,
    pub spent: f32,
    pub built_at: f32,
    /// Seconds since the last upgrade, for the upgrade flash.
    pub upgraded: f32,
    /// Seconds since the last shot, for the muzzle flash.
    pub fired: f32,
    pub kills: u32,
    pub damage_boost: f32,
    pub rate_boost: f32,
}

impl Tower {
    pub fn center(&self) -> (f32, f32) {
        (self.cell.0 as f32, self.cell.1 as f32)
    }

    pub fn range(&self) -> f32 {
        self.kind.stats().range * model::level_range(self.level)
    }

    pub fn damage(&self, tech: f32) -> f32 {
        self.kind.stats().damage * model::level_damage(self.level) * tech * self.damage_boost
    }

    pub fn cooldown_seconds(&self) -> f32 {
        self.kind.stats().cooldown * model::level_cooldown(self.level) / self.rate_boost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileKind {
    Bolt,
    Shell,
    Missile,
}

#[derive(Debug, Clone)]
pub struct Projectile {
    pub kind: ProjectileKind,
    pub source: TowerKind,
    pub pos: (f32, f32),
    pub heading: f32,
    pub target: u32,
    pub speed: f32,
    pub damage: f32,
    pub splash: f32,
    pub pierce: f32,
    pub slow: Option<(f32, f32)>,
    pub age: f32,
    pub owner: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxKind {
    Ring,
    Beam,
    Zap,
    Burst,
    Flash,
}

#[derive(Debug, Clone)]
pub struct Fx {
    pub kind: FxKind,
    pub tint: Tint,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub radius: f32,
    pub points: Vec<(f32, f32)>,
    pub age: f32,
    pub life: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The map draws itself; the AI lays out its first towers.
    Intro,
    Running,
    Cleared,
    Defeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    NextLevel,
    Retry,
}

#[derive(Debug, Clone)]
pub struct Banner {
    pub title: String,
    pub subtitle: String,
    pub age: f32,
    pub life: f32,
}

#[derive(Debug, Clone)]
struct Spawn {
    at: f32,
    kind: MonsterKind,
    path: usize,
    wave: u32,
    hp: f32,
}

/// Knobs from the settings that change the rules.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// Monster health multiplier.
    pub difficulty: f32,
    /// Waves in one level.
    pub waves: u32,
}

pub struct Game {
    pub level: Level,
    pub stage: u32,
    pub rules: Rules,
    pub time: f32,
    pub phase: Phase,
    pub phase_time: f32,
    pub money: f32,
    pub lives: i32,
    pub wave: u32,
    /// Seconds until the next automatic wave.
    pub wave_timer: f32,
    pub towers: Vec<Tower>,
    pub monsters: Vec<Monster>,
    pub projectiles: Vec<Projectile>,
    pub fx: Vec<Fx>,
    pub banner: Option<Banner>,
    pub kills: u32,
    pub leaks: u32,
    pub rng: Rng,
    /// Seed of this game, also used by the AI to break ties differently.
    pub seed: u64,
    pub ai_timer: f32,
    /// Seconds since the wave last started (for spacing early sends).
    pub since_wave: f32,
    pub tech: f32,
    next_id: u32,
    spawns: VecDeque<Spawn>,
    /// The wave whose clear bonus is still owed.
    bonus_wave: Option<u32>,
}

impl Game {
    pub fn new(
        map_index: usize,
        width: i32,
        stage: u32,
        defeats: u32,
        rules: Rules,
        seed: u64,
    ) -> Self {
        let level = Level::build(map_index, width);
        let mut game = Self {
            level,
            stage,
            rules,
            time: 0.0,
            phase: Phase::Intro,
            phase_time: 0.0,
            money: model::starting_money(stage),
            lives: model::STARTING_LIVES,
            wave: 0,
            wave_timer: FIRST_WAVE_DELAY,
            towers: Vec::new(),
            monsters: Vec::new(),
            projectiles: Vec::new(),
            fx: Vec::new(),
            banner: None,
            kills: 0,
            leaks: 0,
            rng: Rng::new(seed),
            seed,
            ai_timer: 0.0,
            since_wave: 0.0,
            tech: model::tech_damage(stage, defeats),
            next_id: 1,
            spawns: VecDeque::new(),
            bonus_wave: None,
        };
        let title = format!("LEVEL {}", stage + 1);
        let unlocked: Vec<&str> = TowerKind::ALL
            .iter()
            .filter(|kind| stage > 0 && kind.unlock_stage() == stage)
            .map(|kind| kind.label())
            .collect();
        let subtitle = if unlocked.is_empty() {
            game.level.name.to_owned()
        } else {
            format!("{}  NEW {}", game.level.name, unlocked.join(" "))
        };
        game.set_banner(title, subtitle, INTRO_SECONDS - 1.0);
        game
    }

    pub fn set_banner(&mut self, title: String, subtitle: String, life: f32) {
        self.banner = Some(Banner {
            title,
            subtitle,
            age: 0.0,
            life,
        });
    }

    pub fn max_tower_level(&self) -> u8 {
        model::max_level(self.stage)
    }

    pub fn is_unlocked(&self, kind: TowerKind) -> bool {
        kind.unlock_stage() <= self.stage
    }

    pub fn tower_at(&self, cell: (i32, i32)) -> Option<usize> {
        self.towers.iter().position(|tower| tower.cell == cell)
    }

    pub fn can_build_at(&self, cell: (i32, i32)) -> bool {
        self.level.is_buildable(cell) && self.tower_at(cell).is_none()
    }

    pub fn build(&mut self, kind: TowerKind, cell: (i32, i32)) -> bool {
        let cost = kind.stats().cost;
        if !self.is_unlocked(kind) || !self.can_build_at(cell) || self.money < cost {
            return false;
        }
        self.money -= cost;
        self.towers.push(Tower {
            kind,
            cell,
            level: 1,
            cooldown: 0.4,
            aim: -std::f32::consts::FRAC_PI_2,
            spent: cost,
            built_at: self.time,
            upgraded: 9.0,
            fired: 9.0,
            kills: 0,
            damage_boost: 1.0,
            rate_boost: 1.0,
        });
        self.fx.push(Fx {
            kind: FxKind::Ring,
            tint: Tint::Tower(kind),
            from: (cell.0 as f32, cell.1 as f32),
            to: (0.0, 0.0),
            radius: kind.stats().range,
            points: Vec::new(),
            age: 0.0,
            life: 0.9,
        });
        self.update_boosts();
        true
    }

    pub fn upgrade(&mut self, index: usize) -> bool {
        let Some(tower) = self.towers.get(index) else {
            return false;
        };
        let cost = model::upgrade_cost(tower.kind, tower.level);
        if tower.level >= self.max_tower_level() || self.money < cost {
            return false;
        }
        self.money -= cost;
        let tower = &mut self.towers[index];
        tower.level += 1;
        tower.spent += cost;
        tower.upgraded = 0.0;
        let (kind, center) = (tower.kind, tower.center());
        let range = tower.range();
        self.fx.push(Fx {
            kind: FxKind::Ring,
            tint: Tint::Tower(kind),
            from: center,
            to: (0.0, 0.0),
            radius: range,
            points: Vec::new(),
            age: 0.0,
            life: 0.9,
        });
        self.update_boosts();
        true
    }

    pub fn sell(&mut self, index: usize) -> bool {
        if index >= self.towers.len() {
            return false;
        }
        let tower = self.towers.remove(index);
        self.money += (tower.spent * 0.7).floor();
        self.fx.push(Fx {
            kind: FxKind::Burst,
            tint: Tint::Money,
            from: tower.center(),
            to: (0.0, 0.0),
            radius: 0.8,
            points: Vec::new(),
            age: 0.0,
            life: 0.6,
        });
        // Projectiles of the sold tower keep flying; their owner index is
        // only a tie-breaker for kill credit, so shift it consistently.
        for projectile in &mut self.projectiles {
            if projectile.owner > index {
                projectile.owner -= 1;
            }
        }
        self.update_boosts();
        true
    }

    /// Launch the next wave now; the remaining timer pays out as a bonus.
    pub fn send_wave_early(&mut self) -> bool {
        if self.phase != Phase::Running
            || self.wave >= self.rules.waves
            || self.since_wave < MIN_WAVE_SPACING
        {
            return false;
        }
        let bonus = (self.wave_timer * 0.8).floor().max(0.0);
        self.money += bonus;
        self.launch_wave();
        if bonus > 0.0 {
            self.set_banner(
                format!("WAVE {}", self.wave),
                format!("SENT EARLY +{bonus:.0}"),
                2.2,
            );
        }
        true
    }

    fn launch_wave(&mut self) {
        self.wave += 1;
        self.since_wave = 0.0;
        self.wave_timer = WAVE_GAP;
        let interest = (self.money * 0.04).min(45.0).floor();
        self.money += interest;
        let wave = self.wave;
        let scale = model::hp_scale(wave, self.stage) * self.rules.difficulty;
        let path_count = self.level.paths.len();
        let mut clock = self.time + 0.4;
        let mut label = String::new();
        for group in model::wave_groups(wave) {
            if label.is_empty() {
                label = group.kind.label().to_owned();
            }
            for index in 0..group.count {
                self.spawns.push_back(Spawn {
                    at: clock,
                    kind: group.kind,
                    path: index as usize % path_count,
                    wave,
                    hp: group.kind.base_hp() * scale,
                });
                clock += group.gap;
            }
            clock += 0.8;
        }
        // Spawns of one wave must stay ordered in time for the queue front.
        let mut ordered: Vec<Spawn> = self.spawns.drain(..).collect();
        ordered.sort_by(|a, b| a.at.total_cmp(&b.at));
        self.spawns = ordered.into();
        self.bonus_wave = Some(wave);
        if self.banner.as_ref().is_none_or(|banner| banner.age > 0.5) {
            self.set_banner(format!("WAVE {wave}"), label, 2.4);
        }
    }

    fn spawn_monster(&mut self, spawn: &Spawn) {
        let id = self.next_id;
        self.next_id += 1;
        let (pos, heading) = if spawn.kind.is_flying() {
            let (start, _) = self.level.position(spawn.path, 0.0);
            let (end, _) = self
                .level
                .position(spawn.path, self.level.path_length(spawn.path));
            (start, (end.1 - start.1).atan2(end.0 - start.0))
        } else {
            self.level.position(spawn.path, 0.0)
        };
        self.monsters.push(Monster {
            id,
            kind: spawn.kind,
            path: spawn.path,
            distance: 0.0,
            pos,
            heading,
            hp: spawn.hp,
            max_hp: spawn.hp,
            wave: spawn.wave,
            bounty: model::bounty(spawn.kind, spawn.wave, self.stage),
            slow_factor: 1.0,
            slow_until: 0.0,
            hurt: 9.0,
            spin: self.rng.range(0.0, std::f32::consts::TAU),
        });
    }

    fn update_boosts(&mut self) {
        let beacons: Vec<((f32, f32), f32, u8)> = self
            .towers
            .iter()
            .filter(|tower| tower.kind == TowerKind::Beacon)
            .map(|tower| (tower.center(), tower.range(), tower.level))
            .collect();
        for tower in &mut self.towers {
            let mut damage = 0.0f32;
            let mut rate = 0.0f32;
            if tower.kind != TowerKind::Beacon {
                for &(center, range, level) in &beacons {
                    let (dx, dy) = (
                        center.0 - tower.cell.0 as f32,
                        center.1 - tower.cell.1 as f32,
                    );
                    if dx * dx + dy * dy <= range * range {
                        damage += 0.2 + 0.06 * f32::from(level - 1);
                        rate += 0.1 + 0.03 * f32::from(level - 1);
                    }
                }
            }
            tower.damage_boost = 1.0 + damage.min(0.7);
            tower.rate_boost = 1.0 + rate.min(0.35);
        }
    }

    /// Advance the game by one `STEP`. Returns a level transition when the
    /// outro of a cleared or lost level is over.
    pub fn step(&mut self) -> Option<Transition> {
        self.time += STEP;
        self.phase_time += STEP;
        self.age_visuals();
        match self.phase {
            Phase::Intro => {
                self.run_ai();
                if self.phase_time >= INTRO_SECONDS {
                    self.phase = Phase::Running;
                    self.phase_time = 0.0;
                }
            }
            Phase::Running => {
                self.run_ai();
                self.run_wave_clock();
                self.move_monsters();
                self.fire_towers();
                self.move_projectiles();
                self.reap_dead();
                self.check_outcome();
            }
            Phase::Cleared => {
                if self.phase_time >= OUTRO_SECONDS {
                    return Some(Transition::NextLevel);
                }
            }
            Phase::Defeat => {
                if self.phase_time >= OUTRO_SECONDS {
                    return Some(Transition::Retry);
                }
            }
        }
        None
    }

    fn run_ai(&mut self) {
        self.ai_timer -= STEP;
        if self.ai_timer <= 0.0 {
            self.ai_timer = super::ai::THINK_SECONDS;
            super::ai::think(self);
        }
    }

    fn age_visuals(&mut self) {
        if let Some(banner) = &mut self.banner {
            banner.age += STEP;
            if banner.age > banner.life {
                self.banner = None;
            }
        }
        for fx in &mut self.fx {
            fx.age += STEP;
        }
        self.fx.retain(|fx| fx.age < fx.life);
        for tower in &mut self.towers {
            tower.upgraded += STEP;
            tower.fired += STEP;
        }
        for monster in &mut self.monsters {
            monster.hurt += STEP;
            monster.spin += STEP * 2.0;
        }
    }

    fn run_wave_clock(&mut self) {
        self.since_wave += STEP;
        if self.wave < self.rules.waves {
            self.wave_timer -= STEP;
            if self.wave_timer <= 0.0 {
                self.launch_wave();
            }
        }
        while self
            .spawns
            .front()
            .is_some_and(|spawn| spawn.at <= self.time)
        {
            if let Some(spawn) = self.spawns.pop_front() {
                self.spawn_monster(&spawn);
            }
        }
    }

    fn move_monsters(&mut self) {
        let time = self.time;
        let mut leaked = 0;
        let level = &self.level;
        self.monsters.retain_mut(|monster| {
            if time >= monster.slow_until {
                monster.slow_factor = 1.0;
            }
            monster.distance += monster.kind.speed() * monster.slow_factor * STEP;
            let length = level.path_length(monster.path);
            if monster.distance >= length {
                leaked += monster.kind.leak_cost();
                return false;
            }
            if monster.kind.is_flying() {
                let (start, _) = level.position(monster.path, 0.0);
                let (end, _) = level.position(monster.path, length);
                let fraction = monster.distance / length;
                monster.pos = (
                    start.0 + (end.0 - start.0) * fraction,
                    start.1 + (end.1 - start.1) * fraction,
                );
                monster.heading = (end.1 - start.1).atan2(end.0 - start.0);
            } else {
                let (pos, heading) = level.position(monster.path, monster.distance);
                monster.pos = pos;
                monster.heading = heading;
            }
            true
        });
        if leaked > 0 {
            self.lives -= leaked;
            self.leaks += leaked as u32;
            self.fx.push(Fx {
                kind: FxKind::Flash,
                tint: Tint::Danger,
                from: (0.0, 0.0),
                to: (0.0, 0.0),
                radius: 0.0,
                points: Vec::new(),
                age: 0.0,
                life: 0.5,
            });
        }
    }

    /// Progress of a monster toward the exit, comparable across paths.
    fn progress(&self, monster: &Monster) -> f32 {
        monster.distance / self.level.path_length(monster.path).max(1.0)
    }

    fn fire_towers(&mut self) {
        for index in 0..self.towers.len() {
            let tower = &mut self.towers[index];
            tower.cooldown -= STEP;
            if tower.cooldown > 0.0 || tower.kind == TowerKind::Beacon {
                continue;
            }
            let Some(target) = self.pick_target(index) else {
                self.towers[index].cooldown = 0.0;
                continue;
            };
            self.shoot(index, target);
            let tower = &mut self.towers[index];
            tower.cooldown = tower.cooldown_seconds();
            tower.fired = 0.0;
        }
    }

    /// Index into `monsters` of the monster tower `index` should shoot.
    fn pick_target(&self, index: usize) -> Option<usize> {
        let tower = &self.towers[index];
        let center = tower.center();
        let range = tower.range();
        let stats = tower.kind.stats();
        let mut best: Option<(usize, f32)> = None;
        for (monster_index, monster) in self.monsters.iter().enumerate() {
            if monster.kind.is_flying() && !stats.hits_air {
                continue;
            }
            let (dx, dy) = (monster.pos.0 - center.0, monster.pos.1 - center.1);
            if dx * dx + dy * dy > range * range {
                continue;
            }
            let mut score = self.progress(monster);
            match tower.kind {
                // Prefer the toughest class, then finish wounded monsters
                // instead of spreading shots over fresh ones.
                TowerKind::Needle => {
                    score += monster.max_hp * 0.002 + (1.0 - monster.hp / monster.max_hp);
                }
                TowerKind::Chill if monster.slow_factor < 1.0 => score -= 2.0,
                TowerKind::Lancer => score += monster.hp * 0.001,
                _ => {}
            }
            if best.is_none_or(|(_, current)| score > current) {
                best = Some((monster_index, score));
            }
        }
        best.map(|(monster_index, _)| monster_index)
    }

    fn shoot(&mut self, tower_index: usize, target_index: usize) {
        let tower = self.towers[tower_index].clone();
        let stats = tower.kind.stats();
        let damage = tower.damage(self.tech);
        let center = tower.center();
        let target = self.monsters[target_index].clone();
        let aim = (target.pos.1 - center.1).atan2(target.pos.0 - center.0);
        self.towers[tower_index].aim = aim;
        match tower.kind {
            TowerKind::Pulse | TowerKind::Needle | TowerKind::Chill | TowerKind::Nova => {
                self.projectiles.push(Projectile {
                    kind: if tower.kind == TowerKind::Nova {
                        ProjectileKind::Shell
                    } else {
                        ProjectileKind::Bolt
                    },
                    source: tower.kind,
                    pos: center,
                    heading: aim,
                    target: target.id,
                    speed: stats.speed,
                    damage,
                    splash: stats.splash,
                    pierce: stats.armor_pierce,
                    slow: tower.kind.slow(tower.level),
                    age: 0.0,
                    owner: tower_index,
                });
            }
            TowerKind::Hive => {
                let targets = self.nearest_targets(tower_index, tower.kind.volley() as usize);
                for (shot, id) in targets.into_iter().enumerate() {
                    let spread = (shot as f32 - 1.0) * 0.9 + self.rng.range(-0.2, 0.2);
                    self.projectiles.push(Projectile {
                        kind: ProjectileKind::Missile,
                        source: tower.kind,
                        pos: center,
                        heading: aim + spread,
                        target: id,
                        speed: stats.speed,
                        damage,
                        splash: stats.splash,
                        pierce: stats.armor_pierce,
                        slow: None,
                        age: 0.0,
                        owner: tower_index,
                    });
                }
            }
            TowerKind::Arc => self.zap(tower_index, target_index, damage),
            TowerKind::Lancer => self.beam(tower_index, aim, damage),
            TowerKind::Beacon => {}
        }
    }

    /// Up to `count` distinct monster ids in range of a tower, front first.
    fn nearest_targets(&self, tower_index: usize, count: usize) -> Vec<u32> {
        let tower = &self.towers[tower_index];
        let center = tower.center();
        let range = tower.range();
        let mut candidates: Vec<(f32, u32)> = self
            .monsters
            .iter()
            .filter(|monster| {
                let (dx, dy) = (monster.pos.0 - center.0, monster.pos.1 - center.1);
                dx * dx + dy * dy <= range * range
            })
            .map(|monster| (-self.progress(monster), monster.id))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ids: Vec<u32> = candidates.iter().map(|&(_, id)| id).take(count).collect();
        if let Some(&first) = ids.first() {
            while ids.len() < count {
                ids.push(first);
            }
        }
        ids
    }

    fn zap(&mut self, tower_index: usize, target_index: usize, damage: f32) {
        let tower = self.towers[tower_index].clone();
        let pierce = tower.kind.stats().armor_pierce;
        let mut points = vec![tower.center()];
        let mut hit: Vec<usize> = vec![target_index];
        let mut current = target_index;
        for _ in 0..tower.kind.chain(tower.level) {
            let origin = self.monsters[current].pos;
            let next = self
                .monsters
                .iter()
                .enumerate()
                .filter(|(index, _)| !hit.contains(index))
                .map(|(index, monster)| {
                    let (dx, dy) = (monster.pos.0 - origin.0, monster.pos.1 - origin.1);
                    (index, dx * dx + dy * dy)
                })
                .filter(|&(_, squared)| squared <= 2.4 * 2.4)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((index, _)) = next else { break };
            hit.push(index);
            current = index;
        }
        let mut dealt = damage;
        for &index in &hit {
            points.push(self.monsters[index].pos);
            self.damage_monster(index, dealt, pierce, Some(tower_index));
            dealt *= 0.85;
        }
        self.fx.push(Fx {
            kind: FxKind::Zap,
            tint: Tint::Tower(TowerKind::Arc),
            from: tower.center(),
            to: tower.center(),
            radius: 0.0,
            points,
            age: 0.0,
            life: 0.22,
        });
    }

    fn beam(&mut self, tower_index: usize, aim: f32, damage: f32) {
        let tower = self.towers[tower_index].clone();
        let pierce = tower.kind.stats().armor_pierce;
        let origin = tower.center();
        let length = tower.range() + 1.0;
        let end = (origin.0 + aim.cos() * length, origin.1 + aim.sin() * length);
        let mut indices = Vec::new();
        for (index, monster) in self.monsters.iter().enumerate() {
            let distance = super::maps::segment_distance(monster.pos, origin, end);
            if distance <= 0.5 + monster.kind.radius() {
                indices.push(index);
            }
        }
        for index in indices {
            self.damage_monster(index, damage, pierce, Some(tower_index));
        }
        self.fx.push(Fx {
            kind: FxKind::Beam,
            tint: Tint::Tower(TowerKind::Lancer),
            from: origin,
            to: end,
            radius: 0.0,
            points: Vec::new(),
            age: 0.0,
            life: 0.28,
        });
    }

    fn damage_monster(&mut self, index: usize, raw: f32, pierce: f32, owner: Option<usize>) {
        let monster = &mut self.monsters[index];
        let armor = monster.kind.armor(monster.wave) * (1.0 - pierce);
        let damage = (raw - armor).max(raw * 0.25);
        let was_alive = monster.hp > 0.0;
        monster.hp -= damage;
        monster.hurt = 0.0;
        if was_alive && monster.hp <= 0.0 {
            if let Some(tower) = owner.and_then(|owner| self.towers.get_mut(owner)) {
                tower.kills += 1;
            }
        }
    }

    fn move_projectiles(&mut self) {
        let mut finished: Vec<Projectile> = Vec::new();
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|projectile| {
            projectile.age += STEP;
            let target = self
                .monsters
                .iter()
                .find(|monster| monster.id == projectile.target && monster.hp > 0.0)
                .map(|monster| monster.pos);
            let Some(goal) = target else {
                // The target died: re-aim at the nearest live monster close by.
                let nearest = self
                    .monsters
                    .iter()
                    .filter(|monster| monster.hp > 0.0)
                    .map(|monster| {
                        let (dx, dy) = (
                            monster.pos.0 - projectile.pos.0,
                            monster.pos.1 - projectile.pos.1,
                        );
                        (monster.id, dx * dx + dy * dy)
                    })
                    .filter(|&(_, squared)| squared < 2.5 * 2.5)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                match nearest {
                    Some((id, _)) => projectile.target = id,
                    None => return false,
                }
                return true;
            };
            let (dx, dy) = (goal.0 - projectile.pos.0, goal.1 - projectile.pos.1);
            let distance = dx.hypot(dy);
            let travel = projectile.speed * STEP;
            if distance <= travel + 0.12 {
                projectile.pos = goal;
                finished.push(projectile.clone());
                return false;
            }
            let desired = dy.atan2(dx);
            if projectile.kind == ProjectileKind::Missile {
                let mut delta = desired - projectile.heading;
                while delta > std::f32::consts::PI {
                    delta -= std::f32::consts::TAU;
                }
                while delta < -std::f32::consts::PI {
                    delta += std::f32::consts::TAU;
                }
                projectile.heading += delta.clamp(-7.0 * STEP, 7.0 * STEP);
            } else {
                projectile.heading = desired;
            }
            projectile.pos.0 += projectile.heading.cos() * travel;
            projectile.pos.1 += projectile.heading.sin() * travel;
            projectile.age < 6.0
        });
        self.projectiles = projectiles;
        for projectile in finished {
            self.impact(&projectile);
        }
    }

    fn impact(&mut self, projectile: &Projectile) {
        let mut hits: Vec<usize> = Vec::new();
        if projectile.splash > 0.0 {
            for (index, monster) in self.monsters.iter().enumerate() {
                if monster.hp <= 0.0 {
                    continue;
                }
                let (dx, dy) = (
                    monster.pos.0 - projectile.pos.0,
                    monster.pos.1 - projectile.pos.1,
                );
                if dx * dx + dy * dy <= projectile.splash * projectile.splash
                    && (!monster.kind.is_flying() || projectile.source != TowerKind::Nova)
                {
                    hits.push(index);
                }
            }
            self.fx.push(Fx {
                kind: FxKind::Burst,
                tint: Tint::Tower(projectile.source),
                from: projectile.pos,
                to: (0.0, 0.0),
                radius: projectile.splash,
                points: Vec::new(),
                age: 0.0,
                life: 0.35,
            });
        } else if let Some(index) = self
            .monsters
            .iter()
            .position(|monster| monster.id == projectile.target && monster.hp > 0.0)
        {
            hits.push(index);
        }
        let time = self.time;
        for index in hits {
            self.damage_monster(
                index,
                projectile.damage,
                projectile.pierce,
                Some(projectile.owner),
            );
            if let Some((factor, seconds)) = projectile.slow {
                let monster = &mut self.monsters[index];
                let effect = monster.kind.slow_effect();
                let applied = 1.0 - (1.0 - factor) * effect;
                if applied <= monster.slow_factor || time >= monster.slow_until {
                    monster.slow_factor = applied;
                    monster.slow_until = time + seconds;
                }
            }
        }
    }

    fn reap_dead(&mut self) {
        let mut spawned: Vec<Monster> = Vec::new();
        let mut earned = 0.0;
        let mut bursts: Vec<((f32, f32), MonsterKind)> = Vec::new();
        let mut next_id = self.next_id;
        let mut kills = 0;
        self.monsters.retain(|monster| {
            if monster.hp > 0.0 {
                return true;
            }
            earned += monster.bounty;
            kills += 1;
            bursts.push((monster.pos, monster.kind));
            if monster.kind == MonsterKind::Splitter {
                for offset in [-0.35f32, 0.35] {
                    let mut shard = monster.clone();
                    shard.id = next_id;
                    next_id += 1;
                    shard.kind = MonsterKind::Shard;
                    shard.hp = monster.max_hp * 0.4;
                    shard.max_hp = shard.hp;
                    shard.bounty = monster.bounty * 0.3;
                    shard.distance = (monster.distance + offset).max(0.0);
                    shard.hurt = 9.0;
                    spawned.push(shard);
                }
            }
            false
        });
        self.next_id = next_id;
        self.money += earned;
        self.kills += kills;
        self.monsters.extend(spawned);
        for (pos, kind) in bursts {
            self.fx.push(Fx {
                kind: FxKind::Burst,
                tint: Tint::Monster(kind),
                from: pos,
                to: (0.0, 0.0),
                radius: kind.radius() * 2.2,
                points: Vec::new(),
                age: 0.0,
                life: 0.45,
            });
        }
    }

    fn check_outcome(&mut self) {
        if self.lives <= 0 {
            self.phase = Phase::Defeat;
            self.phase_time = 0.0;
            self.set_banner(
                "DEFEAT".to_owned(),
                "REGROUPING".to_owned(),
                OUTRO_SECONDS - 0.5,
            );
            return;
        }
        let field_clear = self.monsters.is_empty() && self.spawns.is_empty();
        if field_clear {
            if let Some(wave) = self.bonus_wave.take() {
                self.money += model::wave_bonus(wave);
            }
        }
        if field_clear && self.wave >= self.rules.waves {
            self.phase = Phase::Cleared;
            self.phase_time = 0.0;
            self.set_banner(
                "LEVEL CLEARED".to_owned(),
                format!("TECH UP  {} LIVES LEFT", self.lives),
                OUTRO_SECONDS - 0.5,
            );
        }
    }

    /// Total health (scaled) of the wave launched next, for the AI.
    pub fn next_wave_hp(&self) -> f32 {
        let wave = self.wave + 1;
        let scale = model::hp_scale(wave, self.stage) * self.rules.difficulty;
        model::wave_groups(wave)
            .iter()
            .map(|group| group.kind.base_hp() * group.count as f32 * scale)
            .sum()
    }

    /// Health still alive on the field plus queued.
    pub fn threat_hp(&self) -> f32 {
        self.monsters
            .iter()
            .map(|monster| monster.hp.max(0.0))
            .sum::<f32>()
            + self.spawns.iter().map(|spawn| spawn.hp).sum::<f32>()
    }
}
