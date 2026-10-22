//! Lenient reading of the optional `attention` list in an LLM reply.
//!
//! The list rides along with the restructure reply, whose strict plan type
//! rejects unknown fields. So the key is split off the raw JSON first
//! ([`take_attention_value`]) and parsed separately here: a missing, malformed
//! or partly wrong list degrades to fewer items and never fails the sort.

use std::collections::{HashMap, HashSet};

use ilium_core::NodeId;
use serde_json::Value;

use super::item::{
    AttentionItem, AttentionKind, AttentionSource, Briefing, MAXIMUM_BRIEFING_CHARACTERS,
};

/// Longest reason and question kept from a reply, in characters.
pub const MAXIMUM_REASON_CHARACTERS: usize = 200;
pub const MAXIMUM_QUESTION_CHARACTERS: usize = 400;
/// JSON key carrying the list in the combined reply.
pub const ATTENTION_KEY: &str = "attention";

/// What the caller knows about one pane the call analysed.
#[derive(Debug, Clone)]
pub struct KnownPane {
    pub project_id: Option<NodeId>,
    pub title: String,
    pub fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttention {
    pub items: Vec<AttentionItem>,
    /// Entries dropped for being malformed, unknown, or duplicated.
    pub skipped: usize,
}

/// Removes and returns the attention value so the strict plan parse never sees it.
pub fn take_attention_value(reply: &mut Value) -> Option<Value> {
    reply.as_object_mut()?.remove(ATTENTION_KEY)
}

/// Parses the (already split off) attention value. `None` means the reply had
/// no list, which is normal and yields no items.
pub fn parse_attention(
    value: Option<&Value>,
    known: &HashMap<NodeId, KnownPane>,
    generation: u64,
    now_ms: u64,
) -> ParsedAttention {
    let mut parsed = ParsedAttention {
        items: Vec::new(),
        skipped: 0,
    };
    let Some(entries) = value.and_then(Value::as_array) else {
        return parsed;
    };
    let mut seen = HashSet::new();
    for entry in entries.iter().take(known.len().max(1) * 2) {
        match parse_entry(entry, known, generation, now_ms) {
            Some(item) if seen.insert(item.pane_id) => parsed.items.push(item),
            _ => parsed.skipped += 1,
        }
    }
    parsed
}

fn parse_entry(
    entry: &Value,
    known: &HashMap<NodeId, KnownPane>,
    generation: u64,
    now_ms: u64,
) -> Option<AttentionItem> {
    let object = entry.as_object()?;
    let pane_id = node_id(object.get("id")?)?;
    let pane = known.get(&pane_id)?;
    let kind = AttentionKind::parse(object.get("kind")?.as_str()?)?;
    let reason = clipped(object.get("reason")?.as_str()?, MAXIMUM_REASON_CHARACTERS)?;
    let question = object
        .get("question")
        .and_then(Value::as_str)
        .and_then(|text| clipped(text, MAXIMUM_QUESTION_CHARACTERS));
    // A waiting-on target must be a different pane the call also knew about.
    let waiting_on = object
        .get("waiting_on")
        .and_then(node_id)
        .filter(|target| *target != pane_id && known.contains_key(target));
    let briefing = parse_briefing(object);
    let priority = object
        .get("priority")
        .and_then(Value::as_u64)
        .map_or(50, |value| value.min(100) as u8);
    Some(AttentionItem {
        pane_id,
        project_id: pane.project_id,
        title: pane.title.clone(),
        kind,
        reason,
        question,
        waiting_on,
        briefing,
        priority,
        source: AttentionSource::Llm,
        fingerprint: pane.fingerprint,
        generation,
        created_ms: now_ms,
    })
}

/// Reads the three briefing fields. A missing, non-string, blank or overlong
/// field is dropped or truncated; it never fails the entry.
fn parse_briefing(object: &serde_json::Map<String, Value>) -> Briefing {
    let field = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .and_then(|text| clipped(text, MAXIMUM_BRIEFING_CHARACTERS))
    };
    let mut briefing = Briefing {
        status_summary: field("status_summary"),
        why_here: field("why_here"),
        suggested_action: field("suggested_action"),
        from_llm: false,
    };
    briefing.from_llm = !briefing.is_empty();
    briefing
}

/// Models write ids as numbers or numeric strings; accept both.
fn node_id(value: &Value) -> Option<NodeId> {
    match value {
        Value::Number(number) => number.as_u64().map(NodeId),
        Value::String(text) => text.trim().parse().ok().map(NodeId),
        _ => None,
    }
}

fn clipped(text: &str, limit: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn known() -> HashMap<NodeId, KnownPane> {
        [1u64, 2, 3]
            .into_iter()
            .map(|id| {
                (
                    NodeId(id),
                    KnownPane {
                        project_id: Some(NodeId(10)),
                        title: format!("p{id}"),
                        fingerprint: 100 + id,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn missing_list_yields_nothing_and_no_skips() {
        let parsed = parse_attention(None, &known(), 1, 0);
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.skipped, 0);
        assert!(parse_attention(Some(&json!("oops")), &known(), 1, 0)
            .items
            .is_empty());
        assert!(parse_attention(Some(&json!({"a": 1})), &known(), 1, 0)
            .items
            .is_empty());
    }

    #[test]
    fn take_removes_the_key_so_strict_parse_sees_no_unknown_field() {
        let mut reply = json!({"children": [], "attention": [{"id": 1}]});
        assert!(take_attention_value(&mut reply).is_some());
        assert_eq!(reply, json!({"children": []}));
        assert!(take_attention_value(&mut json!([1])).is_none());
    }

    #[test]
    fn valid_entry_carries_known_pane_facts() {
        let value = json!([{"id": 2, "kind": "Question", "reason": "needs a db choice",
            "question": " Postgres or SQLite? ", "waiting_on": "3", "priority": 250}]);
        let parsed = parse_attention(Some(&value), &known(), 7, 99);
        assert_eq!(parsed.skipped, 0);
        let item = &parsed.items[0];
        assert_eq!(
            (item.pane_id, item.kind, item.priority),
            (NodeId(2), AttentionKind::Question, 100)
        );
        assert_eq!(item.question.as_deref(), Some("Postgres or SQLite?"));
        assert_eq!(item.waiting_on, Some(NodeId(3)));
        assert_eq!(
            (item.fingerprint, item.generation, item.created_ms),
            (102, 7, 99)
        );
        assert_eq!(
            (item.title.as_str(), item.project_id),
            ("p2", Some(NodeId(10)))
        );
    }

    #[test]
    fn bad_entries_are_skipped_and_good_ones_survive() {
        let value = json!([
            {"id": 99, "kind": "stuck", "reason": "unknown pane"},
            {"id": 1, "kind": "nonsense", "reason": "bad kind"},
            {"id": 1, "kind": "stuck", "reason": ""},
            "not an object",
            {"id": 1, "kind": "stuck", "reason": "retrying the same test"},
            {"id": 3, "kind": "changed", "reason": "done", "waiting_on": 3},
            {"id": 1, "kind": "changed", "reason": "duplicate id"}
        ]);
        let parsed = parse_attention(Some(&value), &known(), 1, 0);
        let ids: Vec<u64> = parsed.items.iter().map(|item| item.pane_id.0).collect();
        assert_eq!(ids, vec![1, 3]);
        assert_eq!(
            parsed.skipped, 4,
            "the duplicate lies past the examined bound"
        );
        // Self and unknown waiting-on targets are dropped, not trusted.
        assert_eq!(parsed.items[1].waiting_on, None);
        assert_eq!(parsed.items[0].priority, 50);
    }

    #[test]
    fn long_text_is_clipped_and_list_length_is_bounded() {
        let long = "x".repeat(5_000);
        let value = json!([{"id": 1, "kind": "stuck", "reason": long, "question": long}]);
        let item = &parse_attention(Some(&value), &known(), 1, 0).items[0];
        assert_eq!(item.reason.chars().count(), MAXIMUM_REASON_CHARACTERS);
        assert_eq!(
            item.question.as_ref().unwrap().chars().count(),
            MAXIMUM_QUESTION_CHARACTERS
        );
        let flood: Vec<Value> = (0..1_000)
            .map(|_| json!({"id": 1, "kind": "stuck", "reason": "r"}))
            .collect();
        let parsed = parse_attention(Some(&Value::Array(flood)), &known(), 1, 0);
        assert_eq!(parsed.items.len() + parsed.skipped, 6);
    }

    #[test]
    fn briefing_fields_are_read_trimmed_and_flagged_as_llm_wording() {
        let value = json!([{"id": 1, "kind": "stuck", "reason": "loops",
            "status_summary": " Re-running the same failing test. ",
            "why_here": "It has failed four times.", "suggested_action": "Check the test"}]);
        let item = &parse_attention(Some(&value), &known(), 1, 0).items[0];
        assert_eq!(
            item.briefing.status_summary.as_deref(),
            Some("Re-running the same failing test.")
        );
        assert_eq!(
            item.briefing.why_here.as_deref(),
            Some("It has failed four times.")
        );
        assert_eq!(
            item.briefing.suggested_action.as_deref(),
            Some("Check the test")
        );
        assert!(item.briefing.from_llm);
    }

    #[test]
    fn missing_or_malformed_briefing_fields_never_fail_the_entry() {
        let value = json!([
            {"id": 1, "kind": "stuck", "reason": "r"},
            {"id": 2, "kind": "stuck", "reason": "r", "status_summary": 7,
                "why_here": ["x"], "suggested_action": "   "},
            {"id": 3, "kind": "stuck", "reason": "r", "why_here": "only this"}
        ]);
        let parsed = parse_attention(Some(&value), &known(), 1, 0);
        assert_eq!(parsed.skipped, 0);
        assert!(parsed.items[0].briefing.is_empty());
        assert!(!parsed.items[0].briefing.from_llm);
        assert!(parsed.items[1].briefing.is_empty());
        assert!(!parsed.items[1].briefing.from_llm);
        assert_eq!(
            parsed.items[2].briefing.why_here.as_deref(),
            Some("only this")
        );
        assert!(parsed.items[2].briefing.from_llm);
        assert_eq!(parsed.items[2].briefing.status_summary, None);
    }

    #[test]
    fn overlong_briefing_fields_are_truncated() {
        let long = "y".repeat(5_000);
        let value = json!([{"id": 1, "kind": "stuck", "reason": "r",
            "status_summary": long, "why_here": long, "suggested_action": long}]);
        let item = &parse_attention(Some(&value), &known(), 1, 0).items[0];
        for text in [
            &item.briefing.status_summary,
            &item.briefing.why_here,
            &item.briefing.suggested_action,
        ] {
            assert_eq!(
                text.as_ref().unwrap().chars().count(),
                MAXIMUM_BRIEFING_CHARACTERS
            );
        }
    }
}
