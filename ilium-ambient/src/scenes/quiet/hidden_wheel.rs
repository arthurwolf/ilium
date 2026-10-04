//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HiddenWheelSettings {
    pub marks: i32,
    pub visible_arc: i32,
    pub turn_seconds: i32,
    pub dash_length: i32,
}
impl Default for HiddenWheelSettings {
    fn default() -> Self {
        Self {
            marks: 40,
            visible_arc: 65,
            turn_seconds: 32,
            dash_length: 55,
        }
    }
}
impl SceneSettings for HiddenWheelSettings {
    fn normalized(&self) -> Self {
        Self {
            marks: self.marks.clamp(12, 96),
            visible_arc: self.visible_arc.clamp(20, 95),
            turn_seconds: self.turn_seconds.clamp(8, 120),
            dash_length: self.dash_length.clamp(15, 90),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "marks",
                "Rim marks",
                s.marks,
                (12, 96, 1),
                "",
                "Short orbiting dashes suggesting an invisible wheel.",
            ),
            Control::slider(
                "visible_arc",
                "Visible sector",
                s.visible_arc,
                (20, 95, 1),
                "",
                "Percentage of the rim where marks can appear.",
            ),
            Control::slider(
                "turn_seconds",
                "Turn period",
                s.turn_seconds,
                (8, 120, 1),
                "",
                "Seconds for one wheel rotation.",
            ),
            Control::slider(
                "dash_length",
                "Dash length",
                s.dash_length,
                (15, 90, 1),
                "",
                "Percentage of each mark spacing occupied by ink.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("marks", ControlValue::Number(v)) => self.marks = v,
            ("visible_arc", ControlValue::Number(v)) => self.visible_arc = v,
            ("turn_seconds", ControlValue::Number(v)) => self.turn_seconds = v,
            ("dash_length", ControlValue::Number(v)) => self.dash_length = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct HiddenWheelScene {
    settings: HiddenWheelSettings,
}
impl HiddenWheelScene {
    pub fn new(settings: &HiddenWheelSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &HiddenWheelSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

impl Scene for HiddenWheelScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let aspect = frame.raster.aspect().max(0.01);
        let radius = 0.39 * aspect.min(1.0);
        let time =
            (frame.time.as_secs_f64() / self.settings.turn_seconds as f64).fract() as f32 * TAU;
        let sector = self.settings.visible_arc as f32 / 100.0 * TAU;
        let spacing = TAU / self.settings.marks as f32;
        let length = spacing * self.settings.dash_length as f32 / 100.0;
        for i in 0..self.settings.marks {
            let angle = (i as f32 * spacing + time).rem_euclid(TAU);
            let distance = (angle - PI).abs();
            let fade = ((sector / 2.0 - distance) / 0.18).clamp(0.0, 1.0);
            if fade <= 0.0 {
                continue;
            }
            frame.raster.curve(6, 0.7, fade * 0.85, |u| {
                let a = angle + u * length;
                (0.5 + radius * a.cos() / aspect, 0.5 + radius * a.sin())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut HiddenWheelScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = HiddenWheelScene::from_settings(&HiddenWheelSettings::default());
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
        let mut settings = HiddenWheelSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<HiddenWheelSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<HiddenWheelSettings>("{}").unwrap(),
            HiddenWheelSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = HiddenWheelSettings::default();
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
