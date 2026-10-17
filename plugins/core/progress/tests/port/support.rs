//! Helpers of the port twins. Every use of an item the skeleton lacks is in the gap section.
#![allow(dead_code)]

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Intent, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;

pub const PROBE: &str = "/opt/probe.sh";
pub const JOB: &str = "render-42";

/// One monitor entry in the shape of `GET /progress.json`.
pub fn monitor_with(
    id: u64,
    job: &str,
    status: &str,
    percent: f64,
    message: &str,
    details: &str,
    error: Value,
) -> Value {
    json!({"monitor_id": id, "report": {"job_id": job, "status": status, "percent": percent,
        "message": message, "details": details, "error": error},
        "monitor_health": {"state": "healthy"}, "last_observed_unix_millis": 0})
}

pub fn monitor(id: u64, status: &str, percent: f64, message: &str) -> Value {
    monitor_with(
        id,
        &format!("job-{id}"),
        status,
        percent,
        message,
        "",
        Value::Null,
    )
}

pub fn show(host: &mut FakeHost, monitors: Vec<Value>) {
    host.deliver_one(monitors_message(&json!({"monitors": monitors})));
    host.settle();
}

pub fn footer(host: &mut FakeHost, cols: u16, rows: u16) -> String {
    host.render(ViewTarget::Panel(FOOTER_VIEW.to_string()), cols, rows)
        .text()
}

pub fn segments(host: &FakeHost) -> Vec<Intent> {
    host.submitted()
        .iter()
        .map(|s| s.intent.clone())
        .filter(|i| {
            matches!(
                i,
                Intent::SetStatusSegment(_) | Intent::ClearStatusSegment(_)
            )
        })
        .collect()
}

pub fn pass(host: &mut FakeHost, milliseconds: u64) {
    host.advance_clock(milliseconds);
    host.settle();
}

pub fn code(run: &Run) -> String {
    record_of(&run.stdout, "progress_rejected")["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Settles the monitor `PROBE` of a pane with `output` after one second.
pub fn settle_with(host: &mut FakeHost, output: &str) {
    expect_probe(host, PROBE, output);
    tick(host, 1);
}

// ---------------------------------------------------------------------------------------------
// Gap helpers (see ../API_GAPS.md)
// ---------------------------------------------------------------------------------------------

/// Hover help text at a cell of a view (API_GAPS: FakeHost::hover).
pub fn hover(host: &mut FakeHost, column: u16, row: u16) -> Option<String> {
    host.hover(ViewTarget::Panel(FOOTER_VIEW.to_string()), column, row)
}
