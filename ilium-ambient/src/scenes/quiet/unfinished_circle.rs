//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnfinishedCircleSettings {
    pub rings: i32,
    pub gap_degrees: i32,
    pub wobble: i32,
    pub turn_seconds: i32,
}
impl Default for UnfinishedCircleSettings {
    fn default() -> Self {
        Self {
            rings: 5,
            gap_degrees: 65,
            wobble: 12,
            turn_seconds: 35,
        }
    }
}
impl SceneSettings for UnfinishedCircleSettings {
    fn normalized(&self) -> Self {
        Self {
            rings: self.rings.clamp(1, 12),
            gap_degrees: self.gap_degrees.clamp(15, 150),
            wobble: self.wobble.clamp(0, 40),
            turn_seconds: self.turn_seconds.clamp(8, 120),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "rings",
                "Ring count",
                s.rings,
                (1, 12, 1),
                "",
                "Concentric rings with independent drifting gaps.",
            ),
            Control::slider(
                "gap_degrees",
                "Gap width",
                s.gap_degrees,
                (15, 150, 1),
                "",
                "Angular width of each missing arc.",
            ),
            Control::slider(
                "wobble",
                "Imperfection",
                s.wobble,
                (0, 40, 1),
                "",
                "Small radial ripples as a percentage of ring spacing.",
            ),
            Control::slider(
                "turn_seconds",
                "Gap orbit",
                s.turn_seconds,
                (8, 120, 1),
                "",
                "Seconds for the missing sections to orbit.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("rings", ControlValue::Number(v)) => self.rings = v,
            ("gap_degrees", ControlValue::Number(v)) => self.gap_degrees = v,
            ("wobble", ControlValue::Number(v)) => self.wobble = v,
            ("turn_seconds", ControlValue::Number(v)) => self.turn_seconds = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct UnfinishedCircleScene {
    settings: UnfinishedCircleSettings,
}
impl UnfinishedCircleScene {
    pub fn new(settings: &UnfinishedCircleSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &UnfinishedCircleSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

impl Scene for UnfinishedCircleScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let aspect = frame.raster.aspect().max(0.01);
        let time =
            (frame.time.as_secs_f64() / self.settings.turn_seconds as f64).fract() as f32 * TAU;
        let gap = self.settings.gap_degrees as f32 * PI / 180.0;
        for ring in 0..self.settings.rings {
            let r = (0.12 + 0.3 * (ring + 1) as f32 / self.settings.rings as f32) * aspect.min(1.0);
            let offset = time + ring as f32 * 0.47 + (time * 0.7 + ring as f32).sin() * 0.45;
            frame.raster.curve(96, 0.6, 0.75, |u| {
                let a = offset + gap / 2.0 + u * (TAU - gap);
                let wobble = self.settings.wobble as f32 / 100.0
                    * 0.012
                    * (a * 3.0 + ring as f32 + time).sin();
                (
                    0.5 + (r + wobble) * a.cos() / aspect,
                    0.5 + (r + wobble) * a.sin(),
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut UnfinishedCircleScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = UnfinishedCircleScene::from_settings(&UnfinishedCircleSettings::default());
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
        let mut settings = UnfinishedCircleSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<UnfinishedCircleSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<UnfinishedCircleSettings>("{}").unwrap(),
            UnfinishedCircleSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = UnfinishedCircleSettings::default();
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
