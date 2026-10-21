//! Host implementation of `ilium:host/location` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostLocation` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::location::HostLocation;

/// Host side of the `location` interface.
#[derive(Debug)]
pub struct LocationImport;

impl HostLocation for LocationImport {
    fn current(&mut self) -> Option<(f64, f64)> {
        unimplemented!("host::imports::location::current")
    }
}

#[cfg(test)]
mod tests {
    use super::LocationImport;
    use ilium_plugin_api::generated::host::location::HostLocation;

    /// Largest absolute latitude in decimal degrees, WGS 84 (api-location, "Surface" table).
    const MAX_LATITUDE_DEGREES: f64 = 90.0;

    /// Largest absolute longitude in decimal degrees, WGS 84 (api-location, "Surface" table).
    const MAX_LONGITUDE_DEGREES: f64 = 180.0;

    /// The unit import is in the state the harness gives it: no observer position is set
    /// and no grant state is attached, so `current` must report that no position exists.
    /// Ilium's `current` returns no error result, so a declined grant also shows up as `None`.
    fn fresh_import() -> LocationImport {
        LocationImport
    }

    /// REQ: CAP-013
    #[test]
    fn current_returns_none_when_no_observer_position_is_set() {
        let mut location = fresh_import();
        assert_eq!(location.current(), None);
    }

    /// REQ: USR-061
    #[test]
    fn repeated_calls_without_a_position_keep_returning_none() {
        let mut location = fresh_import();
        for call_index in 0..3 {
            assert_eq!(
                location.current(),
                None,
                "call {call_index} returned a value without a position being set"
            );
        }
    }

    /// REQ: USR-061
    #[test]
    fn unset_position_is_never_replaced_by_a_fabricated_fallback() {
        let mut location = fresh_import();
        let result = location.current();
        assert_ne!(result, Some((0.0, 0.0)), "null island must not stand in for an unset position");
        assert!(
            result.is_none(),
            "a fabricated position would reveal a location the user never set, got {result:?}"
        );
    }

    #[test]
    fn returned_position_is_finite_and_within_wgs84_bounds() {
        let mut location = fresh_import();
        if let Some((latitude, longitude)) = location.current() {
            assert!(latitude.is_finite(), "latitude is not finite: {latitude}");
            assert!(longitude.is_finite(), "longitude is not finite: {longitude}");
            assert!(
                (-MAX_LATITUDE_DEGREES..=MAX_LATITUDE_DEGREES).contains(&latitude),
                "latitude {latitude} is outside -90..=90"
            );
            assert!(
                (-MAX_LONGITUDE_DEGREES..=MAX_LONGITUDE_DEGREES).contains(&longitude),
                "longitude {longitude} is outside -180..=180"
            );
        }
    }
}
