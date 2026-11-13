//! Twins of ilium-client/src/session_stats_timeline.rs: the work ribbon and the time-scale windows.
//!
//! Ops `timeline.work-ribbon` (`{stats:{kinds:[[ms,kind]], spans:[{start_ms,end_ms,outcome}], last_ms}, from,
//! to, columns}` -> `{cells:[{kind,progress_running}|null], outcomes:[outcome|null], has_data,
//! has_running_progress, kinds_present}`) and `timeline.scale-window` (`{stats, scale}` -> `[start,end]|null`).

use serde_json::{json, Value};

use crate::common::client;

fn ribbon(stats: Value, from: i64, to: i64, columns: usize) -> Value {
    client(
        "timeline.work-ribbon",
        json!({ "stats": stats, "from": from, "to": to, "columns": columns }),
    )
}

fn stats(kinds: Value, spans: Value, last_ms: i64) -> Value {
    json!({ "kinds": kinds, "spans": spans, "last_ms": last_ms })
}

/// REQ: PORT:ilium-client/src/session_stats_timeline.rs::failure_outranks_success_in_one_column_and_cleared_runs_show_no_icon
#[test]
fn port_session_stats_timeline_failure_outranks_success_in_one_column_and_cleared_runs_show_no_icon(
) {
    let spans = json!([
        { "start_ms": 0, "end_ms": 50, "outcome": "success" },
        { "start_ms": 0, "end_ms": 51, "outcome": "failure" },
        { "start_ms": 0, "end_ms": 90, "outcome": "cleared" },
    ]);
    let reply = ribbon(stats(json!([[0, "model"]]), spans, 100), 0, 100, 4);
    assert_eq!(reply["outcomes"][2], json!("failure"));
    assert!(reply["outcomes"][3].is_null());
}

/// REQ: PORT:ilium-client/src/session_stats_timeline.rs::open_progress_runs_to_the_end_and_empty_input_is_safe
#[test]
fn port_session_stats_timeline_open_progress_runs_to_the_end_and_empty_input_is_safe() {
    let spans = json!([{ "start_ms": 50, "end_ms": null, "outcome": "running" }]);
    let open = stats(json!([[0, "shell"]]), spans, 100);
    let reply = ribbon(open.clone(), 0, 100, 4);
    assert_eq!(reply["cells"][0]["progress_running"], json!(false));
    assert_eq!(reply["cells"][3]["progress_running"], json!(true));
    assert_eq!(ribbon(json!({}), 0, 100, 4)["has_data"], json!(false));
    let reversed = ribbon(open.clone(), 100, 0, 4);
    assert!(reversed["cells"]
        .as_array()
        .is_some_and(|cells| cells.iter().all(Value::is_null)));
    assert_eq!(ribbon(open, 0, 100, 0)["cells"], json!([]));
}

/// REQ: PORT:ilium-client/src/session_stats_timeline.rs::ribbon_columns_take_the_dominant_state_and_flag_running_progress
#[test]
fn port_session_stats_timeline_ribbon_columns_take_the_dominant_state_and_flag_running_progress() {
    let spans = json!([{ "start_ms": 40, "end_ms": 79, "outcome": "success" }]);
    let reply = ribbon(
        stats(
            json!([[0, "model"], [40, "shell"], [80, "idle"]]),
            spans,
            100,
        ),
        0,
        100,
        10,
    );
    assert_eq!(reply["cells"][0]["kind"], json!("model"));
    assert_eq!(reply["cells"][5]["kind"], json!("shell"));
    assert_eq!(reply["cells"][9]["kind"], json!("idle"));
    let running: Vec<bool> = reply["cells"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|cell| cell["progress_running"] == json!(true))
        .collect();
    assert_eq!(
        running,
        [false, false, false, false, true, true, true, true, false, false]
    );
    assert_eq!(reply["outcomes"][7], json!("success"));
    assert_eq!(
        reply["outcomes"]
            .as_array()
            .map(|all| all.iter().filter(|outcome| !outcome.is_null()).count()),
        Some(1)
    );
    assert_eq!(reply["has_running_progress"], json!(true));
    assert_eq!(reply["kinds_present"], json!(["idle", "model", "shell"]));
}

/// REQ: PORT:ilium-client/src/session_stats_timeline.rs::scale_windows_end_at_the_last_event_and_never_precede_the_first
#[test]
fn port_session_stats_timeline_scale_windows_end_at_the_last_event_and_never_precede_the_first() {
    let window = |stats: Value, scale: &str| {
        client(
            "timeline.scale-window",
            json!({ "stats": stats, "scale": scale }),
        )
    };
    let long = stats(json!([[1_000, "model"]]), json!([]), 10_000_000);
    assert_eq!(window(long.clone(), "all"), json!([1_000, 10_000_000]));
    assert_eq!(
        window(long, "five_minutes"),
        json!([10_000_000 - 300_000, 10_000_000])
    );
    let short = stats(json!([[1_000, "model"]]), json!([]), 60_000);
    assert_eq!(window(short, "day"), json!([1_000, 60_000]));
    assert_eq!(window(json!({}), "all"), Value::Null);
}
