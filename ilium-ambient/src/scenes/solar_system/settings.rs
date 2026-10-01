//! Independent visual scales: orbital distances and body sizes need different
//! compression to make both inner and outer planets legible in a terminal.
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

pub const PLANET_NAMES: [&str; 8] = [
    "Mercury", "Venus", "Earth", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune",
];
pub const PLANET_IDS: [&str; 8] = [
    "mercury", "venus", "earth", "mars", "jupiter", "saturn", "uranus", "neptune",
];
const SPEED_LABELS: [&str; 6] = [
    "1 hour/s",
    "1 day/s",
    "10 days/s",
    "30 days/s",
    "1 year/s",
    "10 years/s",
];
const SPEED_DAYS: [f64; 6] = [1.0 / 24.0, 1.0, 10.0, 30.0, 365.25, 3652.5];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SolarSystemSettings {
    pub visible_planets: [bool; 8],
    pub distance_realism_percent: i32,
    pub size_realism_percent: i32,
    pub time_speed: usize,
    pub orbit_paths: bool,
}
impl Default for SolarSystemSettings {
    fn default() -> Self {
        Self {
            visible_planets: [true; 8],
            distance_realism_percent: 0,
            size_realism_percent: 0,
            time_speed: 3,
            orbit_paths: true,
        }
    }
}
impl SolarSystemSettings {
    pub fn days_per_second(&self) -> f64 {
        SPEED_DAYS[self.time_speed.min(SPEED_DAYS.len() - 1)]
    }
}
impl SceneSettings for SolarSystemSettings {
    fn normalized(&self) -> Self {
        Self {
            distance_realism_percent: self.distance_realism_percent.clamp(0, 100),
            size_realism_percent: self.size_realism_percent.clamp(0, 100),
            time_speed: self.time_speed.min(SPEED_DAYS.len() - 1),
            ..self.clone()
        }
    }
    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::slider("distance_realism", "Distance realism", self.distance_realism_percent, (0,100,1), "%", "0% compresses orbital distances logarithmically; 100% preserves physical distance ratios. Inner planets become clustered near the Sun at realistic scale."),
            Control::slider("size_realism", "Size realism", self.size_realism_percent, (0,100,1), "%", "0% exaggerates body sizes; 100% uses physical radii at the orbital scale. Sub-dot bodies retain a one-dot marker so they remain visible."),
            Control::choice("time_speed", "Simulation speed", self.time_speed, &SPEED_LABELS, "Simulated time per animation second. Orbits follow approximate JPL elements, beginning at J2000; global animation speed also applies."),
            Control::toggle("orbit_paths", "Orbit paths", self.orbit_paths, "Draw each visible planet's orbital path, including eccentricity and inclination, viewed from above the ecliptic."),
        ];
        for index in 0..8 {
            rows.push(Control::toggle(PLANET_IDS[index], PLANET_NAMES[index], self.visible_planets[index], "Show this planet and its orbital path. Hidden planets do not change the scale of the remaining solar system."));
        }
        rows
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        if let Some(index) = PLANET_IDS.iter().position(|candidate| *candidate == id) {
            let next = control::boolean(&value).ok_or("Expected on or off")?;
            let changed = self.visible_planets[index] != next;
            self.visible_planets[index] = next;
            return Ok(changed);
        }
        match id {
            "distance_realism" | "size_realism" => {
                let next = control::number(&value)
                    .ok_or("Expected a number")?
                    .clamp(0, 100);
                let field = if id == "distance_realism" {
                    &mut self.distance_realism_percent
                } else {
                    &mut self.size_realism_percent
                };
                let changed = *field != next;
                *field = next;
                Ok(changed)
            }
            "time_speed" => {
                let next = control::index(&value)
                    .filter(|index| *index < SPEED_DAYS.len())
                    .ok_or("Unknown speed")?;
                let changed = self.time_speed != next;
                self.time_speed = next;
                Ok(changed)
            }
            "orbit_paths" => {
                let next = control::boolean(&value).ok_or("Expected on or off")?;
                let changed = self.orbit_paths != next;
                self.orbit_paths = next;
                Ok(changed)
            }
            _ => Ok(false),
        }
    }
}
