//! Shared helpers of the Worktrees feature tests (manual page core-plugin-worktrees.md).
//!
//! Every use of a testkit or plugin item that the skeleton lacks lives in the "gap helpers" section
//! at the end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_worktrees::{WorktreesClient, WorktreesServer};
use ilium_plugin_sdk::host::process::{ProcessExit, ProcessSpec};
use ilium_plugin_sdk::host::workspace::WorkspaceSpec;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, CreatePaneSpec, Event, Intent, MessageEvent, NodeId,
    PaneInfo, PluginId, PluginSide, PrimitiveKind, Submission, TopicPublishedEvent,
};
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.worktrees";
pub const COMMAND: &str = "ilium.core.worktrees/new-agent";
pub const TOPIC_CHANGED: &str = "ilium.core.worktrees/changed";
pub const BEFORE_CREATE: &str = "ilium.core.worktrees/before-create";
pub const BEFORE_REMOVE: &str = "ilium.core.worktrees/before-remove";
pub const POST_CREATE_STEPS: &str = "ilium.core.worktrees/post-create-steps";

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

/// The text of a manual page by file stem, for example `core-plugin-worktrees`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-worktrees")
}

/// A user document of the repository by file name, for example `worktrees.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

pub fn golden_path(name: &str) -> String {
    format!("{}/tests/features/golden/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Table rows (cells trimmed) of the first table after the line starting with `anchor`.
pub fn table_after(text: &str, anchor: &str) -> Vec<Vec<String>> {
    let mut lines = text.lines().skip_while(|line| !line.starts_with(anchor));
    lines.next().expect("anchor present");
    lines
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect())
        .collect()
}

/// The bodies of the ```json fences of a text, in order.
pub fn json_fences(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```json\n") {
        let body = &rest[start + "```json\n".len()..];
        let end = body.find("```").expect("fence end");
        found.push(body[..end].to_string());
        rest = &body[end + 3..];
    }
    found
}

/// The `<pre class="term">` figure with this index on the page, tags removed, rows without trailing spaces.
pub fn page_figure(index: usize) -> Vec<String> {
    let page = page_text();
    let mut rest = page.as_str();
    let mut found = None;
    for _ in 0..=index {
        let start = rest.find("<pre class=\"term\">").expect("figure present") + "<pre class=\"term\">".len();
        let end = rest[start..].find("</pre>").expect("figure end");
        found = Some(&rest[start..start + end]);
        rest = &rest[start + end..];
    }
    let raw = found.expect("figure found");
    let mut text = String::new();
    let mut in_tag = false;
    for character in raw.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => text.push(other),
            _ => {}
        }
    }
    text.lines().map(|line| line.trim_end().to_string()).filter(|line| !line.is_empty()).collect()
}

// ---- events -------------------------------------------------------------------------------

pub fn ok_exit() -> ProcessExit {
    ProcessExit { status: Some(0), signal: None, stdout: Vec::new(), stderr: Vec::new() }
}

pub fn failed_exit(status: i32) -> ProcessExit {
    ProcessExit { status: Some(status), signal: None, stdout: Vec::new(), stderr: b"fatal: refused".to_vec() }
}

/// The message the client side of this plugin sends to the server side on channel `new-agent`.
pub fn new_agent_message(payload: &Value) -> Event {
    channel_message("new-agent", payload.to_string().into_bytes())
}

pub fn channel_message(channel: &str, payload: Vec<u8>) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: channel.to_string(),
        payload,
    })
}

/// The `pane-created` event of the agent pane that the plugin asked for (the title is the branch).
pub fn pane_created(node: u64, title: &str) -> Event {
    Event::PaneCreated(PaneInfo {
        node: NodeId(node),
        content_kind: "ilium.terminal".to_string(),
        primitive: PrimitiveKind::Terminal,
        columns: 80,
        rows: 24,
        focused: false,
        title: title.to_string(),
        is_restored: false,
    })
}

/// A `command-invoked` event as the palette or the prefix key delivers it.
pub fn command_invoked(command: &str, arguments_json: &str, origin: CommandOrigin) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: arguments_json.to_string(),
        origin,
        context: CommandContext::default(),
    })
}

/// A `topic-published` event of the `changed` topic.
pub fn changed_topic(path: &str, state: &str) -> Event {
    Event::TopicPublished(TopicPublishedEvent {
        topic: TOPIC_CHANGED.to_string(),
        from: PluginId(PLUGIN_ID.to_string()),
        payload_json: json!({"path": path, "state": state}).to_string(),
    })
}

// ---- submissions --------------------------------------------------------------------------

pub fn create_panes(submissions: &[Submission]) -> Vec<CreatePaneSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The `cwd` of a create-pane spec as text, whatever the skeleton type of the field is.
pub fn cwd_text(spec: &CreatePaneSpec) -> String {
    serde_json::to_value(&spec.cwd).expect("cwd serialises").to_string()
}

/// Messages addressed to this plugin's server side, as (channel, payload text).
pub fn server_messages(submissions: &[Submission]) -> Vec<(String, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) if spec.to.plugin == PluginId(PLUGIN_ID.to_string()) && spec.to.side == PluginSide::Server => {
                Some((spec.channel.clone(), String::from_utf8(spec.payload.clone()).expect("UTF-8 payload")))
            }
            _ => None,
        })
        .collect()
}

/// The calls of the fake process service that started `git worktree add`.
pub fn worktree_add_calls(calls: &[ProcessSpec]) -> Vec<ProcessSpec> {
    calls
        .iter()
        .filter(|call| call.program == "git" && call.arguments.first().map(String::as_str) == Some("worktree") && call.arguments.get(1).map(String::as_str) == Some("add"))
        .cloned()
        .collect()
}

pub fn registered(host: &mut FakeHost) -> Vec<WorkspaceSpec> {
    host.workspace().registered()
}

// ---- gap helpers (see ../API_GAPS.md) -----------------------------------------------------

/// A server-side host with the manifest grants, not yet activated.
pub fn server_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build()
}

/// A client-side host with the manifest grants, not yet activated.
pub fn client_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build()
}

/// A server-side host with the plugin activated; every git run succeeds with empty output.
pub fn server_host() -> FakeHost {
    let mut host = server_host_unactivated();
    host.process().default_exit(ok_exit());
    host.activate::<WorktreesServer>();
    host
}

/// A client-side host with the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = client_host_unactivated();
    host.activate::<WorktreesClient>();
    host
}

/// Sends a new-agent request for `branch` and lets every async step finish; returns everything submitted.
pub fn create_worktree(host: &mut FakeHost, payload: &Value) -> Vec<Submission> {
    host.deliver_one(new_agent_message(payload));
    host.settle();
    host.submitted()
}

/// Sets a key of the `[git]` kernel settings tab (the values stay in the same config.toml table).
pub fn set_git(host: &mut FakeHost, key: &str, value: Value) -> Result<(), ilium_plugin_sdk::types::PluginError> {
    host.kernel_settings("git").set(key, value)
}

/// A server-side host whose project root is `root`, not yet activated.
pub fn server_host_unactivated_at(root: &str) -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").project_root(PathBuf::from(root)).build()
}
