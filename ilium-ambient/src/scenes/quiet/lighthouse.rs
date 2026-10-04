//! Bounded procedural lighthouse; no assets, workers or frame history.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LighthouseSettings {
    /// Sea horizon as a percentage from the top. Range 25..=80.
    pub horizon: i32,
    /// Horizontal position of the simple coastal silhouette. Range 5..=95.
    pub location: i32,
    /// Angular spread of the sweeping light in degrees. Range 3..=40.
    pub beam_width: i32,
    /// Maximum sweep above and below the horizon in degrees. Range 10..=85.
    pub sweep: i32,
    /// Speed of the returning sweep. Range 1..=100.
    pub speed: i32,
    /// Strength of short reflected marks; zero hides them. Range 0..=100.
    pub water_glints: i32,
    /// Selects the repeatable water glint pattern. Range 0..=9999.
    pub seed: i32,
}

impl Default for LighthouseSettings {
    fn default() -> Self {
        Self {
            horizon: 55,
            location: 18,
            beam_width: 12,
            sweep: 65,
            speed: 20,
            water_glints: 55,
            seed: 1,
        }
    }
}

impl SceneSettings for LighthouseSettings {
    fn normalized(&self) -> Self {
        Self {
            horizon: self.horizon.clamp(25, 80),
            location: self.location.clamp(5, 95),
            beam_width: self.beam_width.clamp(3, 40),
            sweep: self.sweep.clamp(10, 85),
            speed: self.speed.clamp(1, 100),
            water_glints: self.water_glints.clamp(0, 100),
            seed: self.seed.clamp(0, 9999),
        }
    }
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "horizon",
                "Horizon",
                self.horizon,
                (25, 80, 1),
                "",
                "Sea horizon as a percentage from the top.",
            ),
            Control::slider(
                "location",
                "Lighthouse location",
                self.location,
                (5, 95, 1),
                "",
                "Horizontal position of the simple coastal silhouette.",
            ),
            Control::slider(
                "beam_width",
                "Beam width",
                self.beam_width,
                (3, 40, 1),
                "",
                "Angular spread of the sweeping light in degrees.",
            ),
            Control::slider(
                "sweep",
                "Sweep range",
                self.sweep,
                (10, 85, 1),
                "",
                "Maximum sweep above and below the horizon in degrees.",
            ),
            Control::slider(
                "speed",
                "Sweep speed",
                self.speed,
                (1, 100, 1),
                "",
                "Speed of the returning sweep.",
            ),
            Control::slider(
                "water_glints",
                "Water glints",
                self.water_glints,
                (0, 100, 1),
                "",
                "Strength of short reflected marks; zero hides them.",
            ),
            Control::slider(
                "seed",
                "Seed",
                self.seed,
                (0, 9999, 1),
                "",
                "Selects the repeatable water glint pattern.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let (field, min, max) = match id {
            "horizon" => (&mut self.horizon, 25, 80),
            "location" => (&mut self.location, 5, 95),
            "beam_width" => (&mut self.beam_width, 3, 40),
            "sweep" => (&mut self.sweep, 10, 85),
            "speed" => (&mut self.speed, 1, 100),
            "water_glints" => (&mut self.water_glints, 0, 100),
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

pub struct LighthouseScene {
    settings: LighthouseSettings,
}

impl LighthouseScene {
    pub fn new(settings: &LighthouseSettings, _env: &SceneEnv) -> Self {
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

impl Scene for LighthouseScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let settings = &self.settings;
        let time = frame.time.as_secs_f32();
        let aspect = frame.raster.aspect();
        let horizon = settings.horizon as f32 / 100.0;
        let source_x = settings.location as f32 / 100.0;
        let source_y = horizon - 0.12;
        let direction =
            (time * settings.speed as f32 / 350.0).sin() * (settings.sweep as f32).to_radians();
        let spread = (settings.beam_width as f32).to_radians();
        let near_edge = source_x > 0.5;
        frame.raster.field(|x, y| {
            let dx = (x - source_x) * aspect;
            let dy = y - source_y;
            let forward = if near_edge { -dx } else { dx };
            let angle = dy.atan2(forward);
            let light = (1.0 - ((angle - direction).abs() / spread).powi(2)).max(0.0);
            let distance = (dx * dx + dy * dy).sqrt();
            let beam = light * 0.24 / (1.0 + distance * 2.0);
            // A tapered tower, cap and island cut sky ink out of their silhouette.
            let tower_width = 0.007 + ((y - source_y) / 0.12).clamp(0.0, 1.0) * 0.009;
            let tower = y >= source_y && y <= horizon && (x - source_x).abs() < tower_width;
            let roof = y >= source_y - 0.018 && y < source_y && (x - source_x).abs() < 0.023;
            let island = y >= horizon
                && y < horizon + 0.04
                && (x - source_x).abs() < 0.07 * (1.0 - (y - horizon) / 0.04);
            if tower || roof || island {
                return 0.0;
            }
            if y < horizon {
                return beam;
            }
            let row = (y * 100.0).floor() as u32;
            let phase = random(settings.seed, row) * std::f32::consts::TAU;
            let ripple =
                ((x * (48.0 + random(settings.seed, row + 100) * 50.0) + phase + time * 0.7).sin()
                    - 0.75)
                    .max(0.0)
                    * 4.0;
            let narrow_row = ((y * 100.0).fract() - 0.5).abs() < 0.12;
            let water = if narrow_row {
                ripple * (0.10 + light * 0.75)
            } else {
                0.0
            };
            water * settings.water_glints as f32 / 100.0
        });
        // A tiny lantern remains visible when its beam faces away.
        frame.raster.line(
            (source_x - 0.008, source_y),
            (source_x + 0.008, source_y),
            0.5,
            0.85,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(settings: LighthouseSettings, width: u16, height: u16, seconds: u64) -> Vec<f32> {
        let mut scene = LighthouseScene {
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
            let settings = LighthouseSettings::default();
            let first = render(settings.clone(), width, height, 37);
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert_eq!(first, render(settings, width, height, 37));
        }
        assert_ne!(
            render(LighthouseSettings::default(), 80, 24, 0),
            render(LighthouseSettings::default(), 80, 24, 9)
        );
        let changed = LighthouseSettings {
            seed: 91,
            ..Default::default()
        };
        assert_ne!(
            render(changed, 80, 24, 9),
            render(LighthouseSettings::default(), 80, 24, 9)
        );
    }

    #[test]
    fn settings_roundtrip_controls_and_normalization() {
        let settings = LighthouseSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            settings,
            serde_json::from_str::<LighthouseSettings>(&json).unwrap()
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
    fn sea_glints_can_be_disabled_and_tower_cuts_out_beam() {
        let settings = LighthouseSettings {
            water_glints: 0,
            ..Default::default()
        };
        let dots = render(settings.clone(), 80, 24, 9);
        for (index, value) in dots.iter().enumerate() {
            let x = ((index % 160) as f32 + 0.5) / 160.0;
            let y = ((index / 160) as f32 + 0.5) / 96.0;
            if y > settings.horizon as f32 / 100.0 {
                assert_eq!(*value, 0.0);
            }
            if (x - settings.location as f32 / 100.0).abs() < 0.007 && (0.46..0.54).contains(&y) {
                assert_eq!(*value, 0.0);
            }
        }
        assert_ne!(dots, render(LighthouseSettings::default(), 80, 24, 9));
    }
}
