//! Address search for the shared observer location.
//!
//! Backend: the Open-Meteo Geocoding API (no API key).
//! Documentation: <https://open-meteo.com/en/docs/geocoding-api>.
//! Terms: free for non-commercial use with fair-use rate limits; the place
//! data are GeoNames (CC BY 4.0), see <https://open-meteo.com/en/terms>. All
//! requests go through `source::fetch_cached`, which sends the ilium
//! User-Agent, throttles per host and caches answers on disk for 30 days, so
//! a repeated query never touches the network again.

use crate::location::GeoLocation;
use crate::source::{default_cache_dir, fetch_cached};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

const ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";
const RESULT_COUNT: usize = 5;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
const TIMEOUT: Duration = Duration::from_secs(10);

/// Blocking address lookup, for worker threads only. Returns up to a handful
/// of candidates, best first. Coordinate strings ("48.857, 2.352") are parsed
/// locally without any network access.
pub fn search(query: &str) -> Result<Vec<GeoLocation>, String> {
    search_with_cache(query, &default_cache_dir().join("geocode"))
}

/// `search` with an explicit cache directory (used by tests and probes).
pub fn search_with_cache(query: &str, cache_dir: &Path) -> Result<Vec<GeoLocation>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Type a place name or coordinates".to_owned());
    }
    if let Some(location) = GeoLocation::parse_coordinates(query) {
        return Ok(vec![location]);
    }
    // The service matches a single place name, not "Paris, France": look up the
    // first segment only (one request) and rank by how many of the other
    // segments the label shares.
    let name = query.split(',').next().unwrap_or(query).trim();
    if name.is_empty() {
        return Err(format!("No place name in \"{query}\""));
    }
    let mut results = lookup(name, cache_dir)?;
    if results.is_empty() {
        return Err(format!("No place found for \"{query}\""));
    }
    rank_by_query(&mut results, query);
    results.truncate(RESULT_COUNT);
    Ok(results)
}

fn lookup(name: &str, cache_dir: &Path) -> Result<Vec<GeoLocation>, String> {
    let url = format!(
        "{ENDPOINT}?name={}&count={RESULT_COUNT}&language=en&format=json",
        percent_encode(name)
    );
    let bytes = fetch_cached(
        cache_dir,
        &url,
        "json",
        CACHE_MAX_AGE,
        MAX_RESPONSE_BYTES,
        TIMEOUT,
    )
    .map_err(|error| format!("Address search failed: {error}"))?;
    parse_response(&bytes)
}

/// Stable ordering: more matching trailing segments first, service order kept.
fn rank_by_query(results: &mut [GeoLocation], query: &str) {
    let wanted: Vec<String> = query
        .split(',')
        .skip(1)
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if wanted.is_empty() {
        return;
    }
    let score = |location: &GeoLocation| {
        let label = location.label.to_lowercase();
        wanted
            .iter()
            .filter(|part| label.contains(part.as_str()))
            .count()
    };
    results.sort_by_key(|location| std::cmp::Reverse(score(location)));
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<Place>,
}

#[derive(Debug, Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

/// Pure parser for an Open-Meteo `/v1/search` body. An answer without a
/// `results` key (no match) yields an empty list.
pub fn parse_response(body: &[u8]) -> Result<Vec<GeoLocation>, String> {
    let response: Response = serde_json::from_slice(body)
        .map_err(|error| format!("Unreadable geocoding answer: {error}"))?;
    Ok(response
        .results
        .into_iter()
        .filter(|place| place.latitude.is_finite() && place.longitude.is_finite())
        .map(|place| {
            let mut parts: Vec<String> = vec![place.name.clone()];
            for extra in [place.admin1, place.country].into_iter().flatten() {
                if !extra.is_empty() && !parts.contains(&extra) {
                    parts.push(extra);
                }
            }
            GeoLocation::new(parts.join(", "), place.latitude, place.longitude)
        })
        .collect())
}

/// Percent-encode everything except RFC 3986 unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::cache_file_name;

    /// A real answer recorded from the live API for `name=Paris&count=3`.
    const PARIS_SAMPLE: &str = include_str!("../assets/open_meteo_paris_sample.json");

    #[test]
    fn parses_recorded_sample_into_labelled_locations() {
        let places = parse_response(PARIS_SAMPLE.as_bytes()).unwrap();
        assert_eq!(places.len(), 3);
        assert_eq!(places[0].label, "Paris, Île-de-France Region, France");
        assert!((places[0].latitude - 48.85341).abs() < 1e-6);
        assert!((places[0].longitude - 2.3488).abs() < 1e-6);
        assert_eq!(places[1].label, "Paris, Texas, United States");
        assert!(places[1].longitude < -95.0);
    }

    #[test]
    fn empty_answer_is_an_empty_list_and_garbage_is_an_error() {
        assert!(parse_response(br#"{"generationtime_ms":0.5}"#)
            .unwrap()
            .is_empty());
        assert!(parse_response(b"<html>").is_err());
    }

    #[test]
    fn duplicated_label_parts_are_collapsed() {
        let body = br#"{"results":[{"name":"Monaco","latitude":43.7,"longitude":7.4,"country":"Monaco"}]}"#;
        assert_eq!(parse_response(body).unwrap()[0].label, "Monaco");
    }

    #[test]
    fn coordinates_never_touch_the_network_or_the_cache() {
        let places =
            search_with_cache("48.857, 2.352", Path::new("/nonexistent/never/used")).unwrap();
        assert_eq!(places.len(), 1);
        assert!((places[0].latitude - 48.857).abs() < 1e-9);
    }

    #[test]
    fn empty_query_is_rejected() {
        assert!(search_with_cache("   ", Path::new("/nonexistent")).is_err());
    }

    #[test]
    fn repeated_queries_are_served_from_the_disk_cache() {
        let directory = tempfile::tempdir().unwrap();
        let url = format!("{ENDPOINT}?name=Paris&count={RESULT_COUNT}&language=en&format=json");
        std::fs::write(
            directory.path().join(cache_file_name(&url, "json")),
            PARIS_SAMPLE,
        )
        .unwrap();
        let places = search_with_cache("Paris", directory.path()).unwrap();
        assert_eq!(places[0].label, "Paris, Île-de-France Region, France");
    }

    #[test]
    fn trailing_segments_rank_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let first = "Paris";
        let url_first =
            format!("{ENDPOINT}?name={first}&count={RESULT_COUNT}&language=en&format=json");
        std::fs::write(
            directory.path().join(cache_file_name(&url_first, "json")),
            PARIS_SAMPLE,
        )
        .unwrap();
        let places = search_with_cache("Paris, Texas", directory.path()).unwrap();
        assert_eq!(places[0].label, "Paris, Texas, United States");
    }

    #[test]
    fn percent_encoding_covers_spaces_and_unicode() {
        assert_eq!(percent_encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(percent_encode("a-b_c.d~"), "a-b_c.d~");
    }

    /// Live check against the real service (two requests):
    /// `cargo test -p ilium-ambient live_search -- --ignored --nocapture`
    #[test]
    #[ignore = "uses the network"]
    fn live_search() {
        let directory = tempfile::tempdir().unwrap();
        let paris = search_with_cache("Paris, Texas", directory.path()).unwrap();
        println!("{paris:?}");
        assert_eq!(paris[0].label, "Paris, Texas, United States");
        // The second call is served from the disk cache.
        let again = search_with_cache("Paris, Texas", directory.path()).unwrap();
        assert_eq!(again, paris);
        assert!(search_with_cache("Zzzzqqxx", directory.path()).is_err());
    }
}
