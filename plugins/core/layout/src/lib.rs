//! Core plugin `ilium.core.layout` (manual page core-plugin-layout.md).
//!
//! The client side forwards the four split commands and divider drags to the server side and draws
//! the layout preview panel. The server side turns those messages into `create-pane`,
//! `create-split` and `resize-pane` intents.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Activation, Alignment, Area, BlockSpec, BlockTitle, BorderType,
    Borders, CreatePaneSpec, CreateSplitSpec, ErrorCode, Event, Intent, MessageSpec,
    MessageTarget, NodeFlags, Outcome, Padding, ParagraphSpec, PluginError, PluginId, PluginSide,
    PointerKind, RequestRef, ResizePaneSpec, SplitDirection, Style, Span, Submission,
    TitlePosition, UiNode, UiTree, ViewTarget, Widget, NodeId,
};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The plugin id, also the target of the client-to-server messages.
const PLUGIN_ID: &str = "ilium.core.layout";
/// The four split channels, which are also the names of the client commands.
const SPLIT_CHANNELS: [&str; 4] = ["split-right", "split-left", "split-down", "split-up"];
/// Channel of the divider drag message (focused id, then columns).
const RESIZE_CHANNEL: &str = "resize";
/// Channel of the "New split view" dialog answer that skips the member picker (payload: the group id).
const SPLIT_EMPTY_VERTICAL_CHANNEL: &str = "split-empty-vertical";
/// Content kind of the pane a split creates.
const NEW_PANE_KIND: &str = "ilium.terminal";
/// Key of the preview widget that receives the divider drag.
const PREVIEW_KEY: &str = "preview";
/// How many terminal columns one preview cell stands for.
const COLUMNS_PER_CELL: u16 = 1;

fn message_to_server(channel: &str, payload: Vec<u8>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// Reads the little-endian node id at the start of a payload.
fn read_node(payload: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = payload.get(..8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Activation succeeds when the settings JSON is an object (an empty string means no settings).
fn validate_settings(settings_json: &str) -> Result<(), PluginError> {
    if settings_json.trim().is_empty() {
        return Ok(());
    }
    match serde_json::from_str::<serde_json::Value>(settings_json) {
        Ok(value) if value.is_object() => Ok(()),
        _ => Err(PluginError::new(
            ErrorCode::InvalidArgument,
            "settings must be one JSON object",
        )),
    }
}

// ---- server side ----

/// A split waiting for the id of its new pane.
#[derive(Debug, Clone, Copy)]
struct PendingSplit {
    focused: u64,
    new_pane_first: bool,
    direction: SplitDirection,
}

#[derive(Default)]
struct ServerState {
    /// Pending `create-pane` requests by submission correlation.
    pending: HashMap<u64, PendingSplit>,
}

thread_local! {
    static SERVER: RefCell<ServerState> = RefCell::new(ServerState::default());
    static CLIENT: RefCell<ClientState> = RefCell::new(ClientState::default());
}

/// How a split channel places the new pane: `(new pane first, direction)`.
fn split_shape(channel: &str) -> Option<(bool, SplitDirection)> {
    match channel {
        "split-right" => Some((false, SplitDirection::Vertical)),
        "split-left" => Some((true, SplitDirection::Vertical)),
        "split-down" => Some((false, SplitDirection::Horizontal)),
        "split-up" => Some((true, SplitDirection::Horizontal)),
        _ => None,
    }
}

/// The server side of `ilium.core.layout` (core-plugin-layout.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct LayoutServer;

impl Lifecycle for LayoutServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)?;
        SERVER.with(|cell| *cell.borrow_mut() = ServerState::default());
        Ok(())
    }
}

impl Handler for LayoutServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::Message(message) => {
                    if let Some(submission) = server_message(&message.channel, &message.payload) {
                        submissions.push(submission);
                    }
                }
                Event::RequestCompleted(result) => {
                    if let Some(submission) = split_after_pane(result.request, result.outcome) {
                        submissions.push(submission);
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}

fn server_message(channel: &str, payload: &[u8]) -> Option<Submission> {
    if let Some((new_pane_first, direction)) = split_shape(channel) {
        let focused = read_node(payload)?;
        let submission = Submission::new(Intent::CreatePane(CreatePaneSpec {
            parent: None,
            title: "shell".to_string(),
            content_kind: NEW_PANE_KIND.to_string(),
            cwd: None,
            command: None,
            initial_input: None,
        }));
        SERVER.with(|cell| {
            cell.borrow_mut().pending.insert(
                submission.correlation,
                PendingSplit {
                    focused,
                    new_pane_first,
                    direction,
                },
            );
        });
        return Some(submission);
    }
    match channel {
        RESIZE_CHANNEL => {
            let pane = read_node(payload)?;
            let columns_bytes: [u8; 2] = payload.get(8..10)?.try_into().ok()?;
            Some(Submission::new(Intent::ResizePane(ResizePaneSpec {
                pane: NodeId(pane),
                columns: Some(u16::from_le_bytes(columns_bytes)),
                rows: None,
            })))
        }
        SPLIT_EMPTY_VERTICAL_CHANNEL => {
            // The payload is the group; the split view starts with no members.
            read_node(payload)?;
            Some(Submission::new(Intent::CreateSplit(CreateSplitSpec {
                panes: Vec::new(),
                direction: SplitDirection::Vertical,
            })))
        }
        _ => None,
    }
}

/// Joins the focused pane with the pane a split created, once the host reported its id.
fn split_after_pane(request: RequestRef, outcome: Outcome) -> Option<Submission> {
    let RequestRef::Submission(correlation) = request else {
        return None;
    };
    let pending = SERVER.with(|cell| cell.borrow_mut().pending.remove(&correlation))?;
    let Outcome::OkNode(created) = outcome else {
        return None;
    };
    let focused = NodeId(pending.focused);
    let panes = if pending.new_pane_first {
        vec![created, focused]
    } else {
        vec![focused, created]
    };
    Some(Submission::new(Intent::CreateSplit(CreateSplitSpec {
        panes,
        direction: pending.direction,
    })))
}

// ---- client side ----

#[derive(Default)]
struct ClientState {
    /// Panes in the order they were first seen.
    order: Vec<u64>,
    sizes: HashMap<u64, (u16, u16)>,
    titles: HashMap<u64, String>,
    focused: Option<u64>,
    /// False while the preview is hidden; defaults to visible.
    hidden: bool,
}

impl ClientState {
    fn see(&mut self, node: u64) {
        if !self.order.contains(&node) {
            self.order.push(node);
        }
    }
}

/// The client side of `ilium.core.layout` (core-plugin-layout.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct LayoutClient;

impl Lifecycle for LayoutClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        validate_settings(&context.settings_json)?;
        CLIENT.with(|cell| *cell.borrow_mut() = ClientState::default());
        Ok(())
    }
}

impl Handler for LayoutClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if SPLIT_CHANNELS.contains(&invoked.command.as_str()) => {
                    if let Some(pane) = invoked.context.pane {
                        submissions
                            .push(message_to_server(&invoked.command, pane.0.to_le_bytes().to_vec()));
                    }
                }
                Event::PaneResized(resized) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.see(resized.node.0);
                    state.sizes.insert(resized.node.0, (resized.columns, resized.rows));
                }),
                Event::PaneCreated(info) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.see(info.node.0);
                    state.sizes.insert(info.node.0, (info.columns, info.rows));
                    state.titles.insert(info.node.0, info.title);
                }),
                Event::PaneTitleChanged(changed) => CLIENT.with(|cell| {
                    cell.borrow_mut().titles.insert(changed.node.0, changed.title);
                }),
                Event::PaneFocused(focused) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    if let Some(node) = focused {
                        state.see(node.0);
                        state.focused = Some(node.0);
                    }
                }),
                Event::NodeRemoved(node) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.order.retain(|seen| *seen != node.0);
                    state.sizes.remove(&node.0);
                    state.titles.remove(&node.0);
                    if state.focused == Some(node.0) {
                        state.focused = None;
                    }
                }),
                Event::ViewVisibility(visibility) if is_preview(&visibility.view) => {
                    CLIENT.with(|cell| cell.borrow_mut().hidden = !visibility.visible);
                }
                Event::UiAction(action) if action.key == PREVIEW_KEY => {
                    if let Some(submission) = divider_drag(action.kind, action.value) {
                        submissions.push(submission);
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}

fn is_preview(view: &ViewTarget) -> bool {
    matches!(view, ViewTarget::Panel(name) if name == "layout.presets")
}

/// Converts a divider drag into a resize message for the focused pane.
fn divider_drag(kind: ActionKind, value: Option<ActionValue>) -> Option<Submission> {
    if kind != ActionKind::Pointer {
        return None;
    }
    let Some(ActionValue::Pointer(pointer)) = value else {
        return None;
    };
    if pointer.kind != PointerKind::Drag {
        return None;
    }
    let (focused, hidden) = CLIENT.with(|cell| {
        let state = cell.borrow();
        (state.focused, state.hidden)
    });
    if hidden {
        return None;
    }
    let focused = focused?;
    let columns = pointer.column.saturating_mul(COLUMNS_PER_CELL);
    let mut payload = focused.to_le_bytes().to_vec();
    payload.extend_from_slice(&columns.to_le_bytes());
    Some(message_to_server(RESIZE_CHANNEL, payload))
}

impl Views for LayoutClient {
    fn render(&mut self, _view: ViewTarget, area: Area) -> UiTree {
        CLIENT.with(|cell| build_preview(&cell.borrow(), usize::from(area.columns)))
    }
}

/// Pads or cuts `text` to exactly `width` characters.
fn fit(text: &str, width: usize) -> String {
    let mut cut: String = text.chars().take(width).collect();
    let missing = width.saturating_sub(cut.chars().count());
    cut.extend(std::iter::repeat_n(' ', missing));
    cut
}

fn title_of(state: &ClientState, index: usize) -> (String, bool) {
    match state.order.get(index) {
        Some(node) => (
            state.titles.get(node).cloned().unwrap_or_default(),
            state.focused == Some(*node),
        ),
        None => (String::new(), false),
    }
}

/// The inner lines of the preview: two rows of two panes, a rule between them and the size row.
fn preview_lines(state: &ClientState) -> Vec<String> {
    let (top_left, top_left_focused) = title_of(state, 0);
    let (top_right, _) = title_of(state, 1);
    let (bottom_left, _) = title_of(state, 2);
    let (bottom_right, _) = title_of(state, 3);
    let (first, second) = if top_left_focused {
        (
            format!("\u{258c}{}\u{2590} ", fit(&top_left, 5)),
            format!("\u{258c}{}\u{2590} ", fit("", 5)),
        )
    } else {
        (format!("{} ", fit(&top_left, 7)), fit("", 8))
    };
    let rule = format!("{} \u{254b} {}", "\u{2500}".repeat(8), "\u{2500}".repeat(10));
    let sizes: Vec<String> = state
        .order
        .iter()
        .take(4)
        .map(|node| {
            let (columns, rows) = state.sizes.get(node).copied().unwrap_or((0, 0));
            format!("{columns}x{rows}")
        })
        .collect();
    vec![
        format!("{first}\u{2503} {top_right}"),
        format!("{second}\u{2503}"),
        rule,
        format!("{}\u{2503} {bottom_right}", fit(&bottom_left, 9)),
        sizes.join(" | "),
    ]
}

fn build_preview(state: &ClientState, columns: usize) -> UiTree {
    let inner_width = columns.saturating_sub(4);
    let lines: Vec<Vec<Span>> = preview_lines(state)
        .iter()
        .map(|line| {
            vec![Span {
                text: fit(line, inner_width),
                style: Style::default(),
            }]
        })
        .collect();
    let paragraph = ParagraphSpec {
        lines,
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    };
    let block = BlockSpec {
        borders: Borders::ALL,
        border_type: BorderType::Rounded,
        titles: vec![BlockTitle {
            text: vec![Span {
                text: "Layout".to_string(),
                style: Style::default(),
            }],
            position: TitlePosition::Top,
            alignment: Alignment::Left,
        }],
        padding: Padding {
            left: 1,
            right: 1,
            top: 0,
            bottom: 0,
        },
        style: Style::default(),
        border_style: Style::default(),
    };
    let node = |key: Option<&str>, widget: Widget, children: Vec<u32>| UiNode {
        key: key.map(str::to_string),
        widget,
        children,
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    };
    UiTree {
        version: 1,
        nodes: vec![
            node(None, Widget::Block(block), vec![1]),
            node(Some(PREVIEW_KEY), Widget::Paragraph(paragraph), Vec::new()),
        ],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

    /// The shipped manifest; core-plugin-layout.md "Complete manifest" is the contract it must meet.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// Required top-level keys of the `ilium:plugin@0.1.0` manifest, per core-plugin-layout.md.
    const REQUIRED_TOP_LEVEL_KEYS: [&str; 13] = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "server",
        "client",
        "capabilities",
        "contributes",
        "activation",
        "limits_tier",
    ];

    fn parsed_manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json parses as JSON")
    }

    fn missing_required_keys(manifest: &Value) -> Vec<&'static str> {
        REQUIRED_TOP_LEVEL_KEYS
            .iter()
            .copied()
            .filter(|key| manifest.get(*key).is_none())
            .collect()
    }

    fn string_array(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|item| item.as_str().expect("expected a string").to_owned())
            .collect()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> &'a Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("contributes.extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .unwrap_or_else(|| panic!("extension point {point_id} is declared"))
    }

    /// REQ: COR-050
    #[test]
    fn manifest_has_every_required_top_level_key() {
        let manifest = parsed_manifest();
        let missing = missing_required_keys(&manifest);
        assert!(missing.is_empty(), "manifest.json lacks keys: {missing:?}");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_layout_as_first_party_plugin() {
        let manifest = parsed_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.layout");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_server_and_client_sides_with_worlds() {
        let manifest = parsed_manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_capabilities_per_side_match_the_page() {
        let manifest = parsed_manifest();
        assert_eq!(
            string_array(&manifest["capabilities"]["server"]),
            [
                "panes.read",
                "panes.lifecycle",
                "panes.create",
                "tree.restructure",
                "extensions.provide:ilium.core.layout/layout-presets",
                "extensions.provide:ilium.core.layout/before-split",
            ]
        );
        assert_eq!(
            string_array(&manifest["capabilities"]["client"]),
            ["panes.read", "ui.panels"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_contributes_both_extension_points_with_page_limits() {
        let manifest = parsed_manifest();

        let presets = extension_point(&manifest, "layout-presets");
        assert_eq!(presets["kind"], "collect");
        assert_eq!(presets["max_entries"], 64);
        assert_eq!(presets["dynamic"], Value::Bool(false));
        assert_eq!(presets["risk"], "standard");

        let before_split = extension_point(&manifest, "before-split");
        assert_eq!(before_split["kind"], "hook");
        assert_eq!(before_split["mode"], "veto");
        assert_eq!(before_split["deadline_ms"], 20);
        assert_eq!(before_split["max_handlers"], 8);
        assert_eq!(before_split["risk"], "standard");
        assert_eq!(
            string_array(&before_split["payload_schema"]["required"]),
            ["node", "direction", "pane_count"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activation_names_the_command_and_view_triggers() {
        let manifest = parsed_manifest();
        assert_eq!(
            string_array(&manifest["activation"]),
            ["command:layout.preset", "view:layout.presets"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn empty_manifest_reports_every_required_key_missing() {
        let missing = missing_required_keys(&json!({}));
        assert_eq!(missing, REQUIRED_TOP_LEVEL_KEYS);
    }

    /// REQ: COR-050
    #[test]
    fn truncated_manifest_text_is_refused() {
        assert!(serde_json::from_str::<Value>("{\"api\":").is_err());
    }
}
