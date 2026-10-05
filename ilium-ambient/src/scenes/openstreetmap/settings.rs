//! Project-persisted OpenStreetMap presentation and explicit source selection.
use super::{address_settings::AddressSearchSettings, places};
use crate::control::{Control, ControlValue, SceneSettings};
use crate::location::GeoLocation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    #[default]
    Selected,
    List,
}

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
    /// Label for `coordinates`, never the shared observer location.
    pub location_label: String,
    /// Source 0 can use bundled lists; source 2 can use every named list.
    pub selection: SelectionMode,
    pub place_list: String,
    /// Empty means the initial member (legacy `place` for offline-world).
    /// Nonempty unknown IDs are errors, never nearest-place substitutes.
    pub destination_id: String,
    pub search: AddressSearchSettings,
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
            location_label: String::new(),
            selection: SelectionMode::Selected,
            place_list: places::OFFLINE_LIST_ID.into(),
            destination_id: String::new(),
            search: AddressSearchSettings::default(),
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

impl OpenStreetMapSettings {
    pub fn active_list(&self) -> Result<&'static places::PlaceList, String> {
        places::list(&self.place_list)
            .ok_or_else(|| "Unknown saved OSM place list; choose a listed theme".to_owned())
    }
    pub fn selected_list_index(&self) -> Result<usize, String> {
        let list = self.active_list()?;
        if list.members.is_empty() {
            return Err("The OSM place list is empty".into());
        }
        if self.destination_id.is_empty() {
            return Ok(if list.id == places::OFFLINE_LIST_ID {
                self.place.min(list.members.len() - 1)
            } else {
                0
            });
        }
        list.index_of(&self.destination_id)
            .ok_or_else(|| "The saved destination is not a member of the selected OSM list".into())
    }
    pub fn is_list_tour(&self) -> bool {
        matches!(self.source, 0 | 2) && self.selection == SelectionMode::List
    }
    /// Display/picker access validates raw authored coordinates before making a
    /// GeoLocation. Preserve the validated values independently of constructor normalization.
    pub fn selected_location(&self) -> Result<GeoLocation, String> {
        let (latitude, longitude) = parse_coordinates(&self.coordinates)?;
        validate_label(&self.location_label)?;
        let mut location = GeoLocation::new(self.location_label.trim(), latitude, longitude);
        location.latitude = latitude;
        location.longitude = longitude;
        location.label = self.location_label.trim().to_owned();
        if location.label.is_empty() {
            location.label = location.coordinate_text();
        }
        Ok(location)
    }
    /// Initial selected anchor, not a claim about the running tour's current stop.
    pub fn picker_location(&self) -> Result<GeoLocation, String> {
        if self.is_list_tour() {
            let place = self
                .active_list()?
                .destination(self.selected_list_index()?)?;
            return Ok(GeoLocation::new(
                place.label,
                place.center[0],
                place.center[1],
            ));
        }
        if self.source == 0 {
            let place = &super::catalogue::PLACES[self.place.min(PLACE_NAMES.len() - 1)];
            return Ok(GeoLocation::new(
                place.name,
                place.latitude,
                place.longitude,
            ));
        }
        self.selected_location()
    }
    /// Atomic OSM-only selection. Arbitrary points switch to the explicitly
    /// configured geometry service, even when the previous source was a file.
    /// An empty endpoint stays empty and the request resolver explains it.
    pub fn set_selected_location(&mut self, location: &GeoLocation) -> Result<bool, String> {
        validate_label(&location.label)?;
        let coordinates = format!("{}, {}", location.latitude, location.longitude);
        parse_coordinates(&coordinates)?;
        let mut next = self.clone();
        next.coordinates = coordinates;
        next.location_label = location.label.trim().to_owned();
        next.source = 2;
        next.selection = SelectionMode::Selected;
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
    /// Only an explicit acquisition/selection edit releases failure latches.
    /// Search-provider, label, palette and camera edits do not authorize retries.
    pub(crate) fn same_acquisition(&self, other: &Self) -> bool {
        self.source == other.source
            && self.local_path == other.local_path
            && self.endpoint == other.endpoint
            && self.coordinates == other.coordinates
            && self.selection == other.selection
            && self.place_list == other.place_list
            && self.destination_id == other.destination_id
            && self.place == other.place
            && self.tour == other.tour
            && self.dwell_seconds == other.dwell_seconds
    }
    fn list_controls(&self) -> Vec<Control> {
        let available = self.available_lists();
        let mut names: Vec<&'static str> = available.iter().map(|list| list.label).collect();
        let index = available
            .iter()
            .position(|list| list.id == self.place_list)
            .unwrap_or_else(|| {
                names.push("Unknown saved list (choose another)");
                names.len() - 1
            });
        let mut list_control = Control::choice(
            "place_list", "Tour list", index, &names,
            "The offline themes use bundled OSM extracts. Other themes require an authorized Overpass service; lists never geocode or prefetch.",
        );
        if index == available.len() {
            list_control = list_control.with_disabled_option(
                index,
                "Saved list is unavailable for this map source; choose a listed theme to replace it.",
            );
        }
        let mut rows = vec![list_control];
        if let Some(list) = self
            .active_list()
            .ok()
            .filter(|list| self.source != 0 || list.is_bundled())
        {
            let mut labels: Vec<&'static str> = list
                .members
                .iter()
                .filter_map(|id| places::destination(id).map(|place| place.label))
                .collect();
            let available_destinations = labels.len();
            let index = self.selected_list_index().unwrap_or_else(|_| {
                labels.push("Unknown saved destination (choose another)");
                labels.len() - 1
            });
            let mut destination_control = Control::choice("destination", "Starting place", index, &labels,
                "Choose the first stop or the held place. Stable destination IDs are saved, not the visible row index.");
            if index == available_destinations {
                destination_control = destination_control.with_disabled_option(
                    index,
                    "Saved destination is unavailable in this list; choose a listed place to replace it.",
                );
            }
            rows.push(destination_control);
        }
        rows.extend([
            Control::choice("tour", "Place selection", self.tour, &["Selected place", "Ordered tour", "Shuffled tour"],
                "Every member occurs once per cycle; shuffled tours keep the selected place first. Unseen elapsed stops are not fetched on catch-up."),
            Control::slider("dwell_seconds", "Place duration", self.dwell_seconds.max(60), (60,1800,30), "s",
                "List tours use unscaled wall time with a 60-second minimum. Global Speed affects camera motion, not network admission. Failed stops stay failed until an explicit source/selection edit."),
        ]);
        rows
    }

    fn available_lists(&self) -> Vec<&'static places::PlaceList> {
        if self.source == 0 {
            places::offline_lists().collect()
        } else {
            places::LISTS.iter().collect()
        }
    }
}

fn validate_label(value: &str) -> Result<(), String> {
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(
            "OSM location label must be at most 512 bytes without control characters".into(),
        );
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
        next.location_label = next.location_label.trim().to_owned();
        next.place_list = next.place_list.trim().to_owned();
        next.destination_id = next.destination_id.trim().to_owned();
        next.search = next.search.normalized();
        next
    }
    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice("source", "Map source", self.source, &["World catalogue", "Local OSM extract", "Custom Overpass"],
            "The catalogue works offline. Custom sources use your own Overpass JSON file or explicit HTTPS service; no public service is contacted by default.")];
        if self.source == 0 {
            rows.push(Control::choice("selection", "Catalogue mode",
                usize::from(self.selection == SelectionMode::List),
                &["Classic place", "Offline tour list"],
                "Classic mode retains the original ten-place tour and global Speed. Offline tour lists use the same bundled extracts, stable member IDs and unscaled wall time."));
            if self.is_list_tour() {
                rows.extend(self.list_controls());
            } else {
                rows.extend([
                    Control::choice("place", "Place", self.place, &PLACE_NAMES, "Choose a real OpenStreetMap extract from around the world."),
                    Control::choice("tour", "Place selection", self.tour, &["Selected place", "Ordered tour", "Shuffled tour"], "Hold the selected place or visit the offline catalogue in ordered or shuffled succession."),
                    Control::slider("dwell_seconds", "Place duration", self.dwell_seconds, (30,1800,30), "s", "Time spent at each place during a tour. Global Speed applies."),
                ]);
                rows.push(self.list_controls().remove(0));
            }
        } else {
            if self.source == 1 {
                rows.push(Control::text("local_path", "OSM JSON file", &self.local_path, "absolute path", "An Overpass out geom JSON extract. Parsing runs on an owned worker with strict size and geometry bounds."));
            } else {
                rows.push(Control::text("endpoint", "Overpass service", &self.endpoint, "https://your-service/api/interpreter", "Choose a service you are authorized to use. One bounded cached query; pan and style changes reuse geometry. A public instance is unsuitable as a permanent app backend."));
            }
            if self.source == 2 {
                rows.push(Control::choice("selection", "Destination mode",
                    usize::from(self.selection == SelectionMode::List),
                    &["Selected coordinates", "Named place list"],
                    "Selected coordinates use the OSM picker or Map center; named lists visit real static anchors. Neither changes the sky/weather observer."));
            }
            if self.is_list_tour() {
                rows.extend(self.list_controls());
            } else {
                rows.push(Control::text(
                    "coordinates", "Map center", &self.coordinates, "latitude, longitude",
                    "Center of the local file or bounded custom query, in WGS84 degrees. Editing coordinates clears any old place label.",
                ));
            }
        }
        rows.extend(self.search.controls());
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
        if let Some(result) = next.search.set_control(id, &value) {
            let changed = result?;
            if changed {
                *self = next;
            }
            return Ok(changed);
        }
        match id {
            "selection" => {
                let Some(index) = control::index(&value) else {
                    return Ok(false);
                };
                next.selection = match index {
                    0 => SelectionMode::Selected,
                    1 => SelectionMode::List,
                    _ => return Err("Unknown OSM destination mode".into()),
                };
            }
            "place_list" => {
                let Some(index) = control::index(&value) else {
                    return Ok(false);
                };
                let list = self
                    .available_lists()
                    .get(index)
                    .copied()
                    .ok_or("Unknown OSM place list")?;
                let old_id = self.active_list().ok().and_then(|old| {
                    self.selected_list_index()
                        .ok()
                        .and_then(|index| old.members.get(index).copied())
                });
                let id = old_id
                    .filter(|id| list.index_of(id).is_some())
                    .or_else(|| list.members.first().copied())
                    .ok_or("Empty OSM place list")?;
                next.place_list = list.id.into();
                next.destination_id = id.into();
                if self.source == 0 {
                    next.selection = SelectionMode::List;
                }
            }
            "destination" => {
                let Some(index) = control::index(&value) else {
                    return Ok(false);
                };
                next.destination_id = next.active_list()?.destination(index)?.id.into();
            }
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
                        next.location_label.clear();
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
    fn themed_place_lists_are_exposed_as_a_real_choice() {
        let controls = OpenStreetMapSettings::default().controls();
        let list = controls
            .iter()
            .find(|control| control.label == "Tour list")
            .expect("themed OSM tours must be configurable");
        let ControlKind::Choice { options } = &list.kind else {
            panic!("tour lists must be individually selectable")
        };
        assert!(options.len() >= 4, "offer several interesting themed tours");
    }
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

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn old_saved_settings_keep_their_source_and_center_without_migration_io() {
        let settings: OpenStreetMapSettings = serde_json::from_str(
            r#"{"source":2,"coordinates":"35.6812, 139.7671","place":4,"tour":2}"#,
        )
        .unwrap();
        assert_eq!(settings.source, 2);
        assert_eq!(settings.selection, SelectionMode::Selected);
        assert_eq!(settings.coordinates, "35.6812, 139.7671");
        assert_eq!(settings.place, 4);
        assert_eq!(settings.tour, 2);
        assert_eq!(settings.place_list, "offline-world");
        assert_eq!(settings.search, AddressSearchSettings::default());
    }
    #[test]
    fn selecting_a_point_is_atomic_and_cannot_change_the_observer() {
        let mut ambient = crate::registry::AmbientSettings::default();
        let observer = ambient.location.clone();
        ambient.openstreetmap.source = 1;
        ambient.openstreetmap.local_path = "/unchanged/map.json".into();
        let selected = GeoLocation::new("A chosen point", 10.123456789, -20.987654321);
        assert!(ambient
            .openstreetmap
            .set_selected_location(&selected)
            .unwrap());
        assert_eq!(ambient.location, observer);
        assert_eq!(ambient.openstreetmap.source, 2);
        assert_eq!(ambient.openstreetmap.selection, SelectionMode::Selected);
        assert_eq!(ambient.openstreetmap.selected_location().unwrap(), selected);
        assert_eq!(ambient.openstreetmap.local_path, "/unchanged/map.json");
        assert!(ambient.openstreetmap.endpoint.is_empty());
        assert!(!ambient
            .openstreetmap
            .set_selected_location(&selected)
            .unwrap());
        for (latitude, longitude) in [
            (86., 0.),
            (-86., 0.),
            (0., 181.),
            (f64::NAN, 0.),
            (0., f64::INFINITY),
        ] {
            let mut invalid = selected.clone();
            // Mutate raw values after construction to exercise the strict boundary.
            invalid.latitude = latitude;
            invalid.longitude = longitude;
            let before = ambient.openstreetmap.clone();
            assert!(ambient
                .openstreetmap
                .set_selected_location(&invalid)
                .is_err());
            assert_eq!(ambient.openstreetmap, before);
            assert_eq!(ambient.location, observer);
        }
    }
    #[test]
    fn labels_round_trip_and_coordinate_edits_cannot_keep_an_unrelated_label() {
        let mut settings = OpenStreetMapSettings::default();
        settings
            .set_selected_location(&GeoLocation::new("Public landmark", 48., 2.))
            .unwrap();
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<OpenStreetMapSettings>(&encoded).unwrap(),
            settings
        );
        settings
            .set_control("coordinates", ControlValue::Text("49, 3".into()))
            .unwrap();
        assert!(settings.location_label.is_empty());
        assert_eq!(settings.selected_location().unwrap().latitude, 49.);
        let before = settings.clone();
        let mut bad = GeoLocation::new("valid", 1., 2.);
        bad.label = "bad\u{1b}label".into();
        assert!(settings.set_selected_location(&bad).is_err());
        assert_eq!(settings, before);
    }
    #[test]
    fn explicit_list_edits_save_ids_and_preserve_only_members_of_the_new_list() {
        let mut settings = OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            ..Default::default()
        };
        let historic = places::LISTS
            .iter()
            .position(|list| list.id == "historic-towns")
            .unwrap();
        let parks = places::LISTS
            .iter()
            .position(|list| list.id == "parks")
            .unwrap();
        let all = places::LISTS
            .iter()
            .position(|list| list.id == "world-sampler")
            .unwrap();
        settings
            .set_control("place_list", ControlValue::Index(historic))
            .unwrap();
        settings
            .set_control("destination", ControlValue::Index(1))
            .unwrap();
        assert_eq!(settings.destination_id, "dubrovnik");
        settings
            .set_control("place_list", ControlValue::Index(all))
            .unwrap();
        assert_eq!(settings.destination_id, "dubrovnik");
        settings
            .set_control("place_list", ControlValue::Index(parks))
            .unwrap();
        assert_eq!(settings.destination_id, "central-park");
        let before = settings.clone();
        assert!(settings
            .set_control("destination", ControlValue::Index(usize::MAX))
            .is_err());
        assert_eq!(settings, before);
        assert!(settings
            .set_control("place_list", ControlValue::Index(usize::MAX))
            .is_err());
        assert_eq!(settings, before);
    }
    #[test]
    fn unknown_saved_ids_are_visible_and_fail_closed_instead_of_becoming_paris() {
        let mut settings = OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            place_list: "removed-list".into(),
            destination_id: "removed-stop".into(),
            ..Default::default()
        }
        .normalized();
        assert_eq!(settings.place_list, "removed-list");
        assert!(settings.selected_list_index().is_err());
        assert!(settings
            .controls()
            .iter()
            .find(|row| row.id == "place_list")
            .unwrap()
            .display_value()
            .contains("Unknown saved"));
        settings.place_list = "parks".into();
        assert!(settings
            .controls()
            .iter()
            .find(|row| row.id == "destination")
            .unwrap()
            .display_value()
            .contains("Unknown saved"));
        assert!(settings.picker_location().is_err());
    }
    #[test]
    fn all_source_and_search_controls_round_trip_without_io() {
        for source in 0..=2 {
            for selection in [SelectionMode::Selected, SelectionMode::List] {
                for provider in super::super::address_settings::AddressProvider::ALL {
                    let mut settings = OpenStreetMapSettings {
                        source,
                        selection,
                        place_list: "parks".into(),
                        ..Default::default()
                    };
                    settings.search.provider = provider;
                    let mut ids = std::collections::BTreeSet::new();
                    for control in settings.controls() {
                        assert!(ids.insert(control.id));
                        let mut edited = settings.clone();
                        if let Some(value) = control.stepped(1) {
                            edited.set_control(control.id, value).unwrap();
                        }
                        let saved = serde_json::to_string(&edited).unwrap();
                        assert_eq!(
                            serde_json::from_str::<OpenStreetMapSettings>(&saved).unwrap(),
                            edited
                        );
                    }
                    assert!(ids.contains("source") && ids.contains("address_provider"));
                    assert_eq!(
                        ids.contains("place_list"),
                        source == 0 || (source == 2 && selection == SelectionMode::List)
                    );
                    assert_eq!(
                        ids.contains("coordinates"),
                        source != 0 && !(source == 2 && selection == SelectionMode::List)
                    );
                }
            }
        }
    }

    #[test]
    fn choosing_an_offline_theme_activates_it_without_a_geometry_service() {
        let mut settings = OpenStreetMapSettings::default();
        let choice = settings
            .controls()
            .into_iter()
            .find(|row| row.id == "place_list")
            .unwrap();
        let available = settings.available_lists();
        assert!(available.len() >= 4);
        assert_eq!(choice.display_value(), available[0].label);
        settings
            .set_control("place_list", ControlValue::Index(1))
            .unwrap();
        assert_eq!(settings.source, 0);
        assert_eq!(settings.selection, SelectionMode::List);
        assert_eq!(settings.place_list, available[1].id);
        assert!(settings
            .controls()
            .iter()
            .any(|row| row.id == "destination"));
        let saved = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<OpenStreetMapSettings>(&saved).unwrap(),
            settings
        );
    }

    #[test]
    fn unknown_saved_list_can_be_replaced_with_a_real_offline_choice() {
        let mut settings = OpenStreetMapSettings {
            place_list: "removed-theme".into(),
            ..Default::default()
        };
        let row = settings
            .controls()
            .into_iter()
            .find(|row| row.id == "place_list")
            .unwrap();
        assert_eq!(row.display_value(), "Unknown saved list (choose another)");
        settings
            .set_control("place_list", ControlValue::Index(0))
            .unwrap();
        assert_eq!(settings.place_list, places::OFFLINE_LIST_ID);
        assert_eq!(settings.selection, SelectionMode::List);
    }

    #[test]
    fn unknown_saved_choices_are_visible_but_cannot_be_selected() {
        for (id, settings) in [
            (
                "place_list",
                OpenStreetMapSettings {
                    place_list: "removed-theme".into(),
                    ..Default::default()
                },
            ),
            (
                "destination",
                OpenStreetMapSettings {
                    selection: SelectionMode::List,
                    destination_id: "removed-destination".into(),
                    ..Default::default()
                },
            ),
        ] {
            let row = settings
                .controls()
                .into_iter()
                .find(|row| row.id == id)
                .unwrap();
            let ControlValue::Index(placeholder) = row.value else {
                panic!("expected choice");
            };
            assert!(row.display_value().starts_with("Unknown saved"));
            assert!(
                row.disabled_reason(placeholder).is_some(),
                "{id}: stale saved value was advertised as selectable"
            );
            for direction in [-1, 1] {
                let value = row.stepped(direction).unwrap();
                assert_ne!(value, ControlValue::Index(placeholder));
                let mut next = settings.clone();
                next.set_control(id, value).unwrap();
            }
            let mut next = settings.clone();
            assert!(next
                .set_control(id, ControlValue::Index(placeholder))
                .is_err());
            assert_eq!(next, settings);
        }
    }
}
