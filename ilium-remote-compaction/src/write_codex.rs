//! Codex writer: appends a `compacted` record carrying the full replacement
//! history (summary message plus the verbatim user and assistant messages of
//! the tail) and a small `token_count` event so the next turn does not
//! compact again. Codex's SQLite files are never touched.

use std::path::Path;

use serde_json::{json, Value};

use crate::error::CompactionError;
use crate::neutral::{CodexTail, Role, Turn};
use crate::tokens::estimate_tokens;
use crate::writer_common::{iso_timestamp, read_appended_records};

pub(crate) struct CodexPlan<'a> {
    pub(crate) transcript_path: &'a Path,
    pub(crate) rollout: &'a CodexTail,
    /// Cleaned summary text.
    pub(crate) summary: &'a str,
    /// Tool activity of the tail, appended to the summary message.
    pub(crate) tail_activity: Option<&'a str>,
    pub(crate) tail: &'a [Turn],
}

pub(crate) struct CodexRecords {
    pub(crate) records: Vec<Value>,
    /// Estimated tokens of the whole replacement history.
    pub(crate) history_tokens: u64,
    pub(crate) has_token_count: bool,
}

fn summary_prefix() -> &'static str {
    ilium_prompts::compaction::CODEX_SUMMARY_PREFIX.trim()
}

/// The text of the summary message: the fixed prefix, a newline, the summary.
pub(crate) fn summary_message(summary: &str, tail_activity: Option<&str>) -> String {
    let mut message = format!("{}\n{}", summary_prefix(), summary.trim());
    if let Some(activity) = tail_activity.filter(|activity| !activity.trim().is_empty()) {
        message.push_str("\n\n");
        message.push_str(activity.trim());
    }
    message
}

fn message_item(role: &str, content_type: &str, text: &str) -> Value {
    json!({"type": "message", "role": role, "content": [{"type": content_type, "text": text}]})
}

/// Zeroes every number of a usage object and sets `total_tokens`.
fn usage_with_total(template: Option<&Value>, total: u64) -> Value {
    let mut usage = match template {
        Some(Value::Object(map)) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let replaced = if value.is_number() {
                        json!(0)
                    } else {
                        value.clone()
                    };
                    (key.clone(), replaced)
                })
                .collect(),
        ),
        _ => json!({}),
    };
    usage["total_tokens"] = json!(total);
    usage
}

pub(crate) fn build_codex_records(plan: &CodexPlan<'_>) -> Result<CodexRecords, CompactionError> {
    let message = summary_message(plan.summary, plan.tail_activity);
    let mut history = vec![message_item("user", "input_text", &message)];
    let mut history_tokens = estimate_tokens(&message);
    for turn in plan.tail {
        let text = turn.text();
        if text.trim().is_empty() {
            continue;
        }
        history_tokens += estimate_tokens(&text);
        match turn.role {
            Role::User => history.push(message_item("user", "input_text", &text)),
            Role::Assistant => history.push(message_item("assistant", "output_text", &text)),
            Role::ToolResults | Role::EarlierSummary => {}
        }
    }

    let mut next_ordinal = if plan.rollout.has_ordinals {
        let last = plan
            .rollout
            .last_ordinal
            .ok_or_else(|| CompactionError::MissingOrdinal {
                path: plan.transcript_path.to_path_buf(),
            })?;
        Some(last + 1)
    } else {
        None
    };
    let mut stamp = |mut record: Value, offset: i64| {
        record["timestamp"] = json!(iso_timestamp(offset));
        if let Some(ordinal) = next_ordinal.as_mut() {
            record["ordinal"] = json!(*ordinal);
            *ordinal += 1;
        }
        record
    };

    let mut records = vec![stamp(
        json!({
            "type": "compacted",
            "payload": {"message": message, "replacement_history": history},
        }),
        0,
    )];
    let has_token_count = plan.rollout.token_count_template.is_some();
    if let Some(template) = &plan.rollout.token_count_template {
        let mut event = template.clone();
        let usage = usage_with_total(
            event.pointer("/payload/info/last_token_usage"),
            history_tokens,
        );
        if let Some(info) = event.pointer_mut("/payload/info") {
            info["last_token_usage"] = usage;
        }
        records.push(stamp(event, 1));
    }
    Ok(CodexRecords {
        records,
        history_tokens,
        has_token_count,
    })
}

/// Re-reads the rewritten rollout and checks the structure Codex relies on.
pub(crate) fn verify_codex(
    path: &Path,
    appended_offset: u64,
    appended_bytes: &[u8],
    expected_new: usize,
    has_ordinals: bool,
    previous_ordinal: Option<u64>,
) -> Result<(), String> {
    let appended = read_appended_records(path, appended_offset, expected_new, appended_bytes)?;
    let compacted = &appended[0];
    if compacted.get("type").and_then(Value::as_str) != Some("compacted") {
        return Err("the compacted record is not where it was appended".to_string());
    }
    let first_text = compacted
        .pointer("/payload/replacement_history/0/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !first_text.starts_with(summary_prefix()) {
        return Err("the replacement history does not start with the summary message".to_string());
    }
    if has_ordinals {
        let mut expected = previous_ordinal.map(|ordinal| ordinal + 1);
        for record in appended {
            let ordinal = record
                .get("ordinal")
                .and_then(Value::as_u64)
                .ok_or("an appended record has no ordinal")?;
            if expected.is_some_and(|wanted| wanted != ordinal) {
                return Err(format!("ordinal {ordinal} breaks the contiguous sequence"));
            }
            expected = Some(ordinal + 1);
        }
    }
    Ok(())
}
