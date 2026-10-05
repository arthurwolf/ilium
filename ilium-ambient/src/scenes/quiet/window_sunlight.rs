//! Bounded procedural window sunlight; no assets, workers or frame history.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowGrid {
    #[default]
    TwoByTwo,
    TwoByThree,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSunlightSettings {
    /// Independent projections arranged across the field. Range 1..=4.
    pub window_count: i32,
    /// Projection width within each window slot. Range 15..=90.
    pub size: i32,
    /// Height of each projected window. Range 15..=85.
    pub height: i32,
    /// Dark separation between panes; pollen never lights in these gaps. Range 3..=35.
    pub gap: i32,
    /// Moves the entire group of projections sideways. Range 0..=100.
    pub location_x: i32,
    /// Moves projections up or down. Range 10..=90.
    pub location_y: i32,
    /// Horizontal skew from the top to bottom of each projection. Range -60..=60.
    pub shear: i32,
    /// Slow movement and stretching of the projections. Range 0..=100.
    pub motion: i32,
    /// Small seeded population revealed only inside illuminated panes. Range 0..=128.
    pub pollen_count: i32,
    /// Selects pollen paths and subtle pane texture. Range 0..=9999.
    pub seed: i32,
    pub grid: WindowGrid,
    pub pollen: bool,
}

impl Default for WindowSunlightSettings {
    fn default() -> Self {
        Self {
            window_count: 1,
            size: 65,
            height: 48,
            gap: 12,
            location_x: 50,
            location_y: 50,
            shear: 20,
            motion: 20,
            pollen_count: 40,
            seed: 1,
            grid: WindowGrid::TwoByTwo,
            pollen: false,
        }
    }
}

impl SceneSettings for WindowSunlightSettings {
    fn normalized(&self) -> Self {
        Self {
            window_count: self.window_count.clamp(1, 4),
            size: self.size.clamp(15, 90),
            height: self.height.clamp(15, 85),
            gap: self.gap.clamp(3, 35),
            location_x: self.location_x.clamp(0, 100),
            location_y: self.location_y.clamp(10, 90),
            shear: self.shear.clamp(-60, 60),
            motion: self.motion.clamp(0, 100),
            pollen_count: self.pollen_count.clamp(0, 128),
            seed: self.seed.clamp(0, 9999),
            grid: self.grid,
            pollen: self.pollen,
        }
    }
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "window_count",
                "Windows",
                self.window_count,
                (1, 4, 1),
                "",
                "Independent projections arranged across the field.",
            ),
            Control::slider(
                "size",
                "Window size",
                self.size,
                (15, 90, 1),
                "",
                "Projection width within each window slot.",
            ),
            Control::slider(
                "height",
                "Window height",
                self.height,
                (15, 85, 1),
                "",
                "Height of each projected window.",
            ),
            Control::slider(
                "gap",
                "Pane gap",
                self.gap,
                (3, 35, 1),
                "",
                "Dark separation between panes; pollen never lights in these gaps.",
            ),
            Control::slider(
                "location_x",
                "Horizontal location",
                self.location_x,
                (0, 100, 1),
                "",
                "Moves the entire group of projections sideways.",
            ),
            Control::slider(
                "location_y",
                "Vertical location",
                self.location_y,
                (10, 90, 1),
                "",
                "Moves projections up or down.",
            ),
            Control::slider(
                "shear",
                "Projection slant",
                self.shear,
                (-60, 60, 1),
                "",
                "Horizontal skew from the top to bottom of each projection.",
            ),
            Control::slider(
                "motion",
                "Sun motion",
                self.motion,
                (0, 100, 1),
                "",
                "Slow movement and stretching of the projections.",
            ),
            Control::slider(
                "pollen_count",
                "Pollen count",
                self.pollen_count,
                (0, 128, 1),
                "",
                "Small seeded population revealed only inside illuminated panes.",
            ),
            Control::slider(
                "seed",
                "Seed",
                self.seed,
                (0, 9999, 1),
                "",
                "Selects pollen paths and subtle pane texture.",
            ),
            Control::choice(
                "grid",
                "Pane grid",
                usize::from(self.grid == WindowGrid::TwoByThree),
                &["2 x 2", "2 x 3"],
                "Two columns with two or three rows of separate light panes.",
            ),
            Control::toggle(
                "pollen",
                "Pollen",
                self.pollen,
                "Reveal drifting pollen only within lit panes, preserving dark mullions.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        if id == "grid" {
            let grid = match control::index(&value) {
                Some(0) => WindowGrid::TwoByTwo,
                Some(1) => WindowGrid::TwoByThree,
                _ => return Err("Pane grid expects 2 x 2 or 2 x 3".to_owned()),
            };
            let changed = self.grid != grid;
            self.grid = grid;
            return Ok(changed);
        }
        if id == "pollen" {
            let pollen =
                control::boolean(&value).ok_or_else(|| "Pollen expects On or Off".to_owned())?;
            let changed = self.pollen != pollen;
            self.pollen = pollen;
            return Ok(changed);
        }
        let (field, min, max) = match id {
            "window_count" => (&mut self.window_count, 1, 4),
            "size" => (&mut self.size, 15, 90),
            "height" => (&mut self.height, 15, 85),
            "gap" => (&mut self.gap, 3, 35),
            "location_x" => (&mut self.location_x, 0, 100),
            "location_y" => (&mut self.location_y, 10, 90),
            "shear" => (&mut self.shear, -60, 60),
            "motion" => (&mut self.motion, 0, 100),
            "pollen_count" => (&mut self.pollen_count, 0, 128),
            "seed" => (&mut self.seed, 0, 9999),
            _ => return Ok(false),
        };
        let number = control::number(&value)
            .ok_or_else(|| format!("{id} expects a number"))?
            .clamp(min, max);
        let changed = *field != number;
        *field = number;
        Ok(changed)
    }
}

pub struct WindowSunlightScene {
    settings: WindowSunlightSettings,
}

impl WindowSunlightScene {
    pub fn new(settings: &WindowSunlightSettings, _env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
        }
    }
}

// Integer mixing makes seeks independent of prior renders or random state.
fn random(seed: i32, index: u32) -> f32 {
    let mut value = (seed as u32).wrapping_add(index.wrapping_mul(0x9e3779b9));
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846ca68b);
    (value ^ (value >> 16)) as f32 / u32::MAX as f32
}

fn particle(seed: i32, index: u32, time: f32, drift: f32) -> (f32, f32) {
    let phase = random(seed, index * 4 + 2) * std::f32::consts::TAU;
    let x = (random(seed, index * 4)
        + time * 0.012 * drift
        + 0.025 * (time * 0.4 + phase).sin() * drift)
        .rem_euclid(1.0);
    let y = (random(seed, index * 4 + 1) - time * 0.008 * drift
        + 0.02 * (time * 0.3 + phase).cos() * drift)
        .rem_euclid(1.0);
    (x, y)
}

fn masked_dot(
    raster: &mut crate::raster::Raster,
    position: (f32, f32),
    intensity: f32,
    mut mask: impl FnMut(f32, f32) -> bool,
) {
    let center_x = (position.0 * raster.width as f32) as i32;
    let center_y = (position.1 * raster.height as f32) as i32;
    // At most nine writes per particle. Test the mask per dot, including halos.
    for dy in -1..=1 {
        for dx in -1..=1 {
            let x = center_x + dx;
            let y = center_y + dy;
            if x < 0 || y < 0 || x >= raster.width as i32 || y >= raster.height as i32 {
                continue;
            }
            let u = (x as f32 + 0.5) / raster.width as f32;
            let v = (y as f32 + 0.5) / raster.height as f32;
            if mask(u, v) {
                let falloff = if dx == 0 && dy == 0 { 1.0 } else { 0.22 };
                raster.owned_dot(x as usize, y as usize, intensity * falloff, 0);
            }
        }
    }
}

impl WindowSunlightSettings {
    fn pane_light(&self, x: f32, y: f32, time: f32) -> f32 {
        let count = self.window_count as f32;
        let motion = self.motion as f32 / 100.0;
        let width = self.size as f32 / 100.0 / count;
        let height = self.height as f32 / 100.0 * (1.0 + 0.06 * motion * (time * 0.11).sin());
        let center_y = self.location_y as f32 / 100.0 + motion * 0.03 * (time * 0.1).sin();
        let v = (y - center_y) / height + 0.5;
        if !(0.0..1.0).contains(&v) {
            return 0.0;
        }
        let rows = match self.grid {
            WindowGrid::TwoByTwo => 2.0,
            WindowGrid::TwoByThree => 3.0,
        };
        let row_position = (v * rows).fract();
        let gap = self.gap as f32 / 200.0;
        if row_position <= gap || row_position >= 1.0 - gap {
            return 0.0;
        }
        for index in 0..self.window_count {
            let center_x = (index as f32 + 0.5) / count
                + (self.location_x as f32 - 50.0) / 150.0
                + motion * 0.15 * (time * 0.09 + index as f32 * 0.35).sin();
            let shear = self.shear as f32 / 100.0 * (y - center_y);
            let u = (x - center_x - shear) / width + 0.5;
            if !(0.0..1.0).contains(&u) {
                continue;
            }
            let column_position = (u * 2.0).fract();
            if column_position > gap && column_position < 1.0 - gap {
                let edge = ((row_position - gap)
                    .min(1.0 - gap - row_position)
                    .min(column_position - gap)
                    .min(1.0 - gap - column_position)
                    * 25.0)
                    .min(1.0);
                return 0.42 * edge * (0.9 + 0.1 * (v * 12.0 + time * 0.06).sin());
            }
        }
        0.0
    }
}

impl Scene for WindowSunlightScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let settings = &self.settings;
        let time = frame.time.as_secs_f32();
        frame.raster.field(|x, y| {
            let light = settings.pane_light(x, y, time);
            // Low contrast floor grain is visible only under the projections.
            let grain = 0.92 + 0.08 * (y * 170.0 + x * 3.0 + settings.seed as f32).sin();
            light * grain
        });
        if settings.pollen {
            for index in 0..settings.pollen_count as u32 {
                masked_dot(
                    frame.raster,
                    particle(settings.seed, index, time, 0.4),
                    0.95,
                    |x, y| settings.pane_light(x, y, time) > 0.0,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(settings: WindowSunlightSettings, width: u16, height: u16, seconds: u64) -> Vec<f32> {
        let mut scene = WindowSunlightScene {
            settings: settings.normalized(),
        };
        let mut raster = Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::from_secs(seconds),
            wall: Duration::from_secs(seconds),
            now: SystemTime::UNIX_EPOCH,
        });
        raster.dots
    }

    #[test]
    fn bounded_render_and_absolute_time_seek() {
        for (width, height) in [(0, 0), (1, 1), (80, 24), (240, 80)] {
            let settings = WindowSunlightSettings::default();
            let first = render(settings.clone(), width, height, 37);
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert_eq!(first, render(settings, width, height, 37));
        }
        assert_ne!(
            render(WindowSunlightSettings::default(), 80, 24, 0),
            render(WindowSunlightSettings::default(), 80, 24, 9)
        );
        let changed = WindowSunlightSettings {
            seed: 91,
            ..Default::default()
        };
        assert_ne!(
            render(changed, 80, 24, 9),
            render(WindowSunlightSettings::default(), 80, 24, 9)
        );
    }

    #[test]
    fn default_projection_passes_over_multiple_terminal_columns() {
        let settings = WindowSunlightSettings::default();
        let lit_center = |seconds: u64| {
            let dots = render(settings.clone(), 80, 24, seconds);
            let (weighted_x, total_light) = dots.iter().enumerate().fold(
                (0.0_f64, 0.0_f64),
                |(weighted_x, total_light), (index, intensity)| {
                    let light = f64::from(*intensity);
                    (
                        weighted_x + (index % 160) as f64 * light,
                        total_light + light,
                    )
                },
            );
            assert!(total_light > 0.0);
            weighted_x / total_light / 2.0
        };

        assert!(
            lit_center(17) - lit_center(0) > 2.0,
            "default sunlight should pass over multiple columns at 80x24"
        );
    }

    #[test]
    fn settings_roundtrip_controls_and_normalization() {
        let settings = WindowSunlightSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            settings,
            serde_json::from_str::<WindowSunlightSettings>(&json).unwrap()
        );
        for row in settings.controls() {
            assert!(!row.help.is_empty());
            let mut changed = settings.clone();
            assert_eq!(changed.set_control(row.id, row.value), Ok(false));
        }
        let mut changed = settings;
        assert!(changed
            .set_control("seed", ControlValue::Bool(true))
            .is_err());
        assert_eq!(
            changed.set_control("unknown", ControlValue::Number(12)),
            Ok(false)
        );
        changed
            .set_control("seed", ControlValue::Number(i32::MAX))
            .unwrap();
        assert_eq!(changed.seed, 9999);
    }
    #[test]
    fn both_grids_have_separate_panes_in_every_window() {
        for count in 1..=4 {
            for (grid, rows) in [(WindowGrid::TwoByTwo, 2), (WindowGrid::TwoByThree, 3)] {
                let settings = WindowSunlightSettings {
                    window_count: count,
                    grid,
                    motion: 0,
                    shear: 0,
                    ..Default::default()
                };
                for index in 0..count {
                    let center_x = (index as f32 + 0.5) / count as f32;
                    let width = settings.size as f32 / 100.0 / count as f32;
                    for column in 0..2 {
                        for row in 0..rows {
                            let x = center_x + width * ((column as f32 + 0.5) / 2.0 - 0.5);
                            let y = 0.5
                                + settings.height as f32 / 100.0
                                    * ((row as f32 + 0.5) / rows as f32 - 0.5);
                            assert!(settings.pane_light(x, y, 0.0) > 0.0);
                            assert_eq!(settings.pane_light(center_x, y, 0.0), 0.0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn optional_pollen_never_paints_mullions_or_outside_windows() {
        let settings = WindowSunlightSettings {
            window_count: 3,
            grid: WindowGrid::TwoByThree,
            pollen: true,
            pollen_count: 128,
            ..Default::default()
        };
        let dots = render(settings.clone(), 80, 24, 9);
        let plain = render(
            WindowSunlightSettings {
                pollen: false,
                ..settings.clone()
            },
            80,
            24,
            9,
        );
        assert_ne!(dots, plain);
        for (index, value) in dots.iter().enumerate() {
            let x = ((index % 160) as f32 + 0.5) / 160.0;
            let y = ((index / 160) as f32 + 0.5) / 96.0;
            if settings.pane_light(x, y, 9.0) == 0.0 {
                assert_eq!(*value, 0.0);
            }
        }
        assert_eq!(
            render(
                WindowSunlightSettings {
                    pollen_count: 0,
                    ..settings.clone()
                },
                80,
                24,
                9
            ),
            plain
        );
        let mut changed = settings;
        assert!(changed.set_control("grid", ControlValue::Index(2)).is_err());
        assert_eq!(
            changed.set_control("pollen", ControlValue::Bool(false)),
            Ok(true)
        );
    }
}
