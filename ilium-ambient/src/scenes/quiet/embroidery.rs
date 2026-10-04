//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbroiderySettings {
    pub petals: i32,
    pub step_degrees: i32,
    pub stitches: i32,
    pub cycle_seconds: i32,
}
impl Default for EmbroiderySettings {
    fn default() -> Self {
        Self {
            petals: 6,
            step_degrees: 71,
            stitches: 360,
            cycle_seconds: 45,
        }
    }
}
impl SceneSettings for EmbroiderySettings {
    fn normalized(&self) -> Self {
        Self {
            petals: self.petals.clamp(2, 12),
            step_degrees: self.step_degrees.clamp(1, 179),
            stitches: self.stitches.clamp(60, 720),
            cycle_seconds: self.cycle_seconds.clamp(12, 120),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "petals",
                "Rose parameter",
                s.petals,
                (2, 12, 1),
                "",
                "Integer frequency of the Maurer rose.",
            ),
            Control::slider(
                "step_degrees",
                "Stitch angle",
                s.step_degrees,
                (1, 179, 1),
                "",
                "Angular step between consecutive chords.",
            ),
            Control::slider(
                "stitches",
                "Stitch count",
                s.stitches,
                (60, 720, 1),
                "",
                "Maximum cached chords in the embroidery.",
            ),
            Control::slider(
                "cycle_seconds",
                "Drawing cycle",
                s.cycle_seconds,
                (12, 120, 1),
                "",
                "Seconds for drawing, resting and fading.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("petals", ControlValue::Number(v)) => self.petals = v,
            ("step_degrees", ControlValue::Number(v)) => self.step_degrees = v,
            ("stitches", ControlValue::Number(v)) => self.stitches = v,
            ("cycle_seconds", ControlValue::Number(v)) => self.cycle_seconds = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct EmbroideryScene {
    settings: EmbroiderySettings,
    path: Vec<(f32, f32)>,
}
impl EmbroideryScene {
    pub fn new(settings: &EmbroiderySettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &EmbroiderySettings) -> Self {
        let settings = settings.normalized();
        let path = (0..=settings.stitches)
            .map(|i| {
                let a = (i * settings.step_degrees) as f32 * PI / 180.0;
                let r = (settings.petals as f32 * a).sin();
                (r * a.cos(), r * a.sin())
            })
            .collect();
        Self { settings, path }
    }
}

impl Scene for EmbroideryScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let phase = (frame.time.as_secs_f64() / self.settings.cycle_seconds as f64).fract() as f32;
        let reveal = (phase / 0.75).min(1.0) * self.settings.stitches as f32;
        let fade = 1.0 - crate::raster::smoothstep(0.9, 1.0, phase);
        let aspect = frame.raster.aspect().max(0.01);
        let scale = 0.42 * aspect.min(1.0);
        let point = |p: (f32, f32)| (0.5 + p.0 * scale / aspect, 0.5 + p.1 * scale);
        for (i, pair) in self
            .path
            .windows(2)
            .enumerate()
            .take(reveal.ceil() as usize)
        {
            let fraction = (reveal - i as f32).clamp(0.0, 1.0);
            let end = (
                pair[0].0 + (pair[1].0 - pair[0].0) * fraction,
                pair[0].1 + (pair[1].1 - pair[0].1) * fraction,
            );
            frame
                .raster
                .line(point(pair[0]), point(end), 0.4, 0.58 * fade);
            if fraction < 1.0 || i + 1 == reveal.floor() as usize {
                frame.raster.line(point(end), point(end), 1.5, fade);
            }
        }
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn maurer_vertices_lie_on_rose() {
        let s = EmbroideryScene::from_settings(&EmbroiderySettings::default());
        for (i, p) in s.path.iter().enumerate() {
            let theta = (i as i32 * s.settings.step_degrees) as f32 * PI / 180.0;
            assert!(
                (p.0.hypot(p.1) - (s.settings.petals as f32 * theta).sin().abs()).abs() < 0.0001
            );
        }
        assert!(s.path.len() <= 721);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut EmbroideryScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = EmbroideryScene::from_settings(&EmbroiderySettings::default());
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
        let mut settings = EmbroiderySettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<EmbroiderySettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<EmbroiderySettings>("{}").unwrap(),
            EmbroiderySettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = EmbroiderySettings::default();
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
