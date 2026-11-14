//! The in-pane recap line: the wire format between the two sides of the plugin and the text the
//! client shows in the footer of an agent pane.
//!
//! The client asks for one pane's recap with a `request` message holding the pane id as decimal
//! text. The server answers with a `result` message holding a JSON document (`DotRecap`), and
//! keeps a summarized result in the node metadata of the pane so a restarted client shows it
//! again. A stored recap is shown only while the pane still holds the agent session it was
//! computed for.

use serde_json::{json, Value};

/// The client-to-server channel asking for the recap of one pane.
pub const REQUEST_CHANNEL: &str = "request";
/// The server-to-client channel carrying one `DotRecap`.
pub const RESULT_CHANNEL: &str = "result";
/// The footer-row slot that carries the recap line.
pub const SLOT: &str = "recap";
/// The command that asks for a fresh recap of a pane.
pub const COMMAND: &str = "recap";
/// The footer line shows at most this many characters of the short recap.
pub const LINE_CHARS: usize = 160;
/// The state name of a recap that has text.
pub const SUMMARIZED: &str = "summarized";

/// The outcome of one in-pane recap run for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotRecap {
    pub pane: u64,
    /// The agent session the text was computed for; `None` for a shell pane.
    pub session: Option<String>,
    /// The state name of the pane record (`summarized`, `idle`, `denied`, `failed`, ...).
    pub state: String,
    pub short: Option<String>,
    pub long: Option<String>,
    pub failure: Option<String>,
}

/// A pane id sent as plain decimal text; anything else is refused.
pub fn parse_pane(payload: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(payload).ok()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn record_text(record: &Value, length: &str) -> Option<String> {
    record
        .get(length)?
        .get("text")?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl DotRecap {
    /// A run that produced no text, with the reason.
    pub fn failed(pane: u64, state: &str, reason: &str) -> Self {
        Self {
            pane,
            session: None,
            state: state.to_string(),
            short: None,
            long: None,
            failure: Some(reason.to_string()),
        }
    }

    /// Builds the result from the `recap` record the job produced for the pane.
    pub fn from_record(pane: u64, session: Option<String>, record: &Value) -> Self {
        let state = record
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string();
        Self {
            pane,
            session,
            state,
            short: record_text(record, "short"),
            long: record_text(record, "long"),
            failure: record
                .get("failure")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }

    /// True when the result carries text worth keeping.
    pub fn has_text(&self) -> bool {
        self.state == SUMMARIZED && (self.short.is_some() || self.long.is_some())
    }

    /// True while the pane still holds the session the text was computed for.
    pub fn is_current(&self, session_now: Option<&str>) -> bool {
        self.session.as_deref() == session_now
    }

    /// The JSON document sent between the sides and kept in node metadata.
    pub fn to_payload(&self) -> Vec<u8> {
        json!({
            "pane": self.pane,
            "session": self.session,
            "state": self.state,
            "short": self.short,
            "long": self.long,
            "failure": self.failure,
        })
        .to_string()
        .into_bytes()
    }

    /// Reads a document written by `to_payload`; anything else is refused.
    pub fn from_payload(bytes: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            pane: value.get("pane")?.as_u64()?,
            session: text("session"),
            state: text("state")?,
            short: text("short"),
            long: text("long"),
            failure: text("failure"),
        })
    }

    /// The text of the footer line: the short recap, or the reason there is none.
    pub fn line(&self) -> String {
        let body = match (&self.short, &self.long) {
            (Some(short), _) if self.state == SUMMARIZED => short.clone(),
            (None, Some(long)) if self.state == SUMMARIZED => long.clone(),
            _ => format!(
                "Recap unavailable: {}",
                self.failure.as_deref().unwrap_or(self.state.as_str())
            ),
        };
        format!(" {} ", clip(&body, LINE_CHARS))
    }

    /// The hover text of the footer line: the long recap when there is one.
    pub fn tooltip(&self) -> Option<String> {
        if !self.has_text() {
            return None;
        }
        self.long.clone().or_else(|| self.short.clone())
    }
}

/// The footer line while a recap is being written.
pub fn working_line() -> String {
    " Recap: working... ".to_string()
}

/// `text` on one line, cut to `limit` characters with an ellipsis when it is longer.
pub fn clip(text: &str, limit: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let mut cut: String = flat.chars().take(limit.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summarized() -> DotRecap {
        DotRecap::from_record(
            7,
            Some("session-a".to_string()),
            &json!({
                "state": "summarized",
                "short": {"text": "  Fixing token expiry in auth.  "},
                "long": {"text": "The agent is fixing token expiry in the auth module."},
                "failure": null,
            }),
        )
    }

    #[test]
    fn pane_ids_are_plain_decimal_text() {
        assert_eq!(parse_pane(b"12"), Some(12));
        assert_eq!(parse_pane(b""), None);
        assert_eq!(parse_pane(b"-1"), None);
        assert_eq!(parse_pane(b"1 2"), None);
        assert_eq!(parse_pane(&[0xff, 0xfe]), None);
    }

    #[test]
    fn a_record_becomes_trimmed_text_with_the_session() {
        let recap = summarized();
        assert_eq!(recap.pane, 7);
        assert_eq!(recap.session.as_deref(), Some("session-a"));
        assert_eq!(recap.short.as_deref(), Some("Fixing token expiry in auth."));
        assert!(recap.has_text());
    }

    #[test]
    fn the_payload_round_trips_and_refuses_garbage() {
        let recap = summarized();
        assert_eq!(DotRecap::from_payload(&recap.to_payload()), Some(recap));
        assert_eq!(DotRecap::from_payload(b"not json"), None);
        assert_eq!(DotRecap::from_payload(b"{\"pane\": 1}"), None);
        assert_eq!(DotRecap::from_payload(b"{\"state\": \"failed\"}"), None);
    }

    #[test]
    fn a_stored_recap_is_current_only_for_its_own_session() {
        let recap = summarized();
        assert!(recap.is_current(Some("session-a")));
        assert!(!recap.is_current(Some("session-b")));
        assert!(!recap.is_current(None));
        let shell = DotRecap::failed(3, "idle", "no agent");
        assert!(shell.is_current(None));
    }

    #[test]
    fn the_line_shows_the_short_text_or_the_reason() {
        assert_eq!(summarized().line(), " Fixing token expiry in auth. ");
        let denied = DotRecap::failed(3, "denied", "AI features are off");
        assert_eq!(denied.line(), " Recap unavailable: AI features are off ");
        assert_eq!(denied.tooltip(), None);
        assert_eq!(
            summarized().tooltip().as_deref(),
            Some("The agent is fixing token expiry in the auth module.")
        );
    }

    #[test]
    fn long_text_is_cut_on_one_line_with_an_ellipsis() {
        let clipped = clip("one\n two   three", 8);
        assert_eq!(clipped, "one two\u{2026}");
        assert_eq!(clip("short", 8), "short");
        assert_eq!(clip("anything", 0), "\u{2026}");
    }

    #[test]
    fn text_less_summaries_fall_back_to_the_reason() {
        let empty = DotRecap::from_record(1, None, &json!({"state": "summarized"}));
        assert!(!empty.has_text());
        assert_eq!(empty.line(), " Recap unavailable: summarized ");
    }
}
