//! Bounded frost paths from either the field frame or content-free text anchors.
//! A character anchor says where text is, never what it contains. The separate
//! occupied bit is an exclusion zone for every dot of generated frost.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, OccupancyMask, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

const MAX_SEEDS: usize = 32;
const MAX_DOTS: usize = 8192;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrostMode {
    #[default]
    FrameEdges,
    Characters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrostSettings {
    pub mode: FrostMode,
    /// Maximum reach in approximate terminal-cell units, 2..=20.
    pub reach: i32,
    /// Number of branch tiers, 0..=3.
    pub branching: i32,
    /// Seconds spent growing and again retreating, 3..=30.
    pub growth_seconds: i32,
    pub seed: i32,
}

impl Default for FrostSettings {
    fn default() -> Self {
        Self {
            mode: FrostMode::FrameEdges,
            reach: 9,
            branching: 2,
            growth_seconds: 10,
            seed: 1,
        }
    }
}

impl SceneSettings for FrostSettings {
    fn normalized(&self) -> Self {
        Self {
            mode: self.mode,
            reach: self.reach.clamp(2, 20),
            branching: self.branching.clamp(0, 3),
            growth_seconds: self.growth_seconds.clamp(3, 30),
            seed: self.seed.clamp(0, 9999),
        }
    }

    fn controls(&self) -> Vec<Control> {
        let settings = self.normalized();
        vec![
            Control::choice(
                "mode",
                "Growth source",
                settings.mode as usize,
                &["Frame edges", "Foreground characters"],
                "Start at the outer frame or beside visible text, without reading its content.",
            ),
            Control::slider(
                "reach",
                "Reach",
                settings.reach,
                (2, 20, 1),
                " cells",
                "Maximum distance a crystalline branch travels from its source.",
            ),
            Control::slider(
                "branching",
                "Branching",
                settings.branching,
                (0, 3, 1),
                "",
                "Small side twigs along each cached main branch.",
            ),
            Control::slider(
                "growth_seconds",
                "Growth time",
                settings.growth_seconds,
                (3, 30, 1),
                " s",
                "Grow, hold briefly, retreat, then begin again.",
            ),
            Control::slider(
                "seed",
                "Seed",
                settings.seed,
                (0, 9999, 1),
                "",
                "Repeatable branch placement and crystalline bends.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match id {
            "mode" => {
                self.mode = match control::index(&value)
                    .ok_or_else(|| "mode expects a choice".to_owned())?
                    .min(1)
                {
                    0 => FrostMode::FrameEdges,
                    _ => FrostMode::Characters,
                };
            }
            "reach" | "branching" | "growth_seconds" | "seed" => {
                let number =
                    control::number(&value).ok_or_else(|| format!("{id} expects a number"))?;
                match id {
                    "reach" => self.reach = number,
                    "branching" => self.branching = number,
                    "growth_seconds" => self.growth_seconds = number,
                    "seed" => self.seed = number,
                    _ => unreachable!("matched numeric control ids above"),
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

#[derive(Clone, Copy, Debug)]
struct FrostDot {
    x: usize,
    y: usize,
    reveal: f32,
    brightness: f32,
}

#[derive(Clone, Copy)]
struct Seed {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
}

pub struct FrostScene {
    settings: FrostSettings,
    occupancy: Option<OccupancyMask>,
    cached_size: Option<(u16, u16)>,
    cache_dirty: bool,
    dots: Vec<FrostDot>,
}

impl FrostScene {
    pub fn new(settings: &FrostSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }

    fn from_settings(settings: &FrostSettings) -> Self {
        Self {
            settings: settings.normalized(),
            occupancy: None,
            cached_size: None,
            cache_dirty: true,
            dots: Vec::new(),
        }
    }

    fn frame_seeds(&self, width: u16, height: u16) -> Vec<Seed> {
        let mut seeds = Vec::with_capacity(MAX_SEEDS);
        let dot_width = usize::from(width) * 2;
        let dot_height = usize::from(height) * 4;
        if dot_width == 0 || dot_height == 0 {
            return seeds;
        }
        // Spread the finite seed budget over all four edges. Near-edge starts
        // remain visible behind a one-cell workspace border.
        let horizontal_step = (dot_width / 9).max(2);
        let vertical_step = (dot_height / 9).max(2);
        for x in (0..dot_width).step_by(horizontal_step) {
            if seeds.len() + 2 > MAX_SEEDS {
                break;
            }
            seeds.push(Seed {
                x: x as f32,
                y: 2.0,
                dx: 0.0,
                dy: 1.0,
            });
            seeds.push(Seed {
                x: x as f32,
                y: dot_height.saturating_sub(3) as f32,
                dx: 0.0,
                dy: -1.0,
            });
        }
        for y in (0..dot_height).step_by(vertical_step) {
            if seeds.len() + 2 > MAX_SEEDS {
                break;
            }
            seeds.push(Seed {
                x: 2.0,
                y: y as f32,
                dx: 1.0,
                dy: 0.0,
            });
            seeds.push(Seed {
                x: dot_width.saturating_sub(3) as f32,
                y: y as f32,
                dx: -1.0,
                dy: 0.0,
            });
        }
        seeds
    }

    fn character_seeds(&self, width: u16, height: u16) -> Vec<Seed> {
        let mut seeds = Vec::with_capacity(MAX_SEEDS);
        let Some(mask) = self
            .occupancy
            .as_ref()
            .filter(|mask| mask.width() == width && mask.height() == height)
        else {
            return seeds;
        };
        let neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        let mut seen = 0_usize;
        for row in 0..height {
            for column in 0..width {
                if !mask.is_character(i32::from(column), i32::from(row)) {
                    continue;
                }
                let Some((dx, dy)) = neighbors.into_iter().find(|(dx, dy)| {
                    !mask.is_occupied(i32::from(column) + *dx, i32::from(row) + *dy)
                }) else {
                    continue;
                };
                seen += 1;
                let seed = Seed {
                    x: f32::from(column) * 2.0 + 0.5 + dx as f32 * 1.5,
                    y: f32::from(row) * 4.0 + 1.5 + dy as f32 * 2.5,
                    dx: dx as f32,
                    dy: dy as f32,
                };
                if seeds.len() < MAX_SEEDS {
                    seeds.push(seed);
                } else {
                    // Deterministic reservoir sampling covers a text field
                    // without retaining all its potentially numerous cells.
                    let slot = (crate::raster::hash(
                        i32::from(column) ^ self.settings.seed,
                        i32::from(row),
                    ) * seen as f32) as usize;
                    if slot < MAX_SEEDS {
                        seeds[slot] = seed;
                    }
                }
            }
        }
        seeds
    }

    fn push_dot(
        &mut self,
        x: f32,
        y: f32,
        reveal: f32,
        brightness: f32,
        dot_width: usize,
        dot_height: usize,
    ) {
        if self.dots.len() >= MAX_DOTS || !x.is_finite() || !y.is_finite() {
            return;
        }
        let x = x.round() as i32;
        let y = y.round() as i32;
        if x < 0 || y < 0 || x >= dot_width as i32 || y >= dot_height as i32 {
            return;
        }
        if self.settings.mode == FrostMode::Characters
            && self
                .occupancy
                .as_ref()
                .is_none_or(|mask| mask.is_occupied(x / 2, y / 4))
        {
            return;
        }
        self.dots.push(FrostDot {
            x: x as usize,
            y: y as usize,
            reveal: reveal.clamp(0.0, 1.0),
            brightness: brightness.clamp(0.0, 1.0),
        });
    }

    fn rebuild(&mut self, width: u16, height: u16) {
        self.dots.clear();
        self.cached_size = Some((width, height));
        self.cache_dirty = false;
        let dot_width = usize::from(width) * 2;
        let dot_height = usize::from(height) * 4;
        if dot_width == 0 || dot_height == 0 {
            return;
        }
        let seeds = match self.settings.mode {
            FrostMode::FrameEdges => self.frame_seeds(width, height),
            FrostMode::Characters => self.character_seeds(width, height),
        };
        let reach = self.settings.reach as usize * 3;
        let branch_interval = 8_usize;
        for (index, seed) in seeds.into_iter().enumerate() {
            if self.dots.len() >= MAX_DOTS {
                break;
            }
            let bend_phase = (self.settings.seed as f32 * 0.19 + index as f32 * 1.61).sin();
            let perpendicular = (-seed.dy, seed.dx);
            for step in 0..=reach {
                let travel = step as f32;
                let bend = 0.9 * (travel * 0.25 + bend_phase).sin();
                let x = seed.x + seed.dx * travel + perpendicular.0 * bend;
                let y = seed.y + seed.dy * travel + perpendicular.1 * bend;
                let reveal = step as f32 / reach as f32 * 0.88;
                self.push_dot(x, y, reveal, 0.66, dot_width, dot_height);
                if self.settings.branching == 0 || step == 0 || step % branch_interval != 0 {
                    continue;
                }
                let fork_length = self.settings.branching as usize * 2 + 2;
                for side in [-1.0_f32, 1.0] {
                    for fork_step in 1..=fork_length {
                        let advance = fork_step as f32;
                        self.push_dot(
                            x + seed.dx * advance * 0.45 + perpendicular.0 * side * advance * 0.8,
                            y + seed.dy * advance * 0.45 + perpendicular.1 * side * advance * 0.8,
                            reveal + advance / reach as f32 * 0.1,
                            0.47,
                            dot_width,
                            dot_height,
                        );
                    }
                }
            }
        }
    }

    fn visible_fraction(&self, seconds: f64) -> f32 {
        let growth = self.settings.growth_seconds as f64;
        let phase = seconds.rem_euclid(growth * 2.0 + 3.0);
        if phase < growth {
            (phase / growth) as f32
        } else if phase < growth + 3.0 {
            1.0
        } else {
            (1.0 - (phase - growth - 3.0) / growth) as f32
        }
    }
}

impl Scene for FrostScene {
    fn wants_occupancy(&self) -> bool {
        self.settings.mode == FrostMode::Characters
    }

    fn occupancy(&mut self, mask: &OccupancyMask) {
        if self.settings.mode == FrostMode::Characters && self.occupancy.as_ref() != Some(mask) {
            self.occupancy = Some(mask.clone());
            self.cache_dirty = true;
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.0);
        frame.raster.owner_ids.fill(0);
        if self.cache_dirty || self.cached_size != Some((frame.width, frame.height)) {
            self.rebuild(frame.width, frame.height);
        }
        let visible = self.visible_fraction(frame.time.as_secs_f64());
        if visible <= 0.0 {
            return;
        }
        for dot in &self.dots {
            if dot.reveal <= visible {
                let fresh = (1.0 - (visible - dot.reveal) * 2.0).clamp(0.0, 1.0);
                frame
                    .raster
                    .owned_dot(dot.x, dot.y, dot.brightness + fresh * 0.23, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut FrostScene, width: u16, height: u16, seconds: f64) -> Vec<f32> {
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
    fn frame_edges_grow_hold_retreat_and_seek_at_small_and_large_sizes() {
        let mut scene = FrostScene::from_settings(&FrostSettings::default());
        assert!(!scene.wants_occupancy());
        for (width, height) in [(0, 0), (1, 1), (80, 24), (240, 80)] {
            let first = render(&mut scene, width, height, 6.0);
            assert_eq!(first, render(&mut scene, width, height, 6.0));
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert!(scene.dots.len() <= MAX_DOTS);
        }
        let beginning = render(&mut scene, 80, 24, 0.0);
        let growing = render(&mut scene, 80, 24, 5.0);
        let retreating = render(&mut scene, 80, 24, 22.0);
        assert!(beginning.iter().all(|value| *value == 0.0));
        assert!(growing.iter().any(|value| *value > 0.0));
        assert!(
            retreating.iter().filter(|value| **value > 0.0).count()
                < growing.iter().filter(|value| **value > 0.0).count()
        );
    }

    #[test]
    fn character_mode_uses_only_matching_anchors_and_never_occupied_cells() {
        let settings = FrostSettings {
            mode: FrostMode::Characters,
            ..Default::default()
        };
        let mut scene = FrostScene::from_settings(&settings);
        assert!(scene.wants_occupancy());
        assert!(render(&mut scene, 30, 12, 8.0)
            .iter()
            .all(|value| *value == 0.0));
        let mut wrong_size = OccupancyMask::empty(29, 12);
        wrong_size.set_character(10, 5, true);
        scene.occupancy(&wrong_size);
        assert!(render(&mut scene, 30, 12, 8.0)
            .iter()
            .all(|value| *value == 0.0));
        let mut full = OccupancyMask::from_fn(30, 12, |_, _| true);
        full.set_character(10, 5, true);
        scene.occupancy(&full);
        assert!(render(&mut scene, 30, 12, 8.0)
            .iter()
            .all(|value| *value == 0.0));
        let mut mask = OccupancyMask::empty(30, 12);
        mask.set_character(10, 5, true);
        mask.set(11, 5, true);
        scene.occupancy(&mask);
        let image = render(&mut scene, 30, 12, 8.0);
        assert!(image.iter().any(|value| *value > 0.0));
        for y in 0..48 {
            for x in 0..60 {
                if mask.is_occupied(x / 2, y / 4) {
                    assert_eq!(image[y as usize * 60 + x as usize], 0.0);
                }
            }
        }
        assert_eq!(image, render(&mut scene, 30, 12, 8.0));
        assert!(scene.dots.len() <= MAX_DOTS);
    }

    #[test]
    fn typed_mode_controls_and_serialization() {
        let mut settings = FrostSettings::default();
        assert_eq!(
            serde_json::from_str::<FrostSettings>("{}").unwrap(),
            settings
        );
        for row in settings.controls() {
            assert_eq!(settings.set_control(row.id, row.value), Ok(false));
        }
        assert_eq!(
            settings.set_control("mode", ControlValue::Index(1)),
            Ok(true)
        );
        assert_eq!(settings.mode, FrostMode::Characters);
        assert!(settings
            .set_control("mode", ControlValue::Bool(true))
            .is_err());
        assert_eq!(
            settings.set_control("reach", ControlValue::Number(999)),
            Ok(true)
        );
        assert_eq!(settings.reach, 20);
        assert_eq!(
            settings.set_control("missing", ControlValue::Number(1)),
            Ok(false)
        );
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<FrostSettings>(&json).unwrap(),
            settings
        );
    }
}
