//! Shared helpers: hosts for both sides, event builders and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_agent_toolbar::{AgentToolbarClient, AgentToolbarServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentModel, AgentModelEvent, AgentView, CommandContext,
    CommandInvokedEvent, CommandOrigin, EpochMs, Event, Intent, MessageEvent, NodeClass, NodeId,
    PluginId, PluginSide, RichSpan, RichText, SpanTarget, Style, Submission, UserLineEvent,
};

pub const PLUGIN_ID: &str = "ilium.core.agent-toolbar";
/// The banner caps the trimmed prompt at this many characters (page sample `BANNER_CHARS`).
pub const BANNER_CHARS: usize = 120;
/// 2030-01-01 00:00:00 UTC, the default fake clock (DEV-414), in milliseconds.
pub const CLOCK_START_MS: u64 = 1_893_456_000_000;

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/agent-toolbar).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-agent-toolbar.md");
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
    host.activate::<AgentToolbarServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<AgentToolbarClient>();
    host
}

/// Adds an agent pane with the given signature and activity to the fake tree.
pub fn add_agent_pane(
    host: &mut FakeHost,
    name: &str,
    signature: &str,
    activity: AgentActivity,
) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity,
            objective: None,
            goal: None,
        }),
    })
}

/// Rich text of one plain span, as the banner and recover slots carry it.
pub fn plain(text: &str) -> RichText {
    vec![RichSpan {
        text: text.to_string(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }]
}

/// Concatenated text of a rich text.
pub fn rich_to_string(content: &RichText) -> String {
    content.iter().map(|span| span.text.as_str()).collect()
}

/// A `user-line-submitted` event.
pub fn user_line(pane: u64, text: &str) -> Event {
    Event::UserLineSubmitted(UserLineEvent {
        pane: NodeId(pane),
        text: text.to_string(),
        at: EpochMs(CLOCK_START_MS),
    })
}

/// An `agent-activity-changed` event.
pub fn activity(node: u64, previous: AgentActivity, current: AgentActivity) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous,
        current,
        revision: 1,
    })
}

/// An `agent-model-changed` event.
pub fn model_changed(pane: u64, model: &str, context_window: Option<u32>) -> Event {
    Event::AgentModelChanged(AgentModelEvent {
        pane: NodeId(pane),
        model: AgentModel {
            model: model.to_string(),
            provider: Some("anthropic".to_string()),
            context_window,
            source: "test".to_string(),
        },
    })
}

/// A `command-invoked` event with a pane context.
pub fn command_on(command: &str, origin: CommandOrigin, pane: Option<u64>) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin,
        context: CommandContext {
            pane: pane.map(NodeId),
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: None,
        },
    })
}

/// A `message` event as another side of this plugin would send it.
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

/// `(pane, slot, text)` of every `set-frame-slot` intent, in order.
pub fn set_slots(submissions: &[Submission]) -> Vec<(u64, String, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetFrameSlot(spec) => Some((
                spec.pane.0,
                spec.slot.clone(),
                rich_to_string(&spec.content),
            )),
            _ => None,
        })
        .collect()
}

/// `(pane, slot)` of every `clear-frame-slot` intent, in order.
pub fn cleared_slots(submissions: &[Submission]) -> Vec<(u64, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::ClearFrameSlot(spec) => Some((spec.pane.0, spec.slot.clone())),
            _ => None,
        })
        .collect()
}

/// `(pane, action_id)` of every `agent-action` intent, in order.
pub fn agent_actions(submissions: &[Submission]) -> Vec<(u64, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::AgentAction(spec) => Some((spec.pane.0, spec.action_id.clone())),
            _ => None,
        })
        .collect()
}

/// `(target plugin, target side, channel, payload)` of every `message` intent, in order.
pub fn messages(submissions: &[Submission]) -> Vec<(String, PluginSide, String, Vec<u8>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) => Some((
                spec.to.plugin.0.clone(),
                spec.to.side,
                spec.channel.clone(),
                spec.payload.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// The banner text the plugin must submit for a prompt: `> ` plus the trimmed prompt capped at 120 characters.
pub fn expected_banner(prompt: &str) -> String {
    let trimmed: String = prompt.trim().chars().take(BANNER_CHARS).collect();
    format!("> {trimmed}")
}
