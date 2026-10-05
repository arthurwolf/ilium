//! Pure SDK request-shape contracts; successful parsing grants no world authority.
//! Actual dispatch must still check the native operation and active original handle.
use super::*;
use serde_json::{json, Map, Value};

fn request() -> Map<String, Value> {
    json!({
        "world": {"id": "original-world-7", "kind": "worlds"},
        "x": -2, "y": -64, "z": -3,
        "width": 5, "height": 2, "depth": 7,
        "max_bytes": 4096
    })
    .as_object()
    .unwrap()
    .clone()
}

fn limits() -> RegionLimits {
    RegionLimits {
        cells: 4096,
        palette: 65,
        work: 1000000,
    }
}

#[test]
fn region_request_preserves_signed_saved_coordinates_and_borrows_handle_identity() {
    let fields = request();
    let parsed = parse_region_request(&fields, limits(), 4096).unwrap();
    assert_eq!(parsed.world_id, "original-world-7");
    assert_eq!(parsed.spec.origin, [-2, -64, -3]);
    assert_eq!(parsed.spec.size, [5, 2, 7]);
    assert_eq!(parsed.spec.max_bytes, 4096);
    let original = fields["world"]["id"].as_str().unwrap();
    assert_eq!(parsed.world_id.as_ptr(), original.as_ptr());
    // Negative saved-world height is valid syntax; generated-source bounds are
    // checked independently against the original prepared world after lookup.
}

#[test]
fn region_request_requires_exact_eight_field_inventory() {
    for key in [
        "world",
        "x",
        "y",
        "z",
        "width",
        "height",
        "depth",
        "max_bytes",
    ] {
        let mut fields = request();
        fields.remove(key);
        assert!(
            parse_region_request(&fields, limits(), 4096).is_err(),
            "missing {key}"
        );
    }
    let mut fields = request();
    fields.insert("grant".into(), json!({"id":"forged", "kind":"asset"}));
    assert!(parse_region_request(&fields, limits(), 4096).is_err());
}

#[test]
fn region_request_refuses_malformed_or_wrong_kind_handle_projections() {
    for world in [
        Value::Null,
        json!("original-world-7"),
        json!({"id":"original-world-7", "kind":"asset"}),
        json!({"id":"original-world-7", "kind":"worlds.frame"}),
        json!({"id":"", "kind":"worlds"}),
        json!({"id":"x".repeat(129), "kind":"worlds"}),
        json!({"id":7, "kind":"worlds"}),
        json!({"id":"original-world-7", "kind":"worlds", "identity":"forged"}),
    ] {
        let mut fields = request();
        fields.insert("world".into(), world);
        assert!(parse_region_request(&fields, limits(), 4096).is_err());
    }
}

#[test]
fn region_request_refuses_numeric_coercion_and_unrepresentable_endpoints() {
    for key in ["x", "y", "z"] {
        for value in [
            json!(1.5),
            json!("1"),
            json!(true),
            Value::Null,
            json!(i64::from(i32::MIN) - 1),
            json!(i64::from(i32::MAX) + 1),
        ] {
            let mut fields = request();
            fields.insert(key.into(), value);
            assert!(
                parse_region_request(&fields, limits(), 4096).is_err(),
                "invalid {key}"
            );
        }
    }
    for (coordinate, extent) in [("x", "width"), ("y", "height"), ("z", "depth")] {
        let mut fields = request();
        fields.insert(coordinate.into(), json!(i32::MAX));
        fields.insert(extent.into(), json!(2));
        assert!(parse_region_request(&fields, limits(), 4096).is_err());
    }
}

#[test]
fn region_request_refuses_zero_extents_and_cell_product_overflow() {
    for key in ["width", "height", "depth"] {
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1"),
            json!(u64::from(u32::MAX) + 1),
        ] {
            let mut fields = request();
            fields.insert(key.into(), value);
            assert!(parse_region_request(&fields, limits(), 4096).is_err());
        }
    }
    let mut fields = request();
    for key in ["width", "height", "depth"] {
        fields.insert(key.into(), json!(u32::MAX));
    }
    assert!(parse_region_request(&fields, limits(), 4096).is_err());
    let mut fields = request();
    fields.insert("width".into(), json!(4096));
    assert!(parse_region_request(&fields, limits(), 4096).is_err());
}

#[test]
fn region_request_refuses_unbounded_or_impossible_binary_response_budget() {
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("4096"),
        json!(4097),
        json!(u64::MAX),
    ] {
        let mut fields = request();
        fields.insert("max_bytes".into(), value);
        assert!(parse_region_request(&fields, limits(), 4096).is_err());
    }
    let mut fields = request();
    // Seventy u16 cells require 140 bytes plus the collector's existing
    // 256-byte metadata floor, before any actual palette-state strings.
    fields.insert("max_bytes".into(), json!(395));
    assert!(parse_region_request(&fields, limits(), 4096).is_err());
}
