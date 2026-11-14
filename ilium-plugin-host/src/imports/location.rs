//! Host implementation of `ilium:host/location`.
//!
//! [`LocationImport`] has no observer position and always answers `none`. [`ObserverLocation`]
//! answers with the observer position the host holds (the shared location row of the ambient
//! look), and only to an instance holding `location.observer`.

use ilium_plugin_api::generated::host::location::HostLocation;

use super::env::ImportEnv;

/// The grant that lets an instance read the observer position (api-location).
pub const OBSERVER_GRANT: &str = "location.observer";

/// Host side of the `location` interface without any observer position.
#[derive(Debug)]
pub struct LocationImport;

impl HostLocation for LocationImport {
    fn current(&mut self) -> Option<(f64, f64)> {
        // No observer position is attached to this import, and a position is never fabricated
        // (USR-061): report that none exists.
        None
    }
}

/// `location` of one instance, backed by the observer position the host gave it.
#[derive(Debug, Clone)]
pub struct ObserverLocation {
    env: ImportEnv,
    position: Option<(f64, f64)>,
}

impl ObserverLocation {
    /// The import of the instance `env` describes. A position that is not finite or lies outside
    /// latitude -90..=90 and longitude -180..=180 is dropped, so `current` reports none rather
    /// than a corrected position the user never set.
    pub fn new(env: ImportEnv, position: Option<(f64, f64)>) -> Self {
        let position = position.filter(|(latitude, longitude)| {
            latitude.is_finite()
                && longitude.is_finite()
                && (-90.0..=90.0).contains(latitude)
                && (-180.0..=180.0).contains(longitude)
        });
        Self { env, position }
    }
}

impl HostLocation for ObserverLocation {
    fn current(&mut self) -> Option<(f64, f64)> {
        // The grant is checked on every call, not only when the interface is linked.
        if !self.env.holds(OBSERVER_GRANT) {
            return None;
        }
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::{LocationImport, ObserverLocation};
    use crate::imports::env::ImportEnv;
    use ilium_plugin_api::generated::host::location::HostLocation;

    #[test]
    fn the_observer_position_reaches_only_an_instance_holding_the_grant() {
        let granted = ImportEnv::unrestricted().with_grants(vec!["location.observer".to_owned()]);
        let mut location = ObserverLocation::new(granted, Some((38.72, -9.14)));
        assert_eq!(location.current(), Some((38.72, -9.14)));
        let refused = ImportEnv::unrestricted().with_grants(vec!["http".to_owned()]);
        let mut location = ObserverLocation::new(refused, Some((38.72, -9.14)));
        assert_eq!(location.current(), None);
    }

    #[test]
    fn an_observer_position_out_of_range_is_reported_as_none() {
        for position in [
            (91.0, 0.0),
            (0.0, 181.0),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            let mut location = ObserverLocation::new(ImportEnv::unrestricted(), Some(position));
            assert_eq!(location.current(), None, "{position:?}");
        }
        let mut unset = ObserverLocation::new(ImportEnv::unrestricted(), None);
        assert_eq!(unset.current(), None);
    }

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
        assert_ne!(
            result,
            Some((0.0, 0.0)),
            "null island must not stand in for an unset position"
        );
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
            assert!(
                longitude.is_finite(),
                "longitude is not finite: {longitude}"
            );
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
