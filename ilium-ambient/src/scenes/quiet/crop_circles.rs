//! Progressive geometric crop-circle adaptations over a faint procedural field.
//! Milk Hill's photographed six curved circle chains (14 August 2001) and
//! Barbury Castle's triangle, central rings and corner disks (17 July 1991)
//! inspire bounded geometry, not exact reconstructions or agricultural claims.
//!
//! Shape sources (no image is shipped or fetched):
//! https://cropcircles.lucypringle.co.uk/photos/2001/uk2001df.shtml
//! https://www.cropcirclearchives.co.uk/archives/1991/barbry91.html

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::raster::Raster;
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, TAU};

const MAX_STROKES: usize = 160;
const MAX_POINTS: usize = 5000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CropPattern {
    #[default]
    Procedural,
    MilkHillSixArmed,
    BarburyTriangle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropCirclesSettings {
    pub pattern: CropPattern,
    pub seed: i32,
    /// Simultaneously moving tips, 2..=8.
    pub drawers: i32,
    pub drawing_speed: i32,
    pub scale: i32,
}

impl Default for CropCirclesSettings {
    fn default() -> Self {
        Self {
            pattern: CropPattern::Procedural,
            seed: 1,
            drawers: 3,
            drawing_speed: 45,
            scale: 78,
        }
    }
}

impl SceneSettings for CropCirclesSettings {
    fn normalized(&self) -> Self {
        Self {
            pattern: self.pattern,
            seed: self.seed.clamp(0, 9999),
            drawers: self.drawers.clamp(2, 8),
            drawing_speed: self.drawing_speed.clamp(10, 100),
            scale: self.scale.clamp(35, 100),
        }
    }

    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        vec![
            Control::choice(
                "pattern",
                "Pattern",
                settings.pattern as usize,
                &["Procedural", "Milk Hill six arms", "Barbury triangle"],
                "A new arrangement or geometric adaptations of two documented formations.",
            ),
            Control::slider(
                "seed",
                "Seed",
                settings.seed,
                (0, 9999, 1),
                "",
                "Changes the invented arrangement and small reference variations.",
            ),
            Control::slider(
                "drawers",
                "Visible drawers",
                settings.drawers,
                (2, 8, 1),
                "",
                "Independent tips advance concurrently on different assigned strokes.",
            ),
            Control::slider(
                "drawing_speed",
                "Drawing speed",
                settings.drawing_speed,
                (10, 100, 1),
                "%",
                "Shortens or lengthens the shared drawing interval.",
            ),
            Control::slider(
                "scale",
                "Formation scale",
                settings.scale,
                (35, 100, 1),
                "%",
                "Fits circles to terminal aspect without stretching them.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match id {
            "pattern" => {
                self.pattern = match control::index(&value)
                    .ok_or_else(|| "pattern expects a choice".to_owned())?
                    .min(2)
                {
                    0 => CropPattern::Procedural,
                    1 => CropPattern::MilkHillSixArmed,
                    _ => CropPattern::BarburyTriangle,
                };
            }
            "seed" | "drawers" | "drawing_speed" | "scale" => {
                let number =
                    control::number(&value).ok_or_else(|| format!("{id} expects a number"))?;
                match id {
                    "seed" => self.seed = number,
                    "drawers" => self.drawers = number,
                    "drawing_speed" => self.drawing_speed = number,
                    "scale" => self.scale = number,
                    _ => unreachable!("matched numeric controls above"),
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone)]
struct Stroke {
    points: Vec<Point>,
}
impl Stroke {
    fn segments(&self) -> usize {
        self.points.len().saturating_sub(1)
    }
}

#[derive(Debug, Clone)]
struct DrawerRoute {
    strokes: Vec<usize>,
    segments: usize,
}

fn random(seed: i32, index: u32) -> f32 {
    let mut bits = (seed as u32).wrapping_add(index.wrapping_mul(0x9e37_79b9));
    bits = (bits ^ (bits >> 16)).wrapping_mul(0x7feb_352d);
    bits = (bits ^ (bits >> 15)).wrapping_mul(0x846c_a68b);
    (bits ^ (bits >> 16)) as f32 / u32::MAX as f32
}

pub struct CropCirclesScene {
    settings: CropCirclesSettings,
    strokes: Vec<Stroke>,
    routes: Vec<DrawerRoute>,
}

impl CropCirclesScene {
    pub fn new(settings: &CropCirclesSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }

    fn from_settings(settings: &CropCirclesSettings) -> Self {
        let mut scene = Self {
            settings: settings.normalized(),
            strokes: Vec::new(),
            routes: Vec::new(),
        };
        scene.prepare();
        scene
    }

    fn add_stroke(&mut self, points: Vec<Point>) {
        if points.len() < 2 || self.strokes.len() >= MAX_STROKES {
            return;
        }
        let used: usize = self.strokes.iter().map(|stroke| stroke.points.len()).sum();
        if used + points.len() <= MAX_POINTS {
            self.strokes.push(Stroke { points });
        }
    }

    fn circle(&mut self, center: Point, radius: f32, segments: usize) {
        let segments = segments.clamp(12, 48);
        let points = (0..=segments)
            .map(|step| {
                let angle = TAU * step as f32 / segments as f32;
                Point {
                    x: center.x + radius * angle.cos(),
                    y: center.y + radius * angle.sin(),
                }
            })
            .collect();
        self.add_stroke(points);
    }

    fn line(&mut self, from: Point, to: Point) {
        self.add_stroke(vec![from, to]);
    }

    fn procedural(&mut self) {
        let seed = self.settings.seed;
        for ring in 0..5 {
            self.circle(Point { x: 0.0, y: 0.0 }, 0.12 + ring as f32 * 0.11, 40);
        }
        for index in 0..12 {
            let angle = TAU * (index as f32 + random(seed, index + 10) * 0.35) / 12.0;
            let distance = 0.52 + random(seed, index + 30) * 0.20;
            let radius = 0.035 + random(seed, index + 50) * 0.065;
            self.circle(
                Point {
                    x: angle.cos() * distance,
                    y: angle.sin() * distance,
                },
                radius,
                24,
            );
        }
        for ray in 0..8 {
            let angle = TAU * ray as f32 / 8.0 + random(seed, 70) * 0.18;
            self.line(
                Point {
                    x: angle.cos() * 0.13,
                    y: angle.sin() * 0.13,
                },
                Point {
                    x: angle.cos() * 0.48,
                    y: angle.sin() * 0.48,
                },
            );
        }
    }

    fn milk_hill_six_armed(&mut self) {
        // Six curling chains of shrinking circles and restrained satellites.
        let seed = self.settings.seed;
        for arm in 0..6 {
            for bead in 0..12 {
                let progress = bead as f32 / 11.0;
                let angle = arm as f32 * TAU / 6.0 + progress * 1.13 - 0.16;
                let distance = 0.11 + progress * 0.68;
                let center = Point {
                    x: angle.cos() * distance,
                    y: angle.sin() * distance,
                };
                let variation = 0.92 + random(seed, (arm * 32 + bead) as u32) * 0.16;
                let radius = (0.075 * (1.0 - progress) + 0.026) * variation;
                self.circle(center, radius, 24);
                if bead % 3 == 1 {
                    let side = if arm % 2 == 0 { 1.0 } else { -1.0 };
                    let satellite = Point {
                        x: center.x + side * angle.sin() * (radius + 0.036),
                        y: center.y - side * angle.cos() * (radius + 0.036),
                    };
                    self.circle(satellite, radius * 0.32, 16);
                }
            }
        }
    }

    fn barbury_triangle(&mut self) {
        // Central concentric disk and three corner circles preserve the
        // photographed large-scale relationships; spokes are simplified.
        let mut vertices = Vec::with_capacity(3);
        for corner in 0..3 {
            let angle = -FRAC_PI_2 + corner as f32 * TAU / 3.0;
            vertices.push(Point {
                x: angle.cos() * 0.77,
                y: angle.sin() * 0.77,
            });
        }
        for corner in 0..3 {
            self.line(vertices[corner], vertices[(corner + 1) % 3]);
        }
        for radius in [0.19, 0.31, 0.40] {
            self.circle(Point { x: 0.0, y: 0.0 }, radius, 48);
        }
        for (corner, center) in vertices.into_iter().enumerate() {
            self.circle(center, 0.17, 36);
            self.circle(center, 0.12, 30);
            for spoke in 0..3 {
                let angle = TAU * spoke as f32 / 3.0 + corner as f32 * 0.16;
                self.line(
                    Point {
                        x: center.x + angle.cos() * 0.04,
                        y: center.y + angle.sin() * 0.04,
                    },
                    Point {
                        x: center.x + angle.cos() * 0.16,
                        y: center.y + angle.sin() * 0.16,
                    },
                );
            }
        }
        for spoke in 0..6 {
            let angle = TAU * spoke as f32 / 6.0;
            self.line(
                Point {
                    x: angle.cos() * 0.20,
                    y: angle.sin() * 0.20,
                },
                Point {
                    x: angle.cos() * 0.39,
                    y: angle.sin() * 0.39,
                },
            );
        }
    }

    fn prepare(&mut self) {
        self.strokes.clear();
        match self.settings.pattern {
            CropPattern::Procedural => self.procedural(),
            CropPattern::MilkHillSixArmed => self.milk_hill_six_armed(),
            CropPattern::BarburyTriangle => self.barbury_triangle(),
        }
        self.routes = (0..self.settings.drawers as usize)
            .map(|_| DrawerRoute {
                strokes: Vec::new(),
                segments: 0,
            })
            .collect();
        let route_count = self.routes.len();
        for (index, stroke) in self.strokes.iter().enumerate() {
            let route = &mut self.routes[index % route_count];
            route.strokes.push(index);
            route.segments += stroke.segments();
        }
        for (route_index, route) in self.routes.iter_mut().enumerate() {
            let first_stroke = route_index * self.strokes.len() / route_count;
            let first_assigned = route
                .strokes
                .partition_point(|stroke_index| *stroke_index < first_stroke);
            route.strokes.rotate_left(first_assigned);
        }
    }

    fn phase(&self, seconds: f64) -> (f32, f32) {
        let draw = 1800.0 / f64::from(self.settings.drawing_speed);
        let hold = 5.0;
        let fade = 4.0;
        let phase = seconds.rem_euclid(draw + hold + fade);
        if phase < draw {
            ((phase / draw) as f32, 1.0)
        } else if phase < draw + hold {
            (1.0, 1.0)
        } else {
            (1.0, ((draw + hold + fade - phase) / fade) as f32)
        }
    }

    fn map_point(&self, point: Point, raster: &Raster) -> (f32, f32) {
        let aspect = raster.aspect().max(0.001);
        let radius = 0.43 * self.settings.scale as f32 / 100.0 * aspect.min(1.0);
        (0.5 + point.x * radius / aspect, 0.5 + point.y * radius)
    }

    fn route_tip(&self, route: &DrawerRoute, visible_segments: usize) -> Option<Point> {
        let mut remaining = visible_segments;
        for &stroke_index in &route.strokes {
            let stroke = &self.strokes[stroke_index];
            if remaining <= stroke.segments() {
                return stroke.points.get(remaining).copied();
            }
            remaining -= stroke.segments();
        }
        route
            .strokes
            .last()
            .and_then(|index| self.strokes[*index].points.last().copied())
    }

    fn draw_tip(&self, raster: &mut Raster, point: Point, intensity: f32) {
        let (u, v) = self.map_point(point, raster);
        let center_x = (u * raster.width as f32) as i32;
        let center_y = (v * raster.height as f32) as i32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let x = center_x + dx;
                let y = center_y + dy;
                if x < 0 || y < 0 {
                    continue;
                }
                let brightness = if dx == 0 && dy == 0 {
                    intensity
                } else {
                    intensity * 0.32
                };
                raster.owned_dot(x as usize, y as usize, brightness, 0);
            }
        }
    }
}

impl Scene for CropCirclesScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let raster = &mut frame.raster;
        raster.dots.fill(0.0);
        raster.owner_ids.fill(0);
        if raster.width == 0 || raster.height == 0 {
            return;
        }
        // Seeded field rows avoid asset I/O and per-pixel simulation.
        for row in 1..=15 {
            let y = row as f32 / 16.0;
            let slant = (random(self.settings.seed, row) - 0.5) * 0.018;
            raster.line((0.0, y), (1.0, y + slant), 0.25, 0.12);
        }
        let (progress, fade) = self.phase(frame.time.as_secs_f64());
        for route in &self.routes {
            if route.segments == 0 {
                continue;
            }
            let visible = (route.segments as f32 * progress).floor() as usize;
            let mut remaining = visible;
            for &stroke_index in &route.strokes {
                if remaining == 0 {
                    break;
                }
                let stroke = &self.strokes[stroke_index];
                let take = remaining.min(stroke.segments());
                for segment in 0..take {
                    let from = self.map_point(stroke.points[segment], raster);
                    let to = self.map_point(stroke.points[segment + 1], raster);
                    raster.line(from, to, 0.55, 0.70 * fade);
                }
                remaining -= take;
            }
            if let Some(point) = self.route_tip(route, visible) {
                self.draw_tip(raster, point, 0.96 * fade);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut CropCirclesScene, width: u16, height: u16, seconds: f64) -> Vec<f32> {
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::from_secs_f64(seconds),
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        });
        raster.dots
    }

    #[test]
    fn patterns_are_bounded_distinct_and_seekable() {
        let mut signatures = Vec::new();
        for pattern in [
            CropPattern::Procedural,
            CropPattern::MilkHillSixArmed,
            CropPattern::BarburyTriangle,
        ] {
            let settings = CropCirclesSettings {
                pattern,
                ..Default::default()
            };
            let mut scene = CropCirclesScene::from_settings(&settings);
            assert!(scene.strokes.len() <= MAX_STROKES);
            assert!(
                scene
                    .strokes
                    .iter()
                    .map(|stroke| stroke.points.len())
                    .sum::<usize>()
                    <= MAX_POINTS
            );
            assert!(scene
                .strokes
                .iter()
                .flat_map(|stroke| &stroke.points)
                .all(|point| point.x.is_finite()
                    && point.y.is_finite()
                    && point.x.abs() <= 1.0
                    && point.y.abs() <= 1.0));
            for (width, height) in [(0, 0), (1, 1), (80, 24), (240, 80)] {
                let image = render(&mut scene, width, height, 9.0);
                assert_eq!(image, render(&mut scene, width, height, 9.0));
                assert!(image
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            }
            signatures.push(render(&mut scene, 80, 24, 20.0));
        }
        assert_ne!(signatures[0], signatures[1]);
        assert_ne!(signatures[1], signatures[2]);
    }

    #[test]
    fn three_drawers_move_together_and_restart_after_fade() {
        let mut scene = CropCirclesScene::from_settings(&CropCirclesSettings::default());
        assert_eq!(scene.routes.len(), 3);
        assert!(scene.routes.iter().all(|route| route.segments > 0));
        let (early, _) = scene.phase(3.0);
        let (later, _) = scene.phase(9.0);
        let early_tips: Vec<_> = scene
            .routes
            .iter()
            .map(|route| {
                scene
                    .route_tip(route, (route.segments as f32 * early).floor() as usize)
                    .unwrap()
            })
            .collect();
        let later_tips: Vec<_> = scene
            .routes
            .iter()
            .map(|route| {
                scene
                    .route_tip(route, (route.segments as f32 * later).floor() as usize)
                    .unwrap()
            })
            .collect();
        assert_eq!(early_tips.len(), 3);
        assert!(early_tips.iter().zip(&later_tips).all(|(a, b)| a != b));
        assert!(early_tips.windows(2).all(|pair| pair[0] != pair[1]));
        assert_ne!(
            render(&mut scene, 100, 36, 3.0),
            render(&mut scene, 100, 36, 9.0)
        );
        let cycle = 1800.0 / f64::from(scene.settings.drawing_speed) + 9.0;
        assert_eq!(
            render(&mut scene, 100, 36, 3.0),
            render(&mut scene, 100, 36, cycle + 3.0)
        );
        assert!(scene.phase(cycle - 0.1).1 < 0.1);
    }

    #[test]
    fn concurrent_drawer_tips_remain_separate_after_ordered_dithering() {
        for pattern in [
            CropPattern::Procedural,
            CropPattern::MilkHillSixArmed,
            CropPattern::BarburyTriangle,
        ] {
            let settings = CropCirclesSettings {
                pattern,
                drawers: 3,
                ..Default::default()
            };
            for (columns, rows) in [(48_u16, 16_u16), (80, 24)] {
                for seconds in [3.0, 9.0, 17.0] {
                    let mut scene = CropCirclesScene::from_settings(&settings);
                    let mut raster = Raster::default();
                    raster.resize(usize::from(columns) * 2, usize::from(rows) * 4);
                    let mut colors = Vec::new();
                    scene.render(&mut Frame {
                        raster: &mut raster,
                        cell_colors: &mut colors,
                        width: columns,
                        height: rows,
                        time: Duration::from_secs_f64(seconds),
                        wall: Duration::ZERO,
                        now: SystemTime::UNIX_EPOCH,
                    });
                    let (progress, _) = scene.phase(seconds);
                    let visible_tips: Vec<_> = scene
                        .routes
                        .iter()
                        .filter_map(|route| {
                            let visible = (route.segments as f32 * progress).floor() as usize;
                            let point = scene.route_tip(route, visible)?;
                            let (u, v) = scene.map_point(point, &raster);
                            let center_x = (u * raster.width as f32) as i32;
                            let center_y = (v * raster.height as f32) as i32;
                            (-1..=1)
                                .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                                .filter_map(|(dx, dy)| {
                                    let x = center_x + dx;
                                    let y = center_y + dy;
                                    if x < 0 || y < 0 {
                                        return None;
                                    }
                                    let (x, y) = (x as usize, y as usize);
                                    if x >= raster.width || y >= raster.height {
                                        return None;
                                    }
                                    let index = y * raster.width + x;
                                    let threshold = crate::dither::threshold(
                                        x,
                                        y,
                                        crate::raster::DitherMode::Ordered,
                                    );
                                    (raster.dots[index] * 0.60 > threshold)
                                        .then_some((x as i32, y as i32))
                                })
                                .min_by_key(|(x, y)| (x - center_x).pow(2) + (y - center_y).pow(2))
                        })
                        .collect();
                    assert!(
                        visible_tips.len() >= 2,
                        "{pattern:?} at {columns}x{rows}, t={seconds}: only {} drawer tips survive Ordered dithering",
                        visible_tips.len()
                    );
                    let spaced = visible_tips.iter().enumerate().any(|(index, first)| {
                        visible_tips[index + 1..].iter().any(|second| {
                            (first.0 - second.0).pow(2) + (first.1 - second.1).pow(2) >= 16
                        })
                    });
                    assert!(
                        spaced,
                        "{pattern:?} at {columns}x{rows}, t={seconds}: drawer tips merge after Ordered dithering"
                    );
                }
            }
        }
    }

    #[test]
    fn references_have_six_chains_and_triangle_with_corner_rings() {
        let milk = CropCirclesScene::from_settings(&CropCirclesSettings {
            pattern: CropPattern::MilkHillSixArmed,
            ..Default::default()
        });
        assert!(milk.strokes.len() >= 6 * 12);
        let barbury = CropCirclesScene::from_settings(&CropCirclesSettings {
            pattern: CropPattern::BarburyTriangle,
            ..Default::default()
        });
        assert!(barbury.strokes.len() >= 3 + 3 + 3 * 2);
        assert!(barbury.strokes[..3]
            .iter()
            .all(|stroke| stroke.points.len() == 2));
    }

    #[test]
    fn controls_are_typed_normalized_and_persisted() {
        let mut settings = CropCirclesSettings::default();
        assert_eq!(
            serde_json::from_str::<CropCirclesSettings>("{}").unwrap(),
            settings
        );
        for row in settings.controls() {
            assert!(!row.help.is_empty());
            assert_eq!(settings.set_control(row.id, row.value), Ok(false));
        }
        assert_eq!(
            settings.set_control("drawers", ControlValue::Number(99)),
            Ok(true)
        );
        assert_eq!(settings.drawers, 8);
        assert_eq!(
            settings.set_control("pattern", ControlValue::Index(2)),
            Ok(true)
        );
        assert_eq!(settings.pattern, CropPattern::BarburyTriangle);
        assert!(settings
            .set_control("pattern", ControlValue::Bool(true))
            .is_err());
        assert_eq!(
            settings.set_control("missing", ControlValue::Number(1)),
            Ok(false)
        );
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<CropCirclesSettings>(&json).unwrap(),
            settings
        );
    }
}
