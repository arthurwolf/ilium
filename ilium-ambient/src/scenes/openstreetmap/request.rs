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
            2 => {
                let endpoint = settings.endpoint.trim();
                validate_endpoint(endpoint)?;
                if endpoint.is_empty() {
                    return Err("Set an authorized HTTPS Overpass endpoint".into());
                }
                // Lexical checks precede the HTTP adapter's authoritative URI parse.
                let authority = endpoint
                    .strip_prefix("https://")
                    .and_then(|rest| rest.split('/').next())
                    .unwrap_or("");
                if authority.is_empty()
                    || authority.contains(['[', ']'])
                    || authority.split(':').next().is_none_or(|host| {
                        host.is_empty() || host.starts_with('.') || host.ends_with('.')
                    })
                {
                    return Err("Invalid HTTPS service authority".into());
                }
                if let Some((_, port)) = authority.split_once(':') {
                    if port.parse::<u16>().ok().is_none_or(|port| port == 0) {
                        return Err("Invalid HTTPS service port".into());
                    }
                }
                let query = bounded_query(center);
                Ok(Self::Custom {
                    url: format!("{endpoint}?data={}", encode_query(&query)),
                    center,
                })
            }
            _ => Err("Unknown OSM map source".into()),
        }
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
