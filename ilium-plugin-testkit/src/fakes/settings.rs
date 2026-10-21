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

#[cfg(test)]
mod spec_tests {
    use crate::host::FakeHost;
    use ilium_plugin_sdk_core::types::ErrorCode;
    use serde_json::json;

    /// REQ: DEV-467 (`host.settings().set(key, value)` validates the value and then the value reads back through `get`)
    #[test]
    fn set_then_get_returns_the_written_value() {
        let mut host = FakeHost::new();
        host.settings()
            .set("theme", json!("dark"))
            .expect("a valid value is accepted");
        assert_eq!(host.settings().get("theme"), Some(json!("dark")));
    }

    /// REQ: DEV-467 (`writes` lists every `settings.set` in order, the pairs that trigger `settings-changed`)
    #[test]
    fn writes_list_each_successful_set_in_call_order() {
        let mut host = FakeHost::new();
        host.settings().set("first", json!(1)).expect("valid");
        host.settings().set("second", json!(2)).expect("valid");
        assert_eq!(
            host.settings().writes(),
            vec![
                ("first".to_string(), json!(1)),
                ("second".to_string(), json!(2)),
            ]
        );
    }

    /// REQ: DEV-467 (an invalid value MUST return `Err(invalid-argument)`)
    #[test]
    fn invalid_value_returns_invalid_argument() {
        let mut host = FakeHost::new();
        let outcome = host.settings().set("count", json!("not a number"));
        let error = outcome.expect_err("the schema refuses this value");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: DEV-467 with SET-021 (the schema check runs before the write, so a refused value is neither stored nor recorded)
    #[test]
    fn refused_value_is_neither_stored_nor_recorded() {
        let mut host = FakeHost::new();
        let _ = host.settings().set("count", json!("not a number"));
        assert_eq!(host.settings().get("count"), None);
        assert!(host.settings().writes().is_empty());
    }

    /// REQ: DEV-467 (empty input: a key that was never set reads as absent, not as a default value)
    #[test]
    fn get_of_a_never_set_key_returns_none() {
        let mut host = FakeHost::new();
        assert_eq!(host.settings().get("never-written"), None);
    }

    /// REQ: SET-024 (a text value above 4,096 characters fails the save with invalid-argument)
    #[test]
    fn text_value_above_4096_characters_is_refused() {
        let mut host = FakeHost::new();
        let too_long = "x".repeat(4097);
        let error = host
            .settings()
            .set("label", json!(too_long))
            .expect_err("a text value over the limit is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(host.settings().get("label"), None);
    }
}
