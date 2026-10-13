//! FakeHost: one plugin on one side with deterministic fakes (reference-test-harness.md).

use std::collections::BTreeMap;
use std::path::Path;

use ilium_plugin_sdk_core::exports::{HookCall, HookReply};
use ilium_plugin_sdk_core::generated::host::InstanceState;
use ilium_plugin_sdk_core::types::{
    ActionKind, ActionValue, Color, DeactivationReason, Event, Outcome, PluginError, RequestRef,
    Submission, ViewTarget,
};
use ilium_plugin_sdk_core::NativeEntry;

use crate::errors::HarnessError;
use crate::fakes::clock::FakeClock;
use crate::fakes::cli::FakeCli;
use crate::fakes::fs::FakeFs;
use crate::fakes::http::FakeHttp;
use crate::fakes::jobs::FakeJobs;
use crate::fakes::process::FakeProcess;
use crate::fakes::settings::FakeSettings;
use crate::fakes::storage::FakeStorage;
use crate::fakes::tree::FakeTree;
use crate::fuzz::FuzzFailure;
use crate::replay::ReplayMismatch;

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
        FakeHost { mode: Mode::Native, seed: 1 }
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
    pub fn send_action(&mut self, _target: ViewTarget, _key: &str, _kind: ActionKind, _value: ActionValue) {
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
