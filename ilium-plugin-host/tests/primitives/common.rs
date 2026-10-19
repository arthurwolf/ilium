//! Shared helpers of the primitives tests: guests, a rig that wires a harness to a fake kernel
//! with panes, outcome readers and manual-page readers. Everything outside the skeleton is
//! named in API_GAPS.md.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_plugin_api::generated::primitives::terminal::CellRows;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::generated::types::tree::{AgentActivity, AgentView, NodeClass};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::kernel::TreeNode;
use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::fake_kernel::FakePaneScreen;
use ilium_plugin_host::testing::{FakeKernel, HarnessHandle, HostHarness, NativeGuest};

/// A live terminal pane.
pub const PANE: NodeId = NodeId(10);
/// A terminal pane restored from a snapshot whose process has not started.
pub const RESTORED_PANE: NodeId = NodeId(11);
/// An editor pane.
pub const EDITOR_PANE: NodeId = NodeId(12);
/// A basic pane.
pub const BASIC_PANE: NodeId = NodeId(13);
/// A node id that is not in the tree.
pub const MISSING_PANE: NodeId = NodeId(999);

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// Plugin id the harness gives its plugin (DEV-420: `test.plugin`).
pub fn plugin_id() -> PluginId {
    PluginId("test.plugin".to_owned())
}

/// One event that makes the harness call `on-events` once.
pub fn one_event() -> Vec<Event> {
    vec![Event::ThemeChanged]
}

/// A guest that returns the queued submissions on the next `on-events` call.
pub struct ScriptGuest {
    pub queue: Arc<Mutex<VecDeque<Submission>>>,
    pub seen: Arc<Mutex<Vec<Event>>>,
}

impl NativeGuest for ScriptGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.seen.lock().expect("seen lock").extend(batch);
        Ok(self.queue.lock().expect("queue lock").drain(..).collect())
    }
}

/// A harness, its fake kernel and the script queue of its guest.
pub struct Rig {
    pub kernel: Arc<FakeKernel>,
    pub host: HarnessHandle,
    pub queue: Arc<Mutex<VecDeque<Submission>>>,
    pub seen: Arc<Mutex<Vec<Event>>>,
    next_correlation: u64,
}

impl Rig {
    /// Submits one intent through the guest, runs the dispatcher and returns its outcome.
    pub fn submit(&mut self, intent: Intent) -> Outcome {
        self.submit_keyed(intent, None)
    }

    /// Same with a dedup key.
    pub fn submit_keyed(&mut self, intent: Intent, dedup_key: Option<&str>) -> Outcome {
        self.next_correlation += 1;
        let correlation = self.next_correlation;
        self.queue
            .lock()
            .expect("queue lock")
            .push_back(Submission {
                intent,
                correlation,
                dedup_key: dedup_key.map(str::to_owned),
            });
        self.host.deliver(one_event());
        self.host.pump(8);
        self.host
            .outcomes(&plugin_id())
            .into_iter()
            .rev()
            .find(|result| result.request == RequestRef::Submission(correlation))
            .map(|result| result.outcome)
            .expect("the dispatcher completes the submitted intent")
    }

    /// Events the guest received so far, as kebab-case names.
    pub fn seen_names(&self) -> Vec<String> {
        self.seen
            .lock()
            .expect("seen lock")
            .iter()
            .map(|event| event.case_name().to_owned())
            .collect()
    }
}

fn pane_node(id: NodeId, title: &str, content_kind: &str) -> TreeNode {
    TreeNode {
        id,
        parent: None,
        class: NodeClass::Pane,
        content_kind: Some(content_kind.to_owned()),
        title: title.to_owned(),
        cwd: None,
        agent: None,
        locked: false,
        revision: 1,
    }
}

/// The standard tree: a live terminal pane, a restored terminal pane, an editor pane and a basic
/// pane.
pub fn standard_tree() -> Vec<TreeNode> {
    vec![
        pane_node(PANE, "shell", "ilium.terminal"),
        pane_node(RESTORED_PANE, "restored", "ilium.terminal"),
        pane_node(EDITOR_PANE, "notes.md", "ilium.editor"),
        pane_node(BASIC_PANE, "scratch", "example.scratchpad.notes"),
    ]
}

/// A tree node for an agent pane.
pub fn agent_node(id: NodeId, signature: &str, activity: AgentActivity) -> TreeNode {
    let mut node = pane_node(id, "agent", "ilium.terminal");
    node.agent = Some(AgentView {
        signature: signature.to_owned(),
        activity,
        objective: None,
        goal: None,
    });
    node
}

/// A rig for a T3 plugin on `side` with the given grants.
pub fn rig_on(side: Side, grants: &[&str]) -> Rig {
    let process = match side {
        Side::Server => ProcessKind::Server,
        _ => ProcessKind::Client,
    };
    let kernel = Arc::new(FakeKernel::new(process));
    kernel.set_tree(standard_tree());
    kernel.set_screen(PANE, screen_of(24, 80));
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let host = HostHarness::new()
        .tier(Tier::T3)
        .side(side)
        .grants(grants)
        .kernel(kernel.clone())
        .with_native_guest(Box::new(ScriptGuest {
            queue: queue.clone(),
            seen: seen.clone(),
        }))
        .build()
        .expect("the harness builds a host for a native guest");
    Rig {
        kernel,
        host,
        queue,
        seen,
        next_correlation: 0,
    }
}

/// A client-side rig (terminal, editor and basic primitives live on the client).
pub fn rig(grants: &[&str]) -> Rig {
    rig_on(Side::Client, grants)
}

/// A server-side rig (the agent primitive and detectors live on the server).
pub fn server_rig(grants: &[&str]) -> Rig {
    rig_on(Side::Server, grants)
}

/// A screen of `rows` rows of `columns` characters, row n holding `row-<n>` padded with dots.
pub fn screen_of(rows: usize, columns: usize) -> FakePaneScreen {
    FakePaneScreen {
        rows: (0..rows)
            .map(|row| padded(&format!("row-{row}"), columns))
            .collect(),
        history: Vec::new(),
    }
}

/// A screen with `history` scrollback lines of exactly `line_bytes` bytes, the last line ending in
/// `last-<n>` (line_bytes counts the newline).
pub fn screen_with_history(history_lines: usize, line_bytes: usize) -> FakePaneScreen {
    FakePaneScreen {
        rows: vec![padded("visible", 80)],
        history: (0..history_lines)
            .map(|index| padded(&format!("hist-{index}"), line_bytes - 1))
            .collect(),
    }
}

fn padded(text: &str, width: usize) -> String {
    let mut line = text.to_owned();
    while line.len() < width {
        line.push('.');
    }
    line
}

/// True for the accepted outcomes.
pub fn accepted(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_)
    )
}

/// The refusal code of an outcome, if it is refused.
pub fn refusal_of(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Refused(error) => Some(error.code),
        _ => None,
    }
}

/// The error code of a sync result.
pub fn error_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> Option<ErrorCode> {
    result.err().map(|error| error.code)
}

/// Rows of `columns` empty cells, `count` rows.
pub fn cell_rows_len(rows: &Option<CellRows>) -> Option<usize> {
    rows.as_ref().map(|rows| rows.rows.len())
}

/// All captured log text joined with newlines.
pub fn log_text(host: &HarnessHandle) -> String {
    host.logs()
        .iter()
        .map(|line| format!("{} {} {}", line.plugin, line.level, line.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Number of log lines whose message contains the needle.
pub fn log_count(host: &HarnessHandle, needle: &str) -> usize {
    host.logs()
        .iter()
        .filter(|line| line.message.contains(needle))
        .count()
}

/// Moves the manual clock by whole milliseconds.
pub fn advance_ms(host: &mut HarnessHandle, millis: u64) {
    host.advance_clock(Duration::from_millis(millis));
}

/// Root of the workspace.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Bytes of a built guest fixture; fails red with `fixture.missing` until the toolchain builds it.
pub fn fixture_wasm(name: &str) -> Vec<u8> {
    let path = workspace_root()
        .join("plugins/fixtures")
        .join(name)
        .join("dist")
        .join(format!("{name}.wasm"));
    assert!(path.exists(), "fixture.missing: {}", path.display());
    std::fs::read(&path).expect("fixture file is readable")
}

/// Compares `actual` with `tests/primitives/golden/<name>`; a missing golden file fails red with
/// `golden.missing`.
pub fn assert_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/primitives/golden")
        .join(name);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("golden.missing: {}: {error}", path.display()));
    assert_eq!(
        expected.replace("\r\n", "\n"),
        actual.replace("\r\n", "\n"),
        "golden {name}"
    );
}

/// A client-side rig whose plugin is a built wasm fixture (the gap `HostHarness::with_wasm_bytes`).
pub fn rig_with_wasm(wasm: &[u8], grants: &[&str]) -> Rig {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
    kernel.set_tree(standard_tree());
    let host = HostHarness::new()
        .tier(Tier::T3)
        .side(Side::Client)
        .grants(grants)
        .kernel(kernel.clone())
        .with_wasm_bytes(wasm)
        .build()
        .expect("the harness builds a host for the fixture component");
    Rig {
        kernel,
        host,
        queue: Arc::new(Mutex::new(VecDeque::new())),
        seen: Arc::new(Mutex::new(Vec::new())),
        next_correlation: 0,
    }
}
