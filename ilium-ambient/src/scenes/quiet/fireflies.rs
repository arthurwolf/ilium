//! Bounded procedural fireflies; no assets, workers or frame history.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FirefliesSettings {
    /// Bounded population of independent luminous dots. Range 4..=128.
    pub count: i32,
    /// Size of slow wandering paths; zero fixes positions. Range 0..=100.
    pub drift: i32,
    /// Seconds between shared flashes. Range 1..=12.
    pub pulse_period: i32,
    /// Strength of the gradual gathering into a common rhythm and separation. Range 0..=100.
    pub synchronization: i32,
    /// Selects positions and individual pulse rhythms. Range 0..=9999.
    pub seed: i32,
}

impl Default for FirefliesSettings {
    fn default() -> Self {
        Self {
            count: 36,
            drift: 30,
            pulse_period: 4,
            synchronization: 80,
            seed: 1,
        }
    }
}

impl SceneSettings for FirefliesSettings {
    fn normalized(&self) -> Self {
        Self {
            count: self.count.clamp(4, 128),
            drift: self.drift.clamp(0, 100),
            pulse_period: self.pulse_period.clamp(1, 12),
            synchronization: self.synchronization.clamp(0, 100),
            seed: self.seed.clamp(0, 9999),
        }
    }
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "count",
                "Firefly count",
                self.count,
                (4, 128, 1),
                "",
                "Bounded population of independent luminous dots.",
            ),
            Control::slider(
                "drift",
                "Drift",
                self.drift,
                (0, 100, 1),
                "",
                "Size of slow wandering paths; zero fixes positions.",
            ),
            Control::slider(
                "pulse_period",
                "Pulse period",
                self.pulse_period,
                (1, 12, 1),
                "",
                "Seconds between shared flashes.",
            ),
            Control::slider(
                "synchronization",
                "Synchronization",
                self.synchronization,
                (0, 100, 1),
                "",
                "Strength of the gradual gathering into a common rhythm and separation.",
            ),
            Control::slider(
                "seed",
                "Seed",
                self.seed,
                (0, 9999, 1),
                "",
                "Selects positions and individual pulse rhythms.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let (field, min, max) = match id {
            "count" => (&mut self.count, 4, 128),
            "drift" => (&mut self.drift, 0, 100),
            "pulse_period" => (&mut self.pulse_period, 1, 12),
            "synchronization" => (&mut self.synchronization, 0, 100),
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

pub struct FirefliesScene {
    settings: FirefliesSettings,
}

impl FirefliesScene {
    pub fn new(settings: &FirefliesSettings, _env: &SceneEnv) -> Self {
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

impl FirefliesSettings {
    fn pulse(&self, index: u32, time: f32) -> f32 {
        // Slowly approach a common phase and frequency, then release them.
        // At peak gathering every firefly shares exactly the same rhythm.
        let gathering = (0.5 + 0.5 * (time * 0.055).sin()) * self.synchronization as f32 / 100.0;
        let individuality = 1.0 - gathering;
        let phase = random(self.seed, index + 800) * std::f32::consts::TAU;
        let detuning = random(self.seed, index + 900) - 0.5;
        let shared = time * std::f32::consts::TAU / self.pulse_period as f32;
        let angle = shared + individuality * (phase + (time * 0.2).sin() * detuning);
        // Narrow, soft flashes leave genuine quiet intervals.
        ((angle.cos() - 0.35) / 0.65).max(0.0).powi(2)
    }
}

impl Scene for FirefliesScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.0);
        frame.raster.owner_ids.fill(0);
        let time = frame.time.as_secs_f32();
        for index in 0..self.settings.count as u32 {
            let phase = random(self.settings.seed, index + 300) * std::f32::consts::TAU;
            let drift = self.settings.drift as f32 / 100.0;
            let position = (
                (random(self.settings.seed, index * 2)
                    + drift * 0.10 * (time * 0.16 + phase).sin())
                .rem_euclid(1.0),
                (random(self.settings.seed, index * 2 + 1)
                    + drift * 0.08 * (time * 0.13 + phase).cos())
                .rem_euclid(1.0),
            );
            masked_dot(
                frame.raster,
                position,
                self.settings.pulse(index, time),
                |_, _| true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Raster;
    use std::time::{Duration, SystemTime};

    fn render(settings: FirefliesSettings, width: u16, height: u16, seconds: u64) -> Vec<f32> {
        let mut scene = FirefliesScene {
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
            let settings = FirefliesSettings::default();
            let first = render(settings.clone(), width, height, 37);
            assert!(first
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
            assert_eq!(first, render(settings, width, height, 37));
        }
        assert_ne!(
            render(FirefliesSettings::default(), 80, 24, 0),
            render(FirefliesSettings::default(), 80, 24, 9)
        );
        let changed = FirefliesSettings {
            seed: 91,
            ..Default::default()
        };
        // Synchronized populations legitimately share completely dark phases.
        // Seeded paths must differ over a cycle, not at an arbitrary dark frame.
        assert!((8..=12).any(|time| {
            render(changed.clone(), 80, 24, time)
                != render(FirefliesSettings::default(), 80, 24, time)
        }));
    }

    #[test]
    fn settings_roundtrip_controls_and_normalization() {
        let settings = FirefliesSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            settings,
            serde_json::from_str::<FirefliesSettings>(&json).unwrap()
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
    fn gathering_converges_and_then_releases_the_rhythm() {
        let settings = FirefliesSettings {
            synchronization: 100,
            ..Default::default()
        };
        let peak = std::f32::consts::FRAC_PI_2 / 0.055;
        for index in 1..128 {
            assert!((settings.pulse(index, peak) - settings.pulse(0, peak)).abs() < 0.0001);
        }
        let separate = peak + std::f32::consts::PI / 0.055;
        let low = (0..128)
            .map(|i| settings.pulse(i, separate))
            .fold(1.0_f32, f32::min);
        let high = (0..128)
            .map(|i| settings.pulse(i, separate))
            .fold(0.0_f32, f32::max);
        assert!(high - low > 0.2);
        assert_eq!(
            FirefliesSettings {
                count: -1,
                ..Default::default()
            }
            .normalized()
            .count,
            4
        );
        assert_eq!(
            FirefliesSettings {
                count: i32::MAX,
                ..Default::default()
            }
            .normalized()
            .count,
            128
        );
    }
}
