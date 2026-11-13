//! The `ilium.core.terminal/transfer` service: request parsing, reply shapes and the paste text
//! (moved from ilium-client/src/screen_transfer.rs, which held only the eligibility vocabulary;
//! the screen read and the paste are the plugin's own now).
//!
//! Everything here is pure. The server side (`lib.rs`) reads the screen through the terminal
//! primitive and submits the paste; this module decides what is asked and what is pasted.

use ilium_plugin_sdk::types::{Delivery, Intent, NodeId, SendInputSpec, Submission};
use serde_json::{json, Value};

/// The service name of the manifest (`provides.services`).
pub const SERVICE: &str = "ilium.core.terminal/transfer";
/// Rows read from the source screen (page sample: 500).
pub const SCREEN_MAX_ROWS: u32 = 500;
/// Most history bytes asked for, the newest kept (host limit 4 MiB).
pub const HISTORY_MAX_BYTES: u32 = 1 << 20;
/// Most transfers waiting for their history at one time.
pub const MAX_PENDING: usize = 16;

/// One call of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Paste the visible screen (and the history before it) of `source` into `target`.
    SendScreen { source: NodeId, target: NodeId },
    /// Answer the rows of the visible screen of `pane`.
    ReadScreen { pane: NodeId },
    /// Paste `text` into `pane`.
    SendText { pane: NodeId, text: String },
}

fn pane_field(value: &Value, name: &str) -> Result<NodeId, String> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .map(NodeId)
        .ok_or_else(|| format!("`{name}` must be a pane id (unsigned integer)"))
}

/// Parses the JSON payload of a call: `{"method": "send-screen", "source": 3, "target": 4}`,
/// `{"method": "read-screen", "pane": 3}` or `{"method": "send-text", "pane": 3, "text": "x"}`.
/// A plain `method` argument of the call itself is used when the payload has none.
pub fn from_payload(method_hint: &str, payload_json: &str) -> Result<Request, String> {
    let value: Value = serde_json::from_str(payload_json)
        .map_err(|error| format!("payload is not JSON: {error}"))?;
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or(method_hint);
    match method {
        "send-screen" => Ok(Request::SendScreen {
            source: pane_field(&value, "source")?,
            target: pane_field(&value, "target")?,
        }),
        "read-screen" => Ok(Request::ReadScreen {
            pane: pane_field(&value, "pane")?,
        }),
        "send-text" => Ok(Request::SendText {
            pane: pane_field(&value, "pane")?,
            text: value
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "`text` must be a string".to_string())?
                .to_string(),
        }),
        other => Err(format!("unknown method `{other}`")),
    }
}

/// The reply payload of a failed call.
pub fn error_payload(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// The reply payload of `read-screen`.
pub fn rows_payload(rows: &[String], revision: u64) -> String {
    json!({ "rows": rows, "revision": revision }).to_string()
}

/// The reply payload of `send-screen` and `send-text`: `queued` is true while the paste waits
/// for the history of the source pane.
pub fn sent_payload(queued: bool, lines: usize) -> String {
    json!({ "queued": queued, "lines": lines }).to_string()
}

/// The plain text of a screen: rows without trailing spaces, trailing blank rows dropped. Blank
/// rows between text rows stay. `None` when every row is blank.
pub fn visible_text(rows: &[String]) -> Option<String> {
    let trimmed: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
    let last = trimmed.iter().rposition(|row| !row.is_empty())?;
    Some(trimmed[..=last].join("\n"))
}

/// History first, then the visible screen, joined by one newline. Either part may be empty.
pub fn combine(history: &str, screen: Option<&str>) -> Option<String> {
    let history = history.trim_end();
    match (history.is_empty(), screen) {
        (true, None) => None,
        (true, Some(screen)) => Some(screen.to_string()),
        (false, None) => Some(history.to_string()),
        (false, Some(screen)) => Some(format!("{history}\n{screen}")),
    }
}

/// Dedup key of one transfer: the same screen revision between the same two panes pastes once.
pub fn dedup_key(source: NodeId, target: NodeId, revision: u64) -> String {
    format!("transfer-{}-{}-{revision}", source.0, target.0)
}

/// The paste into `target`: pasted delivery, no stages, no raw bytes, no Enter appended.
pub fn paste(target: NodeId, text: String) -> Intent {
    Intent::SendInput(SendInputSpec {
        pane: target,
        text,
        delivery: Delivery::Pasted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    })
}

/// The keyed submission of a screen transfer.
pub fn transfer_submission(
    source: NodeId,
    target: NodeId,
    revision: u64,
    text: String,
) -> Submission {
    Submission::keyed(paste(target, text), dedup_key(source, target, revision))
}

/// A transfer waiting for the history of its source pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The `history-text` request; its completion event names it.
    pub request: u64,
    pub source: NodeId,
    pub target: NodeId,
    pub revision: u64,
    /// The visible text read when the transfer started, `None` when it was blank.
    pub screen: Option<String>,
}

impl Pending {
    /// The submission once `history` is known (empty when the history could not be read).
    pub fn finish(&self, history: &str) -> Option<Submission> {
        let text = combine(history, self.screen.as_deref())?;
        Some(transfer_submission(
            self.source,
            self.target,
            self.revision,
            text,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn visible_text_drops_trailing_blank_rows_and_keeps_inner_ones() {
        assert_eq!(
            visible_text(&rows(&["one", "", "three", "  ", ""])),
            Some("one\n\nthree".to_string())
        );
        assert_eq!(visible_text(&rows(&["", "  "])), None);
        assert_eq!(visible_text(&[]), None);
    }

    #[test]
    fn visible_text_strips_row_padding() {
        assert_eq!(
            visible_text(&rows(&["$ ls   ", "a b  "])),
            Some("$ ls\na b".to_string())
        );
    }

    #[test]
    fn combine_puts_history_before_the_screen() {
        assert_eq!(
            combine("old 1\nold 2\n", Some("new")),
            Some("old 1\nold 2\nnew".to_string())
        );
        assert_eq!(combine("", Some("new")), Some("new".to_string()));
        assert_eq!(combine("old", None), Some("old".to_string()));
        assert_eq!(combine("", None), None);
    }

    #[test]
    fn requests_parse_from_the_payload() {
        assert_eq!(
            from_payload("call", r#"{"method":"send-screen","source":3,"target":4}"#),
            Ok(Request::SendScreen {
                source: NodeId(3),
                target: NodeId(4)
            })
        );
        assert_eq!(
            from_payload("read-screen", r#"{"pane":9}"#),
            Ok(Request::ReadScreen { pane: NodeId(9) })
        );
        assert_eq!(
            from_payload("call", r#"{"method":"send-text","pane":2,"text":"hi"}"#),
            Ok(Request::SendText {
                pane: NodeId(2),
                text: "hi".to_string()
            })
        );
    }

    #[test]
    fn malformed_requests_are_refused_with_a_reason() {
        assert!(from_payload("call", "not json")
            .unwrap_err()
            .contains("not JSON"));
        assert!(from_payload("call", r#"{"method":"nope"}"#)
            .unwrap_err()
            .contains("unknown method"));
        assert!(
            from_payload("call", r#"{"method":"send-screen","source":3}"#)
                .unwrap_err()
                .contains("target")
        );
        assert!(from_payload("call", r#"{"method":"send-text","pane":-1,"text":"x"}"#).is_err());
    }

    #[test]
    fn the_paste_is_pasted_without_enter_stages_or_raw_bytes() {
        let submission = transfer_submission(NodeId(3), NodeId(4), 42, "x".to_string());
        assert_eq!(submission.dedup_key.as_deref(), Some("transfer-3-4-42"));
        match submission.intent {
            Intent::SendInput(spec) => {
                assert_eq!(spec.pane, NodeId(4));
                assert_eq!(spec.delivery, Delivery::Pasted);
                assert_eq!(spec.text, "x");
                assert!(spec.stages.is_empty() && spec.raw.is_none() && spec.mouse.is_none());
            }
            other => panic!("unexpected intent {other:?}"),
        }
    }

    #[test]
    fn a_pending_transfer_finishes_with_history_then_screen() {
        let pending = Pending {
            request: 5,
            source: NodeId(3),
            target: NodeId(4),
            revision: 7,
            screen: Some("new line".to_string()),
        };
        let submission = pending
            .finish("old line 1\nold line 2")
            .expect("submission");
        assert_eq!(
            submission.intent,
            paste(NodeId(4), "old line 1\nold line 2\nnew line".to_string())
        );
        let blank = Pending {
            screen: None,
            ..pending
        };
        assert!(blank.finish("").is_none());
    }
}
