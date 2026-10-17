//! Helpers of the port twins (lane core-ui, part ports).
#![allow(dead_code)]
use ilium_plugin_core_ambient::AmbientClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ErrorCode, Event, PluginSide, SettingsChange, SettingsOrigin, Submission, ViewTarget,
};
use serde_json::{json, Value};

pub fn manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn activated_client() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<AmbientClient>();
    host
}

pub fn changed(keys: &[&str]) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| key.to_string()).collect(),
        origin: SettingsOrigin::User,
        side: PluginSide::Client,
    })
}

pub fn refused(key: &str, value: Value) -> ErrorCode {
    activated_client()
        .settings()
        .set(key, value)
        .expect_err("refused")
        .code
}

pub fn places(count: usize) -> Value {
    let mut map = serde_json::Map::new();
    for index in 0..count {
        map.insert(
            format!("place-{}", (b'a' + index as u8) as char),
            json!("Somewhere"),
        );
    }
    Value::Object(map)
}

pub fn no_submissions(host: &mut FakeHost, keys: &[&str]) -> Vec<Submission> {
    host.deliver(vec![changed(keys)])
}

pub fn scene_rows_hold() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
    for scene in ["stars", "clouds"] {
        assert!(host.settings().set("scene", json!(scene)).is_ok());
        assert_eq!(host.settings().get("scene"), Some(json!(scene)));
    }
    assert_eq!(refused("scene", json!("pipes")), ErrorCode::InvalidArgument);
    assert!(no_submissions(&mut host, &["scene"]).is_empty());
}

pub fn semantic_holds() {
    let mut host = activated_client();
    assert_eq!(host.settings().get("semantic"), Some(json!(false)));
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    assert!(no_submissions(&mut host, &["semantic"]).is_empty());
    assert_eq!(
        refused("semantic", json!("yes")),
        ErrorCode::InvalidArgument
    );
    assert!(host.settings().set("semantic", json!(false)).is_ok());
    assert_eq!(host.settings().get("semantic"), Some(json!(false)));
}

pub fn places_hold() {
    let mut host = activated_client();
    assert!(host.settings().set("saved_places", places(7)).is_ok());
    assert!(host.settings().set("saved_places", places(8)).is_ok());
    assert_eq!(
        refused("saved_places", places(9)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(host.settings().get("saved_places"), Some(places(8)));
}

pub fn observer_holds() {
    let mut host = activated_client();
    let ok = json!({ "lat": 90.0, "lon": -180.0 });
    assert!(host.settings().set("observer", ok.clone()).is_ok());
    assert_eq!(host.settings().get("observer"), Some(ok));
    assert_eq!(
        refused("observer", json!({ "lat": 90.0001, "lon": 0.0 })),
        ErrorCode::InvalidArgument
    );
}

pub fn render_holds(columns: u16, rows: u16) {
    let mut host = activated_client();
    let _ = host.render(ViewTarget::Panel("gallery".to_string()), columns, rows);
    assert!(host.failures().is_empty());
}

// --- manual-derived checks (manifest and views of core-plugin-ambient.md) -----------------------

pub fn manifest_json() -> Value {
    serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest file"))
        .expect("manifest json")
}

pub fn manifest_row(key: &str) -> Value {
    manifest_json()["contributes"]["settings"][0]["sections"][0]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["key"] == key)
        .cloned()
        .unwrap_or_else(|| panic!("no row {key}"))
}

pub fn manifest_capabilities() -> Vec<String> {
    manifest_json()["capabilities"]["client"]
        .as_array()
        .expect("client capabilities")
        .iter()
        .map(|value| value.as_str().expect("capability").to_string())
        .collect()
}

pub fn direct_tree(view: &str, columns: u16, rows: u16) -> ilium_plugin_sdk::types::UiTree {
    use ilium_plugin_sdk::Views;
    let mut client = AmbientClient::default();
    client.render(
        ViewTarget::Panel(view.to_string()),
        ilium_plugin_sdk::types::Area { columns, rows },
    )
}

/// The preview panel is one keyed Rgb8 surface asking for 40 columns and 4 rows at any size.
pub fn preview_surface_holds(columns: u16, rows: u16) {
    use ilium_plugin_sdk::types::{PixelFormat, Widget};
    let tree = direct_tree("scene-preview", columns, rows);
    let node = tree
        .nodes
        .iter()
        .find(|node| node.key.as_deref() == Some("frame"))
        .expect("frame surface");
    match &node.widget {
        Widget::Surface(spec) => {
            assert_eq!(spec.format, PixelFormat::Rgb8);
            assert_eq!(spec.min_columns, 40);
            assert_eq!(spec.min_rows, 4);
        }
        other => panic!("expected a surface, got {other:?}"),
    }
}

/// Setting a named saved place survives switching the scene back and forth.
pub fn named_values_survive_scene_switches() {
    let mut host = activated_client();
    let named = json!({ "dusk-home": "Home", "work": "Office" });
    assert!(host.settings().set("saved_places", named.clone()).is_ok());
    for scene in ["clouds", "stars", "clouds"] {
        assert!(host.settings().set("scene", json!(scene)).is_ok());
        assert_eq!(host.settings().get("scene"), Some(json!(scene)));
        assert_eq!(host.settings().get("saved_places"), Some(named.clone()));
    }
}
