//! Shared helpers of the Cost feature tests (manual page core-plugin-cost.md).
//!
//! Every use of a testkit item that the skeleton lacks lives in the "gap helpers" section at the
//! end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_cost::{CostClient, CostServer};
use ilium_plugin_sdk::exports::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::primitives::{TranscriptTurn, TurnRole};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentModel, AgentModelEvent, AgentView, Event, Intent, NodeClass,
    NodeId, PluginId, SetDecorationSpec, Submission,
};
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.cost";
pub const PANEL: &str = "cost.panel";
pub const SLOT: &str = "cost";
pub const USAGE_SERVICE: &str = "ilium.core.cost/usage";
/// A model the built-in price table knows (src/docs/agent-cost.md "Prices and [cost.prices]").
pub const KNOWN_MODEL: &str = "claude-sonnet-5-5";
/// A model no table knows.
pub const UNKNOWN_MODEL: &str = "mystery-model-9";
/// The model the page example prices at zero with the pattern `qwen3-*`.
pub const LOCAL_MODEL: &str = "qwen3-32b";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-cost`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-cost")
}

/// A user document of the repository by file name, for example `agent-cost.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

pub fn golden_path(name: &str) -> String {
    format!("{}/tests/features/golden/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else { break };
        blocks.push(after[..end].to_string());
        rest = &after[end + 4..];
    }
    blocks
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else { break };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the table row of `text` whose first cell is `first_cell` (backticks kept).
pub fn table_row(text: &str, first_cell: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            cells.len() > 2 && cells[1] == first_cell
        })
        .unwrap_or_else(|| panic!("no table row starting with {first_cell}"));
    let mut cells: Vec<String> = line.split('|').map(|cell| cell.trim().to_string()).collect();
    cells.remove(0);
    cells.pop();
    cells
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build()
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build()
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<CostServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<CostClient>();
    host
}

/// An agent pane in the fake tree; returns its node id.
pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity: AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    })
}

pub fn model_event(pane: u64, model: &str) -> Event {
    Event::AgentModelChanged(AgentModelEvent {
        pane: NodeId(pane),
        model: AgentModel {
            model: model.to_string(),
            provider: None,
            context_window: None,
            source: "transcript".to_string(),
        },
    })
}

pub fn idle_event(pane: u64) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(pane),
        previous: AgentActivity::Working,
        current: AgentActivity::Idle,
        revision: 1,
    })
}

/// One assistant turn that carries `tokens`.
pub fn assistant_turn(tokens: u32) -> TranscriptTurn {
    TranscriptTurn { role: TurnRole::Assistant, text: String::new(), tool: None, at: None, tokens: Some(tokens) }
}

/// Every `set-decoration` of the slot `cost` in `submissions`, in order.
pub fn cost_decorations(submissions: &[Submission]) -> Vec<SetDecorationSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetDecoration(spec) if spec.slot == SLOT => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The plain text of a decoration (all spans joined).
pub fn decoration_text(spec: &SetDecorationSpec) -> String {
    spec.content.iter().map(|span| span.text.as_str()).collect()
}

/// The latest `cost` decoration for `pane` among the submissions made after `already_seen`.
pub fn latest_cost_decoration(host: &FakeHost, already_seen: usize, pane: u64) -> Option<SetDecorationSpec> {
    let submitted = host.submitted();
    cost_decorations(&submitted[already_seen..]).into_iter().filter(|spec| spec.node == NodeId(pane)).last()
}

/// Calls the usage service the way the host does: through the `service-provider` export.
pub fn call_usage(method: &str, payload_json: &str) -> ServiceReply {
    use ilium_plugin_sdk::ServiceProvider;
    let mut server = CostServer::default();
    server.handle(ServiceCall {
        call: 1,
        service: USAGE_SERVICE.to_string(),
        method: method.to_string(),
        payload_json: payload_json.to_string(),
        caller: PluginId("ilium.core.fleet".to_string()),
    })
}

/// The JSON of a `ServiceReply::Done`; panics (inside the test) for any other reply.
pub fn done_json(reply: ServiceReply) -> Value {
    match reply {
        ServiceReply::Done(text) => serde_json::from_str(&text).expect("reply is JSON"),
        ServiceReply::Deferred => panic!("the usage service answers at once with Done"),
    }
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: every item below is missing from the skeleton testkit (see ../API_GAPS.md).
// ---------------------------------------------------------------------------------------------

/// Gives the pane a model and a transcript with `tokens` in one assistant turn, then lets the
/// plugin see the model change and the pane going idle (it requests the transcript then).
pub fn feed_pane(host: &mut FakeHost, pane: u64, model: &str, tokens: u32) {
    host.agents().set_model(
        pane,
        AgentModel {
            model: model.to_string(),
            provider: None,
            context_window: None,
            source: "transcript".to_string(),
        },
    );
    host.agents().set_transcript(pane, vec![assistant_turn(tokens)]);
    host.deliver(vec![model_event(pane, model), idle_event(pane)]);
    host.settle();
}

/// Sets the stored values of one kernel settings tab, as the kernel would return them from `read`.
pub fn set_kernel_tab(host: &mut FakeHost, tab: &str, values: Value) {
    host.kernel_settings().set_tab(tab, values);
}

/// Every `kernel-settings.write` the plugin made: (tab, key, JSON value text), in order.
pub fn kernel_writes(host: &mut FakeHost) -> Vec<(String, String, String)> {
    host.kernel_settings().written()
}

/// Every topic the plugin published: (topic, payload JSON text), in order.
pub fn published_topics(host: &FakeHost) -> Vec<(String, String)> {
    host.published_topics()
}
