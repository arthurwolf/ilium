//! Twins of ilium-client/src/cost_history.rs: scan of the agents' on-disk session history for calibration.
//!
//! The scan runs over a virtual home described in the request: each session file is
//! `{agent:"claude"|"codex", name, reported_usd|null}` or a codex file with `model`, `input`, `cached`,
//! `output` tokens, or `quota:[[percent, window_secs]...]`. Reply: `{entries, totals, primary, secondary,
//! error}` where `totals` are the sorted per-session dollars, `primary`/`secondary` the sorted quota use.

use serde_json::{json, Value};

use crate::common::{client, near};

fn scan(request: Value) -> Value {
    client("history.scan", request)
}

fn claude(name: &str, usd: Option<f64>) -> Value {
    json!({ "agent": "claude", "project": "-proj", "name": name, "reported_usd": usd })
}

fn error_of(reply: &Value) -> String {
    reply["error"].as_str().unwrap_or("").to_string()
}

/// REQ: PORT:ilium-client/src/cost_history.rs::a_stop_request_ends_the_scan_without_panicking
#[test]
fn port_cost_history_a_stop_request_ends_the_scan_without_panicking() {
    let reply = scan(json!({ "files": [claude("a", Some(5.0))], "window_days": 30, "cancelled": true }));
    assert!(error_of(&reply).contains("incomplete"), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_history.rs::cancelled_scan_is_an_error_and_does_not_publish_partial_cache
#[test]
fn port_cost_history_cancelled_scan_is_an_error_and_does_not_publish_partial_cache() {
    let reply = scan(json!({
        "files": [claude("synthetic-id", Some(4.0))],
        "window_days": 30,
        "cancelled": true,
        "cache": { "path": "synthetic-cache.json", "bytes": "original-cache-bytes" },
    }));
    assert!(error_of(&reply).contains("incomplete"), "{reply}");
    assert_eq!(reply["cache_bytes"], json!("original-cache-bytes"));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::claude_sessions_carry_no_quota
#[test]
fn port_cost_history_claude_sessions_carry_no_quota() {
    let reply = scan(json!({ "files": [claude("a", Some(3.0))], "window_days": 30 }));
    assert_eq!(reply["primary"], json!([]));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::codex_quota_use_is_the_rise_inside_one_window_period
#[test]
fn port_cost_history_codex_quota_use_is_the_rise_inside_one_window_period() {
    let quota = |name: &str, readings: Value| json!({ "agent": "codex", "name": name, "quota": readings });
    let reply = scan(json!({
        "files": [
            quota("rise", json!([[20.0, 100], [24.0, 100], [31.5, 100]])),
            // The window reset mid-session: only the new period's reading counts.
            quota("reset", json!([[90.0, 100], [6.0, 200]])),
            quota("flat", json!([[50.0, 100], [50.0, 100]])),
        ],
        "window_days": 30,
    }));
    assert_eq!(reply["entries"], json!(3));
    // The flat session is negligible; values are rounded to one decimal.
    assert_eq!(reply["primary"], json!([6.0, 11.5]));
    // A constant window consumed nothing.
    assert_eq!(reply["secondary"], json!([]));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::last_line_extraction_handles_missing_and_partial_lines
#[test]
fn port_cost_history_last_line_extraction_handles_missing_and_partial_lines() {
    let find = |text: &str, needle: &str| client("history.last-line-containing", json!({ "text": text, "needle": needle }));
    let text = "one\ntwo needle\nthree\nfour needle tail";
    assert_eq!(find(text, "needle"), json!("two needle"));
    assert_eq!(find("no newline needle", "needle"), Value::Null);
    assert_eq!(find(text, "absent"), Value::Null);
}

/// REQ: PORT:ilium-client/src/cost_history.rs::old_files_are_ignored
#[test]
fn port_cost_history_old_files_are_ignored() {
    let reply = scan(json!({ "files": [claude("a", Some(5.0))], "window_days": 30, "now_offset_days": 200 }));
    assert_eq!(reply["entries"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::prices_codex_sessions_from_cumulative_tokens_and_the_last_model
#[test]
fn port_cost_history_prices_codex_sessions_from_cumulative_tokens_and_the_last_model() {
    let codex = |name: &str, model: &str, input: u64, cached: u64, output: u64| {
        json!({ "agent": "codex", "name": name, "model": model, "input": input, "cached": cached, "output": output })
    };
    let reply = scan(json!({
        "files": [
            codex("a", "gpt-5.6-sol", 2_000_000, 1_000_000, 100_000),
            codex("b", "gpt-reserve", 10, 0, 10),
        ],
        "window_days": 30,
    }));
    // 1M fresh x $4 + 1M cached x $0.40 + 0.1M output x $20.
    let totals = reply["totals"].as_array().cloned().unwrap_or_default();
    assert_eq!(totals.len(), 1);
    assert!(near(&totals[0], 6.4), "{totals:?}");
}

/// REQ: PORT:ilium-client/src/cost_history.rs::reads_the_last_reported_claude_total_and_skips_files_without_one
#[test]
fn port_cost_history_reads_the_last_reported_claude_total_and_skips_files_without_one() {
    let reply = scan(json!({ "files": [claude("a", Some(12.5)), claude("b", None)], "window_days": 30 }));
    assert_eq!(reply["entries"], json!(2));
    assert_eq!(reply["totals"], json!([12.5]));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::refuses_calibration_when_a_configured_history_root_cannot_be_scanned
#[test]
fn port_cost_history_refuses_calibration_when_a_configured_history_root_cannot_be_scanned() {
    let reply = scan(json!({ "files": [], "window_days": 30, "claude_projects_is_a_file": true }));
    // A failed configured-root scan must not look like empty history.
    assert!(error_of(&reply).contains(".claude/projects"), "{reply}");
}

/// REQ: PORT:ilium-client/src/cost_history.rs::unchanged_files_come_from_the_cache_and_changes_are_reparsed
#[test]
fn port_cost_history_unchanged_files_come_from_the_cache_and_changes_are_reparsed() {
    let first = scan(json!({ "files": [claude("a", Some(3.0))], "window_days": 30, "cache": { "path": "cache/history.json" } }));
    assert_eq!(first["entries"], json!(1));
    // Poison the cached total; an unchanged file must return the cache.
    let second = scan(json!({
        "files": [claude("a", Some(3.0))],
        "window_days": 30,
        "cache": { "path": "cache/history.json", "poison_reported_usd": 999.0 },
    }));
    assert_eq!(second["totals"], json!([999.0]));
    // A changed file (its mtime moves on) is parsed again.
    let third = scan(json!({
        "files": [claude("a", Some(7.0))],
        "window_days": 30,
        "cache": { "path": "cache/history.json", "poison_reported_usd": 999.0 },
        "files_changed": true,
    }));
    assert_eq!(third["totals"], json!([7.0]));
}

/// REQ: PORT:ilium-client/src/cost_history.rs::worker_delivers_a_result_and_then_honours_the_rescan_interval
#[test]
fn port_cost_history_worker_delivers_a_result_and_then_honours_the_rescan_interval() {
    let reply = client(
        "history.request-scans",
        json!({ "files": [claude("a", Some(4.0))], "requests": [30, 30, 30, 60], "wait_for_result_after": 1 }),
    );
    // First request starts a scan, a repeat inside the interval does not, a different window forces one.
    assert_eq!(reply["started"], json!([true, false, false, true]));
    assert_eq!(reply["has_result"], json!(true));
    assert_eq!(reply["entries"], json!(1));
}
