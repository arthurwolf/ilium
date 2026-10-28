//! Agent-setup hooks on `FakeHost` that only the port test target uses (clicks, focus, key delivery).
//!
//! Each hook asks the plugin's service `ilium.core.agent-setup/ui` (see `tests/host_ext.rs`). Shared hooks live
//! in `tests/host_ext.rs`.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::host_ext::ui_call;

/// Port-target hooks. Implemented for `FakeHost`.
pub trait AgentSetupPortHostExt {
    /// Clicks a cell of the kernel settings tab the plugin contributes.
    fn click_kernel_tab(&mut self, surface: &str, column: u16, row: u16);
    /// The focused row id of the contributed tab.
    fn focused_row_id(&mut self, surface: &str) -> Option<String>;
    /// Sends a key to the open popup.
    fn key_on_popup(&mut self, key: &str);
    /// Sends a key to the contributed tab.
    fn key_on_tab(&mut self, surface: &str, key: &str);
}

impl AgentSetupPortHostExt for FakeHost {
    fn click_kernel_tab(&mut self, surface: &str, column: u16, row: u16) {
        ui_call(
            self,
            "click",
            json!({ "surface": surface, "column": column, "row": row }),
        );
        self.settle();
    }

    fn focused_row_id(&mut self, surface: &str) -> Option<String> {
        ui_call(self, "focused-row", json!({ "surface": surface }))["row"]
            .as_str()
            .map(str::to_string)
    }

    fn key_on_popup(&mut self, key: &str) {
        ui_call(self, "popup-key", json!({ "key": key }));
        self.settle();
    }

    fn key_on_tab(&mut self, surface: &str, key: &str) {
        ui_call(self, "tab-key", json!({ "surface": surface, "key": key }));
        self.settle();
    }
}
