//! Shared helpers of the Workspace search feature tests (manual page core-plugin-search.md).
//!
//! Every use of an item that the skeleton lacks, or of a wire format that no page names, lives in
//! the "gap helpers" section at the end of this file (see ../API_GAPS.md).
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_search::{SearchClient, SearchServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ClientId, CommandContext, CommandInvokedEvent, CommandOrigin, Event,
    Intent, MessageEvent, NodeClass, PluginId, PluginSide, Submission, UiAction, ViewTarget,
};
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.search";
pub const PANEL: &str = "search";
pub const OPEN_COMMAND: &str = "ilium.core.search/open";
pub const QUERY_SERVICE: &str = "ilium.core.search/query";
/// Channel the client sends a query on (gap helper, see API_GAPS.md).
pub const QUERY_CHANNEL: &str = "ilium.core.search/query";
/// Channel the server answers on (gap helper, see API_GAPS.md).
pub const RESULTS_CHANNEL: &str = "ilium.core.search/results";

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

/// The text of a manual page by file stem, for example `core-plugin-search`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-search")
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
    host.activate::<SearchServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<SearchClient>();
    host
}

pub fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

pub fn open_command_event() -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: OPEN_COMMAND.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Keybinding,
        context: CommandContext::default(),
    })
}

/// A `ui-action` of the panel with a value.
pub fn action(key: &str, kind: ActionKind, value: ActionValue) -> Event {
    Event::UiAction(UiAction {
        view: panel(),
        key: key.to_string(),
        action_id: String::new(),
        kind,
        value: Some(value),
    })
}

pub fn text_changed(text: &str) -> Event {
    action("query", ActionKind::Changed, ActionValue::Text(text.to_string()))
}

pub fn filter_changed(text: &str) -> Event {
    action("query", ActionKind::FilterChanged, ActionValue::Text(text.to_string()))
}

/// The intents of the submissions, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions.iter().map(|submission| submission.intent.clone()).collect()
}

/// Every `open-view` for the panel among the submissions.
pub fn open_views(submissions: &[Submission]) -> Vec<Submission> {
    submissions
        .iter()
        .filter(|submission| matches!(&submission.intent, Intent::OpenView(spec) if spec.view == panel()))
        .cloned()
        .collect()
}

pub fn close_views(submissions: &[Submission]) -> Vec<Submission> {
    submissions
        .iter()
        .filter(|submission| matches!(&submission.intent, Intent::CloseView(view) if *view == panel()))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: items missing from the skeleton, or wire formats no page names (../API_GAPS.md).
// ---------------------------------------------------------------------------------------------

/// A client-to-server query message the way the plugin's client side sends it: every `message`
/// intent on `QUERY_CHANNEL` with its JSON payload `{ "revision", "query" }`.
pub fn query_messages(submissions: &[Submission]) -> Vec<Value> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) if spec.channel == QUERY_CHANNEL => {
                Some(serde_json::from_slice(&spec.payload).expect("query payload is JSON"))
            }
            _ => None,
        })
        .collect()
}

/// The message event the server receives when client `client` asks for `query`.
pub fn query_event(client: u64, revision: u64, query: &str) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Client,
        from_client: Some(ClientId(client)),
        channel: QUERY_CHANNEL.to_string(),
        payload: serde_json::to_vec(&json!({ "revision": revision, "query": query })).expect("json"),
    })
}

/// The message event the client receives when the server answers `revision` with `hits`.
pub fn results_event(revision: u64, hits: Value) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: RESULTS_CHANNEL.to_string(),
        payload: serde_json::to_vec(&json!({ "revision": revision, "hits": hits })).expect("json"),
    })
}

/// The `{ revision, hits }` payloads the server sent on `RESULTS_CHANNEL`, with the client each
/// one was addressed to.
pub fn result_messages(submissions: &[Submission]) -> Vec<(Option<u64>, Value)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) if spec.channel == RESULTS_CHANNEL => Some((
                spec.to.client.map(|client| client.0),
                serde_json::from_slice(&spec.payload).expect("results payload is JSON"),
            )),
            _ => None,
        })
        .collect()
}

/// One hit as the wire carries it: `id`, `kind` (agent, terminal, file, meaning), `title`, an
/// optional `score` and the context `before`, `matched`, `after` with a zero-based `line`.
pub fn hit(id: &str, kind: &str, title: &str) -> Value {
    json!({ "id": id, "kind": kind, "title": title })
}

/// The client, opened and showing `hits` for `query`: it types the query and the server answers.
pub fn client_showing(query: &str, hits: Value) -> FakeHost {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![text_changed(query)]);
    host.settle();
    let revision = query_messages(&host.submitted()).last().map_or(1, |message| message["revision"].as_u64().unwrap_or(1));
    host.deliver(vec![results_event(revision, hits)]);
    host.settle();
    host
}

/// A server with a terminal pane `name` whose screen shows `screen_text` (`terminal.read-screen`).
pub fn add_terminal(host: &mut FakeHost, name: &str, screen_text: &str) -> u64 {
    use ilium_plugin_sdk::testing::TreeNode;
    let id = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree().set_screen(id, screen_text.lines().map(str::to_string).collect());
    id
}

/// Asks the server for `query` as client 1 and returns the hits of the answer.
pub fn ask(host: &mut FakeHost, revision: u64, query: &str) -> Vec<Value> {
    let before = host.submitted().len();
    host.deliver(vec![query_event(1, revision, query)]);
    host.settle();
    let submitted = host.submitted();
    result_messages(&submitted[before..])
        .into_iter()
        .filter(|(_, payload)| payload["revision"] == revision)
        .flat_map(|(_, payload)| payload["hits"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// Runs every outstanding job of the host through the plugin's `job-runner` export, the way the
/// host does in a second instance, and completes it with the returned bytes (or the error).
pub fn drive_jobs(host: &mut FakeHost) {
    use ilium_plugin_sdk::JobRunner;
    for record in host.jobs().list() {
        let mut runner = SearchServer::default();
        let result = runner.run(record.kind.clone(), record.input.clone());
        host.jobs().finish(record.request, result);
    }
    host.settle();
}
