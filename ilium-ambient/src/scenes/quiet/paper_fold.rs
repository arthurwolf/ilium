//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaperFoldSettings {
    pub depth: i32,
    pub cycle_seconds: i32,
    pub scale: i32,
}
impl Default for PaperFoldSettings {
    fn default() -> Self {
        Self {
            depth: 8,
            cycle_seconds: 40,
            scale: 85,
        }
    }
}
impl SceneSettings for PaperFoldSettings {
    fn normalized(&self) -> Self {
        Self {
            depth: self.depth.clamp(3, 10),
            cycle_seconds: self.cycle_seconds.clamp(12, 120),
            scale: self.scale.clamp(30, 100),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "depth",
                "Fold depth",
                s.depth,
                (3, 10, 1),
                "",
                "Each additional fold doubles the path, up to 1024 segments.",
            ),
            Control::slider(
                "cycle_seconds",
                "Fold cycle",
                s.cycle_seconds,
                (12, 120, 1),
                "",
                "Seconds for unfolding, resting, refolding and resting.",
            ),
            Control::slider(
                "scale",
                "Path size",
                s.scale,
                (30, 100, 1),
                "",
                "Percentage of available space used by the trace.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("depth", ControlValue::Number(v)) => self.depth = v,
            ("cycle_seconds", ControlValue::Number(v)) => self.cycle_seconds = v,
            ("scale", ControlValue::Number(v)) => self.scale = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct PaperFoldScene {
    settings: PaperFoldSettings,
}
impl PaperFoldScene {
    pub fn new(settings: &PaperFoldSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &PaperFoldSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

// Recursively duplicate the reversed path about its final hinge. Full opening
// is the classical right-angle dragon, with exactly 2^depth equal segments.
fn dragon(depth: i32, opening: f32) -> Vec<(f32, f32)> {
    let mut path = vec![(0.0, 0.0), (1.0, 0.0)];
    let (s, c) = (opening * PI / 2.0).sin_cos();
    for _ in 0..depth {
        let end = path[path.len() - 1];
        let len = path.len();
        for i in (0..len - 1).rev() {
            let p = path[i];
            let x = p.0 - end.0;
            let y = p.1 - end.1;
            path.push((end.0 + c * x - s * y, end.1 + s * x + c * y));
        }
    }
    path
}
impl Scene for PaperFoldScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let phase = (frame.time.as_secs_f64() / self.settings.cycle_seconds as f64).fract() as f32;
        let opening = if phase < 0.4 {
            crate::raster::smoothstep(0.0, 0.4, phase)
        } else if phase < 0.55 {
            1.0
        } else if phase < 0.95 {
            1.0 - crate::raster::smoothstep(0.55, 0.95, phase)
        } else {
            0.0
        };
        let path = dragon(self.settings.depth, opening);
        let (mut lo, mut hi) = (
            (f32::INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
        );
        for p in &path {
            lo.0 = lo.0.min(p.0);
            lo.1 = lo.1.min(p.1);
            hi.0 = hi.0.max(p.0);
            hi.1 = hi.1.max(p.1);
        }
        let aspect = frame.raster.aspect().max(0.01);
        let scale =
            self.settings.scale as f32 / 100.0 / ((hi.0 - lo.0) / aspect).max(hi.1 - lo.1).max(1.0);
        let point = |p: (f32, f32)| {
            (
                0.5 + (p.0 - (hi.0 + lo.0) * 0.5) * scale / aspect,
                0.5 + (p.1 - (hi.1 + lo.1) * 0.5) * scale,
            )
        };
        for pair in path.windows(2) {
            frame.raster.line(point(pair[0]), point(pair[1]), 0.6, 0.85);
        }
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn fully_open_dragon_has_equal_steps_and_known_endpoint() {
        for depth in 3..=10 {
            let path = dragon(depth, 1.0);
            assert_eq!(path.len(), (1 << depth) + 1);
            for pair in path.windows(2) {
                let d = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
                assert!((d - 1.0).abs() < 0.002);
            }
        }
        let p = dragon(3, 1.0);
        assert!((p[8].0 + 2.0).abs() < 0.001);
        assert!((p[8].1 + 2.0).abs() < 0.001);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut PaperFoldScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = PaperFoldScene::from_settings(&PaperFoldSettings::default());
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
        let mut settings = PaperFoldSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<PaperFoldSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<PaperFoldSettings>("{}").unwrap(),
            PaperFoldSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = PaperFoldSettings::default();
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
