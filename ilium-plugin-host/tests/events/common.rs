//! Shared helpers of the events-intents lane: a scripted native guest, a setup builder over the
//! host harness, event and intent builders, outcome predicates and manual readers.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::types::{
    Activation, AgentActionSpec, AgentActivity, AgentActivityEvent, AgentView, DeactivationReason,
    Delivery, DroppedCount, ErrorCode, Event, FreezePaneSpec, Intent, NodeClass, NodeId, NodeView,
    OutputMatchedEvent, Outcome, PaneExitedEvent, PaneResizedEvent, PluginError, PluginId,
    RenameNodeSpec, RequestRef, RequestResult, ScreenChangedEvent, SendInputSpec,
    SetNodeLockedSpec, Submission, TimerId, TitleSource,
};
use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::{ProcessKind, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::dispatch::PumpReport;
use ilium_plugin_host::error::HostError;
use ilium_plugin_host::testing::{FakeKernel, HarnessHandle, HostHarness, NativeGuest};

/// Poison-proof lock.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What the scripted guest saw and what it is told to do.
#[derive(Default)]
pub struct Shared {
    /// Every batch handed to `on-events`, in call order.
    pub batches: Mutex<Vec<Vec<Event>>>,
    /// Submissions to return from the next `on-events` calls, one list per call.
    pub replies: Mutex<VecDeque<Vec<Submission>>>,
    /// Every `activate` call.
    pub activations: Mutex<Vec<Activation>>,
    /// Every `deactivate` call.
    pub deactivations: Mutex<Vec<DeactivationReason>>,
    /// "activate", "on-events", "deactivate" in call order.
    pub sequence: Mutex<Vec<String>>,
    /// When set, `activate` returns this error.
    pub activate_error: Mutex<Option<PluginError>>,
    /// Runs inside `activate` before it returns (to produce events while activating).
    pub activate_hook: Mutex<Option<Box<dyn Fn() + Send>>>,
    /// Milliseconds `on-events` blocks before it returns.
    pub block_millis: Mutex<u64>,
    /// The bytes `deactivate` returns.
    pub saved_state: Mutex<Option<Vec<u8>>>,
}

impl Shared {
    /// Every event received, flattened.
    pub fn events(&self) -> Vec<Event> {
        lock(&self.batches).iter().flatten().cloned().collect()
    }

    /// Events of one case received.
    pub fn events_of(&self, case: &str) -> Vec<Event> {
        self.events().into_iter().filter(|event| event.case_name() == case).collect()
    }

    /// Number of events of one case received.
    pub fn count_of(&self, case: &str) -> usize {
        self.events_of(case).len()
    }

    /// Queues the submissions for the next `on-events` call.
    pub fn push_reply(&self, submissions: Vec<Submission>) {
        lock(&self.replies).push_back(submissions);
    }

    /// The id the host gave the plugin at activation.
    pub fn plugin(&self) -> PluginId {
        lock(&self.activations)
            .first()
            .map(|activation| activation.plugin.clone())
            .unwrap_or_default()
    }

    /// Number of activations so far.
    pub fn activation_count(&self) -> usize {
        lock(&self.activations).len()
    }
}

/// The guest the harness runs: it records everything and answers from `Shared`.
pub struct ScriptedGuest(pub Arc<Shared>);

impl NativeGuest for ScriptedGuest {
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        lock(&self.0.activations).push(activation);
        lock(&self.0.sequence).push("activate".to_owned());
        if let Some(hook) = lock(&self.0.activate_hook).as_ref() {
            hook();
        }
        match lock(&self.0.activate_error).clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        lock(&self.0.deactivations).push(reason);
        lock(&self.0.sequence).push("deactivate".to_owned());
        lock(&self.0.saved_state).clone()
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        let block = *lock(&self.0.block_millis);
        if block > 0 {
            std::thread::sleep(std::time::Duration::from_millis(block));
        }
        lock(&self.0.sequence).push("on-events".to_owned());
        lock(&self.0.batches).push(batch);
        Ok(lock(&self.0.replies).pop_front().unwrap_or_default())
    }
}

/// A running host under test.
pub struct Env {
    /// The harness handle.
    pub host: HarnessHandle,
    /// What the guest saw.
    pub probe: Arc<Shared>,
    /// The fake kernel behind the host.
    pub kernel: Arc<FakeKernel>,
}

/// Builder for [`Env`].
pub struct Setup {
    pub grants: Vec<&'static str>,
    pub tier: Tier,
    pub side: Side,
    pub limits: Limits,
    pub nodes: Vec<NodeView>,
    pub probe: Arc<Shared>,
    pub kernel: Arc<FakeKernel>,
    pub component: Option<Vec<u8>>,
}

impl Setup {
    /// A T3 server-side plugin holding these grants.
    pub fn new(grants: &[&'static str]) -> Setup {
        Setup {
            grants: grants.to_vec(),
            tier: Tier::T3,
            side: Side::Server,
            limits: Limits::default(),
            nodes: Vec::new(),
            probe: Arc::new(Shared::default()),
            kernel: Arc::new(FakeKernel::new(ProcessKind::Server)),
            component: None,
        }
    }

    pub fn tier(mut self, tier: Tier) -> Setup {
        self.tier = tier;
        self
    }

    pub fn side(mut self, side: Side) -> Setup {
        self.side = side;
        self
    }

    pub fn limit(mut self, key: &str, value: u64) -> Setup {
        self.limits.0.insert(key.to_owned(), value);
        self
    }

    pub fn tree(mut self, nodes: Vec<NodeView>) -> Setup {
        self.nodes = nodes;
        self
    }

    /// Runs a built guest component instead of the scripted native guest.
    pub fn component(mut self, name: &str) -> Setup {
        self.component = Some(fixture_component(name));
        self
    }

    /// Builds and activates; keeps the build result for activation-failure tests.
    pub fn try_build(self) -> (Result<HarnessHandle, HostError>, Arc<Shared>, Arc<FakeKernel>) {
        self.kernel.set_tree(self.nodes.clone());
        let mut harness = HostHarness::new()
            .tier(self.tier)
            .side(self.side)
            .grants(&self.grants)
            .limits(self.limits.clone())
            .kernel(self.kernel.clone());
        harness = match &self.component {
            Some(bytes) => harness.with_wasm_bytes(bytes),
            None => harness.with_native_guest(Box::new(ScriptedGuest(self.probe.clone()))),
        };
        (harness.build(), self.probe, self.kernel)
    }

    /// Builds and activates, panicking when the harness cannot be built.
    pub fn build(self) -> Env {
        let (built, probe, kernel) = self.try_build();
        let host = match built {
            Ok(host) => host,
            Err(error) => panic!("harness build failed: {error}"),
        };
        Env { host, probe, kernel }
    }
}

/// Events that always make the host call `on-events` (own timer, no grant needed).
pub fn trigger() -> Event {
    Event::TimerFired(TimerId(0))
}

impl Env {
    /// Runs the dispatcher until quiet (bounded).
    pub fn settle(&self) -> PumpReport {
        self.host.pump(64)
    }

    /// Queues events and settles.
    pub fn deliver(&self, events: Vec<Event>) {
        self.host.deliver(events);
        self.settle();
    }

    /// The plugin id.
    pub fn plugin(&self) -> PluginId {
        self.probe.plugin()
    }

    /// Outcomes waiting for the plugin.
    pub fn outcomes(&self) -> Vec<RequestResult> {
        self.host.outcomes(&self.plugin())
    }

    /// The outcome of the submission with this correlation.
    pub fn outcome(&self, correlation: u64) -> Option<Outcome> {
        outcome_in(&self.outcomes(), correlation)
    }

    /// Returns these submissions from the next `on-events` call and settles.
    pub fn run(&self, submissions: Vec<Submission>) {
        self.probe.push_reply(submissions);
        self.host.deliver(vec![trigger()]);
        self.settle();
    }

    /// Intents the kernel applied, in order.
    pub fn applied(&self) -> Vec<Intent> {
        self.kernel.applied_intents().into_iter().map(|(_, intent)| intent).collect()
    }
}

/// Finds the outcome of one correlation.
pub fn outcome_in(results: &[RequestResult], correlation: u64) -> Option<Outcome> {
    results
        .iter()
        .find(|result| result.request == RequestRef::Submission(correlation))
        .map(|result| result.outcome.clone())
}

/// How many results carry this correlation.
pub fn count_for(results: &[RequestResult], correlation: u64) -> usize {
    results.iter().filter(|result| result.request == RequestRef::Submission(correlation)).count()
}

/// The error code of a refused or failed outcome.
pub fn code_of(outcome: &Option<Outcome>) -> Option<ErrorCode> {
    match outcome {
        Some(Outcome::Refused(error)) | Some(Outcome::Failed(error)) => Some(error.code),
        _ => None,
    }
}

/// The message of a refused or failed outcome.
pub fn message_of(outcome: &Option<Outcome>) -> String {
    match outcome {
        Some(Outcome::Refused(error)) | Some(Outcome::Failed(error)) => error.message.clone(),
        _ => String::new(),
    }
}

pub fn is_refused(outcome: &Option<Outcome>, code: ErrorCode) -> bool {
    matches!(outcome, Some(Outcome::Refused(error)) if error.code == code)
}

pub fn is_failed(outcome: &Option<Outcome>, code: ErrorCode) -> bool {
    matches!(outcome, Some(Outcome::Failed(error)) if error.code == code)
}

pub fn is_ok(outcome: &Option<Outcome>) -> bool {
    matches!(outcome, Some(Outcome::Ok) | Some(Outcome::OkNode(_)) | Some(Outcome::OkEpoch(_)))
}

pub fn is_duplicate_of(outcome: &Option<Outcome>, correlation: u64) -> bool {
    matches!(outcome, Some(Outcome::Duplicate(RequestRef::Submission(first))) if *first == correlation)
}

// ---- trees -------------------------------------------------------------------------------

pub fn node(id: u64, parent: Option<u64>, class: NodeClass, title: &str) -> NodeView {
    NodeView {
        id: NodeId(id),
        parent: parent.map(NodeId),
        class,
        content_kind: None,
        title: title.to_owned(),
        cwd: None,
        agent: None,
        locked: false,
        revision: 1,
    }
}

/// A terminal pane.
pub fn pane(id: u64, parent: Option<u64>) -> NodeView {
    let mut view = node(id, parent, NodeClass::Pane, "pane");
    view.content_kind = Some("ilium.terminal".to_owned());
    view
}

/// A terminal pane with an identified agent in the given activity.
pub fn agent_pane(id: u64, parent: Option<u64>, activity: AgentActivity) -> NodeView {
    let mut view = pane(id, parent);
    view.agent = Some(AgentView {
        signature: "claude-code".to_owned(),
        activity,
        objective: None,
        goal: None,
    });
    view
}

pub fn folder(id: u64, parent: Option<u64>) -> NodeView {
    node(id, parent, NodeClass::Folder, "folder")
}

pub fn locked(mut view: NodeView) -> NodeView {
    view.locked = true;
    view
}

/// A root container 1 with panes 2..=count+1 below it.
pub fn flat_tree(count: u64) -> Vec<NodeView> {
    let mut nodes = vec![node(1, None, NodeClass::Container, "root")];
    nodes.extend((0..count).map(|index| pane(index + 2, Some(1))));
    nodes
}

// ---- events ------------------------------------------------------------------------------

pub fn output_matched(index: u32) -> Event {
    output_matched_for("f", index)
}

pub fn output_matched_for(filter: &str, index: u32) -> Event {
    Event::OutputMatched(OutputMatchedEvent {
        node: NodeId(2),
        filter_id: filter.to_owned(),
        line: format!("line {index}"),
        row: index,
    })
}

pub fn outputs(count: u32) -> Vec<Event> {
    (0..count).map(output_matched).collect()
}

pub fn pane_exited(node: u64) -> Event {
    Event::PaneExited(PaneExitedEvent { node: NodeId(node), status: Some(0), signal: None })
}

pub fn exits(count: u64) -> Vec<Event> {
    (0..count).map(|index| pane_exited(index + 1)).collect()
}

pub fn screen_changed(pane: u64, revision: u64) -> Event {
    Event::ScreenChanged(ScreenChangedEvent {
        pane: NodeId(pane),
        first_row: 0,
        last_row: 23,
        revision,
    })
}

pub fn pane_resized(node: u64, columns: u16) -> Event {
    Event::PaneResized(PaneResizedEvent { node: NodeId(node), columns, rows: 24 })
}

pub fn activity_changed(node: u64, revision: u64) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision,
    })
}

pub fn tree_changed(ids: &[u64]) -> Event {
    Event::TreeChanged(ids.iter().copied().map(NodeId).collect())
}

pub fn node_removed(node: u64) -> Event {
    Event::NodeRemoved(NodeId(node))
}

/// The ids carried by the tree-changed events received, one list per event.
pub fn tree_changed_lists(events: &[Event]) -> Vec<Vec<u64>> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TreeChanged(ids) => Some(ids.iter().map(|id| id.0).collect()),
            _ => None,
        })
        .collect()
}

/// The records of every events-dropped event received.
pub fn dropped_records(events: &[Event]) -> Vec<DroppedCount> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::EventsDropped(records) => Some(records.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

// ---- intents -----------------------------------------------------------------------------

pub fn sub(correlation: u64, intent: Intent) -> Submission {
    Submission { intent, correlation, dedup_key: None }
}

pub fn keyed(correlation: u64, key: &str, intent: Intent) -> Submission {
    Submission { intent, correlation, dedup_key: Some(key.to_owned()) }
}

pub fn rename(node: u64, title: &str) -> Intent {
    Intent::RenameNode(RenameNodeSpec {
        node: NodeId(node),
        title: title.to_owned(),
        source: TitleSource::Plugin,
        expected_revision: None,
    })
}

pub fn close(node: u64) -> Intent {
    Intent::CloseNode(NodeId(node))
}

pub fn set_locked(node: u64, locked: bool) -> Intent {
    Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(node), locked })
}

pub fn freeze(pane: u64) -> Intent {
    Intent::FreezePane(FreezePaneSpec { pane: NodeId(pane) })
}

pub fn send_input(pane: u64, text: &str) -> Intent {
    Intent::SendInput(SendInputSpec {
        pane: NodeId(pane),
        text: text.to_owned(),
        delivery: Delivery::Typed,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    })
}

pub fn send_input_journaled(pane: u64, text: &str) -> Intent {
    match send_input(pane, text) {
        Intent::SendInput(mut spec) => {
            spec.journal = true;
            Intent::SendInput(spec)
        }
        other => other,
    }
}

pub fn agent_action(pane: u64, action: &str) -> Intent {
    Intent::AgentAction(AgentActionSpec { pane: NodeId(pane), action_id: action.to_owned() })
}

// ---- manual and fixtures -------------------------------------------------------------------

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Text of a manual page (without extension).
pub fn manual_page(name: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{name}.md"));
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("manual page {} unreadable: {error}", path.display()),
    }
}

/// Text of a WIT file below the workspace `wit/` directory.
pub fn wit_file(relative: &str) -> String {
    let path = workspace_root().join("wit").join(relative);
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("wit file {} unreadable: {error}", path.display()),
    }
}

/// A built guest component. Fails red with `fixture.missing` until the toolchain builds it.
pub fn fixture_component(name: &str) -> Vec<u8> {
    let path = workspace_root().join("target/plugin-fixtures").join(format!("{name}.wasm"));
    assert!(path.exists(), "fixture.missing: {}", path.display());
    match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => panic!("fixture.missing: {} ({error})", path.display()),
    }
}

/// The log lines of the plugin whose message contains `needle`.
pub fn log_lines_containing(env: &Env, needle: &str) -> Vec<String> {
    env.host
        .logs()
        .into_iter()
        .filter(|line| line.message.contains(needle))
        .map(|line| line.message)
        .collect()
}
