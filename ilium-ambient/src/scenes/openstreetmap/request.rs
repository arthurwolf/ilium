//! Validated source identity. Presentation controls never change a request.
use super::{
    catalogue::PLACES,
    settings::{parse_coordinates, validate_endpoint, OpenStreetMapSettings},
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum MapRequest {
    Catalogue(usize),
    Local { path: PathBuf, center: [f64; 2] },
    Custom { url: String, center: [f64; 2] },
}
impl MapRequest {
    pub fn from_settings(settings: &OpenStreetMapSettings, place: usize) -> Result<Self, String> {
        if settings.is_list_tour() {
            // Validate the saved selection even when this frame points at a
            // different tour member. No malformed ID can turn into index zero.
            settings.selected_list_index()?;
            let destination = settings.active_list()?.destination(place)?;
            if let Some(index) = destination.bundle_index {
                return Ok(Self::Catalogue(index));
            }
            if settings.source == 0 {
                return Err("This saved theme needs a configured Overpass service; choose an offline tour list".into());
            }
            return Self::custom(settings.endpoint.trim(), destination.center)
                .map_err(|error| format!("{}: {error}", destination.label));
        }
        if settings.source == 0 {
            return Ok(Self::Catalogue(place.min(PLACES.len() - 1)));
        }
        let (lat, lon) = parse_coordinates(&settings.coordinates)?;
        let center = [lat, lon];
        match settings.source {
            1 => {
                let value = settings.local_path.trim();
                if value.is_empty() || value.len() > 4096 || value.contains(['\n', '\r', '\0']) {
                    return Err("Set an absolute OSM JSON file path (maximum 4096 bytes)".into());
                }
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err("OSM JSON file path must be absolute".into());
                }
                Ok(Self::Local { path, center })
            }
            2 => Self::custom(settings.endpoint.trim(), center),
            _ => Err("Unknown OSM map source".into()),
        }
    }
    /// Label is not part of geometry/cache identity. The scene can update a
    /// selected label without another download or decode.
    pub fn selection(
        settings: &OpenStreetMapSettings,
        place: usize,
    ) -> Result<(Self, String), String> {
        let request = Self::from_settings(settings, place)?;
        let center = request.center()?;
        let label = if settings.is_list_tour() {
            settings.active_list()?.destination(place)?.label.to_owned()
        } else {
            match &request {
                Self::Catalogue(index) => PLACES[*index].name.to_owned(),
                Self::Local { .. } => {
                    format!("Local extract at {:.4}, {:.4}", center[0], center[1])
                }
                Self::Custom { .. } => {
                    let location = settings.selected_location()?;
                    if settings.location_label.trim().is_empty() {
                        format!("Custom extract at {:.4}, {:.4}", center[0], center[1])
                    } else {
                        location.label
                    }
                }
            }
        };
        Ok((request, label))
    }
    fn custom(endpoint: &str, center: [f64; 2]) -> Result<Self, String> {
        validate_endpoint(endpoint)?;
        if endpoint.is_empty() {
            return Err("This OSM destination has no bundled geometry; set an authorized HTTPS Overpass endpoint".into());
        }
        // Keep the frozen source policy and URL bounds. The worker performs the
        // authoritative URI parse and never follows redirects.
        let authority = endpoint
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("");
        if authority.is_empty()
            || authority.contains(['[', ']'])
            || authority
                .split(':')
                .next()
                .is_none_or(|host| host.is_empty() || host.starts_with('.') || host.ends_with('.'))
        {
            return Err("Invalid HTTPS service authority".into());
        }
        if let Some((_, port)) = authority.split_once(':') {
            if port.parse::<u16>().ok().is_none_or(|port| port == 0) {
                return Err("Invalid HTTPS service port".into());
            }
        }
        let request = Self::Custom {
            url: format!("{endpoint}?data={}", encode_query(&bounded_query(center))),
            center,
        };
        request.center()?;
        Ok(request)
    }
    pub fn center(&self) -> Result<[f64; 2], String> {
        let center = match self {
            Self::Catalogue(index) => {
                let place = PLACES.get(*index).ok_or("Unknown catalogue place")?;
                [place.latitude, place.longitude]
            }
            Self::Local { center, .. } | Self::Custom { center, .. } => *center,
        };
        if !center.iter().all(|value| value.is_finite())
            || !(-85. ..=85.).contains(&center[0])
            || !(-180. ..=180.).contains(&center[1])
        {
            return Err("Invalid OSM request center".into());
        }
        Ok(center)
    }
}
fn bounded_query(center: [f64; 2]) -> String {
    // Match the proven catalogue acquisition window. Geometry is clipped at
    // the window boundary; the decoder preserves null gaps and open rings.
    let [lat, lon] = center;
    let south = (lat - 0.008).max(-85.);
    let north = (lat + 0.008).min(85.);
    let west = (lon - 0.012).max(-180.);
    let east = (lon + 0.012).min(180.);
    let bbox = format!("{south:.7},{west:.7},{north:.7},{east:.7}");
    let filters = [
        "highway",
        "building",
        "building:part",
        "waterway",
        "natural",
        "landuse",
        "leisure",
        "railway",
        "amenity",
        "tourism",
        "historic",
        "shop",
    ];
    let selectors = filters
        .iter()
        .map(|tag| format!("nwr[\"{tag}\"]({bbox});"))
        .collect::<String>();
    format!("[out:json][timeout:15][maxsize:16777216];({selectors});out geom({bbox});")
}
fn encode_query(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 3);
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_catalogue_ignores_unused_custom_fields() {
        let settings = OpenStreetMapSettings {
            coordinates: "invalid".into(),
            endpoint: "http://invalid".into(),
            ..Default::default()
        };
        assert_eq!(
            MapRequest::from_settings(&settings, 3).unwrap(),
            MapRequest::Catalogue(3)
        );
    }
    #[test]
    fn custom_sources_validate_before_any_io_and_style_does_not_change_identity() {
        let mut settings = OpenStreetMapSettings {
            source: 1,
            local_path: "relative.json".into(),
            ..Default::default()
        };
        assert!(MapRequest::from_settings(&settings, 0).is_err());
        settings.local_path = "/maps/osm.json".into();
        let original = MapRequest::from_settings(&settings, 0).unwrap();
        settings.brightness_percent = 1;
        settings.roads = false;
        settings.camera = 0;
        settings.place = 5;
        assert_eq!(MapRequest::from_settings(&settings, 9).unwrap(), original);
        settings.source = 2;
        for endpoint in [
            "",
            "http://example.com/api",
            "https://host:0/api",
            "https://host:abc/api",
            "https://user@host/api",
            "https://host/api?secret=x",
        ] {
            settings.endpoint = endpoint.into();
            assert!(
                MapRequest::from_settings(&settings, 0).is_err(),
                "{endpoint}"
            );
        }
        settings.endpoint = "https://example.org:8443/api/interpreter".into();
        let MapRequest::Custom { url, .. } = MapRequest::from_settings(&settings, 0).unwrap()
        else {
            panic!("custom request expected")
        };
        assert!(url.starts_with("https://example.org:8443/api/interpreter?data=%5Bout%3Ajson%5D"));
        assert!(!url.contains(' '));
        settings.coordinates = "invalid saved coordinates".into();
        assert!(MapRequest::from_settings(&settings, 0).is_err());
    }
}

#[cfg(test)]
mod expanded_selection_tests {
    use super::super::{places, settings::SelectionMode};
    use super::*;
    fn themed(id: &str) -> OpenStreetMapSettings {
        OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            place_list: id.into(),
            endpoint: "https://maps.example.org/api/interpreter".into(),
            ..Default::default()
        }
    }
    #[test]
    fn every_unbundled_destination_requests_its_own_actual_center() {
        for list in places::LISTS {
            let settings = themed(list.id);
            for (index, id) in list.members.iter().enumerate() {
                let destination = places::destination(id).unwrap();
                let (request, label) = MapRequest::selection(&settings, index).unwrap();
                assert_eq!(request.center().unwrap(), destination.center);
                assert_eq!(label, destination.label);
                match destination.bundle_index {
                    Some(bundle) => assert_eq!(request, MapRequest::Catalogue(bundle)),
                    None => assert!(matches!(request, MapRequest::Custom { .. })),
                }
            }
        }
    }
    #[test]
    fn bad_or_unconfigured_lists_cannot_fall_back_to_a_nearby_bundle() {
        let mut settings = themed("historic-towns");
        settings.endpoint.clear();
        assert!(MapRequest::from_settings(&settings, 0)
            .unwrap_err()
            .contains("no bundled geometry"));
        settings.place_list = "not-a-real-list".into();
        assert!(MapRequest::from_settings(&settings, 0).is_err());
        settings.place_list = "historic-towns".into();
        settings.destination_id = "paris".into();
        assert!(MapRequest::from_settings(&settings, 0).is_err());
        settings.destination_id.clear();
        assert!(MapRequest::from_settings(&settings, usize::MAX).is_err());
    }
    #[test]
    fn listed_bundles_need_no_endpoint_but_arbitrary_nearby_points_do() {
        let mut settings = themed("offline-world");
        settings.endpoint.clear();
        assert_eq!(
            MapRequest::from_settings(&settings, 0).unwrap(),
            MapRequest::Catalogue(0)
        );
        settings.selection = SelectionMode::Selected;
        settings.coordinates = "48.858401, 2.294501".into();
        assert!(MapRequest::from_settings(&settings, 0).is_err());
        settings.endpoint = "https://maps.example.org/api/interpreter".into();
        assert!(matches!(
            MapRequest::from_settings(&settings, 0).unwrap(),
            MapRequest::Custom { .. }
        ));
    }
    #[test]
    fn geometry_identity_excludes_labels_search_and_presentation_but_includes_source_and_center() {
        let mut settings = OpenStreetMapSettings {
            source: 2,
            endpoint: "https://maps.example.org/api/interpreter".into(),
            location_label: "First label".into(),
            ..Default::default()
        };
        let original = MapRequest::selection(&settings, 0).unwrap();
        settings.location_label = "Corrected label".into();
        settings.camera = 0;
        settings.brightness_percent = 0;
        settings.search.provider = super::super::address_settings::AddressProvider::Disabled;
        let changed = MapRequest::selection(&settings, 0).unwrap();
        assert_eq!(original.0, changed.0);
        assert_ne!(original.1, changed.1);
        settings.coordinates = "0, 0".into();
        assert_ne!(MapRequest::from_settings(&settings, 0).unwrap(), original.0);
        settings.coordinates = "48.8584, 2.2945".into();
        settings.endpoint = "https://other.example.org/api/interpreter".into();
        assert_ne!(MapRequest::from_settings(&settings, 0).unwrap(), original.0);
    }

    #[test]
    fn offline_themes_resolve_their_actual_bundle_and_reject_unbundled_members() {
        for list in places::offline_lists() {
            let settings = OpenStreetMapSettings {
                source: 0,
                selection: SelectionMode::List,
                place_list: list.id.into(),
                endpoint: String::new(),
                ..Default::default()
            };
            for (index, id) in list.members.iter().enumerate() {
                let destination = places::destination(id).unwrap();
                let (request, label) = MapRequest::selection(&settings, index).unwrap();
                assert_eq!(
                    request,
                    MapRequest::Catalogue(destination.bundle_index.unwrap())
                );
                assert_eq!(request.center().unwrap(), destination.center);
                assert_eq!(label, destination.label);
            }
        }
        let invalid = OpenStreetMapSettings {
            source: 0,
            selection: SelectionMode::List,
            place_list: "historic-towns".into(),
            ..Default::default()
        };
        assert!(MapRequest::from_settings(&invalid, 0).is_err());
    }
}
