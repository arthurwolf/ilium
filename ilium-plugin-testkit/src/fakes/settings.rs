//! Fake settings (reference-test-harness.md "Fake storage and settings").

use ilium_plugin_sdk_core::types::PluginError;

/// Handle on the fake settings of a host.
#[derive(Debug)]
pub struct FakeSettings {
    _private: (),
}

impl FakeSettings {
    /// Changes a setting, validates it and delivers `settings-changed` (DEV-467).
    pub fn set(&mut self, _key: &str, _value: serde_json::Value) -> Result<(), PluginError> {
        unimplemented!("DEV-467")
    }

    /// Reads a setting.
    pub fn get(&self, _key: &str) -> Option<serde_json::Value> {
        unimplemented!("DEV-467")
    }
}
