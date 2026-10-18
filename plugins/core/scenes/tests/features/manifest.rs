//! The exact manifest of the Scenes plugin (page section "Manifest", COR-010, COR-011, COR-050).
use crate::common::*;
use crate::feature_support::*;
use serde_json::json;

/// REQ: COR-010
#[test]
fn manifest_010_declares_identity_and_api() {
    let m = manifest_value();
    assert_eq!(m["id"], "ilium.core.scenes");
    assert_eq!(m["api"], "ilium:plugin@0.1.0");
    assert_eq!(m["name"], "Scenes");
    assert_eq!(m["version"], "0.1.0");
    assert_eq!(m["publisher"], "Ilium");
    assert_eq!(m["license"], "MIT");
}

/// REQ: COR-010
#[test]
fn manifest_010_scene_side_only_with_base_world() {
    let m = manifest_value();
    assert_eq!(m["scene"]["entry"], "scene.wasm");
    assert_eq!(m["scene"]["world"], "ilium:anim/base@0.1.0");
    assert_eq!(m["scene"]["exports"], json!([]));
    for side in ["server", "client"] {
        assert!(m.get(side).is_none(), "no {side} side expected");
    }
}

/// REQ: COR-010
#[test]
fn manifest_010_scene_capabilities_are_exact() {
    let m = manifest_value();
    assert_eq!(
        m["capabilities"],
        json!({"scene": ["media.video", "location.observer", "gpu.kernels", "http:services.swpc.noaa.gov"]})
    );
}

/// REQ: COR-010
#[test]
fn manifest_010_activation_is_scene_wildcard_and_tier_first_party() {
    let m = manifest_value();
    assert_eq!(m["activation"], json!(["scene:*"]));
    assert_eq!(m["limits_tier"], "first-party");
}

/// REQ: COR-010, COR-011
#[test]
fn manifest_011_contributes_exactly_three_scenes_in_page_order() {
    let ids: Vec<String> = scene_entries()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, MANIFEST_SCENES);
}

/// REQ: COR-011, F-096
#[test]
fn manifest_011_waves_entry_is_exact() {
    assert_eq!(
        scene_entry("waves"),
        json!({"id":"waves","title":"Waves","family":"procedural","modes":["live","pre-rendered"],"frame_rate_cap":24,"clip_seconds":12,"settings_schema":{"type":"object","properties":{}}})
    );
}

/// REQ: COR-011, F-096
#[test]
fn manifest_011_clock_entry_is_exact() {
    assert_eq!(
        scene_entry("clock"),
        json!({"id":"clock","title":"Clock","family":"procedural","modes":["live"],"frame_rate_cap":2,"glyph_plane":true,"settings_schema":{"type":"object","properties":{}}})
    );
}

/// REQ: COR-011, F-100
#[test]
fn manifest_011_stars_entry_is_exact() {
    assert_eq!(
        scene_entry("stars-overhead"),
        json!({"id":"stars-overhead","title":"Stars overhead","family":"maps-space","modes":["live"],"frame_rate_cap":10,"settings_schema":{"type":"object","properties":{}}})
    );
}

/// REQ: COR-011
#[test]
fn manifest_011_no_scene_declares_look_controls() {
    for entry in scene_entries() {
        let props = entry["settings_schema"]["properties"].as_object().unwrap();
        for forbidden in [
            "brightness",
            "contrast",
            "hue",
            "palette",
            "dither",
            "gamma",
            "saturation",
        ] {
            assert!(
                !props.contains_key(forbidden),
                "{} declares {forbidden}",
                entry["id"]
            );
        }
    }
}

/// REQ: COR-011
#[test]
fn manifest_011_boundary_frame_rate_caps_are_positive_and_within_sixty() {
    for entry in scene_entries() {
        let cap = entry["frame_rate_cap"].as_u64().unwrap();
        assert!((1..=60).contains(&cap), "{} cap {cap}", entry["id"]);
    }
}

/// REQ: COR-011
#[test]
fn manifest_011_only_waves_is_pre_rendered_and_has_a_clip_length() {
    for entry in scene_entries() {
        let pre = entry["modes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "pre-rendered");
        assert_eq!(pre, entry["id"] == "waves", "{}", entry["id"]);
        assert_eq!(entry.get("clip_seconds").is_some(), pre, "{}", entry["id"]);
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_page_json_block_equals_the_skeleton_manifest() {
    let page = scenes_page();
    let block = json_block_after(&page, "This is the whole manifest");
    assert_eq!(block, manifest_value());
}

/// REQ: COR-050
#[test]
fn manifest_050_scene_wasm_package_is_not_shipped_with_a_server_or_client_entry() {
    let m = manifest_value();
    assert!(m["scene"]["entry"].as_str().unwrap().ends_with(".wasm"));
    assert!(m["scene"].get("config").is_none());
}
