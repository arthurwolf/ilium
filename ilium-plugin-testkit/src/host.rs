//! FakeHost: one plugin on one side with deterministic fakes (reference-test-harness.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ilium_plugin_sdk_core::exports::{HookCall, HookReply, MenuContext, MenuEntry};
use ilium_plugin_sdk_core::generated::host::InstanceState;
use ilium_plugin_sdk_core::host::inference::{Completion, CompletionRequest};
use ilium_plugin_sdk_core::host::keymap::Binding;
use ilium_plugin_sdk_core::host::workspace::WorkspaceSpec;
use ilium_plugin_sdk_core::primitives::terminal::{OutputFilter, Selection};
use ilium_plugin_sdk_core::primitives::{SessionRef, SignatureSpec, TranscriptTurn};
use ilium_plugin_sdk_core::types::{
    ActionKind, ActionValue, AgentModel, Color, ColorRole, DeactivationReason, Event, Outcome,
    PluginError, PointerModifiers, RequestId, RequestRef, Submission, UiTree, ViewTarget,
};
use ilium_plugin_sdk_core::NativeEntry;
use serde_json::Value;

use crate::errors::HarnessError;
use crate::fakes::cli::FakeCli;
use crate::fakes::clock::FakeClock;
use crate::fakes::fs::FakeFs;
use crate::fakes::http::FakeHttp;
use crate::fakes::jobs::FakeJobs;
use crate::fakes::process::FakeProcess;
use crate::fakes::settings::FakeSettings;
use crate::fakes::storage::FakeStorage;
use crate::fakes::tree::FakeTree;
use crate::fuzz::FuzzFailure;
use crate::replay::ReplayMismatch;

/// One HTTP reply delivered to the plugin: `(request, status, headers, body)`.
pub type HttpReply = (u64, u16, Vec<(String, String)>, Vec<u8>);

/// How the plugin runs: as a native library or as a built component (reference-test-harness.md "Modes").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The plugin runs as a plain library.
    Native,
    /// The built `.wasm` runs under the runtime.
    Component,
}

/// Identity of an activated plugin instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHandle {
    /// "server", "client" or "scene".
    pub side: String,
    /// Instance number, starting at 1.
    pub instance: u64,
}

/// One cell of a rendered screen (the terminal cell, not the generated `primitives::terminal::Cell`).
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenCell {
    /// The grapheme drawn in the cell.
    pub symbol: String,
    /// Foreground colour.
    pub foreground: Color,
    /// Background colour.
    pub background: Color,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
}

impl Default for ScreenCell {
    /// A blank cell in the default foreground role.
    fn default() -> Self {
        let color = Color::Role(ColorRole::Foreground);
        ScreenCell {
            symbol: " ".to_string(),
            foreground: color.clone(),
            background: color,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}
/// Harness-page name of [`ScreenCell`].
pub use ScreenCell as Cell;

/// A rendered screen (reference-test-harness.md "Harness types").
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// Width in cells.
    pub columns: u16,
    /// Height in cells.
    pub rows: u16,
    /// Cells, row-major.
    pub cells: Vec<ScreenCell>,
}

impl Screen {
    /// Rows joined by `\n`, trailing spaces trimmed.
    pub fn text(&self) -> String {
        unimplemented!("harness.screen-text")
    }

    /// The cell at `(row, column)`; outside the screen fails with `harness.cell-out-of-range`.
    pub fn cell(&self, _row: u16, _column: u16) -> ScreenCell {
        unimplemented!("harness.screen-cell")
    }

    /// The text of one row, trailing spaces trimmed.
    pub fn row(&self, _row: u16) -> String {
        unimplemented!("harness.screen-row")
    }

    /// First position `(row, column)` where the text starts.
    pub fn find(&self, _text: &str) -> Option<(u16, u16)> {
        unimplemented!("harness.screen-find")
    }

    /// Every row's text, trailing spaces trimmed (reference-test-harness.md "Harness types").
    pub fn lines(&self) -> Vec<String> {
        unimplemented!("harness.screen-lines")
    }
}

/// A restart scheduled after a failure (DEV-490).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restart {
    /// Fake time of the restart decision in milliseconds.
    pub at_ms: u64,
    /// Delay until the restart in milliseconds.
    pub delay_ms: u32,
}

/// A diagnostic the host recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Dotted code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// A failure inside the 10 minute window (DEV-498).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureRecord {
    /// Fake time of the failure in milliseconds.
    pub at_ms: u64,
    /// Trap or error code.
    pub code: String,
}

/// One log line the plugin wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Level name.
    pub level: String,
    /// Message text.
    pub message: String,
}

impl LogLine {
    /// Whether the message contains `text` (reference-test-harness.md "Logs").
    pub fn contains(&self, text: &str) -> bool {
        self.message.contains(text)
    }
}

/// State record for restore tests (reference-test-harness.md "Snapshot and golden files").
#[derive(Debug, Clone, PartialEq)]
pub struct HostSnapshot {
    /// Instance number.
    pub instance: u64,
    /// Lifecycle state.
    pub state: InstanceState,
    /// Every bucket with its keys and values.
    pub storage: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    /// The bytes the previous deactivate returned.
    pub restored: Option<Vec<u8>>,
}

/// Harness-page name of the setup type: the builder is the host itself (testing.md chains `FakeHost::new().grant(..)`).
pub type FakeHostBuilder = FakeHost;

/// One plugin on one side with deterministic fakes.
#[derive(Debug)]
pub struct FakeHost {
    mode: Mode,
    seed: u64,
}

impl FakeHost {
    /// An empty host: no grants, tier T1, side server, empty tree. Seed 1 unless `ILIUM_PLUGIN_TEST_SEED` is set (DEV-413).
    pub fn new() -> FakeHost {
        FakeHost {
            mode: Mode::Native,
            seed: 1,
        }
    }

    /// Takes id, side, grants and tier from a manifest (DEV-411).
    pub fn for_manifest(_path: impl AsRef<Path>) -> Result<FakeHost, HarnessError> {
        unimplemented!("DEV-411")
    }

    /// Grants one capability; fails with `harness.grant-unknown` or `harness.grant-not-declared` (DEV-411, DEV-412).
    pub fn grant(self, _name: &str) -> FakeHost {
        unimplemented!("DEV-411")
    }

    /// Grants several capabilities.
    pub fn grants(self, _names: &[&str]) -> FakeHost {
        unimplemented!("DEV-411")
    }

    /// Sets the plugin tier, for example "T1".
    pub fn tier(self, _tier: &str) -> FakeHost {
        unimplemented!("DEV-410")
    }

    /// Sets the side: "server", "client" or "scene".
    pub fn side(self, _side: &str) -> FakeHost {
        unimplemented!("DEV-410")
    }

    /// Sets the platform name.
    pub fn platform(self, _platform: &str) -> FakeHost {
        unimplemented!("DEV-410")
    }

    /// Loads a fixture file into the fakes.
    pub fn with_fixture(self, _path: impl AsRef<Path>) -> FakeHost {
        unimplemented!("DEV-423")
    }

    /// Sets the seed; an explicit call wins over the environment (DEV-413).
    pub fn seed(self, _seed: u64) -> FakeHost {
        unimplemented!("DEV-413")
    }

    /// Ends the setup without activating.
    pub fn build(self) -> FakeHost {
        unimplemented!("DEV-410")
    }

    /// Loads a storage snapshot file; unknown shapes fail with `harness.snapshot-invalid` (DEV-497).
    pub fn with_storage_snapshot(self, _path: impl AsRef<Path>) -> FakeHost {
        unimplemented!("DEV-497")
    }

    /// Sets the stored data version for migration tests (DEV-497).
    pub fn with_data_version(self, _version: u32) -> FakeHost {
        unimplemented!("DEV-497")
    }

    /// Selects native or component mode.
    pub fn mode(self, _mode: Mode) -> FakeHost {
        unimplemented!("DEV-400")
    }

    /// The mode this host runs in.
    pub fn current_mode(&self) -> Mode {
        self.mode
    }

    /// The seed in effect.
    pub fn current_seed(&self) -> u64 {
        self.seed
    }

    /// Activates the plugin type; panics with the harness code on failure.
    pub fn activate<P: NativeEntry + Default>(&mut self) -> PluginHandle {
        unimplemented!("DEV-420")
    }

    /// Activates the plugin type and returns the activation error instead of panicking.
    pub fn try_activate<P: NativeEntry + Default>(&mut self) -> Result<PluginHandle, PluginError> {
        unimplemented!("DEV-420")
    }

    /// Deactivates with a reason and returns the saved state bytes; `crash-restart` is reserved (`harness.reason-reserved`).
    pub fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        unimplemented!("DEV-421")
    }

    /// Delivers up to 256 events and returns the submissions of the batch (DEV-430).
    pub fn deliver(&mut self, _events: Vec<Event>) -> Vec<Submission> {
        unimplemented!("DEV-430")
    }

    /// Delivers one event.
    pub fn deliver_one(&mut self, _event: Event) -> Vec<Submission> {
        unimplemented!("DEV-430")
    }

    /// Runs queued deliveries and completions until idle; fails with `harness.settle-limit` after 100 rounds.
    pub fn settle(&mut self) {
        unimplemented!("DEV-431")
    }

    /// Outcomes completed so far, in order.
    pub fn outcomes(&self) -> Vec<Outcome> {
        unimplemented!("DEV-432")
    }

    /// Every submission the plugin made, in order.
    pub fn submitted(&self) -> Vec<Submission> {
        unimplemented!("DEV-432")
    }

    /// Requests the plugin made that the test has not completed.
    pub fn pending_requests(&self) -> Vec<RequestRef> {
        unimplemented!("DEV-433")
    }

    /// Completes a pending request with an outcome.
    pub fn complete(&mut self, _request: RequestRef, _outcome: Outcome) {
        unimplemented!("DEV-433")
    }

    /// Renders a view at `columns` by `rows` (10 to 500 by 1 to 200, else `harness.render-size`).
    pub fn render(&mut self, _target: ViewTarget, _columns: u16, _rows: u16) -> Screen {
        unimplemented!("DEV-440")
    }

    /// Sends an action to a widget key of a view.
    pub fn send_action(
        &mut self,
        _target: ViewTarget,
        _key: &str,
        _kind: ActionKind,
        _value: ActionValue,
    ) {
        unimplemented!("DEV-441")
    }

    /// Clicks a cell of a view; true when a widget took the click.
    pub fn click(&mut self, _target: ViewTarget, _column: u16, _row: u16) -> bool {
        unimplemented!("DEV-442")
    }

    /// Sends a key chord such as "ctrl+s" to a view.
    pub fn key(&mut self, _target: ViewTarget, _key: &str) {
        unimplemented!("DEV-443")
    }

    /// The fake clock.
    pub fn clock(&self) -> FakeClock {
        unimplemented!("DEV-450")
    }

    /// Moves time forward and fires due timers in order (DEV-450).
    pub fn advance_clock(&mut self, _ms: u64) {
        unimplemented!("DEV-450")
    }

    /// Sets time; moving back fails with `harness.clock-backwards`.
    pub fn set_clock(&mut self, _instant_ms: u64) {
        unimplemented!("DEV-451")
    }

    /// Grants a capability while the plugin runs and delivers `grants-changed`.
    pub fn grant_now(&mut self, _name: &str) -> Result<(), HarnessError> {
        unimplemented!("DEV-414")
    }

    /// Revokes a capability while the plugin runs.
    pub fn revoke(&mut self, _name: &str) {
        unimplemented!("DEV-416")
    }

    /// Delivers a `grants-changed` event for these capabilities.
    pub fn grants_changed(&mut self, _names: &[&str]) {
        unimplemented!("DEV-414")
    }

    /// Applies the scheduling-table effect of a deadline breach in this export (DEV-565).
    pub fn breach_deadline(&mut self, _export: &str) {
        unimplemented!("DEV-565")
    }

    /// The fake file system.
    pub fn fs(&mut self) -> &mut FakeFs {
        unimplemented!("DEV-455")
    }

    /// The fake process service.
    pub fn process(&mut self) -> &mut FakeProcess {
        unimplemented!("DEV-464")
    }

    /// Alias of [`FakeHost::process`] used by testing.md.
    pub fn processes(&mut self) -> &mut FakeProcess {
        unimplemented!("DEV-464")
    }

    /// The fake network.
    pub fn http(&mut self) -> &mut FakeHttp {
        unimplemented!("DEV-462")
    }

    /// The fake storage.
    pub fn storage(&self) -> &FakeStorage {
        unimplemented!("DEV-465")
    }

    /// The fake settings.
    pub fn settings(&mut self) -> &mut FakeSettings {
        unimplemented!("DEV-467")
    }

    /// The fake tree.
    pub fn tree(&mut self) -> &mut FakeTree {
        unimplemented!("DEV-470")
    }

    /// The fake job service.
    pub fn jobs(&mut self) -> &mut FakeJobs {
        unimplemented!("DEV-472")
    }

    /// The fake command line.
    pub fn cli(&mut self) -> &mut FakeCli {
        unimplemented!("DEV-473")
    }

    /// Expects the next call to trap with this code; the test fails if it does not (DEV-494).
    pub fn expect_trap(&mut self, _code: &str) {
        unimplemented!("DEV-494")
    }

    /// Makes the next call trap with this code; counts as a failure (DEV-495).
    pub fn inject_trap(&mut self, _code: &str) {
        unimplemented!("DEV-495")
    }

    /// Makes the named host import return this error.
    pub fn inject_failure(&mut self, _import: &str, _error: PluginError) {
        unimplemented!("DEV-494")
    }

    /// Restarts scheduled so far (DEV-490).
    pub fn restarts(&self) -> Vec<Restart> {
        unimplemented!("DEV-490")
    }

    /// True after the third failure in the window.
    pub fn is_disabled(&self) -> bool {
        unimplemented!("DEV-498")
    }

    /// Diagnostics recorded so far.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        unimplemented!("DEV-498")
    }

    /// Restarts the instance with these saved state bytes.
    pub fn restart_with(&mut self, _bytes: Vec<u8>) {
        unimplemented!("DEV-492")
    }

    /// Traps after the next journal append is durable (DEV-496).
    pub fn crash_after_journal_write(&mut self) {
        unimplemented!("DEV-496")
    }

    /// Failures of the last 10 minutes of fake time, oldest first (DEV-498).
    pub fn failures(&self) -> Vec<FailureRecord> {
        unimplemented!("DEV-498")
    }

    /// Current lifecycle state, `stopped-by-failure` after the third failure in the window.
    pub fn lifecycle_state(&self) -> InstanceState {
        unimplemented!("DEV-498")
    }

    /// The bytes restored at the last activation.
    pub fn restored(&self) -> Option<Vec<u8>> {
        unimplemented!("DEV-492")
    }

    /// Log lines the plugin wrote.
    pub fn logs(&self) -> Vec<LogLine> {
        unimplemented!("DEV-499")
    }

    /// Events the loaded fixture queued.
    pub fn fixture_events(&self) -> Vec<Event> {
        unimplemented!("DEV-423")
    }

    /// State record for restore tests.
    pub fn snapshot(&self) -> HostSnapshot {
        unimplemented!("DEV-500")
    }

    /// Runs a fixture file against this host.
    pub fn run_fixture(&mut self, _path: impl AsRef<Path>) -> Result<(), HarnessError> {
        unimplemented!("DEV-423")
    }

    /// Calls the hook handler and returns its reply; fails with `harness.not-a-hook-handler` (DEV-487).
    pub fn run_hook(&mut self, _call: HookCall) -> HookReply {
        unimplemented!("DEV-487")
    }

    /// A valid random event batch; the same seed gives the same batch (DEV-520).
    pub fn fuzz_events(&self, _seed: u64, _count: u32) -> Vec<Event> {
        unimplemented!("DEV-520")
    }

    /// Delivers random batches and fails on an unexpected trap (DEV-521).
    pub fn fuzz_run(&mut self, _seed: u64, _rounds: u32) -> Result<(), FuzzFailure> {
        unimplemented!("DEV-521")
    }

    /// Records the inputs of this host to a replay file (DEV-540).
    pub fn record_to(&mut self, _path: &Path) {
        unimplemented!("DEV-540")
    }

    /// Re-runs a replay file (DEV-541).
    pub fn replay(&mut self, _path: &Path) -> Result<(), ReplayMismatch> {
        unimplemented!("DEV-541")
    }
}

impl Default for FakeHost {
    fn default() -> FakeHost {
        FakeHost::new()
    }
}

// Shared helper surface added for the plugin test lanes. Every method below is a skeleton; the
// manual page or gap note it belongs to is named in its doc comment (gap notes:
// manual_gaps_fakehost.md of the test wave).
impl FakeHost {
    // ---- builder extras ----

    /// Sets the project root the plugin sees (reference-test-harness.md "Builder").
    pub fn with_project_root(self, _root: PathBuf) -> FakeHost {
        unimplemented!("harness.builder-project-root")
    }

    /// Seeds the plugin's own settings before activation (reference-test-harness.md "Builder").
    pub fn with_settings(self, _settings: Value) -> FakeHost {
        unimplemented!("harness.builder-settings")
    }

    /// The project root the plugin sees (reference-test-harness.md "Builder").
    pub fn project_root(&self) -> PathBuf {
        unimplemented!("harness.project-root")
    }

    /// Sets an environment variable visible to the plugin's host (reference-test-harness.md "Fakes").
    pub fn set_env(&mut self, _name: &str, _value: &str) {
        unimplemented!("harness.set-env")
    }

    /// Overrides the platform name the plugin is told (reference-test-harness.md "Builder").
    pub fn set_platform(&mut self, _platform: &str) {
        unimplemented!("harness.set-platform")
    }

    /// Seeds one plugin setting without schema validation (gap: text-triggers, [SET-030]).
    pub fn seed_settings_unchecked(&mut self, _key: &str, _value: Value) {
        unimplemented!("harness.seed-settings-unchecked")
    }

    // ---- kernel settings ----

    /// The kernel settings tabs the plugin may read and write (reference-test-harness.md "Fakes").
    pub fn kernel_settings(&mut self) -> &mut FakeKernelSettings {
        unimplemented!("harness.kernel-settings")
    }

    // ---- inference ----

    /// The scripted inference fake (reference-test-harness.md "Fakes").
    pub fn inference(&mut self) -> &mut FakeInference {
        unimplemented!("harness.inference")
    }

    /// Completion requests the plugin sent, with their request ids (reference-test-harness.md "Fakes").
    pub fn inference_requests(&self) -> Vec<(RequestId, CompletionRequest)> {
        unimplemented!("harness.inference-requests")
    }

    /// Answers one pending completion request (reference-test-harness.md "Fakes").
    pub fn complete_inference(&mut self, _request: RequestId, _completion: Completion) {
        unimplemented!("harness.complete-inference")
    }

    /// Scripts the replies of the next completion requests, in order (reference-test-harness.md "Fakes").
    pub fn script_inference(&mut self, _replies: Vec<Result<String, PluginError>>) {
        unimplemented!("harness.script-inference")
    }

    // ---- plugin instance ----

    /// Direct access to the activated native plugin instance (reference-test-harness.md "Modes").
    pub fn instance_mut<T: NativeEntry + 'static>(&mut self) -> &mut T {
        unimplemented!("harness.instance-mut")
    }

    // ---- extension points ----

    /// Contributes an entry to a collect point as another plugin (reference-test-harness.md "Extension points").
    pub fn contribute_extension(
        &mut self,
        _point: &str,
        _id: &str,
        _priority: impl Into<i64>,
        _entry: impl IntoJsonText,
    ) {
        unimplemented!("harness.contribute-extension")
    }

    /// Contributes an entry without a priority (reference-test-harness.md "Extension points").
    pub fn contribute_entry(&mut self, _point: &str, _id: &str, _entry_json: &str) {
        unimplemented!("harness.contribute-entry")
    }

    /// Contributes one raw JSON entry to a collect point (reference-test-harness.md "Extension points").
    pub fn contribute_collect(&mut self, _point: &str, _entry_json: &str) {
        unimplemented!("harness.contribute-collect")
    }

    /// Replaces every entry of a collect point (reference-test-harness.md "Extension points").
    pub fn set_extension_entries(&mut self, _point: &str, _entries: Vec<impl IntoJsonText>) {
        unimplemented!("harness.set-extension-entries")
    }

    /// Withdraws one contributed entry (reference-test-harness.md "Extension points").
    pub fn withdraw_extension(&mut self, _point: &str, _id: &str) {
        unimplemented!("harness.withdraw-extension")
    }

    /// The entries a collect point currently holds, by priority (reference-test-harness.md "Extension points").
    pub fn extension_entries(&self, _point: &str) -> Vec<ExtensionEntry> {
        unimplemented!("harness.extension-entries")
    }

    /// How often the plugin listed a collect point (reference-test-harness.md "Extension points").
    pub fn extension_list_calls(&self, _point: &str) -> usize {
        unimplemented!("harness.extension-list-calls")
    }

    /// A collect point handle for building up contributions (reference-test-harness.md "Extension points").
    pub fn collect_point(&mut self, _point: &str) -> &mut CollectPoint {
        unimplemented!("harness.collect-point")
    }

    /// The extension-point fake: collect points, hooks and services of other plugins (reference-test-harness.md "Extension points").
    pub fn extensions(&mut self) -> &mut FakeExtensions {
        unimplemented!("harness.extensions")
    }

    // ---- hooks ----

    /// Hook calls the plugin made at `point`, in order (reference-test-harness.md "Extension points").
    pub fn hook_calls(&self, _point: &str) -> Vec<HookCall> {
        unimplemented!("harness.hook-calls")
    }

    /// Payloads of the hook calls at `point`, as JSON text (reference-test-harness.md "Extension points").
    pub fn hook_payloads(&self, _point: &str) -> Vec<String> {
        unimplemented!("harness.hook-payloads")
    }

    /// Every hook call the plugin made, at any point (reference-test-harness.md "Extension points").
    pub fn all_hook_calls(&self) -> Vec<HookCall> {
        unimplemented!("harness.all-hook-calls")
    }

    /// Payloads of the hook runs the plugin received at `point` (reference-test-harness.md "Extension points").
    pub fn hook_runs(&self, _point: &str) -> Vec<String> {
        unimplemented!("harness.hook-runs")
    }

    /// Makes a fake contributor answer every run of `point` with `reply` (reference-test-harness.md "Extension points").
    pub fn set_hook_reply(&mut self, _point: &str, _reply: HookReply) {
        unimplemented!("harness.set-hook-reply")
    }

    /// Answers `point` with `reply` only when the payload matches `when` (reference-test-harness.md "Extension points").
    pub fn set_hook_reply_when(&mut self, _point: &str, _when: Value, _reply: HookReply) {
        unimplemented!("harness.set-hook-reply-when")
    }

    /// Makes the fake contributor at `point` trap (reference-test-harness.md "Extension points").
    pub fn set_hook_trap(&mut self, _point: &str) {
        unimplemented!("harness.set-hook-trap")
    }

    /// Makes the fake contributor at `point` fail with an error (reference-test-harness.md "Extension points").
    pub fn fail_hook(&mut self, _point: &str) {
        unimplemented!("harness.fail-hook")
    }

    /// Installs a closure as the fake contributor at `point` (reference-test-harness.md "Extension points").
    pub fn set_hook_handler(
        &mut self,
        _point: &str,
        _handler: impl Fn(HookCall) -> HookReply + 'static,
    ) {
        unimplemented!("harness.set-hook-handler")
    }

    /// The chain of fake contributors at `point` (reference-test-harness.md "Extension points").
    pub fn hook_chain(&mut self, _point: &str) -> &mut HookChain {
        unimplemented!("harness.hook-chain")
    }

    // ---- services ----

    /// Calls a service method the plugin exposes, as the kernel would (reference-test-harness.md "Services").
    pub fn call_service(
        &mut self,
        _service: &str,
        _method: &str,
        _payload_json: &str,
    ) -> Result<String, PluginError> {
        unimplemented!("harness.call-service")
    }

    /// Calls the plugin's own single-method service (reference-test-harness.md "Services").
    pub fn call_own_service(
        &mut self,
        _service: &str,
        _request_json: &str,
    ) -> Result<String, PluginError> {
        unimplemented!("harness.call-own-service")
    }

    /// Service calls the plugin made: `(service, method, payload_json)` (reference-test-harness.md "Services").
    pub fn service_calls(&self) -> Vec<(String, String, String)> {
        unimplemented!("harness.service-calls")
    }

    /// Scripts the reply of another plugin's service method (reference-test-harness.md "Services").
    pub fn stub_service(&mut self, _service: &str, _method: &str, _reply_json: &str) {
        unimplemented!("harness.stub-service")
    }

    /// Makes another plugin's service method fail with `message` (reference-test-harness.md "Services").
    pub fn stub_service_failure(&mut self, _service: &str, _method: &str, _message: &str) {
        unimplemented!("harness.stub-service-failure")
    }

    /// Scripts the reply of another plugin's service, whatever the method (reference-test-harness.md "Services").
    pub fn set_service_reply(&mut self, _service: &str, _reply: impl IntoJsonText) {
        unimplemented!("harness.set-service-reply")
    }

    /// Makes another plugin's service fail (reference-test-harness.md "Services").
    pub fn fail_service(&mut self, _service: &str) {
        unimplemented!("harness.fail-service")
    }

    // ---- jobs ----

    /// Runs one of the plugin's job kinds to completion (reference-test-harness.md "Jobs").
    pub fn run_job(
        &mut self,
        _kind: &str,
        _input: impl AsRef<[u8]>,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("harness.run-job")
    }

    // ---- client requests ----

    /// Requests the server side sent to clients, and their answers (reference-test-harness.md "Fakes").
    pub fn client_requests(&mut self) -> &mut FakeClientRequests {
        unimplemented!("harness.client-requests")
    }

    // ---- agents, terminal, journal ----

    /// Agent state of panes: transcripts, sessions, models, signatures (reference-test-harness.md "Fakes").
    pub fn agents(&mut self) -> &mut FakeAgents {
        unimplemented!("harness.agents")
    }

    /// Terminal content of panes: rows, history, selection (reference-test-harness.md "Fakes").
    pub fn terminal(&mut self) -> &mut FakeTerminal {
        unimplemented!("harness.terminal")
    }

    /// The pane journal (reference-test-harness.md "Fakes").
    pub fn journal(&mut self) -> &mut FakeJournal {
        unimplemented!("harness.journal")
    }

    // ---- ui ----

    /// The menu entries the plugin offers for `context` (reference-test-harness.md "Rendering").
    pub fn menu_items(&mut self, _context: MenuContext) -> Vec<MenuEntry> {
        unimplemented!("harness.menu-items")
    }

    /// The hover text at `(column, row)` of a view, if any (reference-test-harness.md "Rendering").
    pub fn hover(&mut self, _target: ViewTarget, _column: u16, _row: u16) -> Option<String> {
        unimplemented!("harness.hover")
    }

    /// The last UI tree the plugin produced for `target` (reference-test-harness.md "Rendering").
    pub fn last_tree(&self, _target: ViewTarget) -> UiTree {
        unimplemented!("harness.last-tree")
    }

    /// Views the plugin invalidated since the last render (reference-test-harness.md "Rendering").
    pub fn invalidated(&self) -> Vec<ViewTarget> {
        unimplemented!("harness.invalidated")
    }

    /// Popup texts the plugin opened, in order (reference-test-harness.md "Rendering").
    pub fn popups(&self) -> Vec<String> {
        unimplemented!("harness.popups")
    }

    /// The last notice the plugin showed the user (reference-test-harness.md "Rendering").
    pub fn last_notice(&self) -> Option<String> {
        unimplemented!("harness.last-notice")
    }

    /// Action labels the plugin currently offers (reference-test-harness.md "Rendering").
    pub fn offered_actions(&self) -> Vec<String> {
        unimplemented!("harness.offered-actions")
    }

    /// Cancels the pending dialog or confirmation (reference-test-harness.md "Rendering").
    pub fn cancel_pending(&mut self) {
        unimplemented!("harness.cancel-pending")
    }

    // ---- topics, timers, http, filters ----

    /// Every topic publication: `(topic, payload_json)` (reference-test-harness.md "Fakes").
    pub fn published_topics(&self) -> Vec<(String, String)> {
        unimplemented!("harness.published-topics")
    }

    /// The payloads published on one topic (reference-test-harness.md "Fakes").
    pub fn published(&self, _topic: &str) -> Vec<String> {
        unimplemented!("harness.published")
    }

    /// Durable timers the plugin registered: `(id, fire_at_ms)` (reference-test-harness.md "Fakes").
    pub fn durable_timers(&self) -> Vec<(String, u64)> {
        unimplemented!("harness.durable-timers")
    }

    /// Active timers: `(timer, repeat_ms)` (reference-test-harness.md "Fakes").
    pub fn active_timers(&self) -> Vec<(u32, Option<u32>)> {
        unimplemented!("harness.active-timers")
    }

    /// HTTP replies delivered to the plugin: `(request, status, headers, body)` (reference-test-harness.md "Fakes").
    pub fn http_replies(&self) -> Vec<HttpReply> {
        unimplemented!("harness.http-replies")
    }

    /// Output filters the plugin installed, per pane (reference-test-harness.md "Fakes").
    pub fn output_filters(&self) -> Vec<(u64, OutputFilter)> {
        unimplemented!("harness.output-filters")
    }

    // ---- more fakes ----

    /// Mutable access to the storage fake (reference-test-harness.md "Fakes").
    pub fn storage_mut(&mut self) -> &mut FakeStorage {
        unimplemented!("harness.storage-mut")
    }

    /// The keymap fake (reference-test-harness.md "Fakes").
    pub fn keymap(&mut self) -> &mut FakeKeymap {
        unimplemented!("harness.keymap")
    }

    /// The plugin-registry fake (reference-test-harness.md "Fakes").
    pub fn plugins(&mut self) -> &mut FakePlugins {
        unimplemented!("harness.plugins")
    }

    /// The help-topic fake (reference-test-harness.md "Fakes").
    pub fn help(&mut self) -> &mut FakeHelp {
        unimplemented!("harness.help")
    }

    /// The clipboard fake (reference-test-harness.md "Fakes").
    pub fn clipboard(&mut self) -> &mut FakeClipboard {
        unimplemented!("harness.clipboard")
    }

    /// The vector-store fake (reference-test-harness.md "Fakes").
    pub fn vector(&mut self) -> &mut FakeVector {
        unimplemented!("harness.vector")
    }

    /// The audio fake (reference-test-harness.md "Fakes").
    pub fn audio(&mut self) -> &mut FakeAudio {
        unimplemented!("harness.audio")
    }

    /// The key-grab fake (reference-test-harness.md "Fakes").
    pub fn keys(&mut self) -> &mut FakeKeys {
        unimplemented!("harness.keys")
    }

    /// The workspace fake (reference-test-harness.md "Fakes").
    pub fn workspace(&mut self) -> &mut FakeWorkspace {
        unimplemented!("harness.workspace")
    }

    /// The pane fake (reference-test-harness.md "Fakes").
    pub fn panes(&mut self) -> &mut FakePanes {
        unimplemented!("harness.panes")
    }

    /// The sessions fake (reference-test-harness.md "Fakes").
    pub fn sessions(&mut self) -> &mut FakeSessions {
        unimplemented!("harness.sessions")
    }

    // ---- plugin lifecycle and ordering ----

    /// Disables the plugin under test, as the user would (reference-test-harness.md "Lifecycle").
    pub fn disable_plugin(&mut self) {
        unimplemented!("harness.disable-plugin")
    }

    /// Re-enables the plugin under test (reference-test-harness.md "Lifecycle").
    pub fn enable_plugin(&mut self) {
        unimplemented!("harness.enable-plugin")
    }

    /// Makes every later submission complete with `outcome` (reference-test-harness.md "Submissions").
    pub fn auto_outcome(&mut self, _outcome: Outcome) {
        unimplemented!("harness.auto-outcome")
    }

    /// The order in which host calls and submissions happened (reference-test-harness.md "Submissions").
    pub fn order_of_calls_and_submissions(&self) -> CallOrder {
        unimplemented!("harness.call-order")
    }
}

/// Values that become JSON text for a fake contribution or reply.
pub trait IntoJsonText {
    /// The JSON text.
    fn into_json_text(self) -> String;
}

impl IntoJsonText for Value {
    fn into_json_text(self) -> String {
        self.to_string()
    }
}

impl IntoJsonText for &Value {
    fn into_json_text(self) -> String {
        self.to_string()
    }
}

impl IntoJsonText for String {
    fn into_json_text(self) -> String {
        self
    }
}

impl IntoJsonText for &String {
    fn into_json_text(self) -> String {
        self.clone()
    }
}

impl IntoJsonText for &str {
    fn into_json_text(self) -> String {
        self.to_string()
    }
}

/// Kernel settings tabs visible to the plugin (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeKernelSettings;

impl FakeKernelSettings {
    /// Replaces a whole tab.
    pub fn set_tab(&mut self, _tab: &str, _values: Value) {
        unimplemented!("harness.kernel-settings-set-tab")
    }

    /// Marks a tab unreadable so reads fail.
    pub fn set_unreadable(&mut self, _tab: &str) {
        unimplemented!("harness.kernel-settings-set-unreadable")
    }

    /// The current content of a tab.
    pub fn tab(&self, _tab: &str) -> Option<Value> {
        unimplemented!("harness.kernel-settings-tab")
    }

    /// One value of a tab.
    pub fn get(&self, _tab: &str, _key: &str) -> Option<Value> {
        unimplemented!("harness.kernel-settings-get")
    }

    /// Writes one value through the plugin's host, with the kernel's validation.
    pub fn set(&mut self, _tab: &str, _key: &str, _value: Value) -> Result<(), PluginError> {
        unimplemented!("harness.kernel-settings-set")
    }

    /// Writes the plugin made: `(tab, key, value_json)`.
    pub fn writes(&self) -> Vec<(String, String, String)> {
        unimplemented!("harness.kernel-settings-writes")
    }

    /// Alias of `writes`.
    pub fn written(&self) -> Vec<(String, String, String)> {
        self.writes()
    }
}

/// Scripted inference provider (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeInference;

impl FakeInference {
    /// Answers one pending completion request (reference-test-harness.md "Fakes").
    pub fn complete_inference(&mut self, _request: RequestId, _completion: Completion) {
        unimplemented!("harness.fake-inference-complete")
    }

    /// Requests the plugin sent, with their ids.
    pub fn requests(&self) -> Vec<(RequestId, CompletionRequest)> {
        unimplemented!("harness.inference-requests")
    }

    /// Replies the plugin received as a provider: `(call, result)`.
    pub fn provider_replies(&self) -> Vec<(u64, Result<Completion, PluginError>)> {
        unimplemented!("harness.inference-provider-replies")
    }

    /// Scripts a completion for a matching request.
    pub fn script_completion(&mut self, _request: CompletionRequest, _completion: Completion) {
        unimplemented!("harness.inference-script-completion")
    }

    /// Answers the next request with `text`.
    pub fn reply_text(&mut self, _text: &str) {
        unimplemented!("harness.inference-reply-text")
    }

    /// Fails the next request with `message`.
    pub fn fail_next(&mut self, _message: &str) {
        unimplemented!("harness.inference-fail-next")
    }
}

/// One entry of a collect point (reference-test-harness.md "Extension points").
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionEntry {
    /// Contributor-chosen id.
    pub id: String,
    /// Sort priority.
    pub priority: i64,
    /// The parsed entry.
    pub entry: Value,
}

/// A collect point under construction (reference-test-harness.md "Extension points").
#[derive(Debug, Default)]
pub struct CollectPoint;

impl CollectPoint {
    /// Adds an entry.
    pub fn add(&mut self, _id: &str, _priority: impl Into<i64>, _entry: Value) {
        unimplemented!("harness.collect-point-add")
    }
}

/// A chain of fake hook contributors at one point (reference-test-harness.md "Extension points").
#[derive(Debug, Default)]
pub struct HookChain;

impl HookChain {
    /// Adds a contributor with a fixed reply.
    pub fn add(&mut self, _id: &str, _priority: impl Into<i64>, _reply: HookReply) {
        unimplemented!("harness.hook-chain-add")
    }

    /// Makes every contributor answer with `reply`.
    pub fn reply_all(&mut self, _reply: HookReply) {
        unimplemented!("harness.hook-chain-reply-all")
    }

    /// The payload of the last run, parsed.
    pub fn last_payload(&self) -> Option<Value> {
        unimplemented!("harness.hook-chain-last-payload")
    }
}

/// Collect points, hooks and services of other plugins (reference-test-harness.md "Extension points").
#[derive(Debug, Default)]
pub struct FakeExtensions;

impl FakeExtensions {
    /// Contributes an entry to a collect point.
    pub fn contribute(
        &mut self,
        _point: &str,
        _id: &str,
        _priority: impl Into<i64>,
        _entry: impl IntoJsonText,
    ) {
        unimplemented!("harness.extensions-contribute")
    }

    /// Contributes an entry and returns nothing; alias used by lanes that collect right away.
    pub fn contribute_collect(
        &mut self,
        _point: &str,
        _id: &str,
        _priority: impl Into<i64>,
        _entry: impl IntoJsonText,
    ) {
        unimplemented!("harness.extensions-contribute-collect")
    }

    /// Contributes an inference profile entry.
    pub fn contribute_profile(&mut self, _profile_json: &str) {
        unimplemented!("harness.extensions-contribute-profile")
    }

    /// The entries of a collect point, parsed, by priority.
    pub fn collect(&self, _point: &str) -> Vec<Value> {
        unimplemented!("harness.extensions-collect")
    }

    /// Answers every run of the plugin's hook `point` with `reply`.
    pub fn on_hook(&mut self, _point: &str, _reply: HookReply) {
        unimplemented!("harness.extensions-on-hook")
    }

    /// Alias of `on_hook`.
    pub fn on_run_hook(&mut self, point: &str, reply: HookReply) {
        self.on_hook(point, reply)
    }

    /// Makes the contributor at `point` trap.
    pub fn on_hook_trap(&mut self, _point: &str) {
        unimplemented!("harness.extensions-on-hook-trap")
    }

    /// Payloads of the hook calls the plugin made at `point`.
    pub fn hook_calls(&self, _point: &str) -> Vec<String> {
        unimplemented!("harness.extensions-hook-calls")
    }

    /// Alias of `hook_calls`.
    pub fn hook_payloads(&self, point: &str) -> Vec<String> {
        self.hook_calls(point)
    }

    /// Scripts the reply of another plugin's service.
    pub fn set_service_reply(&mut self, _service: &str, _reply: impl IntoJsonText) {
        unimplemented!("harness.extensions-set-service-reply")
    }

    /// Makes another plugin's service fail.
    pub fn fail_service(&mut self, _service: &str) {
        unimplemented!("harness.extensions-fail-service")
    }
}

/// A request the server side sent to a client (reference-test-harness.md "Fakes").
#[derive(Debug, Clone, PartialEq)]
pub struct SentClientRequest {
    /// Request kind.
    pub kind: String,
    /// Target client.
    pub client: u64,
    /// The request's reference, for answering it.
    pub request: RequestRef,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// JSON payload.
    pub payload_json: String,
}

/// A response delivered to the plugin for a client request (reference-test-harness.md "Fakes").
#[derive(Debug, Clone, PartialEq)]
pub struct ClientResponse {
    /// The call number.
    pub call: u64,
    /// The result delivered.
    pub result: Result<String, PluginError>,
}

/// Client requests and their answers (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeClientRequests;

impl FakeClientRequests {
    /// Requests sent so far.
    pub fn sent(&self) -> Vec<SentClientRequest> {
        unimplemented!("harness.client-requests-sent")
    }

    /// Answers a request.
    pub fn answer(&mut self, _request: RequestRef, _result: Result<String, PluginError>) {
        unimplemented!("harness.client-requests-answer")
    }

    /// Responses the plugin received.
    pub fn responses(&self) -> Vec<ClientResponse> {
        unimplemented!("harness.client-requests-responses")
    }
}

/// A transcript seeded into a pane: raw bytes or parsed turns.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeTranscript {
    /// The raw transcript file.
    Bytes(Vec<u8>),
    /// Parsed turns.
    Turns(Vec<TranscriptTurn>),
}

impl From<Vec<u8>> for FakeTranscript {
    fn from(bytes: Vec<u8>) -> FakeTranscript {
        FakeTranscript::Bytes(bytes)
    }
}

impl From<&[u8]> for FakeTranscript {
    fn from(bytes: &[u8]) -> FakeTranscript {
        FakeTranscript::Bytes(bytes.to_vec())
    }
}

impl From<&str> for FakeTranscript {
    fn from(text: &str) -> FakeTranscript {
        FakeTranscript::Bytes(text.as_bytes().to_vec())
    }
}

impl From<String> for FakeTranscript {
    fn from(text: String) -> FakeTranscript {
        FakeTranscript::Bytes(text.into_bytes())
    }
}

impl From<Vec<TranscriptTurn>> for FakeTranscript {
    fn from(turns: Vec<TranscriptTurn>) -> FakeTranscript {
        FakeTranscript::Turns(turns)
    }
}

/// Agent state of panes (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeAgents;

impl FakeAgents {
    /// Seeds a pane's transcript.
    pub fn set_transcript(&mut self, _pane: u64, _transcript: impl Into<FakeTranscript>) {
        unimplemented!("harness.agents-set-transcript")
    }

    /// Marks a pane's transcript as verified (or not).
    pub fn set_transcript_verified(&mut self, _pane: u64, _verified: bool) {
        unimplemented!("harness.agents-set-transcript-verified")
    }

    /// Sets the context tokens a pane's agent has used.
    pub fn set_context_used_tokens(&mut self, _pane: u64, _tokens: u64) {
        unimplemented!("harness.agents-set-context-used-tokens")
    }

    /// The pane's current transcript bytes.
    pub fn transcript(&self, _pane: u64) -> Option<Vec<u8>> {
        unimplemented!("harness.agents-transcript")
    }

    /// Backups the plugin made of the pane's transcript: `(file name, bytes)` per backup, in order
    /// (core-plugin-compaction.md "Backups": `<name>.pre-compaction-<stamp>.bak`).
    pub fn transcript_backups(&self, _pane: u64) -> Vec<(String, Vec<u8>)> {
        unimplemented!("harness.agents-transcript-backups")
    }

    /// Sets (or clears) the pane's agent session.
    pub fn set_session(&mut self, _pane: u64, _session: impl Into<Option<SessionRef>>) {
        unimplemented!("harness.agents-set-session")
    }

    /// Sets the pane's agent model.
    pub fn set_model(&mut self, _pane: u64, _model: AgentModel) {
        unimplemented!("harness.agents-set-model")
    }

    /// Registers a detection signature.
    pub fn add_signature(&mut self, _signature: SignatureSpec) {
        unimplemented!("harness.agents-add-signature")
    }
}

/// Terminal content of panes (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeTerminal;

impl FakeTerminal {
    /// Sets the visible rows of a pane.
    pub fn set_rows(&mut self, _pane: u64, _rows: &[&str]) {
        unimplemented!("harness.terminal-set-rows")
    }

    /// Sets the visible rows and the scrollback length of a pane.
    pub fn set_screen(&mut self, _pane: u64, _rows: &[&str], _scrollback_lines: u64) {
        unimplemented!("harness.terminal-set-screen")
    }

    /// Sets the scrollback history text of a pane.
    pub fn set_history(&mut self, _pane: u64, _history: &str) {
        unimplemented!("harness.terminal-set-history")
    }

    /// Sets the current selection of a pane.
    pub fn set_selection(&mut self, _pane: u64, _selection: Selection) {
        unimplemented!("harness.terminal-set-selection")
    }

    /// Pointer claims the plugin made on a pane, in order: the claimed modifiers, `None` for a
    /// release (primitives-terminal-pointer.md).
    pub fn pointer_claims(&self, _pane: u64) -> Vec<Option<PointerModifiers>> {
        unimplemented!("harness.terminal-pointer-claims")
    }
}

/// The pane journal (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeJournal;

impl FakeJournal {
    /// Appends an entry for a pane.
    pub fn append(&mut self, _pane: u64, _at_ms: u64, _kind: &str, _text: &str) {
        unimplemented!("harness.journal-append")
    }

    /// Entries the plugin wrote: `(pane, kind, text)`.
    pub fn entries(&self) -> Vec<(Option<u64>, String, String)> {
        unimplemented!("harness.journal-entries")
    }
}

/// The keymap fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeKeymap;

impl FakeKeymap {
    /// Replaces the bindings.
    pub fn set_bindings(&mut self, _bindings: Vec<Binding>) {
        unimplemented!("harness.keymap-set-bindings")
    }

    /// Sets the label of an action.
    pub fn set_label(&mut self, _action: &str, _label: &str) {
        unimplemented!("harness.keymap-set-label")
    }
}

/// The plugin-registry fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakePlugins;

impl FakePlugins {
    /// Adds another plugin with a name and enabled state.
    pub fn add_simple(&mut self, _id: &str, _name: &str, _enabled: bool) {
        unimplemented!("harness.plugins-add-simple")
    }

    /// Enable/disable calls the plugin made: `(id, enabled)`.
    pub fn set_enabled_calls(&self) -> Vec<(String, bool)> {
        unimplemented!("harness.plugins-set-enabled-calls")
    }

    /// Fails the next enable/disable call.
    pub fn fail_next_set_enabled(&mut self, _error: PluginError) {
        unimplemented!("harness.plugins-fail-next-set-enabled")
    }

    /// Marks a plugin as failed.
    pub fn mark_failed(&mut self, _id: &str) {
        unimplemented!("harness.plugins-mark-failed")
    }
}

/// The help-topic fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeHelp;

impl FakeHelp {
    /// Adds a help topic.
    pub fn add_topic(&mut self, _id: &str, _text: &str) {
        unimplemented!("harness.help-add-topic")
    }
}

/// The clipboard fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeClipboard;

impl FakeClipboard {
    /// The last text written.
    pub fn last_text(&self) -> Option<String> {
        unimplemented!("harness.clipboard-last-text")
    }

    /// Fails the next write with `message`.
    pub fn fail_next_write(&mut self, _message: &str) {
        unimplemented!("harness.clipboard-fail-next-write")
    }
}

/// The vector-store fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeVector;

impl FakeVector {
    /// Sizes of the embed batches the plugin requested.
    pub fn embed_batch_sizes(&self) -> Vec<usize> {
        unimplemented!("harness.vector-embed-batch-sizes")
    }

    /// `k` of every top-k query the plugin made.
    pub fn top_k_requests(&self) -> Vec<usize> {
        unimplemented!("harness.vector-top-k-requests")
    }

    /// Scripts the next top-k answer: `(id, score)`.
    pub fn set_top_k(&mut self, _hits: Vec<(String, f64)>) {
        unimplemented!("harness.vector-set-top-k")
    }
}

/// One audio capture the plugin opened (reference-test-harness.md "Fakes").
#[derive(Debug, Clone, PartialEq)]
pub struct AudioCapture {
    /// Capture id.
    pub id: u64,
    /// Whether it is still open.
    pub open: bool,
    /// Device name, `None` for the default device.
    pub device: Option<String>,
    /// Sample rate.
    pub sample_rate_hz: u32,
    /// Channel count.
    pub channels: u16,
}

/// The audio fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeAudio;

impl FakeAudio {
    /// Captures opened so far.
    pub fn captures(&self) -> Vec<AudioCapture> {
        unimplemented!("harness.audio-captures")
    }

    /// Replaces the input device list.
    pub fn set_input_devices(&mut self, _devices: Vec<String>) {
        unimplemented!("harness.audio-set-input-devices")
    }

    /// Replaces the output device list.
    pub fn set_output_devices(&mut self, _devices: Vec<String>) {
        unimplemented!("harness.audio-set-output-devices")
    }

    /// Plugs a device in, delivering the change event.
    pub fn plug(&mut self, _device: &str) {
        unimplemented!("harness.audio-plug")
    }

    /// Unplugs a device, delivering the change event.
    pub fn unplug(&mut self, _device: &str) {
        unimplemented!("harness.audio-unplug")
    }
}

/// One key grab the plugin holds (reference-test-harness.md "Fakes").
#[derive(Debug, Clone, PartialEq)]
pub struct KeyGrab {
    /// Grab id.
    pub id: u64,
    /// Whether it was released.
    pub released: bool,
    /// `GrabSpec::chords` of the grab (reference-wit.md section 5.22).
    pub chords: Vec<String>,
    /// `GrabSpec::pane` of the grab: the pane id, or `None` for a global grab.
    pub pane: Option<u64>,
    /// `GrabSpec::until_release` of the grab.
    pub until_release: bool,
}

/// The key-grab fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeKeys;

impl FakeKeys {
    /// Grabs the plugin made so far.
    pub fn grabs(&self) -> Vec<KeyGrab> {
        unimplemented!("harness.keys-grabs")
    }
}

/// The workspace fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeWorkspace;

impl FakeWorkspace {
    /// Workspaces the plugin registered.
    pub fn registered(&self) -> Vec<WorkspaceSpec> {
        unimplemented!("harness.workspace-registered")
    }

    /// Adds a worktree another tool created.
    pub fn add_foreign(&mut self, _path: &str, _branch: &str) {
        unimplemented!("harness.workspace-add-foreign")
    }
}

/// The pane fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakePanes;

impl FakePanes {
    /// Adds a pane with a process running in `path`.
    pub fn add_running_in(&mut self, _path: &str) {
        unimplemented!("harness.panes-add-running-in")
    }
}

/// The sessions fake (reference-test-harness.md "Fakes").
#[derive(Debug, Default)]
pub struct FakeSessions;

impl FakeSessions {
    /// Adds a reachable live session.
    pub fn add_live(&mut self, _name: &str, _path: &str, _is_self: bool, _panes_json: &str) {
        unimplemented!("harness.sessions-add-live")
    }

    /// Adds a session that cannot be reached.
    pub fn add_unreachable(&mut self, _name: &str, _path: &str) {
        unimplemented!("harness.sessions-add-unreachable")
    }

    /// Requests the plugin sent to sessions: `(session, kind, payload_json)`.
    pub fn requests(&self) -> Vec<(String, String, String)> {
        unimplemented!("harness.sessions-requests")
    }
}

/// The order of host calls and submissions (reference-test-harness.md "Submissions").
#[derive(Debug, Default)]
pub struct CallOrder;

impl CallOrder {
    /// Whether the process call `process` happened before the submission `submission`.
    pub fn process_before_submission(&self, _process: &str, _submission: &str) -> bool {
        unimplemented!("harness.call-order-process-before-submission")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// Builds a screen whose cells hold the given one-grapheme symbols, row-major.
    fn screen_from_symbols(columns: u16, rows: u16, symbols: &[&str]) -> Screen {
        Screen {
            columns,
            rows,
            cells: symbols
                .iter()
                .map(|symbol| ScreenCell {
                    symbol: symbol.to_string(),
                    ..ScreenCell::default()
                })
                .collect(),
        }
    }

    /// A 3x2 screen holding "ab " on row 0 and " c " on row 1; trailing blanks are trimmed when read.
    fn sample_screen() -> Screen {
        screen_from_symbols(3, 2, &["a", "b", " ", " ", "c", " "])
    }

    /// Manual: Screen.text
    #[test]
    fn screen_text_joins_rows_and_trims_trailing_spaces() {
        assert_eq!(sample_screen().text(), "ab\n c");
    }

    /// REQ: DEV-440
    #[test]
    fn screen_cell_takes_row_before_column_from_zero() {
        let screen = screen_from_symbols(2, 2, &["a", "b", "c", "d"]);
        assert_eq!(screen.cell(1, 0).symbol, "c");
        assert_eq!(screen.cell(0, 1).symbol, "b");
    }

    #[test]
    fn screen_lines_and_row_return_every_row_trimmed() {
        let screen = sample_screen();
        assert_eq!(screen.lines(), vec!["ab".to_string(), " c".to_string()]);
        assert_eq!(screen.row(1), " c");
    }

    /// Manual: Screen.find
    #[test]
    fn screen_find_returns_first_position_or_none_for_missing_text() {
        let screen = sample_screen();
        assert_eq!(screen.find("c"), Some((1, 1)));
        assert_eq!(screen.find("zz"), None);
    }

    /// REQ: DEV-400, DEV-404, DEV-413
    #[test]
    fn default_host_uses_seed_one_and_native_mode_under_cargo_test() {
        let host = FakeHost::new();
        if std::env::var_os("ILIUM_PLUGIN_TEST_SEED").is_none() {
            assert_eq!(host.current_seed(), 1);
        }
        assert_eq!(host.current_mode(), Mode::Native);
    }

    /// REQ: DEV-410, DEV-413
    #[test]
    fn builder_methods_chain_and_explicit_seed_wins() {
        let host = FakeHost::new()
            .tier("T2")
            .side("client")
            .platform("linux")
            .seed(3)
            .build();
        assert_eq!(host.current_seed(), 3);
    }

    /// REQ: DEV-412
    #[test]
    fn grant_now_refuses_a_grant_name_outside_the_catalogue() {
        let mut host = FakeHost::new();
        let error = host
            .grant_now("no.such.grant")
            .expect_err("an unknown grant name must fail");
        assert_eq!(error.code, "harness.grant-unknown");
    }

    /// REQ: DEV-411
    #[test]
    fn for_manifest_with_missing_file_returns_an_error() {
        assert!(FakeHost::for_manifest("does-not-exist/manifest.json").is_err());
    }

    /// REQ: DEV-520
    #[test]
    fn fuzz_events_same_seed_gives_same_batch() {
        let host = FakeHost::new();
        let first = host.fuzz_events(42, 8);
        assert_eq!(first.len(), 8);
        assert_eq!(first, host.fuzz_events(42, 8));
    }

    /// REQ: DEV-520
    #[test]
    fn fuzz_events_with_zero_count_is_empty() {
        let host = FakeHost::new();
        assert!(host.fuzz_events(42, 0).is_empty());
    }
}
