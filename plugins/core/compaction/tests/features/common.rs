//! Shared helpers: hosts for both sides, event builders and submission readers.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_compaction::{CompactionClient, CompactionServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, NodeClass, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent, MessageEvent, NodeId, PluginId, PluginSide,
    Submission,
};

pub const PLUGIN_ID: &str = "ilium.core.compaction";
/// The qualified command id the page samples compare against.
pub const COMPACT_COMMAND: &str = "ilium.core.compaction/compact";
/// Chunk size of the job runner (page sample 1: `input.chunks(32 * 1024)`).
pub const CHUNK_BYTES: usize = 32 * 1024;

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/compaction).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-compaction.md");
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
        blocks.push(text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"));
        rest = &body[end..];
    }
    blocks
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.activate::<CompactionServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<CompactionClient>();
    host
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
    submissions.iter().map(|submission| submission.intent.clone()).collect()
}

/// `(target plugin, target side, channel, payload)` of every `message` intent, in order.
pub fn messages(submissions: &[Submission]) -> Vec<(String, PluginSide, String, Vec<u8>)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) => {
                Some((spec.to.plugin.0.clone(), spec.to.side, spec.channel.clone(), spec.payload.clone()))
            }
            _ => None,
        })
        .collect()
}

/// A transcript of `bytes` bytes that is not a multiple of anything special.
pub fn transcript_of(bytes: usize) -> Vec<u8> {
    (0..bytes).map(|index| b'a' + (index % 26) as u8).collect()
}

/// Text of another manual page.
pub fn pages_text(file: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The cells of the first table row of `page` that starts with `prefix`; cell 0 is the empty text before the first bar.
pub fn table_row(page: &str, prefix: &str) -> Vec<String> {
    let line = page.lines().find(|line| line.starts_with(prefix)).unwrap_or_else(|| panic!("row {prefix} missing"));
    line.split('|').map(|cell| cell.trim().to_string()).collect()
}

/// The parsed manifest.
pub fn manifest_value() -> serde_json::Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// Adds an agent pane with the given signature and activity to the fake tree.
pub fn add_agent_pane(host: &mut FakeHost, name: &str, signature: &str, activity: AgentActivity) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView { signature: signature.to_string(), activity, objective: None, goal: None }),
    })
}

/// A server host with an idle Claude pane whose verified transcript holds `transcript`.
pub fn server_with_transcript(transcript: &[u8]) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude: refactor", "claude", AgentActivity::Idle);
    host.agents().set_transcript(pane, transcript.to_vec());
    (host, pane)
}
