//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayedReflectionSettings {
    pub delay_seconds: i32,
    pub boundary: i32,
    pub distortion: i32,
    pub sway_seconds: i32,
}
impl Default for DelayedReflectionSettings {
    fn default() -> Self {
        Self {
            delay_seconds: 3,
            boundary: 52,
            distortion: 15,
            sway_seconds: 22,
        }
    }
}
impl SceneSettings for DelayedReflectionSettings {
    fn normalized(&self) -> Self {
        Self {
            delay_seconds: self.delay_seconds.clamp(0, 15),
            boundary: self.boundary.clamp(35, 65),
            distortion: self.distortion.clamp(0, 50),
            sway_seconds: self.sway_seconds.clamp(6, 90),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "delay_seconds",
                "Reflection delay",
                s.delay_seconds,
                (0, 15, 1),
                "",
                "Seconds by which the lower ribbon follows the upper ribbon.",
            ),
            Control::slider(
                "boundary",
                "Waterline",
                s.boundary,
                (35, 65, 1),
                "",
                "Vertical position of the reflecting boundary, in percent.",
            ),
            Control::slider(
                "distortion",
                "Ripple distortion",
                s.distortion,
                (0, 50, 1),
                "",
                "Horizontal ripple amplitude of the reflected curve.",
            ),
            Control::slider(
                "sway_seconds",
                "Sway period",
                s.sway_seconds,
                (6, 90, 1),
                "",
                "Time for the original ribbon to sway once.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("delay_seconds", ControlValue::Number(v)) => self.delay_seconds = v,
            ("boundary", ControlValue::Number(v)) => self.boundary = v,
            ("distortion", ControlValue::Number(v)) => self.distortion = v,
            ("sway_seconds", ControlValue::Number(v)) => self.sway_seconds = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct DelayedReflectionScene {
    settings: DelayedReflectionSettings,
}
impl DelayedReflectionScene {
    pub fn new(settings: &DelayedReflectionSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &DelayedReflectionSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

fn ribbon(u: f32, time: f32, boundary: f32) -> (f32, f32) {
    (
        0.08 + 0.84 * u,
        boundary
            - 0.16
            - 0.095 * (u * TAU + time).sin()
            - 0.025 * (u * TAU * 2.0 - time * 0.63).cos(),
    )
}
fn reflected(u: f32, time: f32, boundary: f32, distortion: f32) -> (f32, f32) {
    let p = ribbon(u, time, boundary);
    (
        p.0 + distortion * 0.025 * (u * TAU * 3.0 + time).sin(),
        2.0 * boundary - p.1,
    )
}
impl Scene for DelayedReflectionScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let boundary = self.settings.boundary as f32 / 100.0;
        let period = self.settings.sway_seconds as f64;
        let time = (frame.time.as_secs_f64() / period).rem_euclid(100.0) as f32 * TAU;
        let delayed = ((frame.time.as_secs_f64() - self.settings.delay_seconds as f64) / period)
            .rem_euclid(100.0) as f32
            * TAU;
        frame
            .raster
            .curve(96, 0.8, 0.9, |u| ribbon(u, time, boundary));
        frame.raster.curve(96, 0.65, 0.5, |u| {
            reflected(
                u,
                delayed,
                boundary,
                self.settings.distortion as f32 / 100.0,
            )
        });
        frame
            .raster
            .line((0.05, boundary), (0.95, boundary), 0.15, 0.22);
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn reflection_is_exact_delayed_mirror_without_distortion() {
        for i in 0..=100 {
            let u = i as f32 / 100.0;
            let original = ribbon(u, 1.7, 0.52);
            let reflected = reflected(u, 1.7, 0.52, 0.0);
            assert_eq!(original.0, reflected.0);
            assert!((original.1 + reflected.1 - 1.04).abs() < 0.00001);
        }
        assert_ne!(ribbon(0.3, 1.0, 0.52), ribbon(0.3, 2.0, 0.52));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut DelayedReflectionScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
            DelayedReflectionScene::from_settings(&DelayedReflectionSettings::default());
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
        let mut settings = DelayedReflectionSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<DelayedReflectionSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<DelayedReflectionSettings>("{}").unwrap(),
            DelayedReflectionSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = DelayedReflectionSettings::default();
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
