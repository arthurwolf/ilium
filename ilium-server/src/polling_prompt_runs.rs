//! Run metadata over already sanitized polling evidence.
use serde_json::{json, Value};

pub(super) fn compress(ordered: Vec<Value>) -> Vec<Value> {
    let mut output = Vec::new();
    let mut offset = 0;
    while offset < ordered.len() {
        let mut accepted = None;
        for width in (1..=3).rev() {
            let Some(pattern) = ordered.get(offset..offset + width) else {
                continue;
            };
            if !eligible(pattern) {
                continue;
            }
            let mut repetitions = 1;
            while let Some(next) =
                ordered.get(offset + width * repetitions..offset + width * (repetitions + 1))
            {
                if !same_cycle(pattern, next) {
                    break;
                }
                repetitions += 1;
            }
            if repetitions >= 3 {
                accepted = Some((width, repetitions));
                break;
            }
        }
        if let Some((width, repetitions)) = accepted {
            let end = offset + width * repetitions;
            output.push(run(&ordered[offset..end], width, repetitions));
            offset = end;
        } else {
            output.push(ordered[offset].clone());
            offset += 1;
        }
    }
    output
}

fn eligible(pattern: &[Value]) -> bool {
    if pattern.iter().any(|event| event["truncated"] != false) {
        return false;
    }
    if pattern.len() == 1 {
        return pattern[0]["kind"] == "message"
            && pattern[0]["text"]
                .as_str()
                .is_some_and(|text| !text.is_empty());
    }
    let call = &pattern[0];
    let result = &pattern[1];
    let operation = call["tool"]
        .as_str()
        .unwrap_or("")
        .rsplit("__")
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("");
    call["kind"] == "tool_call"
        && (matches!(operation, "wait" | "write_stdin" | "wait_agent")
            || call["text"] == "possible status check"
            || call["text"] == "wait or continuation")
        && !call["target"].is_null()
        && result["kind"] == "tool_result"
        && result["target"] == call["target"]
        && result["text"] != "unknown"
        && (pattern.len() == 2 || pattern[2]["kind"] == "message")
}

fn same_cycle(first: &[Value], next: &[Value]) -> bool {
    eligible(next)
        && first.iter().zip(next).all(|(left, right)| {
            if right["kind"] == "tool_result" && right["changed_since_last"] != false {
                return false;
            }
            ["kind", "tool", "target", "text", "truncated"]
                .iter()
                .all(|key| left[*key] == right[*key])
        })
}

fn run(events: &[Value], width: usize, repetitions: usize) -> Value {
    let timestamps: Option<Vec<_>> = events
        .iter()
        .map(|event| {
            event["timestamp"]
                .as_str()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|time| time.timestamp_millis())
        })
        .collect();
    let timing = timestamps.filter(|times| times.windows(2).all(|pair| pair[1] >= pair[0]));
    let elapsed = timing
        .as_ref()
        .and_then(|times| times.last()?.checked_sub(*times.first()?));
    let intervals: Option<Vec<_>> = timing
        .as_ref()
        .map(|times| times.iter().step_by(width).copied().collect::<Vec<_>>())
        .map(|starts| {
            starts
                .windows(2)
                .filter_map(|pair| pair[1].checked_sub(pair[0]))
                .collect()
        });
    let durations: Vec<_> = events
        .iter()
        .filter_map(|event| event["requested_wait_ms"].as_u64())
        .collect();
    json!({
        "kind": "run", "repetitions": repetitions,
        "events": &events[..width], "last_events": &events[events.len()-width..],
        "first_timestamp": events.first().map(|event| &event["timestamp"]),
        "last_timestamp": events.last().map(|event| &event["timestamp"]),
        "elapsed_ms": elapsed,
        "minimum_interval_ms": intervals.as_ref().and_then(|values| values.iter().min()),
        "maximum_interval_ms": intervals.as_ref().and_then(|values| values.iter().max()),
        "minimum_requested_wait_ms": durations.iter().min(),
        "maximum_requested_wait_ms": durations.iter().max(),
        "commentary_between": events.iter().any(|event| event["kind"] == "message")
    })
}
