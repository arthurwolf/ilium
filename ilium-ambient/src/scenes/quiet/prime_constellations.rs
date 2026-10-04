//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrimeConstellationsSettings {
    pub grid_size: i32,
    pub sweep_seconds: i32,
    pub background: i32,
}
impl Default for PrimeConstellationsSettings {
    fn default() -> Self {
        Self {
            grid_size: 35,
            sweep_seconds: 18,
            background: 12,
        }
    }
}
impl SceneSettings for PrimeConstellationsSettings {
    fn normalized(&self) -> Self {
        Self {
            grid_size: self.grid_size.clamp(17, 65) | 1,
            sweep_seconds: self.sweep_seconds.clamp(5, 90),
            background: self.background.clamp(0, 40),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "grid_size",
                "Spiral width",
                s.grid_size,
                (17, 65, 2),
                "",
                "Odd grid width; at most 4225 cached number positions.",
            ),
            Control::slider(
                "sweep_seconds",
                "Sweep period",
                s.sweep_seconds,
                (5, 90, 1),
                "",
                "Time for diagonal illumination to cross the number spiral.",
            ),
            Control::slider(
                "background",
                "Faint dots",
                s.background,
                (0, 40, 1),
                "",
                "Brightness of nonprime points, in percent.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("grid_size", ControlValue::Number(v)) => self.grid_size = v,
            ("sweep_seconds", ControlValue::Number(v)) => self.sweep_seconds = v,
            ("background", ControlValue::Number(v)) => self.background = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct PrimeConstellationsScene {
    settings: PrimeConstellationsSettings,
    numbers: Vec<(i32, i32, bool)>,
}
impl PrimeConstellationsScene {
    pub fn new(settings: &PrimeConstellationsSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &PrimeConstellationsSettings) -> Self {
        let settings = settings.normalized();
        let numbers = ulam(settings.grid_size as usize);
        Self { settings, numbers }
    }
}

fn ulam(width: usize) -> Vec<(i32, i32, bool)> {
    let total = width * width;
    let mut prime = vec![true; total + 1];
    prime[0] = false;
    prime[1] = false;
    for p in 2..=total {
        if p * p > total {
            break;
        }
        if prime[p] {
            for n in (p * p..=total).step_by(p) {
                prime[n] = false;
            }
        }
    }
    let mut result = Vec::with_capacity(total);
    let (mut x, mut y, mut direction, mut run, mut remaining, mut legs) = (0, 0, 0, 1, 1, 0);
    for is_prime in prime.into_iter().skip(1) {
        result.push((x, y, is_prime));
        let (dx, dy) = [(1, 0), (0, -1), (-1, 0), (0, 1)][direction];
        x += dx;
        y += dy;
        remaining -= 1;
        if remaining == 0 {
            direction = (direction + 1) % 4;
            legs += 1;
            if legs % 2 == 0 {
                run += 1;
            }
            remaining = run;
        }
    }
    result
}
impl Scene for PrimeConstellationsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let phase = (frame.time.as_secs_f64() / self.settings.sweep_seconds as f64).fract() as f32;
        let n = self.settings.grid_size as f32;
        let sweep = -n + phase * 2.0 * n;
        let aspect = frame.raster.aspect().max(0.01);
        let spacing = 0.88 / n * aspect.min(1.0);
        for &(x, y, prime) in &self.numbers {
            let p = (0.5 + x as f32 * spacing / aspect, 0.5 + y as f32 * spacing);
            let d = (x as f32 + y as f32 - sweep).abs();
            let glow = (1.0 - d / (n * 0.13)).max(0.0);
            let brightness = if prime {
                0.3 + glow * 0.7
            } else {
                self.settings.background as f32 / 100.0
            };
            frame
                .raster
                .line(p, p, if prime { 0.65 } else { 0.15 }, brightness);
        }
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn spiral_adjacent_and_prime_counts_correct() {
        let a = ulam(17);
        assert_eq!(a.len(), 289);
        assert_eq!(
            &a[..5],
            &[
                (0, 0, false),
                (1, 0, true),
                (1, -1, true),
                (0, -1, false),
                (-1, -1, true)
            ]
        );
        for pair in a.windows(2) {
            assert_eq!(
                (pair[0].0 - pair[1].0).abs() + (pair[0].1 - pair[1].1).abs(),
                1
            );
        }
        for (i, (_, _, p)) in a.iter().enumerate() {
            let n = i + 1;
            assert_eq!(*p, n >= 2 && (2..n).all(|d| n % d != 0));
        }
        assert!(a.iter().all(|(x, y, _)| x.abs() <= 8 && y.abs() <= 8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(
        scene: &mut PrimeConstellationsScene,
        width: u16,
        height: u16,
        time: f64,
    ) -> Vec<f32> {
        let mut raster = crate::raster::Raster::default();
        raster.resize(usize::from(width) * 2, usize::from(height) * 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width,
            height,
            time: Duration::from_secs_f64(time),
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        });
        raster.dots
    }

    #[test]
    fn seeking_is_reproducible_and_motion_changes_pixels() {
        let mut scene =
            PrimeConstellationsScene::from_settings(&PrimeConstellationsSettings::default());
        let first = render(&mut scene, 140, 40, 3.0);
        let later = render(&mut scene, 140, 40, 9.0);
        assert!(first.iter().any(|v| *v > 0.1));
        assert_ne!(first, later);
        assert_eq!(first, render(&mut scene, 140, 40, 3.0));
        for (w, h) in [(0, 0), (1, 1), (8, 3), (140, 40)] {
            assert!(render(&mut scene, w, h, 1_000_000.0)
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn settings_roundtrip_and_old_empty_objects_receive_defaults() {
        let mut settings = PrimeConstellationsSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<PrimeConstellationsSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<PrimeConstellationsSettings>("{}").unwrap(),
            PrimeConstellationsSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = PrimeConstellationsSettings::default();
        for control in settings.controls() {
            match control.kind {
                crate::control::ControlKind::Slider { max, .. } => {
                    settings
                        .set_control(control.id, ControlValue::Number(i32::MAX))
                        .unwrap();
                    assert_eq!(
                        settings
                            .controls()
                            .iter()
                            .find(|c| c.id == control.id)
                            .unwrap()
                            .value,
                        ControlValue::Number(max)
                    );
                    assert!(!settings
                        .set_control(control.id, ControlValue::Bool(true))
                        .unwrap());
                }
                crate::control::ControlKind::Choice { options } => {
                    settings
                        .set_control(control.id, ControlValue::Index(usize::MAX))
                        .unwrap();
                    assert_eq!(
                        settings
                            .controls()
                            .iter()
                            .find(|c| c.id == control.id)
                            .unwrap()
                            .value,
                        ControlValue::Index(options.len() - 1)
                    );
                }
                _ => {}
            }
        }
        assert!(!settings
            .set_control("unknown", ControlValue::Number(0))
            .unwrap());
    }
}
