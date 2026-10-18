//! Shared helpers: hosts for both sides, pane builders, event builders and submission readers.
//!
//! Every call into a fake that the skeleton testkit lacks (terminal, clipboard, inference, menu
//! provider, topics, contributors) is made from here or from the test bodies through the names
//! listed in API_GAPS.md, so the implementation phase adds them in one pass.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_smart_copy::{SmartCopyClient, SmartCopyServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent, MessageEvent, NodeClass,
    NodeId, PluginId, PluginSide, PointerButton, PointerEvent, PointerKind, PointerModifiers,
    Submission, ViewTarget,
};

/// The plugin id of the page.
pub const PLUGIN_ID: &str = "ilium.core.smart-copy";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// A server-side host with the manifest grants, not yet activated.
pub fn server_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build()
}

/// A client-side host with the manifest grants, not yet activated.
pub fn client_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build()
}

/// A server-side host with the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = server_host_unactivated();
    host.activate::<SmartCopyServer>();
    host
}

/// A client-side host with the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = client_host_unactivated();
    host.activate::<SmartCopyClient>();
    host
}

/// Adds a terminal pane to the fake tree of the host and returns its id.
pub fn add_terminal_pane(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// Adds an editor pane to the fake tree of the host and returns its id.
pub fn add_editor_pane(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.editor".to_string()),
        agent: None,
    })
}

/// A pointer event with no modifiers and the left button.
pub fn pointer(node: u64, kind: PointerKind, row: u32, column: u16) -> Event {
    Event::Pointer(PointerEvent {
        node: NodeId(node),
        row,
        column,
        kind,
        button: PointerButton::Left,
        modifiers: PointerModifiers(0),
        sub_x: 0,
        sub_y: 0,
    })
}

/// A command invocation as the right-click menu delivers it.
pub fn menu_command(command: &str, pane: u64, menu_item: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Menu,
        context: CommandContext {
            pane: Some(NodeId(pane)),
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: Some(menu_item.to_string()),
        },
    })
}

/// A command invocation from the palette with an optional pane.
pub fn palette_command(command: &str, pane: Option<u64>) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext {
            pane: pane.map(NodeId),
            ..CommandContext::default()
        },
    })
}

/// The `candidates` message the server side sends to the client side of this plugin.
pub fn candidates_message(lines: &[&str]) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: "candidates".to_string(),
        payload: lines.join("\n").into_bytes(),
    })
}

/// The picker popup target of the page.
pub fn picker_target() -> ViewTarget {
    ViewTarget::Popup("smart-copy".to_string())
}

/// The intents of a submission list, in order.
pub fn intents(submissions: &[Submission]) -> Vec<Intent> {
    submissions
        .iter()
        .map(|submission| submission.intent.clone())
        .collect()
}

/// Message intents addressed to the client side of this plugin on channel `candidates`: the payloads as text.
pub fn candidate_payloads(submissions: &[Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Message(spec)
                if spec.channel == "candidates"
                    && spec.to.plugin == PluginId(PLUGIN_ID.to_string())
                    && spec.to.side == PluginSide::Client =>
            {
                Some(String::from_utf8(spec.payload.clone()).expect("candidate payload is UTF-8"))
            }
            _ => None,
        })
        .collect()
}

/// Candidate lines of the first `candidates` message in the list; empty when none was sent.
pub fn candidate_lines(submissions: &[Submission]) -> Vec<String> {
    candidate_payloads(submissions)
        .first()
        .map(|payload| payload.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// True when any status segment intent carries text containing `needle`.
pub fn status_contains(submissions: &[Submission], needle: &str) -> bool {
    submissions
        .iter()
        .any(|submission| match &submission.intent {
            Intent::SetStatusSegment(spec) => serde_json::to_string(&spec.content)
                .expect("rich text serialises")
                .contains(needle),
            _ => false,
        })
}

// ---- helpers of the ports part ----

use ilium_plugin_sdk::types::{GrabId, KeyEvent, NodeId as PortNodeId, ScreenChangedEvent};

/// A key event by name.
pub fn key(name: &str) -> Event {
    Event::KeyPressed(KeyEvent {
        grab: GrabId(0),
        pane: None,
        chord: name.to_string(),
        repeat: false,
    })
}

/// A server host showing these rows on a fresh terminal pane; returns the host and the pane id.
pub fn server_showing(rows: &[&str]) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, rows);
    (host, pane)
}

/// The candidate lines the server side sends for a screen with these rows.
pub fn scan(rows: &[&str]) -> Vec<String> {
    let (mut host, pane) = server_showing(rows);
    let last = rows.len().saturating_sub(1) as u32;
    let event = Event::ScreenChanged(ScreenChangedEvent {
        pane: PortNodeId(pane),
        first_row: 0,
        last_row: last,
        revision: 1,
    });
    candidate_lines(&host.deliver_one(event))
}

/// True when the scan of one row offers the token as a candidate line.
pub fn offers(row: &str, token: &str) -> bool {
    scan(&[row]).iter().any(|line| line == token)
}

/// A client host with the picker open on these candidates.
pub fn open_picker(lines: &[&str]) -> FakeHost {
    let mut host = client_host();
    host.deliver_one(candidates_message(lines));
    host
}
