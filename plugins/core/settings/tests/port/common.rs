//! Shared helpers of the port twins: the client host, tab targets, toggles, text of a view.

use std::path::PathBuf;

use ilium_plugin_core_settings::SettingsClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::{Value, json};

/// Path of the plugin manifest.
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<SettingsClient>();
    host
}

/// The settings page of a tab id (core-plugin-settings.md, "The 25 tabs").
pub fn tab(id: &str) -> ViewTarget {
    ViewTarget::SettingsPage(id.to_string())
}

/// The screen text of a tab at a size.
pub fn tab_text(host: &mut FakeHost, id: &str, columns: u16, rows: u16) -> String {
    host.render(tab(id), columns, rows).text()
}

/// Flip a toggle row of a tab the way the user does.
pub fn toggle(host: &mut FakeHost, tab_id: &str, key: &str, value: bool) {
    host.send_action(tab(tab_id), key, ActionKind::Toggled, ActionValue::Flag(value));
    host.settle();
}

/// Choose an option (or type a colour) in a row of a tab.
pub fn choose(host: &mut FakeHost, tab_id: &str, key: &str, value: &str) {
    host.send_action(tab(tab_id), key, ActionKind::Changed, ActionValue::Text(value.to_string()));
    host.settle();
}

/// Type a number into a row of a tab and let the 600 ms auto-save elapse.
pub fn type_number(host: &mut FakeHost, tab_id: &str, key: &str, value: f64) {
    host.send_action(tab(tab_id), key, ActionKind::Changed, ActionValue::Number(value));
    host.advance_clock(600);
    host.settle();
}

/// Press Enter in a row of a tab (saves a typed value at once).
pub fn submit(host: &mut FakeHost, tab_id: &str, key: &str) {
    host.send_action(tab(tab_id), key, ActionKind::Submitted, ActionValue::None);
    host.settle();
}

/// The stored value of a key.
pub fn value_of(host: &mut FakeHost, key: &str) -> Option<Value> {
    host.settings().get(key)
}

/// `json!(true)` shorthand used in assertions.
pub fn yes() -> Value {
    json!(true)
}

/// True when a setting has no stored override (absent or null).
pub fn is_unset(value: Option<Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

/// Press the keys in order on a tab page (a focused row receives them).
pub fn press(host: &mut FakeHost, tab_id: &str, keys: &[&str]) {
    for key in keys {
        host.key(tab(tab_id), key);
    }
    host.settle();
}
