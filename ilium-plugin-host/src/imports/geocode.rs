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
