//! The five contributed rows: defaults, valid values, rejected values (error code invalid-argument),
//! persistence round trips. Limits come from reference-settings-rows.md rows 2, 13, 16 and the manifest.

use ilium_plugin_sdk::types::ErrorCode;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

fn rejected(key: &str, value: Value) -> ErrorCode {
    let mut host = activated_client();
    let error = host.settings().set(key, value).expect_err("the value must be refused");
    error.code
}

fn accepted(key: &str, value: Value) {
    let mut host = activated_client();
    assert!(host.settings().set(key, value.clone()).is_ok(), "{value}");
    assert_eq!(host.settings().get(key), Some(value));
}

fn one(name: String, label: &str) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(name, json!(label));
    Value::Object(map)
}

fn place(lat: f64, lon: f64) -> Value {
    json!({ "lat": lat, "lon": lon })
}

fn places(count: usize) -> Value {
    let mut map = serde_json::Map::new();
    for index in 0..count {
        let letter = (b'a' + index as u8) as char;
        map.insert(format!("place-{letter}"), json!("Somewhere"));
    }
    Value::Object(map)
}

/// The key grammar of the manifest, `^[a-z-]{1,24}$`, written out for the oracle.
fn is_place_name(text: &str) -> bool {
    let count = text.chars().count();
    (1..=24).contains(&count) && text.chars().all(|symbol| symbol.is_ascii_lowercase() || symbol == '-')
}

// --- defaults ---------------------------------------------------------------------------------

/// REQ: COR-011, F-093
#[test]
fn settings_011_activation_succeeds_for_the_client_side() {
    let mut host = client_host();
    assert!(host.try_activate::<ilium_plugin_core_ambient::AmbientClient>().is_ok());
}

/// REQ: COR-011, F-093
#[test]
fn settings_011_scene_defaults_to_stars() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

/// REQ: COR-011, F-103
#[test]
fn settings_011_semantic_defaults_to_off() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("semantic"), Some(json!(false)));
}

/// REQ: COR-011, F-093
#[test]
fn settings_011_observer_defaults_to_lisbon() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("observer"), Some(json!({ "lat": 38.7223, "lon": -9.1393, "label": "Lisbon" })));
}

/// REQ: COR-011, F-093
#[test]
fn settings_011_saved_places_defaults_to_an_empty_map() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("saved_places"), Some(json!({})));
}

/// REQ: COR-011
#[test]
fn settings_011_preview_row_has_no_value() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("scene-preview"), None);
}

// --- scene choice -----------------------------------------------------------------------------

/// REQ: F-093
#[test]
fn settings_093_scene_accepts_clouds() {
    accepted("scene", json!("clouds"));
}

/// REQ: F-093
#[test]
fn settings_093_scene_accepts_stars_again() {
    accepted("scene", json!("stars"));
}

/// REQ: F-093
#[test]
fn settings_093_scene_outside_the_options_is_invalid_argument() {
    assert_eq!(rejected("scene", json!("pipes")), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_scene_option_titles_are_not_values_invalid_argument() {
    assert_eq!(rejected("scene", json!("Stars")), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_scene_empty_string_is_invalid_argument() {
    assert_eq!(rejected("scene", json!("")), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_scene_number_is_invalid_argument() {
    assert_eq!(rejected("scene", json!(1)), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_rejected_scene_leaves_the_stored_value() {
    let mut host = activated_client();
    let _ = host.settings().set("scene", json!("pipes"));
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

// --- semantic toggle --------------------------------------------------------------------------

/// REQ: F-103
#[test]
fn settings_103_semantic_accepts_true() {
    accepted("semantic", json!(true));
}

/// REQ: F-103
#[test]
fn settings_103_semantic_accepts_false() {
    accepted("semantic", json!(false));
}

/// REQ: F-103
#[test]
fn settings_103_semantic_string_is_invalid_argument() {
    assert_eq!(rejected("semantic", json!("yes")), ErrorCode::InvalidArgument);
}

/// REQ: F-103
#[test]
fn settings_103_semantic_integer_is_invalid_argument() {
    assert_eq!(rejected("semantic", json!(1)), ErrorCode::InvalidArgument);
}

/// REQ: F-103
#[test]
fn settings_103_semantic_null_is_invalid_argument() {
    assert_eq!(rejected("semantic", Value::Null), ErrorCode::InvalidArgument);
}

// --- observer location ------------------------------------------------------------------------

/// REQ: F-093
#[test]
fn settings_093_observer_accepts_a_place_with_a_label() {
    accepted("observer", json!({ "lat": 48.8566, "lon": 2.3522, "label": "Paris" }));
}

/// The label is optional (`{ lat, lon, label? }`).
///
/// REQ: F-093
#[test]
fn settings_093_observer_accepts_a_place_without_a_label() {
    accepted("observer", place(0.0, 0.0));
}

/// REQ: F-093
#[test]
fn settings_093_observer_accepts_both_latitude_extremes() {
    accepted("observer", place(90.0, 0.0));
    accepted("observer", place(-90.0, 0.0));
}

/// REQ: F-093
#[test]
fn settings_093_observer_accepts_both_longitude_extremes() {
    accepted("observer", place(0.0, 180.0));
    accepted("observer", place(0.0, -180.0));
}

/// REQ: F-093
#[test]
fn settings_093_observer_latitude_above_ninety_is_invalid_argument() {
    assert_eq!(rejected("observer", place(90.0001, 0.0)), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_latitude_below_minus_ninety_is_invalid_argument() {
    assert_eq!(rejected("observer", place(-90.0001, 0.0)), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_longitude_above_one_eighty_is_invalid_argument() {
    assert_eq!(rejected("observer", place(0.0, 180.0001)), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_longitude_below_minus_one_eighty_is_invalid_argument() {
    assert_eq!(rejected("observer", place(0.0, -180.0001)), ErrorCode::InvalidArgument);
}

/// Both `lat` and `lon` are required.
///
/// REQ: F-093
#[test]
fn settings_093_observer_without_longitude_is_invalid_argument() {
    assert_eq!(rejected("observer", json!({ "lat": 10.0 })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_without_latitude_is_invalid_argument() {
    assert_eq!(rejected("observer", json!({ "lon": 10.0 })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_label_of_one_hundred_twenty_characters_is_accepted() {
    accepted("observer", json!({ "lat": 1.0, "lon": 2.0, "label": "x".repeat(120) }));
}

/// REQ: F-093
#[test]
fn settings_093_observer_label_of_one_hundred_twenty_one_characters_is_invalid_argument() {
    assert_eq!(
        rejected("observer", json!({ "lat": 1.0, "lon": 2.0, "label": "x".repeat(121) })),
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-093
#[test]
fn settings_093_observer_text_instead_of_an_object_is_invalid_argument() {
    assert_eq!(rejected("observer", json!("Lisbon")), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_observer_text_coordinates_are_invalid_argument() {
    assert_eq!(rejected("observer", json!({ "lat": "38.7", "lon": "-9.1" })), ErrorCode::InvalidArgument);
}

// --- saved places map -------------------------------------------------------------------------

/// REQ: F-093
#[test]
fn settings_093_saved_places_accepts_one_named_place() {
    accepted("saved_places", json!({ "home": "Home town" }));
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_accepts_eight_entries() {
    accepted("saved_places", places(8));
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_seven_entries_are_accepted() {
    accepted("saved_places", places(7));
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_nine_entries_are_invalid_argument() {
    assert_eq!(rejected("saved_places", places(9)), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_name_of_twenty_four_characters_is_accepted() {
    accepted("saved_places", one("a".repeat(24), "Label"));
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_name_of_twenty_five_characters_is_invalid_argument() {
    assert_eq!(rejected("saved_places", one("a".repeat(25), "Label")), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_empty_name_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "": "Label" })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_hyphen_is_allowed_in_a_name() {
    accepted("saved_places", json!({ "my-home": "Label" }));
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_upper_case_name_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "Home": "Label" })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_digit_in_a_name_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "home1": "Label" })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_underscore_in_a_name_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "my_home": "Label" })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_space_in_a_name_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "my home": "Label" })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_integer_value_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!({ "home": 3 })), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_array_instead_of_an_object_is_invalid_argument() {
    assert_eq!(rejected("saved_places", json!(["home"])), ErrorCode::InvalidArgument);
}

/// REQ: F-093
#[test]
fn settings_093_saved_places_can_be_emptied_again() {
    let mut host = activated_client();
    host.settings().set("saved_places", json!({ "home": "Home" })).expect("set");
    host.settings().set("saved_places", json!({})).expect("clear");
    assert_eq!(host.settings().get("saved_places"), Some(json!({})));
}

// --- unknown rows -----------------------------------------------------------------------------

/// REQ: COR-011
#[test]
fn settings_011_unknown_key_is_not_found() {
    assert_eq!(rejected("palette", json!("dusk")), ErrorCode::NotFound);
}

/// Shared look values (colour mode, palette, dither, frame-rate cap) are kernel values, not rows of this plugin.
///
/// REQ: F-093, F-094, COR-010
#[test]
fn settings_094_shared_look_values_are_not_rows_of_the_plugin() {
    for key in ["color_mode", "dither", "frame_rate_cap", "speed", "density"] {
        assert_eq!(rejected(key, json!(1)), ErrorCode::NotFound, "{key}");
    }
}

// --- properties -------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-093
    #[test]
    fn settings_093_property_every_coordinate_inside_the_ranges_is_accepted(lat in -90.0f64..=90.0, lon in -180.0f64..=180.0) {
        let mut host = activated_client();
        prop_assert!(host.settings().set("observer", place(lat, lon)).is_ok());
    }

    // REQ: F-093
    #[test]
    fn settings_093_property_a_latitude_beyond_the_poles_is_refused(lat in 90.001f64..1000.0, lon in -180.0f64..=180.0) {
        let mut host = activated_client();
        let error = host.settings().set("observer", place(lat, lon)).expect_err("out of range");
        prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // REQ: F-093
    #[test]
    fn settings_093_property_a_longitude_beyond_the_date_line_is_refused(lat in -90.0f64..=90.0, lon in 180.001f64..1000.0) {
        let mut host = activated_client();
        let error = host.settings().set("observer", place(lat, lon)).expect_err("out of range");
        prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // REQ: F-093
    #[test]
    fn settings_093_property_a_place_name_is_accepted_exactly_when_it_matches_the_manifest_pattern(name in "[a-zA-Z0-9_ -]{0,30}") {
        let mut host = activated_client();
        let result = host.settings().set("saved_places", one(name.clone(), "Label"));
        prop_assert_eq!(result.is_ok(), is_place_name(&name));
    }
}
