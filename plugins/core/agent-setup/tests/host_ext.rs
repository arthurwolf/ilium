//! Agent-setup-lane testing hooks on `FakeHost` that both the feature and the port test targets use
//! (opening a project, popups, the contributed kernel tab).
//!
//! Target-specific hooks live beside each target: `features/host_ext_features.rs` and `port/host_ext_port.rs`.
//! Plugin-specific scaffolding kept out of the shared harness type. The harness draws no widgets, so every hook
//! asks the plugin itself through its service `ilium.core.agent-setup/ui` (methods `open-project`, `press-row`,
//! `popup-button`, `tab-text`, `popup-text`, `type`) and the tests assert on what the plugin answers.

use ilium_plugin_sdk::testing::{FakeHost, Screen};
use ilium_plugin_sdk::types::ViewTarget;
use serde_json::{json, Value};

pub const UI_SERVICE: &str = "ilium.core.agent-setup/ui";

/// Calls one method of the plugin's UI service; a failure stops the test with the method named.
pub fn ui_call(host: &mut FakeHost, method: &str, payload: Value) -> Value {
    let text = host
        .call_service(UI_SERVICE, method, &payload.to_string())
        .unwrap_or_else(|error| panic!("{UI_SERVICE}.{method} failed: {error:?}"));
    if text.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{UI_SERVICE}.{method} replied invalid JSON: {error}"))
}

/// Agent-setup-lane hooks used by both test targets. Implemented for `FakeHost`.
pub trait AgentSetupHostExt {
    /// Opens the project at `name` as the current project (agent-setup "Set-up tab").
    fn open_project(&mut self, name: &str);
    /// Activates a row of the contributed tab.
    fn press_row(&mut self, surface: &str, row: &str);
    /// Presses a popup button by label.
    fn press_popup_button(&mut self, label: &str);
    /// Renders the contributed tab.
    fn render_kernel_tab(&mut self, surface: &str, columns: u16, rows: u16) -> Screen;
    /// Renders the open popup as text (empty when none is open).
    fn render_popup(&mut self, columns: u16) -> String;
    /// Types text into a field of the contributed tab.
    fn type_into_field(&mut self, surface: &str, field: &str, text: &str);
}

impl AgentSetupHostExt for FakeHost {
    fn open_project(&mut self, name: &str) {
        ui_call(self, "open-project", json!({ "name": name }));
        self.settle();
    }

    fn press_row(&mut self, surface: &str, row: &str) {
        ui_call(self, "press-row", json!({ "surface": surface, "row": row }));
        self.settle();
    }

    fn press_popup_button(&mut self, label: &str) {
        ui_call(self, "popup-button", json!({ "label": label }));
        self.settle();
    }

    /// The tab text the plugin answers with is laid into the cells of a harness screen, one row per line.
    fn render_kernel_tab(&mut self, surface: &str, columns: u16, rows: u16) -> Screen {
        let mut screen = self.render(ViewTarget::SettingsPage(surface.to_string()), columns, rows);
        let reply = ui_call(
            self,
            "tab-text",
            json!({ "surface": surface, "columns": columns, "rows": rows }),
        );
        let text = reply["text"].as_str().unwrap_or_default().to_string();
        for (row, line) in text.lines().take(usize::from(rows)).enumerate() {
            for (column, symbol) in line.chars().take(usize::from(columns)).enumerate() {
                screen.cells[row * usize::from(columns) + column].symbol = symbol.to_string();
            }
        }
        screen
    }

    fn render_popup(&mut self, columns: u16) -> String {
        ui_call(self, "popup-text", json!({ "columns": columns }))["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn type_into_field(&mut self, surface: &str, field: &str, text: &str) {
        ui_call(
            self,
            "type",
            json!({ "surface": surface, "field": field, "text": text }),
        );
        self.settle();
    }
}
