//! Claude Code writer: appends a `compact_boundary` system record, the
//! summary user record and the verbatim tail as freshly chained records.
//! The boundary form was verified to resume correctly against the real CLI.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::input::result_lookup;
use crate::neutral::{ClaudeHead, Part, Role, Turn};
use crate::writer_common::{iso_timestamp, read_appended_records};

pub(crate) struct ClaudePlan<'a> {
    pub(crate) head: &'a ClaudeHead,
    pub(crate) session_id: &'a str,
    /// Summary text, without the wrapper.
    pub(crate) summary: &'a str,
    pub(crate) tail: &'a [Turn],
    pub(crate) pre_tokens: u64,
    pub(crate) post_tokens: u64,
    pub(crate) duration_ms: u64,
}

fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// Builds records with the shared template fields and a strictly increasing
/// timestamp, chained to the previous record.
struct RecordBuilder<'a> {
    template: &'a Map<String, Value>,
    session_id: &'a str,
    parent: Option<String>,
    emitted: i64,
    records: Vec<Value>,
}

impl RecordBuilder<'_> {
    fn base(&mut self) -> Map<String, Value> {
        let mut record = self.template.clone();
        record.insert("sessionId".into(), json!(self.session_id));
        record.insert("timestamp".into(), json!(iso_timestamp(self.emitted)));
        self.emitted += 1;
        record
    }

    /// Adds a record chained after the previous one; returns its uuid.
    fn push(&mut self, mut record: Map<String, Value>) -> String {
        let uuid = new_uuid();
        record.insert("parentUuid".into(), json!(self.parent));
        record.insert("uuid".into(), json!(uuid));
        self.parent = Some(uuid.clone());
        self.records.push(Value::Object(record));
        uuid
    }

    fn user_record(&mut self, content: Value) {
        let mut record = self.base();
        record.insert("type".into(), json!("user"));
        record.insert(
            "message".into(),
            json!({"role": "user", "content": content}),
        );
        self.push(record);
    }

    fn assistant_record(
        &mut self,
        message_id: &str,
        model: &str,
        block: Value,
        stop_reason: Value,
    ) {
        let mut record = self.base();
        record.insert("type".into(), json!("assistant"));
        record.insert(
            "message".into(),
            json!({
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [block],
                "stop_reason": stop_reason,
                "stop_sequence": null,
            }),
        );
        self.push(record);
    }
}

fn tool_input(arguments: &Value) -> Value {
    match arguments {
        Value::Object(_) => arguments.clone(),
        Value::Null => json!({}),
        other => json!({ "input": other }),
    }
}

fn user_content(turn: &Turn) -> Value {
    let pieces: Vec<&str> = turn
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) | Part::Omitted(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    json!(pieces.join("\n\n"))
}

/// Records of one summarized-and-compacted transcript tail: boundary,
/// summary and the re-emitted verbatim tail.
pub(crate) fn build_claude_records(plan: &ClaudePlan<'_>) -> Vec<Value> {
    let mut builder = RecordBuilder {
        template: &plan.head.template,
        session_id: plan.session_id,
        parent: None,
        emitted: 0,
        records: Vec::new(),
    };

    let mut boundary = builder.base();
    boundary.insert("type".into(), json!("system"));
    boundary.insert("subtype".into(), json!("compact_boundary"));
    boundary.insert("content".into(), json!("Conversation compacted"));
    boundary.insert("isMeta".into(), json!(false));
    boundary.insert("level".into(), json!("info"));
    boundary.insert("logicalParentUuid".into(), json!(plan.head.leaf_uuid));
    boundary.insert(
        "compactMetadata".into(),
        json!({
            "trigger": "manual",
            "preTokens": plan.pre_tokens,
            "postTokens": plan.post_tokens,
            "durationMs": plan.duration_ms,
        }),
    );
    builder.push(boundary);

    let wrapped = ilium_prompts::render_value(
        "compaction/claude-summary-wrapper",
        &json!({
            "summary": plan.summary,
            "tail_kept": plan.tail.iter().any(|turn| turn.role != Role::ToolResults),
        }),
    );
    let mut summary = builder.base();
    summary.insert("type".into(), json!("user"));
    summary.insert("isSidechain".into(), json!(false));
    summary.insert(
        "message".into(),
        json!({"role": "user", "content": wrapped.trim_end()}),
    );
    summary.insert("isVisibleInTranscriptOnly".into(), json!(true));
    summary.insert("isCompactSummary".into(), json!(true));
    builder.push(summary);

    let results = result_lookup(plan.tail);
    let missing_text = ilium_prompts::conversion::MISSING_RESULT.trim();
    let empty_text = ilium_prompts::conversion::EMPTY_RESULT.trim();
    for turn in plan.tail {
        match turn.role {
            Role::User => builder.user_record(user_content(turn)),
            Role::Assistant => {
                let message_id = format!("msg_{}", Uuid::new_v4().simple());
                let model = turn
                    .model
                    .as_deref()
                    .or(plan.head.model.as_deref())
                    .unwrap_or("claude");
                let blocks: Vec<Value> = turn
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        Part::Text(text) => Some(json!({"type": "text", "text": text})),
                        Part::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": tool_input(arguments),
                        })),
                        _ => None,
                    })
                    .collect();
                let last_index = blocks.len().saturating_sub(1);
                for (index, block) in blocks.into_iter().enumerate() {
                    let stop_reason = if index < last_index {
                        Value::Null
                    } else if block["type"] == "tool_use" {
                        json!("tool_use")
                    } else {
                        json!("end_turn")
                    };
                    builder.assistant_record(&message_id, model, block, stop_reason);
                }
                // Every tool_use is answered immediately, in call order.
                for part in &turn.parts {
                    let Part::ToolCall { id, .. } = part else {
                        continue;
                    };
                    let (text, is_error) = match results.get(id.as_str()) {
                        Some(Part::ToolResult { text, is_error, .. }) => (
                            if text.is_empty() {
                                empty_text
                            } else {
                                text.as_str()
                            },
                            *is_error,
                        ),
                        _ => (missing_text, true),
                    };
                    let mut block =
                        json!({"type": "tool_result", "tool_use_id": id, "content": text});
                    if is_error {
                        block["is_error"] = json!(true);
                    }
                    builder.user_record(json!([block]));
                }
            }
            Role::ToolResults | Role::EarlierSummary => {}
        }
    }
    builder.records
}

/// Re-reads the rewritten transcript and checks the structure Claude Code
/// relies on when resuming. `expected_new` is the number of appended records.
pub(crate) fn verify_claude(
    path: &Path,
    appended_offset: u64,
    appended_bytes: &[u8],
    expected_new: usize,
    session_id: &str,
) -> Result<(), String> {
    let values = read_appended_records(path, appended_offset, expected_new, appended_bytes)?;
    let boundary_index = 0;
    if values[boundary_index]
        .get("subtype")
        .and_then(Value::as_str)
        != Some("compact_boundary")
    {
        return Err("the compact boundary record is missing".to_string());
    }
    let by_uuid: HashMap<&str, usize> = values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            value
                .get("uuid")
                .and_then(Value::as_str)
                .map(|uuid| (uuid, index))
        })
        .collect();

    let mut cursor = values.len() - 1;
    let mut visited = HashSet::new();
    let mut chain = Vec::new();
    loop {
        if !visited.insert(cursor) {
            return Err("the parent chain contains a cycle".to_string());
        }
        chain.push(cursor);
        if cursor == boundary_index {
            break;
        }
        let parent = values[cursor]
            .get("parentUuid")
            .and_then(Value::as_str)
            .ok_or("a record in the new chain has no parentUuid")?;
        cursor = *by_uuid
            .get(parent)
            .ok_or("a record in the new chain points at a missing parent")?;
        if cursor < boundary_index {
            return Err("the new chain does not run through the boundary".to_string());
        }
    }
    if chain.len() != expected_new {
        return Err(format!(
            "the parent chain holds {} of {expected_new} appended records",
            chain.len()
        ));
    }
    if !values[boundary_index]["parentUuid"].is_null() {
        return Err("the boundary record has a parentUuid".to_string());
    }

    let mut used = HashSet::new();
    let mut answered = HashSet::new();
    for &index in &chain {
        let value = &values[index];
        if value.get("sessionId").and_then(Value::as_str) != Some(session_id) {
            return Err("a new record carries a different session id".to_string());
        }
        let blocks = value
            .pointer("/message/content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    used.extend(block.get("id").and_then(Value::as_str));
                }
                Some("tool_result") => {
                    answered.extend(block.get("tool_use_id").and_then(Value::as_str));
                }
                _ => {}
            }
        }
    }
    if used != answered {
        return Err(
            "a tool_use has no tool_result (or the reverse) in the new records".to_string(),
        );
    }
    Ok(())
}
