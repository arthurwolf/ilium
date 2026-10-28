//! Agent-setup hooks on `FakeHost` that only the port test target uses (clicks, focus, key delivery).
//!
//! Skeleton bodies. Shared hooks live in `tests/host_ext.rs`.

use ilium_plugin_sdk::testing::FakeHost;

/// Port-target hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait AgentSetupPortHostExt {
    /// Clicks a cell of the kernel settings tab the plugin contributes.
    fn click_kernel_tab(&mut self, _surface: &str, _column: u16, _row: u16) {
        unimplemented!("agent-setup.click-kernel-tab")
    }
    /// The focused row id of the contributed tab.
    fn focused_row_id(&self, _surface: &str) -> Option<String> {
        unimplemented!("agent-setup.focused-row-id")
    }
    /// Sends a key to the open popup.
    fn key_on_popup(&mut self, _key: &str) {
        unimplemented!("agent-setup.key-on-popup")
    }
    /// Sends a key to the contributed tab.
    fn key_on_tab(&mut self, _surface: &str, _key: &str) {
        unimplemented!("agent-setup.key-on-tab")
    }
}

impl AgentSetupPortHostExt for FakeHost {}
