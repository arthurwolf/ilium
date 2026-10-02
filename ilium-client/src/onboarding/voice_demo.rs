//! Capability-limited executor for the onboarding Realtime session. It has no
//! App, IPC or terminal references: even an unexpected provider tool call
//! cannot operate a real pane.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const TOOL_NAME: &str = "set_lightbulb";
const MAX_CALLS: usize = 1024;
const MAX_ARGUMENT_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BulbReceipt {
    pub ok: bool,
    pub on: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BulbArguments {
    on: bool,
}

#[derive(Debug, Default)]
pub struct VoiceDemo {
    pub on: bool,
    receipts: HashMap<String, BulbReceipt>,
}

impl VoiceDemo {
    pub fn execute(&mut self, call_id: &str, name: &str, arguments: &str) -> BulbReceipt {
        // Duplicate provider delivery receives the original acknowledgement,
        // even if another call has changed the bulb in the meantime.
        if let Some(receipt) = self.receipts.get(call_id) {
            return receipt.clone();
        }
        if call_id.is_empty() || call_id.len() > 128 {
            return self.failure("Invalid tool call identity");
        }
        if self.receipts.len() >= MAX_CALLS {
            return self.failure("Demo call limit reached; start a new test");
        }
        let receipt = if name != TOOL_NAME {
            self.failure("This demo only supports the lightbulb tool")
        } else if arguments.len() > MAX_ARGUMENT_BYTES {
            self.failure("Lightbulb arguments are too large")
        } else {
            match serde_json::from_str::<BulbArguments>(arguments) {
                Ok(arguments) => {
                    self.on = arguments.on;
                    BulbReceipt {
                        ok: true,
                        on: self.on,
                        error: None,
                    }
                }
                Err(_) => self.failure("Expected an object containing only a boolean 'on'"),
            }
        };
        self.receipts.insert(call_id.to_owned(), receipt.clone());
        receipt
    }

    fn failure(&self, message: &str) -> BulbReceipt {
        BulbReceipt {
            ok: false,
            on: self.on,
            error: Some(message.to_owned()),
        }
    }
}

pub fn tool_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {"on": {"type": "boolean", "description": "Whether the lightbulb is on"}},
        "required": ["on"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_tool_execution_turns_bulb_on_and_off() {
        let mut demo = VoiceDemo::default();
        assert!(demo.execute("first", TOOL_NAME, r#"{"on":true}"#).ok);
        assert!(demo.on);
        assert!(demo.execute("second", TOOL_NAME, r#"{"on":false}"#).ok);
        assert!(!demo.on);
    }

    #[test]
    fn duplicate_on_after_off_returns_original_receipt_without_relighting() {
        let mut demo = VoiceDemo::default();
        let receipt = demo.execute("first", TOOL_NAME, r#"{"on":true}"#);
        demo.execute("second", TOOL_NAME, r#"{"on":false}"#);
        assert_eq!(demo.execute("first", TOOL_NAME, r#"{"on":true}"#), receipt);
        assert!(!demo.on);
    }

    #[test]
    fn real_controls_and_malformed_arguments_are_rejected_without_state_changes() {
        let mut demo = VoiceDemo::default();
        for (index, (name, arguments)) in [
            ("create_pane", "{}"),
            (TOOL_NAME, "{}"),
            (TOOL_NAME, r#"{"on":"true"}"#),
            (TOOL_NAME, r#"{"on":true,"pane_id":1}"#),
        ]
        .iter()
        .enumerate()
        {
            assert!(!demo.execute(&index.to_string(), name, arguments).ok);
            assert!(!demo.on);
        }
    }

    #[test]
    fn exhausted_demo_retains_replay_receipts_and_refuses_new_mutations() {
        let mut demo = VoiceDemo::default();
        for index in 0..MAX_CALLS {
            assert!(
                demo.execute(&index.to_string(), TOOL_NAME, r#"{"on":false}"#)
                    .ok
            );
        }
        assert!(!demo.execute("overflow", TOOL_NAME, r#"{"on":true}"#).ok);
        assert!(demo.execute("0", TOOL_NAME, r#"{"on":false}"#).ok);
        assert!(!demo.on);
    }
}
