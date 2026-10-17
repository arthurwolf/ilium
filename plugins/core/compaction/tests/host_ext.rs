//! Compaction-lane testing hooks on `FakeHost`, kept out of the shared harness type.
//!
//! Every method is plugin-specific scaffolding (report engine, scan engine, settings page,
//! remote dialog, worker and flow state) that only this lane needs. Bodies are skeletons:
//! `unimplemented!()` until the plugin exists. Shared harness behaviour stays in
//! `ilium_plugin_sdk::testing::FakeHost`.

#![allow(dead_code)]

use std::fmt;
use std::time::Duration;

use ilium_plugin_sdk::testing::{FakeHost, Screen, ScreenCell};
use ilium_plugin_sdk::types::{Color, PluginError, Submission, ViewTarget};
use serde_json::Value;

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

/// Agent config files seen by the plugin, through the host.
pub struct AgentFiles<'a>(pub &'a mut FakeHost);

impl AgentFiles<'_> {
    /// Writes a file.
    pub fn write(&mut self, _path: &str, _bytes: &[u8]) {
        unimplemented!("compaction.agent-files-write")
    }

    /// Reads a file.
    pub fn read(&self, _path: &str) -> Option<Vec<u8>> {
        unimplemented!("compaction.agent-files-read")
    }
}

/// Compaction-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait CompactionHostExt {
    // ---- report engine ----
    fn build_report(&mut self, _input: Value) -> Result<Value, String> {
        unimplemented!("compaction.build-report")
    }
    fn price_weights(&self, _model: &str) -> Option<Value> {
        unimplemented!("compaction.price-weights")
    }
    fn default_grid(&self, _agent: &str) -> Vec<u64> {
        unimplemented!("compaction.default-grid")
    }
    fn format_value(&self, _kind: &str, _value: f64) -> String {
        unimplemented!("compaction.format-value")
    }
    fn format_token_quantiles(&self, _count: u32, _p25: f64, _p50: f64, _p75: f64) -> String {
        unimplemented!("compaction.format-token-quantiles")
    }
    fn widening_triggers(&self, _agent: &str, _grid: &[u64], _current: u64) -> Vec<u64> {
        unimplemented!("compaction.widening-triggers")
    }

    // ---- scan engine ----
    fn scan_agent_homes(
        &mut self,
        _agent: &str,
        _home: Option<&str>,
        _limit: Option<usize>,
        _timeout_ms: Option<u32>,
    ) -> Result<Value, ScanError> {
        unimplemented!("compaction.scan-agent-homes")
    }
    fn scan_agent_homes_growing(
        &mut self,
        _agent: &str,
        _home: &str,
        _path: &str,
    ) -> Result<Value, ScanError> {
        unimplemented!("compaction.scan-agent-homes-growing")
    }
    fn scan_agent_homes_shrinking(
        &mut self,
        _agent: &str,
        _home: &str,
        _keep: usize,
    ) -> Result<Value, ScanError> {
        unimplemented!("compaction.scan-agent-homes-shrinking")
    }
    fn scan_agent_homes_vanishing(
        &mut self,
        _agent: &str,
        _home: &str,
    ) -> Result<Value, ScanError> {
        unimplemented!("compaction.scan-agent-homes-vanishing")
    }
    fn scan_agent_homes_with_current(
        &mut self,
        _agent: &str,
        _current: u64,
    ) -> Result<Value, ScanError> {
        unimplemented!("compaction.scan-agent-homes-with-current")
    }
    fn scan_event_stream(&mut self, _agent: &str) -> Vec<Value> {
        unimplemented!("compaction.scan-event-stream")
    }
    fn scan_progress_reports(&mut self, _agent: &str) -> Vec<Value> {
        unimplemented!("compaction.scan-progress-reports")
    }
    fn list_transcripts(&mut self, _agent: &str) -> Vec<Value> {
        unimplemented!("compaction.list-transcripts")
    }
    fn list_transcripts_with_stop(&mut self, _agent: &str) -> Vec<Value> {
        unimplemented!("compaction.list-transcripts-with-stop")
    }
    fn listing_warnings(&self) -> Vec<String> {
        unimplemented!("compaction.listing-warnings")
    }
    fn last_report(&self, _agent: &str) -> Value {
        unimplemented!("compaction.last-report")
    }
    fn default_cache_path(&self, _agent: &str) -> String {
        unimplemented!("compaction.default-cache-path")
    }
    fn single_trace_turns(&mut self, _agent: &str, _path: &str) -> u64 {
        unimplemented!("compaction.single-trace-turns")
    }
    fn touch(&mut self, _agent: &str, _path: &str) {
        unimplemented!("compaction.touch")
    }
    fn disable_inference_and_agent_access(&mut self) {
        unimplemented!("compaction.disable-inference-and-agent-access")
    }
    fn read_bounded_lines(&self, _bytes: &[u8], _limit: usize) -> Vec<Value> {
        unimplemented!("compaction.read-bounded-lines")
    }
    fn read_bounded_lines_stop_after(
        &self,
        _bytes: &[u8],
        _limit: usize,
        _stop_after: usize,
    ) -> Vec<Value> {
        unimplemented!("compaction.read-bounded-lines-stop-after")
    }
    fn is_scanning(&self, _agent: &str) -> bool {
        unimplemented!("compaction.is-scanning")
    }
    fn scans_started(&self) -> Vec<Value> {
        unimplemented!("compaction.scans-started")
    }
    fn scan_request(&self, _index: usize) -> Value {
        unimplemented!("compaction.scan-request")
    }
    fn finish_scans(&mut self) {
        unimplemented!("compaction.finish-scans")
    }
    fn set_scan_cancelled(&mut self) {
        unimplemented!("compaction.set-scan-cancelled")
    }
    fn set_scan_failure(&mut self, _message: &str) {
        unimplemented!("compaction.set-scan-failure")
    }
    fn set_scan_result(&mut self, _sessions: u64, _turns: u64) {
        unimplemented!("compaction.set-scan-result")
    }
    fn set_recommendation(&mut self, _tokens: u64, _alternative: u64) {
        unimplemented!("compaction.set-recommendation")
    }
    fn recommended_tokens(&self) -> u64 {
        unimplemented!("compaction.recommended-tokens")
    }
    fn extrapolated_alternative_tokens(&self) -> Option<u64> {
        unimplemented!("compaction.extrapolated-alternative-tokens")
    }
    fn select_agent_target(&mut self, _target: &str) {
        unimplemented!("compaction.select-agent-target")
    }
    fn agent_file_writes(&self) -> Vec<String> {
        unimplemented!("compaction.agent-file-writes")
    }
    fn agent_files(&mut self) -> AgentFiles<'_> {
        unimplemented!("compaction.agent-files")
    }
    fn set_note(&mut self, _agent: &str, _note: &str) {
        unimplemented!("compaction.set-note")
    }
    fn set_progress(&mut self, _agent: &str, _done: u64, _total: u64, _message: &str) {
        unimplemented!("compaction.set-progress")
    }
    fn set_report(&mut self, _report_json: &str) {
        unimplemented!("compaction.set-report")
    }
    fn set_revert_record(&mut self, _previous: u64, _current: u64) {
        unimplemented!("compaction.set-revert-record")
    }
    fn clear_revert_record(&mut self) {
        unimplemented!("compaction.clear-revert-record")
    }
    fn transcript_format_for(&self, _agent: &str) -> Option<String> {
        unimplemented!("compaction.transcript-format-for")
    }
    fn technique_ids(&self) -> Vec<String> {
        unimplemented!("compaction.technique-ids")
    }
    fn technique_description(&self, _id: &str) -> String {
        unimplemented!("compaction.technique-description")
    }
    fn technique_fidelity(&self, _id: &str) -> String {
        unimplemented!("compaction.technique-fidelity")
    }
    fn convert_session(&mut self, _from: &str, _to: &str, _path: &str) -> Vec<Submission> {
        unimplemented!("compaction.convert-session")
    }
    fn convert_session_failing(&mut self, _from: &str, _to: &str) -> Vec<Submission> {
        unimplemented!("compaction.convert-session-failing")
    }
    fn cancel_job_after_chunks(&mut self, _chunks: usize) {
        unimplemented!("compaction.cancel-job-after-chunks")
    }
    fn job_progress_reports(&self) -> Vec<f64> {
        unimplemented!("compaction.job-progress-reports")
    }
    fn run_job_for_pane(
        &mut self,
        _pane: u64,
        _kind: &str,
        _input: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("compaction.run-job-for-pane")
    }

    // ---- ui pages ----
    fn bar_parts(&self, _fraction: f64, _width: usize) -> (String, String) {
        unimplemented!("compaction.bar-parts")
    }
    fn kept_columns(&self, _columns: &[(&str, u16, usize)], _width: usize) -> Vec<String> {
        unimplemented!("compaction.kept-columns")
    }
    fn wrap_text(&self, _text: &str, _width: usize) -> Vec<String> {
        unimplemented!("compaction.wrap-text")
    }
    fn truncate_to(&self, _text: &str, _width: usize) -> String {
        unimplemented!("compaction.truncate-to")
    }
    fn button_is_filled(&mut self, _target: ViewTarget, _label: &str) -> bool {
        unimplemented!("compaction.button-is-filled")
    }
    fn close_confirmation(&mut self) {
        unimplemented!("compaction.close-confirmation")
    }
    fn click_help_anchor(&mut self, _target: ViewTarget, _topic: &str) {
        unimplemented!("compaction.click-help-anchor")
    }
    fn open_view(&mut self, _target: ViewTarget) {
        unimplemented!("compaction.open-view")
    }
    fn close_view(&mut self, _target: ViewTarget) {
        unimplemented!("compaction.close-view")
    }
    fn view_closed(&self, _target: ViewTarget) -> bool {
        unimplemented!("compaction.view-closed")
    }
    fn confirmation_layout(
        &mut self,
        _target: ViewTarget,
        _columns: u16,
        _rows: u16,
    ) -> ConfirmationLayout {
        unimplemented!("compaction.confirmation-layout")
    }
    fn custom_prompt_max_chars(&self) -> usize {
        unimplemented!("compaction.custom-prompt-max-chars")
    }
    fn custom_prompt_max_serialized_bytes(&self) -> usize {
        unimplemented!("compaction.custom-prompt-max-serialized-bytes")
    }
    fn declared_help_topics(&self) -> Vec<String> {
        unimplemented!("compaction.declared-help-topics")
    }
    fn help_anchor_topics(&mut self, _target: ViewTarget) -> Vec<String> {
        unimplemented!("compaction.help-anchor-topics")
    }
    fn help_anchors(
        &mut self,
        _target: ViewTarget,
        _columns: u16,
        _rows: u16,
    ) -> Vec<(String, u16)> {
        unimplemented!("compaction.help-anchors")
    }
    fn help_topic_exists(&self, _topic: &str) -> bool {
        unimplemented!("compaction.help-topic-exists")
    }
    fn help_topic_is_dialog_only(&self, _topic: &str) -> bool {
        unimplemented!("compaction.help-topic-is-dialog-only")
    }
    fn open_help_topic(&self) -> Option<String> {
        unimplemented!("compaction.open-help-topic")
    }
    fn key_consumed(&mut self, _target: ViewTarget, _key: &str) -> bool {
        unimplemented!("compaction.key-consumed")
    }
    fn max_scroll(&mut self, _target: ViewTarget, _columns: u16, _rows: u16) -> usize {
        unimplemented!("compaction.max-scroll")
    }
    fn scroll_offset(&self, _target: ViewTarget) -> usize {
        unimplemented!("compaction.scroll-offset")
    }
    fn scroll_wheel(&mut self, _target: ViewTarget, _delta: i32) {
        unimplemented!("compaction.scroll-wheel")
    }
    fn open_settings_tab(&mut self, _tab: &str) {
        unimplemented!("compaction.open-settings-tab")
    }
    fn render_settings(&mut self, _columns: u16, _rows: u16) -> Screen {
        unimplemented!("compaction.render-settings")
    }
    fn prompt_editor_text(&self) -> Option<String> {
        unimplemented!("compaction.prompt-editor-text")
    }
    fn last_settings_command_bytes(&self) -> usize {
        unimplemented!("compaction.last-settings-command-bytes")
    }
    fn icon_assignment_suggestions(&self) -> Vec<String> {
        unimplemented!("compaction.icon-assignment-suggestions")
    }
    fn icon_assignment_set(&mut self, _glyph: &str) {
        unimplemented!("compaction.icon-assignment-set")
    }
    fn icon_assignment_click(&mut self, _row: &str, _action: &str) {
        unimplemented!("compaction.icon-assignment-click")
    }
    fn icon_assignment_glyph(&self) -> String {
        unimplemented!("compaction.icon-assignment-glyph")
    }
    fn icon_picker_open(&self) -> bool {
        unimplemented!("compaction.icon-picker-open")
    }
    fn sidebar_last_list_cell(&mut self, _columns: u16, _rows: u16, _label: &str) -> ScreenCell {
        unimplemented!("compaction.sidebar-last-list-cell")
    }
    fn accent_background(&self) -> Color {
        unimplemented!("compaction.accent-background")
    }
    fn toolbar_buttons(&self, _columns: u16, _rows: u16) -> Vec<(String, u16, u16)> {
        unimplemented!("compaction.toolbar-buttons")
    }
    fn toolbar_action_at(
        &self,
        _columns: u16,
        _rows: u16,
        _column: u16,
        _row: u16,
    ) -> Option<String> {
        unimplemented!("compaction.toolbar-action-at")
    }
    fn status_bar_text(&self, _columns: u16) -> String {
        unimplemented!("compaction.status-bar-text")
    }
    fn set_codex_reset_watch_expiry(&mut self, _in: Duration, _label: &str) {
        unimplemented!("compaction.set-codex-reset-watch-expiry")
    }
    fn set_screen_size(&mut self, _columns: u16, _rows: u16) {
        unimplemented!("compaction.set-screen-size")
    }

    // ---- settings page ----
    fn settings_row_geometry(&self, _row: &str) -> RowGeometry {
        unimplemented!("compaction.settings-row-geometry")
    }
    fn settings_hit(&self, _column: u16, _row: u16) -> Option<SettingsHit> {
        unimplemented!("compaction.settings-hit")
    }
    fn settings_hit_after_scroll(&mut self, _column: u16, _row: u16) -> Option<SettingsHit> {
        unimplemented!("compaction.settings-hit-after-scroll")
    }
    fn settings_banner_close_rect(&self) -> Option<CloseRect> {
        unimplemented!("compaction.settings-banner-close-rect")
    }
    fn settings_banner_close_rect_at(&self, _columns: u16) -> Option<CloseRect> {
        unimplemented!("compaction.settings-banner-close-rect-at")
    }
    fn settings_banner_visible(&self) -> bool {
        unimplemented!("compaction.settings-banner-visible")
    }
    fn settings_row_span(&self, _index: usize) -> RowSpan {
        unimplemented!("compaction.settings-row-span")
    }
    fn settings_scroll_for_selection(
        &self,
        _selected: usize,
        _scroll: usize,
        _rows: usize,
    ) -> usize {
        unimplemented!("compaction.settings-scroll-for-selection")
    }
    fn settings_max_scroll(&self, _rows: usize, _height: usize) -> usize {
        unimplemented!("compaction.settings-max-scroll")
    }
    fn settings_help_anchors(&self) -> Vec<(String, usize)> {
        unimplemented!("compaction.settings-help-anchors")
    }
    fn settings_row_help_id(&self, _row: &str) -> String {
        unimplemented!("compaction.settings-row-help-id")
    }
    fn settings_painted_control(&self, _row: &str, _columns: u16) -> String {
        unimplemented!("compaction.settings-painted-control")
    }
    fn settings_painted_glyph(&self, _column: u16, _row: u16) -> char {
        unimplemented!("compaction.settings-painted-glyph")
    }
    fn settings_page_lines(&self, _columns: u16) -> Vec<String> {
        unimplemented!("compaction.settings-page-lines")
    }
    fn settings_page_text(&self, _columns: u16) -> String {
        unimplemented!("compaction.settings-page-text")
    }
    fn settings_privacy_banner_text(&self, _settings: Value) -> String {
        unimplemented!("compaction.settings-privacy-banner-text")
    }
    fn settings_row_labels(&self) -> Vec<String> {
        unimplemented!("compaction.settings-row-labels")
    }
    fn settings_section_headings(&self) -> Vec<String> {
        unimplemented!("compaction.settings-section-headings")
    }
    fn settings_rows(&self) -> Vec<String> {
        unimplemented!("compaction.settings-rows")
    }
    fn settings_selected_row(&self) -> usize {
        unimplemented!("compaction.settings-selected-row")
    }
    fn settings_row_index(&self, _row: &str) -> usize {
        unimplemented!("compaction.settings-row-index")
    }
    fn settings_save_pending(&self) -> bool {
        unimplemented!("compaction.settings-save-pending")
    }
    fn settings_write_count(&self) -> usize {
        unimplemented!("compaction.settings-write-count")
    }
    fn settings_set_value(&mut self, _row: &str, _value: Value) {
        unimplemented!("compaction.settings-set-value")
    }
    fn settings_value(&self, _row: &str) -> Value {
        unimplemented!("compaction.settings-value")
    }
    fn settings_dialog(&self) -> Option<Value> {
        unimplemented!("compaction.settings-dialog")
    }
    fn settings_dialog_choose(&mut self, _option: &str) {
        unimplemented!("compaction.settings-dialog-choose")
    }
    fn settings_dialog_click_option(&mut self, _index: usize) {
        unimplemented!("compaction.settings-dialog-click-option")
    }
    fn settings_dialog_key(&mut self, _key: &str) {
        unimplemented!("compaction.settings-dialog-key")
    }
    fn settings_dismiss_banner(&mut self) {
        unimplemented!("compaction.settings-dismiss-banner")
    }
    fn settings_clear_prompt(&mut self) {
        unimplemented!("compaction.settings-clear-prompt")
    }
    fn settings_commit_prompt(&mut self, _text: &str) {
        unimplemented!("compaction.settings-commit-prompt")
    }
    fn settings_click(&mut self, _column: u16, _row: u16) {
        unimplemented!("compaction.settings-click")
    }
    fn settings_key(&mut self, _key: &str) {
        unimplemented!("compaction.settings-key")
    }
    fn settings_adjust_row(&mut self, _row: &str, _delta: i32) {
        unimplemented!("compaction.settings-adjust-row")
    }
    fn settings_serialized_bound_ok(&self, _settings: &Value) -> bool {
        unimplemented!("compaction.settings-serialized-bound-ok")
    }

    // ---- remote settings ----
    fn remote_settings_adjust(&mut self, _row: &str, _delta: i32) -> bool {
        unimplemented!("compaction.remote-settings-adjust")
    }
    fn remote_settings_current(&self) -> Value {
        unimplemented!("compaction.remote-settings-current")
    }
    fn remote_settings_defaults(&self) -> Value {
        unimplemented!("compaction.remote-settings-defaults")
    }
    fn remote_settings_help_ids(&self) -> Vec<String> {
        unimplemented!("compaction.remote-settings-help-ids")
    }
    fn remote_settings_help_topic_count(&self) -> usize {
        unimplemented!("compaction.remote-settings-help-topic-count")
    }
    fn remote_settings_options_for(&self, _settings: &Value, _row: &str) -> Value {
        unimplemented!("compaction.remote-settings-options-for")
    }
    fn remote_settings_row_ids(&self) -> Vec<String> {
        unimplemented!("compaction.remote-settings-row-ids")
    }
    fn remote_settings_sanitize(&self, _settings: Value) -> Value {
        unimplemented!("compaction.remote-settings-sanitize")
    }
    fn remote_settings_set(&mut self, _settings: Value) {
        unimplemented!("compaction.remote-settings-set")
    }
    fn remote_settings_technique_for(&self, _settings: &Value, _agent: &str) -> String {
        unimplemented!("compaction.remote-settings-technique-for")
    }
    fn default_summarizer_max_output_tokens(&self) -> u64 {
        unimplemented!("compaction.default-summarizer-max-output-tokens")
    }

    // ---- flow and monitor ----
    fn flow_start(&mut self, _agent: &str, _technique: &str) {
        unimplemented!("compaction.flow-start")
    }
    fn flow_event(&mut self, _event: Value) {
        unimplemented!("compaction.flow-event")
    }
    fn flow_key(&mut self, _key: &str) {
        unimplemented!("compaction.flow-key")
    }
    fn flow_pending_job(&self) -> Option<Value> {
        unimplemented!("compaction.flow-pending-job")
    }
    fn flow_replaces_native_compact(&self, _pane: u64) -> bool {
        unimplemented!("compaction.flow-replaces-native-compact")
    }
    fn flow_requests(&self) -> Vec<Value> {
        unimplemented!("compaction.flow-requests")
    }
    fn flow_set_agent_stopped(&mut self, _stopped: bool) {
        unimplemented!("compaction.flow-set-agent-stopped")
    }
    fn flow_set_phase(&mut self, _phase: &str) {
        unimplemented!("compaction.flow-set-phase")
    }
    fn flow_state(&self) -> Value {
        unimplemented!("compaction.flow-state")
    }
    fn flow_take_cancel(&mut self) -> bool {
        unimplemented!("compaction.flow-take-cancel")
    }
    fn flow_try_start(&mut self, _pane: u64, _force: bool) -> bool {
        unimplemented!("compaction.flow-try-start")
    }
    fn monitor_record_attempt(&mut self, _pane: u64, _at_ms: u64) {
        unimplemented!("compaction.monitor-record-attempt")
    }
    fn monitor_record_failure(&mut self, _pane: u64) {
        unimplemented!("compaction.monitor-record-failure")
    }
    fn monitor_record_success(&mut self, _pane: u64) {
        unimplemented!("compaction.monitor-record-success")
    }
    fn monitor_should_trigger(
        &mut self,
        _pane: u64,
        _used: f64,
        _threshold: f64,
        _now_ms: u64,
        _cooldown_ms: u64,
    ) -> bool {
        unimplemented!("compaction.monitor-should-trigger")
    }
    fn native_trigger_evaluate(
        &self,
        _agent: &str,
        _state: Value,
        _used: u64,
        _limit: u64,
    ) -> Value {
        unimplemented!("compaction.native-trigger-evaluate")
    }
    fn native_trigger_detect(&mut self, _agent: &str) -> Value {
        unimplemented!("compaction.native-trigger-detect")
    }

    // ---- remote dialog ----
    fn dialog_banner_close_width(&self) -> u16 {
        unimplemented!("compaction.dialog-banner-close-width")
    }
    fn dialog_event(&mut self, _event: Value) {
        unimplemented!("compaction.dialog-event")
    }
    fn dialog_fail(&mut self, _message: &str) {
        unimplemented!("compaction.dialog-fail")
    }
    fn dialog_hint(&self) -> String {
        unimplemented!("compaction.dialog-hint")
    }
    fn dialog_open(&mut self, _request: Value) {
        unimplemented!("compaction.dialog-open")
    }
    fn dialog_render(&self, _columns: u16, _rows: u16, _focus: Option<&str>) -> String {
        unimplemented!("compaction.dialog-render")
    }
    fn dialog_set_agent_stopped(&mut self, _stopped: bool) {
        unimplemented!("compaction.dialog-set-agent-stopped")
    }
    fn dialog_state(&self) -> Value {
        unimplemented!("compaction.dialog-state")
    }
    fn dialog_token_bar_lines(&self, _state: Value, _width: usize) -> Vec<String> {
        unimplemented!("compaction.dialog-token-bar-lines")
    }

    // ---- remote worker ----
    fn worker_annotate_progress_overflow(
        &self,
        _result: Result<Value, String>,
        _overflow: bool,
    ) -> Result<Value, String> {
        unimplemented!("compaction.worker-annotate-progress-overflow")
    }
    fn worker_classify_inference_error(&self, _error: Value) -> String {
        unimplemented!("compaction.worker-classify-inference-error")
    }
    fn worker_await_pause(
        &self,
        _agent: &str,
        _reason: &str,
        _timeout: Duration,
        _poll: Duration,
        _interruptible: bool,
    ) -> PauseOutcome {
        unimplemented!("compaction.worker-await-pause")
    }
    fn worker_cancel(&mut self) {
        unimplemented!("compaction.worker-cancel")
    }
    fn worker_cancel_observed(&self, _within: Duration) -> bool {
        unimplemented!("compaction.worker-cancel-observed")
    }
    fn worker_fill_event_queue(&mut self) {
        unimplemented!("compaction.worker-fill-event-queue")
    }
    fn worker_idle(&self, _within: Duration) -> bool {
        unimplemented!("compaction.worker-idle")
    }
    fn worker_pause_probe_cost_input_bytes(&self) -> u64 {
        unimplemented!("compaction.worker-pause-probe-cost-input-bytes")
    }
    fn worker_start_pause_job(&mut self, _agent: &str, _reason: &str, _timeout: Duration) {
        unimplemented!("compaction.worker-start-pause-job")
    }
    fn worker_wait_interrupt_pending(&self, _within: Duration) -> bool {
        unimplemented!("compaction.worker-wait-interrupt-pending")
    }
}

impl CompactionHostExt for FakeHost {}
