//! Core plugin `ilium.core.layout` (manual page core-plugin-layout.md).
//!
//! The client side forwards the four split commands and divider drags to the server side, draws
//! the layout preview panel, and owns the "New split view" dialog (orientation step, then the
//! optional member picker). The server side turns the client's messages into `create-pane`,
//! `create-split` and `resize-pane` intents.

pub mod new_split;
pub mod padding;
pub mod padding_rows;
pub mod tree_layout_dialog;
pub mod tree_layout_rows;

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::generated::host::tree::HostTree;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Activation, Alignment, Area, BlockSpec, BlockTitle, BorderType,
    Borders, CreatePaneSpec, CreateSplitSpec, ErrorCode, Event, Intent, InvokeCommandSpec,
    MessageSpec, MessageTarget, NodeFlags, NodeId, OpenViewSpec, Outcome, Padding, ParagraphSpec,
    PluginError, PluginId, PluginSide, PointerKind, RequestRef, ResizePaneSpec, Span,
    SplitDirection, Style, Submission, TitlePosition, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, Views};

/// The plugin id, also the target of the client-to-server messages.
const PLUGIN_ID: &str = "ilium.core.layout";
/// The four split channels, which are also the names of the client commands.
const SPLIT_CHANNELS: [&str; 4] = ["split-right", "split-left", "split-down", "split-up"];
/// Channel of the divider drag message (focused id, then columns).
const RESIZE_CHANNEL: &str = "resize";
/// Channel of the "New split view" dialog answer and name of the command that opens the dialog.
/// The payload is JSON: `{"parent": id, "direction": "vertical"|"horizontal", "panes": [id]}`.
const NEW_SPLIT_CHANNEL: &str = "new-split";
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
        NEW_SPLIT_CHANNEL => {
            let answer = new_split::Answer::from_payload(payload)?;
            Some(Submission::new(Intent::CreateSplit(CreateSplitSpec {
                panes: answer.panes.into_iter().map(NodeId).collect(),
                direction: answer.direction,
                parent: Some(NodeId(answer.parent)),
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
        parent: None,
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
    /// The open "New split view" dialog.
    dialog: Option<new_split::Dialog>,
    /// The open tree row layout editor.
    tree_layout: Option<tree_layout_dialog::Dialog>,
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

/// Most nodes the dialog reads from the tree; a larger tree offers its first panes only.
const TREE_READ_LIMIT: usize = 4096;

/// Builds the "New split view" dialog for a group from the tree as it is drawn. Without the
/// `tree.read` grant the tree reads empty and the dialog offers no pane.
fn new_split_dialog(parent: u64) -> new_split::Dialog {
    let mut api = host();
    let roots = HostTree::roots(&mut *api);
    let root = roots.first().map_or(0, |root| root.0);
    let mut nodes = Vec::new();
    let mut panes = Vec::new();
    let mut stack: Vec<NodeId> = roots.into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        if nodes.len() >= TREE_READ_LIMIT {
            break;
        }
        let Some(view) = HostTree::get(&mut *api, id) else {
            continue;
        };
        let fact = new_split::TreeFact::from_view(&view);
        if fact.kind == "pane" {
            panes.push(fact.id);
        }
        nodes.push(fact);
        stack.extend(HostTree::children(&mut *api, id).into_iter().rev());
    }
    new_split::Dialog::from_facts(parent, root, &nodes, &panes)
}

impl Handler for LayoutClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(invoked)
                    if SPLIT_CHANNELS.contains(&invoked.command.as_str()) =>
                {
                    if let Some(pane) = invoked.context.pane {
                        submissions.push(message_to_server(
                            &invoked.command,
                            pane.0.to_le_bytes().to_vec(),
                        ));
                    }
                }
                Event::CommandInvoked(invoked) if invoked.command == NEW_SPLIT_CHANNEL => {
                    // Arguments without a group open nothing: there is nothing to split in.
                    if let Some(parent) = new_split::parent_argument(&invoked.arguments_json) {
                        let dialog = new_split_dialog(parent);
                        CLIENT.with(|cell| cell.borrow_mut().dialog = Some(dialog));
                        submissions.push(Submission::new(Intent::OpenView(OpenViewSpec {
                            view: new_split::view(),
                            focus: true,
                        })));
                    }
                }
                Event::CommandInvoked(invoked)
                    if invoked.command == tree_layout_dialog::COMMAND =>
                {
                    // Arguments without the layout and the specimen glyphs open nothing.
                    if let Some(request) =
                        tree_layout_dialog::Request::from_arguments(&invoked.arguments_json)
                    {
                        CLIENT.with(|cell| {
                            cell.borrow_mut().tree_layout =
                                Some(tree_layout_dialog::Dialog::new(request));
                        });
                        submissions.push(Submission::new(Intent::OpenView(OpenViewSpec {
                            view: tree_layout_dialog::view(),
                            focus: true,
                        })));
                    }
                }
                Event::UiAction(action) if action.view == tree_layout_dialog::view() => {
                    submissions.extend(tree_layout_action(
                        &action.key,
                        &action.action_id,
                        action.kind,
                        action.value,
                    ));
                }
                Event::UiAction(action) if action.view == new_split::view() => {
                    submissions.extend(new_split_action(
                        &action.key,
                        &action.action_id,
                        action.kind,
                        action.value,
                    ));
                }
                Event::PaneResized(resized) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.see(resized.node.0);
                    state
                        .sizes
                        .insert(resized.node.0, (resized.columns, resized.rows));
                }),
                Event::PaneCreated(info) => CLIENT.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.see(info.node.0);
                    state.sizes.insert(info.node.0, (info.columns, info.rows));
                    state.titles.insert(info.node.0, info.title);
                }),
                Event::PaneTitleChanged(changed) => CLIENT.with(|cell| {
                    cell.borrow_mut()
                        .titles
                        .insert(changed.node.0, changed.title);
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

/// Applies one action of the "New split view" dialog; when it ends the dialog, returns the
/// message for the server side (if a split is wanted) and the intent that closes the popup.
fn new_split_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = CLIENT.with(|cell| {
        let mut state = cell.borrow_mut();
        let step = state
            .dialog
            .as_mut()
            .map(|dialog| dialog.act(key, action_id, kind, value));
        if matches!(
            step,
            Some(new_split::Step::Create(_) | new_split::Step::Cancel)
        ) {
            state.dialog = None;
        }
        step
    });
    let close = || Submission::new(Intent::CloseView(new_split::view()));
    match step {
        Some(new_split::Step::Create(answer)) => vec![
            message_to_server(NEW_SPLIT_CHANNEL, answer.to_payload()),
            close(),
        ],
        // The kernel already removed a popup it closed itself; every other end closes it here.
        Some(new_split::Step::Cancel) if kind != ActionKind::Closed => vec![close()],
        _ => Vec::new(),
    }
}

/// Applies one action of the tree row layout editor. A change goes to the kernel at once; the
/// end of the dialog closes the popup (a popup the kernel closed itself is already gone).
fn tree_layout_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = CLIENT.with(|cell| {
        let mut state = cell.borrow_mut();
        let step = state
            .tree_layout
            .as_mut()
            .map(|dialog| dialog.act(key, action_id, kind, value));
        if matches!(
            step,
            Some(tree_layout_dialog::Step::Done | tree_layout_dialog::Step::Cancel(_))
        ) {
            state.tree_layout = None;
        }
        step
    });
    let set_layout = |layout: &tree_layout_rows::TreeRowLayout| {
        Submission::new(Intent::InvokeCommand(InvokeCommandSpec {
            command: tree_layout_dialog::SET_LAYOUT_COMMAND.to_owned(),
            arguments_json: tree_layout_dialog::set_layout_arguments(layout),
        }))
    };
    let close = || Submission::new(Intent::CloseView(tree_layout_dialog::view()));
    match step {
        Some(tree_layout_dialog::Step::Set(layout)) => vec![set_layout(&layout)],
        Some(tree_layout_dialog::Step::Done) if kind != ActionKind::Closed => vec![close()],
        Some(tree_layout_dialog::Step::Cancel(restore)) => restore
            .iter()
            .map(set_layout)
            .chain((kind != ActionKind::Closed).then(close))
            .collect(),
        _ => Vec::new(),
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
    fn render(&mut self, view: ViewTarget, area: Area) -> UiTree {
        CLIENT.with(|cell| {
            let state = cell.borrow();
            match (&state.dialog, &state.tree_layout) {
                (Some(dialog), _) if view == new_split::view() => dialog.render(),
                (_, Some(dialog)) if view == tree_layout_dialog::view() => dialog.render(),
                _ => build_preview(&state, usize::from(area.columns)),
            }
        })
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
    let rule = format!(
        "{} \u{254b} {}",
        "\u{2500}".repeat(8),
        "\u{2500}".repeat(10)
    );
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
            ["panes.read", "tree.read", "agent.read", "ui.panels"]
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
            [
                "command:ilium.core.layout/preset",
                "command:ilium.core.layout/new-split",
                "command:ilium.core.layout/tree-layout",
                "view:ilium.core.layout/presets"
            ]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_contributes_the_new_split_command() {
        let manifest = parsed_manifest();
        let commands: Vec<String> = manifest["contributes"]["commands"]
            .as_array()
            .expect("contributes.commands is an array")
            .iter()
            .map(|command| command["id"].as_str().expect("a command id").to_owned())
            .collect();
        assert_eq!(commands, ["preset", "new-split", "tree-layout"]);
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
