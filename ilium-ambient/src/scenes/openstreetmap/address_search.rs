//! Explicit, blocking OSM-picker search. Call only from the picker's admitted worker.
//! Search results are coordinates, never OSM geometry or shared observer state.
use super::address_settings::{AddressProvider, AddressSearchSettings};
use crate::{
    location::GeoLocation,
    source::{default_cache_dir, provider_host_lease, USER_AGENT},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const CACHE_KEY_BYTES: usize = 32;
const CACHE_SLOTS: u8 = 32;
const MAX_CACHE_BYTES: usize = MAX_RESPONSE_BYTES + CACHE_KEY_BYTES;
const MAX_RESULTS: usize = 5;
const MAX_QUERY_BYTES: usize = 512;
const CACHE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(12);
const SPACING: Duration = Duration::from_secs(1);
const ERROR_COOLDOWN: Duration = Duration::from_secs(60);
const CITY_ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";

#[derive(Default)]
struct GateState {
    busy: bool,
    next: Option<Instant>,
    disabled: bool,
}
#[derive(Default)]
struct Gate(Mutex<GateState>);
static GATE: OnceLock<Gate> = OnceLock::new();
struct Permit<'a>(&'a Gate);
impl Gate {
    fn reserve(&self, now: Instant) -> Result<Permit<'_>, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Address-search admission is unavailable")?;
        if state.disabled {
            return Err("Address-search cooldown is unavailable in this process".into());
        }
        if state.busy {
            return Err("An address request is already running".into());
        }
        if let Some(next) = state.next.filter(|next| *next > now) {
            return Err(format!(
                "Address service cooldown: {} seconds remaining",
                next.duration_since(now).as_secs() + 1
            ));
        }
        state.next = now.checked_add(SPACING);
        if state.next.is_none() {
            state.disabled = true;
            return Err("Address-search cooldown overflow".into());
        }
        state.busy = true;
        Ok(Permit(self))
    }

    fn defer(&self, now: Instant, seconds: u64) {
        if let Ok(mut state) = self.0.lock() {
            match now.checked_add(Duration::from_secs(seconds.max(ERROR_COOLDOWN.as_secs()))) {
                Some(until) => state.next = Some(state.next.map_or(until, |old| old.max(until))),
                None => state.disabled = true,
            }
        }
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let gate = self.0;
        if let Ok(mut state) = gate.0.lock() {
            state.busy = false;
        }
    }
}

fn cooldown_seconds(status: u16, retry_after: Option<&str>) -> Option<u64> {
    matches!(status, 429 | 503).then(|| {
        retry_after
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(ERROR_COOLDOWN.as_secs())
    })
}

/// Photon/Nominatim receive the full query. CityOnly searches the first city
/// token and ranks candidates as before. Every provider here uses bounded
/// transport, cache reads, and raw-point validation.
pub fn search(settings: &AddressSearchSettings, query: &str) -> Result<Vec<GeoLocation>, String> {
    search_with_cache(
        settings,
        query,
        &default_cache_dir().join("openstreetmap-address"),
    )
}

pub fn search_with_cache(
    settings: &AddressSearchSettings,
    query: &str,
    cache_dir: &Path,
) -> Result<Vec<GeoLocation>, String> {
    search_with_cache_using(
        settings,
        query,
        cache_dir,
        &default_cache_dir().join("provider-admission"),
        fetch,
    )
}

fn search_with_cache_using(
    settings: &AddressSearchSettings,
    query: &str,
    cache_dir: &Path,
    lease_dir: &Path,
    fetch_response: impl FnOnce(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<GeoLocation>, String> {
    let query = query.trim();
    if query.is_empty() || query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control) {
        return Err("Enter one address or place, at most 512 bytes on one line".into());
    }
    if let Some((latitude, longitude)) = raw_coordinate_pair(query) {
        if !valid_point(latitude, longitude) {
            return Err("Latitude must be -85 to 85; longitude -180 to 180".into());
        }
        let mut location = GeoLocation::new("", latitude, longitude);
        location.label = location.coordinate_text();
        return Ok(vec![location]);
    }
    let (endpoint, lookup) = match settings.provider {
        AddressProvider::Disabled => return Err("Address search is disabled".into()),
        AddressProvider::CityOnly => {
            let name = query.split(',').next().unwrap_or(query).trim();
            if name.is_empty() {
                return Err("Enter a city name".into());
            }
            (CITY_ENDPOINT, name)
        }
        AddressProvider::Photon | AddressProvider::Nominatim => (
            settings
                .configured_endpoint()?
                .ok_or("Address service is not configured")?,
            query,
        ),
    };
    let url = request_url(settings.provider, endpoint, lookup)?;
    let path = cache_path(cache_dir, settings.provider, &url);
    if let Some(mut results) = read_cache(&path, settings.provider, &url) {
        if settings.provider == AddressProvider::CityOnly {
            rank_city_results(&mut results, query);
        }
        return Ok(results);
    }
    validate_address_uri(&url)?;
    // Hold a process-safe host lease through fetch and cache publication. A
    // second client rechecks the shared cache after it acquires this lease.
    let _lease = provider_host_lease(&url, lease_dir, SPACING, Instant::now() + TIMEOUT, None)
        .map_err(|error| format!("Address service admission: {error}"))?;
    if let Some(mut results) = read_cache(&path, settings.provider, &url) {
        if settings.provider == AddressProvider::CityOnly {
            rank_city_results(&mut results, query);
        }
        return Ok(results);
    }
    let bytes = fetch_response(&url)?;
    let mut results = parse_response(settings.provider, &bytes)?;
    if settings.provider == AddressProvider::CityOnly {
        rank_city_results(&mut results, query);
    }
    // Do not retain empty, malformed or transport-error answers as a false
    // negative. A successful answer is still usable if cache publication fails.
    if !results.is_empty() {
        let _ = write_cache(&path, settings.provider, &url, &bytes);
    }
    Ok(results)
}

fn request_url(provider: AddressProvider, endpoint: &str, query: &str) -> Result<String, String> {
    let escaped = percent_encode(query);
    let suffix = match provider {
        AddressProvider::Photon => format!("?q={escaped}&limit={MAX_RESULTS}&lang=en"),
        AddressProvider::Nominatim => format!(
            "?q={escaped}&format=jsonv2&addressdetails=1&limit={MAX_RESULTS}&accept-language=en"
        ),
        AddressProvider::CityOnly => {
            format!("?name={escaped}&count={MAX_RESULTS}&language=en&format=json")
        }
        AddressProvider::Disabled => {
            return Err("This address provider has no HTTPS request".into())
        }
    };
    let url = format!("{endpoint}{suffix}");
    if url.len() > 4096 {
        return Err("Address-search request is too long".into());
    }
    Ok(url)
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
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

/// Parse numeric text without constructing a normalizing `GeoLocation`.
/// Invalid numeric ranges must fail locally, not become provider searches.
fn raw_coordinate_pair(text: &str) -> Option<(f64, f64)> {
    fn part(text: &str, positive: char, negative: char) -> Option<f64> {
        let upper = text.to_ascii_uppercase();
        let (digits, sign) = if let Some(digits) = upper.strip_suffix(positive) {
            (digits, 1.0)
        } else if let Some(digits) = upper.strip_suffix(negative) {
            (digits, -1.0)
        } else {
            (upper.as_str(), 1.0)
        };
        digits.parse::<f64>().ok().map(|number| number * sign)
    }
    let cleaned = text.replace(',', " ");
    let mut parts = cleaned.split_whitespace();
    let latitude = part(parts.next()?, 'N', 'S')?;
    let longitude = part(parts.next()?, 'E', 'W')?;
    parts.next().is_none().then_some((latitude, longitude))
}

fn validate_address_uri(url: &str) -> Result<(), String> {
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|error| format!("Invalid address URI: {error}"))?;
    if uri.scheme_str() != Some("https")
        || uri.host().is_none_or(str::is_empty)
        || uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
    {
        return Err("Use an HTTPS address endpoint without credentials".into());
    }
    Ok(())
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    validate_address_uri(url)?;
    let gate = GATE.get_or_init(Default::default);
    let _permit = gate.reserve(Instant::now())?;
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .timeout_connect(Some(Duration::from_secs(5)))
        .max_response_header_size(16 * 1024)
        .user_agent(USER_AGENT)
        .build();
    let agent = ilium_http::agent(config);
    let mut response = agent
        .get(url)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|error| format!("Address transport: {error}"))?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok());
    if let Some(seconds) = cooldown_seconds(status, retry_after) {
        gate.defer(Instant::now(), seconds);
    }
    if !(200..300).contains(&status) {
        return Err(format!("Address service HTTP {status}; no automatic retry"));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !content_type.eq_ignore_ascii_case("application/json")
        && !content_type.eq_ignore_ascii_case("application/geo+json")
    {
        return Err("Address response must be JSON".into());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Address response read: {error}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Address response exceeds 256 KiB".into());
    }
    Ok(bytes)
}

fn cache_path(cache_dir: &Path, provider: AddressProvider, url: &str) -> PathBuf {
    let key = cache_key(provider, url);
    // Fixed slot names cap the live cache at 32 files. A slot collision only
    // evicts a prior answer; the full digest prevents serving the wrong one.
    cache_dir.join(format!("address-v1-{:02}.json", key[0] % CACHE_SLOTS))
}

fn cache_key(provider: AddressProvider, url: &str) -> [u8; CACHE_KEY_BYTES] {
    let digest = Sha256::digest(format!("{}:{url}", provider.index()).as_bytes());
    let mut key = [0; CACHE_KEY_BYTES];
    key.copy_from_slice(&digest);
    key
}

fn read_bounded_file(path: &Path) -> Option<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_CACHE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_CACHE_BYTES).then_some(bytes)
}

fn read_cache(path: &Path, provider: AddressProvider, url: &str) -> Option<Vec<GeoLocation>> {
    let age = fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    if age > CACHE_AGE {
        return None;
    }
    let bytes = read_bounded_file(path)?;
    let key = cache_key(provider, url);
    if bytes.get(..CACHE_KEY_BYTES)? != &key[..] {
        return None;
    }
    parse_response(provider, &bytes[CACHE_KEY_BYTES..])
        .ok()
        .filter(|results| !results.is_empty())
}

fn write_cache(
    path: &Path,
    provider: AddressProvider,
    url: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    write_cache_with_sequence(path, provider, url, bytes, sequence)
}

fn write_cache_with_sequence(
    path: &Path,
    provider: AddressProvider,
    url: &str,
    bytes: &[u8],
    sequence: u64,
) -> std::io::Result<()> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(std::io::Error::other("Address cache body exceeds 256 KiB"));
    }
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("Missing cache parent"));
    };
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), sequence));
    // A failed create_new did not create this path. Never remove another
    // writer's pre-existing temporary file on that error.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let write_result = (|| {
        file.write_all(&cache_key(provider, url))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn valid_point(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-85.0..=85.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

fn parse_response(provider: AddressProvider, bytes: &[u8]) -> Result<Vec<GeoLocation>, String> {
    match provider {
        AddressProvider::Photon => parse_photon_response(bytes),
        AddressProvider::Nominatim => parse_nominatim_response(bytes),
        AddressProvider::CityOnly => parse_city_response(bytes),
        AddressProvider::Disabled => Err("This provider has no JSON response parser".into()),
    }
}

#[derive(Deserialize)]
struct PhotonResponse {
    #[serde(default)]
    features: Vec<PhotonFeature>,
}
#[derive(Deserialize)]
struct PhotonFeature {
    properties: PhotonProperties,
    geometry: PhotonGeometry,
}
#[derive(Deserialize)]
struct PhotonGeometry {
    #[serde(rename = "type")]
    kind: String,
    coordinates: Vec<f64>,
}
#[derive(Deserialize)]
struct PhotonProperties {
    name: Option<String>,
    housenumber: Option<String>,
    street: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
}

pub fn parse_photon_response(bytes: &[u8]) -> Result<Vec<GeoLocation>, String> {
    let response: PhotonResponse = serde_json::from_slice(bytes)
        .map_err(|error| format!("Unreadable Photon answer: {error}"))?;
    let mut results = Vec::new();
    for feature in response.features {
        if feature.geometry.kind != "Point" || feature.geometry.coordinates.len() != 2 {
            continue;
        }
        let longitude = feature.geometry.coordinates[0];
        let latitude = feature.geometry.coordinates[1];
        if !valid_point(latitude, longitude) {
            continue;
        }
        let p = feature.properties;
        let mut parts = Vec::<String>::new();
        if let Some(name) = p.name {
            parts.push(name);
        }
        if let Some(street) = p.street {
            let address = match p.housenumber {
                Some(number) => format!("{number} {street}"),
                None => street,
            };
            parts.push(address);
        }
        if let Some(city) = p.city {
            parts.push(match p.postcode {
                Some(code) => format!("{code} {city}"),
                None => city,
            });
        }
        for part in [p.state, p.country].into_iter().flatten() {
            parts.push(part);
        }
        let mut clean = Vec::<String>::new();
        for part in parts {
            let part = part.trim();
            if part.is_empty() || part.chars().any(char::is_control) {
                continue;
            }
            if !clean.iter().any(|known| known.eq_ignore_ascii_case(part)) {
                clean.push(part.into());
            }
        }
        let label = clean.join(", ");
        if label.is_empty() || label.len() > 512 {
            continue;
        }
        results.push(GeoLocation::new(label, latitude, longitude));
        if results.len() == MAX_RESULTS {
            break;
        }
    }
    Ok(results)
}

#[derive(Deserialize)]
struct NominatimPlace {
    display_name: String,
    lat: String,
    lon: String,
}

pub fn parse_nominatim_response(bytes: &[u8]) -> Result<Vec<GeoLocation>, String> {
    let response: Vec<NominatimPlace> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Unreadable Nominatim answer: {error}"))?;
    let mut results = Vec::new();
    for place in response {
        let (Ok(latitude), Ok(longitude)) = (place.lat.parse(), place.lon.parse()) else {
            continue;
        };
        if !valid_point(latitude, longitude) {
            continue;
        }
        let label = place.display_name.trim();
        if label.is_empty() || label.len() > 512 || label.chars().any(char::is_control) {
            continue;
        }
        results.push(GeoLocation::new(label, latitude, longitude));
        if results.len() == MAX_RESULTS {
            break;
        }
    }
    Ok(results)
}

#[derive(Deserialize)]
struct CityResponse {
    #[serde(default)]
    results: Vec<CityPlace>,
}
#[derive(Deserialize)]
struct CityPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

fn parse_city_response(bytes: &[u8]) -> Result<Vec<GeoLocation>, String> {
    let response: CityResponse = serde_json::from_slice(bytes)
        .map_err(|error| format!("Unreadable city-search answer: {error}"))?;
    let mut results = Vec::new();
    for place in response.results {
        if !valid_point(place.latitude, place.longitude) {
            continue;
        }
        let mut parts = Vec::<String>::new();
        for part in [Some(place.name), place.admin1, place.country]
            .into_iter()
            .flatten()
        {
            let part = part.trim();
            if part.is_empty() || part.chars().any(char::is_control) {
                continue;
            }
            if !parts.iter().any(|known| known.eq_ignore_ascii_case(part)) {
                parts.push(part.into());
            }
        }
        let label = parts.join(", ");
        if label.is_empty() || label.len() > 512 {
            continue;
        }
        results.push(GeoLocation::new(label, place.latitude, place.longitude));
        if results.len() == MAX_RESULTS {
            break;
        }
    }
    Ok(results)
}

fn rank_city_results(results: &mut [GeoLocation], query: &str) {
    let wanted: Vec<String> = query
        .split(',')
        .skip(1)
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if wanted.is_empty() {
        return;
    }
    results.sort_by_key(|location| {
        let label = location.label.to_lowercase();
        std::cmp::Reverse(
            wanted
                .iter()
                .filter(|part| label.contains(part.as_str()))
                .count(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const ADDRESS_SAMPLE: &str = include_str!("../../../assets/photon_paris_address_sample.json");
    const CITY_SAMPLE: &str = include_str!("../../../assets/open_meteo_paris_sample.json");

    #[test]
    fn local_gate_keeps_a_request_busy_and_spaces_later_starts() {
        let gate = Gate::default();
        let now = Instant::now();
        let permit = gate.reserve(now).unwrap();
        assert!(
            gate.reserve(now + SPACING).is_err(),
            "live permit owns admission"
        );
        drop(permit);
        assert!(gate
            .reserve(now + SPACING - Duration::from_millis(1))
            .is_err());
        assert!(gate.reserve(now + SPACING).is_ok());
    }

    #[test]
    fn status_cooldown_denies_429_and_503_retries_and_overflow_fails_closed() {
        assert_eq!(cooldown_seconds(200, Some("999")), None);
        assert_eq!(cooldown_seconds(429, Some("120")), Some(120));
        assert_eq!(cooldown_seconds(503, Some("invalid")), Some(60));
        let gate = Gate::default();
        let now = Instant::now();
        gate.defer(now, cooldown_seconds(429, Some("120")).unwrap());
        assert!(gate.reserve(now + Duration::from_secs(119)).is_err());
        let permit = gate.reserve(now + Duration::from_secs(120)).unwrap();
        drop(permit);
        gate.defer(
            now + Duration::from_secs(120),
            cooldown_seconds(503, Some("1")).unwrap(),
        );
        assert!(gate.reserve(now + Duration::from_secs(179)).is_err());
        let permit = gate.reserve(now + Duration::from_secs(180)).unwrap();
        drop(permit);
        gate.defer(now + Duration::from_secs(180), u64::MAX);
        assert!(gate.reserve(now + Duration::from_secs(1000)).is_err());
    }

    #[test]
    fn recorded_full_street_address_is_not_reduced_to_a_city() {
        let results = parse_photon_response(ADDRESS_SAMPLE.as_bytes()).unwrap();
        assert_eq!(results.len(), 5);
        assert!(results[0].label.contains("5 Avenue Anatole France"));
        assert!((results[0].latitude - 48.8582599).abs() < 1e-8);
        assert!((results[0].longitude - 2.2945006).abs() < 1e-8);
        let url = request_url(
            AddressProvider::Photon,
            "https://photon.komoot.io/api/",
            "5 Avenue Anatole France, Paris",
        )
        .unwrap();
        assert!(url.contains("q=5%20Avenue%20Anatole%20France%2C%20Paris"));
        assert!(url.ends_with("&limit=5&lang=en"));
    }

    #[test]
    fn invalid_raw_points_are_skipped_before_geolocation_normalizes() {
        let photon = br#"{"features":[{"properties":{"name":"wrong"},"geometry":{"type":"Point","coordinates":[181,91]}},{"properties":{"name":"valid"},"geometry":{"type":"Point","coordinates":[2,48]}}]}"#;
        let results = parse_photon_response(photon).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "valid");
        let nominatim = br#"[{"display_name":"wrong","lat":"91","lon":"181"},{"display_name":"correct","lat":"48","lon":"2"}]"#;
        assert_eq!(
            parse_nominatim_response(nominatim).unwrap()[0].label,
            "correct"
        );
    }

    #[test]
    fn cache_hit_is_bounded_and_never_contacts_a_provider() {
        let cache = tempfile::tempdir().unwrap();
        let settings = AddressSearchSettings::default();
        let query = "5 Avenue Anatole France, Paris";
        let url = request_url(settings.provider, &settings.photon_endpoint, query).unwrap();
        let path = cache_path(cache.path(), settings.provider, &url);
        write_cache(&path, settings.provider, &url, ADDRESS_SAMPLE.as_bytes()).unwrap();
        assert_eq!(
            search_with_cache(&settings, query, cache.path())
                .unwrap()
                .len(),
            5
        );
        fs::write(&path, vec![b' '; MAX_CACHE_BYTES + 1]).unwrap();
        assert!(read_cache(&path, settings.provider, &url).is_none());
    }

    #[test]
    fn process_safe_admission_rechecks_address_cache_before_second_provider_call() {
        let cache = tempfile::tempdir().unwrap();
        let mut settings = AddressSearchSettings::default();
        settings.photon_endpoint = "https://address-cache-race.invalid/api/".into();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first_cache = cache.path().to_path_buf();
        let first_lease_dir = cache.path().join("provider-admission");
        let first_settings = settings.clone();
        let first = std::thread::spawn(move || {
            search_with_cache_using(
                &first_settings,
                "Paris",
                &first_cache,
                &first_lease_dir,
                move |_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(ADDRESS_SAMPLE.as_bytes().to_vec())
                },
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second_cache = cache.path().to_path_buf();
        let second_lease_dir = cache.path().join("provider-admission");
        let second = std::thread::spawn(move || {
            search_with_cache_using(&settings, "Paris", &second_cache, &second_lease_dir, |_| {
                panic!("address cache hit must suppress duplicate provider request")
            })
        });
        release_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap().len(), 5);
        assert_eq!(second.join().unwrap().unwrap().len(), 5);
    }

    #[test]
    fn expired_positive_cache_is_not_served() {
        let cache = tempfile::tempdir().unwrap();
        let provider = AddressProvider::Photon;
        let url = "https://example.org/api/?q=Paris";
        let path = cache_path(cache.path(), provider, url);
        write_cache(&path, provider, url, ADDRESS_SAMPLE.as_bytes()).unwrap();
        assert_eq!(read_cache(&path, provider, url).unwrap().len(), 5);
        let stale = std::time::SystemTime::now()
            .checked_sub(CACHE_AGE + Duration::from_secs(2))
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stale))
            .unwrap();
        assert!(read_cache(&path, provider, url).is_none());
    }

    #[test]
    fn existing_temporary_cache_file_is_never_removed_on_create_new_failure() {
        let cache = tempfile::tempdir().unwrap();
        let provider = AddressProvider::Photon;
        let url = "https://example.org/api/?q=Paris";
        let path = cache_path(cache.path(), provider, url);
        let sequence = 77;
        let temporary =
            path.with_extension(format!("json.{}.{}.tmp", std::process::id(), sequence));
        fs::write(&temporary, b"another writer's sentinel").unwrap();
        assert!(write_cache_with_sequence(
            &path,
            provider,
            url,
            ADDRESS_SAMPLE.as_bytes(),
            sequence
        )
        .is_err());
        assert_eq!(fs::read(&temporary).unwrap(), b"another writer's sentinel");
        assert!(!path.exists());
    }

    #[test]
    fn fixed_cache_slots_verify_full_key_before_serving_a_colliding_query() {
        let cache = tempfile::tempdir().unwrap();
        let provider = AddressProvider::Photon;
        let first = "https://example.org/api/?q=Paris";
        let first_path = cache_path(cache.path(), provider, first);
        let collision = (0..1024)
            .map(|number| format!("https://example.org/api/?q=City{number}"))
            .find(|url| cache_path(cache.path(), provider, url) == first_path)
            .unwrap();
        write_cache(&first_path, provider, first, ADDRESS_SAMPLE.as_bytes()).unwrap();
        assert!(read_cache(&first_path, provider, &collision).is_none());
        write_cache(&first_path, provider, &collision, ADDRESS_SAMPLE.as_bytes()).unwrap();
        assert!(read_cache(&first_path, provider, first).is_none());
        assert_eq!(
            read_cache(&first_path, provider, &collision).unwrap().len(),
            5
        );
        let slots: std::collections::BTreeSet<_> = (0..1024)
            .map(|number| {
                cache_path(
                    cache.path(),
                    provider,
                    &format!("https://example.org/api/?q={number}"),
                )
            })
            .collect();
        assert_eq!(slots.len(), usize::from(CACHE_SLOTS));
    }

    #[test]
    fn city_only_keeps_city_ranking_but_rejects_raw_out_of_range_points() {
        let cache = tempfile::tempdir().unwrap();
        let settings = AddressSearchSettings {
            provider: AddressProvider::CityOnly,
            ..Default::default()
        };
        let url = request_url(settings.provider, CITY_ENDPOINT, "Paris").unwrap();
        let path = cache_path(cache.path(), settings.provider, &url);
        write_cache(&path, settings.provider, &url, CITY_SAMPLE.as_bytes()).unwrap();
        let results = search_with_cache(&settings, "Paris, Texas", cache.path()).unwrap();
        assert_eq!(results[0].label, "Paris, Texas, United States");
        let invalid = br#"{"results":[{"name":"wrong","latitude":48,"longitude":181},{"name":"valid","latitude":48,"longitude":2}]}"#;
        let parsed = parse_city_response(invalid).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].label, "valid");
    }

    #[test]
    fn disabled_provider_still_allows_only_valid_direct_osm_coordinates() {
        let settings = AddressSearchSettings {
            provider: AddressProvider::Disabled,
            ..Default::default()
        };
        assert_eq!(
            search_with_cache(&settings, "48.858, 2.294", Path::new("/absent"))
                .unwrap()
                .len(),
            1
        );
        assert!(search_with_cache(&settings, "89, 2", Path::new("/absent")).is_err());
        assert!(search_with_cache(&settings, "48, 181", Path::new("/absent")).is_err());
        assert_eq!(
            search_with_cache(&settings, "48.858N 2.294E", Path::new("/absent"))
                .unwrap()
                .len(),
            1
        );
        assert!(search_with_cache(
            &settings,
            "5 Avenue Anatole France, Paris",
            Path::new("/absent")
        )
        .is_err());
    }
}
