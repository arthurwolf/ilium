//! User-facing galaxy settings; all controls use the host's existing persistence.
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GalacticEmpiresSettings {
    /// Independent multiplier of economic, diplomacy and fleet time.
    pub simulation_speed: i32,
    /// Independent clockwise camera orbit multiplier; zero holds the camera.
    pub camera_speed: i32,
    pub star_count: i32,
    pub empire_count: i32,
    /// Zero chooses a fresh seed when a scene starts; positive values reproduce it.
    pub seed: i32,
    pub territory_strength: i32,
    pub show_fleets: bool,
}

impl Default for GalacticEmpiresSettings {
    fn default() -> Self {
        Self {
            simulation_speed: 100,
            camera_speed: 100,
            star_count: 320,
            empire_count: 8,
            seed: 0,
            territory_strength: 35,
            show_fleets: true,
        }
    }
}

impl SceneSettings for GalacticEmpiresSettings {
    fn normalized(&self) -> Self {
        Self {
            simulation_speed: self.simulation_speed.clamp(25, 400),
            camera_speed: self.camera_speed.clamp(0, 300),
            star_count: self.star_count.clamp(120, 420),
            empire_count: self.empire_count.clamp(3, 12),
            seed: self.seed.clamp(0, 999_999),
            territory_strength: self.territory_strength.clamp(0, 100),
            show_fleets: self.show_fleets,
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "simulation_speed",
                "Simulation speed",
                self.simulation_speed,
                (25, 400, 25),
                "%",
                "How fast empires earn resources, build fleets, negotiate and conquer. Global Speed also applies.",
            ),
            Control::slider(
                "camera_speed",
                "Camera speed",
                self.camera_speed,
                (0, 300, 10),
                "%",
                "Clockwise orbit around the galaxy. 100% makes one full sweep in 15 minutes at global Speed 1; 0% holds the view.",
            ),
            Control::slider(
                "star_count",
                "Star systems",
                self.star_count,
                (120, 420, 20),
                "",
                "Size of the procedural connected galaxy. Changing this starts a new simulation.",
            ),
            Control::slider(
                "empire_count",
                "Starting empires",
                self.empire_count,
                (3, 12, 1),
                "",
                "Civilizations begin separately, expand and meet, then eventually unify under one empire.",
            ),
            Control::slider(
                "seed",
                "Galaxy seed",
                self.seed,
                (0, 999_999, 1),
                "",
                "0 chooses a fresh galaxy. A positive seed reproduces the initial map and its simulation; later cycles use new derived seeds.",
            ),
            Control::slider(
                "territory_strength",
                "Territory shading",
                self.territory_strength,
                (0, 100, 5),
                "%",
                "Rounded colored territories around owned stars. Controls both fills and borders; keep it low for a quiet background behind text.",
            ),
            Control::toggle(
                "show_fleets",
                "Show fleets",
                self.show_fleets,
                "Small moving markers travel along hyperlanes. Scene control edits restart the galaxy with the selected settings.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut changed = self.clone();
        if id == "show_fleets" {
            changed.show_fleets =
                control::boolean(&value).ok_or("Show fleets expects On or Off")?;
        } else {
            let target = match id {
                "simulation_speed" => &mut changed.simulation_speed,
                "camera_speed" => &mut changed.camera_speed,
                "star_count" => &mut changed.star_count,
                "empire_count" => &mut changed.empire_count,
                "seed" => &mut changed.seed,
                "territory_strength" => &mut changed.territory_strength,
                _ => return Ok(false),
            };
            *target = control::number(&value).ok_or_else(|| format!("{id} expects a number"))?;
        }
        let changed = changed.normalized();
        let is_changed = *self != changed;
        *self = changed;
        Ok(is_changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_validate_clamp_and_preserve_settings_on_invalid_input() {
        let mut settings = GalacticEmpiresSettings::default();
        let before = settings.clone();
        assert!(settings
            .set_control("camera_speed", ControlValue::Bool(true))
            .is_err());
        assert_eq!(settings, before);
        assert!(!settings
            .set_control("unknown", ControlValue::Bool(true))
            .unwrap());
        for control in settings.controls() {
            assert!(settings
                .set_control(control.id, control.value.clone())
                .is_ok());
        }
        assert!(settings
            .set_control("simulation_speed", ControlValue::Number(i32::MAX))
            .unwrap());
        assert_eq!(settings.simulation_speed, 400);
        assert!(settings
            .set_control("camera_speed", ControlValue::Number(i32::MIN))
            .unwrap());
        assert_eq!(settings.camera_speed, 0);
        assert!(settings
            .set_control("show_fleets", ControlValue::Bool(false))
            .unwrap());
        let saved = serde_json::to_vec(&settings).unwrap();
        assert_eq!(
            serde_json::from_slice::<GalacticEmpiresSettings>(&saved).unwrap(),
            settings
        );
        assert_eq!(
            serde_json::from_str::<GalacticEmpiresSettings>("{}").unwrap(),
            GalacticEmpiresSettings::default()
        );
    }
}
