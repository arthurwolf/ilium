//! Compaction-lane testing hooks used only by the port test target.
//!
//! Every method is plugin-specific scaffolding that drives the compaction plugin through the host.
//! Engine operations are plugin jobs named `compaction.<operation>` with a JSON argument and a JSON
//! result. A mutating call runs on the test host and is mirrored on a per-thread engine host, so
//! read-only (`&self`) queries see the same operations. A plugin that does not answer a job makes
//! the calling test fail with the job name. UI hooks use the real render, click and key calls of
//! the host. Shared harness behaviour stays in `ilium_plugin_sdk::testing::FakeHost`.

use ilium_plugin_sdk::testing::{FakeHost, Screen, ScreenCell};
use ilium_plugin_sdk::types::{Color, ColorRole, ViewTarget};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fmt;

use crate::host_ext_shared::CompactionSharedHostExt;
use std::time::Duration;

/// Error of an agent-home scan (compaction scan engine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanError {
    /// Stable error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl ScanError {
    /// The stable error code.
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ScanError {}

/// Geometry of the confirmation dialog (compaction remote dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmationLayout {
    pub text_right: u16,
    pub scrollbar_x: u16,
    pub text_y: u16,
    pub scrollbar_y: u16,
    pub text_height: u16,
    pub scrollbar_height: u16,
    pub scrollbar_right: u16,
    pub scrollbar_bottom: u16,
    pub text_bottom: u16,
    pub hint_row_y: u16,
}

/// Column and line geometry of one settings row (compaction settings page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowGeometry {
    pub box_column: u16,
    pub first_line: u16,
    pub control_column: u16,
    pub control_line: u16,
    pub next_column: u16,
    pub previous_column: u16,
    pub open_column: u16,
    pub value_column: u16,
    pub value_right: u16,
    pub value_slot_width: u16,
    pub value_width: u16,
}

/// What a click on the settings page hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsHit {
    pub action: String,
    pub row: String,
}

/// The close button rectangle of the settings banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseRect {
    pub row: String,
    pub text: String,
    pub height: u16,
    pub width: u16,
    pub column: u16,
}

/// Line span of one settings row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub last_line: usize,
}

/// Outcome of a worker pause (compaction remote worker).
#[derive(Debug, Clone, PartialEq)]
pub struct PauseOutcome {
    pub result: Result<Value, String>,
    pub interrupts: u32,
}

use crate::common::server_host;

thread_local! {
    /// Mirror host answering the read-only queries of this test thread.
    static ENGINE: RefCell<Option<FakeHost>> = const { RefCell::new(None) };
    /// The last settings screen rendered through a hook (answers painted-text queries).
    static SCREEN: RefCell<Option<Screen>> = const { RefCell::new(None) };
    /// Screen size set through `set_screen_size`.
    static SIZE: RefCell<(u16, u16)> = const { RefCell::new((100, 40)) };
    /// Number of scans the test finished through `finish_scans`.
    static FINISHED_SCANS: RefCell<usize> = const { RefCell::new(0) };
}

fn with_engine<T>(body: impl FnOnce(&mut FakeHost) -> T) -> T {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let host = slot.get_or_insert_with(server_host);
        body(host)
    })
}

/// Runs one plugin job on a host and parses the JSON result; the error is the failure message.
fn call(host: &mut FakeHost, operation: &str, arguments: Value) -> Result<Value, String> {
    let kind = format!("compaction.{operation}");
    match host.run_job(&kind, arguments.to_string()) {
        Ok(bytes) if bytes.is_empty() => Ok(Value::Null),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("job {kind} returned invalid JSON: {error}")),
        Err(error) => Err(format!("{kind}: {error:?}")),
    }
}

/// A mutating job: runs on the test host and is mirrored on the engine host.
fn mutate(host: &mut FakeHost, operation: &str, arguments: Value) -> Result<Value, String> {
    // The mirror result is dropped on purpose; the test host's answer is the contract.
    let _mirror = with_engine(|engine| call(engine, operation, arguments.clone()));
    call(host, operation, arguments)
}

/// Mirrors an input already delivered to the test host (click, key) onto the engine host.
fn mirror_input(operation: &str, arguments: Value) {
    // Dropped on purpose: only read-only queries consult the mirror.
    let _mirror = with_engine(|engine| call(engine, operation, arguments));
}

/// A mutating job whose failure fails the test.
fn act(host: &mut FakeHost, operation: &str, arguments: Value) -> Value {
    mutate(host, operation, arguments)
        .unwrap_or_else(|message| panic!("compaction job `{operation}` failed: {message}"))
}

/// A read-only job; a failure fails the test.
fn query(operation: &str, arguments: Value) -> Value {
    with_engine(|engine| call(engine, operation, arguments))
        .unwrap_or_else(|message| panic!("compaction query `{operation}` failed: {message}"))
}

fn as_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn texts(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().map(as_text).collect())
        .unwrap_or_default()
}

fn number(value: &Value) -> u64 {
    value.as_u64().unwrap_or_default()
}

fn list(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        _ => Vec::new(),
    }
}

fn scan_error(message: String) -> ScanError {
    let code = message
        .split(':')
        .next()
        .unwrap_or("scan-failed")
        .trim()
        .to_owned();
    ScanError { code, message }
}

fn remember(screen: Screen) -> Screen {
    SCREEN.with(|cell| *cell.borrow_mut() = Some(screen.clone()));
    screen
}

fn last_screen() -> Screen {
    SCREEN
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| panic!("no settings screen was rendered yet; call render_settings first"))
}

fn screen_size() -> (u16, u16) {
    SIZE.with(|cell| *cell.borrow())
}

fn geometry_field(value: &Value, key: &str) -> u16 {
    u16::try_from(number(&value[key])).unwrap_or(u16::MAX)
}

fn target_name(target: &ViewTarget) -> Value {
    json!(format!("{target:?}"))
}

fn hit_from(value: &Value) -> Option<SettingsHit> {
    if value.is_null() {
        return None;
    }
    Some(SettingsHit {
        action: as_text(&value["action"]),
        row: as_text(&value["row"]),
    })
}

fn close_rect_from(value: &Value) -> Option<CloseRect> {
    if value.is_null() {
        return None;
    }
    Some(CloseRect {
        row: as_text(&value["row"]),
        text: as_text(&value["text"]),
        height: geometry_field(value, "height"),
        width: geometry_field(value, "width"),
        column: geometry_field(value, "column"),
    })
}

pub trait CompactionPortHostExt {
    fn build_report(&mut self, input: Value) -> Result<Value, String>;
    fn price_weights(&self, model: &str) -> Option<Value>;
    fn default_grid(&self, agent: &str) -> Vec<u64>;
    fn format_value(&self, kind: &str, value: f64) -> String;
    fn format_token_quantiles(&self, count: u32, p25: f64, p50: f64, p75: f64) -> String;
    fn widening_triggers(&self, agent: &str, grid: &[u64], current: u64) -> Vec<u64>;
    fn scan_agent_homes(&mut self, agent: &str, home: Option<&str>, limit: Option<usize>, timeout_ms: Option<u32>) -> Result<Value, ScanError>;
    fn scan_agent_homes_growing(&mut self, agent: &str, home: &str, path: &str) -> Result<Value, ScanError>;
    fn scan_agent_homes_shrinking(&mut self, agent: &str, home: &str, keep: usize) -> Result<Value, ScanError>;
    fn scan_agent_homes_vanishing(&mut self, agent: &str, home: &str) -> Result<Value, ScanError>;
    fn scan_agent_homes_with_current(&mut self, agent: &str, current: u64) -> Result<Value, ScanError>;
    fn scan_event_stream(&mut self, agent: &str) -> Vec<Value>;
    fn scan_progress_reports(&mut self, agent: &str) -> Vec<Value>;
    fn list_transcripts(&mut self, agent: &str) -> Vec<Value>;
    fn list_transcripts_with_stop(&mut self, agent: &str) -> Vec<Value>;
    fn listing_warnings(&self) -> Vec<String>;
    fn last_report(&self, agent: &str) -> Value;
    fn default_cache_path(&self, agent: &str) -> String;
    fn single_trace_turns(&mut self, agent: &str, path: &str) -> u64;
    fn touch(&mut self, agent: &str, path: &str);
    fn disable_inference_and_agent_access(&mut self);
    fn read_bounded_lines(&self, bytes: &[u8], limit: usize) -> Vec<Value>;
    fn read_bounded_lines_stop_after(&self, bytes: &[u8], limit: usize, stop_after: usize) -> Vec<Value>;
    fn is_scanning(&self, agent: &str) -> bool;
    fn scan_request(&self, index: usize) -> Value;
    fn finish_scans(&mut self);
    fn set_scan_cancelled(&mut self);
    fn set_scan_failure(&mut self, message: &str);
    fn extrapolated_alternative_tokens(&self) -> Option<u64>;
    fn set_note(&mut self, agent: &str, note: &str);
    fn set_progress(&mut self, agent: &str, done: u64, total: u64, message: &str);
    fn set_report(&mut self, report_json: &str);
    fn set_revert_record(&mut self, previous: u64, current: u64);
    fn clear_revert_record(&mut self);
    fn transcript_format_for(&self, agent: &str) -> Option<String>;
    fn technique_ids(&self) -> Vec<String>;
    fn technique_description(&self, id: &str) -> String;
    fn technique_fidelity(&self, id: &str) -> String;
    fn bar_parts(&self, fraction: f64, width: usize) -> (String, String);
    fn kept_columns(&self, columns: &[(&str, u16, usize)], width: usize) -> Vec<String>;
    fn wrap_text(&self, text: &str, width: usize) -> Vec<String>;
    fn truncate_to(&self, text: &str, width: usize) -> String;
    fn button_is_filled(&mut self, target: ViewTarget, label: &str) -> bool;
    fn close_confirmation(&mut self);
    fn click_help_anchor(&mut self, target: ViewTarget, topic: &str);
    fn close_view(&mut self, target: ViewTarget);
    fn confirmation_layout(&mut self, target: ViewTarget, columns: u16, rows: u16) -> ConfirmationLayout;
    fn custom_prompt_max_chars(&self) -> usize;
    fn custom_prompt_max_serialized_bytes(&self) -> usize;
    fn declared_help_topics(&self) -> Vec<String>;
    fn help_anchor_topics(&mut self, target: ViewTarget) -> Vec<String>;
    fn help_anchors(&mut self, target: ViewTarget, columns: u16, rows: u16) -> Vec<(String, u16)>;
    fn help_topic_exists(&self, topic: &str) -> bool;
    fn help_topic_is_dialog_only(&self, topic: &str) -> bool;
    fn open_help_topic(&self) -> Option<String>;
    fn key_consumed(&mut self, target: ViewTarget, key: &str) -> bool;
    fn max_scroll(&mut self, target: ViewTarget, columns: u16, rows: u16) -> usize;
    fn scroll_offset(&self, target: ViewTarget) -> usize;
    fn scroll_wheel(&mut self, target: ViewTarget, delta: i32);
    fn open_settings_tab(&mut self, tab: &str);
    fn render_settings(&mut self, columns: u16, rows: u16) -> Screen;
    fn prompt_editor_text(&self) -> Option<String>;
    fn last_settings_command_bytes(&self) -> usize;
    fn icon_assignment_suggestions(&self) -> Vec<String>;
    fn icon_assignment_set(&mut self, glyph: &str);
    fn icon_assignment_click(&mut self, row: &str, action: &str);
    fn icon_assignment_glyph(&self) -> String;
    fn icon_picker_open(&self) -> bool;
    fn sidebar_last_list_cell(&mut self, columns: u16, rows: u16, label: &str) -> ScreenCell;
    fn accent_background(&self) -> Color;
    fn toolbar_buttons(&self, columns: u16, rows: u16) -> Vec<(String, u16, u16)>;
    fn toolbar_action_at(&self, columns: u16, rows: u16, column: u16, row: u16) -> Option<String>;
    fn status_bar_text(&self, columns: u16) -> String;
    fn set_codex_reset_watch_expiry(&mut self, in_: Duration, label: &str);
    fn set_screen_size(&mut self, columns: u16, rows: u16);
    fn settings_row_geometry(&self, row: &str) -> RowGeometry;
    fn settings_hit(&self, column: u16, row: u16) -> Option<SettingsHit>;
    fn settings_hit_after_scroll(&mut self, column: u16, row: u16) -> Option<SettingsHit>;
    fn settings_banner_close_rect(&self) -> Option<CloseRect>;
    fn settings_banner_close_rect_at(&self, columns: u16) -> Option<CloseRect>;
    fn settings_banner_visible(&self) -> bool;
    fn settings_row_span(&self, index: usize) -> RowSpan;
    fn settings_scroll_for_selection(&self, selected: usize, scroll: usize, rows: usize) -> usize;
    fn settings_max_scroll(&self, rows: usize, height: usize) -> usize;
    fn settings_help_anchors(&self) -> Vec<(String, usize)>;
    fn settings_row_help_id(&self, row: &str) -> String;
    fn settings_painted_control(&self, row: &str, columns: u16) -> String;
    fn settings_painted_glyph(&self, column: u16, row: u16) -> char;
    fn settings_page_lines(&self, columns: u16) -> Vec<String>;
    fn settings_page_text(&self, columns: u16) -> String;
    fn settings_privacy_banner_text(&self, settings: Value) -> String;
    fn settings_row_labels(&self) -> Vec<String>;
    fn settings_section_headings(&self) -> Vec<String>;
    fn settings_rows(&self) -> Vec<String>;
    fn settings_selected_row(&self) -> usize;
    fn settings_row_index(&self, row: &str) -> usize;
    fn settings_save_pending(&self) -> bool;
    fn settings_write_count(&self) -> usize;
    fn settings_set_value(&mut self, row: &str, value: Value);
    fn settings_value(&self, row: &str) -> Value;
    fn settings_dialog(&self) -> Option<Value>;
    fn settings_dialog_choose(&mut self, option: &str);
    fn settings_dialog_click_option(&mut self, index: usize);
    fn settings_dialog_key(&mut self, key: &str);
    fn settings_dismiss_banner(&mut self);
    fn settings_clear_prompt(&mut self);
    fn settings_commit_prompt(&mut self, text: &str);
    fn settings_click(&mut self, column: u16, row: u16);
    fn settings_key(&mut self, key: &str);
    fn settings_adjust_row(&mut self, row: &str, delta: i32);
    fn settings_serialized_bound_ok(&self, settings: &Value) -> bool;
    fn remote_settings_adjust(&mut self, row: &str, delta: i32) -> bool;
    fn remote_settings_current(&self) -> Value;
    fn remote_settings_defaults(&self) -> Value;
    fn remote_settings_help_ids(&self) -> Vec<String>;
    fn remote_settings_help_topic_count(&self) -> usize;
    fn remote_settings_options_for(&self, settings: &Value, row: &str) -> Value;
    fn remote_settings_row_ids(&self) -> Vec<String>;
    fn remote_settings_sanitize(&self, settings: Value) -> Value;
    fn remote_settings_set(&mut self, settings: Value);
    fn remote_settings_technique_for(&self, settings: &Value, agent: &str) -> String;
    fn default_summarizer_max_output_tokens(&self) -> u64;
    fn flow_start(&mut self, agent: &str, technique: &str);
    fn flow_event(&mut self, event: Value);
    fn flow_key(&mut self, key: &str);
    fn flow_pending_job(&self) -> Option<Value>;
    fn flow_replaces_native_compact(&self, pane: u64) -> bool;
    fn flow_requests(&self) -> Vec<Value>;
    fn flow_set_agent_stopped(&mut self, stopped: bool);
    fn flow_set_phase(&mut self, phase: &str);
    fn flow_state(&self) -> Value;
    fn flow_take_cancel(&mut self) -> bool;
    fn flow_try_start(&mut self, pane: u64, force: bool) -> bool;
    fn monitor_record_attempt(&mut self, pane: u64, at_ms: u64);
    fn monitor_record_failure(&mut self, pane: u64);
    fn monitor_record_success(&mut self, pane: u64);
    fn monitor_should_trigger(&mut self, pane: u64, used: f64, threshold: f64, now_ms: u64, cooldown_ms: u64) -> bool;
    fn native_trigger_evaluate(&self, agent: &str, state: Value, used: u64, limit: u64) -> Value;
    fn native_trigger_detect(&mut self, agent: &str) -> Value;
    fn dialog_banner_close_width(&self) -> u16;
    fn dialog_event(&mut self, event: Value);
    fn dialog_fail(&mut self, message: &str);
    fn dialog_hint(&self) -> String;
    fn dialog_open(&mut self, request: Value);
    fn dialog_render(&self, columns: u16, rows: u16, focus: Option<&str>) -> String;
    fn dialog_set_agent_stopped(&mut self, stopped: bool);
    fn dialog_state(&self) -> Value;
    fn dialog_token_bar_lines(&self, state: Value, width: usize) -> Vec<String>;
    fn worker_annotate_progress_overflow(&self, result: Result<Value, String>, overflow: bool) -> Result<Value, String>;
    fn worker_classify_inference_error(&self, error: Value) -> String;
    fn worker_await_pause(&self, agent: &str, reason: &str, timeout: Duration, poll: Duration, interruptible: bool) -> PauseOutcome;
    fn worker_cancel(&mut self);
    fn worker_cancel_observed(&self, within: Duration) -> bool;
    fn worker_fill_event_queue(&mut self);
    fn worker_idle(&self, within: Duration) -> bool;
    fn worker_pause_probe_cost_input_bytes(&self) -> u64;
    fn worker_start_pause_job(&mut self, agent: &str, reason: &str, timeout: Duration);
    fn worker_wait_interrupt_pending(&self, within: Duration) -> bool;
}

impl CompactionPortHostExt for FakeHost {
    fn build_report(&mut self, input: Value) -> Result<Value, String> {
        mutate(self, "build-report", input)
    }
    fn price_weights(&self, model: &str) -> Option<Value> {
        let reply = query("price-weights", json!({ "model": model }));
        if reply.is_null() { None } else { Some(reply) }
    }
    fn default_grid(&self, agent: &str) -> Vec<u64> {
        query("default-grid", json!({ "agent": agent })).as_array().map(|items| items.iter().map(number).collect()).unwrap_or_default()
    }
    fn format_value(&self, kind: &str, value: f64) -> String {
        as_text(&query("format-value", json!({ "kind": kind, "value": value })))
    }
    fn format_token_quantiles(&self, count: u32, p25: f64, p50: f64, p75: f64) -> String {
        as_text(&query("format-token-quantiles", json!({ "count": count, "p25": p25, "p50": p50, "p75": p75 })))
    }
    fn widening_triggers(&self, agent: &str, grid: &[u64], current: u64) -> Vec<u64> {
        query("widening-triggers", json!({ "agent": agent, "grid": grid, "current": current })).as_array().map(|items| items.iter().map(number).collect()).unwrap_or_default()
    }
    fn scan_agent_homes(&mut self, agent: &str, home: Option<&str>, limit: Option<usize>, timeout_ms: Option<u32>) -> Result<Value, ScanError> {
        mutate(self, "scan-agent-homes", json!({ "agent": agent, "home": home, "limit": limit, "timeout_ms": timeout_ms })).map_err(scan_error)
    }
    fn scan_agent_homes_growing(&mut self, agent: &str, home: &str, path: &str) -> Result<Value, ScanError> {
        mutate(self, "scan-agent-homes-growing", json!({ "agent": agent, "home": home, "path": path })).map_err(scan_error)
    }
    fn scan_agent_homes_shrinking(&mut self, agent: &str, home: &str, keep: usize) -> Result<Value, ScanError> {
        mutate(self, "scan-agent-homes-shrinking", json!({ "agent": agent, "home": home, "keep": keep })).map_err(scan_error)
    }
    fn scan_agent_homes_vanishing(&mut self, agent: &str, home: &str) -> Result<Value, ScanError> {
        mutate(self, "scan-agent-homes-vanishing", json!({ "agent": agent, "home": home })).map_err(scan_error)
    }
    fn scan_agent_homes_with_current(&mut self, agent: &str, current: u64) -> Result<Value, ScanError> {
        mutate(self, "scan-agent-homes-with-current", json!({ "agent": agent, "current": current })).map_err(scan_error)
    }
    fn scan_event_stream(&mut self, agent: &str) -> Vec<Value> {
        list(act(self, "scan-event-stream", json!({ "agent": agent })))
    }
    fn scan_progress_reports(&mut self, agent: &str) -> Vec<Value> {
        list(act(self, "scan-progress-reports", json!({ "agent": agent })))
    }
    fn list_transcripts(&mut self, agent: &str) -> Vec<Value> {
        list(act(self, "list-transcripts", json!({ "agent": agent })))
    }
    fn list_transcripts_with_stop(&mut self, agent: &str) -> Vec<Value> {
        list(act(self, "list-transcripts-with-stop", json!({ "agent": agent })))
    }
    fn listing_warnings(&self) -> Vec<String> {
        texts(&query("listing-warnings", json!({})))
    }
    fn last_report(&self, agent: &str) -> Value {
        query("last-report", json!({ "agent": agent }))
    }
    fn default_cache_path(&self, agent: &str) -> String {
        as_text(&query("default-cache-path", json!({ "agent": agent })))
    }
    fn single_trace_turns(&mut self, agent: &str, path: &str) -> u64 {
        number(&act(self, "single-trace-turns", json!({ "agent": agent, "path": path })))
    }
    fn touch(&mut self, agent: &str, path: &str) {
        act(self, "touch", json!({ "agent": agent, "path": path }));
    }
    fn disable_inference_and_agent_access(&mut self) {
        act(self, "disable-inference-and-agent-access", json!({}));
    }
    fn read_bounded_lines(&self, bytes: &[u8], limit: usize) -> Vec<Value> {
        list(query("read-bounded-lines", json!({ "bytes": bytes, "limit": limit })))
    }
    fn read_bounded_lines_stop_after(&self, bytes: &[u8], limit: usize, stop_after: usize) -> Vec<Value> {
        list(query("read-bounded-lines", json!({ "bytes": bytes, "limit": limit, "stop_after": stop_after })))
    }
    fn is_scanning(&self, agent: &str) -> bool {
        let finished = FINISHED_SCANS.with(|cell| *cell.borrow());
        self.scans_started().iter().skip(finished).any(|scan| scan["agent"] == agent)
    }
    fn scan_request(&self, index: usize) -> Value {
        let scans = self.scans_started();
        scans.get(index).cloned().unwrap_or_else(|| panic!("scan request {index} was never started ({} started)", scans.len()))
    }
    fn finish_scans(&mut self) {
        act(self, "finish-scans", json!({}));
        let started = self.scans_started().len();
        FINISHED_SCANS.with(|cell| *cell.borrow_mut() = started);
    }
    fn set_scan_cancelled(&mut self) {
        act(self, "set-scan-cancelled", json!({}));
    }
    fn set_scan_failure(&mut self, message: &str) {
        act(self, "set-scan-failure", json!({ "message": message }));
    }
    fn extrapolated_alternative_tokens(&self) -> Option<u64> {
        query("extrapolated-alternative-tokens", json!({})).as_u64()
    }
    fn set_note(&mut self, agent: &str, note: &str) {
        act(self, "set-note", json!({ "agent": agent, "note": note }));
    }
    fn set_progress(&mut self, agent: &str, done: u64, total: u64, message: &str) {
        act(self, "set-progress", json!({ "agent": agent, "done": done, "total": total, "message": message }));
    }
    fn set_report(&mut self, report_json: &str) {
        let report: Value = serde_json::from_str(report_json).unwrap_or_else(|error| panic!("set_report needs JSON: {error}"));
        act(self, "set-report", report);
    }
    fn set_revert_record(&mut self, previous: u64, current: u64) {
        act(self, "set-revert-record", json!({ "previous": previous, "current": current }));
    }
    fn clear_revert_record(&mut self) {
        act(self, "clear-revert-record", json!({}));
    }
    fn transcript_format_for(&self, agent: &str) -> Option<String> {
        query("transcript-format-for", json!({ "agent": agent })).as_str().map(str::to_owned)
    }
    fn technique_ids(&self) -> Vec<String> {
        texts(&query("technique-ids", json!({})))
    }
    fn technique_description(&self, id: &str) -> String {
        as_text(&query("technique-description", json!({ "id": id })))
    }
    fn technique_fidelity(&self, id: &str) -> String {
        as_text(&query("technique-fidelity", json!({ "id": id })))
    }
    fn bar_parts(&self, fraction: f64, width: usize) -> (String, String) {
        let parts = query("bar-parts", json!({ "fraction": fraction, "width": width }));
        (as_text(&parts[0]), as_text(&parts[1]))
    }
    fn kept_columns(&self, columns: &[(&str, u16, usize)], width: usize) -> Vec<String> {
        let columns: Vec<Value> = columns.iter().map(|(name, priority, minimum)| json!({ "name": name, "priority": priority, "min": minimum })).collect();
        texts(&query("kept-columns", json!({ "columns": columns, "width": width })))
    }
    fn wrap_text(&self, text: &str, width: usize) -> Vec<String> {
        texts(&query("wrap-text", json!({ "text": text, "width": width })))
    }
    fn truncate_to(&self, text: &str, width: usize) -> String {
        as_text(&query("truncate-to", json!({ "text": text, "width": width })))
    }
    fn button_is_filled(&mut self, target: ViewTarget, label: &str) -> bool {
        let (columns, rows) = screen_size();
        let screen = self.render(target, columns, rows);
        let Some((row, column)) = screen.find(label) else {
            panic!("button `{label}` is not on the screen:\n{}", screen.text());
        };
        let cell = screen.cell(row, column);
        cell.background != Color::Role(ColorRole::Foreground) && cell.background != screen.cell(row, 0).background
    }
    fn close_confirmation(&mut self) {
        act(self, "close-confirmation", json!({}));
    }
    fn click_help_anchor(&mut self, target: ViewTarget, topic: &str) {
        let (columns, rows) = screen_size();
        let anchors = self.help_anchors(target.clone(), columns, rows);
        let Some((_, row)) = anchors.iter().find(|(name, _)| name == topic) else {
            panic!("help anchor `{topic}` is not on the screen; anchors: {anchors:?}");
        };
        let screen = self.render(target.clone(), columns, rows);
        let column = screen.row(*row).chars().count().saturating_sub(1);
        let column = u16::try_from(column).unwrap_or(0);
        self.click(target, column, *row);
    }
    fn close_view(&mut self, target: ViewTarget) {
        act(self, "close-view", json!({ "target": target_name(&target) }));
    }
    fn confirmation_layout(&mut self, target: ViewTarget, columns: u16, rows: u16) -> ConfirmationLayout {
        let layout = act(self, "confirmation-layout", json!({ "target": target_name(&target), "columns": columns, "rows": rows }));
        ConfirmationLayout {
            text_right: geometry_field(&layout, "text_right"),
            scrollbar_x: geometry_field(&layout, "scrollbar_x"),
            text_y: geometry_field(&layout, "text_y"),
            scrollbar_y: geometry_field(&layout, "scrollbar_y"),
            text_height: geometry_field(&layout, "text_height"),
            scrollbar_height: geometry_field(&layout, "scrollbar_height"),
            scrollbar_right: geometry_field(&layout, "scrollbar_right"),
            scrollbar_bottom: geometry_field(&layout, "scrollbar_bottom"),
            text_bottom: geometry_field(&layout, "text_bottom"),
            hint_row_y: geometry_field(&layout, "hint_row_y"),
        }
    }
    fn custom_prompt_max_chars(&self) -> usize {
        usize::try_from(number(&query("custom-prompt-max-chars", json!({})))).unwrap_or(usize::MAX)
    }
    fn custom_prompt_max_serialized_bytes(&self) -> usize {
        usize::try_from(number(&query("custom-prompt-max-serialized-bytes", json!({})))).unwrap_or(usize::MAX)
    }
    fn declared_help_topics(&self) -> Vec<String> {
        texts(&query("declared-help-topics", json!({})))
    }
    fn help_anchor_topics(&mut self, target: ViewTarget) -> Vec<String> {
        self.help_anchors(target, screen_size().0, screen_size().1).into_iter().map(|(topic, _)| topic).collect()
    }
    fn help_anchors(&mut self, target: ViewTarget, columns: u16, rows: u16) -> Vec<(String, u16)> {
        list(act(self, "help-anchors", json!({ "target": target_name(&target), "columns": columns, "rows": rows })))
            .into_iter()
            .map(|anchor| (as_text(&anchor["topic"]), geometry_field(&anchor, "row")))
            .collect()
    }
    fn help_topic_exists(&self, topic: &str) -> bool {
        query("help-topic-exists", json!({ "topic": topic })).as_bool().unwrap_or(false)
    }
    fn help_topic_is_dialog_only(&self, topic: &str) -> bool {
        query("help-topic-is-dialog-only", json!({ "topic": topic })).as_bool().unwrap_or(false)
    }
    fn open_help_topic(&self) -> Option<String> {
        query("open-help-topic", json!({})).as_str().map(str::to_owned)
    }
    fn key_consumed(&mut self, target: ViewTarget, key: &str) -> bool {
        act(self, "key-consumed", json!({ "target": target_name(&target), "key": key })).as_bool().unwrap_or(false)
    }
    fn max_scroll(&mut self, target: ViewTarget, columns: u16, rows: u16) -> usize {
        usize::try_from(number(&act(self, "max-scroll", json!({ "target": target_name(&target), "columns": columns, "rows": rows })))).unwrap_or(usize::MAX)
    }
    fn scroll_offset(&self, target: ViewTarget) -> usize {
        usize::try_from(number(&query("scroll-offset", json!({ "target": target_name(&target) })))).unwrap_or(usize::MAX)
    }
    fn scroll_wheel(&mut self, target: ViewTarget, delta: i32) {
        act(self, "scroll-wheel", json!({ "target": target_name(&target), "delta": delta }));
    }
    fn open_settings_tab(&mut self, tab: &str) {
        act(self, "open-settings-tab", json!({ "tab": tab }));
    }
    fn render_settings(&mut self, columns: u16, rows: u16) -> Screen {
        let screen = self.render(ViewTarget::SettingsPage("remote-compaction".to_string()), columns, rows);
        remember(screen)
    }
    fn prompt_editor_text(&self) -> Option<String> {
        query("prompt-editor-text", json!({})).as_str().map(str::to_owned)
    }
    fn last_settings_command_bytes(&self) -> usize {
        usize::try_from(number(&query("last-settings-command-bytes", json!({})))).unwrap_or(usize::MAX)
    }
    fn icon_assignment_suggestions(&self) -> Vec<String> {
        texts(&query("icon-assignment-suggestions", json!({})))
    }
    fn icon_assignment_set(&mut self, glyph: &str) {
        act(self, "icon-assignment-set", json!({ "glyph": glyph }));
    }
    fn icon_assignment_click(&mut self, row: &str, action: &str) {
        act(self, "icon-assignment-click", json!({ "row": row, "action": action }));
    }
    fn icon_assignment_glyph(&self) -> String {
        as_text(&query("icon-assignment-glyph", json!({})))
    }
    fn icon_picker_open(&self) -> bool {
        query("icon-picker-open", json!({})).as_bool().unwrap_or(false)
    }
    fn sidebar_last_list_cell(&mut self, columns: u16, rows: u16, label: &str) -> ScreenCell {
        let screen = self.render(ViewTarget::Panel("sidebar".to_string()), columns, rows);
        let Some((row, _)) = screen.find(label) else {
            panic!("sidebar entry `{label}` is not on the screen:\n{}", screen.text());
        };
        screen.cell(row, columns - 1)
    }
    fn accent_background(&self) -> Color {
        Color::Role(ColorRole::Accent)
    }
    fn toolbar_buttons(&self, columns: u16, rows: u16) -> Vec<(String, u16, u16)> {
        list(query("toolbar-buttons", json!({ "columns": columns, "rows": rows })))
            .into_iter()
            .map(|button| (as_text(&button["action"]), geometry_field(&button, "column"), geometry_field(&button, "width")))
            .collect()
    }
    fn toolbar_action_at(&self, columns: u16, rows: u16, column: u16, row: u16) -> Option<String> {
        query("toolbar-action-at", json!({ "columns": columns, "rows": rows, "column": column, "row": row })).as_str().map(str::to_owned)
    }
    fn status_bar_text(&self, columns: u16) -> String {
        as_text(&query("status-bar-text", json!({ "columns": columns })))
    }
    fn set_codex_reset_watch_expiry(&mut self, in_: Duration, label: &str) {
        act(self, "set-codex-reset-watch-expiry", json!({ "in_ms": u64::try_from(in_.as_millis()).unwrap_or(u64::MAX), "label": label }));
    }
    fn set_screen_size(&mut self, columns: u16, rows: u16) {
        SIZE.with(|cell| *cell.borrow_mut() = (columns, rows));
        act(self, "set-screen-size", json!({ "columns": columns, "rows": rows }));
    }
    fn settings_row_geometry(&self, row: &str) -> RowGeometry {
        let geometry = query("settings-row-geometry", json!({ "row": row }));
        RowGeometry {
            box_column: geometry_field(&geometry, "box_column"),
            first_line: geometry_field(&geometry, "first_line"),
            control_column: geometry_field(&geometry, "control_column"),
            control_line: geometry_field(&geometry, "control_line"),
            next_column: geometry_field(&geometry, "next_column"),
            previous_column: geometry_field(&geometry, "previous_column"),
            open_column: geometry_field(&geometry, "open_column"),
            value_column: geometry_field(&geometry, "value_column"),
            value_right: geometry_field(&geometry, "value_right"),
            value_slot_width: geometry_field(&geometry, "value_slot_width"),
            value_width: geometry_field(&geometry, "value_width"),
        }
    }
    fn settings_hit(&self, column: u16, row: u16) -> Option<SettingsHit> {
        hit_from(&query("settings-hit", json!({ "column": column, "row": row })))
    }
    fn settings_hit_after_scroll(&mut self, column: u16, row: u16) -> Option<SettingsHit> {
        hit_from(&act(self, "settings-hit-after-scroll", json!({ "column": column, "row": row })))
    }
    fn settings_banner_close_rect(&self) -> Option<CloseRect> {
        close_rect_from(&query("settings-banner-close-rect", json!({})))
    }
    fn settings_banner_close_rect_at(&self, columns: u16) -> Option<CloseRect> {
        close_rect_from(&query("settings-banner-close-rect", json!({ "columns": columns })))
    }
    fn settings_banner_visible(&self) -> bool {
        query("settings-banner-visible", json!({})).as_bool().unwrap_or(false)
    }
    fn settings_row_span(&self, index: usize) -> RowSpan {
        RowSpan { last_line: usize::try_from(number(&query("settings-row-span", json!({ "index": index }))["last_line"])).unwrap_or(usize::MAX) }
    }
    fn settings_scroll_for_selection(&self, selected: usize, scroll: usize, rows: usize) -> usize {
        usize::try_from(number(&query("settings-scroll-for-selection", json!({ "selected": selected, "scroll": scroll, "rows": rows })))).unwrap_or(usize::MAX)
    }
    fn settings_max_scroll(&self, rows: usize, height: usize) -> usize {
        usize::try_from(number(&query("settings-max-scroll", json!({ "rows": rows, "height": height })))).unwrap_or(usize::MAX)
    }
    fn settings_help_anchors(&self) -> Vec<(String, usize)> {
        list(query("settings-help-anchors", json!({})))
            .into_iter()
            .map(|anchor| (as_text(&anchor["topic"]), usize::try_from(number(&anchor["row"])).unwrap_or(usize::MAX)))
            .collect()
    }
    fn settings_row_help_id(&self, row: &str) -> String {
        as_text(&query("settings-row-help-id", json!({ "row": row })))
    }
    fn settings_painted_control(&self, row: &str, columns: u16) -> String {
        let screen = last_screen();
        let columns = columns.min(screen.columns);
        let geometry = self.settings_row_geometry(row);
        let line = screen.row(geometry.control_line);
        line.chars().skip(usize::from(geometry.control_column)).take(usize::from(columns)).collect()
    }
    fn settings_painted_glyph(&self, column: u16, row: u16) -> char {
        let screen = last_screen();
        screen.cell(row, column).symbol.chars().next().unwrap_or(' ')
    }
    fn settings_page_lines(&self, columns: u16) -> Vec<String> {
        texts(&query("settings-page-lines", json!({ "columns": columns })))
    }
    fn settings_page_text(&self, columns: u16) -> String {
        self.settings_page_lines(columns).join("\n")
    }
    fn settings_privacy_banner_text(&self, settings: Value) -> String {
        as_text(&query("settings-privacy-banner-text", json!({ "settings": settings })))
    }
    fn settings_row_labels(&self) -> Vec<String> {
        texts(&query("settings-row-labels", json!({})))
    }
    fn settings_section_headings(&self) -> Vec<String> {
        texts(&query("settings-section-headings", json!({})))
    }
    fn settings_rows(&self) -> Vec<String> {
        texts(&query("settings-rows", json!({})))
    }
    fn settings_selected_row(&self) -> usize {
        usize::try_from(number(&query("settings-selected-row", json!({})))).unwrap_or(usize::MAX)
    }
    fn settings_row_index(&self, row: &str) -> usize {
        let rows = self.settings_rows();
        rows.iter().position(|name| name == row).unwrap_or_else(|| panic!("unknown settings row `{row}`; rows: {rows:?}"))
    }
    fn settings_save_pending(&self) -> bool {
        query("settings-save-pending", json!({})).as_bool().unwrap_or(false)
    }
    fn settings_write_count(&self) -> usize {
        usize::try_from(number(&query("settings-write-count", json!({})))).unwrap_or(usize::MAX)
    }
    fn settings_set_value(&mut self, row: &str, value: Value) {
        act(self, "settings-set-value", json!({ "row": row, "value": value }));
    }
    fn settings_value(&self, row: &str) -> Value {
        query("settings-value", json!({ "row": row }))
    }
    fn settings_dialog(&self) -> Option<Value> {
        let dialog = query("settings-dialog", json!({}));
        if dialog.is_null() { None } else { Some(dialog) }
    }
    fn settings_dialog_choose(&mut self, option: &str) {
        act(self, "settings-dialog-choose", json!({ "option": option }));
    }
    fn settings_dialog_click_option(&mut self, index: usize) {
        act(self, "settings-dialog-click-option", json!({ "index": index }));
    }
    fn settings_dialog_key(&mut self, key: &str) {
        act(self, "settings-dialog-key", json!({ "key": key }));
    }
    fn settings_dismiss_banner(&mut self) {
        act(self, "settings-dismiss-banner", json!({}));
    }
    fn settings_clear_prompt(&mut self) {
        act(self, "settings-clear-prompt", json!({}));
    }
    fn settings_commit_prompt(&mut self, text: &str) {
        act(self, "settings-commit-prompt", json!({ "text": text }));
    }
    fn settings_click(&mut self, column: u16, row: u16) {
        self.click(ViewTarget::SettingsPage("remote-compaction".to_string()), column, row);
        mirror_input("settings-click", json!({ "column": column, "row": row }));
    }
    fn settings_key(&mut self, key: &str) {
        self.key(ViewTarget::SettingsPage("remote-compaction".to_string()), key);
        mirror_input("settings-key", json!({ "key": key }));
    }
    fn settings_adjust_row(&mut self, row: &str, delta: i32) {
        act(self, "settings-adjust-row", json!({ "row": row, "delta": delta }));
    }
    fn settings_serialized_bound_ok(&self, settings: &Value) -> bool {
        query("settings-serialized-bound-ok", json!({ "settings": settings })).as_bool().unwrap_or(false)
    }
    fn remote_settings_adjust(&mut self, row: &str, delta: i32) -> bool {
        act(self, "remote-settings-adjust", json!({ "row": row, "delta": delta })).as_bool().unwrap_or(false)
    }
    fn remote_settings_current(&self) -> Value {
        query("remote-settings-current", json!({}))
    }
    fn remote_settings_defaults(&self) -> Value {
        query("remote-settings-defaults", json!({}))
    }
    fn remote_settings_help_ids(&self) -> Vec<String> {
        texts(&query("remote-settings-help-ids", json!({})))
    }
    fn remote_settings_help_topic_count(&self) -> usize {
        usize::try_from(number(&query("remote-settings-help-topic-count", json!({})))).unwrap_or(usize::MAX)
    }
    fn remote_settings_options_for(&self, settings: &Value, row: &str) -> Value {
        query("remote-settings-options-for", json!({ "settings": settings, "row": row }))
    }
    fn remote_settings_row_ids(&self) -> Vec<String> {
        texts(&query("remote-settings-row-ids", json!({})))
    }
    fn remote_settings_sanitize(&self, settings: Value) -> Value {
        query("remote-settings-sanitize", settings)
    }
    fn remote_settings_set(&mut self, settings: Value) {
        act(self, "remote-settings-set", settings);
    }
    fn remote_settings_technique_for(&self, settings: &Value, agent: &str) -> String {
        as_text(&query("remote-settings-technique-for", json!({ "settings": settings, "agent": agent })))
    }
    fn default_summarizer_max_output_tokens(&self) -> u64 {
        number(&query("default-summarizer-max-output-tokens", json!({})))
    }
    fn flow_start(&mut self, agent: &str, technique: &str) {
        act(self, "flow-start", json!({ "agent": agent, "technique": technique }));
    }
    fn flow_event(&mut self, event: Value) {
        act(self, "flow-event", event);
    }
    fn flow_key(&mut self, key: &str) {
        act(self, "flow-key", json!({ "key": key }));
    }
    fn flow_pending_job(&self) -> Option<Value> {
        let job = query("flow-pending-job", json!({}));
        if job.is_null() { None } else { Some(job) }
    }
    fn flow_replaces_native_compact(&self, pane: u64) -> bool {
        query("flow-replaces-native-compact", json!({ "pane": pane })).as_bool().unwrap_or(false)
    }
    fn flow_requests(&self) -> Vec<Value> {
        list(query("flow-requests", json!({})))
    }
    fn flow_set_agent_stopped(&mut self, stopped: bool) {
        act(self, "flow-set-agent-stopped", json!({ "stopped": stopped }));
    }
    fn flow_set_phase(&mut self, phase: &str) {
        act(self, "flow-set-phase", json!({ "phase": phase }));
    }
    fn flow_state(&self) -> Value {
        query("flow-state", json!({}))
    }
    fn flow_take_cancel(&mut self) -> bool {
        act(self, "flow-take-cancel", json!({})).as_bool().unwrap_or(false)
    }
    fn flow_try_start(&mut self, pane: u64, force: bool) -> bool {
        act(self, "flow-try-start", json!({ "pane": pane, "force": force })).as_bool().unwrap_or(false)
    }
    fn monitor_record_attempt(&mut self, pane: u64, at_ms: u64) {
        act(self, "monitor-record-attempt", json!({ "pane": pane, "at_ms": at_ms }));
    }
    fn monitor_record_failure(&mut self, pane: u64) {
        act(self, "monitor-record-failure", json!({ "pane": pane }));
    }
    fn monitor_record_success(&mut self, pane: u64) {
        act(self, "monitor-record-success", json!({ "pane": pane }));
    }
    fn monitor_should_trigger(&mut self, pane: u64, used: f64, threshold: f64, now_ms: u64, cooldown_ms: u64) -> bool {
        act(self, "monitor-should-trigger", json!({ "pane": pane, "used": used, "threshold": threshold, "now_ms": now_ms, "cooldown_ms": cooldown_ms })).as_bool().unwrap_or(false)
    }
    fn native_trigger_evaluate(&self, agent: &str, state: Value, used: u64, limit: u64) -> Value {
        query("native-trigger-evaluate", json!({ "agent": agent, "state": state, "used": used, "limit": limit }))
    }
    fn native_trigger_detect(&mut self, agent: &str) -> Value {
        act(self, "native-trigger-detect", json!({ "agent": agent }))
    }
    fn dialog_banner_close_width(&self) -> u16 {
        geometry_field(&query("dialog-banner-close-width", json!({})), "width")
    }
    fn dialog_event(&mut self, event: Value) {
        act(self, "dialog-event", event);
    }
    fn dialog_fail(&mut self, message: &str) {
        act(self, "dialog-fail", json!({ "message": message }));
    }
    fn dialog_hint(&self) -> String {
        as_text(&query("dialog-hint", json!({})))
    }
    fn dialog_open(&mut self, request: Value) {
        act(self, "dialog-open", request);
    }
    fn dialog_render(&self, columns: u16, rows: u16, focus: Option<&str>) -> String {
        as_text(&query("dialog-render", json!({ "columns": columns, "rows": rows, "focus": focus })))
    }
    fn dialog_set_agent_stopped(&mut self, stopped: bool) {
        act(self, "dialog-set-agent-stopped", json!({ "stopped": stopped }));
    }
    fn dialog_state(&self) -> Value {
        query("dialog-state", json!({}))
    }
    fn dialog_token_bar_lines(&self, state: Value, width: usize) -> Vec<String> {
        texts(&query("dialog-token-bar-lines", json!({ "state": state, "width": width })))
    }
    fn worker_annotate_progress_overflow(&self, result: Result<Value, String>, overflow: bool) -> Result<Value, String> {
        let (ok, value) = match &result {
            Ok(value) => (true, value.clone()),
            Err(message) => (false, json!(message)),
        };
        let reply = query("worker-annotate-progress-overflow", json!({ "ok": ok, "value": value, "overflow": overflow }));
        if reply["ok"].as_bool().unwrap_or(false) { Ok(reply["value"].clone()) } else { Err(as_text(&reply["value"])) }
    }
    fn worker_classify_inference_error(&self, error: Value) -> String {
        as_text(&query("worker-classify-inference-error", json!({ "error": error })))
    }
    fn worker_await_pause(&self, agent: &str, reason: &str, timeout: Duration, poll: Duration, interruptible: bool) -> PauseOutcome {
        let reply = query("worker-await-pause", json!({ "agent": agent, "reason": reason, "timeout_ms": u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX), "poll_ms": u64::try_from(poll.as_millis()).unwrap_or(u64::MAX), "interruptible": interruptible }));
        let result = if reply["ok"].as_bool().unwrap_or(false) { Ok(reply["value"].clone()) } else { Err(as_text(&reply["value"])) };
        PauseOutcome { result, interrupts: u32::try_from(number(&reply["interrupts"])).unwrap_or(u32::MAX) }
    }
    fn worker_cancel(&mut self) {
        act(self, "worker-cancel", json!({}));
    }
    fn worker_cancel_observed(&self, within: Duration) -> bool {
        query("worker-cancel-observed", json!({ "within_ms": u64::try_from(within.as_millis()).unwrap_or(u64::MAX) })).as_bool().unwrap_or(false)
    }
    fn worker_fill_event_queue(&mut self) {
        act(self, "worker-fill-event-queue", json!({}));
    }
    fn worker_idle(&self, within: Duration) -> bool {
        query("worker-idle", json!({ "within_ms": u64::try_from(within.as_millis()).unwrap_or(u64::MAX) })).as_bool().unwrap_or(false)
    }
    fn worker_pause_probe_cost_input_bytes(&self) -> u64 {
        number(&query("worker-pause-probe-cost-input-bytes", json!({})))
    }
    fn worker_start_pause_job(&mut self, agent: &str, reason: &str, timeout: Duration) {
        act(self, "worker-start-pause-job", json!({ "agent": agent, "reason": reason, "timeout_ms": u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX) }));
    }
    fn worker_wait_interrupt_pending(&self, within: Duration) -> bool {
        query("worker-wait-interrupt-pending", json!({ "within_ms": u64::try_from(within.as_millis()).unwrap_or(u64::MAX) })).as_bool().unwrap_or(false)
    }
}
