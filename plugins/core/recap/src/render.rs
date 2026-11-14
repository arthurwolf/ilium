//! Renders a `RecapBatch` as Markdown for people or as JSONL records for
//! scripts. Both read the same typed result; neither calls a model.

use serde_json::{json, Value};

use crate::model::{
    PaneRecap, PaneRecapFacts, RecapBatch, RecapLength, RecapPaneState, RecapPointer, RecapText,
};

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

fn pane_markdown(pane: &PaneRecap, goal_lines: &[String], lengths: &[RecapLength]) -> String {
    let mut section = format!(
        "### {} (`{}`)\n\nState: {}. Evidence: {}.\n",
        pane.pane_name,
        pane.pane_id,
        pane.state.name(),
        age_label(pane)
    );
    if !goal_lines.is_empty() {
        section.push('\n');
        for line in goal_lines {
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
    let entries: Vec<(PaneRecap, Vec<String>)> = batch
        .panes
        .iter()
        .map(|pane| (pane.clone(), pane.facts.goal_lines()))
        .collect();
    render_markdown_entries(&entries, batch.cancelled, lengths)
}

/// Markdown report of panes with their already rendered goal lines.
pub fn render_markdown_entries(
    entries: &[(PaneRecap, Vec<String>)],
    cancelled: bool,
    lengths: &[RecapLength],
) -> String {
    let mut output = String::new();
    for (pane, goal_lines) in entries {
        output.push_str(&pane_markdown(pane, goal_lines, lengths));
        output.push('\n');
    }
    if cancelled {
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

/// The `recap` record of one pane, as one JSON object.
pub fn pane_record(pane: &PaneRecap, lengths: &[RecapLength]) -> Value {
    json!({
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
    })
}

/// The `warning` record that accompanies a failed or stale pane, if it has one.
pub fn pane_warning(pane: &PaneRecap) -> Option<Value> {
    matches!(pane.state, RecapPaneState::Failed | RecapPaneState::Stale).then(|| {
        json!({
            "type": "warning",
            "pane_id": pane.pane_id,
            "state": pane.state.name(),
            "message": pane.failure,
        })
    })
}

/// The final `summary` record from the per-state counts of `states` (state names).
pub fn summary_record<'a>(states: impl Iterator<Item = &'a str>, cancelled: bool) -> Value {
    let mut counts = [0usize; 6];
    let mut panes = 0usize;
    for state in states {
        panes += 1;
        let index = match state {
            "summarized" => 0,
            "idle" => 1,
            "denied" => 2,
            "unavailable" => 3,
            "stale" => 4,
            _ => 5,
        };
        counts[index] += 1;
    }
    json!({
        "type": "summary",
        "panes": panes,
        "summarized": counts[0],
        "idle": counts[1],
        "denied": counts[2],
        "unavailable": counts[3],
        "stale": counts[4],
        "failed": counts[5],
        "cancelled": cancelled,
    })
}

/// JSONL records: one `recap` per pane, a `warning` for each failed or stale
/// pane, and one final `summary`.
pub fn render_records(batch: &RecapBatch, lengths: &[RecapLength]) -> Vec<Value> {
    let mut records = Vec::new();
    for pane in &batch.panes {
        records.push(pane_record(pane, lengths));
        records.extend(pane_warning(pane));
    }
    records.push(summary_record(
        batch.panes.iter().map(|pane| pane.state.name()),
        batch.cancelled,
    ));
    records
}

fn state_from_name(name: &str) -> RecapPaneState {
    match name {
        "summarized" => RecapPaneState::Summarized,
        "idle" => RecapPaneState::Idle,
        "denied" => RecapPaneState::Denied,
        "unavailable" => RecapPaneState::Unavailable,
        "stale" => RecapPaneState::Stale,
        _ => RecapPaneState::Failed,
    }
}

fn text_from_value(value: &Value) -> Option<RecapText> {
    let object = value.as_object()?;
    Some(RecapText {
        text: object.get("text")?.as_str()?.to_string(),
        pointers: object
            .get("pointers")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(RecapPointer {
                            path: item.get("path")?.as_str()?.to_string(),
                            note: item
                                .get("note")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        rejected_pointers: object
            .get("rejected_pointers")
            .and_then(Value::as_u64)
            .map_or(0, |count| count as usize),
    })
}

/// Reads a record written by `pane_record` back as a `PaneRecap`. Goal lines are kept as they
/// were rendered, so the structured goal list is not restored; the Markdown report prints the
/// recorded lines.
pub fn pane_from_record(record: &Value) -> Option<(PaneRecap, Vec<String>)> {
    let goal_lines: Vec<String> = record
        .get("goals")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let pane = PaneRecap {
        pane_id: record.get("pane_id")?.as_str()?.to_string(),
        pane_name: record
            .get("pane_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        state: state_from_name(record.get("state")?.as_str()?),
        facts: PaneRecapFacts {
            goals: Vec::new(),
            goals_omitted: 0,
            evidence_age_seconds: record.get("evidence_age_seconds").and_then(Value::as_u64),
            evidence_stale: record
                .get("evidence_stale")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            window_truncated: record
                .get("window_truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            redactions: 0,
        },
        short: record.get("short").and_then(text_from_value),
        long: record.get("long").and_then(text_from_value),
        failure: record
            .get("failure")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    Some((pane, goal_lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PaneRecapFacts, RecapBatch};

    fn pane(state: RecapPaneState, short: Option<&str>) -> PaneRecap {
        PaneRecap {
            pane_id: "7".into(),
            pane_name: "agent".into(),
            state,
            facts: PaneRecapFacts {
                goals: Vec::new(),
                goals_omitted: 0,
                evidence_age_seconds: Some(5),
                evidence_stale: false,
                window_truncated: false,
                redactions: 0,
            },
            short: short.map(|text| RecapText {
                text: text.into(),
                pointers: vec![RecapPointer {
                    path: "src/lib.rs".into(),
                    note: "edited".into(),
                }],
                rejected_pointers: 1,
            }),
            long: None,
            failure: None,
        }
    }

    #[test]
    fn records_end_with_one_summary_and_a_warning_for_failures() {
        let batch = RecapBatch {
            panes: vec![
                pane(RecapPaneState::Summarized, Some("Fixing tests.")),
                pane(RecapPaneState::Failed, None),
            ],
            cancelled: false,
        };
        let records = render_records(&batch, &[RecapLength::Short, RecapLength::Long]);
        let kinds: Vec<&str> = records
            .iter()
            .map(|record| record["type"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(kinds, vec!["recap", "recap", "warning", "summary"]);
        assert_eq!(records[3]["summarized"], 1);
        assert_eq!(records[3]["failed"], 1);
    }

    #[test]
    fn a_record_round_trips_through_pane_from_record() {
        let original = pane(RecapPaneState::Summarized, Some("Fixing tests."));
        let record = pane_record(&original, &[RecapLength::Short]);
        let (restored, _) = pane_from_record(&record).expect("record parses");
        assert_eq!(restored.state, RecapPaneState::Summarized);
        assert_eq!(restored.short, original.short);
        assert_eq!(restored.long, None);
    }

    #[test]
    fn markdown_prints_state_and_text_per_pane() {
        let batch = RecapBatch {
            panes: vec![pane(RecapPaneState::Summarized, Some("Fixing tests."))],
            cancelled: true,
        };
        let markdown = render_markdown(&batch, &[RecapLength::Short]);
        assert!(markdown.contains("### agent (`7`)"));
        assert!(markdown.contains("Fixing tests."));
        assert!(markdown.contains("- `src/lib.rs`: edited"));
        assert!(markdown.contains("Cancelled"));
    }
}
