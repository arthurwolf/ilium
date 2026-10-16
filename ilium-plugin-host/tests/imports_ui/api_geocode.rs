//! `ilium:host/geocode` (api-geocode.md).

use std::time::Duration;

use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
use ilium_plugin_api::generated::types::geo::{GeoPlace, GeoPoint};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["location.geocode"];

fn place(label: &str) -> GeoPlace {
    GeoPlace { point: GeoPoint { lat: 48.85, lon: 2.35 }, label: label.to_owned(), country_code: Some("FR".to_owned()), kind: None }
}

fn search_for(text: String, limit: u8) -> GeocodeQuery {
    GeocodeQuery { text, near: None, limit, language: None }
}

fn seeded() -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.set_geocode_places(vec![place("Paris"), place("Parisot")]);
    rig
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_completes_with_places() {
    let mut rig = seeded();
    let id = rig.host.geocode().search(search_for("Paris".to_owned(), 5)).expect("accepted");
    rig.host.pump(8);
    let places = rig.host.geocode().take_places(id).expect("places");
    assert_eq!(places[0].label, "Paris");
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_text_length_boundaries_invalid_argument() {
    let mut rig = seeded();
    assert_eq!(code_of(rig.host.geocode().search(search_for(text_of(1), 5))), ErrorCode::InvalidArgument);
    assert!(rig.host.geocode().search(search_for(text_of(2), 5)).is_ok());
    rig.host.advance_clock(Duration::from_secs(2));
    assert!(rig.host.geocode().search(search_for(text_of(200), 5)).is_ok());
    rig.host.advance_clock(Duration::from_secs(2));
    assert_eq!(code_of(rig.host.geocode().search(search_for(text_of(201), 5))), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_limit_boundaries_invalid_argument() {
    let mut rig = seeded();
    assert_eq!(code_of(rig.host.geocode().search(search_for("Paris".to_owned(), 0))), ErrorCode::InvalidArgument);
    assert!(rig.host.geocode().search(search_for("Paris".to_owned(), 1)).is_ok());
    rig.host.advance_clock(Duration::from_secs(2));
    assert!(rig.host.geocode().search(search_for("Paris".to_owned(), 8)).is_ok());
    rig.host.advance_clock(Duration::from_secs(2));
    assert_eq!(code_of(rig.host.geocode().search(search_for("Paris".to_owned(), 9))), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_second_call_within_a_second_rate_limited() {
    let mut rig = seeded();
    rig.host.geocode().search(search_for("Paris".to_owned(), 5)).expect("first");
    let second = rig.host.geocode().search(search_for("Paris".to_owned(), 5));
    let error = second.expect_err("the gate is closed");
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retry_after_ms.is_some_and(|ms| (1..=1_000).contains(&ms)));
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_after_a_second_the_gate_reopens() {
    let mut rig = seeded();
    rig.host.geocode().search(search_for("Paris".to_owned(), 5)).expect("first");
    rig.host.advance_clock(Duration::from_millis(1_001));
    assert!(rig.host.geocode().search(search_for("Paris".to_owned(), 5)).is_ok());
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_ten_seconds_without_answer_timed_out() {
    let mut rig = seeded();
    rig.kernel.set_geocode_silent(true);
    let id = rig.host.geocode().search(search_for("Paris".to_owned(), 5)).expect("accepted");
    rig.host.advance_clock(Duration::from_secs(10));
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), Some(ErrorCode::TimedOut));
}

/// REQ: WIT:ilium:host/geocode.search
#[test]
fn wit_geocode_search_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["location.geocode"]);
    assert_eq!(code_of(rig.host.geocode().search(search_for("Paris".to_owned(), 5))), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/geocode.reverse
#[test]
fn wit_geocode_reverse_completes_with_a_place() {
    let mut rig = seeded();
    let id = rig.host.geocode().reverse(GeoPoint { lat: 48.85, lon: 2.35 }).expect("accepted");
    rig.host.pump(8);
    assert!(!rig.host.geocode().take_places(id).expect("places").is_empty());
}

/// REQ: WIT:ilium:host/geocode.reverse
#[test]
fn wit_geocode_reverse_rate_limited_within_a_second() {
    let mut rig = seeded();
    rig.host.geocode().reverse(GeoPoint { lat: 1.0, lon: 1.0 }).expect("first");
    assert_eq!(code_of(rig.host.geocode().reverse(GeoPoint { lat: 1.0, lon: 1.0 })), ErrorCode::RateLimited);
}

/// REQ: WIT:ilium:host/geocode.reverse
#[test]
fn wit_geocode_reverse_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["location.geocode"]);
    assert_eq!(code_of(rig.host.geocode().reverse(GeoPoint { lat: 0.0, lon: 0.0 })), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/geocode.take-places
#[test]
fn wit_geocode_take_places_returns_the_list_once() {
    let mut rig = seeded();
    let id = rig.host.geocode().search(search_for("Paris".to_owned(), 5)).expect("accepted");
    rig.host.pump(8);
    assert!(rig.host.geocode().take_places(id).is_some());
    assert!(rig.host.geocode().take_places(id).is_none());
}

/// REQ: WIT:ilium:host/geocode.take-places
#[test]
fn wit_geocode_take_places_unknown_request_is_none() {
    let mut rig = seeded();
    assert!(rig.host.geocode().take_places(RequestId(55_555)).is_none());
}
