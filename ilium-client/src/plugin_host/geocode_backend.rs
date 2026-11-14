//! The place and address search behind `ilium:host/geocode` for plugin scenes.
//!
//! The import (`ilium_plugin_host::imports::geocode`) owns the contract: validation, the rate,
//! the worker threads and `take-places`. This backend asks the providers the kernel location
//! picker already uses: the OpenStreetMap address search (Photon, full addresses) first, then the
//! Open-Meteo place search (city names, and coordinates typed as text). Both send the Ilium user
//! agent, throttle per host and cache answers on disk, so a repeated query does not reach the
//! network again. No reverse geocoding provider exists in the kernel yet, so `reverse` fails and
//! the request completes without places.

use ilium_ambient::{AddressSearchSettings, GeoLocation};
use ilium_plugin_api::generated::host::geocode::GeocodeQuery;
use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
use ilium_plugin_host::imports::geocode::GeocodeBackend;

/// One blocking provider search: candidates best first, or a short reason.
type Search = fn(&str) -> Result<Vec<GeoLocation>, String>;

/// The start of the Open-Meteo search's error for a query that matched no place, which is an
/// answer (no place), not a failure.
const NO_PLACE_PREFIX: &str = "No place found";

fn address_search(text: &str) -> Result<Vec<GeoLocation>, String> {
    ilium_ambient::openstreetmap_address_search::search(&AddressSearchSettings::default(), text)
}

fn place_search(text: &str) -> Result<Vec<GeoLocation>, String> {
    ilium_ambient::geocode::search(text)
}

/// The client's geocoder: address search first, place search when it has no answer.
pub struct ClientGeocoder {
    address: Search,
    places: Search,
}

impl ClientGeocoder {
    /// The geocoder on the real providers.
    pub fn new() -> Self {
        Self {
            address: address_search,
            places: place_search,
        }
    }
}

impl Default for ClientGeocoder {
    fn default() -> Self {
        Self::new()
    }
}

fn place(location: GeoLocation) -> GeoPlace {
    GeoPlace {
        point: GeoPoint {
            lat: location.latitude,
            lon: location.longitude,
        },
        label: location.label,
        country_code: None,
        kind: None,
    }
}

impl GeocodeBackend for ClientGeocoder {
    fn search(&self, query: &GeocodeQuery) -> Result<Vec<GeoPlace>, String> {
        let text = query.text.trim();
        // `Ok(())` when the address search answered with no place.
        let address = match (self.address)(text) {
            Ok(found) if !found.is_empty() => return Ok(found.into_iter().map(place).collect()),
            other => other.map(|_| ()),
        };
        match (self.places)(text) {
            Ok(found) => Ok(found.into_iter().map(place).collect()),
            // Both providers answered: no place is an answer.
            Err(reason) if reason.starts_with(NO_PLACE_PREFIX) => Ok(Vec::new()),
            Err(_) if address.is_ok() => Ok(Vec::new()),
            Err(reason) => Err(match address {
                Err(address_reason) => format!("{address_reason}; {reason}"),
                Ok(_) => reason,
            }),
        }
    }

    fn reverse(&self, _point: GeoPoint) -> Result<Vec<GeoPlace>, String> {
        Err("no reverse geocoding provider".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(label: &str) -> GeoLocation {
        GeoLocation {
            label: label.to_owned(),
            latitude: 38.72,
            longitude: -9.14,
        }
    }

    fn query(text: &str) -> GeocodeQuery {
        GeocodeQuery {
            text: text.to_owned(),
            near: None,
            limit: 5,
            language: None,
        }
    }

    fn labels(result: Result<Vec<GeoPlace>, String>) -> Result<Vec<String>, String> {
        result.map(|places| places.into_iter().map(|place| place.label).collect())
    }

    #[test]
    fn an_address_answer_wins_and_keeps_its_coordinates() {
        let geocoder = ClientGeocoder {
            address: |_| Ok(vec![located("Rua Augusta 1, Lisboa")]),
            places: |_| panic!("the place search is not asked"),
        };
        let places = geocoder.search(&query("Rua Augusta 1")).expect("answer");
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].label, "Rua Augusta 1, Lisboa");
        assert_eq!(
            places[0].point,
            GeoPoint {
                lat: 38.72,
                lon: -9.14
            }
        );
    }

    #[test]
    fn the_place_search_answers_when_the_address_search_has_nothing_or_fails() {
        let nothing: Search = |_| Ok(Vec::new());
        let cooling: Search = |_| Err("Address service cooldown".to_owned());
        for address in [nothing, cooling] {
            let geocoder = ClientGeocoder {
                address,
                places: |_| Ok(vec![located("Lisbon, Portugal")]),
            };
            assert_eq!(
                labels(geocoder.search(&query("Lisbon"))),
                Ok(vec!["Lisbon, Portugal".to_owned()])
            );
        }
    }

    #[test]
    fn no_place_is_an_answer_and_two_failures_are_a_failure() {
        let nothing = ClientGeocoder {
            address: |_| Err("down".to_owned()),
            places: |text| Err(format!("No place found for \"{text}\"")),
        };
        assert_eq!(labels(nothing.search(&query("Atlantis"))), Ok(Vec::new()));
        let empty_then_failed = ClientGeocoder {
            address: |_| Ok(Vec::new()),
            places: |_| Err("timed out".to_owned()),
        };
        assert_eq!(
            labels(empty_then_failed.search(&query("Atlantis"))),
            Ok(Vec::new())
        );
        let failed = ClientGeocoder {
            address: |_| Err("photon down".to_owned()),
            places: |_| Err("open-meteo down".to_owned()),
        };
        assert_eq!(
            labels(failed.search(&query("Lisbon"))),
            Err("photon down; open-meteo down".to_owned())
        );
        assert!(failed.reverse(GeoPoint { lat: 0.0, lon: 0.0 }).is_err());
    }
}
