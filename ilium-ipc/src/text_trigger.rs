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
        }
    }
}

/// The `[text_triggers]` TOML table. An empty list is inert by default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TextTriggerSettings {
    pub triggers: Vec<TextTrigger>,
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
            }],
        };
        let encoded = bincode::serialize(&settings).expect("serialize settings");
        let decoded: TextTriggerSettings =
            bincode::deserialize(&encoded).expect("deserialize settings");
        assert_eq!(decoded, settings);
    }
}
