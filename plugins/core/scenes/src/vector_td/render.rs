//! Draws a `Game` as vector art: grid, glowing path, tower glyphs, monster
//! outlines, shots, effects, status line and banners.

use super::draw::{text_width, Canvas, Stroke};
use super::maps::{Level, HUD_ROWS};
use super::model::{self, MonsterKind, TowerKind};
use super::palette::{Colors, Role};
use super::settings::VectorTdSettings;
use super::sim::{
    Banner, Fx, FxKind, Game, Monster, Phase, Projectile, ProjectileKind, Tint, Tower,
    INTRO_SECONDS, OUTRO_SECONDS, WAVE_GAP,
};
use std::f32::consts::{PI, TAU};

/// Seconds of the wipe that hides the end of a level.
const WIPE_SECONDS: f32 = 1.4;

/// Maps cell coordinates to raster dots.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub scale: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

impl View {
    pub fn fit(raster_width: usize, raster_height: usize, columns: i32, rows: i32) -> Self {
        let scale = (raster_width as f32 / columns as f32).min(raster_height as f32 / rows as f32);
        Self {
            scale,
            origin_x: (raster_width as f32 - scale * columns as f32) / 2.0,
            origin_y: (raster_height as f32 - scale * rows as f32) / 2.0,
        }
    }

    pub fn point(&self, cell: (f32, f32)) -> (f32, f32) {
        (
            self.origin_x + (cell.0 + 0.5) * self.scale,
            self.origin_y + (cell.1 + 0.5) * self.scale,
        )
    }
}

pub struct Painter<'a, 'b> {
    pub canvas: &'b mut Canvas<'a>,
    pub view: View,
    pub colors: &'b Colors,
    pub settings: &'b VectorTdSettings,
    /// Fill strength 0..=1.
    glow: f32,
    /// Half width of thin strokes in dots.
    stroke: f32,
    /// Seconds of game time.
    time: f32,
}

fn rotate(point: (f32, f32), angle: f32) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    (point.0 * cos - point.1 * sin, point.0 * sin + point.1 * cos)
}

fn regular_polygon(sides: usize, phase: f32) -> Vec<(f32, f32)> {
    (0..sides)
        .map(|index| {
            let angle = phase + TAU * index as f32 / sides as f32;
            (angle.cos(), angle.sin())
        })
        .collect()
}

fn hash_unit(a: u32, b: u32) -> f32 {
    let mut value = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b) ^ 0xc2b2_ae35;
    value ^= value >> 15;
    value = value.wrapping_mul(0x2c1b_3c6d);
    value ^= value >> 12;
    value = value.wrapping_mul(0x297a_2d39);
    value ^= value >> 15;
    (value & 0xffff) as f32 / 65535.0
}

impl<'a, 'b> Painter<'a, 'b> {
    pub fn new(
        canvas: &'b mut Canvas<'a>,
        view: View,
        colors: &'b Colors,
        settings: &'b VectorTdSettings,
        time: f32,
    ) -> Self {
        Self {
            canvas,
            view,
            colors,
            settings,
            glow: settings.glow as f32 / 100.0,
            stroke: (view.scale * 0.055).clamp(0.5, 1.2),
            time,
        }
    }

    fn ink(&self, role: Role, tone: f32) -> (f32, [u8; 3]) {
        (self.colors.tone(tone), self.colors.color(role))
    }

    fn line(&mut self, from: (f32, f32), to: (f32, f32), width: f32, role: Role, tone: f32) {
        let (tone, color) = self.ink(role, tone);
        self.canvas.line(from, to, width, tone, color);
    }

    fn outline(&mut self, points: &[(f32, f32)], closed: bool, width: f32, role: Role, tone: f32) {
        let (tone, color) = self.ink(role, tone);
        self.canvas.polyline(points, closed, width, tone, color);
    }

    fn fill(&mut self, points: &[(f32, f32)], role: Role, tone: f32) {
        let (tone, color) = self.ink(role, tone * self.glow);
        self.canvas.fill_polygon(points, tone, color);
    }

    fn disk(&mut self, center: (f32, f32), radius: f32, role: Role, tone: f32) {
        let (tone, color) = self.ink(role, tone);
        self.canvas.disk(center, radius, tone, color);
    }

    fn circle(&mut self, center: (f32, f32), radius: f32, width: f32, role: Role, tone: f32) {
        let (tone, color) = self.ink(role, tone);
        let stroke = Stroke { width, tone, color };
        self.canvas.circle(center, radius, stroke);
    }

    /// Points of a unit shape placed at `center` (dots), rotated, scaled.
    fn place(shape: &[(f32, f32)], center: (f32, f32), angle: f32, size: f32) -> Vec<(f32, f32)> {
        shape
            .iter()
            .map(|&point| {
                let rotated = rotate(point, angle);
                (center.0 + rotated.0 * size, center.1 + rotated.1 * size)
            })
            .collect()
    }

    pub fn paint(&mut self, game: &Game) {
        self.paint_grid(&game.level);
        self.paint_paths(game);
        for tower in &game.towers {
            self.paint_range(tower);
        }
        for tower in &game.towers {
            self.paint_tower(tower, game);
        }
        for monster in game
            .monsters
            .iter()
            .filter(|monster| monster.kind.is_flying())
        {
            self.paint_monster(monster, true);
        }
        for monster in &game.monsters {
            self.paint_monster(monster, false);
        }
        for projectile in &game.projectiles {
            self.paint_projectile(projectile);
        }
        for fx in &game.fx {
            self.paint_fx(fx);
        }
        self.paint_wipe(game);
        if self.settings.hud {
            self.paint_hud(game);
        }
        if let Some(banner) = &game.banner {
            self.paint_banner(banner);
        }
    }

    fn paint_grid(&mut self, level: &Level) {
        if !self.settings.grid {
            return;
        }
        let (tone, color) = self.ink(Role::Grid, 0.55);
        let arm = (self.view.scale * 0.12).max(1.0);
        for y in (HUD_ROWS - 1)..level.height {
            for x in 0..=level.width {
                let center = (
                    self.view.origin_x + x as f32 * self.view.scale,
                    self.view.origin_y + (y as f32 + 0.0) * self.view.scale + self.view.scale * 0.0,
                );
                let (cx, cy) = (center.0.round() as i32, center.1.round() as i32);
                self.canvas.plot(cx, cy, tone, color);
                if arm >= 2.0 {
                    for step in 1..=(arm as i32 / 2) {
                        self.canvas.plot(cx + step, cy, tone * 0.7, color);
                        self.canvas.plot(cx - step, cy, tone * 0.7, color);
                        self.canvas.plot(cx, cy + step, tone * 0.7, color);
                        self.canvas.plot(cx, cy - step, tone * 0.7, color);
                    }
                }
            }
        }
    }

    /// The part of `path` revealed so far, as cell points.
    fn revealed_points(level: &Level, path: usize, reveal: f32) -> Vec<(f32, f32)> {
        let points = &level.paths[path];
        let cumulative = &level.cumulative[path];
        let mut kept = vec![points[0]];
        for index in 1..points.len() {
            if cumulative[index] <= reveal {
                kept.push(points[index]);
            } else {
                kept.push(level.position(path, reveal).0);
                break;
            }
        }
        kept
    }

    /// Offset a polyline sideways by `distance` cells (mitred corners).
    fn offset_polyline(points: &[(f32, f32)], distance: f32) -> Vec<(f32, f32)> {
        let normal = |from: (f32, f32), to: (f32, f32)| {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let length = dx.hypot(dy).max(1e-4);
            (-dy / length, dx / length)
        };
        (0..points.len())
            .map(|index| {
                let previous = (index > 0).then(|| normal(points[index - 1], points[index]));
                let next =
                    (index + 1 < points.len()).then(|| normal(points[index], points[index + 1]));
                let offset = match (previous, next) {
                    (Some(a), Some(b)) => {
                        let denominator = (1.0 + a.0 * b.0 + a.1 * b.1).max(0.2);
                        ((a.0 + b.0) / denominator, (a.1 + b.1) / denominator)
                    }
                    (Some(n), None) | (None, Some(n)) => n,
                    (None, None) => (0.0, 0.0),
                };
                (
                    points[index].0 + offset.0 * distance,
                    points[index].1 + offset.1 * distance,
                )
            })
            .collect()
    }

    fn paint_paths(&mut self, game: &Game) {
        let level = &game.level;
        let reveal_fraction = match game.phase {
            Phase::Intro => (game.phase_time / (INTRO_SECONDS * 0.7)).min(1.0),
            _ => 1.0,
        };
        for path in 0..level.paths.len() {
            let length = level.path_length(path);
            let reveal = length * reveal_fraction;
            let points = Self::revealed_points(level, path, reveal);
            if points.len() < 2 {
                continue;
            }
            let center: Vec<(f32, f32)> = points.iter().map(|&p| self.view.point(p)).collect();
            // The channel: a wide dim fill under two crisp edges.
            let (fill_tone, fill_color) = self.ink(Role::PathFill, 0.55 * self.glow);
            for pair in center.windows(2) {
                self.canvas.line(
                    pair[0],
                    pair[1],
                    self.view.scale * 0.5,
                    fill_tone,
                    fill_color,
                );
            }
            for side in [-0.5f32, 0.5] {
                let edge: Vec<(f32, f32)> = Self::offset_polyline(&points, side)
                    .into_iter()
                    .map(|p| self.view.point(p))
                    .collect();
                self.outline(&edge, false, self.stroke, Role::PathEdge, 0.9);
            }
            // Dashes flowing toward the exit.
            let spacing = 1.6;
            let phase = (self.time * 2.2).rem_euclid(spacing);
            let mut distance = phase;
            while distance + 0.5 < reveal {
                let from = self.view.point(level.position(path, distance).0);
                let to = self.view.point(level.position(path, distance + 0.5).0);
                self.line(from, to, self.stroke, Role::Flow, 0.7);
                distance += spacing;
            }
        }
        if game.phase != Phase::Intro || reveal_fraction > 0.95 {
            self.paint_gates(level);
        }
    }

    /// Entrance portals and the exit gate, where each path crosses the board.
    fn paint_gates(&mut self, level: &Level) {
        let inside = |p: (f32, f32)| {
            p.0 >= 0.4
                && p.0 <= level.width as f32 - 1.4
                && p.1 >= HUD_ROWS as f32 - 0.6
                && p.1 <= level.height as f32 - 1.4
        };
        for path in 0..level.paths.len() {
            let samples: Vec<_> = level.samples.iter().filter(|s| s.path == path).collect();
            let Some(first) = samples.iter().find(|sample| inside(sample.pos)) else {
                continue;
            };
            let Some(last) = samples.iter().rev().find(|sample| inside(sample.pos)) else {
                continue;
            };
            let entry = self.view.point(first.pos);
            let exit = self.view.point(last.pos);
            let radius = self.view.scale * 0.5;
            let pulse = 0.85 + 0.15 * (self.time * 3.0).sin();
            let diamond = Self::place(&regular_polygon(4, 0.0), entry, 0.0, radius * pulse);
            self.outline(&diamond, true, self.stroke, Role::Tint(Tint::Danger), 1.0);
            let inner = Self::place(
                &regular_polygon(4, 0.0),
                entry,
                PI / 4.0,
                radius * 0.5 * pulse,
            );
            self.outline(&inner, true, self.stroke, Role::Tint(Tint::Danger), 0.8);
            let gate = Self::place(&regular_polygon(4, PI / 4.0), exit, 0.0, radius * 0.85);
            self.outline(&gate, true, self.stroke, Role::Tint(Tint::Frost), 1.0);
            self.line(
                (exit.0 - radius * 0.4, exit.1),
                (exit.0 + radius * 0.4, exit.1),
                self.stroke,
                Role::Tint(Tint::Frost),
                0.8,
            );
        }
    }

    fn paint_range(&mut self, tower: &Tower) {
        if !self.settings.range_rings {
            return;
        }
        let center = self.view.point(tower.center());
        let radius = tower.range() * self.view.scale;
        let (tone, color) = self.ink(Role::Tint(Tint::Tower(tower.kind)), 0.4);
        let count = (radius / 3.0).ceil().max(12.0) as usize;
        let stroke = Stroke {
            width: self.stroke * 0.7,
            tone,
            color,
        };
        self.canvas
            .dashed_circle(center, radius, (count, 0.0), stroke);
    }

    fn paint_tower(&mut self, tower: &Tower, game: &Game) {
        let role = Role::Tint(Tint::Tower(tower.kind));
        let center = self.view.point(tower.center());
        let age = (game.time - tower.built_at).max(0.0);
        let grow = (age / 0.45).clamp(0.0, 1.0);
        let grow = grow * grow * (3.0 - 2.0 * grow);
        let unit = self.view.scale * 0.36 * grow * (1.0 + 0.05 * f32::from(tower.level - 1));
        let half = self.view.scale * 0.44 * grow;
        // Pedestal.
        let base = Self::place(
            &regular_polygon(4, PI / 4.0),
            center,
            0.0,
            half * std::f32::consts::SQRT_2,
        );
        self.fill(&base, role, 0.35);
        self.outline(&base, true, self.stroke, role, 0.55);
        let aim = tower.aim;
        let spin = self.time;
        match tower.kind {
            TowerKind::Pulse => {
                self.circle(center, unit * 0.58, self.stroke, role, 1.0);
                self.disk(center, unit * 0.24, role, 1.0);
                let tip = (
                    center.0 + aim.cos() * unit * 1.1,
                    center.1 + aim.sin() * unit * 1.1,
                );
                self.line(center, tip, self.stroke, role, 1.0);
            }
            TowerKind::Needle => {
                let shape = [(1.35, 0.0), (-0.7, 0.55), (-0.7, -0.55)];
                let points = Self::place(&shape, center, aim, unit);
                self.fill(&points, role, 0.7);
                self.outline(&points, true, self.stroke, role, 1.0);
            }
            TowerKind::Nova => {
                let hex = Self::place(&regular_polygon(6, 0.0), center, spin * 0.4, unit * 0.9);
                self.fill(&hex, role, 0.6);
                self.outline(&hex, true, self.stroke, role, 1.0);
                let pulse = 0.35 + 0.1 * (spin * 4.0).sin();
                self.circle(center, unit * pulse, self.stroke, role, 1.0);
            }
            TowerKind::Chill => {
                for arm in 0..3 {
                    let angle = spin * 0.5 + arm as f32 * PI / 3.0;
                    let (dx, dy) = (angle.cos() * unit * 0.95, angle.sin() * unit * 0.95);
                    self.line(
                        (center.0 - dx, center.1 - dy),
                        (center.0 + dx, center.1 + dy),
                        self.stroke,
                        role,
                        1.0,
                    );
                }
                self.circle(center, unit * 0.3, self.stroke, role, 0.9);
            }
            TowerKind::Arc => {
                let bolt = [(-0.45, -0.9), (0.15, -0.1), (-0.2, -0.1), (0.45, 0.9)];
                let points = Self::place(&bolt, center, 0.0, unit);
                self.outline(&points, false, self.stroke * 1.2, role, 1.0);
                let (tone, color) = self.ink(role, 0.55);
                let stroke = Stroke {
                    width: self.stroke * 0.8,
                    tone,
                    color,
                };
                self.canvas
                    .dashed_circle(center, unit * 1.05, (8, spin), stroke);
            }
            TowerKind::Lancer => {
                let shape = [(1.5, 0.0), (0.0, 0.42), (-0.8, 0.0), (0.0, -0.42)];
                let points = Self::place(&shape, center, aim, unit);
                self.fill(&points, role, 0.7);
                self.outline(&points, true, self.stroke, role, 1.0);
                let tip = (
                    center.0 + aim.cos() * unit * 1.9,
                    center.1 + aim.sin() * unit * 1.9,
                );
                self.line(center, tip, self.stroke, role, 0.9);
            }
            TowerKind::Hive => {
                for pod in 0..3 {
                    let angle = spin * 0.6 + pod as f32 * TAU / 3.0;
                    let spot = (
                        center.0 + angle.cos() * unit * 0.55,
                        center.1 + angle.sin() * unit * 0.55,
                    );
                    self.circle(spot, unit * 0.27, self.stroke, role, 1.0);
                }
                self.circle(center, unit * 0.95, self.stroke * 0.8, role, 0.5);
            }
            TowerKind::Beacon => {
                let diamond = Self::place(&regular_polygon(4, 0.0), center, 0.0, unit * 0.95);
                self.fill(&diamond, role, 0.7);
                self.outline(&diamond, true, self.stroke, role, 1.0);
                let phase = (spin * 0.5 + tower.cell.0 as f32 * 0.13).rem_euclid(1.0);
                let radius = unit + (tower.range() * self.view.scale - unit) * phase;
                self.circle(center, radius, self.stroke * 0.8, role, 0.7 * (1.0 - phase));
            }
        }
        // Upgrade pips.
        let pip_y = center.1 + self.view.scale * 0.62;
        let spacing = (self.view.scale * 0.2).max(2.0);
        let first = center.0 - spacing * (f32::from(tower.level) - 1.0) / 2.0;
        for pip in 0..tower.level {
            self.disk(
                (first + spacing * f32::from(pip), pip_y),
                self.stroke * 0.8,
                role,
                1.0,
            );
        }
        // Fully evolved towers wear a rotating ring.
        if tower.level >= game.max_tower_level() {
            let (tone, color) = self.ink(role, 0.75);
            let stroke = Stroke {
                width: self.stroke * 0.8,
                tone,
                color,
            };
            self.canvas
                .dashed_circle(center, self.view.scale * 0.66, (10, -spin * 0.8), stroke);
        }
        // Muzzle and upgrade flashes.
        if tower.fired < 0.1 && tower.kind != TowerKind::Beacon {
            self.disk(
                center,
                unit * 0.8 * (1.0 - tower.fired * 6.0).max(0.0),
                role,
                1.0,
            );
        }
        if tower.upgraded < 0.5 {
            let t = tower.upgraded / 0.5;
            self.circle(
                center,
                self.view.scale * (0.5 + 0.9 * t),
                self.stroke,
                role,
                1.0 - t,
            );
        }
    }

    fn monster_shape(kind: MonsterKind) -> Vec<(f32, f32)> {
        match kind {
            MonsterKind::Drone | MonsterKind::Shard => {
                vec![(1.0, 0.0), (-0.8, 0.85), (-0.8, -0.85)]
            }
            MonsterKind::Dart => vec![(1.4, 0.0), (-0.8, 0.5), (-0.4, 0.0), (-0.8, -0.5)],
            MonsterKind::Shell => vec![(0.9, 0.9), (-0.9, 0.9), (-0.9, -0.9), (0.9, -0.9)],
            MonsterKind::Splitter => regular_polygon(6, 0.0),
            MonsterKind::Swarm => regular_polygon(7, 0.0),
            MonsterKind::Wisp => vec![
                (1.0, 0.0),
                (-0.1, 0.3),
                (-0.9, 1.0),
                (-0.5, 0.0),
                (-0.9, -1.0),
                (-0.1, -0.3),
            ],
            MonsterKind::Boss => regular_polygon(8, PI / 8.0),
        }
    }

    fn paint_monster(&mut self, monster: &Monster, shadow: bool) {
        let role = Role::Tint(Tint::Monster(monster.kind));
        let mut center = self.view.point(monster.pos);
        if shadow {
            // Flyers cast a dim shadow on the board and are drawn above it.
            let offset = self.view.scale * 0.45;
            let radius = monster.kind.radius() * self.view.scale;
            let points = Self::place(
                &Self::monster_shape(monster.kind),
                (center.0 + offset * 0.4, center.1 + offset),
                monster.heading,
                radius,
            );
            self.outline(&points, true, self.stroke * 0.7, Role::Grid, 0.8);
            return;
        }
        if monster.kind.is_flying() {
            center.1 -= self.view.scale * 0.35;
        }
        let radius = monster.kind.radius() * self.view.scale;
        let fraction = (monster.hp / monster.max_hp).clamp(0.0, 1.0);
        let angle = if monster.kind == MonsterKind::Boss || monster.kind == MonsterKind::Swarm {
            monster.spin * 0.25
        } else {
            monster.heading
        };
        let points = Self::place(&Self::monster_shape(monster.kind), center, angle, radius);
        let fill_tone = if monster.hurt < 0.08 {
            1.4
        } else {
            0.18 + 0.7 * fraction
        };
        self.fill(&points, role, fill_tone);
        let width = if monster.kind == MonsterKind::Boss {
            self.stroke * 1.4
        } else {
            self.stroke
        };
        self.outline(&points, true, width, role, 1.0);
        match monster.kind {
            MonsterKind::Shell => {
                let inner = Self::place(
                    &Self::monster_shape(monster.kind),
                    center,
                    angle + PI / 4.0,
                    radius * 0.5,
                );
                self.outline(&inner, true, self.stroke, role, 0.8);
            }
            MonsterKind::Splitter => {
                self.line(
                    (center.0 - radius * 0.6, center.1 - radius * 0.6),
                    (center.0 + radius * 0.6, center.1 + radius * 0.6),
                    self.stroke,
                    role,
                    0.9,
                );
            }
            MonsterKind::Boss => {
                let inner = Self::place(
                    &regular_polygon(8, 0.0),
                    center,
                    -monster.spin * 0.5,
                    radius * 0.6,
                );
                self.outline(&inner, true, self.stroke, role, 0.9);
                for spike in 0..4 {
                    let a = monster.spin * 0.25 + spike as f32 * PI / 2.0;
                    self.line(
                        (center.0 + a.cos() * radius, center.1 + a.sin() * radius),
                        (
                            center.0 + a.cos() * radius * 1.35,
                            center.1 + a.sin() * radius * 1.35,
                        ),
                        self.stroke,
                        role,
                        1.0,
                    );
                }
                // Health bar.
                let bar_width = radius * 2.2;
                let left = center.0 - bar_width / 2.0;
                let y = center.1 - radius * 1.6;
                self.line(
                    (left, y),
                    (left + bar_width, y),
                    self.stroke * 0.8,
                    Role::Grid,
                    0.8,
                );
                self.line(
                    (left, y),
                    (left + bar_width * fraction, y),
                    self.stroke * 0.8,
                    role,
                    1.0,
                );
            }
            _ => {}
        }
        if monster.slow_factor < 1.0 {
            self.circle(
                center,
                radius * 1.45,
                self.stroke * 0.8,
                Role::Tint(Tint::Frost),
                0.8,
            );
        }
    }

    fn paint_projectile(&mut self, projectile: &Projectile) {
        let role = Role::Tint(Tint::Tower(projectile.source));
        let center = self.view.point(projectile.pos);
        let direction = (projectile.heading.cos(), projectile.heading.sin());
        match projectile.kind {
            ProjectileKind::Bolt => {
                let length = match projectile.source {
                    TowerKind::Needle => self.view.scale * 1.0,
                    _ => self.view.scale * 0.32,
                };
                self.line(
                    (
                        center.0 - direction.0 * length,
                        center.1 - direction.1 * length,
                    ),
                    center,
                    self.stroke,
                    role,
                    1.0,
                );
            }
            ProjectileKind::Shell => {
                self.disk(center, self.view.scale * 0.17, role, 1.0);
                self.circle(center, self.view.scale * 0.28, self.stroke * 0.7, role, 0.6);
            }
            ProjectileKind::Missile => {
                let length = self.view.scale * 0.55;
                for step in 0..3 {
                    let t = step as f32 / 3.0;
                    let from = (
                        center.0 - direction.0 * length * t,
                        center.1 - direction.1 * length * t,
                    );
                    let to = (
                        center.0 - direction.0 * length * (t + 0.28),
                        center.1 - direction.1 * length * (t + 0.28),
                    );
                    self.line(from, to, self.stroke, role, 1.0 - t * 0.8);
                }
            }
        }
    }

    fn paint_fx(&mut self, fx: &Fx) {
        let t = (fx.age / fx.life).clamp(0.0, 1.0);
        let role = Role::Tint(fx.tint);
        match fx.kind {
            FxKind::Ring => {
                let radius = fx.radius * self.view.scale * (1.0 - (1.0 - t) * (1.0 - t));
                let center = self.view.point(fx.from);
                let (tone, color) = self.ink(role, 0.9 * (1.0 - t));
                let count = (radius / 3.0).ceil().max(10.0) as usize;
                let stroke = Stroke {
                    width: self.stroke * 0.8,
                    tone,
                    color,
                };
                self.canvas
                    .dashed_circle(center, radius, (count, 0.0), stroke);
            }
            FxKind::Beam => {
                let from = self.view.point(fx.from);
                let to = self.view.point(fx.to);
                self.line(from, to, self.stroke * (2.2 - t * 1.4), role, 1.0 - t * 0.6);
            }
            FxKind::Zap => {
                let seed = (fx.age * 40.0) as u32;
                let mut previous = self.view.point(fx.points[0]);
                for (index, &point) in fx.points.iter().enumerate().skip(1) {
                    let target = self.view.point(point);
                    let steps = 4;
                    for step in 1..=steps {
                        let fraction = step as f32 / steps as f32;
                        let mut next = (
                            previous.0 + (target.0 - previous.0) * fraction,
                            previous.1 + (target.1 - previous.1) * fraction,
                        );
                        if step < steps {
                            let jitter = self.view.scale * 0.35;
                            next.0 += (hash_unit(seed + index as u32, step) - 0.5) * jitter;
                            next.1 += (hash_unit(step, seed + index as u32 * 7) - 0.5) * jitter;
                        }
                        self.line(previous, next, self.stroke, role, 1.0 - t * 0.5);
                        previous = next;
                    }
                }
            }
            FxKind::Burst => {
                let center = self.view.point(fx.from);
                let radius = (fx.radius * self.view.scale).max(self.view.scale * 0.4);
                let spokes = 8;
                for spoke in 0..spokes {
                    let angle =
                        TAU * spoke as f32 / spokes as f32 + hash_unit(spoke as u32, 3) * 0.5;
                    let inner = radius * (0.25 + 0.75 * t);
                    let outer = radius * (0.5 + 0.9 * t);
                    self.line(
                        (
                            center.0 + angle.cos() * inner,
                            center.1 + angle.sin() * inner,
                        ),
                        (
                            center.0 + angle.cos() * outer,
                            center.1 + angle.sin() * outer,
                        ),
                        self.stroke,
                        role,
                        1.0 - t,
                    );
                }
            }
            FxKind::Flash => {
                // A red frame around the board when a monster leaks.
                let (width, height) = (self.canvas.width() as f32, self.canvas.height() as f32);
                let corners = [
                    (1.0, 1.0),
                    (width - 2.0, 1.0),
                    (width - 2.0, height - 2.0),
                    (1.0, height - 2.0),
                ];
                self.outline(&corners, true, self.stroke * 2.0, role, 1.0 - t);
            }
        }
    }

    /// Hide the old level from the left, then reveal the new one.
    fn paint_wipe(&mut self, game: &Game) {
        let width = self.canvas.width() as f32;
        let height = self.canvas.height() as f32;
        match game.phase {
            Phase::Cleared | Phase::Defeat if game.phase_time > OUTRO_SECONDS - WIPE_SECONDS => {
                let fraction = (game.phase_time - (OUTRO_SECONDS - WIPE_SECONDS)) / WIPE_SECONDS;
                self.canvas
                    .clear_rect(0.0, 0.0, width * fraction.min(1.0), height);
            }
            Phase::Intro if game.phase_time < 0.9 => {
                let fraction = game.phase_time / 0.9;
                self.canvas.clear_rect(width * fraction, 0.0, width, height);
            }
            _ => {}
        }
    }

    fn text_scale(&self) -> i32 {
        if self.canvas.height() >= 120 {
            2
        } else {
            1
        }
    }

    fn text(&mut self, x: f32, y: f32, text: &str, scale: i32, role: Role, tone: f32) -> f32 {
        let (tone, color) = self.ink(role, tone);
        self.canvas.text(x, y, text, scale, tone, color)
    }

    fn paint_hud(&mut self, game: &Game) {
        let scale = self.text_scale();
        let width = self.canvas.width() as f32;
        let y = self.view.origin_y + self.view.scale * 0.35;
        let margin = self.view.origin_x + self.view.scale * 0.6;
        let level = format!("LEVEL {}", game.stage + 1);
        let used = self.text(margin, y, &level, scale, Role::Text, 1.0);
        self.text(
            margin + used + 4.0 * scale as f32,
            y,
            game.level.name,
            scale,
            Role::Tint(Tint::Frost),
            0.8,
        );

        let wave = format!(
            "WAVE {}/{}",
            game.wave.min(game.rules.waves),
            game.rules.waves
        );
        let wave_width = text_width(&wave, scale);
        let center = width / 2.0;
        self.text(center - wave_width / 2.0, y, &wave, scale, Role::Text, 1.0);
        // Countdown to the next wave.
        if game.phase == Phase::Running && game.wave < game.rules.waves {
            let fraction = (game.wave_timer / WAVE_GAP).clamp(0.0, 1.0);
            let bar_y = y + 6.0 * scale as f32 + 1.0;
            let left = center - wave_width / 2.0;
            self.line(
                (left, bar_y),
                (left + wave_width, bar_y),
                self.stroke * 0.7,
                Role::Grid,
                0.9,
            );
            self.line(
                (left, bar_y),
                (left + wave_width * fraction, bar_y),
                self.stroke * 0.7,
                Role::Tint(Tint::Money),
                1.0,
            );
            let next = model::wave_groups(game.wave + 1)
                .first()
                .map(|group| group.kind.label())
                .unwrap_or("");
            let label = format!("NEXT {next}");
            let label_width = text_width(&label, scale);
            self.text(
                center - label_width / 2.0,
                bar_y + 3.0,
                &label,
                scale,
                Role::Tint(Tint::Frost),
                0.7,
            );
        }

        let right = width - self.view.origin_x - self.view.scale * 0.6;
        let lives = format!("LIVES {}", game.lives.max(0));
        let lives_width = text_width(&lives, scale);
        let lives_role = if game.lives <= 6 {
            Tint::Danger
        } else {
            Tint::Frost
        };
        self.text(
            right - lives_width,
            y,
            &lives,
            scale,
            Role::Tint(lives_role),
            1.0,
        );
        let money = format!("${}", game.money.floor() as i64);
        let money_width = text_width(&money, scale);
        self.text(
            right - lives_width - 6.0 * scale as f32 - money_width,
            y,
            &money,
            scale,
            Role::Tint(Tint::Money),
            1.0,
        );
    }

    fn paint_banner(&mut self, banner: &Banner) {
        let fade_in = (banner.age / 0.35).clamp(0.0, 1.0);
        let fade_out = ((banner.life - banner.age) / 0.5).clamp(0.0, 1.0);
        let alpha = fade_in.min(fade_out);
        if alpha <= 0.0 {
            return;
        }
        let width = self.canvas.width() as f32;
        let height = self.canvas.height() as f32;
        let big = self.text_scale() * 2;
        let small = self.text_scale();
        let title_width = text_width(&banner.title, big);
        let subtitle_width = text_width(&banner.subtitle, small);
        let box_width = title_width.max(subtitle_width) + 10.0;
        let box_height = (5 * big + 5 * small + 8 + 6) as f32;
        let left = (width - box_width) / 2.0;
        let top = height * 0.38 - box_height / 2.0;
        if alpha > 0.4 {
            self.canvas
                .clear_rect(left, top, left + box_width, top + box_height);
        }
        self.text(
            (width - title_width) / 2.0,
            top + 4.0,
            &banner.title,
            big,
            Role::Text,
            alpha,
        );
        self.text(
            (width - subtitle_width) / 2.0,
            top + 4.0 + (5 * big) as f32 + 4.0,
            &banner.subtitle,
            small,
            Role::Tint(Tint::Money),
            alpha * 0.9,
        );
    }
}
