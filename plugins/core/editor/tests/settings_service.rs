//! Accepted editor settings must be saved rather than acknowledged and lost.

use ilium_plugin_core_editor::EditorServer;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

#[test]
fn editor_setting_is_persisted_and_an_invalid_value_does_not_replace_it() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<EditorServer>();

    let service = "ilium.core.editor/set-setting";
    let reply = host
        .call_service(
            service,
            "set",
            &json!({"tab": "editor", "key": "show_line_numbers", "value": "false"})
                .to_string(),
        )
        .expect("service answers");
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap(), json!({}));
    assert_eq!(
        host.kernel_settings().get("editor", "show_line_numbers"),
        Some(json!(false))
    );

    let reply = host
        .call_service(
            service,
            "set",
            &json!({"tab": "editor", "key": "show_line_numbers", "value": "invalid"})
                .to_string(),
        )
        .expect("service answers");
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["error"]["code"], "invalid-argument");
    assert_eq!(
        host.kernel_settings().get("editor", "show_line_numbers"),
        Some(json!(false))
    );
}

#[test]
fn all_editor_settings_are_validated_and_written_with_their_native_types() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest).unwrap().side("server").build();
    host.activate::<EditorServer>();
    for (key, value, expected) in [
        ("line_display", "Wrap", json!("wrap")),
        ("show_minimap", "false", json!(false)),
        ("autosave_enabled", "false", json!(false)),
        ("autosave_delay_ms", "2500", json!(2500)),
        ("markdown_rendered_by_default", "true", json!(true)),
    ] {
        let reply = host.call_service(
            "ilium.core.editor/set-setting", "set",
            &json!({"tab":"editor", "key":key, "value":value}).to_string(),
        ).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&reply).unwrap(), json!({}));
        assert_eq!(host.kernel_settings().get("editor", key), Some(expected));
    }
    for (key, value) in [("line_display", "sideways"), ("autosave_delay_ms", "249"), ("unknown", "true")] {
        let reply = host.call_service(
            "ilium.core.editor/set-setting", "set",
            &json!({"tab":"editor", "key":key, "value":value}).to_string(),
        ).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["error"]["code"], "invalid-argument");
    }
}

#[test]
fn editor_setting_reports_a_host_write_refusal() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest).unwrap().side("server").build();
    host.kernel_settings().set_unreadable("editor");
    host.activate::<EditorServer>();
    let reply = host.call_service(
        "ilium.core.editor/set-setting", "set",
        &json!({"tab":"editor", "key":"show_line_numbers", "value":"false"}).to_string(),
    ).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["error"]["code"], "not-permitted");
}
