//! Helpers of the Worktrees test suites that both targets mount by path (manual page
//! core-plugin-worktrees.md). Helpers only the feature suite uses live in ../features/common.rs.
//!
//! Every use of a testkit or plugin item that the skeleton lacks lives in a gap section of the
//! suite that needs it (see ../API_GAPS.md), so a missing item breaks that file only.
use std::path::PathBuf;

use ilium_plugin_core_worktrees::WorktreesClient;
use ilium_plugin_sdk::host::process::{ProcessExit, ProcessSpec};
use ilium_plugin_sdk::host::workspace::WorkspaceSpec;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, CreatePaneSpec, Event, Intent,
    MessageEvent, NodeId, PaneInfo, PluginId, PluginSide, PrimitiveKind, Submission,
    TopicPublishedEvent,
};
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.worktrees";
pub const COMMAND: &str = "ilium.core.worktrees/new-agent";
pub const TOPIC_CHANGED: &str = "ilium.core.worktrees/changed";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

// ---- events -------------------------------------------------------------------------------

pub fn ok_exit() -> ProcessExit {
    ProcessExit {
        status: Some(0),
        signal: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

pub fn failed_exit(status: i32) -> ProcessExit {
    ProcessExit {
        status: Some(status),
        signal: None,
        stdout: Vec::new(),
        stderr: b"fatal: refused".to_vec(),
    }
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
    serde_json::to_value(&spec.cwd)
        .expect("cwd serialises")
        .to_string()
}

/// Messages addressed to this plugin's server side, as (channel, payload text).
pub fn server_messages(submissions: &[Submission]) -> Vec<(String, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec)
                if spec.to.plugin == PluginId(PLUGIN_ID.to_string())
                    && spec.to.side == PluginSide::Server =>
            {
                Some((
                    spec.channel.clone(),
                    String::from_utf8(spec.payload.clone()).expect("UTF-8 payload"),
                ))
            }
            _ => None,
        })
        .collect()
}

/// The calls of the fake process service that started `git worktree add`.
pub fn worktree_add_calls(calls: &[ProcessSpec]) -> Vec<ProcessSpec> {
    calls
        .iter()
        .filter(|call| {
            call.program == "git"
                && call.arguments.first().map(String::as_str) == Some("worktree")
                && call.arguments.get(1).map(String::as_str) == Some("add")
        })
        .cloned()
        .collect()
}

pub fn registered(host: &mut FakeHost) -> Vec<WorkspaceSpec> {
    host.workspace().registered()
}

// ---- hosts and requests -------------------------------------------------------------------

/// A client-side host with the manifest grants, not yet activated.
pub fn client_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build()
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
pub fn set_git(
    host: &mut FakeHost,
    key: &str,
    value: Value,
) -> Result<(), ilium_plugin_sdk::types::PluginError> {
    host.kernel_settings().set("git", key, value)
}
