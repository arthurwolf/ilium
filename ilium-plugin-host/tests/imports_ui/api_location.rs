//! `ilium:host/location` (api-location.md).

use ilium_plugin_api::generated::host::location::HostLocation;

use crate::common::*;

/// REQ: WIT:ilium:host/location.current
#[test]
fn wit_location_current_returns_the_configured_point() {
    let mut rig = rig(&["location.current"]);
    rig.kernel.set_location(Some((48.8566, 2.3522)));
    assert_eq!(rig.host.location().current(), Some((48.8566, 2.3522)));
}

/// REQ: WIT:ilium:host/location.current
#[test]
fn wit_location_current_unset_location_is_none() {
    let mut rig = rig(&["location.current"]);
    rig.kernel.set_location(None);
    assert_eq!(rig.host.location().current(), None);
}

/// REQ: WIT:ilium:host/location.current
#[test]
fn wit_location_current_declined_grant_is_none() {
    let mut rig = declined_rig(&["location.current"]);
    rig.kernel.set_location(Some((1.0, 2.0)));
    assert_eq!(rig.host.location().current(), None);
}
