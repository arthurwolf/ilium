//! Agent-setup-lane testing hooks on `FakeHost` (kernel settings tab driving, popups, commands).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

#![allow(dead_code)]

use ilium_plugin_sdk::testing::{FakeHost, Screen};

/// Agent-setup-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait AgentSetupHostExt {
    /// Opens the project at `name` as the current project (agent-setup "Set-up tab").
    fn open_project(&mut self, _name: &str) { unimplemented!("agent-setup.open-project") }
    /// Runs a registered command by id (agent-setup "Commands").
    fn invoke_command(&mut self, _command: &str) { unimplemented!("agent-setup.invoke-command") }
    /// Clicks a cell of the kernel settings tab the plugin contributes.
    fn click_kernel_tab(&mut self, _surface: &str, _column: u16, _row: u16) { unimplemented!("agent-setup.click-kernel-tab") }
    /// The focused row id of the contributed tab.
    fn focused_row_id(&self, _surface: &str) -> Option<String> { unimplemented!("agent-setup.focused-row-id") }
    /// Row ids of the contributed tab, in order.
    fn kernel_tab_row_ids(&self, _surface: &str) -> Vec<String> { unimplemented!("agent-setup.kernel-tab-row-ids") }
    /// Sends a key to the open popup.
    fn key_on_popup(&mut self, _key: &str) { unimplemented!("agent-setup.key-on-popup") }
    /// Sends a key to the contributed tab.
    fn key_on_tab(&mut self, _surface: &str, _key: &str) { unimplemented!("agent-setup.key-on-tab") }
    /// Sends a key to a text field of the contributed tab.
    fn press_field_key(&mut self, _surface: &str, _field: &str, _key: char) { unimplemented!("agent-setup.press-field-key") }
    /// Presses a popup button by label.
    fn press_popup_button(&mut self, _label: &str) { unimplemented!("agent-setup.press-popup-button") }
    /// Activates a row of the contributed tab.
    fn press_row(&mut self, _surface: &str, _row: &str) { unimplemented!("agent-setup.press-row") }
    /// Provides a help file the plugin ships.
    fn provide_help_file(&mut self, _path: &str, _text: &str) { unimplemented!("agent-setup.provide-help-file") }
    /// Renders the contributed tab.
    fn render_kernel_tab(&mut self, _surface: &str, _columns: u16, _rows: u16) -> Screen { unimplemented!("agent-setup.render-kernel-tab") }
    /// Renders the open popup as text (empty when none is open).
    fn render_popup(&mut self, _columns: u16) -> String { unimplemented!("agent-setup.render-popup") }
    /// Types text into a field of the contributed tab.
    fn type_into_field(&mut self, _surface: &str, _field: &str, _text: &str) { unimplemented!("agent-setup.type-into-field") }
}

impl AgentSetupHostExt for FakeHost {}
