//! Bounded, sanitized evidence for advisory polling classification.
//!
//! Raw tool output and arbitrary command arguments can contain whole files or
//! credentials. Summarize those locally before constructing outbound evidence.

use ilium_agent_session::polling_activity::normalized_call_shape;
use ilium_core::polling_detection::{ActivityEvent, ActivityKind};
use serde_json::{json, Value};
use std::collections::HashMap;

#[path = "polling_prompt_runs.rs"]
mod runs;

/// Build valid JSON within the byte budget, retaining the newest complete
/// events. An impossibly small budget returns `None` rather than broken JSON.
pub fn compact_evidence(
    events: &[ActivityEvent],
    api_key: &str,
    max_bytes: usize,
) -> Option<String> {
    // Opaque identifiers preserve equality within this window without sending
    // command arguments or provider call identifiers to the classifier.
    let mut targets: HashMap<(Option<&str>, String), usize> = HashMap::new();
    let mut calls: HashMap<&str, usize> = HashMap::new();
    let mut event_targets = Vec::with_capacity(events.len());
    let mut previous_results: HashMap<usize, (&str, Option<bool>)> = HashMap::new();
    let mut result_changes = Vec::with_capacity(events.len());
    for event in events {
        let target = match event.kind {
            ActivityKind::ToolCall if !event.text.is_empty() && !event.truncated => {
                let next = targets.len() + 1;
                let target = *targets
                    .entry((event.name.as_deref(), normalized_call_shape(event)))
                    .or_insert(next);
                if let Some(call_id) = event.call_id.as_deref() {
                    calls.insert(call_id, target);
                }
                Some(target)
            }
            ActivityKind::ToolResult => event
                .call_id
                .as_deref()
                .and_then(|id| calls.get(id).copied()),
            _ => None,
        };
        event_targets.push(target);
        let changed = if event.kind == ActivityKind::ToolResult {
            target.and_then(|target| {
                if event.truncated || event.text.is_empty() {
                    previous_results.remove(&target);
                    return None;
                }
                previous_results
                    .insert(target, (&event.text, event.is_error))
                    .and_then(|(previous, previous_error)| {
                        if previous != event.text.as_str() {
                            return Some(true);
                        }
                        match (previous_error, event.is_error) {
                            (Some(left), Some(right)) => Some(left != right),
                            (None, None) => Some(false),
                            _ => None,
                        }
                    })
            })
        } else {
            None
        };
        result_changes.push(changed);
    }
    let mut selected = Vec::new();
    for ((event, target), changed) in events.iter().zip(event_targets).zip(result_changes).rev() {
        let text = match event.kind {
            ActivityKind::Message => sanitize(&event.text, api_key),
            ActivityKind::ToolCall => command_shape(event).to_owned(),
            ActivityKind::ToolResult => result_state(event).to_owned(),
        };
        let kind = match event.kind {
            ActivityKind::Message => "message",
            ActivityKind::ToolCall => "tool_call",
            ActivityKind::ToolResult => "tool_result",
        };
        let item = json!({"kind": kind, "timestamp": event.timestamp.as_deref().map(|s| sanitize(s, api_key)),
            "tool": event.name.as_deref().map(|s| sanitize(s, api_key)), "target": target, "text": text,
            "requested_wait_ms": requested_wait_ms(event), "changed_since_last": changed, "truncated": event.truncated});
        selected.push(item);
        if serialize(&selected, events.len() - selected.len()).len() > max_bytes {
            selected.pop();
            break;
        }
    }
    let output = serialize(&selected, events.len() - selected.len());
    (output.len() <= max_bytes).then_some(output)
}

fn requested_wait_ms(event: &ActivityEvent) -> Option<u64> {
    if event.kind != ActivityKind::ToolCall || event.truncated {
        return None;
    }
    let name = event.name.as_deref().unwrap_or("");
    let operation = name
        .rsplit("__")
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or(name);
    if !matches!(operation, "write_stdin" | "wait_agent" | "wait") {
        return None;
    }
    let arguments: Value = serde_json::from_str(&event.text).ok()?;
    arguments
        .get("yield_time_ms")
        .or_else(|| arguments.get("timeout_ms"))?
        .as_u64()
}

fn serialize(reversed: &[Value], omitted: usize) -> String {
    let ordered: Vec<_> = reversed.iter().rev().cloned().collect();
    json!({"events": runs::compress(ordered), "omitted_events": omitted}).to_string()
}

fn sanitize(value: &str, api_key: &str) -> String {
    let cleaned = ilium_logging::redact_secrets(value).text;
    let cleaned = if api_key.is_empty() {
        cleaned
    } else {
        cleaned.replace(api_key, "[redacted]")
    };
    cleaned.chars().take(200).collect()
}

fn command_shape(event: &ActivityEvent) -> &'static str {
    let name = event.name.as_deref().unwrap_or("");
    // Structured wait tools are identifiable without inspecting private arguments.
    if matches!(
        name.rsplit("__")
            .next()
            .unwrap_or(name)
            .rsplit('.')
            .next()
            .unwrap_or(name),
        "write_stdin" | "wait_agent" | "wait"
    ) {
        return "wait or continuation";
    }
    let arguments: Option<Value> = serde_json::from_str(&event.text).ok();
    let command = match &arguments {
        Some(arguments) => arguments
            .get("cmd")
            .or_else(|| arguments.get("command"))
            .and_then(Value::as_str),
        None => Some(event.text.as_str()),
    };
    let Some(command) = command else {
        return "other tool call";
    };
    // Only recognize a simple command's actual operation. Script bodies,
    // pipelines and quoted commentary cannot establish a status query.
    if command.contains([';', '|', '&', '\n', '`', '$', '\'', '"']) {
        return "other tool call";
    }
    let words: Vec<_> = command.split_whitespace().take(3).collect();
    let executable = words
        .first()
        .and_then(|word| word.rsplit('/').next())
        .unwrap_or("");
    match (executable, words.get(1).copied(), words.get(2).copied()) {
        ("ilium", Some("progress"), Some("wait"))
        | ("wait_agent" | "write_stdin" | "wait", _, _) => "wait or continuation",
        ("ilium", Some("progress"), Some("status")) | ("status", _, _) => "possible status check",
        _ => "other tool call",
    }
}

fn result_state(event: &ActivityEvent) -> &'static str {
    if event.is_error == Some(true) {
        return "error";
    }
    let lower = event.text.to_ascii_lowercase();
    if lower.contains("not completed")
        || lower.contains("not pending")
        || lower.contains("not running")
        || lower.contains("no longer running")
    {
        "unknown"
    } else if lower.contains("completed") {
        "completed"
    } else if lower.contains("running") || lower.contains("pending") {
        "pending"
    } else {
        "unknown"
    }
}
