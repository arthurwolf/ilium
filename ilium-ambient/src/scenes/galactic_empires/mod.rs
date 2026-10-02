//! A procedural, dithered strategic galaxy: expansion, diplomacy and conquest.
mod settings;
pub use settings::GalacticEmpiresSettings;
mod camera;
mod simulation;
mod territory;

use crate::{control::SceneSettings, Frame, Scene, SceneEnv};
use camera::Camera;
use simulation::{Galaxy, TICK_SECONDS};
use std::time::Duration;
use territory::Territory;

// One foreground per Braille cell: reserve complete marker cells before drawing.
#[derive(Clone, Copy)]
struct Marker {
    from: (f32, f32),
    to: (f32, f32),
    radius: f32,
    intensity: f32,
    color: [u8; 3],
    rank: u8, // Star > fleet head > fleet tail; distance breaks same-rank ties.
}

impl Marker {
    fn distance_squared(&self, x: f32, y: f32) -> f32 {
        let dx = self.to.0 - self.from.0;
        let dy = self.to.1 - self.from.1;
        let length = dx * dx + dy * dy;
        let px = x - self.from.0;
        let py = y - self.from.1;
        let t = if length > 0.0001 {
            ((px * dx + py * dy) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (px - t * dx).powi(2) + (py - t * dy).powi(2)
    }
}

pub struct GalacticEmpiresScene {
    settings: GalacticEmpiresSettings,
    galaxy: Galaxy,
    territory: Territory,
    marker_cells: Vec<Option<(Marker, f32)>>,
    marker_primitives: Vec<Marker>,
    base_seed: u64,
    cycle: u64,
    seed_initialized: bool,
    last_wall: Duration,
    accumulator: f64,
    camera_time: f64,
}

impl GalacticEmpiresScene {
    pub fn new(settings: &GalacticEmpiresSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let seed = settings.seed.max(1) as u64;
        let galaxy = Galaxy::new(
            seed,
            settings.star_count as usize,
            settings.empire_count as usize,
        );
        let territory = Territory::new(&galaxy.stars);
        let seed_initialized = settings.seed != 0;
        Self {
            settings,
            galaxy,
            territory,
            marker_cells: Vec::new(),
            marker_primitives: Vec::new(),
            base_seed: seed,
            cycle: 0,
            seed_initialized,
            last_wall: Duration::ZERO,
            accumulator: 0.0,
            camera_time: 0.0,
        }
    }

    fn renew(&mut self, seed: u64) {
        self.galaxy = Galaxy::new(
            seed,
            self.settings.star_count as usize,
            self.settings.empire_count as usize,
        );
        self.territory = Territory::new(&self.galaxy.stars);
    }

    fn advance(&mut self, frame: &Frame<'_>) {
        if !self.seed_initialized {
            self.seed_initialized = true;
            // Frame owns the civil clock; the scene never reads a system clock.
            self.base_seed = frame
                .now
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map_or(1, |duration| duration.as_nanos() as u64)
                .max(1);
            self.renew(self.base_seed);
        }
        let wall_delta = frame.wall.saturating_sub(self.last_wall).as_secs_f64();
        self.last_wall = frame.wall;
        // Host global Speed edits rescale frame.time without rebuilding us.
        // Infer the current multiplier instead of subtracting scaled timestamps.
        let multiplier = if frame.wall.is_zero() {
            1.0
        } else {
            frame.time.as_secs_f64() / frame.wall.as_secs_f64()
        }
        .clamp(0.0, 10.0);
        let delta = wall_delta.min(4.0) * multiplier;
        self.camera_time += delta;
        self.accumulator += delta * f64::from(self.settings.simulation_speed) / 100.0;
        // Discard elapsed suspend time rather than monopolizing the UI thread.
        let ticks = (self.accumulator / TICK_SECONDS).floor().min(80.0) as usize;
        self.accumulator -= ticks as f64 * TICK_SECONDS;
        self.accumulator = self.accumulator.min(TICK_SECONDS);
        for _ in 0..ticks {
            self.galaxy.step();
            if self
                .galaxy
                .victory_tick
                .is_some_and(|tick| self.galaxy.tick - tick >= 60)
            {
                self.cycle += 1;
                self.renew(
                    self.base_seed
                        .wrapping_add(self.cycle.wrapping_mul(0x9e37_79b9_7f4a_7c15)),
                );
            }
        }
    }

    fn draw_field(&self, frame: &mut Frame<'_>, camera: &Camera) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        let shading = self.settings.territory_strength as f32 / 100.0;
        let pulse = (self.camera_time as f32 * 2.0).sin().abs();
        for cy in 0..usize::from(frame.height) {
            for cx in 0..usize::from(frame.width) {
                let center = camera.world(
                    (cx as f32 + 0.5) / f32::from(frame.width),
                    (cy as f32 + 0.5) / f32::from(frame.height),
                );
                let Some(cell) = self.territory.sample(center) else {
                    continue;
                };
                frame.cell_colors[cy * usize::from(frame.width) + cx] =
                    self.galaxy.empires[cell.owner].color;
                if shading == 0.0 {
                    continue;
                }
                for y in cy * 4..((cy + 1) * 4).min(height) {
                    for x in cx * 2..((cx + 1) * 2).min(width) {
                        let point = camera.world(
                            (x as f32 + 0.5) / width as f32,
                            (y as f32 + 0.5) / height as f32,
                        );
                        let Some(sample) = self.territory.sample(point) else {
                            continue;
                        };
                        // Do not display another owner's dots in this cell's hue.
                        if sample.owner != cell.owner {
                            continue;
                        }
                        let contact = sample.contact.map_or(0.0, |(other, band)| {
                            band * if self.galaxy.relations[sample.owner][other].war {
                                0.28 + 0.03 * pulse
                            } else {
                                0.20
                            }
                        });
                        frame.raster.dots[y * width + x] =
                            shading * sample.coverage * (0.22 + 0.35 * sample.contour + contact);
                    }
                }
            }
        }
    }

    fn queue_marker(
        frame: &Frame<'_>,
        cells: &mut [Option<(Marker, f32)>],
        primitives: &mut Vec<Marker>,
        mut m: Marker,
    ) {
        let w = frame.raster.width as f32;
        let h = frame.raster.height as f32;
        m.from = (m.from.0 * w, m.from.1 * h);
        m.to = (m.to.0 * w, m.to.1 * h);
        let outer = m.radius + 0.6;
        let left = ((m.from.0.min(m.to.0) - outer).max(0.0) / 2.0) as usize;
        let top = ((m.from.1.min(m.to.1) - outer).max(0.0) / 4.0) as usize;
        let right = (((m.from.0.max(m.to.0) + outer).max(0.0) / 2.0).ceil() as usize)
            .min(usize::from(frame.width));
        let bottom = (((m.from.1.max(m.to.1) + outer).max(0.0) / 4.0).ceil() as usize)
            .min(usize::from(frame.height));
        for cy in top..bottom {
            for cx in left..right {
                let mut distance = f32::INFINITY;
                for y in cy * 4..((cy + 1) * 4).min(frame.raster.height) {
                    for x in cx * 2..((cx + 1) * 2).min(frame.raster.width) {
                        distance = distance.min(m.distance_squared(x as f32 + 0.5, y as f32 + 0.5));
                    }
                }
                if distance >= outer * outer {
                    continue;
                }
                let slot = &mut cells[cy * usize::from(frame.width) + cx];
                let replace = match *slot {
                    None => true,
                    Some((old, old_distance)) => {
                        m.rank > old.rank || (m.rank == old.rank && distance < old_distance)
                    }
                };
                if replace {
                    *slot = Some((m, distance));
                }
            }
        }
        primitives.push(m);
    }

    fn paint_markers(
        frame: &mut Frame<'_>,
        cells: &[Option<(Marker, f32)>],
        primitives: &[Marker],
    ) {
        for (index, entry) in cells.iter().enumerate() {
            let Some((m, _)) = entry else {
                continue;
            };
            let cx = index % usize::from(frame.width);
            let cy = index / usize::from(frame.width);
            frame.cell_colors[index] = m.color;
            // Clear lower layers instead of recoloring their dots as this owner.
            for y in cy * 4..((cy + 1) * 4).min(frame.raster.height) {
                for x in cx * 2..((cx + 1) * 2).min(frame.raster.width) {
                    frame.raster.dots[y * frame.raster.width + x] = 0.0;
                }
            }
        }
        // Keep ALL compatible stars and fleet head/tail pieces, not just the
        // winning primitive. Only incompatible colors need to be suppressed.
        for m in primitives {
            let outer = m.radius + 0.6;
            let left = (m.from.0.min(m.to.0) - outer).max(0.0) as usize;
            let top = (m.from.1.min(m.to.1) - outer).max(0.0) as usize;
            let right =
                ((m.from.0.max(m.to.0) + outer).ceil().max(0.0) as usize).min(frame.raster.width);
            let bottom =
                ((m.from.1.max(m.to.1) + outer).ceil().max(0.0) as usize).min(frame.raster.height);
            for y in top..bottom {
                for x in left..right {
                    let cell = (y / 4) * usize::from(frame.width) + x / 2;
                    if cells[cell].is_none_or(|(winner, _)| winner.color != m.color) {
                        continue;
                    }
                    let coverage = (outer
                        - m.distance_squared(x as f32 + 0.5, y as f32 + 0.5).sqrt())
                    .clamp(0.0, 1.0);
                    let dot = &mut frame.raster.dots[y * frame.raster.width + x];
                    *dot = (*dot).max(coverage * m.intensity);
                }
            }
        }
    }
}

impl Scene for GalacticEmpiresScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.advance(frame);
        self.territory.sync(&self.galaxy.stars);
        frame.raster.dots.fill(0.0);
        frame.cell_colors.resize(
            usize::from(frame.width) * usize::from(frame.height),
            [96, 119, 148],
        );
        frame.cell_colors.fill([96, 119, 148]);
        if frame.width == 0
            || frame.height == 0
            || frame.raster.width == 0
            || frame.raster.height == 0
        {
            return;
        }
        let camera = Camera::new(
            frame.raster.aspect(),
            self.camera_time,
            self.settings.camera_speed,
        );
        self.draw_field(frame, &camera);
        for &(first, second) in &self.galaxy.lanes {
            let a = &self.galaxy.stars[first];
            let b = &self.galaxy.stars[second];
            let intensity = match (a.owner, b.owner) {
                (Some(first), Some(second))
                    if first != second && self.galaxy.relations[first][second].war =>
                {
                    0.72
                }
                (Some(first), Some(second)) if first == second => 0.60,
                _ => 0.52,
            };
            frame.raster.line(
                camera.project(a.position),
                camera.project(b.position),
                0.35,
                intensity,
            );
        }
        self.marker_cells.resize(frame.cell_colors.len(), None);
        self.marker_cells.fill(None);
        self.marker_primitives.clear();
        if self.settings.show_fleets {
            for fleet in &self.galaxy.fleets {
                let from = self.galaxy.stars[fleet.from].position;
                let to = self.galaxy.stars[fleet.to].position;
                let position = (
                    from.0 + (to.0 - from.0) * fleet.progress,
                    from.1 + (to.1 - from.1) * fleet.progress,
                );
                let projected = camera.project(position);
                let tail = camera.project((
                    from.0 + (to.0 - from.0) * (fleet.progress - 0.08).max(0.0),
                    from.1 + (to.1 - from.1) * (fleet.progress - 0.08).max(0.0),
                ));
                let color = self.galaxy.empires[fleet.owner].color;
                Self::queue_marker(
                    frame,
                    &mut self.marker_cells,
                    &mut self.marker_primitives,
                    Marker {
                        from: tail,
                        to: projected,
                        radius: 0.35,
                        intensity: 0.65,
                        color,
                        rank: 1,
                    },
                );
                Self::queue_marker(
                    frame,
                    &mut self.marker_cells,
                    &mut self.marker_primitives,
                    Marker {
                        from: projected,
                        to: projected,
                        radius: if fleet.campaign { 1.1 } else { 0.8 },
                        intensity: 1.0,
                        color,
                        rank: 2,
                    },
                );
            }
        }
        for (index, star) in self.galaxy.stars.iter().enumerate() {
            let color = star
                .owner
                .map_or([184, 186, 178], |owner| self.galaxy.empires[owner].color);
            let light = color.map(|channel| (u16::from(channel) + 255).div_ceil(2) as u8);
            let point = camera.project(star.position);
            let captured = self
                .galaxy
                .last_captures
                .iter()
                .any(|&(_, to, _)| to == index);
            Self::queue_marker(
                frame,
                &mut self.marker_cells,
                &mut self.marker_primitives,
                Marker {
                    from: point,
                    to: point,
                    radius: if captured { 1.5 } else { 1.2 },
                    intensity: 1.0,
                    color: light,
                    rank: 3,
                },
            );
        }
        Self::paint_markers(frame, &self.marker_cells, &self.marker_primitives);
    }

    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        if let Some(winner) = self.galaxy.winner {
            return Some(format!(
                "Empire {} united all {} stars · new galaxy soon",
                winner + 1,
                self.galaxy.stars.len()
            ));
        }
        let mut alive = vec![false; self.galaxy.empires.len()];
        for star in &self.galaxy.stars {
            if let Some(owner) = star.owner {
                alive[owner] = true;
            }
        }
        let wars = self
            .galaxy
            .relations
            .iter()
            .enumerate()
            .filter(|(index, _)| alive[*index])
            .map(|(index, row)| {
                row.iter()
                    .enumerate()
                    .skip(index + 1)
                    .filter(|(other, relation)| alive[*other] && relation.war)
                    .count()
            })
            .sum::<usize>();
        Some(format!(
            "{} empires · {} wars · {} fleets · galaxy {}{}",
            self.galaxy.living_empires(),
            wars,
            self.galaxy.fleets.len(),
            self.cycle + 1,
            if self.galaxy.hegemon.is_some() {
                " · final campaign"
            } else {
                ""
            }
        ))
    }
}

#[cfg(test)]
mod tests;
