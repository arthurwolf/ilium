//! Host implementation of `ilium:host/geocode`.
//!
//! The import validates every call (API-005, API-060) and completes the request with the places
//! the host knows. No geocoding provider is linked into this crate, so a valid request completes
//! with an empty place list; the location service replaces that completion when it is wired in.
//! The import is a unit struct, so answers waiting for `take-places` are kept per thread.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Shortest and longest search text in characters.
const TEXT_MIN_CHARS: usize = 2;
const TEXT_MAX_CHARS: usize = 200;
/// Largest number of places one search may ask for.
const LIMIT_MAX: u8 = 8;

#[derive(Debug, Default)]
struct GeocodeState {
    next_request: u64,
    answers: HashMap<u64, Vec<GeoPlace>>,
}

thread_local! {
    static STATE: RefCell<GeocodeState> = RefCell::new(GeocodeState::default());
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

fn complete_with(places: Vec<GeoPlace>) -> RequestId {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.next_request += 1;
        let request = state.next_request;
        state.answers.insert(request, places);
        RequestId(request)
    })
}

/// Host side of the `geocode` interface.
#[derive(Debug)]
pub struct GeocodeImport;

impl HostGeocode for GeocodeImport {
    fn search(&mut self, q: GeocodeQuery) -> Result<RequestId, PluginError> {
        let text_chars = q.text.chars().count();
        if !(TEXT_MIN_CHARS..=TEXT_MAX_CHARS).contains(&text_chars) {
            return Err(invalid_argument(
                "text must be 2 to 200 characters long",
            ));
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
        Ok(complete_with(Vec::new()))
    }

    fn reverse(&mut self, point: GeoPoint) -> Result<RequestId, PluginError> {
        if !is_valid_point(&point) {
            return Err(invalid_argument(
                "point must be latitude -90 to 90 and longitude -180 to 180",
            ));
        }
        Ok(complete_with(Vec::new()))
    }

    fn take_places(&mut self, request: RequestId) -> Option<Vec<GeoPlace>> {
        STATE.with(|cell| cell.borrow_mut().answers.remove(&request.0))
    }
}

#[cfg(test)]
mod tests {
    use super::GeocodeImport;
    use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::geo::GeoPoint;
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
        let mut import = GeocodeImport;
        import.search(query)
    }

    fn run_reverse(point: GeoPoint) -> Result<RequestId, PluginError> {
        let mut import = GeocodeImport;
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
}
