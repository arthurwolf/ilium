//! FakeHost: one plugin on one side with deterministic fakes (reference-test-harness.md).

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ilium_plugin_sdk_core::exports::{HookCall, HookReply, MenuContext, MenuEntry};
use ilium_plugin_sdk_core::generated::host::{Bucket, InstanceState};
use ilium_plugin_sdk_core::host::inference::{Completion, CompletionRequest};
use ilium_plugin_sdk_core::host::keymap::Binding;
use ilium_plugin_sdk_core::host::workspace::WorkspaceSpec;
use ilium_plugin_sdk_core::primitives::terminal::{OutputFilter, Selection};
use ilium_plugin_sdk_core::primitives::{SessionRef, SignatureSpec, TranscriptTurn};
use ilium_plugin_sdk_core::types::{
    ActionKind, ActionValue, Activation, AgentModel, Color, ColorRole, DeactivationReason,
    ErrorCode, Event, Intent, Outcome, PluginError, PluginId, PluginSide, PointerModifiers,
    RequestId, RequestRef, RequestResult, Submission, UiTree, ViewTarget,
};
use ilium_plugin_sdk_core::NativeEntry;
use serde_json::Value;

use crate::errors::{codes, HarnessError};
use crate::fakes::cli::FakeCli;
use crate::fakes::clock::FakeClock;
use crate::fakes::fs::FakeFs;
use crate::fakes::http::FakeHttp;
use crate::fakes::jobs::FakeJobs;
use crate::fakes::process::FakeProcess;
use crate::fakes::settings::{FakeSettings, ValueKind};
use crate::fakes::storage::FakeStorage;
use crate::fakes::tree::{FakeTree, Outbox};
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
    /// A blank screen of the given size.
    pub(crate) fn blank(columns: u16, rows: u16) -> Screen {
        Screen {
            columns,
            rows,
            cells: vec![ScreenCell::default(); usize::from(columns) * usize::from(rows)],
        }
    }

    /// The untrimmed text of one row; an out-of-range row is a harness failure.
    fn raw_row(&self, row: u16) -> String {
        if row >= self.rows {
            fail(
                codes::CELL_OUT_OF_RANGE,
                &format!("row {row} of a screen with {} rows", self.rows),
            );
        }
        let start = usize::from(row) * usize::from(self.columns);
        let end = start + usize::from(self.columns);
        self.cells
            .iter()
            .skip(start)
            .take(end - start)
            .map(|cell| cell.symbol.as_str())
            .collect()
    }

    /// Rows joined by `\n`, trailing spaces trimmed.
    pub fn text(&self) -> String {
        self.lines().join("\n")
    }

    /// The cell at `(row, column)`; outside the screen fails with `harness.cell-out-of-range`.
    pub fn cell(&self, row: u16, column: u16) -> ScreenCell {
        if row >= self.rows || column >= self.columns {
            fail(
                codes::CELL_OUT_OF_RANGE,
                &format!(
                    "cell ({row}, {column}) of a {} by {} screen",
                    self.rows, self.columns
                ),
            );
        }
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        match self.cells.get(index) {
            Some(cell) => cell.clone(),
            None => fail(codes::CELL_OUT_OF_RANGE, "the cell list is shorter than the size"),
        }
    }

    /// The text of one row, trailing spaces trimmed.
    pub fn row(&self, row: u16) -> String {
        self.raw_row(row).trim_end_matches(' ').to_string()
    }

    /// First position `(row, column)` where the text starts.
    pub fn find(&self, text: &str) -> Option<(u16, u16)> {
        for row in 0..self.rows {
            let line = self.raw_row(row);
            if let Some(offset) = line.find(text) {
                let column = line[..offset].chars().count();
                return Some((row, u16::try_from(column).unwrap_or(u16::MAX)));
            }
        }
        None
    }

    /// Every row's text, trailing spaces trimmed (reference-test-harness.md "Harness types").
    pub fn lines(&self) -> Vec<String> {
        (0..self.rows).map(|row| self.row(row)).collect()
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

/// Raises a harness failure the way the manual says: a panic whose text starts with the dotted code.
fn fail(code: &str, message: &str) -> ! {
    panic!("{code}: {message}")
}

/// Fake instant a new host starts at: 2030-01-01 00:00:00 UTC (reference-test-harness.md "Fake clock").
const START_MS: u64 = 1_893_456_000_000;
/// Largest event batch `deliver` takes (DEV-430).
const MAX_BATCH_EVENTS: usize = 256;
/// Rounds `settle` runs before it gives up (DEV-431).
const SETTLE_ROUNDS: usize = 100;
/// Failures older than this leave the window (DEV-498).
const FAILURE_WINDOW_MS: u64 = 600_000;
/// A dedup key repeats as `duplicate` for this long (DEV-433).
const DEDUP_WINDOW_MS: u64 = 600_000;
/// Delay before the first and the second restart (DEV-490).
const RESTART_DELAYS_MS: [u32; 2] = [1_000, 5_000];
/// API version told to the plugin.
const API_VERSION: &str = "ilium:plugin@0.1.0";

/// Every capability name the kernel knows (reference-capabilities.md).
const GRANT_CATALOGUE: &[&str] = &[
    "agent.config-propose",
    "agent.control",
    "agent.read-prompts",
    "agent.read",
    "agent.signatures",
    "agent.transcript",
    "audio",
    "audio.capture",
    "audio.loopback",
    "cli.contribute",
    "clipboard",
    "commands.invoke",
    "editor.edit",
    "editor.provide",
    "editor.read",
    "extensions.contribute",
    "extensions.provide",
    "fs.pick",
    "fs.read",
    "fs.watch",
    "fs.write",
    "gpu.kernels",
    "help.read",
    "http.api-route",
    "http",
    "inference.embed",
    "inference.provide",
    "inference",
    "jobs.run",
    "journal.read",
    "keymap.read",
    "keys.intercept",
    "location.geocode",
    "location.observer",
    "media.video",
    "messages.peer",
    "notify.desktop",
    "notify.toast",
    "open.external",
    "panes.create",
    "panes.lifecycle",
    "panes.provide-kind",
    "panes.read",
    "panes.signal",
    "plugins.manage",
    "process.long-lived",
    "process.run",
    "services",
    "sessions.cross",
    "settings.all",
    "settings.contribute",
    "settings.read-kernel",
    "settings.write-kernel",
    "sound.play",
    "terminal.history",
    "terminal.input-lock",
    "terminal.output-filter",
    "terminal.pointer",
    "terminal.raw-input",
    "terminal.read-cells",
    "terminal.read-screen",
    "terminal.write",
    "topics.subscribe",
    "tree.attention",
    "tree.decorate",
    "tree.metadata",
    "tree.read",
    "tree.restructure",
    "tree.write",
    "ui.chrome",
    "ui.panels",
    "workspace.manage",
];

/// Host import families `inject_failure` accepts (DEV-494).
const IMPORT_FAMILIES: &[&str] = &[
    "storage", "fs", "process", "http", "settings", "tree", "jobs", "cli", "inference", "notify",
    "sound", "clipboard", "journal", "keymap", "plugins", "help", "panes", "terminal", "agent",
    "editor", "audio", "keys", "workspace", "sessions", "messages", "topics", "extensions",
    "services", "vector", "location", "media", "gpu", "commands", "ui", "open",
];

/// The name of a grant without its scope: `process.run:git` is `process.run`.
fn grant_base(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

/// Whether a grant name (with or without scope) is in the catalogue.
fn is_known_grant(name: &str) -> bool {
    GRANT_CATALOGUE.contains(&grant_base(name))
}

/// Grants of which the plugin must hold at least one to receive the event; `None` when always delivered.
fn event_grants(event: &Event) -> Option<&'static [&'static str]> {
    let grants: &'static [&'static str] = match event {
        Event::TreeChanged(_) | Event::NodeRemoved(_) => &["tree.read"],
        Event::PaneCreated(_)
        | Event::PaneExited(_)
        | Event::PaneFocused(_)
        | Event::PaneResized(_)
        | Event::PaneTitleChanged(_)
        | Event::PaneLifecycleChanged(_) => &["panes.read"],
        Event::AgentIdentified(_)
        | Event::AgentActivityChanged(_)
        | Event::AgentGoalChanged(_)
        | Event::SignalChanged(_)
        | Event::AgentModelChanged(_) => &["agent.read"],
        Event::InputSubmitted(_) | Event::UserLineSubmitted(_) => &["agent.read-prompts"],
        Event::OutputMatched(_) => &["terminal.output-filter"],
        Event::EditorOpened(_)
        | Event::EditorClosed(_)
        | Event::EditorChanged(_)
        | Event::EditorSaved(_)
        | Event::EditorPointer(_)
        | Event::EditorViewportChanged(_) => &["editor.read"],
        Event::TopicPublished(_) => &["topics.subscribe"],
        Event::FsChanged(_) => &["fs.watch"],
        Event::AudioFrames(_) => &["audio.capture", "audio.loopback"],
        Event::CliInvoked(_) | Event::CliCancelled(_) => &["cli.contribute"],
        Event::HttpCalled(_) => &["http.api-route"],
        Event::ScreenChanged(_) => &["terminal.read-cells"],
        Event::ChildOutput(_) | Event::ChildExited(_) => &["process.long-lived"],
        Event::AttentionChanged(_) => &["tree.attention", "tree.read"],
        Event::KeymapChanged => &["keymap.read"],
        _ => return None,
    };
    Some(grants)
}

/// Short event name for diagnostics.
fn event_name(event: &Event) -> String {
    match serde_json::to_value(event) {
        Ok(Value::Object(map)) => map
            .get("tag")
            .and_then(Value::as_str)
            .unwrap_or("event")
            .to_string(),
        _ => "event".to_string(),
    }
}

/// The grant an intent needs on top of being declared (DEV-433). Other intents are treated as permitted.
fn intent_grant(intent: &Intent) -> Option<&'static str> {
    match intent {
        Intent::PlaySound(_) => Some("sound.play"),
        Intent::CloseNode(_) => Some("tree.write"),
        _ => None,
    }
}

/// Decodes standard base64 (with or without padding); `None` for any other character.
pub(crate) fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for character in text.trim_end_matches('=').chars() {
        let value = match character {
            'A'..='Z' => character as u32 - 'A' as u32,
            'a'..='z' => character as u32 - 'a' as u32 + 26,
            '0'..='9' => character as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn bucket_of(name: &str) -> Option<Bucket> {
    match name {
        "global" => Some(Bucket::Global),
        "project" => Some(Bucket::Project),
        "session" => Some(Bucket::Session),
        "shared" => Some(Bucket::Shared),
        _ => None,
    }
}

/// A value that only carries a manual `Debug` so the host can derive it.
struct Opaque<T>(T);

impl<T> std::fmt::Debug for Opaque<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("..")
    }
}

/// The plugin behind a uniform interface, whatever its type.
trait ErasedPlugin {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError>;
    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>>;
    fn on_events(&mut self, events: Vec<Event>) -> Vec<Submission>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct NativeWrapper<P>(P);

impl<P: NativeEntry + 'static> ErasedPlugin for NativeWrapper<P> {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        self.0.native_activate(activation)
    }
    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        self.0.native_deactivate(reason)
    }
    fn on_events(&mut self, events: Vec<Event>) -> Vec<Submission> {
        self.0.native_on_events(events)
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.0
    }
}

type PluginFactory = Box<dyn Fn() -> Box<dyn ErasedPlugin>>;

/// A submission whose outcome the test has not seen yet.
#[derive(Debug, Clone)]
struct PendingRequest {
    correlation: u64,
    needed_grant: Option<&'static str>,
    fixed: Option<Outcome>,
}

/// One plugin on one side with deterministic fakes.
#[derive(Debug)]
pub struct FakeHost {
    mode: Mode,
    seed: u64,
    seed_explicit: bool,
    tier: String,
    explicit_side: Option<String>,
    side: String,
    platform: String,
    plugin_id: String,
    declared: Option<BTreeSet<String>>,
    manifest_server_grants: Vec<String>,
    manifest_client_grants: Vec<String>,
    granted: BTreeSet<String>,
    setup_closed: bool,
    state: InstanceState,
    instance: u64,
    now_ms: u64,
    outbox: Outbox,
    storage: FakeStorage,
    tree: FakeTree,
    settings: FakeSettings,
    jobs: FakeJobs,
    cli: FakeCli,
    fs: FakeFs,
    process: FakeProcess,
    http: FakeHttp,
    project_root: PathBuf,
    env: BTreeMap<String, String>,
    data_version: u32,
    restored: Option<Vec<u8>>,
    exports: &'static [&'static str],
    plugin: Opaque<Option<Box<dyn ErasedPlugin>>>,
    factory: Opaque<Option<PluginFactory>>,
    submitted: Vec<Submission>,
    pending: Vec<PendingRequest>,
    outcomes: Vec<Outcome>,
    auto_outcome: Option<Outcome>,
    dedup: BTreeMap<String, (u64, u64)>,
    failures: Vec<FailureRecord>,
    restarts: Vec<Restart>,
    restart_due: Option<u64>,
    disabled_by_failure: bool,
    disabled_by_user: bool,
    diagnostics: Vec<Diagnostic>,
    pending_trap: Option<String>,
    crash_armed: bool,
    injected_failures: Vec<(String, PluginError)>,
    fixture_events: Vec<Event>,
    hook_runs: BTreeMap<String, Vec<String>>,
    call_order: CallOrder,
    kernel_settings: FakeKernelSettings,
    inference: FakeInference,
    extensions: FakeExtensions,
    client_requests: FakeClientRequests,
    agents: FakeAgents,
    terminal: FakeTerminal,
    journal: FakeJournal,
    keymap: FakeKeymap,
    plugins: FakePlugins,
    help: FakeHelp,
    clipboard: FakeClipboard,
    vector: FakeVector,
    audio: FakeAudio,
    keys: FakeKeys,
    workspace: FakeWorkspace,
    panes: FakePanes,
    sessions: FakeSessions,
    recording: Option<PathBuf>,
}

impl FakeHost {
    /// An empty host: no grants, tier T1, side server, empty tree. Seed 1 unless `ILIUM_PLUGIN_TEST_SEED` is set (DEV-413).
    pub fn new() -> FakeHost {
        let seed = std::env::var("ILIUM_PLUGIN_TEST_SEED")
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .unwrap_or(1);
        let outbox: Outbox = Outbox::default();
        FakeHost {
            mode: Mode::Native,
            seed,
            seed_explicit: false,
            tier: "T1".to_string(),
            explicit_side: None,
            side: "server".to_string(),
            platform: "linux".to_string(),
            plugin_id: "test.plugin".to_string(),
            declared: None,
            manifest_server_grants: Vec::new(),
            manifest_client_grants: Vec::new(),
            granted: BTreeSet::new(),
            setup_closed: false,
            state: InstanceState::Inactive,
            instance: 0,
            now_ms: START_MS,
            outbox: outbox.clone(),
            storage: FakeStorage::new(),
            tree: FakeTree::with_outbox(outbox.clone()),
            settings: FakeSettings::with_outbox(outbox.clone()),
            jobs: FakeJobs::with_outbox(outbox.clone()),
            cli: FakeCli::with_outbox(outbox),
            fs: FakeFs::default(),
            process: FakeProcess::default(),
            http: FakeHttp::default(),
            project_root: PathBuf::from("/project"),
            env: BTreeMap::new(),
            data_version: 0,
            restored: None,
            exports: &[],
            plugin: Opaque(None),
            factory: Opaque(None),
            submitted: Vec::new(),
            pending: Vec::new(),
            outcomes: Vec::new(),
            auto_outcome: None,
            dedup: BTreeMap::new(),
            failures: Vec::new(),
            restarts: Vec::new(),
            restart_due: None,
            disabled_by_failure: false,
            disabled_by_user: false,
            diagnostics: Vec::new(),
            pending_trap: None,
            crash_armed: false,
            injected_failures: Vec::new(),
            fixture_events: Vec::new(),
            hook_runs: BTreeMap::new(),
            call_order: CallOrder::default(),
            kernel_settings: FakeKernelSettings::default(),
            inference: FakeInference::default(),
            extensions: FakeExtensions::default(),
            client_requests: FakeClientRequests::default(),
            agents: FakeAgents::default(),
            terminal: FakeTerminal::default(),
            journal: FakeJournal::default(),
            keymap: FakeKeymap::default(),
            plugins: FakePlugins::default(),
            help: FakeHelp::default(),
            clipboard: FakeClipboard::default(),
            vector: FakeVector::default(),
            audio: FakeAudio::default(),
            keys: FakeKeys::default(),
            workspace: FakeWorkspace::default(),
            panes: FakePanes::default(),
            sessions: FakeSessions::default(),
            recording: None,
        }
    }

    /// Fails with `harness.setup-closed` once the plugin has been activated.
    fn require_setup_open(&self, what: &str) {
        if self.setup_closed {
            fail(
                codes::SETUP_CLOSED,
                &format!("{what} was called after activate"),
            );
        }
    }

    /// Fails with `harness.not-active` unless the plugin has been activated at least once.
    fn require_started(&self, what: &str) {
        if self.instance == 0 {
            fail(codes::NOT_ACTIVE, &format!("{what} was called before activate"));
        }
    }

    /// Takes id, side, grants and tier from a manifest (DEV-411).
    pub fn for_manifest(path: impl AsRef<Path>) -> Result<FakeHost, HarnessError> {
        let path = path.as_ref();
        let manifest_path = if path.is_dir() {
            path.join("ilium-plugin.json")
        } else {
            path.to_path_buf()
        };
        let text = std::fs::read_to_string(&manifest_path).map_err(|error| {
            HarnessError::new(
                codes::PLUGIN_UNKNOWN,
                format!("cannot read {}: {error}", manifest_path.display()),
            )
        })?;
        let manifest: Value = serde_json::from_str(&text).map_err(|error| {
            HarnessError::new(
                codes::PLUGIN_UNKNOWN,
                format!("{} is not JSON: {error}", manifest_path.display()),
            )
        })?;
        let mut host = FakeHost::new();
        if let Some(id) = manifest.get("id").and_then(Value::as_str) {
            host.plugin_id = id.to_string();
        }
        let grants_of = |side: &str| -> Vec<String> {
            manifest
                .get("capabilities")
                .and_then(|capabilities| capabilities.get(side))
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        host.manifest_server_grants = grants_of("server");
        host.manifest_client_grants = grants_of("client");
        let declared: BTreeSet<String> = host
            .manifest_server_grants
            .iter()
            .chain(host.manifest_client_grants.iter())
            .cloned()
            .collect();
        host.declared = Some(declared);
        host.granted = host.manifest_server_grants.iter().cloned().collect();
        host.declare_settings_rows(&manifest);
        Ok(host)
    }

    /// Declares the settings rows of the manifest so wrong-typed writes are refused.
    fn declare_settings_rows(&mut self, manifest: &Value) {
        let Some(tabs) = manifest
            .get("contributes")
            .and_then(|contributes| contributes.get("settings"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for tab in tabs {
            let Some(sections) = tab.get("sections").and_then(Value::as_array) else {
                continue;
            };
            for section in sections {
                let Some(rows) = section.get("rows").and_then(Value::as_array) else {
                    continue;
                };
                for row in rows {
                    let Some(key) = row.get("key").and_then(Value::as_str) else {
                        continue;
                    };
                    let row_type = row.get("type").and_then(Value::as_str).unwrap_or("");
                    self.settings.declare(
                        key,
                        ValueKind::of_row_type(row_type),
                        row.get("default").cloned(),
                    );
                }
            }
        }
    }

    /// Validates a grant name against the catalogue and the manifest; panics with the harness code.
    fn check_grant(&self, name: &str) -> Result<(), HarnessError> {
        if !is_known_grant(name) {
            return Err(HarnessError::new(
                codes::GRANT_UNKNOWN,
                format!("{name} is not a capability name"),
            ));
        }
        if let Some(declared) = &self.declared {
            let is_declared = declared
                .iter()
                .any(|entry| entry == name || grant_base(entry) == grant_base(name));
            if !is_declared {
                return Err(HarnessError::new(
                    codes::GRANT_NOT_DECLARED,
                    format!("{name} is not declared in the manifest"),
                ));
            }
        }
        Ok(())
    }

    /// Grants one capability; fails with `harness.grant-unknown` or `harness.grant-not-declared` (DEV-411, DEV-412).
    pub fn grant(mut self, name: &str) -> FakeHost {
        self.require_setup_open("grant");
        if let Err(error) = self.check_grant(name) {
            fail(&error.code, &error.message);
        }
        self.granted.insert(name.to_string());
        self
    }

    /// Grants several capabilities.
    pub fn grants(mut self, names: &[&str]) -> FakeHost {
        self.require_setup_open("grants");
        for name in names {
            if let Err(error) = self.check_grant(name) {
                fail(&error.code, &error.message);
            }
            self.granted.insert((*name).to_string());
        }
        self
    }

    /// Sets the plugin tier, for example "T1".
    pub fn tier(mut self, tier: &str) -> FakeHost {
        self.require_setup_open("tier");
        self.tier = tier.to_string();
        self
    }

    /// Sets the side: "server", "client" or "scene".
    pub fn side(mut self, side: &str) -> FakeHost {
        self.require_setup_open("side");
        self.explicit_side = Some(side.to_string());
        self.side = side.to_string();
        if side == "client" && self.declared.is_some() {
            self.granted = self.manifest_client_grants.iter().cloned().collect();
        } else if side == "server" && self.declared.is_some() {
            self.granted = self.manifest_server_grants.iter().cloned().collect();
        }
        self
    }

    /// Sets the platform name.
    pub fn platform(mut self, platform: &str) -> FakeHost {
        self.require_setup_open("platform");
        self.platform = platform.to_string();
        self
    }

    /// Loads a fixture file into the fakes.
    pub fn with_fixture(mut self, path: impl AsRef<Path>) -> FakeHost {
        self.require_setup_open("with_fixture");
        let result = crate::fixture::Fixture::load(path.as_ref())
            .and_then(|fixture| fixture.apply_to(&mut self));
        if let Err(error) = result {
            fail(&error.code, &error.message);
        }
        self
    }

    /// Sets the seed; an explicit call wins over the environment (DEV-413).
    pub fn seed(mut self, seed: u64) -> FakeHost {
        self.require_setup_open("seed");
        self.seed = seed;
        self.seed_explicit = true;
        self
    }

    /// Ends the setup without activating.
    pub fn build(self) -> FakeHost {
        self
    }

    /// Loads a storage snapshot file; unknown shapes fail with `harness.snapshot-invalid` (DEV-497).
    pub fn with_storage_snapshot(mut self, path: impl AsRef<Path>) -> FakeHost {
        self.require_setup_open("with_storage_snapshot");
        let path = path.as_ref();
        let invalid = |reason: String| -> ! {
            fail(
                codes::SNAPSHOT_INVALID,
                &format!("{}: {reason}", path.display()),
            )
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => invalid(format!("cannot read the file: {error}")),
        };
        let document: Value = match serde_json::from_str(&text) {
            Ok(document) => document,
            Err(error) => invalid(format!("not JSON: {error}")),
        };
        let Value::Object(buckets) = document else {
            invalid("the top level must be an object".to_string())
        };
        for (bucket_name, entries) in buckets {
            let Some(bucket) = bucket_of(&bucket_name) else {
                invalid(format!("unknown bucket {bucket_name}"))
            };
            let Value::Object(entries) = entries else {
                invalid(format!("bucket {bucket_name} must be an object"))
            };
            for (key, value) in entries {
                let Some(encoded) = value.as_str() else {
                    invalid(format!("{bucket_name}.{key} must be a base64 string"))
                };
                let Some(bytes) = decode_base64(encoded) else {
                    invalid(format!("{bucket_name}.{key} is not base64"))
                };
                self.storage.put(bucket, &key, &bytes);
            }
        }
        self
    }

    /// Sets the stored data version for migration tests (DEV-497).
    pub fn with_data_version(mut self, version: u32) -> FakeHost {
        self.require_setup_open("with_data_version");
        self.data_version = version;
        self
    }

    /// Selects native or component mode.
    pub fn mode(mut self, mode: Mode) -> FakeHost {
        self.require_setup_open("mode");
        self.mode = mode;
        self
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
    pub fn activate<P: NativeEntry + Default + 'static>(&mut self) -> PluginHandle {
        match self.try_activate::<P>() {
            Ok(handle) => handle,
            Err(error) => fail(
                "harness.activation-failed",
                &format!("{:?}: {}", error.code, error.message),
            ),
        }
    }

    /// Activates the plugin type and returns the activation error instead of panicking.
    pub fn try_activate<P: NativeEntry + Default + 'static>(
        &mut self,
    ) -> Result<PluginHandle, PluginError> {
        if self.plugin.0.is_some() || self.state == InstanceState::RestartScheduled {
            fail(codes::ALREADY_ACTIVE, "the plugin is already active on this host");
        }
        self.exports = P::EXPORTS;
        let side = self
            .explicit_side
            .clone()
            .unwrap_or_else(|| P::SIDE.to_string());
        self.side = side;
        self.settings.set_side(if self.side == "client" {
            PluginSide::Client
        } else {
            PluginSide::Server
        });
        self.factory.0 = Some(Box::new(|| Box::new(NativeWrapper(P::default()))));
        self.setup_closed = true;
        self.disabled_by_failure = false;
        self.disabled_by_user = false;
        let mut plugin: Box<dyn ErasedPlugin> = Box::new(NativeWrapper(P::default()));
        self.instance += 1;
        self.state = InstanceState::Activating;
        let activation = self.build_activation(None);
        if let Err(error) = plugin.activate(activation) {
            self.state = InstanceState::Failed;
            return Err(error);
        }
        self.plugin.0 = Some(plugin);
        self.state = InstanceState::Active;
        self.replay_tree();
        Ok(PluginHandle {
            side: self.side.clone(),
            instance: self.instance,
        })
    }

    fn build_activation(&self, restored: Option<Vec<u8>>) -> Activation {
        Activation {
            plugin: PluginId(self.plugin_id.clone()),
            api: API_VERSION.to_string(),
            instance: self.instance,
            side: self.side.clone(),
            settings_json: self.settings.as_json(),
            granted: self.granted.iter().cloned().collect(),
            restored,
        }
    }

    /// Replaces the outbox by a `tree-changed` replay when the plugin can read a non-empty tree.
    fn replay_tree(&mut self) {
        self.outbox.borrow_mut().clear();
        if !self.granted.contains("tree.read") {
            return;
        }
        let ids = self.tree.preorder_ids();
        if ids.is_empty() {
            return;
        }
        self.outbox.borrow_mut().push(Event::TreeChanged(
            ids.into_iter().map(ilium_plugin_sdk_core::types::NodeId).collect(),
        ));
    }

    /// Deactivates with a reason and returns the saved state bytes; `crash-restart` is reserved (`harness.reason-reserved`).
    pub fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        if reason == DeactivationReason::CrashRestart {
            fail(
                codes::REASON_RESERVED,
                "crash-restart is reserved for the host; use inject_trap",
            );
        }
        let Some(mut plugin) = self.plugin.0.take() else {
            fail(codes::NOT_ACTIVE, "deactivate was called on a host with no active plugin");
        };
        self.state = InstanceState::Deactivating;
        let bytes = plugin.deactivate(reason);
        self.state = InstanceState::Stopped;
        bytes
    }

    /// Delivers up to 256 events and returns the submissions of the batch (DEV-430).
    pub fn deliver(&mut self, events: Vec<Event>) -> Vec<Submission> {
        if events.len() > MAX_BATCH_EVENTS {
            fail(
                codes::BATCH_TOO_LARGE,
                &format!("{} events in one batch, the limit is {MAX_BATCH_EVENTS}", events.len()),
            );
        }
        self.require_started("deliver");
        self.deliver_batch(events)
    }

    /// Delivers one event.
    pub fn deliver_one(&mut self, event: Event) -> Vec<Submission> {
        self.deliver(vec![event])
    }

    /// Delivers a batch to the live plugin: grant filter, trap injection, panic capture and submission checks.
    fn deliver_batch(&mut self, events: Vec<Event>) -> Vec<Submission> {
        if self.plugin.0.is_none() {
            return Vec::new();
        }
        let mut accepted = Vec::new();
        for event in events {
            let allowed = match event_grants(&event) {
                None => true,
                Some(needed) => needed.iter().any(|grant| self.granted.contains(*grant)),
            };
            if allowed {
                accepted.push(event);
            } else {
                self.diagnostics.push(Diagnostic {
                    code: "events-not-granted".to_string(),
                    message: format!("{} dropped: no matching grant", event_name(&event)),
                });
            }
        }
        if accepted.is_empty() {
            return Vec::new();
        }
        if let Some(code) = self.pending_trap.take() {
            self.record_failure(&code, "injected by the test");
            return Vec::new();
        }
        let outcome = match self.plugin.0.as_mut() {
            Some(plugin) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plugin.on_events(accepted)
            })),
            None => return Vec::new(),
        };
        let submissions = match outcome {
            Ok(submissions) => submissions,
            Err(_) => {
                self.record_failure("trap.panic", "the plugin panicked in on-events");
                return Vec::new();
            }
        };
        if self.crash_armed {
            self.crash_armed = false;
            self.record_failure("trap.panic", "crash after the journal write");
        }
        self.admit_submissions(submissions)
    }

    /// Checks a batch of submissions (dedup keys, grants) and queues them as pending requests.
    fn admit_submissions(&mut self, submissions: Vec<Submission>) -> Vec<Submission> {
        if submissions.len() > MAX_BATCH_EVENTS {
            fail(
                "trap.submission-batch",
                &format!("{} submissions in one batch", submissions.len()),
            );
        }
        if submissions.is_empty() {
            return submissions;
        }
        let mut verdicts: Vec<Option<Outcome>> = Vec::with_capacity(submissions.len());
        let mut seen_in_batch: BTreeMap<String, u64> = BTreeMap::new();
        let mut first_refused: Option<usize> = None;
        for (index, submission) in submissions.iter().enumerate() {
            let mut verdict = None;
            if let Some(key) = &submission.dedup_key {
                let valid = !key.is_empty()
                    && key.chars().count() <= 128
                    && key.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "._:/-".contains(character)
                    });
                if !valid {
                    verdict = Some(Outcome::Refused(PluginError::new(
                        ErrorCode::InvalidArgument,
                        format!("dedup-key-invalid: {key}"),
                    )));
                } else if let Some(first) = seen_in_batch.get(key) {
                    verdict = Some(Outcome::Duplicate(RequestRef::Submission(*first)));
                } else if let Some((at_ms, first)) = self.dedup.get(key) {
                    if self.now_ms.saturating_sub(*at_ms) <= DEDUP_WINDOW_MS {
                        verdict = Some(Outcome::Duplicate(RequestRef::Submission(*first)));
                    }
                }
            }
            if verdict.is_none() {
                if let Some(grant) = intent_grant(&submission.intent) {
                    if !self.granted.contains(grant) {
                        verdict = Some(Outcome::Refused(PluginError::new(
                            ErrorCode::NotPermitted,
                            format!("the {grant} grant is not held"),
                        )));
                    }
                }
            }
            if verdict.is_none() {
                if let Some(key) = &submission.dedup_key {
                    seen_in_batch.insert(key.clone(), submission.correlation);
                }
            }
            if matches!(verdict, Some(Outcome::Refused(_))) && first_refused.is_none() {
                first_refused = Some(index);
            }
            verdicts.push(verdict);
        }
        if let Some(lowest) = first_refused {
            for (index, verdict) in verdicts.iter_mut().enumerate() {
                if !matches!(verdict, Some(Outcome::Refused(_))) {
                    let _ = index;
                    *verdict = Some(Outcome::Refused(PluginError::new(
                        ErrorCode::Cancelled,
                        format!("batch refused: submission {lowest}"),
                    )));
                }
            }
        } else {
            for (submission, verdict) in submissions.iter().zip(verdicts.iter()) {
                if let (Some(key), None) = (&submission.dedup_key, verdict) {
                    self.dedup
                        .insert(key.clone(), (self.now_ms, submission.correlation));
                }
            }
        }
        for (submission, verdict) in submissions.iter().zip(verdicts) {
            let needed_grant = if verdict.is_none() {
                intent_grant(&submission.intent)
            } else {
                None
            };
            self.call_order
                .calls
                .push(format!("submission:{}", submission.correlation));
            self.submitted.push(submission.clone());
            self.pending.push(PendingRequest {
                correlation: submission.correlation,
                needed_grant,
                fixed: verdict,
            });
        }
        submissions
    }

    /// Records a failure, schedules the restart or disables the plugin (DEV-490, DEV-498).
    fn record_failure(&mut self, code: &str, message: &str) {
        let now = self.now_ms;
        self.failures.push(FailureRecord {
            at_ms: now,
            code: code.to_string(),
        });
        self.failures
            .retain(|record| now.saturating_sub(record.at_ms) < FAILURE_WINDOW_MS);
        self.diagnostics.push(Diagnostic {
            code: code.to_string(),
            message: message.to_string(),
        });
        self.plugin.0 = None;
        let count = self.failures.len();
        match RESTART_DELAYS_MS.get(count.saturating_sub(1)) {
            Some(delay) if count >= 1 => {
                self.restarts.push(Restart {
                    at_ms: now,
                    delay_ms: *delay,
                });
                self.restart_due = Some(now + u64::from(*delay));
                self.state = InstanceState::RestartScheduled;
            }
            _ => {
                self.restart_due = None;
                self.disabled_by_failure = true;
                self.state = InstanceState::StoppedByFailure;
                self.diagnostics.push(Diagnostic {
                    code: "plugin.disabled-by-failure".to_string(),
                    message: format!("{count} failures within 10 minutes"),
                });
            }
        }
    }

    /// Runs a restart whose delay has passed; true when one ran.
    fn perform_due_restart(&mut self) -> bool {
        let Some(due) = self.restart_due else {
            return false;
        };
        if self.state != InstanceState::RestartScheduled || due > self.now_ms {
            return false;
        }
        self.restart_due = None;
        self.start_fresh_instance(None);
        true
    }

    /// Creates a new plugin object through the factory and activates it.
    fn start_fresh_instance(&mut self, restored: Option<Vec<u8>>) {
        let Some(factory) = self.factory.0.as_ref() else {
            return;
        };
        let mut plugin = factory();
        self.instance += 1;
        self.state = InstanceState::Activating;
        self.restored = restored.clone();
        let activation = self.build_activation(restored);
        match plugin.activate(activation) {
            Ok(()) => {
                self.plugin.0 = Some(plugin);
                self.state = InstanceState::Active;
                self.replay_tree();
            }
            Err(error) => {
                self.state = InstanceState::Failed;
                self.record_failure("trap.abi", &error.message);
            }
        }
    }

    /// Runs queued deliveries and completions until idle; fails with `harness.settle-limit` after 100 rounds.
    pub fn settle(&mut self) {
        self.require_started("settle");
        for _round in 0..=SETTLE_ROUNDS {
            let mut worked = self.perform_due_restart();
            if self.plugin.0.is_none() {
                self.pending.clear();
                self.outbox.borrow_mut().clear();
                if !worked {
                    return;
                }
                continue;
            }
            let completions = self.complete_all_pending();
            if !completions.is_empty() {
                worked = true;
                self.deliver_batch(completions);
            }
            let queued: Vec<Event> = self.outbox.borrow_mut().drain(..).collect();
            if !queued.is_empty() {
                worked = true;
                let mut rest = queued;
                while !rest.is_empty() {
                    let take = rest.len().min(MAX_BATCH_EVENTS);
                    let batch: Vec<Event> = rest.drain(..take).collect();
                    self.deliver_batch(batch);
                }
            }
            if !worked {
                return;
            }
        }
        fail(
            codes::SETTLE_LIMIT,
            &format!("the queues were not empty after {SETTLE_ROUNDS} rounds"),
        );
    }

    /// Completes every pending request with its outcome and returns the `request-completed` events.
    fn complete_all_pending(&mut self) -> Vec<Event> {
        let pending = std::mem::take(&mut self.pending);
        let mut events = Vec::new();
        for request in pending {
            let outcome = request
                .fixed
                .or_else(|| self.auto_outcome.clone())
                .unwrap_or(Outcome::Ok);
            self.outcomes.push(outcome.clone());
            events.push(Event::RequestCompleted(RequestResult {
                request: RequestRef::Submission(request.correlation),
                outcome,
            }));
        }
        events
    }

    /// Outcomes completed so far, in order.
    pub fn outcomes(&self) -> Vec<Outcome> {
        self.outcomes.clone()
    }

    /// Every submission the plugin made, in order.
    pub fn submitted(&self) -> Vec<Submission> {
        self.submitted.clone()
    }

    /// Requests the plugin made that the test has not completed.
    pub fn pending_requests(&self) -> Vec<RequestRef> {
        self.pending
            .iter()
            .map(|request| RequestRef::Submission(request.correlation))
            .collect()
    }

    /// Completes a pending request with an outcome.
    pub fn complete(&mut self, request: RequestRef, outcome: Outcome) {
        self.pending
            .retain(|pending| RequestRef::Submission(pending.correlation) != request);
        self.outcomes.push(outcome.clone());
        self.outbox
            .borrow_mut()
            .push(Event::RequestCompleted(RequestResult { request, outcome }));
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        FakeHost::new()
    }
}

impl FakeHost {
    // ---- views (best effort: the native entry has no view methods) ----

    fn require_active(&self, what: &str) {
        if self.plugin.0.is_none() {
            fail(codes::NOT_ACTIVE, &format!("{what} needs an active plugin"));
        }
    }

    /// Renders a view; the size must be 10..=500 by 1..=200 (DEV-440).
    pub fn render(&mut self, _target: ViewTarget, columns: u16, rows: u16) -> Screen {
        if !(10..=500).contains(&columns) || !(1..=200).contains(&rows) {
            fail(
                codes::RENDER_SIZE,
                &format!("{columns} by {rows} is outside 10..=500 by 1..=200"),
            );
        }
        self.require_active("render");
        Screen::blank(columns, rows)
    }

    /// Sends a widget action to a view.
    pub fn send_action(
        &mut self,
        _target: ViewTarget,
        _key: &str,
        _kind: ActionKind,
        _value: ActionValue,
    ) {
        self.require_active("send_action");
    }

    /// Clicks a cell of a view; true when a widget took the click.
    pub fn click(&mut self, _target: ViewTarget, _column: u16, _row: u16) -> bool {
        self.require_active("click");
        false
    }

    /// Sends a key to a view.
    pub fn key(&mut self, _target: ViewTarget, _key: &str) {
        self.require_active("key");
    }

    /// Hover text at a cell.
    pub fn hover(&mut self, _target: ViewTarget, _column: u16, _row: u16) -> Option<String> {
        None
    }

    /// The last UI tree a view produced.
    pub fn last_tree(&self, _target: ViewTarget) -> UiTree {
        UiTree {
            version: 1,
            nodes: Vec::new(),
            root: 0,
        }
    }

    /// Views the plugin invalidated.
    pub fn invalidated(&self) -> Vec<ViewTarget> {
        Vec::new()
    }

    /// Popups the plugin opened.
    pub fn popups(&self) -> Vec<String> {
        Vec::new()
    }

    /// The last notice the plugin showed.
    pub fn last_notice(&self) -> Option<String> {
        None
    }

    /// Actions the plugin offered.
    pub fn offered_actions(&self) -> Vec<String> {
        Vec::new()
    }

    /// Menu entries the plugin contributes for a context.
    pub fn menu_items(&mut self, _context: MenuContext) -> Vec<MenuEntry> {
        Vec::new()
    }

    /// Cancels every pending request.
    pub fn cancel_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for request in pending {
            self.complete(RequestRef::Submission(request.correlation), Outcome::Cancelled);
        }
    }

    // ---- clock and grants ----

    /// The fake clock at its current instant.
    pub fn clock(&self) -> FakeClock {
        FakeClock::at(self.now_ms)
    }

    /// Moves the clock forward; restarts run at the next `settle` (DEV-491).
    pub fn advance_clock(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
        self.fs.set_now(self.now_ms);
    }

    /// Sets the clock; moving it back fails with `harness.clock-backwards`.
    pub fn set_clock(&mut self, instant_ms: u64) {
        if instant_ms < self.now_ms {
            fail(
                codes::CLOCK_BACKWARDS,
                &format!("{instant_ms} is before {}", self.now_ms),
            );
        }
        self.now_ms = instant_ms;
        self.fs.set_now(self.now_ms);
    }

    /// Re-activates the same plugin object with the current grants.
    fn reactivate_cycle(&mut self, reason: DeactivationReason) {
        if self.plugin.0.is_none() {
            return;
        }
        let Some(mut plugin) = self.plugin.0.take() else {
            return;
        };
        let _ = plugin.deactivate(reason);
        self.instance += 1;
        self.state = InstanceState::Activating;
        let activation = self.build_activation(None);
        match plugin.activate(activation) {
            Ok(()) => {
                self.plugin.0 = Some(plugin);
                self.state = InstanceState::Active;
            }
            Err(error) => {
                self.state = InstanceState::Failed;
                self.record_failure("trap.abi", &error.message);
            }
        }
    }

    /// Grants a capability to a running plugin (DEV-412).
    pub fn grant_now(&mut self, name: &str) -> Result<(), HarnessError> {
        self.check_grant(name)?;
        self.granted.insert(name.to_string());
        self.reactivate_cycle(DeactivationReason::Granted);
        Ok(())
    }

    /// Revokes a capability from a running plugin; pending requests that need it fail as revoked (DEV-412).
    pub fn revoke(&mut self, name: &str) {
        let remaining: Vec<String> = self
            .granted
            .iter()
            .filter(|grant| grant.as_str() != name)
            .cloned()
            .collect();
        let names: Vec<&str> = remaining.iter().map(String::as_str).collect();
        self.grants_changed(&names);
    }

    /// Replaces the whole grant list.
    pub fn grants_changed(&mut self, names: &[&str]) {
        let new: BTreeSet<String> = names.iter().map(|name| (*name).to_string()).collect();
        let removed: Vec<String> = self.granted.difference(&new).cloned().collect();
        let added = new.difference(&self.granted).next().is_some();
        self.granted = new;
        if !removed.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            for request in pending {
                let revoked = request
                    .needed_grant
                    .map(|grant| removed.iter().any(|name| name == grant))
                    .unwrap_or(false);
                if revoked {
                    self.outcomes.push(Outcome::Failed(PluginError::new(
                        ErrorCode::Revoked,
                        "the grant was revoked",
                    )));
                } else {
                    self.pending.push(request);
                }
            }
            self.reactivate_cycle(DeactivationReason::Revoked);
        } else if added {
            self.reactivate_cycle(DeactivationReason::Granted);
        }
    }

    /// Records a missed deadline as a failure (DEV-495).
    pub fn breach_deadline(&mut self, export: &str) {
        self.require_active("breach_deadline");
        self.record_failure("trap.deadline", &format!("{export} missed its deadline"));
    }

    // ---- fake accessors ----

    /// The fake file system.
    pub fn fs(&mut self) -> &mut FakeFs {
        &mut self.fs
    }

    /// The fake process runner.
    pub fn process(&mut self) -> &mut FakeProcess {
        &mut self.process
    }

    /// Alias of [`FakeHost::process`].
    pub fn processes(&mut self) -> &mut FakeProcess {
        &mut self.process
    }

    /// The fake HTTP client.
    pub fn http(&mut self) -> &mut FakeHttp {
        &mut self.http
    }

    /// The fake storage.
    pub fn storage(&self) -> &FakeStorage {
        &self.storage
    }

    /// The fake storage, mutable.
    pub fn storage_mut(&mut self) -> &mut FakeStorage {
        &mut self.storage
    }

    /// The fake settings store.
    pub fn settings(&mut self) -> &mut FakeSettings {
        &mut self.settings
    }

    /// The fake tree.
    pub fn tree(&mut self) -> &mut FakeTree {
        &mut self.tree
    }

    /// The fake job runner.
    pub fn jobs(&mut self) -> &mut FakeJobs {
        &mut self.jobs
    }

    /// The fake CLI registry.
    pub fn cli(&mut self) -> &mut FakeCli {
        &mut self.cli
    }

    // ---- failure and state ----

    /// The next delivery fails with this trap code (DEV-492).
    pub fn expect_trap(&mut self, code: &str) {
        self.inject_trap(code);
    }

    /// The next delivery fails with this trap code without running the plugin.
    pub fn inject_trap(&mut self, code: &str) {
        if !codes::TRAP_CODES.contains(&code) {
            fail(codes::ERROR_UNKNOWN, &format!("{code} is not a trap code"));
        }
        self.pending_trap = Some(code.to_string());
    }

    /// Makes a host import fail; recorded only (the SDK host API is not simulated).
    pub fn inject_failure(&mut self, import: &str, error: PluginError) {
        let family = import.split(['.', '/', ':']).next().unwrap_or(import);
        if !IMPORT_FAMILIES.contains(&family) {
            fail(codes::ERROR_UNKNOWN, &format!("{import} is not a host import"));
        }
        self.injected_failures.push((import.to_string(), error));
    }

    /// Restarts decided so far.
    pub fn restarts(&self) -> Vec<Restart> {
        self.restarts.clone()
    }

    /// Whether the plugin is disabled by repeated failure or by the user.
    pub fn is_disabled(&self) -> bool {
        self.disabled_by_failure || self.disabled_by_user
    }

    /// Diagnostics recorded so far.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }

    /// Current settings as JSON text.
    pub(crate) fn settings_json_snapshot(&self) -> String {
        self.settings.as_json()
    }

    /// Plugin id the host reports to the plugin.
    pub(crate) fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Replaces the instance by a new one that restores from `bytes` (DEV-497).
    pub fn restart_with(&mut self, bytes: Vec<u8>) {
        self.require_active("restart_with");
        let _ = self.deactivate(DeactivationReason::Upgrade);
        self.start_fresh_instance(Some(bytes));
    }

    /// The next delivery runs, then records a crash.
    pub fn crash_after_journal_write(&mut self) {
        self.crash_armed = true;
    }

    /// Failures inside the 10 minute window, oldest first.
    pub fn failures(&self) -> Vec<FailureRecord> {
        self.failures
            .iter()
            .filter(|record| self.now_ms.saturating_sub(record.at_ms) < FAILURE_WINDOW_MS)
            .cloned()
            .collect()
    }

    /// Lifecycle state.
    pub fn lifecycle_state(&self) -> InstanceState {
        self.state
    }

    /// The bytes the plugin was restored from.
    pub fn restored(&self) -> Option<Vec<u8>> {
        self.restored.clone()
    }

    /// Log lines the plugin wrote (the SDK host API is not simulated, so none).
    pub fn logs(&self) -> Vec<LogLine> {
        Vec::new()
    }

    /// Events the loaded fixture asked to deliver.
    pub fn fixture_events(&self) -> Vec<Event> {
        self.fixture_events.clone()
    }

    pub(crate) fn set_fixture_events(&mut self, events: Vec<Event>) {
        self.fixture_events = events;
    }

    /// Applies the fixture's grants, tier and side where the builder did not set them (DEV-510).
    pub(crate) fn apply_fixture_basics(
        &mut self,
        grants: Option<&[String]>,
        tier: Option<&str>,
        side: Option<&str>,
    ) -> Result<(), HarnessError> {
        if let Some(grants) = grants {
            if self.granted.is_empty() {
                for name in grants {
                    self.check_grant(name)?;
                    self.granted.insert(name.clone());
                }
            }
        }
        if let Some(tier) = tier {
            if self.tier == "T1" {
                self.tier = tier.to_string();
            }
        }
        if let Some(side) = side {
            if self.explicit_side.is_none() {
                self.side = side.to_string();
            }
        }
        Ok(())
    }

    /// Writes one starting storage value; false for an unknown bucket name.
    pub(crate) fn seed_storage_value(&mut self, bucket_name: &str, key: &str, bytes: &[u8]) -> bool {
        match bucket_of(bucket_name) {
            Some(bucket) => {
                self.storage.put(bucket, key, bytes);
                true
            }
            None => false,
        }
    }

    /// State record for restore tests.
    pub fn snapshot(&self) -> HostSnapshot {
        HostSnapshot {
            instance: self.instance,
            state: self.state,
            storage: self.storage.dump_all(),
            restored: self.restored.clone(),
        }
    }

    /// Loads a fixture, delivers its events and runs its lifecycle steps.
    pub fn run_fixture(&mut self, path: impl AsRef<Path>) -> Result<(), HarnessError> {
        let fixture = crate::fixture::Fixture::load(path.as_ref())?;
        fixture.apply_to(self)?;
        fixture.run_steps(self)
    }

    // ---- hooks ----

    /// Calls a hook point of the plugin as the kernel would.
    pub fn run_hook(&mut self, call: HookCall) -> HookReply {
        if !self.exports.contains(&"hook-handler") {
            fail(
                codes::NOT_A_HOOK_HANDLER,
                "the plugin does not export hook-handler",
            );
        }
        self.require_active("run_hook");
        self.hook_runs
            .entry(call.point.clone())
            .or_default()
            .push(call.payload_json.clone());
        let (reply, trap) = self.extensions.answer_hook(&call);
        self.extensions.hook_log.push(call);
        if trap {
            self.record_failure("trap.panic", "the hook handler trapped");
        }
        reply
    }

    /// Hook calls received at a point.
    pub fn hook_calls(&self, point: &str) -> Vec<HookCall> {
        self.extensions
            .hook_log
            .iter()
            .filter(|call| call.point == point)
            .cloned()
            .collect()
    }

    /// Payloads of the hook calls received at a point.
    pub fn hook_payloads(&self, point: &str) -> Vec<String> {
        self.hook_calls(point)
            .into_iter()
            .map(|call| call.payload_json)
            .collect()
    }

    /// Every hook call.
    pub fn all_hook_calls(&self) -> Vec<HookCall> {
        self.extensions.hook_log.clone()
    }

    /// Payloads of the hook runs at a point.
    pub fn hook_runs(&self, point: &str) -> Vec<String> {
        self.hook_runs.get(point).cloned().unwrap_or_default()
    }

    /// Sets the reply for a hook point.
    pub fn set_hook_reply(&mut self, point: &str, reply: HookReply) {
        self.extensions.on_hook(point, reply);
    }

    /// Sets the reply for a hook point when the payload equals `when`.
    pub fn set_hook_reply_when(&mut self, point: &str, when: Value, reply: HookReply) {
        self.extensions
            .conditional_replies
            .entry(point.to_string())
            .or_default()
            .push((when, reply));
    }

    /// Makes the hook at a point trap.
    pub fn set_hook_trap(&mut self, point: &str) {
        self.extensions.on_hook_trap(point);
    }

    /// Makes the hook at a point fail.
    pub fn fail_hook(&mut self, point: &str) {
        self.extensions.on_hook_trap(point);
    }

    /// Installs a closure as the fake contributor at `point`.
    pub fn set_hook_handler(
        &mut self,
        point: &str,
        handler: impl Fn(HookCall) -> HookReply + 'static,
    ) {
        self.extensions
            .handlers
            .insert(point.to_string(), Opaque(Rc::new(handler)));
    }

    /// The chain of fake contributors at `point`.
    pub fn hook_chain(&mut self, point: &str) -> &mut HookChain {
        self.extensions.chains.entry(point.to_string()).or_default()
    }

    // ---- services ----

    /// Calls a service method the plugin exposes.
    pub fn call_service(
        &mut self,
        service: &str,
        method: &str,
        payload_json: &str,
    ) -> Result<String, PluginError> {
        self.extensions.service_log.push((
            service.to_string(),
            method.to_string(),
            payload_json.to_string(),
        ));
        self.call_order
            .calls
            .push(format!("service:{service}.{method}"));
        self.extensions.answer_service(service, method)
    }

    /// Calls the plugin's own single-method service.
    pub fn call_own_service(
        &mut self,
        service: &str,
        request_json: &str,
    ) -> Result<String, PluginError> {
        self.call_service(service, "call", request_json)
    }

    /// Service calls made so far as `(service, method, payload)`.
    pub fn service_calls(&self) -> Vec<(String, String, String)> {
        self.extensions.service_log.clone()
    }

    /// Stubs a service method reply.
    pub fn stub_service(&mut self, service: &str, method: &str, reply_json: &str) {
        self.extensions.service_stubs.insert(
            (service.to_string(), method.to_string()),
            Ok(reply_json.to_string()),
        );
    }

    /// Stubs a service method failure.
    pub fn stub_service_failure(&mut self, service: &str, method: &str, message: &str) {
        self.extensions.service_stubs.insert(
            (service.to_string(), method.to_string()),
            Err(message.to_string()),
        );
    }

    /// Sets the reply of every method of a service.
    pub fn set_service_reply(&mut self, service: &str, reply: impl IntoJsonText) {
        self.extensions.set_service_reply(service, reply);
    }

    /// Makes every method of a service fail.
    pub fn fail_service(&mut self, service: &str) {
        self.extensions.fail_service(service);
    }

    /// Runs one of the plugin's job kinds; the native entry has no job method, so it is unsupported.
    pub fn run_job(
        &mut self,
        kind: &str,
        input: impl AsRef<[u8]>,
    ) -> Result<Vec<u8>, PluginError> {
        let request = self.jobs_next_request();
        self.jobs.record(request, kind, input.as_ref());
        Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("the native entry has no job runner for {kind}"),
        ))
    }

    fn jobs_next_request(&mut self) -> u64 {
        self.call_order.next_request += 1;
        self.call_order.next_request
    }

    // ---- fuzz and replay ----

    /// Deterministic random events for a seed.
    pub fn fuzz_events(&self, seed: u64, count: u32) -> Vec<Event> {
        crate::fuzz::fuzz_events(seed, count)
    }

    /// Delivers `rounds` fuzz batches to the active plugin; fails when the plugin failed.
    pub fn fuzz_run(&mut self, seed: u64, rounds: u32) -> Result<(), FuzzFailure> {
        for round in 0..rounds {
            let before = self.failures.len();
            let events = crate::fuzz::fuzz_events(seed.wrapping_add(u64::from(round)), 16);
            self.deliver_batch(events);
            if self.failures.len() > before || self.plugin.0.is_none() {
                let code = self
                    .failures
                    .last()
                    .map(|record| record.code.clone())
                    .unwrap_or_else(|| "trap.panic".to_string());
                return Err(FuzzFailure { seed, round, code });
            }
        }
        Ok(())
    }

    /// Starts recording delivered events to a file.
    pub fn record_to(&mut self, path: &Path) {
        self.recording = Some(path.to_path_buf());
        crate::replay::record_to(path);
    }

    /// Replays a recording against the plugin.
    pub fn replay(&mut self, path: &Path) -> Result<(), ReplayMismatch> {
        crate::replay::replay(path)
    }

    // ---- environment ----

    /// Sets the project root.
    pub fn with_project_root(mut self, root: PathBuf) -> FakeHost {
        self.require_setup_open("with_project_root");
        self.project_root = root;
        self
    }

    /// Seeds settings from a JSON object.
    pub fn with_settings(mut self, settings: Value) -> FakeHost {
        self.require_setup_open("with_settings");
        if let Value::Object(map) = settings {
            for (key, value) in map {
                self.seed_settings_unchecked(&key, value);
            }
        }
        self
    }

    /// The project root.
    pub fn project_root(&self) -> PathBuf {
        self.project_root.clone()
    }

    /// Sets an environment variable of the fake process environment.
    pub fn set_env(&mut self, name: &str, value: &str) {
        self.env.insert(name.to_string(), value.to_string());
    }

    /// Sets the platform name.
    pub fn set_platform(&mut self, platform: &str) {
        self.platform = platform.to_string();
    }

    /// Seeds one setting without type checks.
    pub fn seed_settings_unchecked(&mut self, key: &str, value: Value) {
        let mut current: serde_json::Map<String, Value> =
            match serde_json::from_str(&self.settings.as_json()) {
                Ok(Value::Object(map)) => map,
                _ => serde_json::Map::new(),
            };
        current.insert(key.to_string(), value);
        self.settings.seed(&Value::Object(current).to_string());
    }

    // ---- kernel settings and inference ----

    /// The fake kernel settings.
    pub fn kernel_settings(&mut self) -> &mut FakeKernelSettings {
        &mut self.kernel_settings
    }

    /// The fake inference provider.
    pub fn inference(&mut self) -> &mut FakeInference {
        &mut self.inference
    }

    /// Inference requests the plugin made.
    pub fn inference_requests(&self) -> Vec<(RequestId, CompletionRequest)> {
        self.inference.requests()
    }

    /// Completes an inference request.
    pub fn complete_inference(&mut self, request: RequestId, completion: Completion) {
        self.inference.complete_inference(request, completion);
    }

    /// Scripts the next completions.
    pub fn script_inference(&mut self, replies: Vec<Result<String, PluginError>>) {
        self.inference.scripted.extend(replies);
    }

    // ---- instance and extension points ----

    /// The plugin object, for white-box assertions.
    pub fn instance_mut<T: NativeEntry + 'static>(&mut self) -> &mut T {
        let Some(plugin) = self.plugin.0.as_mut() else {
            fail(codes::NOT_ACTIVE, "instance_mut needs an active plugin");
        };
        match plugin.as_any_mut().downcast_mut::<T>() {
            Some(instance) => instance,
            None => fail("harness.instance-type", "the plugin is not of the requested type"),
        }
    }

    /// Contributes an entry to a collect point as another plugin.
    pub fn contribute_extension(
        &mut self,
        point: &str,
        id: &str,
        priority: impl Into<i64>,
        entry: impl IntoJsonText,
    ) {
        self.extensions.contribute(point, id, priority, entry);
    }

    /// Contributes an entry without a priority.
    pub fn contribute_entry(&mut self, point: &str, id: &str, entry_json: &str) {
        self.extensions.contribute(point, id, 0, entry_json);
    }

    /// Contributes an anonymous entry to a collect point.
    pub fn contribute_collect(&mut self, point: &str, entry_json: &str) {
        let id = format!("entry-{}", self.extensions.entries_len(point));
        self.extensions.contribute(point, &id, 0, entry_json);
    }

    /// Replaces the entries of a point.
    pub fn set_extension_entries(&mut self, point: &str, entries: Vec<impl IntoJsonText>) {
        self.extensions.clear_point(point);
        for (index, entry) in entries.into_iter().enumerate() {
            self.extensions
                .contribute(point, &format!("entry-{index}"), 0, entry);
        }
    }

    /// Withdraws one entry.
    pub fn withdraw_extension(&mut self, point: &str, id: &str) {
        self.extensions.withdraw(point, id);
    }

    /// Entries of a point, by priority.
    pub fn extension_entries(&self, point: &str) -> Vec<ExtensionEntry> {
        self.extensions.entries(point)
    }

    /// How often the plugin listed the point (the host API is not simulated, so zero).
    pub fn extension_list_calls(&self, point: &str) -> usize {
        self.extensions.list_calls.get(point).copied().unwrap_or(0)
    }

    /// A collect point.
    pub fn collect_point(&mut self, point: &str) -> &mut CollectPoint {
        self.extensions.points.entry(point.to_string()).or_default()
    }

    /// The fake extension registry.
    pub fn extensions(&mut self) -> &mut FakeExtensions {
        &mut self.extensions
    }

    /// The fake client requests.
    pub fn client_requests(&mut self) -> &mut FakeClientRequests {
        &mut self.client_requests
    }

    /// The fake agents.
    pub fn agents(&mut self) -> &mut FakeAgents {
        &mut self.agents
    }

    /// The fake terminal.
    pub fn terminal(&mut self) -> &mut FakeTerminal {
        &mut self.terminal
    }

    /// The fake journal.
    pub fn journal(&mut self) -> &mut FakeJournal {
        &mut self.journal
    }

    /// The fake keymap.
    pub fn keymap(&mut self) -> &mut FakeKeymap {
        &mut self.keymap
    }

    /// The fake plugin registry.
    pub fn plugins(&mut self) -> &mut FakePlugins {
        &mut self.plugins
    }

    /// The fake help store.
    pub fn help(&mut self) -> &mut FakeHelp {
        &mut self.help
    }

    /// The fake clipboard.
    pub fn clipboard(&mut self) -> &mut FakeClipboard {
        &mut self.clipboard
    }

    /// The fake vector store.
    pub fn vector(&mut self) -> &mut FakeVector {
        &mut self.vector
    }

    /// The fake audio devices.
    pub fn audio(&mut self) -> &mut FakeAudio {
        &mut self.audio
    }

    /// The fake key grabs.
    pub fn keys(&mut self) -> &mut FakeKeys {
        &mut self.keys
    }

    /// The fake workspace registry.
    pub fn workspace(&mut self) -> &mut FakeWorkspace {
        &mut self.workspace
    }

    /// The fake pane registry.
    pub fn panes(&mut self) -> &mut FakePanes {
        &mut self.panes
    }

    /// The fake sessions.
    pub fn sessions(&mut self) -> &mut FakeSessions {
        &mut self.sessions
    }

    // ---- topics, timers and other plugin-initiated output (the SDK host API is not simulated) ----

    /// Topics the plugin published as `(topic, payload)`.
    pub fn published_topics(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Payloads published on a topic.
    pub fn published(&self, _topic: &str) -> Vec<String> {
        Vec::new()
    }

    /// Durable timers as `(name, due)`.
    pub fn durable_timers(&self) -> Vec<(String, u64)> {
        Vec::new()
    }

    /// Active timers as `(id, repeat)`.
    pub fn active_timers(&self) -> Vec<(u32, Option<u32>)> {
        Vec::new()
    }

    /// HTTP replies the plugin sent to route calls.
    pub fn http_replies(&self) -> Vec<HttpReply> {
        Vec::new()
    }

    /// Output filters the plugin installed.
    pub fn output_filters(&self) -> Vec<(u64, OutputFilter)> {
        Vec::new()
    }

    // ---- lifecycle and ordering ----

    /// Disables the plugin as the user would.
    pub fn disable_plugin(&mut self) {
        if self.plugin.0.is_some() {
            let _ = self.deactivate(DeactivationReason::Disabled);
        }
        self.disabled_by_user = true;
    }

    /// Enables the plugin again and starts a new instance.
    pub fn enable_plugin(&mut self) {
        self.disabled_by_user = false;
        if self.plugin.0.is_none() && self.factory.0.is_some() {
            self.start_fresh_instance(None);
        }
    }

    /// Outcome used for every request the test does not complete itself.
    pub fn auto_outcome(&mut self, outcome: Outcome) {
        self.auto_outcome = Some(outcome);
    }

    /// Order of fake calls and submissions.
    pub fn order_of_calls_and_submissions(&self) -> CallOrder {
        self.call_order.clone()
    }
}

/// Text forms accepted where the harness takes a JSON document.
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

/// Fake of the kernel settings tabs a plugin may read or write.
#[derive(Debug, Default)]
pub struct FakeKernelSettings {
    tabs: BTreeMap<String, Value>,
    unreadable: BTreeSet<String>,
    writes: Vec<(String, String, String)>,
}

impl FakeKernelSettings {
    /// Sets the values of a tab.
    pub fn set_tab(&mut self, tab: &str, values: Value) {
        self.tabs.insert(tab.to_string(), values);
    }

    /// Makes a tab unreadable and unwritable.
    pub fn set_unreadable(&mut self, tab: &str) {
        self.unreadable.insert(tab.to_string());
    }

    /// The values of a tab.
    pub fn tab(&self, tab: &str) -> Option<Value> {
        if self.unreadable.contains(tab) {
            return None;
        }
        self.tabs.get(tab).cloned()
    }

    /// One value of a tab.
    pub fn get(&self, tab: &str, key: &str) -> Option<Value> {
        self.tab(tab)?.get(key).cloned()
    }

    /// Writes one value; fails on an unreadable tab.
    pub fn set(&mut self, tab: &str, key: &str, value: Value) -> Result<(), PluginError> {
        if self.unreadable.contains(tab) {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                format!("the {tab} tab is not writable"),
            ));
        }
        self.writes
            .push((tab.to_string(), key.to_string(), value.to_string()));
        let entry = self
            .tabs
            .entry(tab.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(map) = entry {
            map.insert(key.to_string(), value);
        }
        Ok(())
    }

    /// Writes made so far as `(tab, key, value json)`.
    pub fn writes(&self) -> Vec<(String, String, String)> {
        self.writes.clone()
    }

    /// Alias of [`FakeKernelSettings::writes`].
    pub fn written(&self) -> Vec<(String, String, String)> {
        self.writes()
    }
}

/// Fake inference provider.
#[derive(Debug, Default)]
pub struct FakeInference {
    requests: Vec<(RequestId, CompletionRequest)>,
    replies: Vec<(u64, Result<Completion, PluginError>)>,
    scripted: Vec<Result<String, PluginError>>,
    completions: Vec<(CompletionRequest, Completion)>,
    failure: Option<String>,
    text: Option<String>,
}

impl FakeInference {
    /// Completes a request with a completion.
    pub fn complete_inference(&mut self, request: RequestId, completion: Completion) {
        self.replies.push((request.0, Ok(completion)));
    }

    /// Requests made to the provider.
    pub fn requests(&self) -> Vec<(RequestId, CompletionRequest)> {
        self.requests.clone()
    }

    /// Replies given as `(request, result)`.
    pub fn provider_replies(&self) -> Vec<(u64, Result<Completion, PluginError>)> {
        self.replies.clone()
    }

    /// Answers requests equal to `request` with `completion`.
    pub fn script_completion(&mut self, request: CompletionRequest, completion: Completion) {
        self.completions.push((request, completion));
    }

    /// Answers every request with this text.
    pub fn reply_text(&mut self, text: &str) {
        self.text = Some(text.to_string());
    }

    /// Fails the next request with this message.
    pub fn fail_next(&mut self, message: &str) {
        self.failure = Some(message.to_string());
    }
}

/// An entry contributed to an extension point.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionEntry {
    /// Entry id.
    pub id: String,
    /// Priority; lower runs first.
    pub priority: i64,
    /// The entry document.
    pub entry: Value,
}

/// A collect point with its contributed entries.
#[derive(Debug, Default)]
pub struct CollectPoint {
    entries: Vec<ExtensionEntry>,
}

impl CollectPoint {
    /// Adds or replaces an entry.
    pub fn add(&mut self, id: &str, priority: impl Into<i64>, entry: Value) {
        self.entries.retain(|existing| existing.id != id);
        self.entries.push(ExtensionEntry {
            id: id.to_string(),
            priority: priority.into(),
            entry,
        });
    }
}

/// Fake contributors of a hook point.
#[derive(Debug, Default)]
pub struct HookChain {
    items: Vec<(String, i64, HookReply)>,
    all: Option<HookReply>,
    last_payload: Option<Value>,
}

impl HookChain {
    /// Adds a contributor.
    pub fn add(&mut self, id: &str, priority: impl Into<i64>, reply: HookReply) {
        self.items.retain(|(existing, _, _)| existing != id);
        self.items.push((id.to_string(), priority.into(), reply));
        self.items.sort_by_key(|(_, priority, _)| *priority);
    }

    /// Makes every contributor answer this.
    pub fn reply_all(&mut self, reply: HookReply) {
        self.all = Some(reply);
    }

    /// Payload of the last call.
    pub fn last_payload(&self) -> Option<Value> {
        self.last_payload.clone()
    }
}

/// Fake extension registry: collect points, hook contributors and service stubs.
#[derive(Debug, Default)]
pub struct FakeExtensions {
    points: BTreeMap<String, CollectPoint>,
    chains: BTreeMap<String, HookChain>,
    replies: BTreeMap<String, HookReply>,
    conditional_replies: BTreeMap<String, Vec<(Value, HookReply)>>,
    traps: BTreeSet<String>,
    handlers: BTreeMap<String, Opaque<Rc<dyn Fn(HookCall) -> HookReply>>>,
    hook_log: Vec<HookCall>,
    service_stubs: BTreeMap<(String, String), Result<String, String>>,
    service_replies: BTreeMap<String, String>,
    failed_services: BTreeSet<String>,
    service_log: Vec<(String, String, String)>,
    profiles: Vec<String>,
    list_calls: BTreeMap<String, usize>,
}

impl FakeExtensions {
    /// Contributes an entry to a point.
    pub fn contribute(
        &mut self,
        point: &str,
        id: &str,
        priority: impl Into<i64>,
        entry: impl IntoJsonText,
    ) {
        let text = entry.into_json_text();
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        self.points
            .entry(point.to_string())
            .or_default()
            .add(id, priority, value);
    }

    /// Contributes an entry to a collect point.
    pub fn contribute_collect(
        &mut self,
        point: &str,
        id: &str,
        priority: impl Into<i64>,
        entry: impl IntoJsonText,
    ) {
        self.contribute(point, id, priority, entry);
    }

    /// Contributes a profile document.
    pub fn contribute_profile(&mut self, profile_json: &str) {
        self.profiles.push(profile_json.to_string());
    }

    /// Entry documents of a point, by priority.
    pub fn collect(&self, point: &str) -> Vec<Value> {
        self.entries(point)
            .into_iter()
            .map(|entry| entry.entry)
            .collect()
    }

    fn entries(&self, point: &str) -> Vec<ExtensionEntry> {
        let mut entries = self
            .points
            .get(point)
            .map(|collect| collect.entries.clone())
            .unwrap_or_default();
        entries.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }

    fn entries_len(&self, point: &str) -> usize {
        self.points
            .get(point)
            .map(|collect| collect.entries.len())
            .unwrap_or(0)
    }

    fn clear_point(&mut self, point: &str) {
        self.points.remove(point);
    }

    fn withdraw(&mut self, point: &str, id: &str) {
        if let Some(collect) = self.points.get_mut(point) {
            collect.entries.retain(|entry| entry.id != id);
        }
    }

    /// Sets the reply at a hook point.
    pub fn on_hook(&mut self, point: &str, reply: HookReply) {
        self.replies.insert(point.to_string(), reply);
    }

    /// Alias of [`FakeExtensions::on_hook`].
    pub fn on_run_hook(&mut self, point: &str, reply: HookReply) {
        self.on_hook(point, reply)
    }

    /// Makes the hook at a point trap.
    pub fn on_hook_trap(&mut self, point: &str) {
        self.traps.insert(point.to_string());
    }

    /// Payloads of the calls received at a point.
    pub fn hook_calls(&self, point: &str) -> Vec<String> {
        self.hook_log
            .iter()
            .filter(|call| call.point == point)
            .map(|call| call.payload_json.clone())
            .collect()
    }

    /// Alias of [`FakeExtensions::hook_calls`].
    pub fn hook_payloads(&self, point: &str) -> Vec<String> {
        self.hook_calls(point)
    }

    /// Sets the reply of every method of a service.
    pub fn set_service_reply(&mut self, service: &str, reply: impl IntoJsonText) {
        self.service_replies
            .insert(service.to_string(), reply.into_json_text());
    }

    /// Makes every method of a service fail.
    pub fn fail_service(&mut self, service: &str) {
        self.failed_services.insert(service.to_string());
    }

    /// Chooses the reply for a hook call; the flag is true when the handler traps.
    fn answer_hook(&mut self, call: &HookCall) -> (HookReply, bool) {
        let payload: Value = serde_json::from_str(&call.payload_json).unwrap_or(Value::Null);
        if let Some(chain) = self.chains.get_mut(&call.point) {
            chain.last_payload = Some(payload.clone());
        }
        if self.traps.contains(&call.point) {
            return (HookReply::Pass, true);
        }
        if let Some(handler) = self.handlers.get(&call.point) {
            return ((handler.0)(call.clone()), false);
        }
        if let Some(conditions) = self.conditional_replies.get(&call.point) {
            if let Some((_, reply)) = conditions.iter().find(|(when, _)| *when == payload) {
                return (reply.clone(), false);
            }
        }
        if let Some(reply) = self.replies.get(&call.point) {
            return (reply.clone(), false);
        }
        if let Some(chain) = self.chains.get(&call.point) {
            if let Some(reply) = &chain.all {
                return (reply.clone(), false);
            }
            if let Some((_, _, reply)) = chain.items.first() {
                return (reply.clone(), false);
            }
        }
        (HookReply::Pass, false)
    }

    /// Chooses the reply for a service call.
    fn answer_service(&self, service: &str, method: &str) -> Result<String, PluginError> {
        if self.failed_services.contains(service) {
            return Err(PluginError::new(
                ErrorCode::Internal,
                format!("service {service} failed"),
            ));
        }
        if let Some(stub) = self
            .service_stubs
            .get(&(service.to_string(), method.to_string()))
        {
            return stub
                .clone()
                .map_err(|message| PluginError::new(ErrorCode::Internal, message));
        }
        if let Some(reply) = self.service_replies.get(service) {
            return Ok(reply.clone());
        }
        Ok("{\"ok\":true}".to_string())
    }
}

/// A request the server side sent to a client.
#[derive(Debug, Clone, PartialEq)]
pub struct SentClientRequest {
    /// Request kind.
    pub kind: String,
    /// Target client.
    pub client: u64,
    /// Request reference.
    pub request: RequestRef,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Payload JSON.
    pub payload_json: String,
}

/// An answer a client gave.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientResponse {
    /// Call id.
    pub call: u64,
    /// The result.
    pub result: Result<String, PluginError>,
}

/// Fake client request channel.
#[derive(Debug, Default)]
pub struct FakeClientRequests {
    sent: Vec<SentClientRequest>,
    responses: Vec<ClientResponse>,
}

impl FakeClientRequests {
    /// Requests sent so far.
    pub fn sent(&self) -> Vec<SentClientRequest> {
        self.sent.clone()
    }

    /// Answers a request.
    pub fn answer(&mut self, request: RequestRef, result: Result<String, PluginError>) {
        let call = match request {
            RequestRef::Submission(correlation) => correlation,
            RequestRef::Call(id) => id.0,
        };
        self.responses.push(ClientResponse { call, result });
    }

    /// Answers given so far.
    pub fn responses(&self) -> Vec<ClientResponse> {
        self.responses.clone()
    }
}

/// A transcript given to the fake agent.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeTranscript {
    /// Raw bytes.
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

/// Fake agent registry.
#[derive(Debug, Default)]
pub struct FakeAgents {
    transcripts: BTreeMap<u64, FakeTranscript>,
    verified: BTreeMap<u64, bool>,
    tokens: BTreeMap<u64, u64>,
    sessions: BTreeMap<u64, Option<SessionRef>>,
    models: BTreeMap<u64, AgentModel>,
    signatures: Vec<SignatureSpec>,
}

impl FakeAgents {
    /// Sets the transcript of a pane.
    pub fn set_transcript(&mut self, pane: u64, transcript: impl Into<FakeTranscript>) {
        self.transcripts.insert(pane, transcript.into());
    }

    /// Marks the transcript as verified or not.
    pub fn set_transcript_verified(&mut self, pane: u64, verified: bool) {
        self.verified.insert(pane, verified);
    }

    /// Sets the used context tokens.
    pub fn set_context_used_tokens(&mut self, pane: u64, tokens: u64) {
        self.tokens.insert(pane, tokens);
    }

    /// The transcript bytes of a pane.
    pub fn transcript(&self, pane: u64) -> Option<Vec<u8>> {
        match self.transcripts.get(&pane)? {
            FakeTranscript::Bytes(bytes) => Some(bytes.clone()),
            FakeTranscript::Turns(turns) => Some(format!("{turns:?}").into_bytes()),
        }
    }

    /// Backups taken of a transcript as `(name, bytes)`.
    pub fn transcript_backups(&self, _pane: u64) -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }

    /// Sets the agent session of a pane.
    pub fn set_session(&mut self, pane: u64, session: impl Into<Option<SessionRef>>) {
        self.sessions.insert(pane, session.into());
    }

    /// Sets the model of a pane.
    pub fn set_model(&mut self, pane: u64, model: AgentModel) {
        self.models.insert(pane, model);
    }

    /// Adds a detection signature.
    pub fn add_signature(&mut self, signature: SignatureSpec) {
        self.signatures.push(signature);
    }
}

/// Fake terminal contents.
#[derive(Debug, Default)]
pub struct FakeTerminal {
    rows: BTreeMap<u64, Vec<String>>,
    scrollback: BTreeMap<u64, u64>,
    history: BTreeMap<u64, String>,
    selections: BTreeMap<u64, Selection>,
    claims: BTreeMap<u64, Vec<Option<PointerModifiers>>>,
}

impl FakeTerminal {
    /// Sets the visible rows of a pane.
    pub fn set_rows(&mut self, pane: u64, rows: &[&str]) {
        self.rows
            .insert(pane, rows.iter().map(|row| (*row).to_string()).collect());
    }

    /// Sets the visible rows and the scrollback length.
    pub fn set_screen(&mut self, pane: u64, rows: &[&str], scrollback_lines: u64) {
        self.set_rows(pane, rows);
        self.scrollback.insert(pane, scrollback_lines);
    }

    /// Sets the history text.
    pub fn set_history(&mut self, pane: u64, history: &str) {
        self.history.insert(pane, history.to_string());
    }

    /// Sets the selection.
    pub fn set_selection(&mut self, pane: u64, selection: Selection) {
        self.selections.insert(pane, selection);
    }

    /// Pointer claims made on a pane.
    pub fn pointer_claims(&self, pane: u64) -> Vec<Option<PointerModifiers>> {
        self.claims.get(&pane).cloned().unwrap_or_default()
    }
}

/// Fake pane journal.
#[derive(Debug, Default)]
pub struct FakeJournal {
    entries: Vec<(Option<u64>, String, String)>,
}

impl FakeJournal {
    /// Appends an entry.
    pub fn append(&mut self, pane: u64, _at_ms: u64, kind: &str, text: &str) {
        self.entries
            .push((Some(pane), kind.to_string(), text.to_string()));
    }

    /// Entries as `(pane, kind, text)`.
    pub fn entries(&self) -> Vec<(Option<u64>, String, String)> {
        self.entries.clone()
    }
}

/// Fake keymap.
#[derive(Debug, Default)]
pub struct FakeKeymap {
    bindings: Vec<Binding>,
    labels: BTreeMap<String, String>,
}

impl FakeKeymap {
    /// Replaces the bindings.
    pub fn set_bindings(&mut self, bindings: Vec<Binding>) {
        self.bindings = bindings;
    }

    /// Sets the label of an action.
    pub fn set_label(&mut self, action: &str, label: &str) {
        self.labels.insert(action.to_string(), label.to_string());
    }
}

/// Fake plugin registry.
#[derive(Debug, Default)]
pub struct FakePlugins {
    plugins: BTreeMap<String, (String, bool)>,
    enabled_calls: Vec<(String, bool)>,
    next_error: Option<PluginError>,
    failed: BTreeSet<String>,
}

impl FakePlugins {
    /// Adds a plugin.
    pub fn add_simple(&mut self, id: &str, name: &str, enabled: bool) {
        self.plugins
            .insert(id.to_string(), (name.to_string(), enabled));
    }

    /// Calls of set-enabled as `(id, enabled)`.
    pub fn set_enabled_calls(&self) -> Vec<(String, bool)> {
        self.enabled_calls.clone()
    }

    /// Fails the next set-enabled call.
    pub fn fail_next_set_enabled(&mut self, error: PluginError) {
        self.next_error = Some(error);
    }

    /// Marks a plugin as failed.
    pub fn mark_failed(&mut self, id: &str) {
        self.failed.insert(id.to_string());
    }
}

/// Fake help topics.
#[derive(Debug, Default)]
pub struct FakeHelp {
    topics: BTreeMap<String, String>,
}

impl FakeHelp {
    /// Adds a topic.
    pub fn add_topic(&mut self, id: &str, text: &str) {
        self.topics.insert(id.to_string(), text.to_string());
    }
}

/// Fake clipboard.
#[derive(Debug, Default)]
pub struct FakeClipboard {
    text: Option<String>,
    failure: Option<String>,
}

impl FakeClipboard {
    /// Last text written.
    pub fn last_text(&self) -> Option<String> {
        self.text.clone()
    }

    /// Fails the next write.
    pub fn fail_next_write(&mut self, message: &str) {
        self.failure = Some(message.to_string());
    }
}

/// Fake vector index.
#[derive(Debug, Default)]
pub struct FakeVector {
    batch_sizes: Vec<usize>,
    top_k: Vec<usize>,
    hits: Vec<(String, f64)>,
}

impl FakeVector {
    /// Sizes of the embed batches.
    pub fn embed_batch_sizes(&self) -> Vec<usize> {
        self.batch_sizes.clone()
    }

    /// The `k` of each top-k request.
    pub fn top_k_requests(&self) -> Vec<usize> {
        self.top_k.clone()
    }

    /// Sets the hits every top-k request returns.
    pub fn set_top_k(&mut self, hits: Vec<(String, f64)>) {
        self.hits = hits;
    }
}

/// One audio capture.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioCapture {
    /// Capture id.
    pub id: u64,
    /// Whether it is still open.
    pub open: bool,
    /// Device name.
    pub device: Option<String>,
    /// Sample rate.
    pub sample_rate_hz: u32,
    /// Channel count.
    pub channels: u16,
}

/// Fake audio devices.
#[derive(Debug, Default)]
pub struct FakeAudio {
    captures: Vec<AudioCapture>,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl FakeAudio {
    /// Captures opened so far.
    pub fn captures(&self) -> Vec<AudioCapture> {
        self.captures.clone()
    }

    /// Sets the input devices.
    pub fn set_input_devices(&mut self, devices: Vec<String>) {
        self.inputs = devices;
    }

    /// Sets the output devices.
    pub fn set_output_devices(&mut self, devices: Vec<String>) {
        self.outputs = devices;
    }

    /// Plugs in a device.
    pub fn plug(&mut self, device: &str) {
        if !self.inputs.iter().any(|name| name == device) {
            self.inputs.push(device.to_string());
        }
    }

    /// Unplugs a device.
    pub fn unplug(&mut self, device: &str) {
        self.inputs.retain(|name| name != device);
        self.outputs.retain(|name| name != device);
    }
}

/// One key grab.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyGrab {
    /// Grab id.
    pub id: u64,
    /// Whether it was released.
    pub released: bool,
    /// Chords grabbed.
    pub chords: Vec<String>,
    /// Pane limit.
    pub pane: Option<u64>,
    /// Whether it lasts until key release.
    pub until_release: bool,
}

/// Fake key grabs.
#[derive(Debug, Default)]
pub struct FakeKeys {
    grabs: Vec<KeyGrab>,
}

impl FakeKeys {
    /// Grabs made so far.
    pub fn grabs(&self) -> Vec<KeyGrab> {
        self.grabs.clone()
    }
}

/// Fake workspace registry.
#[derive(Debug, Default)]
pub struct FakeWorkspace {
    registered: Vec<WorkspaceSpec>,
    foreign: Vec<(String, String)>,
}

impl FakeWorkspace {
    /// Workspaces the plugin registered.
    pub fn registered(&self) -> Vec<WorkspaceSpec> {
        self.registered.clone()
    }

    /// Adds a workspace owned by someone else.
    pub fn add_foreign(&mut self, path: &str, branch: &str) {
        self.foreign.push((path.to_string(), branch.to_string()));
    }
}

/// Fake pane registry.
#[derive(Debug, Default)]
pub struct FakePanes {
    running_in: Vec<String>,
}

impl FakePanes {
    /// Adds a pane running in a directory.
    pub fn add_running_in(&mut self, path: &str) {
        self.running_in.push(path.to_string());
    }
}

/// Fake cross-session registry.
#[derive(Debug, Default)]
pub struct FakeSessions {
    live: Vec<(String, String, bool, String)>,
    unreachable: Vec<(String, String)>,
    requests: Vec<(String, String, String)>,
}

impl FakeSessions {
    /// Adds a live session.
    pub fn add_live(&mut self, name: &str, path: &str, is_self: bool, panes_json: &str) {
        self.live.push((
            name.to_string(),
            path.to_string(),
            is_self,
            panes_json.to_string(),
        ));
    }

    /// Adds an unreachable session.
    pub fn add_unreachable(&mut self, name: &str, path: &str) {
        self.unreachable.push((name.to_string(), path.to_string()));
    }

    /// Requests made to sessions as `(session, kind, payload)`.
    pub fn requests(&self) -> Vec<(String, String, String)> {
        self.requests.clone()
    }
}

/// Order of fake calls and submissions.
#[derive(Debug, Default, Clone)]
pub struct CallOrder {
    calls: Vec<String>,
    next_request: u64,
}

impl CallOrder {
    /// Whether the process call came before the submission.
    pub fn process_before_submission(&self, process: &str, submission: &str) -> bool {
        let call = self
            .calls
            .iter()
            .position(|entry| *entry == format!("process:{process}"));
        let sent = self
            .calls
            .iter()
            .position(|entry| *entry == format!("submission:{submission}"));
        matches!((call, sent), (Some(call), Some(sent)) if call < sent)
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
