//! Project-persisted OpenStreetMap presentation and explicit source selection.
use crate::control::{Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenStreetMapSettings {
    pub place: usize,
    pub tour: usize,
    pub dwell_seconds: i32,
    pub camera: usize,
    pub camera_speed: i32,
    pub map_width_meters: i32,
    pub brightness_percent: i32,
    pub line_width_percent: i32,
    pub roads: bool,
    pub buildings: bool,
    pub water: bool,
    pub green_space: bool,
    pub railways: bool,
    pub points_of_interest: bool,
    pub source: usize,
    pub local_path: String,
    pub endpoint: String,
    pub coordinates: String,
}
impl Default for OpenStreetMapSettings {
    fn default() -> Self {
        Self {
            place: 0,
            tour: 0,
            dwell_seconds: 120,
            camera: 1,
            camera_speed: 100,
            map_width_meters: 1400,
            brightness_percent: 100,
            line_width_percent: 100,
            roads: true,
            buildings: true,
            water: true,
            green_space: true,
            railways: true,
            points_of_interest: false,
            source: 0,
            local_path: String::new(),
            endpoint: String::new(),
            coordinates: "48.8584, 2.2945".to_owned(),
        }
    }
}
pub const PLACE_NAMES: [&str; 10] = [
    "Paris",
    "London",
    "Venice",
    "New York",
    "Tokyo",
    "Cape Town",
    "Sydney",
    "Rio de Janeiro",
    "Singapore",
    "Reykjavik",
];

/// Only finite WGS84 coordinates within the local map projection's latitude range.
pub fn parse_coordinates(value: &str) -> Result<(f64, f64), String> {
    let Some((latitude, longitude)) = value.split_once(',') else {
        return Err("Enter latitude, longitude (for example 48.8584, 2.2945)".to_owned());
    };
    let latitude: f64 = latitude.trim().parse().map_err(|_| "Invalid latitude")?;
    let longitude: f64 = longitude.trim().parse().map_err(|_| "Invalid longitude")?;
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-85.0..=85.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err("Latitude must be -85 to 85; longitude -180 to 180".to_owned());
    }
    Ok((latitude, longitude))
}

/// Lexical validation only; URL parsing and all I/O belong to the source worker.
pub fn validate_endpoint(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    let Some(rest) = value.strip_prefix("https://") else {
        return Err("Use an HTTPS Overpass interpreter endpoint".to_owned());
    };
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty()
        || value.len() > 2048
        || value.chars().any(char::is_whitespace)
        || value.contains(['@', '#', '?', '\\'])
    {
        return Err("Use an HTTPS endpoint without credentials, query or fragment".to_owned());
    }
    Ok(())
}

impl SceneSettings for OpenStreetMapSettings {
    fn normalized(&self) -> Self {
        let mut next = self.clone();
        next.place = next.place.min(PLACE_NAMES.len() - 1);
        next.tour = next.tour.min(2);
        next.camera = next.camera.min(2);
        next.source = next.source.min(2);
        next.dwell_seconds = next.dwell_seconds.clamp(30, 1800);
        next.camera_speed = next.camera_speed.clamp(0, 300);
        next.map_width_meters = next.map_width_meters.clamp(300, 2000);
        next.brightness_percent = next.brightness_percent.clamp(0, 200);
        next.line_width_percent = next.line_width_percent.clamp(25, 200);
        // Preserve malformed authored source values; the worker validates before I/O.
        next.local_path = next.local_path.trim().to_owned();
        next.endpoint = next.endpoint.trim().to_owned();
        next.coordinates = next.coordinates.trim().to_owned();
        next
    }
    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice("source", "Map source", self.source, &["World catalogue", "Local OSM extract", "Custom Overpass"],
            "The catalogue works offline. Custom sources use your own Overpass JSON file or explicit HTTPS service; no public service is contacted by default.")];
        if self.source == 0 {
            rows.extend([
                Control::choice("place", "Place", self.place, &PLACE_NAMES, "Choose a real OpenStreetMap extract from around the world."),
                Control::choice("tour", "Place selection", self.tour, &["Selected place", "World tour", "Shuffled tour"], "Hold the selected place or visit the offline catalogue in ordered or shuffled succession."),
                Control::slider("dwell_seconds", "Place duration", self.dwell_seconds, (30,1800,30), "s", "Time spent at each place during a tour. Global Speed applies."),
            ]);
        } else {
            if self.source == 1 {
                rows.push(Control::text("local_path", "OSM JSON file", &self.local_path, "absolute path", "An Overpass out geom JSON extract. Parsing runs on an owned worker with strict size and geometry bounds."));
            } else {
                rows.push(Control::text("endpoint", "Overpass service", &self.endpoint, "https://your-service/api/interpreter", "Choose a service you are authorized to use. One bounded cached query; pan and style changes reuse geometry. A public instance is unsuitable as a permanent app backend."));
            }
            rows.push(Control::text(
                "coordinates",
                "Map center",
                &self.coordinates,
                "latitude, longitude",
                "Center of the local file or bounded custom query, in WGS84 degrees.",
            ));
        }
        rows.extend([
            Control::choice("camera", "Camera", self.camera, &["Fixed", "Slow orbit", "East-west pan"], "Move only within the loaded extract. Camera motion makes no network requests."),
            Control::slider("camera_speed", "Pan speed", self.camera_speed, (0,300,10), "%", "100% completes a slow sweep in two minutes. 0 holds the camera; global Speed also applies."),
            Control::slider("map_width_meters", "Map width", self.map_width_meters, (300,2000,100), "m", "Width of the viewport in metres. Smaller values zoom in."),
            Control::slider("brightness_percent", "Map brightness", self.brightness_percent, (0,200,5), "%", "Dot intensity before shared density/dither; 0 hides every map dot. Shared Color controls tint the map."),
            Control::slider("line_width_percent", "Line weight", self.line_width_percent, (25,200,5), "%", "Weight of roads, railway tracks and building outlines in Braille dots."),
            Control::toggle("roads", "Roads and paths", self.roads, "OSM highway ways, including footpaths."),
            Control::toggle("buildings", "Buildings", self.buildings, "Closed OSM building footprints, including multipolygon holes."),
            Control::toggle("water", "Water", self.water, "Rivers, waterways and water-area polygons."),
            Control::toggle("green_space", "Green areas", self.green_space, "Parks, woods, grass, farmland and other green land-use polygons."),
            Control::toggle("railways", "Railways", self.railways, "OSM railway ways."),
            Control::toggle("points_of_interest", "Points of interest", self.points_of_interest, "Small dots at OSM amenity nodes."),
        ]);
        rows
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        use crate::control;
        let mut next = self.clone();
        match id {
            "place" | "tour" | "camera" | "source" => {
                let Some(index) = control::index(&value) else {
                    return Ok(false);
                };
                match id {
                    "place" => next.place = index,
                    "tour" => next.tour = index,
                    "camera" => next.camera = index,
                    _ => next.source = index,
                }
            }
            "roads" | "buildings" | "water" | "green_space" | "railways" | "points_of_interest" => {
                let Some(on) = control::boolean(&value) else {
                    return Ok(false);
                };
                match id {
                    "roads" => next.roads = on,
                    "buildings" => next.buildings = on,
                    "water" => next.water = on,
                    "green_space" => next.green_space = on,
                    "railways" => next.railways = on,
                    _ => next.points_of_interest = on,
                }
            }
            "dwell_seconds" | "camera_speed" | "map_width_meters" | "brightness_percent"
            | "line_width_percent" => {
                let Some(number) = control::number(&value) else {
                    return Ok(false);
                };
                match id {
                    "dwell_seconds" => next.dwell_seconds = number,
                    "camera_speed" => next.camera_speed = number,
                    "map_width_meters" => next.map_width_meters = number,
                    "brightness_percent" => next.brightness_percent = number,
                    _ => next.line_width_percent = number,
                }
            }
            "local_path" | "endpoint" | "coordinates" => {
                let Some(text) = control::text(&value) else {
                    return Ok(false);
                };
                let text = text.trim();
                match id {
                    "endpoint" => {
                        validate_endpoint(text)?;
                        next.endpoint = text.to_owned();
                    }
                    "coordinates" => {
                        parse_coordinates(text)?;
                        next.coordinates = text.to_owned();
                    }
                    _ => {
                        if text.len() > 4096 || text.contains(['\n', '\r', '\0']) {
                            return Err("Invalid local file path".to_owned());
                        }
                        next.local_path = text.to_owned();
                    }
                }
            }
            _ => return Ok(false),
        }
        next = next.normalized();
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlKind;
    #[test]
    fn layer_edits_are_independent_and_brightness_can_extinguish_map() {
        let mut settings = OpenStreetMapSettings::default();
        assert!(settings
            .set_control("roads", ControlValue::Bool(false))
            .unwrap());
        assert!(!settings.roads);
        assert!(settings.buildings && settings.water && settings.green_space && settings.railways);
        assert!(settings
            .set_control("brightness_percent", ControlValue::Number(0))
            .unwrap());
        assert_eq!(settings.brightness_percent, 0);
    }
    #[test]
    fn invalid_custom_source_edits_are_atomic() {
        let mut settings = OpenStreetMapSettings::default();
        let before = settings.clone();
        assert!(settings
            .set_control(
                "endpoint",
                ControlValue::Text("http://example.com/interpreter".into())
            )
            .is_err());
        assert_eq!(settings, before);
        assert!(settings
            .set_control("coordinates", ControlValue::Text("91, 181".into()))
            .is_err());
        assert_eq!(settings, before);
    }
    #[test]
    fn every_displayed_control_can_be_edited_and_round_tripped() {
        let mut settings = OpenStreetMapSettings::default();
        let controls = settings.controls();
        assert!(controls.len() >= 14);
        let mut ids = std::collections::HashSet::new();
        for control in controls {
            assert!(ids.insert(control.id));
            let value = match control.kind {
                ControlKind::Text { .. } => continue,
                _ => control.stepped(1).unwrap(),
            };
            assert!(
                settings.set_control(control.id, value).unwrap(),
                "{}",
                control.id
            );
        }
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<OpenStreetMapSettings>(&encoded).unwrap(),
            settings
        );
    }
    #[test]
    fn invalid_saved_custom_center_is_never_silently_replaced_with_paris() {
        let settings = OpenStreetMapSettings {
            source: 2,
            coordinates: "invalid authored center".into(),
            ..Default::default()
        }
        .normalized();
        assert_eq!(settings.coordinates, "invalid authored center");
        assert!(parse_coordinates(&settings.coordinates).is_err());
    }
    #[test]
    fn conditional_source_fields_are_visible_editable_and_retained() {
        let mut settings = OpenStreetMapSettings::default();
        settings
            .set_control("source", ControlValue::Index(1))
            .unwrap();
        assert!(settings.controls().iter().any(|row| row.id == "local_path"));
        assert!(settings
            .set_control("local_path", ControlValue::Text("/maps/tokyo.json".into()))
            .unwrap());
        assert!(settings
            .set_control(
                "coordinates",
                ControlValue::Text("35.6812, 139.7671".into())
            )
            .unwrap());
        settings
            .set_control("source", ControlValue::Index(2))
            .unwrap();
        assert!(settings.controls().iter().any(|row| row.id == "endpoint"));
        assert!(!settings.controls().iter().any(|row| row.id == "local_path"));
        assert!(settings
            .set_control(
                "endpoint",
                ControlValue::Text("https://example.com/api/interpreter".into())
            )
            .unwrap());
        assert_eq!(settings.local_path, "/maps/tokyo.json");
        assert_eq!(settings.coordinates, "35.6812, 139.7671");
        assert_eq!(
            serde_json::from_str::<OpenStreetMapSettings>(
                &serde_json::to_string(&settings).unwrap()
            )
            .unwrap(),
            settings
        );
    }
    #[test]
    fn malformed_persisted_values_normalize_before_rendering() {
        let settings = OpenStreetMapSettings {
            place: 999,
            tour: 999,
            camera: 999,
            source: 999,
            dwell_seconds: -1,
            map_width_meters: i32::MAX,
            brightness_percent: -200,
            ..Default::default()
        }
        .normalized();
        assert!(
            settings.place < 10 && settings.tour < 3 && settings.camera < 3 && settings.source < 3
        );
        assert_eq!(settings.dwell_seconds, 30);
        assert_eq!(settings.map_width_meters, 2000);
        assert_eq!(settings.brightness_percent, 0);
    }
}
