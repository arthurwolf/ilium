//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NeedleThreadsSettings {
    pub threads: i32,
    pub opening_x: i32,
    pub spread: i32,
    pub sway_seconds: i32,
}
impl Default for NeedleThreadsSettings {
    fn default() -> Self {
        Self {
            threads: 9,
            opening_x: 50,
            spread: 65,
            sway_seconds: 24,
        }
    }
}
impl SceneSettings for NeedleThreadsSettings {
    fn normalized(&self) -> Self {
        Self {
            threads: self.threads.clamp(2, 24),
            opening_x: self.opening_x.clamp(20, 80),
            spread: self.spread.clamp(10, 90),
            sway_seconds: self.sway_seconds.clamp(6, 90),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "threads",
                "Thread count",
                s.threads,
                (2, 24, 1),
                "",
                "Number of curves constrained through the shared eye.",
            ),
            Control::slider(
                "opening_x",
                "Eye position",
                s.opening_x,
                (20, 80, 1),
                "",
                "Horizontal position of the shared opening, in percent.",
            ),
            Control::slider(
                "spread",
                "Fan spread",
                s.spread,
                (10, 90, 1),
                "",
                "Vertical extent of the loose threads.",
            ),
            Control::slider(
                "sway_seconds",
                "Sway period",
                s.sway_seconds,
                (6, 90, 1),
                "",
                "Seconds for one gentle thread sway.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("threads", ControlValue::Number(v)) => self.threads = v,
            ("opening_x", ControlValue::Number(v)) => self.opening_x = v,
            ("spread", ControlValue::Number(v)) => self.spread = v,
            ("sway_seconds", ControlValue::Number(v)) => self.sway_seconds = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct NeedleThreadsScene {
    settings: NeedleThreadsSettings,
}
impl NeedleThreadsScene {
    pub fn new(settings: &NeedleThreadsSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &NeedleThreadsSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

fn thread_point(u: f32, eye: f32, index: i32, count: i32, spread: f32, time: f32) -> (f32, f32) {
    let side = if u < eye {
        (eye - u) / eye
    } else {
        (u - eye) / (1.0 - eye)
    };
    let rank = (index as f32 / (count - 1) as f32 - 0.5) * spread;
    let sway = (u * TAU + time + index as f32 * 0.5).sin() * 0.065 * side;
    (u, 0.5 + rank * side.powf(0.7) + sway)
}
impl Scene for NeedleThreadsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let eye = self.settings.opening_x as f32 / 100.0;
        let time =
            (frame.time.as_secs_f64() / self.settings.sway_seconds as f64).fract() as f32 * TAU;
        for i in 0..self.settings.threads {
            frame.raster.curve(64, 0.45, 0.65, |u| {
                thread_point(
                    u,
                    eye,
                    i,
                    self.settings.threads,
                    self.settings.spread as f32 / 100.0,
                    time,
                )
            });
        }
        frame.raster.curve(32, 0.7, 0.9, |u| {
            let a = u * TAU;
            (eye + 0.009 * a.cos(), 0.5 + 0.028 * a.sin())
        });
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn every_thread_passes_through_same_opening() {
        for eye in [0.2, 0.5, 0.8] {
            for time in [0.0, 2.0, 6.0] {
                for i in 0..24 {
                    assert_eq!(thread_point(eye, eye, i, 24, 0.9, time), (eye, 0.5));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut NeedleThreadsScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = NeedleThreadsScene::from_settings(&NeedleThreadsSettings::default());
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
        let mut settings = NeedleThreadsSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<NeedleThreadsSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<NeedleThreadsSettings>("{}").unwrap(),
            NeedleThreadsSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = NeedleThreadsSettings::default();
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
