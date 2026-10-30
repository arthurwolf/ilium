//! Fake settings (reference-test-harness.md "Fake storage and settings").

use std::collections::BTreeMap;

use ilium_plugin_sdk_core::generated::types::{
    Event, PluginSide, SettingsChange, SettingsOrigin,
};
use ilium_plugin_sdk_core::types::{ErrorCode, PluginError};
use serde_json::Value;

use super::tree::Outbox;

/// Longest text value a setting accepts (SET-024).
const MAX_TEXT_CHARS: usize = 4096;

/// Range of `ui.last_prompt_max_lines` (reference-config-and-data-migration.md "[ui] toolbar").
const MIN_PROMPT_LINES: i64 = 1;
const MAX_PROMPT_LINES: i64 = 20;

/// What kind of value a setting row takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    Bool,
    Number,
    Text,
    Any,
}

impl ValueKind {
    fn of(value: &Value) -> ValueKind {
        match value {
            Value::Bool(_) => ValueKind::Bool,
            Value::Number(_) => ValueKind::Number,
            Value::String(_) => ValueKind::Text,
            _ => ValueKind::Any,
        }
    }

    /// Kind of a manifest row type such as "toggle" or "number".
    pub(crate) fn of_row_type(row_type: &str) -> ValueKind {
        match row_type {
            "toggle" => ValueKind::Bool,
            "number" | "slider" | "stepper" => ValueKind::Number,
            "text" | "select" | "choice" | "color" | "path" => ValueKind::Text,
            _ => ValueKind::Any,
        }
    }
}

/// Handle on the fake settings of a host.
#[derive(Debug, Default)]
pub struct FakeSettings {
    values: BTreeMap<String, Value>,
    kinds: BTreeMap<String, ValueKind>,
    infos: BTreeMap<String, String>,
    writes: Vec<(String, Value)>,
    side: Option<PluginSide>,
    outbox: Outbox,
}

impl FakeSettings {
    /// Settings that publish `settings-changed` into `outbox`.
    pub(crate) fn with_outbox(outbox: Outbox) -> FakeSettings {
        FakeSettings {
            outbox,
            ..FakeSettings::default()
        }
    }

    /// Declares a row with its default value and kind (from the manifest).
    pub(crate) fn declare(&mut self, key: &str, kind: ValueKind, default: Option<Value>) {
        self.kinds.insert(key.to_string(), kind);
        if let Some(default) = default {
            self.values.entry(key.to_string()).or_insert(default);
        }
    }

    /// Declares the kernel keys that first-party plugins read (`[ui]` toolbar keys and
    /// `remote_compaction.enabled`) with their defaults; the values are kept if already present.
    pub(crate) fn declare_kernel_keys(&mut self) {
        for key in ["ui.last_prompt_enabled", "ui.agent_toolbar_enabled", "ui.show_toolbar_labels"] {
            self.declare(key, ValueKind::Bool, Some(Value::Bool(true)));
        }
        self.declare("ui.last_prompt_max_lines", ValueKind::Number, Some(Value::from(4)));
        self.declare("remote_compaction.enabled", ValueKind::Bool, Some(Value::Bool(false)));
    }

    /// Side that owns the keys in `settings-changed`.
    pub(crate) fn set_side(&mut self, side: PluginSide) {
        self.side = Some(side);
    }

    /// Records the info text of a row.
    pub(crate) fn set_info(&mut self, key: &str, text: &str) {
        self.infos.insert(key.to_string(), text.to_string());
    }

    fn expected_kind(&self, key: &str) -> ValueKind {
        if let Some(kind) = self.kinds.get(key) {
            return *kind;
        }
        if let Some(existing) = self.values.get(key) {
            return ValueKind::of(existing);
        }
        let lower = key.to_ascii_lowercase();
        let numeric_suffixes = ["count", "limit", "size", "width", "height", "seconds", "millis"];
        if numeric_suffixes.iter().any(|suffix| lower.ends_with(suffix)) {
            return ValueKind::Number;
        }
        ValueKind::Any
    }

    /// Changes a setting, validates it and delivers `settings-changed` (DEV-467).
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), PluginError> {
        if let Value::String(text) = &value {
            if text.chars().count() > MAX_TEXT_CHARS {
                return Err(PluginError::new(
                    ErrorCode::InvalidArgument,
                    format!("value of {key} is longer than {MAX_TEXT_CHARS} characters"),
                ));
            }
        }
        let expected = self.expected_kind(key);
        if expected != ValueKind::Any && ValueKind::of(&value) != expected {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                format!("value of {key} has the wrong type"),
            ));
        }
        if key == "ui.last_prompt_max_lines" {
            let in_range = value
                .as_i64()
                .is_some_and(|lines| (MIN_PROMPT_LINES..=MAX_PROMPT_LINES).contains(&lines));
            if !in_range {
                return Err(PluginError::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{key} = {value} must be between {MIN_PROMPT_LINES} and {MAX_PROMPT_LINES}"
                    ),
                ));
            }
        }
        self.values.insert(key.to_string(), value.clone());
        self.writes.push((key.to_string(), value));
        self.outbox
            .borrow_mut()
            .push(Event::SettingsChanged(SettingsChange {
                keys: vec![key.to_string()],
                origin: SettingsOrigin::User,
                side: self.side.unwrap_or(PluginSide::Server),
            }));
        Ok(())
    }

    /// Reads a setting.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.values.get(key).cloned()
    }

    /// The info text the plugin set on a settings row (lane hook, see manual_gaps_fakes.md; DEV-467).
    pub fn info(&self, key: &str) -> Option<String> {
        self.infos.get(key).cloned()
    }

    /// The stored value of one top-level settings section (lane hook, see manual_gaps_fakes.md; DEV-467).
    pub fn section_value(&self, key: &str) -> Option<Value> {
        if let Some(value) = self.values.get(key) {
            return Some(value.clone());
        }
        let prefix = format!("{key}.");
        let members: serde_json::Map<String, Value> = self
            .values
            .iter()
            .filter_map(|(name, value)| {
                name.strip_prefix(&prefix)
                    .map(|rest| (rest.to_string(), value.clone()))
            })
            .collect();
        if members.is_empty() {
            None
        } else {
            Some(Value::Object(members))
        }
    }

    /// Every `(key, value)` the plugin wrote through `settings.set`, in order (lane hook, see manual_gaps_fakes.md).
    pub fn writes(&self) -> Vec<(String, Value)> {
        self.writes.clone()
    }

    /// Replaces the stored settings object with this JSON text, without validation (lane hook, see manual_gaps_fakes.md).
    pub fn seed(&mut self, json: &str) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) {
            self.values = map.into_iter().collect();
        }
    }

    /// The stored settings object as JSON text, for `Activation.settings_json`.
    pub(crate) fn as_json(&self) -> String {
        let map: serde_json::Map<String, Value> = self
            .values
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Value::Object(map).to_string()
    }
}

#[cfg(test)]
mod spec_tests {
    use crate::host::FakeHost;
    use ilium_plugin_sdk_core::types::ErrorCode;
    use serde_json::json;

    /// REQ: DEV-467
    #[test]
    fn set_then_get_returns_the_written_value() {
        let mut host = FakeHost::new();
        host.settings()
            .set("theme", json!("dark"))
            .expect("a valid value is accepted");
        assert_eq!(host.settings().get("theme"), Some(json!("dark")));
    }

    /// REQ: DEV-467
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

    /// REQ: DEV-467
    #[test]
    fn invalid_value_returns_invalid_argument() {
        let mut host = FakeHost::new();
        let outcome = host.settings().set("count", json!("not a number"));
        let error = outcome.expect_err("the schema refuses this value");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: DEV-467
    #[test]
    fn refused_value_is_neither_stored_nor_recorded() {
        let mut host = FakeHost::new();
        let _ = host.settings().set("count", json!("not a number"));
        assert_eq!(host.settings().get("count"), None);
        assert!(host.settings().writes().is_empty());
    }

    /// REQ: DEV-467
    #[test]
    fn get_of_a_never_set_key_returns_none() {
        let mut host = FakeHost::new();
        assert_eq!(host.settings().get("never-written"), None);
    }

    /// REQ: SET-024
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
