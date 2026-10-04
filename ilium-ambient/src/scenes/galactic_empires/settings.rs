//! User-facing galaxy settings; all controls use the host's existing persistence.
use super::simulation::GenerationConfig;
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GalacticEmpiresSettings {
    /// Independent multiplier of economic, diplomacy and fleet time.
    pub simulation_speed: i32,
    /// Independent clockwise camera orbit multiplier; zero holds the camera.
    pub camera_speed: i32,
    pub orbit_period_seconds: i32,
    pub camera_zoom: i32,
    pub star_count: i32,
    pub empire_count: i32,
    /// Zero chooses a fresh seed when a scene starts; positive values reproduce it.
    pub seed: i32,
    pub spiral_arms: i32,
    pub arm_spread: i32,
    pub arm_twist: i32,
    pub lane_links: i32,
    pub territory_strength: i32,
    pub territory_radius: i32,
    pub territory_softness: i32,
    pub territory_border: i32,
    pub territory_contact: i32,
    pub star_size: i32,
    pub star_brightness: i32,
    pub show_lanes: bool,
    pub lane_width: i32,
    pub lane_brightness: i32,
    pub show_fleets: bool,
    pub fleet_size: i32,
    pub fleet_brightness: i32,
    pub fleet_trail_length: i32,
    pub capture_flashes: bool,
    pub victory_hold_seconds: i32,
}

impl Default for GalacticEmpiresSettings {
    fn default() -> Self {
        Self {
            simulation_speed: 100,
            camera_speed: 100,
            orbit_period_seconds: 900,
            camera_zoom: 100,
            star_count: 480,
            empire_count: 8,
            seed: 0,
            spiral_arms: 4,
            arm_spread: 100,
            arm_twist: 100,
            lane_links: 2,
            territory_strength: 35,
            territory_radius: 100,
            territory_softness: 100,
            territory_border: 100,
            territory_contact: 100,
            star_size: 100,
            star_brightness: 100,
            show_lanes: true,
            lane_width: 100,
            lane_brightness: 100,
            show_fleets: true,
            fleet_size: 100,
            fleet_brightness: 100,
            fleet_trail_length: 100,
            capture_flashes: true,
            victory_hold_seconds: 30,
        }
    }
}

impl GalacticEmpiresSettings {
    pub(super) fn generation(&self) -> GenerationConfig {
        GenerationConfig {
            star_count: self.star_count.clamp(120, 720) as usize,
            empire_count: self.empire_count.clamp(3, 12) as usize,
            spiral_arms: self.spiral_arms.clamp(2, 6) as usize,
            arm_spread: self.arm_spread,
            arm_twist: self.arm_twist,
            lane_links: self.lane_links.clamp(0, 4) as usize,
        }
    }
}

impl SceneSettings for GalacticEmpiresSettings {
    fn normalized(&self) -> Self {
        Self {
            simulation_speed: self.simulation_speed.clamp(25, 400),
            camera_speed: self.camera_speed.clamp(0, 300),
            orbit_period_seconds: self.orbit_period_seconds.clamp(60, 3600),
            camera_zoom: self.camera_zoom.clamp(50, 200),
            star_count: self.star_count.clamp(120, 720),
            empire_count: self.empire_count.clamp(3, 12),
            seed: self.seed.clamp(0, 999_999),
            spiral_arms: self.spiral_arms.clamp(2, 6),
            arm_spread: self.arm_spread.clamp(25, 200),
            arm_twist: self.arm_twist.clamp(0, 200),
            lane_links: self.lane_links.clamp(0, 4),
            territory_strength: self.territory_strength.clamp(0, 100),
            territory_radius: self.territory_radius.clamp(50, 150),
            territory_softness: self.territory_softness.clamp(50, 200),
            territory_border: self.territory_border.clamp(0, 200),
            territory_contact: self.territory_contact.clamp(0, 200),
            star_size: self.star_size.clamp(50, 200),
            star_brightness: self.star_brightness.clamp(0, 100),
            show_lanes: self.show_lanes,
            lane_width: self.lane_width.clamp(50, 200),
            lane_brightness: self.lane_brightness.clamp(0, 150),
            show_fleets: self.show_fleets,
            fleet_size: self.fleet_size.clamp(50, 200),
            fleet_brightness: self.fleet_brightness.clamp(0, 100),
            fleet_trail_length: self.fleet_trail_length.clamp(0, 200),
            capture_flashes: self.capture_flashes,
            victory_hold_seconds: self.victory_hold_seconds.clamp(10, 120),
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
                "Rate of economy, diplomacy and fleet ticks. Global Speed also applies; changing this keeps the current galaxy.",
            ),
            Control::slider(
                "camera_speed",
                "Camera speed",
                self.camera_speed,
                (0, 300, 10),
                "%",
                "Clockwise orbit multiplier; 0% holds the camera while empires keep moving.",
            ),
            Control::slider(
                "orbit_period_seconds",
                "Orbit period",
                self.orbit_period_seconds,
                (60, 3600, 60),
                "s",
                "Seconds for one orbit at Camera speed 100% and Global Speed 1. Default: 900 s.",
            ),
            Control::slider(
                "camera_zoom",
                "Camera zoom",
                self.camera_zoom,
                (50, 200, 10),
                "%",
                "100% shows one quarter of the disk's area. Higher zoom shows a smaller field without changing the galaxy.",
            ),
            Control::slider(
                "star_count",
                "Star systems",
                self.star_count,
                (120, 720, 20),
                "",
                "Size of the connected galaxy. Default: 480, 50% more than the previous 320. Changing this starts a new galaxy.",
            ),
            Control::slider(
                "empire_count",
                "Starting empires",
                self.empire_count,
                (3, 12, 1),
                "",
                "Separately founded civilizations; changing this starts a new galaxy.",
            ),
            Control::slider(
                "seed",
                "Galaxy seed",
                self.seed,
                (0, 999_999, 1),
                "",
                "0 chooses a fresh galaxy. A positive seed reproduces its initial map and simulation; changing it restarts.",
            ),
            Control::slider(
                "spiral_arms",
                "Spiral arms",
                self.spiral_arms,
                (2, 6, 1),
                "",
                "Number of arms in the generated map. Changing the shape starts a new galaxy.",
            ),
            Control::slider(
                "arm_spread",
                "Arm spread",
                self.arm_spread,
                (25, 200, 5),
                "%",
                "Angular scatter around each arm; 100% retains the original shape.",
            ),
            Control::slider(
                "arm_twist",
                "Arm twist",
                self.arm_twist,
                (0, 200, 5),
                "%",
                "How much arms turn from center to rim; 100% retains the original twist.",
            ),
            Control::slider(
                "lane_links",
                "Extra hyperlane links",
                self.lane_links,
                (0, 4, 1),
                "",
                "Up to this many short extra links per star. Zero still keeps a connected backbone.",
            ),
            Control::slider(
                "territory_strength",
                "Territory shading",
                self.territory_strength,
                (0, 100, 5),
                "%",
                "Overall territory ink. Zero hides fills, borders and contact but leaves stars and lanes.",
            ),
            Control::slider(
                "territory_radius",
                "Territory size",
                self.territory_radius,
                (50, 150, 5),
                "%",
                "Influence radius around each star; adjusting it rebuilds only the territory field.",
            ),
            Control::slider(
                "territory_softness",
                "Territory softness",
                self.territory_softness,
                (50, 200, 5),
                "%",
                "Width of each soft edge. 100% retains the original rounded border.",
            ),
            Control::slider(
                "territory_border",
                "Border emphasis",
                self.territory_border,
                (0, 200, 10),
                "%",
                "Strength of the territory contour. Zero removes the contour, not the fill.",
            ),
            Control::slider(
                "territory_contact",
                "Contact emphasis",
                self.territory_contact,
                (0, 200, 10),
                "%",
                "Brightness where opposing territories meet; wars pulse slightly.",
            ),
            Control::slider(
                "star_size",
                "Star size",
                self.star_size,
                (50, 200, 10),
                "%",
                "Radius of system markers; 100% retains the original size.",
            ),
            Control::slider(
                "star_brightness",
                "Star brightness",
                self.star_brightness,
                (0, 100, 5),
                "%",
                "System marker intensity. Zero hides markers without erasing territory or lanes.",
            ),
            Control::toggle(
                "show_lanes",
                "Show hyperlanes",
                self.show_lanes,
                "Draw the connection graph between systems. Hiding it does not disconnect the simulation.",
            ),
            Control::slider(
                "lane_width",
                "Hyperlane width",
                self.lane_width,
                (50, 200, 10),
                "%",
                "Stroke radius of visible hyperlanes.",
            ),
            Control::slider(
                "lane_brightness",
                "Hyperlane brightness",
                self.lane_brightness,
                (0, 150, 5),
                "%",
                "Visible lane intensity. Zero hides lane ink without changing fleet routes.",
            ),
            Control::toggle(
                "show_fleets",
                "Show fleets",
                self.show_fleets,
                "Moving markers on hyperlanes; turning them off keeps the simulation running.",
            ),
            Control::slider(
                "fleet_size",
                "Fleet size",
                self.fleet_size,
                (50, 200, 10),
                "%",
                "Size of fleet heads and trails.",
            ),
            Control::slider(
                "fleet_brightness",
                "Fleet brightness",
                self.fleet_brightness,
                (0, 100, 5),
                "%",
                "Fleet marker intensity. Zero hides fleet ink without changing the simulation.",
            ),
            Control::slider(
                "fleet_trail_length",
                "Fleet trails",
                self.fleet_trail_length,
                (0, 200, 10),
                "%",
                "Trail length behind moving fleet heads. Zero disables trails; 100% retains the original length.",
            ),
            Control::toggle(
                "capture_flashes",
                "Capture flashes",
                self.capture_flashes,
                "Make freshly captured system markers briefly larger.",
            ),
            Control::slider(
                "victory_hold_seconds",
                "Victory pause",
                self.victory_hold_seconds,
                (10, 120, 5),
                "s",
                "Simulation seconds to show the winner before the next galaxy; Simulation speed and Global Speed scale wall time.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut changed = self.clone();
        match id {
            "show_lanes" => {
                changed.show_lanes =
                    control::boolean(&value).ok_or("Show hyperlanes expects On or Off")?;
            }
            "show_fleets" => {
                changed.show_fleets =
                    control::boolean(&value).ok_or("Show fleets expects On or Off")?;
            }
            "capture_flashes" => {
                changed.capture_flashes =
                    control::boolean(&value).ok_or("Capture flashes expects On or Off")?;
            }
            _ => {
                let target = match id {
                    "simulation_speed" => &mut changed.simulation_speed,
                    "camera_speed" => &mut changed.camera_speed,
                    "orbit_period_seconds" => &mut changed.orbit_period_seconds,
                    "camera_zoom" => &mut changed.camera_zoom,
                    "star_count" => &mut changed.star_count,
                    "empire_count" => &mut changed.empire_count,
                    "seed" => &mut changed.seed,
                    "spiral_arms" => &mut changed.spiral_arms,
                    "arm_spread" => &mut changed.arm_spread,
                    "arm_twist" => &mut changed.arm_twist,
                    "lane_links" => &mut changed.lane_links,
                    "territory_strength" => &mut changed.territory_strength,
                    "territory_radius" => &mut changed.territory_radius,
                    "territory_softness" => &mut changed.territory_softness,
                    "territory_border" => &mut changed.territory_border,
                    "territory_contact" => &mut changed.territory_contact,
                    "star_size" => &mut changed.star_size,
                    "star_brightness" => &mut changed.star_brightness,
                    "lane_width" => &mut changed.lane_width,
                    "lane_brightness" => &mut changed.lane_brightness,
                    "fleet_size" => &mut changed.fleet_size,
                    "fleet_brightness" => &mut changed.fleet_brightness,
                    "fleet_trail_length" => &mut changed.fleet_trail_length,
                    "victory_hold_seconds" => &mut changed.victory_hold_seconds,
                    _ => return Ok(false),
                };
                *target =
                    control::number(&value).ok_or_else(|| format!("{id} expects a number"))?;
            }
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
        assert_eq!(settings.controls().len(), 27);
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

#[cfg(test)]
mod complete_control_contract_tests {
    use super::*;
    use crate::control::ControlKind;

    #[test]
    fn every_control_clamps_rejects_wrong_types_and_survives_serialization() {
        let defaults = GalacticEmpiresSettings::default();
        let mut ids = std::collections::HashSet::new();
        for control in defaults.controls() {
            assert!(ids.insert(control.id), "duplicate {}", control.id);
            let mut settings = defaults.clone();
            let cases = match control.kind {
                ControlKind::Slider { min, max, .. } => vec![
                    (ControlValue::Number(i32::MIN), ControlValue::Number(min)),
                    (ControlValue::Number(i32::MAX), ControlValue::Number(max)),
                ],
                ControlKind::Toggle => vec![
                    (ControlValue::Bool(false), ControlValue::Bool(false)),
                    (ControlValue::Bool(true), ControlValue::Bool(true)),
                ],
                _ => panic!("unexpected galaxy control kind"),
            };
            for (input, expected) in cases {
                settings.set_control(control.id, input).unwrap();
                let actual = settings
                    .controls()
                    .into_iter()
                    .find(|row| row.id == control.id)
                    .unwrap();
                assert_eq!(actual.value, expected, "{}", control.id);
                let saved = serde_json::to_vec(&settings).unwrap();
                assert_eq!(
                    serde_json::from_slice::<GalacticEmpiresSettings>(&saved).unwrap(),
                    settings
                );
            }
            let before = settings.clone();
            assert!(settings
                .set_control(control.id, ControlValue::Text("bad".into()))
                .is_err());
            assert_eq!(before, settings, "invalid input changed {}", control.id);
        }
        assert_eq!(ids.len(), 27);
    }
}
