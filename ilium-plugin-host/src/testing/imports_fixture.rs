//! Imports fixture: one plugin instance with a manifest, grants, a project directory, a manual
//! clock and limits, exposing every host import bound to that instance (test plan lane
//! imports-core, SET-*, WIT:ilium:host/*). Test support only.

use std::path::Path;
use std::time::Duration;

use ilium_plugin_api::generated::host::{
    Bucket, ChildHandle, FieldError, HostClock, HostCodec, HostFs, HostHttp, HostJobs, HostJournal,
    HostLog, HostPanes, HostProcess, HostSettings, HostStorage, HostTimers, HostTree,
};
use ilium_plugin_api::generated::types::{
    ErrorCode, Event, FsPath, MeterSegment, NodeId, Outcome, PluginError, RequestId, RichSpan,
    RowState, StreamId, WatchId,
};
use ilium_plugin_api::side::Side;
use ilium_plugin_api::Tier;

use crate::telemetry::LogLine;

/// How the user changed a settings row (SET-020..024, 031, 050, 051).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserInput {
    /// A toggle.
    Toggle,
    /// A choice.
    Choice,
    /// A dialog confirmation.
    DialogConfirm,
    /// A number step.
    NumberStep,
    /// Typed text.
    Typed,
    /// A text area.
    TextArea,
}

/// What a user save did (SET-020..024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserSave {
    /// Saved.
    Saved,
    /// Still pending.
    Pending,
    /// Refused with a message.
    Refused(String),
}

/// The settings-validate export of the fixture plugin: patch JSON, merged JSON.
pub type SettingsValidate = Box<dyn FnMut(&str, &str) -> Result<(), Vec<FieldError>>>;

/// The job-runner export of the fixture plugin: kind, input.
pub type JobRunner = Box<dyn FnMut(&str, &[u8]) -> Result<Vec<u8>, PluginError>>;

/// Builds an [`ImportsFixture`].
pub struct ImportsFixtureBuilder {
    _private: (),
}

impl std::fmt::Debug for ImportsFixtureBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixtureBuilder")
    }
}

impl ImportsFixtureBuilder {
    /// The tier.
    pub fn tier(self, tier: Tier) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::tier")
    }

    /// The side.
    pub fn side(self, side: Side) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::side")
    }

    /// The grants.
    pub fn grants(self, grants: &[&str]) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::grants")
    }

    /// The manifest; installs `test.imports` unless the JSON sets another id.
    pub fn manifest_json(self, json: &str) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::manifest_json")
    }

    /// The project root backing the `project` fs scope.
    pub fn project_root(self, root: &Path) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::project_root")
    }

    /// A named limit (`fs_read_all_mib`, `fs_write_mib`, `journal_entries`, `storage_bucket_mib`, `node_metadata_kib`).
    pub fn limit(self, name: &str, value: u64) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::limit")
    }

    /// Installs a second plugin.
    pub fn also_install(self, manifest_json: &str) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::also_install")
    }

    /// The fake settings-validate export.
    pub fn settings_validate(self, validate: SettingsValidate) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::settings_validate")
    }

    /// The virtual latency of the fake settings-validate export.
    pub fn settings_validate_latency(self, latency: Duration) -> ImportsFixtureBuilder {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixtureBuilder::settings_validate_latency"
        )
    }

    /// The fake job-runner export.
    pub fn job_runner(self, runner: JobRunner) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::job_runner")
    }

    /// Whether the websocket feature is on.
    pub fn websocket_feature(self, on: bool) -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::websocket_feature")
    }

    /// Builds the fixture.
    pub fn build(self) -> ImportsFixture {
        unimplemented!("host::testing::imports_fixture::ImportsFixtureBuilder::build")
    }
}

/// The restricted import set of a job instance (WIT:ilium:host/jobs.*).
pub struct JobInstance {
    _private: (),
}

impl std::fmt::Debug for JobInstance {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JobInstance")
    }
}

impl JobInstance {
    /// The `jobs` import of the job instance.
    pub fn jobs(&mut self) -> &mut dyn HostJobs {
        unimplemented!("host::testing::imports_fixture::JobInstance::jobs")
    }

    /// The `settings` import of the job instance.
    pub fn settings(&mut self) -> &mut dyn HostSettings {
        unimplemented!("host::testing::imports_fixture::JobInstance::settings")
    }

    /// The `clock` import of the job instance.
    pub fn clock(&mut self) -> &mut dyn HostClock {
        unimplemented!("host::testing::imports_fixture::JobInstance::clock")
    }

    /// The `log` import of the job instance.
    pub fn log(&mut self) -> &mut dyn HostLog {
        unimplemented!("host::testing::imports_fixture::JobInstance::log")
    }

    /// Cancels the job from the host.
    pub fn cancel_from_host(&mut self) {
        unimplemented!("host::testing::imports_fixture::JobInstance::cancel_from_host")
    }

    /// The progress values reported, clamped to 0..=100 with NaN dropped.
    pub fn reported_progress(&self) -> Vec<f32> {
        unimplemented!("host::testing::imports_fixture::JobInstance::reported_progress")
    }
}

/// One plugin instance with every host import bound to it (test plan lane imports-core).
pub struct ImportsFixture {
    _private: (),
}

impl std::fmt::Debug for ImportsFixture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixture")
    }
}

impl ImportsFixture {
    /// Starts a builder.
    pub fn builder() -> ImportsFixtureBuilder {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::builder")
    }

    /// The `log` import.
    pub fn log(&mut self) -> &mut dyn HostLog {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::log")
    }

    /// The `clock` import.
    pub fn clock(&mut self) -> &mut dyn HostClock {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::clock")
    }

    /// The `codec` import.
    pub fn codec(&mut self) -> &mut dyn HostCodec {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::codec")
    }

    /// The `fs` import.
    pub fn fs(&mut self) -> &mut dyn HostFs {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::fs")
    }

    /// The `http` import.
    pub fn http(&mut self) -> &mut dyn HostHttp {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::http")
    }

    /// The `jobs` import.
    pub fn jobs(&mut self) -> &mut dyn HostJobs {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::jobs")
    }

    /// The `journal` import.
    pub fn journal(&mut self) -> &mut dyn HostJournal {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::journal")
    }

    /// The `panes` import.
    pub fn panes(&mut self) -> &mut dyn HostPanes {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::panes")
    }

    /// The `process` import.
    pub fn process(&mut self) -> &mut dyn HostProcess {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::process")
    }

    /// The `settings` import.
    pub fn settings(&mut self) -> &mut dyn HostSettings {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::settings")
    }

    /// The `storage` import.
    pub fn storage(&mut self) -> &mut dyn HostStorage {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::storage")
    }

    /// The `timers` import.
    pub fn timers(&mut self) -> &mut dyn HostTimers {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::timers")
    }

    /// The `tree` import.
    pub fn tree(&mut self) -> &mut dyn HostTree {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::tree")
    }

    /// Starts a job instance of a kind.
    pub fn job_instance(&mut self, kind: &str, input: &[u8]) -> JobInstance {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::job_instance")
    }

    /// Moves the manual clock and runs due work.
    pub fn advance(&mut self, by: Duration) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::advance")
    }

    /// Runs pending work without moving the clock.
    pub fn pump(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::pump")
    }

    /// Sets the wall clock.
    pub fn set_wall_clock_unix_millis(&mut self, millis: u64) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::set_wall_clock_unix_millis")
    }

    /// Keeps async requests pending.
    pub fn hold_async_completions(&mut self, on: bool) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::hold_async_completions")
    }

    /// Waits for the outcome of a request.
    pub fn wait_outcome(&mut self, request: RequestId, timeout: Duration) -> Option<Outcome> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_outcome")
    }

    /// Every log line so far.
    pub fn logs(&self) -> Vec<LogLine> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::logs")
    }

    /// Every event delivered to the fixture plugin.
    pub fn events(&self) -> Vec<Event> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::events")
    }

    /// Every event delivered to a plugin.
    pub fn events_of(&self, plugin: &str) -> Vec<Event> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::events_of")
    }

    /// The settings target last opened.
    pub fn opened_settings_target(&self) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::opened_settings_target")
    }

    /// The values of a choice row.
    pub fn choice_values(&self, key: &str) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_values")
    }

    /// The item ids of a choice row.
    pub fn choice_item_ids(&self, key: &str) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_item_ids")
    }

    /// Whether a choice row shows a value as unavailable.
    pub fn choice_shows_unavailable(&self, key: &str, value: &str) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_shows_unavailable")
    }

    /// The stored value of a setting.
    pub fn settings_value(&self, key: &str) -> serde_json::Value {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::settings_value")
    }

    /// Whether the plugin instance is stopped.
    pub fn plugin_is_stopped(&self) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::plugin_is_stopped")
    }

    /// The note of a settings row.
    pub fn row_note(&self, key: &str) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::row_note")
    }

    /// The text of a live info row.
    pub fn info_text(&self, key: &str) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::info_text")
    }

    /// The spans of a rich info row.
    pub fn info_spans(&self, key: &str) -> Vec<RichSpan> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::info_spans")
    }

    /// The state of a settings row.
    pub fn row_state(&self, key: &str) -> RowState {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::row_state")
    }

    /// The state of one item of a list row.
    pub fn item_state(&self, key: &str, index: u32) -> Option<RowState> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::item_state")
    }

    /// The segments of a meter row.
    pub fn meter_segments(&self, id: &str) -> Vec<MeterSegment> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::meter_segments")
    }

    /// The warning codes raised while loading settings.
    pub fn load_warning_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::load_warning_codes")
    }

    /// Plugin sections drawn in a settings tab.
    pub fn drawn_plugin_sections(&self, tab: &str) -> usize {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::drawn_plugin_sections")
    }

    /// Plugin tabs drawn.
    pub fn drawn_plugin_tabs(&self) -> usize {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::drawn_plugin_tabs")
    }

    /// The steps of the last save.
    pub fn save_trace(&self) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::save_trace")
    }

    /// Replaces the stored settings JSON.
    pub fn restore_stored_json(&mut self, json: &str) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::restore_stored_json")
    }

    /// The user edits a row.
    pub fn user_edit(&mut self, key: &str, value_json: &str, input: UserInput) -> UserSave {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::user_edit")
    }

    /// The user saves a patch.
    pub fn user_save(&mut self, patch_json: &str) -> UserSave {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::user_save")
    }

    /// The next job traps.
    pub fn make_next_job_trap(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::make_next_job_trap")
    }

    /// The next job never returns.
    pub fn make_next_job_run_forever(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::make_next_job_run_forever")
    }

    /// The next job exceeds its memory limit.
    pub fn make_next_job_exceed_memory(&mut self) {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixture::make_next_job_exceed_memory"
        )
    }

    /// The next settings-validate call traps.
    pub fn make_next_validate_trap(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::make_next_validate_trap")
    }

    /// Stops the plugin instance.
    pub fn stop_plugin(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::stop_plugin")
    }

    /// Starts a background request.
    pub fn start_background_request(&mut self) -> RequestId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::start_background_request")
    }

    /// Completes a background request.
    pub fn complete_background_request(&mut self, request: RequestId) {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixture::complete_background_request"
        )
    }

    /// The user chooses paths in the picker.
    pub fn picker_choose(&mut self, paths: Vec<FsPath>) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::picker_choose")
    }

    /// The user cancels the picker.
    pub fn picker_cancel(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::picker_cancel")
    }

    /// Waits for an event of a watch.
    pub fn wait_fs_event(&mut self, watch: WatchId, timeout: Duration) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_fs_event")
    }

    /// Waits for chunks of a stream.
    pub fn wait_stream_chunks(
        &mut self,
        stream: StreamId,
        count: usize,
        timeout: Duration,
    ) -> Vec<Vec<u8>> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_stream_chunks")
    }

    /// Waits for output of a child.
    pub fn wait_child_output(
        &mut self,
        child: ChildHandle,
        expected: &[u8],
        timeout: Duration,
    ) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_child_output")
    }

    /// Waits for a child to exit.
    pub fn wait_child_exit(&mut self, child: ChildHandle, timeout: Duration) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_child_exit")
    }

    /// Adds a terminal pane.
    pub fn add_terminal_pane(&mut self, title: &str, columns: u16, rows: u16) -> NodeId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::add_terminal_pane")
    }

    /// Adds a pane restored from a snapshot.
    pub fn add_restored_pane(&mut self, title: &str, columns: u16, rows: u16) -> NodeId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::add_restored_pane")
    }

    /// Adds a group.
    pub fn add_group(&mut self, title: &str) -> NodeId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::add_group")
    }

    /// Adds a group under a parent.
    pub fn add_group_under(&mut self, parent: NodeId, title: &str) -> NodeId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::add_group_under")
    }

    /// Adds a terminal pane under a parent.
    pub fn add_terminal_pane_under(
        &mut self,
        parent: NodeId,
        title: &str,
        columns: u16,
        rows: u16,
    ) -> NodeId {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::add_terminal_pane_under")
    }

    /// Exits a pane.
    pub fn exit_pane(&mut self, pane: NodeId) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::exit_pane")
    }

    /// Focuses a pane.
    pub fn focus_pane(&mut self, pane: NodeId) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::focus_pane")
    }

    /// Records a host journal entry (`pane-created`, `pane-exited`, `input-sent`).
    pub fn record_host_journal_entry(&mut self, pane: NodeId, kind: &str) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::record_host_journal_entry")
    }

    /// Takes a storage lease as a second caller.
    pub fn other_caller_lease(&mut self, key: &str, ttl_millis: u32) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::other_caller_lease")
    }

    /// Makes the on-disk data version of a bucket newer.
    pub fn make_bucket_data_version_newer(&mut self, bucket: Bucket) {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixture::make_bucket_data_version_newer"
        )
    }

    /// The code of the last refused infallible import call.
    pub fn last_import_error_code(&self) -> Option<ErrorCode> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::last_import_error_code")
    }

    /// Sets the agent signature of a node.
    pub fn set_agent(&mut self, node: NodeId, signature: &str) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::set_agent")
    }

    /// Locks a node.
    pub fn lock_node(&mut self, node: NodeId) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::lock_node")
    }

    /// Renames a node.
    pub fn rename_node(&mut self, node: NodeId, title: &str) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::rename_node")
    }

    /// Reads node metadata as a second installed plugin.
    pub fn other_plugin_metadata(&mut self, node: NodeId) -> Option<Vec<u8>> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::other_plugin_metadata")
    }
}
