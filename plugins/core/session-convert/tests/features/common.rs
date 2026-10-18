//! Shared helpers: hosts for both sides, event builders and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_session_convert::{SessionConvertClient, SessionConvertServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    MessageEvent, NodeClass, NodeId, PluginId, PluginSide, Submission,
};

pub const PLUGIN_ID: &str = "ilium.core.session-convert";
/// The safety-net timer of a conversion: ten minutes, in milliseconds (page sample `WATCHDOG_MILLIS`).
pub const WATCHDOG_MILLIS: u64 = 10 * 60 * 1000;
/// 2030-01-01 00:00:00 UTC, the default fake clock (DEV-414), in milliseconds.
pub const CLOCK_START_MS: u64 = 1_893_456_000_000;
/// Session id of the source session used by the tests.
pub const SOURCE_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/session-convert).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Absolute path of a golden file of this suite.
pub fn golden(name: &str) -> String {
    format!(
        "{}/tests/features/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-session-convert.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The `<pre class="term">` figures of the page with tags removed and entities decoded.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let body = &rest[start + "<pre class=\"term\">".len()..];
        let end = body.find("</pre>").expect("figure end");
        let mut text = String::new();
        let mut inside_tag = false;
        for character in body[..end].chars() {
            match character {
                '<' => inside_tag = true,
                '>' => inside_tag = false,
                other if !inside_tag => text.push(other),
                _ => {}
            }
        }
        blocks.push(
            text.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        );
        rest = &body[end..];
    }
    blocks
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<SessionConvertServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<SessionConvertClient>();
    host
}

/// Adds an agent pane with the given signature to the fake tree.
pub fn add_agent_pane(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
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

/// A `command-invoked` event for the menu item of a pane, as the page describes it:
/// `arguments_json` is `{"item":"to-codex","node":42}`.
pub fn convert_click(item: &str, pane: u64) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: "convert".to_string(),
        arguments_json: format!(r#"{{"item":"{item}","node":{pane}}}"#),
        origin: CommandOrigin::Menu,
        context: CommandContext {
            pane: Some(NodeId(pane)),
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: Some(item.to_string()),
        },
    })
}

/// A `message` event as the other side of this plugin would send it.
pub fn message_from(side: PluginSide, channel: &str, payload: &[u8]) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: side,
        from_client: None,
        channel: channel.to_string(),
        payload: payload.to_vec(),
    })
}

/// The intents of a submission list, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions
        .iter()
        .map(|submission| submission.intent.clone())
        .collect()
}

/// Panes of every `freeze-pane` intent, in order.
pub fn frozen(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::FreezePane(spec) => Some(spec.pane.0),
            _ => None,
        })
        .collect()
}

/// Panes of every `unfreeze-pane` intent, in order.
pub fn unfrozen(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::UnfreezePane(spec) => Some(spec.pane.0),
            _ => None,
        })
        .collect()
}

/// Panes of every `close-node` intent, in order.
pub fn closed(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CloseNode(node) => Some(node.0),
            _ => None,
        })
        .collect()
}

/// `(title, content_kind, command)` of every `create-pane` intent, in order.
pub fn created(submissions: &[Submission]) -> Vec<(String, String, Vec<String>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) => Some((
                spec.title.clone(),
                spec.content_kind.clone(),
                spec.command.clone().unwrap_or_default(),
            )),
            _ => None,
        })
        .collect()
}

/// `(pane, command, keep_scrollback)` of every `replace-pane-command` intent, in order.
pub fn replaced(submissions: &[Submission]) -> Vec<(u64, Vec<String>, bool)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::ReplacePaneCommand(spec) => {
                Some((spec.pane.0, spec.command.clone(), spec.keep_scrollback))
            }
            _ => None,
        })
        .collect()
}

/// `(target plugin, target side, channel, payload text)` of every `message` intent, in order.
pub fn messages(submissions: &[Submission]) -> Vec<(String, PluginSide, String, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) => Some((
                spec.to.plugin.0.clone(),
                spec.to.side,
                spec.channel.clone(),
                String::from_utf8_lossy(&spec.payload).to_string(),
            )),
            _ => None,
        })
        .collect()
}
