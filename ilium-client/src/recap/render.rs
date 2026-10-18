//! Renders a `RecapBatch` as Markdown for people or as JSONL records for
//! scripts. Both read the same typed result; neither calls a model.

use serde_json::{json, Value};

use super::model::{PaneRecap, RecapLength, RecapPaneState, RecapPointer, RecapText};
use super::RecapBatch;

fn age_label(pane: &PaneRecap) -> String {
    match pane.facts.evidence_age_seconds {
        Some(seconds) if pane.facts.evidence_stale => format!("{seconds} s old (stale)"),
        Some(seconds) => format!("{seconds} s old"),
        None => "no transcript age".to_string(),
    }
}

fn state_sentence(pane: &PaneRecap) -> String {
    let reason = pane.failure.as_deref().unwrap_or("no reason recorded");
    match pane.state {
        RecapPaneState::Summarized => String::new(),
        RecapPaneState::Idle => format!("Idle: {reason}."),
        RecapPaneState::Denied => format!("Denied: {reason}. Nothing was read."),
        RecapPaneState::Unavailable => format!("Unavailable: {reason}."),
        RecapPaneState::Stale => format!("Stale: {reason}. Text dropped."),
        RecapPaneState::Failed => format!("Failed: {reason}."),
    }
}

fn pointer_line(pointer: &RecapPointer) -> String {
    if pointer.note.is_empty() {
        format!("- `{}`", pointer.path)
    } else {
        format!("- `{}`: {}", pointer.path, pointer.note)
    }
}

fn text_block(heading: &str, text: &RecapText) -> String {
    let mut block = format!("#### {heading}\n\n{}\n", text.text);
    if !text.pointers.is_empty() {
        block.push('\n');
        for pointer in &text.pointers {
            block.push_str(&pointer_line(pointer));
            block.push('\n');
        }
    }
    block
}

fn pane_markdown(pane: &PaneRecap, lengths: &[RecapLength]) -> String {
    let mut section = format!(
        "### {} (`{}`)\n\nState: {}. Evidence: {}.\n",
        pane.pane_name,
        pane.pane_id,
        pane.state.name(),
        age_label(pane)
    );
    let goal_lines = pane.facts.goal_lines();
    if !goal_lines.is_empty() {
        section.push('\n');
        for line in &goal_lines {
            section.push_str(&format!("- {line}\n"));
        }
    }
    let sentence = state_sentence(pane);
    if !sentence.is_empty() {
        section.push_str(&format!("\n{sentence}\n"));
    }
    if pane.state == RecapPaneState::Summarized || pane.state == RecapPaneState::Failed {
        for length in lengths {
            let text = match length {
                RecapLength::Short => pane.short.as_ref().map(|text| ("Short", text)),
                RecapLength::Long => pane.long.as_ref().map(|text| ("Long", text)),
            };
            if let Some((heading, text)) = text {
                section.push('\n');
                section.push_str(&text_block(heading, text));
            }
        }
    }
    section
}

/// Markdown report: one section per pane, in input order.
pub fn render_markdown(batch: &RecapBatch, lengths: &[RecapLength]) -> String {
    let mut output = String::new();
    for pane in &batch.panes {
        output.push_str(&pane_markdown(pane, lengths));
        output.push('\n');
    }
    if batch.cancelled {
        output.push_str("Cancelled: results above may be partial.\n");
    }
    output
}

fn text_value(text: Option<&RecapText>, lengths: &[RecapLength], length: RecapLength) -> Value {
    match text {
        Some(text) if lengths.contains(&length) => json!({
            "text": text.text,
            "pointers": text.pointers.iter().map(|pointer| json!({
                "path": pointer.path,
                "note": pointer.note,
            })).collect::<Vec<_>>(),
            "rejected_pointers": text.rejected_pointers,
        }),
        _ => Value::Null,
    }
}

/// JSONL records: one `recap` per pane, a `warning` for each failed or stale
/// pane, and one final `summary`.
pub fn render_records(batch: &RecapBatch, lengths: &[RecapLength]) -> Vec<Value> {
    let mut records = Vec::new();
    let mut counts = [0usize; 6];
    for pane in &batch.panes {
        let index = match pane.state {
            RecapPaneState::Summarized => 0,
            RecapPaneState::Idle => 1,
            RecapPaneState::Denied => 2,
            RecapPaneState::Unavailable => 3,
            RecapPaneState::Stale => 4,
            RecapPaneState::Failed => 5,
        };
        counts[index] += 1;
        records.push(json!({
            "type": "recap",
            "pane_id": pane.pane_id,
            "pane_name": pane.pane_name,
            "state": pane.state.name(),
            "goals": pane.facts.goal_lines(),
            "evidence_age_seconds": pane.facts.evidence_age_seconds,
            "evidence_stale": pane.facts.evidence_stale,
            "window_truncated": pane.facts.window_truncated,
            "short": text_value(pane.short.as_ref(), lengths, RecapLength::Short),
            "long": text_value(pane.long.as_ref(), lengths, RecapLength::Long),
            "failure": pane.failure,
        }));
        if matches!(pane.state, RecapPaneState::Failed | RecapPaneState::Stale) {
            records.push(json!({
                "type": "warning",
                "pane_id": pane.pane_id,
                "state": pane.state.name(),
                "message": pane.failure,
            }));
        }
    }
    records.push(json!({
        "type": "summary",
        "panes": batch.panes.len(),
        "summarized": counts[0],
        "idle": counts[1],
        "denied": counts[2],
        "unavailable": counts[3],
        "stale": counts[4],
        "failed": counts[5],
        "cancelled": batch.cancelled,
    }));
    records
}
