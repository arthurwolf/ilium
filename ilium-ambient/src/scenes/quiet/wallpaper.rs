//! Bounded, seekable procedural geometry; shared appearance is supplied by the host.
use crate::control::{Control, ControlValue, SceneSettings};
use crate::scene::{Frame, Scene, SceneEnv};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallpaperSettings {
    pub columns: i32,
    pub rotation_seconds: i32,
    pub motif: i32,
    pub symmetry: i32,
}
impl Default for WallpaperSettings {
    fn default() -> Self {
        Self {
            columns: 8,
            rotation_seconds: 36,
            motif: 0,
            symmetry: 1,
        }
    }
}
impl SceneSettings for WallpaperSettings {
    fn normalized(&self) -> Self {
        Self {
            columns: self.columns.clamp(3, 18),
            rotation_seconds: self.rotation_seconds.clamp(8, 120),
            motif: self.motif.clamp(0, 2),
            symmetry: self.symmetry.clamp(0, 2),
        }
    }
    fn controls(&self) -> Vec<Control> {
        let s = self.normalized();
        vec![
            Control::slider(
                "columns",
                "Repeat columns",
                s.columns,
                (3, 18, 1),
                "",
                "Number of repeating motifs across the viewport.",
            ),
            Control::slider(
                "rotation_seconds",
                "Rotation period",
                s.rotation_seconds,
                (8, 120, 1),
                "",
                "Time for each tile motif to rotate once.",
            ),
            Control::choice(
                "motif",
                "Motif",
                s.motif as usize,
                &["Petals", "Diamonds", "Pinwheels"],
                "Choose petals, diamonds or pinwheels.",
            ),
            Control::choice(
                "symmetry",
                "Symmetry",
                s.symmetry as usize,
                &["Repeat", "Mirror", "Quarter turns"],
                "Repeat identically, alternate mirror or alternate quarter turns.",
            ),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        match (id, value) {
            ("columns", ControlValue::Number(v)) => self.columns = v,
            ("rotation_seconds", ControlValue::Number(v)) => self.rotation_seconds = v,
            ("motif", ControlValue::Index(v)) => self.motif = v.min(2) as i32,
            ("symmetry", ControlValue::Index(v)) => self.symmetry = v.min(2) as i32,
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}
pub struct WallpaperScene {
    settings: WallpaperSettings,
}
impl WallpaperScene {
    pub fn new(settings: &WallpaperSettings, _env: &SceneEnv) -> Self {
        Self::from_settings(settings)
    }
    fn from_settings(settings: &WallpaperSettings) -> Self {
        let settings = settings.normalized();
        Self { settings }
    }
}

impl Scene for WallpaperScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let aspect = frame.raster.aspect().max(0.01);
        let columns = self.settings.columns;
        let rows = ((columns as f32 / aspect).ceil() as i32).clamp(1, 24);
        let angle =
            (frame.time.as_secs_f64() / self.settings.rotation_seconds as f64).fract() as f32 * TAU;
        for row in 0..rows {
            for col in 0..columns {
                let center = (
                    (col as f32 + 0.5) / columns as f32,
                    (row as f32 + 0.5) / rows as f32,
                );
                let odd = (row + col) % 2 == 1;
                let rotation = match self.settings.symmetry {
                    1 if odd => -angle,
                    2 if odd => angle + PI / 2.0,
                    _ => angle,
                };
                let point = |u: f32| {
                    let local_angle = u * TAU;
                    let theta = local_angle + rotation;
                    let radius = match self.settings.motif {
                        0 => 0.22 + 0.19 * (3.0 * local_angle).cos(),
                        1 => 0.40 / (local_angle.cos().abs() + local_angle.sin().abs()).max(0.1),
                        _ => 0.17 + 0.24 * u,
                    };
                    (
                        center.0 + radius * theta.cos() / columns as f32,
                        center.1 + radius * theta.sin() / rows as f32,
                    )
                };
                frame.raster.curve(32, 0.45, 0.72, point);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn render(scene: &mut WallpaperScene, width: u16, height: u16, time: f64) -> Vec<f32> {
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
        let mut scene = WallpaperScene::from_settings(&WallpaperSettings::default());
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
        let mut settings = WallpaperSettings::default();
        let first = settings.controls().remove(0);
        if let Some(value) = first.stepped(1) {
            settings.set_control(first.id, value).unwrap();
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<WallpaperSettings>(&encoded).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<WallpaperSettings>("{}").unwrap(),
            WallpaperSettings::default()
        );
    }

    #[test]
    fn controls_normalize_and_reject_wrong_types() {
        let mut settings = WallpaperSettings::default();
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
