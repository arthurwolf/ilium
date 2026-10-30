//! The kernel `[ui]` keys and `remote_compaction.enabled` this plugin reads (flat dotted keys).

use ilium_plugin_sdk::host;
use serde_json::Value;

/// The values the client side reads before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiSettings {
    pub last_prompt_enabled: bool,
    pub remote_compaction: bool,
}

fn flag(map: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(default)
}

impl UiSettings {
    /// Reads the live settings; a missing or unreadable object falls back to the defaults.
    pub(crate) fn read() -> UiSettings {
        let parsed: Option<Value> = serde_json::from_str(&host().get_json()).ok();
        let empty = serde_json::Map::new();
        let map = parsed.as_ref().and_then(Value::as_object).unwrap_or(&empty);
        UiSettings {
            last_prompt_enabled: flag(map, "ui.last_prompt_enabled", true),
            remote_compaction: flag(map, "remote_compaction.enabled", false),
        }
    }
}
