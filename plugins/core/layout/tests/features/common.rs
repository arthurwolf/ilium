//! Shared helpers of the layout feature tests.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_layout::{LayoutClient, LayoutServer};
use ilium_plugin_sdk::testing::{manual, FakeHost};
use ilium_plugin_sdk::types::{
    ClientId, CommandContext, CommandInvokedEvent, CommandOrigin, CreatePaneSpec, CreateSplitSpec,
    Event, Intent, MessageEvent, MessageSpec, NodeClass, NodeId, PluginId, PluginSide,
    ResizePaneSpec, Submission,
};

pub const PLUGIN_ID: &str = "ilium.core.layout";
pub const PANEL: &str = "layout.presets";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> serde_json::Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn page_text() -> String {
    std::fs::read_to_string(manual::pages_dir().join("core-plugin-layout.md"))
        .expect("page is readable")
}

pub fn extension_points_page_text() -> String {
    std::fs::read_to_string(manual::pages_dir().join("reference-core-extension-points.md"))
        .expect("page is readable")
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else {
            break;
        };
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
        let Some(end) = after.find("</pre>") else {
            break;
        };
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
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<LayoutServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<LayoutClient>();
    host
}

pub fn command_event(name: &str, pane: Option<u64>) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: name.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext {
            pane: pane.map(NodeId),
            ..CommandContext::default()
        },
    })
}

pub fn message_from_client(channel: &str, payload: Vec<u8>) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Client,
        from_client: Some(ClientId(1)),
        channel: channel.to_string(),
        payload,
    })
}

pub fn id_bytes(node: u64) -> Vec<u8> {
    node.to_le_bytes().to_vec()
}

pub fn resize_payload(node: u64, columns: u16) -> Vec<u8> {
    let mut payload = node.to_le_bytes().to_vec();
    payload.extend_from_slice(&columns.to_le_bytes());
    payload
}

pub fn messages(submissions: &[Submission]) -> Vec<MessageSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn create_panes(submissions: &[Submission]) -> Vec<CreatePaneSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn create_splits(submissions: &[Submission]) -> Vec<CreateSplitSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreateSplit(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn resizes(submissions: &[Submission]) -> Vec<ResizePaneSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::ResizePane(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// A tree node for `host.tree().add`.
pub fn pane_node(
    name: &str,
    content_kind: &str,
    parent: Option<u64>,
) -> ilium_plugin_sdk::testing::TreeNode {
    ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some(content_kind.to_string()),
        agent: None,
    }
}

pub const SPLIT_CHANNELS: [&str; 4] = ["split-right", "split-left", "split-down", "split-up"];
