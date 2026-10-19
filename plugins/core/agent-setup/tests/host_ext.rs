//! Agent-setup-lane testing hooks on `FakeHost` that both the feature and the port test targets use
//! (opening a project, popups, the contributed kernel tab).
//!
//! Target-specific hooks live beside each target: `features/host_ext_features.rs` and `port/host_ext_port.rs`.
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

use ilium_plugin_sdk::testing::{FakeHost, Screen};

/// Agent-setup-lane hooks used by both test targets. Implemented for `FakeHost` with skeleton bodies.
pub trait AgentSetupHostExt {
    /// Opens the project at `name` as the current project (agent-setup "Set-up tab").
    fn open_project(&mut self, _name: &str) {
        unimplemented!("agent-setup.open-project")
    }
    /// Activates a row of the contributed tab.
    fn press_row(&mut self, _surface: &str, _row: &str) {
        unimplemented!("agent-setup.press-row")
    }
    /// Presses a popup button by label.
    fn press_popup_button(&mut self, _label: &str) {
        unimplemented!("agent-setup.press-popup-button")
    }
    /// Renders the contributed tab.
    fn render_kernel_tab(&mut self, _surface: &str, _columns: u16, _rows: u16) -> Screen {
        unimplemented!("agent-setup.render-kernel-tab")
    }
    /// Renders the open popup as text (empty when none is open).
    fn render_popup(&mut self, _columns: u16) -> String {
        unimplemented!("agent-setup.render-popup")
    }
    /// Types text into a field of the contributed tab.
    fn type_into_field(&mut self, _surface: &str, _field: &str, _text: &str) {
        unimplemented!("agent-setup.type-into-field")
    }
}

impl AgentSetupHostExt for FakeHost {}
