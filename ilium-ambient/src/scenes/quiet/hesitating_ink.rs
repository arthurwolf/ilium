//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HesitatingInkSettings {
    pub loops: i32,
    pub cycle_seconds: i32,
    pub hesitation: i32,
    pub seed: i32,
}
impl Default for HesitatingInkSettings {
    fn default() -> Self {
        Self {
            loops: 5,
            cycle_seconds: 40,
            hesitation: 55,
            seed: 7,
        }
    }
}
impl SceneSettings for HesitatingInkSettings {
    fn normalized(&self) -> Self {
        Self {
            loops: self.loops.clamp(2, 12),
            cycle_seconds: self.cycle_seconds.clamp(12, 120),
            hesitation: self.hesitation.clamp(0, 85),
            seed: self.seed.clamp(0, 9999),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "loops",
                "Curl count",
                s.loops,
                (2, 12, 1),
                "",
                "Number of curls in the bounded wandering stroke.",
            ),
            Control::slider(
                "cycle_seconds",
                "Drawing cycle",
                s.cycle_seconds,
                (12, 120, 1),
                "",
                "Time for drawing, hesitating, resting and fading.",
            ),
            Control::slider(
                "hesitation",
                "Pause strength",
                s.hesitation,
                (0, 85, 1),
                "",
                "Time held still within each curl. Zero draws continuously.",
            ),
            Control::slider(
                "seed",
                "Path seed",
                s.seed,
                (0, 9999, 1),
                "",
                "Stable seed changes the curl phases and amplitudes.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("loops", ControlValue::Number(v)) => self.loops = v,
            ("cycle_seconds", ControlValue::Number(v)) => self.cycle_seconds = v,
            ("hesitation", ControlValue::Number(v)) => self.hesitation = v,
            ("seed", ControlValue::Number(v)) => self.seed = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct HesitatingInkScene {
    settings: HesitatingInkSettings,
    path: Vec<(f32, f32)>,
}
impl HesitatingInkScene {
    pub fn new(settings: &HesitatingInkSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &HesitatingInkSettings) -> Self {
        let settings = settings.normalized();
        let path = (0..=384)
            .map(|i| ink_point(i as f32 / 384.0, settings.loops, settings.seed))
            .collect();
        Self { settings, path }
    }
}

fn ink_point(u: f32, loops: i32, seed: i32) -> (f32, f32) {
    let phase = crate::raster::hash(seed, 17) * TAU;
    let a = u * loops as f32 * TAU;
    (
        0.08 + 0.84 * u + 0.045 * (a + phase).sin(),
        0.5 + 0.20 * a.sin() + 0.10 * (a * 0.37 + phase).cos(),
    )
}
// Each curl moves halfway, holds a genuine plateau, then resumes. The two
// smooth ramps have zero velocity at the plateau edges and never reverse.
// A zero hesitation setting preserves uninterrupted linear drawing.
fn ink_progress(u: f32, loops: i32, hesitation: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    if hesitation <= 0.0 {
        return u;
    }
    let position = u * loops as f32;
    let curl = position.floor();
    let local = position.fract();
    let pause_fraction = hesitation.clamp(0.0, 0.85) * 0.5;
    let ramp_duration = (1.0 - pause_fraction) * 0.5;
    let progress = if local < ramp_duration {
        0.5 * crate::raster::smoothstep(0.0, ramp_duration, local)
    } else if local <= ramp_duration + pause_fraction {
        0.5
    } else {
        0.5 + 0.5 * crate::raster::smoothstep(ramp_duration + pause_fraction, 1.0, local)
    };
    ((curl + progress) / loops as f32).clamp(0.0, 1.0)
}
impl Scene for HesitatingInkScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let phase = (frame.time.as_secs_f64() / self.settings.cycle_seconds as f64).fract() as f32;
        let progress = ink_progress(
            (phase / 0.82).min(1.0),
            self.settings.loops,
            self.settings.hesitation as f32 / 100.0,
        );
        let reveal = progress * (self.path.len() - 1) as f32;
        let fade = 1.0 - crate::raster::smoothstep(0.9, 1.0, phase);
        for (i, pair) in self
            .path
            .windows(2)
            .enumerate()
            .take(reveal.ceil() as usize)
        {
            let f = (reveal - i as f32).clamp(0.0, 1.0);
            let p = (
                pair[0].0 + (pair[1].0 - pair[0].0) * f,
                pair[0].1 + (pair[1].1 - pair[0].1) * f,
            );
            frame.raster.line(pair[0], p, 0.65, 0.78 * fade);
            if f < 1.0 {
                frame.raster.line(p, p, 1.4, fade);
            }
        }
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn ink_never_reverses_during_hesitation() {
        let mut last = 0.0;
        for i in 0..=1000 {
            let next = ink_progress(i as f32 / 1000.0, 12, 0.85);
            assert!(next >= last);
            last = next;
        }
        assert_eq!(last, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut HesitatingInkScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
    fn stroke_pauses_in_the_middle_then_resumes_and_can_disable_pauses() {
        let mut scene = HesitatingInkScene::from_settings(&HesitatingInkSettings::default());
        let paused = render(&mut scene, 120, 40, 2.8);
        assert!(paused.iter().any(|value| *value > 0.1));
        assert!(
            paused == render(&mut scene, 120, 40, 3.8),
            "the drawing tip must hold still during the first curl"
        );
        assert!(
            paused != render(&mut scene, 120, 40, 4.8),
            "the drawing tip must resume after its pause"
        );
        assert!(
            paused != render(&mut scene, 120, 40, 35.0),
            "a middle-of-stroke pause is not the completed-stroke hold"
        );
        let continuous = HesitatingInkSettings {
            hesitation: 0,
            ..Default::default()
        };
        let mut scene = HesitatingInkScene::from_settings(&continuous);
        let earlier = render(&mut scene, 120, 40, 2.8);
        assert!(earlier != render(&mut scene, 120, 40, 3.8));
    }

    #[test]
    fn seeking_is_reproducible_and_motion_changes_pixels() {
        let mut scene = HesitatingInkScene::from_settings(&HesitatingInkSettings::default());
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
        let mut settings = HesitatingInkSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<HesitatingInkSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<HesitatingInkSettings>("{}").unwrap(),
            HesitatingInkSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = HesitatingInkSettings::default();
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
