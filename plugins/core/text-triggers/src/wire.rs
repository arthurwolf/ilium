//! Messages between the two sides of the text-triggers plugin, and the test-rule preview.
//!
//! The client side shows and edits the rules; the server side owns them (it reads and writes the
//! `text-triggers` kernel settings tab). Every payload is a small JSON object; a malformed payload
//! yields `None`.
//!
//! The preview helpers moved from `ilium-client/src/text_trigger_dialog/preview.rs`.

use regex::Regex;
use serde_json::{json, Value};

use crate::config::Rule;

/// Client to server: send the rules to the asking client. Empty payload.
pub const STATE_REQUEST_CHANNEL: &str = "state-request";
/// Server to client: the rules and the pending delivery count, see [`state_payload`].
pub const STATE_CHANNEL: &str = "state";
/// Client to server: add or replace one rule. A [`Rule::to_json`] object.
pub const SAVE_RULE_CHANNEL: &str = "save-rule";
/// Client to server: remove one rule. `{"id"}`.
pub const DELETE_RULE_CHANNEL: &str = "delete-rule";
/// Client to server: switch one rule on or off. `{"id", "enabled"}`.
pub const TOGGLE_RULE_CHANNEL: &str = "toggle-rule";
/// Server to client: a refusal to show the user. `{"text"}`.
pub const NOTICE_CHANNEL: &str = "notice";

/// Most characters of a sample the preview scans (1 MiB of text is cut, not refused).
pub const MAX_SAMPLE_CHARS: usize = 1024 * 1024;

/// What the client shows: the rules and how many replies wait for their delay.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub rules: Vec<Rule>,
    pub pending: usize,
}

pub fn state_payload(rules: &[Rule], pending: usize) -> Vec<u8> {
    json!({
        "rules": rules.iter().map(Rule::to_json).collect::<Vec<_>>(),
        "pending": pending,
    })
    .to_string()
    .into_bytes()
}

pub fn decode_state(payload: &[u8]) -> Option<Shown> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    let rules = value
        .get("rules")?
        .as_array()?
        .iter()
        .map(|item| Rule::from_json(item).ok())
        .collect::<Option<Vec<_>>>()?;
    let pending = usize::try_from(value.get("pending")?.as_u64()?).ok()?;
    Some(Shown { rules, pending })
}

pub fn rule_payload(rule: &Rule) -> Vec<u8> {
    rule.to_json().to_string().into_bytes()
}

pub fn rule_from_payload(payload: &[u8]) -> Option<Rule> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    Rule::from_json(&value).ok()
}

pub fn id_payload(id: &str) -> Vec<u8> {
    json!({ "id": id }).to_string().into_bytes()
}

pub fn id_from_payload(payload: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    Some(value.get("id")?.as_str()?.to_owned())
}

pub fn toggle_payload(id: &str, enabled: bool) -> Vec<u8> {
    json!({ "id": id, "enabled": enabled })
        .to_string()
        .into_bytes()
}

pub fn toggle_from_payload(payload: &[u8]) -> Option<(String, bool)> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    Some((
        value.get("id")?.as_str()?.to_owned(),
        value.get("enabled")?.as_bool()?,
    ))
}

pub fn notice_payload(text: &str) -> Vec<u8> {
    json!({ "text": text }).to_string().into_bytes()
}

pub fn notice_from_payload(payload: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    Some(value.get("text")?.as_str()?.to_owned())
}

/// The preview of one rule against a sample: one line per sample line, plus warnings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preview {
    pub lines: Vec<String>,
    pub issues: Vec<String>,
}

/// Marks every match of `pattern` in each line of `sample` (`✓ a[bb]a` for a match, `· xxx` for a
/// line without one) and warns when the reply would match the pattern itself, which can loop.
pub fn preview(pattern: &str, reply: &str, sample: &str) -> Result<Preview, String> {
    let regex = Regex::new(pattern).map_err(|error| format!("Invalid regexp: {error}"))?;
    let mut result = Preview::default();
    let cut: String;
    let sample = if sample.chars().count() > MAX_SAMPLE_CHARS {
        cut = sample.chars().take(MAX_SAMPLE_CHARS).collect();
        result.issues.push("capture-limit".to_owned());
        cut.as_str()
    } else {
        sample
    };
    for line in sample.lines() {
        let mut marked = String::new();
        let mut last = 0;
        let mut zero_width = false;
        let mut matched = false;
        for found in regex.find_iter(line) {
            matched = true;
            if found.as_str().is_empty() {
                zero_width = true;
                continue;
            }
            marked.push_str(&line[last..found.start()]);
            marked.push('[');
            marked.push_str(found.as_str());
            marked.push(']');
            last = found.end();
        }
        marked.push_str(&line[last..]);
        result.lines.push(match (matched, zero_width) {
            (false, _) => format!("· {line}"),
            (true, true) if last == 0 => format!("✓ {line}  ⟪zero-width match⟫"),
            (true, _) => format!("✓ {marked}"),
        });
    }
    if !reply.is_empty() && regex.is_match(reply) {
        result
            .lines
            .push("⚠ Reply also matches this regexp; echoed input can loop.".to_owned());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str) -> Rule {
        Rule {
            id: id.to_owned(),
            regexp: "go".to_owned(),
            message: "yes".to_owned(),
            ..Rule::default()
        }
    }

    #[test]
    fn state_round_trips() {
        let payload = state_payload(&[rule("a"), rule("b")], 3);
        let shown = decode_state(&payload).expect("decodes");
        assert_eq!(shown.rules, vec![rule("a"), rule("b")]);
        assert_eq!(shown.pending, 3);
        assert_eq!(decode_state(b"nope"), None);
    }

    #[test]
    fn small_payloads_round_trip() {
        assert_eq!(
            rule_from_payload(&rule_payload(&rule("a"))),
            Some(rule("a"))
        );
        assert_eq!(id_from_payload(&id_payload("a")), Some("a".to_owned()));
        assert_eq!(
            toggle_from_payload(&toggle_payload("a", false)),
            Some(("a".to_owned(), false))
        );
        assert_eq!(
            notice_from_payload(&notice_payload("x")),
            Some("x".to_owned())
        );
    }

    #[test]
    fn preview_brackets_matches_and_dots_misses() {
        let preview = preview("b+", "", "abba\nxxx").expect("valid");
        assert_eq!(preview.lines, vec!["✓ a[bb]a", "· xxx"]);
    }

    #[test]
    fn preview_marks_zero_width_matches() {
        let preview = preview("^", "", "alpha").expect("valid");
        assert_eq!(preview.lines, vec!["✓ alpha  ⟪zero-width match⟫"]);
    }

    #[test]
    fn preview_warns_when_the_reply_matches() {
        let preview = preview("go", "go on", "go").expect("valid");
        assert!(preview
            .lines
            .last()
            .is_some_and(|line| line.starts_with('⚠')));
    }

    #[test]
    fn preview_rejects_an_invalid_pattern() {
        assert!(preview("(", "", "x").is_err());
    }

    #[test]
    fn preview_cuts_an_oversized_sample() {
        let sample = "a".repeat(MAX_SAMPLE_CHARS + 5);
        let preview = preview("a", "", &sample).expect("valid");
        assert_eq!(preview.issues, vec!["capture-limit".to_owned()]);
    }
}
