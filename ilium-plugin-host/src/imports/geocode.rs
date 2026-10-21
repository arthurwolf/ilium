//! Host implementation of `ilium:host/geocode` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostGeocode` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `geocode` interface.
#[derive(Debug)]
pub struct GeocodeImport;

impl HostGeocode for GeocodeImport {
    fn search(&mut self, _q: GeocodeQuery) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::geocode::search")
    }

    fn reverse(&mut self, _point: GeoPoint) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::geocode::reverse")
    }

    fn take_places(&mut self, _request: RequestId) -> Option<Vec<GeoPlace>> {
        unimplemented!("host::imports::geocode::take_places")
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
        GeocodeQuery { text: text.to_string(), near: None, limit, language: None }
    }

    fn near_query(near: GeoPoint) -> GeocodeQuery {
        GeocodeQuery { text: "Lisbon".to_string(), near: Some(near), limit: 5, language: None }
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
            Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument, "{case}: {}", error.message),
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
                assert!(!error.message.is_empty(), "the error record must carry a message");
                assert!(
                    error.message.chars().count() <= 256,
                    "the message is at most 256 characters, got {}",
                    error.message.chars().count()
                );
                assert_eq!(error.retry_after_ms, None, "only rate-limited carries retry-after-ms");
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
        assert_not_invalid_argument(run_search(text_query(&accepted, 5)), "200 two-byte characters");
        let refused = "é".repeat(201);
        assert_invalid_argument(run_search(text_query(&refused, 5)), "201 two-byte characters");
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
        assert_invalid_argument(run_search(near_query(point(90.5, 0.0))), "near latitude 90.5");
    }

    /// REQ: API-005
    #[test]
    fn search_rejects_near_longitude_below_minus_180() {
        assert_invalid_argument(run_search(near_query(point(0.0, -180.5))), "near longitude -180.5");
    }

    /// REQ: API-005
    #[test]
    fn search_accepts_near_at_range_corners() {
        assert_not_invalid_argument(run_search(near_query(point(90.0, 180.0))), "near 90, 180");
        assert_not_invalid_argument(run_search(near_query(point(-90.0, -180.0))), "near -90, -180");
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
