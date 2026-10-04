//! Bounded procedural pollen; no assets, workers or frame history.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PollenSettings {
    /// Small seeded population; zero leaves only the sunbeam. Range 0..=128.
    pub count: i32,
    /// Tilt from vertical in degrees. Range -80..=80.
    pub beam_angle: i32,
    /// Width of the shaft as a percentage of the field. Range 5..=70.
    pub beam_width: i32,
    /// Horizontal center of the beam. Range 0..=100.
    pub beam_location: i32,
    /// Slow sideways sway; zero holds the beam still. Range 0..=100.
    pub beam_motion: i32,
    /// Speed of drifting pollen; zero freezes particle positions. Range 0..=100.
    pub drift: i32,
    /// Selects repeatable pollen positions. Range 0..=9999.
    pub seed: i32,
}

impl Default for PollenSettings {
    fn default() -> Self {
        Self {
            count: 40,
            beam_angle: 25,
            beam_width: 24,
            beam_location: 50,
            beam_motion: 15,
            drift: 35,
            seed: 1,
        }
    }
}

impl SceneSettings for PollenSettings {
    fn normalized(&self) -> Self {
        Self {
            count: self.count.clamp(0, 128),
            beam_angle: self.beam_angle.clamp(-80, 80),
            beam_width: self.beam_width.clamp(5, 70),
            beam_location: self.beam_location.clamp(0, 100),
            beam_motion: self.beam_motion.clamp(0, 100),
            drift: self.drift.clamp(0, 100),
            seed: self.seed.clamp(0, 9999),
        }
    }
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "count",
                "Pollen count",
                self.count,
                (0, 128, 1),
                "",
                "Small seeded population; zero leaves only the sunbeam.",
            ),
            Control::slider(
                "beam_angle",
                "Beam angle",
                self.beam_angle,
                (-80, 80, 1),
                "",
                "Tilt from vertical in degrees.",
            ),
            Control::slider(
                "beam_width",
                "Beam width",
                self.beam_width,
                (5, 70, 1),
                "",
                "Width of the shaft as a percentage of the field.",
            ),
            Control::slider(
                "beam_location",
                "Beam location",
                self.beam_location,
                (0, 100, 1),
                "",
                "Horizontal center of the beam.",
            ),
            Control::slider(
                "beam_motion",
                "Beam motion",
                self.beam_motion,
                (0, 100, 1),
                "",
                "Slow sideways sway; zero holds the beam still.",
            ),
            Control::slider(
                "drift",
                "Drift",
                self.drift,
                (0, 100, 1),
                "",
                "Speed of drifting pollen; zero freezes particle positions.",
            ),
            Control::slider(
                "seed",
                "Seed",
                self.seed,
                (0, 9999, 1),
                "",
                "Selects repeatable pollen positions.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let (field, min, max) = match id {
            "count" => (&mut self.count, 0, 128),
            "beam_angle" => (&mut self.beam_angle, -80, 80),
            "beam_width" => (&mut self.beam_width, 5, 70),
            "beam_location" => (&mut self.beam_location, 0, 100),
            "beam_motion" => (&mut self.beam_motion, 0, 100),
            "drift" => (&mut self.drift, 0, 100),
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

pub struct PollenScene {
    settings: PollenSettings,
}

impl PollenScene {
    pub fn new(settings: &PollenSettings, _env: &SceneEnv) -> Self {
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

impl PollenSettings {
    fn beam(&self, x: f32, y: f32, time: f32, aspect: f32) -> f32 {
        let tilt = (self.beam_angle as f32).to_radians().tan() / aspect.max(0.1);
        let center = self.beam_location as f32 / 100.0
            + tilt * (y - 0.5)
            + self.beam_motion as f32 / 1000.0 * (time * 0.12).sin();
        let radius = self.beam_width as f32 / 200.0;
        (1.0 - ((x - center).abs() / radius).powi(2)).max(0.0)
    }
}

impl Scene for PollenScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let settings = &self.settings;
        let time = frame.time.as_secs_f32();
        let aspect = frame.raster.aspect();
        frame
            .raster
            .field(|x, y| settings.beam(x, y, time, aspect) * 0.10);
        for index in 0..settings.count as u32 {
            let position = particle(settings.seed, index, time, settings.drift as f32 / 100.0);
            let brightness = 0.65 + 0.3 * random(settings.seed, index + 700);
            masked_dot(frame.raster, position, brightness, |x, y| {
                settings.beam(x, y, time, aspect) > 0.0
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(settings: PollenSettings, width: u16, height: u16, seconds: u64) -> Vec<f32> {
        let mut scene = PollenScene {
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
            let settings = PollenSettings::default();
            let first = render(settings.clone(), width, height, 37);
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert_eq!(first, render(settings, width, height, 37));
        }
        assert_ne!(
            render(PollenSettings::default(), 80, 24, 0),
            render(PollenSettings::default(), 80, 24, 9)
        );
        let changed = PollenSettings {
            seed: 91,
            ..Default::default()
        };
        assert_ne!(
            render(changed, 80, 24, 9),
            render(PollenSettings::default(), 80, 24, 9)
        );
    }

    #[test]
    fn settings_roundtrip_controls_and_normalization() {
        let settings = PollenSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            settings,
            serde_json::from_str::<PollenSettings>(&json).unwrap()
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
    fn pollen_and_halos_are_confined_to_the_beam() {
        let settings = PollenSettings {
            count: 128,
            beam_width: 12,
            beam_angle: -30,
            ..Default::default()
        };
        let dots = render(settings.clone(), 80, 24, 9);
        for (index, value) in dots.iter().enumerate() {
            let x = ((index % 160) as f32 + 0.5) / 160.0;
            let y = ((index / 160) as f32 + 0.5) / 96.0;
            if settings.beam(x, y, 9.0, 160.0 / 96.0) == 0.0 {
                assert_eq!(*value, 0.0);
            }
        }
        let empty = PollenSettings {
            count: 0,
            ..settings.clone()
        };
        let no_dots = render(empty, 80, 24, 9);
        assert!(no_dots.iter().all(|value| *value <= 0.1));
        assert!(dots.iter().any(|value| *value > 0.1));
        assert_eq!(
            PollenSettings {
                count: i32::MAX,
                ..settings
            }
            .normalized()
            .count,
            128
        );
    }
}
