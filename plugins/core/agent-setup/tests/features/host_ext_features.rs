//! Agent-setup hooks on `FakeHost` that only the feature test target uses (commands, help files, field keys).
//!
//! Skeleton bodies. Shared hooks live in `tests/host_ext.rs`.

use ilium_plugin_sdk::testing::FakeHost;

/// Feature-target hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait AgentSetupFeaturesHostExt {
    /// Runs a registered command by id (agent-setup "Commands").
    fn invoke_command(&mut self, _command: &str) {
        unimplemented!("agent-setup.invoke-command")
    }
    /// Row ids of the contributed tab, in order.
    fn kernel_tab_row_ids(&self, _surface: &str) -> Vec<String> {
        unimplemented!("agent-setup.kernel-tab-row-ids")
    }
    /// Sends a key to a text field of the contributed tab.
    fn press_field_key(&mut self, _surface: &str, _field: &str, _key: char) {
        unimplemented!("agent-setup.press-field-key")
    }
    /// Provides a help file the plugin ships.
    fn provide_help_file(&mut self, _path: &str, _text: &str) {
        unimplemented!("agent-setup.provide-help-file")
    }
}

impl AgentSetupFeaturesHostExt for FakeHost {}
