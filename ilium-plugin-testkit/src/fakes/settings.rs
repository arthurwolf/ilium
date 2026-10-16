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

    /// The info text the plugin set on a settings row (lane hook, see manual_gaps_fakes.md; DEV-467).
    pub fn info(&self, _key: &str) -> Option<String> {
        unimplemented!("DEV-467")
    }

    /// The stored value of one top-level settings section (lane hook, see manual_gaps_fakes.md; DEV-467).
    pub fn section_value(&self, _key: &str) -> Option<serde_json::Value> {
        unimplemented!("DEV-467")
    }

    /// Every `(key, value)` the plugin wrote through `settings.set`, in order (lane hook, see manual_gaps_fakes.md).
    pub fn writes(&self) -> Vec<(String, serde_json::Value)> {
        unimplemented!("DEV-467")
    }

    /// Replaces the stored settings object with this JSON text, without validation (lane hook, see manual_gaps_fakes.md).
    pub fn seed(&mut self, _json: &str) {
        unimplemented!("DEV-467")
    }
}
