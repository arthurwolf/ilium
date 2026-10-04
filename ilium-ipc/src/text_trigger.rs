//! Durable, transport-safe text-trigger configuration shared by the client
//! settings UI and the detached server. Regex compilation deliberately stays
//! server/client-side: this crate carries data only and has no runtime I/O or
//! regex-engine dependency.

use serde::{Deserialize, Serialize};

/// Which terminal identities a text trigger may observe.
///
/// `Terminals` means a terminal currently classified as a plain shell;
/// detected agents are intentionally separate so a broad terminal rule does
/// not silently send prompts into an agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextTriggerTarget {
    Agents,
    Terminals,
    #[default]
    Both,
}

impl TextTriggerTarget {
    pub const ALL: [Self; 3] = [Self::Agents, Self::Terminals, Self::Both];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Terminals => "Terminals",
            Self::Both => "Both",
        }
    }
}

/// Seconds between detecting a trigger match and sending its message when the
/// rule does not say otherwise. Rules stored before the delay existed carry no
/// `delay_seconds` key and load with this value too.
pub const DEFAULT_TEXT_TRIGGER_DELAY_SECONDS: u32 = 60;

/// Longest accepted delay (24 hours). Bounds the server's pending-delivery
/// timers; `0` sends immediately.
pub const MAX_TEXT_TRIGGER_DELAY_SECONDS: u32 = 24 * 60 * 60;

/// One ordered rule. Each instance of text matching `regexp` on the pane's
/// screen attempts one `message` submission, however that text later scrolls,
/// repaints or moves. An instance is identified by its matched text; it is
/// released for reuse once it has stayed off the screen for a settle window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextTrigger {
    /// Stable rule identity for editing, execution diagnostics, and future
    /// clients. The client assigns an RFC 4122 UUID when creating a rule.
    pub id: String,
    /// Disabled rules remain retained and previewable but never execute.
    pub enabled: bool,
    pub regexp: String,
    pub message: String,
    pub target: TextTriggerTarget,
    /// Draft/sample text retained with the rule so its preview remains useful
    /// when the user returns to edit it.
    pub sample_text: String,
    /// Whole seconds to wait between detecting a match and sending `message`.
    /// A missing key (rules authored before this setting) deserializes to
    /// [`DEFAULT_TEXT_TRIGGER_DELAY_SECONDS`] through the struct default.
    pub delay_seconds: u32,
}

impl Default for TextTrigger {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            regexp: String::new(),
            message: String::new(),
            target: TextTriggerTarget::Both,
            sample_text: String::new(),
            delay_seconds: DEFAULT_TEXT_TRIGGER_DELAY_SECONDS,
        }
    }
}

/// The `[text_triggers]` TOML table. An empty list is inert by default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TextTriggerSettings {
    pub triggers: Vec<TextTrigger>,
}

impl TextTriggerSettings {
    /// Checks authored identities without normalizing them. Non-UUID IDs are
    /// valid; absent, blank, control-bearing, and duplicate IDs are not.
    /// Invalid documents stay on disk for their author to repair.
    pub fn validate_identities(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for (index, trigger) in self.triggers.iter().enumerate() {
            if trigger.id.trim().is_empty() || trigger.id.chars().any(char::is_control) {
                return Err(format!(
                    "Text Trigger {} has an invalid stable id",
                    index + 1
                ));
            }
            if !ids.insert(&trigger.id) {
                return Err(format!("Text Trigger {} repeats an existing id", index + 1));
            }
            if trigger.delay_seconds > MAX_TEXT_TRIGGER_DELAY_SECONDS {
                return Err(format!(
                    "Text Trigger {} delay exceeds {MAX_TEXT_TRIGGER_DELAY_SECONDS} seconds",
                    index + 1
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_rule_order_and_target() {
        let settings = TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "71860ee0-7f3d-45cc-8512-7c4ed6feee72".to_string(),
                enabled: true,
                regexp: "ready$".to_string(),
                message: "continue".to_string(),
                target: TextTriggerTarget::Agents,
                sample_text: "ready".to_string(),
                delay_seconds: 15,
            }],
        };
        let encoded = bincode::serialize(&settings).expect("serialize settings");
        let decoded: TextTriggerSettings =
            bincode::deserialize(&encoded).expect("deserialize settings");
        assert_eq!(decoded, settings);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn settings(id: &str) -> TextTriggerSettings {
        TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: id.to_owned(),
                ..TextTrigger::default()
            }],
        }
    }

    #[test]
    fn identity_validation_preserves_authored_non_uuid_ids() {
        let value = settings("authored-rule");
        let before = value.clone();
        assert_eq!(value.validate_identities(), Ok(()));
        assert_eq!(value, before);
        assert_eq!(TextTriggerSettings::default().validate_identities(), Ok(()));
    }

    #[test]
    fn identity_validation_rejects_missing_blank_control_and_duplicate_ids() {
        for id in ["", "   ", "rule\nname", "\t"] {
            assert!(settings(id).validate_identities().is_err());
        }
        let mut value = settings("stable");
        value.triggers.push(value.triggers[0].clone());
        assert!(value.validate_identities().is_err());
    }
}

#[cfg(test)]
mod delay_tests {
    use super::*;

    #[test]
    fn rules_stored_before_the_delay_existed_load_with_the_default() {
        let legacy = r#"
            triggers = [
                { id = "old", enabled = true, regexp = "ready", message = "go", target = "agents", sample_text = "" },
            ]
        "#;
        let settings: TextTriggerSettings = toml::from_str(legacy).expect("legacy rule parses");
        assert_eq!(settings.triggers[0].delay_seconds, 60);
        assert_eq!(TextTrigger::default().delay_seconds, 60);
    }

    #[test]
    fn explicit_delay_round_trips_through_toml_including_zero() {
        for delay in [0, 1, 90, MAX_TEXT_TRIGGER_DELAY_SECONDS] {
            let settings = TextTriggerSettings {
                triggers: vec![TextTrigger {
                    id: "r".into(),
                    delay_seconds: delay,
                    ..TextTrigger::default()
                }],
            };
            let text = toml::to_string(&settings).expect("serialize");
            let back: TextTriggerSettings = toml::from_str(&text).expect("parse");
            assert_eq!(back.triggers[0].delay_seconds, delay);
        }
    }

    #[test]
    fn validation_rejects_delays_beyond_the_maximum() {
        let settings = TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "r".into(),
                delay_seconds: MAX_TEXT_TRIGGER_DELAY_SECONDS + 1,
                ..TextTrigger::default()
            }],
        };
        assert!(settings.validate_identities().is_err());
    }
}
