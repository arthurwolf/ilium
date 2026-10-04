//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlmostTouchingSettings {
    pub minimum_gap: i32,
    pub reach: i32,
    pub cycle_seconds: i32,
    pub dwell: i32,
}
impl Default for AlmostTouchingSettings {
    fn default() -> Self {
        Self {
            minimum_gap: 4,
            reach: 60,
            cycle_seconds: 26,
            dwell: 55,
        }
    }
}
impl SceneSettings for AlmostTouchingSettings {
    fn normalized(&self) -> Self {
        Self {
            minimum_gap: self.minimum_gap.clamp(1, 20),
            reach: self.reach.clamp(20, 90),
            cycle_seconds: self.cycle_seconds.clamp(8, 100),
            dwell: self.dwell.clamp(0, 85),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "minimum_gap",
                "Closest gap",
                s.minimum_gap,
                (1, 20, 1),
                "",
                "Minimum distance between tips, as a percentage of width.",
            ),
            Control::slider(
                "reach",
                "Arc reach",
                s.reach,
                (20, 90, 1),
                "",
                "How far the arc bodies curl away from their tips.",
            ),
            Control::slider(
                "cycle_seconds",
                "Approach cycle",
                s.cycle_seconds,
                (8, 100, 1),
                "",
                "Seconds for approach, lingering and retreat.",
            ),
            Control::slider(
                "dwell",
                "Near-contact linger",
                s.dwell,
                (0, 85, 1),
                "",
                "Higher values slow the tips near closest approach.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("minimum_gap", ControlValue::Number(v)) => self.minimum_gap = v,
            ("reach", ControlValue::Number(v)) => self.reach = v,
            ("cycle_seconds", ControlValue::Number(v)) => self.cycle_seconds = v,
            ("dwell", ControlValue::Number(v)) => self.dwell = v,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct AlmostTouchingScene {
    settings: AlmostTouchingSettings,
}
impl AlmostTouchingScene {
    pub fn new(settings: &AlmostTouchingSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &AlmostTouchingSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

fn separation(phase: f32, minimum: f32, dwell: f32) -> f32 {
    let wave = (0.5 + 0.5 * (phase * TAU).cos()).powf(1.0 + dwell * 5.0);
    minimum + 0.25 * wave
}
fn arc_point(u: f32, right: bool, gap: f32, reach: f32) -> (f32, f32) {
    let sign = if right { 1.0 } else { -1.0 };
    let angle = u * PI * 0.83;
    (
        0.5 + sign * (gap / 2.0 + reach * 0.30 * (1.0 - angle.cos())),
        0.5 + reach * 0.35 * angle.sin(),
    )
}
impl Scene for AlmostTouchingScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let phase = (frame.time.as_secs_f64() / self.settings.cycle_seconds as f64).fract() as f32;
        let gap = separation(
            phase,
            self.settings.minimum_gap as f32 / 100.0,
            self.settings.dwell as f32 / 100.0,
        );
        let reach = self.settings.reach as f32 / 100.0;
        for right in [false, true] {
            frame
                .raster
                .curve(72, 0.8, 0.85, |u| arc_point(u, right, gap, reach));
            let tip = arc_point(0.0, right, gap, reach);
            frame.raster.line(tip, tip, 1.05, 0.95);
        }
    }
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn arcs_respect_gap_at_every_point_and_dwell() {
        for i in 0..=100 {
            let phase = i as f32 / 100.0;
            let gap = separation(phase, 0.04, 0.85);
            assert!(gap >= 0.04);
            let a = arc_point(0.0, false, gap, 0.9);
            let b = arc_point(0.0, true, gap, 0.9);
            assert!((b.0 - a.0 - gap).abs() < 0.000001);
        }
        assert_eq!(separation(0.5, 0.04, 0.85), 0.04);
        assert!(separation(0.4, 0.04, 0.85) < separation(0.4, 0.04, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut AlmostTouchingScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = AlmostTouchingScene::from_settings(&AlmostTouchingSettings::default());
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
        let mut settings = AlmostTouchingSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<AlmostTouchingSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<AlmostTouchingSettings>("{}").unwrap(),
            AlmostTouchingSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = AlmostTouchingSettings::default();
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
