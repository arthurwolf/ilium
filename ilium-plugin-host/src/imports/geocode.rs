//! Host implementation of `ilium:host/geocode`.
//!
//! The import validates every call (API-005, API-060). With a [`GeocodeBackend`] (the client
//! lends its place and address search to scenes) a valid request runs on a worker thread and its
//! places wait for `take-places`, which answers `none` until then and after a provider failure.
//! Without one no provider is linked: a valid request completes at once with an empty place
//! list. Answers waiting for `take-places` belong to the import value of one plugin instance.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Shortest and longest search text in characters.
const TEXT_MIN_CHARS: usize = 2;
const TEXT_MAX_CHARS: usize = 200;
/// Largest number of places one search may ask for.
const LIMIT_MAX: u8 = 8;
/// One provider request per second per plugin instance (api-geocode, "Rate").
const PROVIDER_SPACING: Duration = Duration::from_secs(1);

/// The provider behind `geocode`: blocking calls, run on the import's worker threads only.
pub trait GeocodeBackend: Send + Sync {
    /// Up to `query.limit` places for `query.text`, best first; an empty list when the provider
    /// knows none, `Err` (a short reason) when it failed.
    fn search(&self, query: &GeocodeQuery) -> Result<Vec<GeoPlace>, String>;
    /// The places at `point`, nearest first, under the same rules.
    fn reverse(&self, point: GeoPoint) -> Result<Vec<GeoPlace>, String>;
}

#[derive(Default)]
struct GeocodeState {
    next_request: u64,
    answers: HashMap<u64, Vec<GeoPlace>>,
    /// When the last provider request started, for the one-per-second rate.
    last_provider_call: Option<Instant>,
    /// The worker of each request still running. A worker answers through a weak handle, so a
    /// dropped import drops its late answers; the handles of finished workers are joined on the
    /// next call. A provider call is bounded by its own timeout, so a worker left running when
    /// the import is dropped ends by itself.
    workers: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for GeocodeState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeocodeState")
            .field("next_request", &self.next_request)
            .field("answers", &self.answers.len())
            .field("workers", &self.workers.len())
            .finish()
    }
}

fn invalid_argument(message: &str) -> PluginError {
    PluginError {
        code: ErrorCode::InvalidArgument,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// True for a finite latitude/longitude pair inside the valid range.
fn is_valid_point(point: &GeoPoint) -> bool {
    point.lat.is_finite()
        && point.lon.is_finite()
        && (-90.0..=90.0).contains(&point.lat)
        && (-180.0..=180.0).contains(&point.lon)
}

/// Host side of the `geocode` interface of one plugin instance. Cloning shares its answers.
#[derive(Clone, Default)]
pub struct GeocodeImport {
    state: Arc<Mutex<GeocodeState>>,
    backend: Option<Arc<dyn GeocodeBackend>>,
}

impl std::fmt::Debug for GeocodeImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeocodeImport")
            .field("state", &self.state)
            .field("backend", &self.backend.is_some())
            .finish()
    }
}

/// What one provider call asks.
enum ProviderCall {
    Search(GeocodeQuery),
    Reverse(GeoPoint),
}

impl GeocodeImport {
    /// An import whose requests go to `backend`.
    pub fn with_backend(backend: Arc<dyn GeocodeBackend>) -> Self {
        Self {
            state: Arc::default(),
            backend: Some(backend),
        }
    }

    fn complete_with(&self, places: Vec<GeoPlace>) -> RequestId {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.next_request += 1;
        let request = state.next_request;
        state.answers.insert(request, places);
        RequestId(request)
    }

    /// Runs `call` on the backend, or completes it at once with no place when there is none.
    fn dispatch(&self, call: ProviderCall) -> Result<RequestId, PluginError> {
        let Some(backend) = &self.backend else {
            return Ok(self.complete_with(Vec::new()));
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if let Some(wait) = state
            .last_provider_call
            .and_then(|last| PROVIDER_SPACING.checked_sub(now.saturating_duration_since(last)))
            .filter(|wait| !wait.is_zero())
        {
            return Err(PluginError {
                code: ErrorCode::RateLimited,
                message: "one geocode request per second".to_owned(),
                retry_after_ms: Some(u32::try_from(wait.as_millis()).unwrap_or(u32::MAX).max(1)),
            });
        }
        let (finished, running): (Vec<_>, Vec<_>) = std::mem::take(&mut state.workers)
            .into_iter()
            .partition(JoinHandle::is_finished);
        state.workers = running;
        for worker in finished {
            // A worker that panicked has already lost its answer; nothing else to do.
            let _ = worker.join();
        }
        state.last_provider_call = Some(now);
        state.next_request += 1;
        let request = state.next_request;
        let answers: Weak<Mutex<GeocodeState>> = Arc::downgrade(&self.state);
        let backend = Arc::clone(backend);
        let worker = std::thread::Builder::new()
            .name("ilium-plugin-geocode".to_owned())
            .spawn(move || {
                let result = match call {
                    ProviderCall::Search(query) => {
                        let limit = usize::from(query.limit);
                        backend.search(&query).map(|mut places| {
                            places.truncate(limit);
                            places
                        })
                    }
                    ProviderCall::Reverse(point) => backend.reverse(point).map(|mut places| {
                        places.truncate(usize::from(LIMIT_MAX));
                        places
                    }),
                };
                // A failure leaves the request without places: `take-places` answers `none`.
                if let (Ok(places), Some(state)) = (result, answers.upgrade()) {
                    state
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .answers
                        .insert(request, places);
                }
            })
            .map_err(|error| PluginError {
                code: ErrorCode::Internal,
                message: format!("geocode worker did not start: {error}"),
                retry_after_ms: None,
            })?;
        state.workers.push(worker);
        Ok(RequestId(request))
    }
}

impl HostGeocode for GeocodeImport {
    fn search(&mut self, q: GeocodeQuery) -> Result<RequestId, PluginError> {
        let text_chars = q.text.chars().count();
        if !(TEXT_MIN_CHARS..=TEXT_MAX_CHARS).contains(&text_chars) {
            return Err(invalid_argument("text must be 2 to 200 characters long"));
        }
        if q.limit == 0 || q.limit > LIMIT_MAX {
            return Err(invalid_argument("limit must be 1 to 8"));
        }
        if let Some(near) = &q.near {
            if !is_valid_point(near) {
                return Err(invalid_argument(
                    "near must be latitude -90 to 90 and longitude -180 to 180",
                ));
            }
        }
        self.dispatch(ProviderCall::Search(q))
    }

    fn reverse(&mut self, point: GeoPoint) -> Result<RequestId, PluginError> {
        if !is_valid_point(&point) {
            return Err(invalid_argument(
                "point must be latitude -90 to 90 and longitude -180 to 180",
            ));
        }
        self.dispatch(ProviderCall::Reverse(point))
    }

    fn take_places(&mut self, request: RequestId) -> Option<Vec<GeoPlace>> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .answers
            .remove(&request.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{GeocodeImport, PROVIDER_SPACING};
    use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
    use ilium_plugin_api::generated::types::ids::RequestId;

    fn text_query(text: &str, limit: u8) -> GeocodeQuery {
        GeocodeQuery {
            text: text.to_string(),
            near: None,
            limit,
            language: None,
        }
    }

    fn near_query(near: GeoPoint) -> GeocodeQuery {
        GeocodeQuery {
            text: "Lisbon".to_string(),
            near: Some(near),
            limit: 5,
            language: None,
        }
    }

    fn point(lat: f64, lon: f64) -> GeoPoint {
        GeoPoint { lat, lon }
    }

    fn run_search(query: GeocodeQuery) -> Result<RequestId, PluginError> {
        let mut import = GeocodeImport::default();
        import.search(query)
    }

    fn run_reverse(point: GeoPoint) -> Result<RequestId, PluginError> {
        let mut import = GeocodeImport::default();
        import.reverse(point)
    }

    /// Asserts that the call failed at once (synchronously) with invalid-argument.
    fn assert_invalid_argument(result: Result<RequestId, PluginError>, case: &str) {
        match result {
            Ok(_) => panic!("{case}: expected invalid-argument, got a request id"),
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "{case}: {}",
                error.message
            ),
        }
    }

    /// Asserts that the input was not refused as invalid-argument. Other outcomes (consent, rate, success) are allowed.
    fn assert_not_invalid_argument(result: Result<RequestId, PluginError>, case: &str) {
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "{case}: a valid input was refused as invalid-argument: {}",
                error.message
            );
        }
    }

    /// REQ: API-001
    #[test]
    fn search_invalid_argument_returns_error_record_with_code_message_and_no_retry_delay() {
        let result = run_search(text_query("", 5));
        match result {
            Ok(_) => panic!("empty text must be refused"),
            Err(error) => {
                assert_eq!(error.code, ErrorCode::InvalidArgument);
                assert!(
                    !error.message.is_empty(),
                    "the error record must carry a message"
                );
                assert!(
                    error.message.chars().count() <= 256,
                    "the message is at most 256 characters, got {}",
                    error.message.chars().count()
                );
                assert_eq!(
                    error.retry_after_ms, None,
                    "only rate-limited carries retry-after-ms"
                );
            }
        }
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_empty_text_at_once() {
        assert_invalid_argument(run_search(text_query("", 5)), "empty text");
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_text_of_one_character() {
        assert_invalid_argument(run_search(text_query("L", 5)), "one character");
    }

    /// REQ: API-005
    #[test]
    fn search_accepts_text_of_two_characters_at_lower_bound() {
        assert_not_invalid_argument(run_search(text_query("Lx", 5)), "two characters");
    }

    /// REQ: API-005
    #[test]
    fn search_accepts_text_of_200_characters_at_upper_bound() {
        let text = "a".repeat(200);
        assert_not_invalid_argument(run_search(text_query(&text, 5)), "200 characters");
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_text_of_201_characters() {
        let text = "a".repeat(201);
        assert_invalid_argument(run_search(text_query(&text, 5)), "201 characters");
    }

    /// REQ: API-005
    #[test]
    fn search_counts_text_length_in_characters_not_bytes() {
        // Each "é" is two bytes in UTF-8: 200 of them are 400 bytes but still 200 characters.
        let accepted = "é".repeat(200);
        assert_not_invalid_argument(
            run_search(text_query(&accepted, 5)),
            "200 two-byte characters",
        );
        let refused = "é".repeat(201);
        assert_invalid_argument(
            run_search(text_query(&refused, 5)),
            "201 two-byte characters",
        );
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_limit_zero() {
        assert_invalid_argument(run_search(text_query("Lisbon", 0)), "limit 0");
    }

    /// REQ: API-005
    #[test]
    fn search_accepts_limit_one_and_eight_at_bounds() {
        assert_not_invalid_argument(run_search(text_query("Lisbon", 1)), "limit 1");
        assert_not_invalid_argument(run_search(text_query("Lisbon", 8)), "limit 8");
    }

    /// REQ: API-060
    #[test]
    fn search_rejects_limit_nine_instead_of_clamping_it() {
        assert_invalid_argument(run_search(text_query("Lisbon", 9)), "limit 9");
    }

    /// REQ: API-060
    #[test]
    fn search_rejects_limit_255_instead_of_clamping_it() {
        assert_invalid_argument(run_search(text_query("Lisbon", 255)), "limit 255");
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_near_latitude_above_90() {
        assert_invalid_argument(
            run_search(near_query(point(90.5, 0.0))),
            "near latitude 90.5",
        );
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_near_longitude_below_minus_180() {
        assert_invalid_argument(
            run_search(near_query(point(0.0, -180.5))),
            "near longitude -180.5",
        );
    }

    /// REQ: API-005
    #[test]
    fn search_accepts_near_at_range_corners() {
        assert_not_invalid_argument(run_search(near_query(point(90.0, 180.0))), "near 90, 180");
        assert_not_invalid_argument(
            run_search(near_query(point(-90.0, -180.0))),
            "near -90, -180",
        );
    }

    /// REQ: API-005
    #[test]
    fn reverse_rejects_latitude_above_90() {
        assert_invalid_argument(run_reverse(point(91.0, 0.0)), "reverse latitude 91");
    }

    /// REQ: API-005
    #[test]
    fn reverse_rejects_latitude_below_minus_90() {
        assert_invalid_argument(run_reverse(point(-90.5, 0.0)), "reverse latitude -90.5");
    }

    /// REQ: API-005
    #[test]
    fn reverse_rejects_longitude_above_180() {
        assert_invalid_argument(run_reverse(point(0.0, 180.5)), "reverse longitude 180.5");
    }

    /// REQ: API-005
    #[test]
    fn reverse_accepts_range_corners() {
        assert_not_invalid_argument(run_reverse(point(90.0, 180.0)), "reverse 90, 180");
        assert_not_invalid_argument(run_reverse(point(-90.0, -180.0)), "reverse -90, -180");
    }

    /// A provider that answers when the test releases it.
    struct HeldProvider {
        release: std::sync::Mutex<std::sync::mpsc::Receiver<Result<usize, String>>>,
    }

    impl super::GeocodeBackend for HeldProvider {
        fn search(&self, query: &GeocodeQuery) -> Result<Vec<GeoPlace>, String> {
            let count = self.release.lock().unwrap().recv().unwrap()?;
            Ok((0..count)
                .map(|index| GeoPlace {
                    point: point(38.7, -9.1),
                    label: format!("{} {index}", query.text),
                    country_code: Some("PT".to_owned()),
                    kind: Some("city".to_owned()),
                })
                .collect())
        }

        fn reverse(&self, at: GeoPoint) -> Result<Vec<GeoPlace>, String> {
            self.release.lock().unwrap().recv().unwrap()?;
            Ok(vec![GeoPlace {
                point: at,
                label: "here".to_owned(),
                country_code: None,
                kind: None,
            }])
        }
    }

    fn held() -> (
        GeocodeImport,
        std::sync::mpsc::Sender<Result<usize, String>>,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let import = GeocodeImport::with_backend(std::sync::Arc::new(HeldProvider {
            release: std::sync::Mutex::new(receiver),
        }));
        (import, sender)
    }

    /// Waits (bounded) until `take_places` answers `request`.
    fn wait_for_places(import: &mut GeocodeImport, request: RequestId) -> Option<Vec<GeoPlace>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(places) = import.take_places(request) {
                return Some(places);
            }
            if std::time::Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_backend_answers_later_and_the_places_are_taken_once_and_cut_to_the_limit() {
        let (mut import, release) = held();
        let request = import.search(text_query("Lisbon", 2)).expect("accepted");
        assert_eq!(import.take_places(request), None, "still running");
        release.send(Ok(5)).unwrap();
        let places = wait_for_places(&mut import, request).expect("places arrive");
        assert_eq!(
            places
                .iter()
                .map(|place| place.label.as_str())
                .collect::<Vec<_>>(),
            ["Lisbon 0", "Lisbon 1"]
        );
        assert_eq!(import.take_places(request), None, "a second take is none");
    }

    #[test]
    fn a_provider_failure_leaves_the_request_without_places_and_no_place_is_an_answer() {
        let (mut import, release) = held();
        let failed = import.search(text_query("Nowhere", 5)).expect("accepted");
        release.send(Err("network".to_owned())).unwrap();
        std::thread::sleep(PROVIDER_SPACING);
        let empty = import.reverse(point(0.0, 0.0)).expect("accepted");
        release.send(Err("down".to_owned())).unwrap();
        std::thread::sleep(PROVIDER_SPACING);
        let answered = import.search(text_query("Atlantis", 5)).expect("accepted");
        release.send(Ok(0)).unwrap();
        assert_eq!(wait_for_places(&mut import, answered), Some(Vec::new()));
        assert_eq!(import.take_places(failed), None);
        assert_eq!(import.take_places(empty), None);
    }

    #[test]
    fn a_second_provider_request_within_a_second_is_rate_limited() {
        let (mut import, release) = held();
        let first = import.search(text_query("Lisbon", 1)).expect("accepted");
        let refused = import.reverse(point(1.0, 1.0)).expect_err("rate limited");
        assert_eq!(refused.code, ErrorCode::RateLimited);
        assert!(refused.retry_after_ms.is_some_and(|wait| wait <= 1000));
        // An invalid call is still refused as invalid, before the rate.
        assert_invalid_argument(import.search(text_query("", 1)), "empty text");
        release.send(Ok(1)).unwrap();
        assert!(wait_for_places(&mut import, first).is_some());
    }

    #[test]
    fn a_dropped_import_drops_its_late_answers() {
        let (import, release) = held();
        let mut keep = import.clone();
        let request = keep.search(text_query("Porto", 1)).expect("accepted");
        let state = std::sync::Arc::downgrade(&import.state);
        drop(import);
        drop(keep);
        release.send(Ok(1)).unwrap();
        // The worker finds no import to answer and ends.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.upgrade().is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            state.upgrade().is_none(),
            "request {request:?} kept the state alive"
        );
    }
}
