//! Shared helpers: hosts for both sides, node builders, JSONL readers and submission readers.
//!
//! Every use of a testkit item that the skeleton lacks lives here (see ../API_GAPS.md), so a
//! missing item breaks one file only.

use std::path::PathBuf;

use ilium_plugin_core_fleet::{FleetClient, FleetServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, AgentView, Delivery, Event, HttpCalledEvent, Intent, MessageEvent,
    NodeClass, NodeId, Outcome, PluginId, PluginSide, SendInputSpec, Submission, UiAction, ViewTarget,
};
use serde_json::{Value, json};

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/fleet).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-fleet`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    page("core-plugin-fleet")
}

/// Path of a golden file in this directory.
pub fn golden_path(name: &str) -> String {
    format!("{}/tests/features/golden/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.activate::<FleetServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<FleetClient>();
    host
}

/// Adds an agent pane in the given activity to the fake tree and returns its id.
pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str, activity: AgentActivity) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView { signature: signature.to_string(), activity, objective: None, goal: None }),
    })
}

/// Adds a plain shell pane (no agent) to the fake tree and returns its id.
pub fn add_shell(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// One pane record as the `list-panes` cross-session reply carries it (cli-reference.md, `ilium panes`).
pub fn pane_json(pane_id: u64, name: &str, kind: &str, agent: Option<&str>, state: Option<&str>) -> Value {
    json!({
        "pane_id": pane_id,
        "name": name,
        "kind": kind,
        "agent": agent,
        "state": state,
        "project": "/home/me/dev/ilium",
        "cwd": "/home/me/dev/ilium",
        "is_self": false,
    })
}

/// Registers a reachable live session whose `list-panes` reply is the given panes.
pub fn add_session(host: &mut FakeHost, name: &str, is_self: bool, panes: &[Value]) {
    host.sessions().add_live(name, "/home/me/dev/ilium", is_self, &Value::Array(panes.to_vec()).to_string());
}

/// Registers a live session that cannot be read (`session-unreachable`).
pub fn add_unreachable_session(host: &mut FakeHost, name: &str) {
    host.sessions().add_unreachable(name, "/home/me/dev/ilium");
}

/// Parses JSONL text into one value per non-empty line.
pub fn records(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("not JSON: {line:?}: {error}")))
        .collect()
}

/// Records of one `type`.
pub fn of_type<'a>(all: &'a [Value], kind: &str) -> Vec<&'a Value> {
    all.iter().filter(|record| record["type"] == kind).collect()
}

/// The `summary` record (exactly one is expected).
pub fn summary(all: &[Value]) -> Value {
    let found = of_type(all, "summary");
    assert_eq!(found.len(), 1, "one summary record expected in {all:?}");
    found[0].clone()
}

/// Runs `ilium <args>` through the fake command line and returns (stdout, stderr, exit code).
pub fn run_cli(host: &mut FakeHost, args: &[&str], stdin: &[u8]) -> (String, String, Option<u8>) {
    let invocation = host.cli().invoke(args, stdin);
    (invocation.stdout(), invocation.stderr(), invocation.exit_code())
}

/// Runs the command line as if typed in the given pane (`ILIUM_PANE_ID`).
pub fn run_cli_in_pane(host: &mut FakeHost, pane: u64, args: &[&str]) -> (String, String, Option<u8>) {
    host.cli().set_env("ILIUM_PANE_ID", &pane.to_string());
    run_cli(host, args, b"")
}

/// A `message` event as the other side of the plugin sends it.
pub fn message(channel: &str, payload: Vec<u8>) -> Event {
    Event::Message(MessageEvent {
        from: PluginId("ilium.core.fleet".to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: channel.to_string(),
        payload,
    })
}

/// A `http-called` event.
pub fn http_call(call: u64, method: &str, path: &str, body: &[u8]) -> Event {
    Event::HttpCalled(HttpCalledEvent {
        call,
        method: method.to_string(),
        path: path.to_string(),
        headers: Vec::new(),
        body: body.to_vec(),
    })
}

/// A `ui-action` event for the dialog popup.
pub fn dialog_action(key: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Popup("ask-for-update".to_string()),
        key: key.to_string(),
        action_id: String::new(),
        kind,
        value,
    })
}

/// Every `send-input` spec of a submission list, in order.
pub fn send_inputs(submissions: &[Submission]) -> Vec<SendInputSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SendInput(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// Every `send-input` spec the host has seen so far (CLI runs submit through the host, not through a return value).
pub fn sent_inputs(host: &FakeHost) -> Vec<SendInputSpec> {
    send_inputs(&host.submitted())
}

/// The pane ids that received a `send-input`, in order.
pub fn sent_panes(host: &FakeHost) -> Vec<NodeId> {
    sent_inputs(host).into_iter().map(|spec| spec.pane).collect()
}

/// Every `create-pane` spec the host has seen.
pub fn created_panes(host: &FakeHost) -> Vec<ilium_plugin_sdk::types::CreatePaneSpec> {
    host.submitted()
        .into_iter()
        .filter_map(|submission| match submission.intent {
            Intent::CreatePane(spec) => Some(spec),
            _ => None,
        })
        .collect()
}

/// The `Delivery::Submitted` marker, for readability in asserts.
pub const SUBMITTED: Delivery = Delivery::Submitted;

/// Makes every later submission complete with the given outcome.
pub fn auto_outcome(host: &mut FakeHost, outcome: Outcome) {
    host.auto_outcome(outcome);
}
