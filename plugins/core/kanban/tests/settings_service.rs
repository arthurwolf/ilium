//! The settings service must persist accepted changes and reject invalid values.

use ilium_plugin_core_kanban::KanbanServer;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

#[test]
fn settings_service_writes_valid_values_and_leaves_rejected_values_unchanged() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<KanbanServer>();

    for (key, value, expected) in [
        ("card_preview", "false", json!(false)),
        ("column_width", "42", json!(42)),
    ] {
        let payload = json!({"tab": "kanban-board", "key": key, "value": value});
        let reply = host
            .call_service("ilium.core.kanban/set-setting", "set", &payload.to_string())
            .expect("service answers");
        assert_eq!(
            serde_json::from_str::<Value>(&reply).expect("JSON reply"),
            json!({})
        );
        assert_eq!(
            host.kernel_settings().get("kanban-board", key),
            Some(expected)
        );
    }

    let reply = host
        .call_service(
            "ilium.core.kanban/set-setting",
            "set",
            &json!({"tab": "kanban-board", "key": "column_width", "value": "61"}).to_string(),
        )
        .expect("service answers");
    let reply: Value = serde_json::from_str(&reply).expect("JSON reply");
    assert_eq!(reply["error"]["code"], "invalid-argument");
    assert_eq!(
        host.kernel_settings().get("kanban-board", "column_width"),
        Some(json!(42))
    );
}

#[test]
fn settings_service_reports_a_refused_write() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.kernel_settings().set_unreadable("kanban-board");
    host.activate::<KanbanServer>();
    let reply = host
        .call_service(
            "ilium.core.kanban/set-setting",
            "set",
            &json!({"tab": "kanban-board", "key": "column_width", "value": "42"}).to_string(),
        )
        .expect("service answers");
    let reply: Value = serde_json::from_str(&reply).expect("JSON reply");
    assert_eq!(reply["error"]["code"], "not-permitted");
}

#[test]
fn settings_service_preserves_existing_board_controls_and_ranges() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json");
    let mut host = FakeHost::for_manifest(manifest)
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<KanbanServer>();
    for (key, value, expected) in [
        ("card_preview_lines", "10", json!(10)),
        ("minimum_column_width", "80", json!(80)),
    ] {
        let payload = json!({"tab": "kanban-board", "key": key, "value": value});
        let reply = host
            .call_service("ilium.core.kanban/set-setting", "set", &payload.to_string())
            .expect("service answers");
        assert_eq!(serde_json::from_str::<Value>(&reply).expect("JSON reply"), json!({}));
        assert_eq!(host.kernel_settings().get("kanban-board", key), Some(expected));
    }
}
