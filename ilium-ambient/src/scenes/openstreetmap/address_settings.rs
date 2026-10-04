//! Persisted search-provider selection. This module performs no I/O.
//! Search execution is separate from map-geometry acquisition.
use crate::control::{Control, ControlValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AddressProvider {
    #[default]
    Photon,
    Nominatim,
    CityOnly,
    Disabled,
}
impl AddressProvider {
    pub const ALL: [Self; 4] = [
        Self::Photon,
        Self::Nominatim,
        Self::CityOnly,
        Self::Disabled,
    ];
    pub const LABELS: [&'static str; 4] = [
        "Photon (city/address)",
        "Configured Nominatim",
        "Open-Meteo (city only)",
        "Disabled",
    ];
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddressSearchSettings {
    pub provider: AddressProvider,
    /// Complete Photon search endpoint, not a host to which paths are guessed.
    pub photon_endpoint: String,
    /// Complete /search endpoint of a service the user is authorized to use.
    pub nominatim_endpoint: String,
}
impl Default for AddressSearchSettings {
    fn default() -> Self {
        Self {
            provider: AddressProvider::Photon,
            photon_endpoint: "https://photon.komoot.io/api/".into(),
            nominatim_endpoint: String::new(),
        }
    }
}

/// Pure endpoint validation, also used before worker admission. Empty is an
/// editable unconfigured state, never authorization to select a public fallback.
/// HTTP Uri is already re-exported by the locked ureq dependency.
pub fn validate_search_endpoint(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 2048
        || !value.is_ascii()
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || value.contains(['@', '?', '#', '\\'])
    {
        return Err(
            "Use an ASCII HTTPS search endpoint without credentials, query or fragment".into(),
        );
    }
    let uri: ureq::http::Uri = value.parse().map_err(|_| "Invalid HTTPS search endpoint")?;
    let host = uri.host().ok_or("The search endpoint needs a host")?;
    if uri.scheme_str() != Some("https")
        || host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
    {
        return Err("Use an HTTPS search endpoint with an explicit host".into());
    }
    let authority = uri
        .authority()
        .ok_or("The search endpoint needs an authority")?
        .as_str();
    // Reject malformed/zero/overflow ports even if the URI parser retained them.
    let port = if authority.starts_with('[') {
        let (_, suffix) = authority.split_once(']').ok_or("Invalid IPv6 authority")?;
        if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or("Invalid service port")?)
        }
    } else {
        authority.split_once(':').map(|(_, port)| port)
    };
    if port.is_some_and(|port| port.parse::<u16>().ok().is_none_or(|port| port == 0)) {
        return Err("Invalid HTTPS search service port".into());
    }
    // Conservative product policy, not a claim that every manual OSMF use is
    // prohibited. A per-client limiter cannot establish aggregate permission.
    if host.eq_ignore_ascii_case("nominatim.openstreetmap.org") {
        return Err("The public OSMF Nominatim service is not offered by this adapter. Configure an authorized alternative; policy: https://operations.osmfoundation.org/policies/nominatim/".into());
    }
    Ok(())
}

impl AddressSearchSettings {
    pub fn normalized(&self) -> Self {
        Self {
            provider: self.provider,
            photon_endpoint: self.photon_endpoint.trim().to_owned(),
            nominatim_endpoint: self.nominatim_endpoint.trim().to_owned(),
        }
    }
    pub fn configured_endpoint(&self) -> Result<Option<&str>, String> {
        let endpoint = match self.provider {
            AddressProvider::Photon => self.photon_endpoint.trim(),
            AddressProvider::Nominatim => self.nominatim_endpoint.trim(),
            AddressProvider::CityOnly | AddressProvider::Disabled => return Ok(None),
        };
        validate_search_endpoint(endpoint)?;
        if endpoint.is_empty() {
            return Err("Configure an authorized HTTPS search endpoint first".into());
        }
        Ok(Some(endpoint))
    }
    pub fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice(
            "address_provider", "Place search", self.provider.index(), &AddressProvider::LABELS,
            "Only Enter in the OSM picker submits a search. Coordinates and map movement stay local. Search does not download map geometry.",
        )];
        match self.provider {
            AddressProvider::Photon => rows.push(Control::text(
                "photon_endpoint", "Photon service", &self.photon_endpoint,
                "https://your-service/api",
                "The default operator welcomes reasonable project API use, with no availability guarantee. Use your own service for larger workloads. https://github.com/komoot/photon#demo-server",
            )),
            AddressProvider::Nominatim => rows.push(Control::text(
                "nominatim_endpoint", "Nominatim service", &self.nominatim_endpoint,
                "https://your-service/search",
                "Configure a service you are authorized to use. No public OSMF fallback, autocomplete or bulk search. https://operations.osmfoundation.org/policies/nominatim/",
            )),
            AddressProvider::CityOnly => {
                rows[0].help_detail = Some("Existing city-name service, not house-address lookup. Its Open-Meteo terms apply: https://open-meteo.com/en/terms".into());
            }
            AddressProvider::Disabled => {}
        }
        rows
    }
    /// None denotes an unrelated control, preserving the scene's dispatcher.
    pub fn set_control(&mut self, id: &str, value: &ControlValue) -> Option<Result<bool, String>> {
        let mut next = self.clone();
        match id {
            "address_provider" => {
                let ControlValue::Index(index) = value else {
                    return Some(Ok(false));
                };
                let Some(provider) = AddressProvider::ALL.get(*index) else {
                    return Some(Err("Unknown address search provider".into()));
                };
                next.provider = *provider;
            }
            "photon_endpoint" | "nominatim_endpoint" => {
                let ControlValue::Text(text) = value else {
                    return Some(Ok(false));
                };
                let text = text.trim();
                if let Err(error) = validate_search_endpoint(text) {
                    return Some(Err(error));
                }
                if id == "photon_endpoint" {
                    next.photon_endpoint = text.into();
                } else {
                    next.nominatim_endpoint = text.into();
                }
            }
            _ => return None,
        }
        let changed = next != *self;
        *self = next;
        Some(Ok(changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_do_not_imply_a_public_nominatim_fallback() {
        let mut settings = AddressSearchSettings::default();
        assert_eq!(
            settings.configured_endpoint().unwrap(),
            Some("https://photon.komoot.io/api/")
        );
        settings.provider = AddressProvider::Nominatim;
        assert!(settings.configured_endpoint().is_err());
        settings.provider = AddressProvider::Disabled;
        assert_eq!(settings.configured_endpoint().unwrap(), None);
    }
    #[test]
    fn rejected_edits_are_atomic_and_saved_invalid_endpoints_stay_invalid() {
        let mut settings = AddressSearchSettings::default();
        for endpoint in [
            "http://host/search",
            "https:///search",
            "https://u:p@host/search",
            "https://host/search?token=x",
            "https://host/search#x",
            "https://host:0/search",
            "https://host:65536/search",
            "https://host:abc/search",
            "https://host./search",
            "https://NOMINATIM.OPENSTREETMAP.ORG/search",
            "https://host/\u{1b}search",
        ] {
            let before = settings.clone();
            assert!(
                settings
                    .set_control("nominatim_endpoint", &ControlValue::Text(endpoint.into()))
                    .unwrap()
                    .is_err(),
                "{endpoint}"
            );
            assert_eq!(settings, before);
        }
        settings.provider = AddressProvider::Nominatim;
        settings.nominatim_endpoint = "http://invalid".into();
        assert_eq!(settings.normalized().nominatim_endpoint, "http://invalid");
        assert!(settings.normalized().configured_endpoint().is_err());
    }
    #[test]
    fn endpoints_are_provider_specific_and_serializable() {
        let mut settings = AddressSearchSettings::default();
        let original_photon = settings.photon_endpoint.clone();
        assert!(settings
            .set_control(
                "nominatim_endpoint",
                &ControlValue::Text("https://maps.example.org:8443/search".into())
            )
            .unwrap()
            .unwrap());
        assert!(settings
            .set_control("address_provider", &ControlValue::Index(1))
            .unwrap()
            .unwrap());
        assert_eq!(
            settings.configured_endpoint().unwrap(),
            Some("https://maps.example.org:8443/search")
        );
        assert_eq!(settings.photon_endpoint, original_photon);
        assert_eq!(
            serde_json::from_str::<AddressSearchSettings>(
                &serde_json::to_string(&settings).unwrap()
            )
            .unwrap(),
            settings
        );
        assert!(validate_search_endpoint("https://[::1]:8443/search").is_ok());
    }
}
