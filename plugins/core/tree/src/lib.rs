//! Core plugin `ilium.core.tree` (manual page core-plugin-tree.md).
//!
//! The server side keeps the tree order keys (`order` decorations), publishes
//! `ilium.core.tree/changed`, closes groups and folders that lose their last child, provides the
//! `ilium.core.tree/bookmarks` service and runs the node commands (move up and down, bookmark,
//! lock closed, indent, outdent). The client side turns the tree context menu into intents.

pub mod move_mode;
pub mod new_group;
pub mod node_ops;
pub mod ordering;
pub mod recent_activity;
pub mod toolbar_rows;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use ilium_plugin_sdk::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::generated::host::tree::HostTree;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::host::{HostIntentsHost, HostTopics};
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Activation, Area, CommandInvokedEvent, CreateFolderSpec,
    CreateGroupSpec, DecorationVisibility, ErrorCode, Intent, MessageSpec, MessageTarget, NodeId,
    OpenViewSpec, Outcome, Placement, PluginError, PluginId, PluginSide, SetBookmarkedSpec,
    SetDecorationSpec, SetNodeLockedSpec, Submission, UiTree, ViewTarget,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, ServiceProvider, Views};
use node_ops::{NodeOp, Snapshot, PLUGIN_ID};
use serde_json::{json, Value};

/// The topic published after every structural change.
pub const CHANGED_TOPIC: &str = "ilium.core.tree/changed";
/// The service that lists, adds and removes bookmarks.
pub const BOOKMARKS_SERVICE: &str = "ilium.core.tree/bookmarks";
/// The slot of the `order` decoration.
pub const ORDER_SLOT: &str = "order";
/// The metadata byte of a bookmarked node.
const BOOKMARK_BYTE: u8 = 1;
/// Most nodes read from the tree for one move decision.
const TREE_READ_LIMIT: usize = 4096;

/// Reads the kernel `[ui]` flag `key` from the live settings; a missing or unreadable value is `default`.
fn ui_flag(key: &str, default: bool) -> bool {
    flag_from_json(&host().get_json(), key, default)
}

/// The boolean `key` of a settings document; a missing, non-boolean or unparsable value is `default`.
fn flag_from_json(settings_json: &str, key: &str, default: bool) -> bool {
    serde_json::from_str::<Value>(settings_json)
        .ok()
        .and_then(|settings| settings.get(key).and_then(Value::as_bool))
        .unwrap_or(default)
}

/// The server side of `ilium.core.tree` (core-plugin-tree.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct TreeServer {
    /// Nodes seen in a `tree-changed` event and not yet removed.
    known: HashSet<u64>,
    /// The parent of each seen node, remembered so a removal can find the group it leaves.
    parents: HashMap<u64, u64>,
    /// The last `order` key submitted per node.
    keys: HashMap<u64, i64>,
    /// Intents of a service call that the host refused to take directly; sent with the next batch.
    pending: Vec<Submission>,
}

impl Lifecycle for TreeServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl TreeServer {
    fn publish_change(node: u64, change: &str) {
        let mut api = host();
        let payload = json!({ "node": node, "change": change }).to_string();
        if let Err(error) = HostTopics::publish(&mut *api, CHANGED_TOPIC.to_string(), payload) {
            ilium_plugin_sdk::log::log(
                Level::Warn,
                &format!("tree topic not published: {error:?}"),
            );
        }
    }

    /// Reads each changed node, publishes the change and refreshes its `order` key.
    fn observe(&mut self, nodes: Vec<NodeId>, submissions: &mut Vec<Submission>) {
        let mut api = host();
        for node in nodes {
            let Some(view) = HostTree::get(&mut *api, node) else {
                continue;
            };
            match view.parent {
                Some(parent) => {
                    self.parents.insert(node.0, parent.0);
                }
                None => {
                    self.parents.remove(&node.0);
                }
            }
            let change = if self.known.insert(node.0) {
                "added"
            } else {
                "updated"
            };
            Self::publish_change(node.0, change);
            let key = ordering::sort_key(&view.title);
            if self.keys.insert(node.0, key) != Some(key) {
                submissions.push(Submission::new(Intent::SetDecoration(order_decoration(
                    node, key,
                ))));
            }
        }
    }

    /// A node was removed: forget it and close the group or folder it leaves empty.
    fn removed(&mut self, node: u64, submissions: &mut Vec<Submission>) {
        let was_known = self.known.remove(&node);
        self.keys.remove(&node);
        if was_known {
            Self::publish_change(node, "removed");
        }
        let Some(parent) = self.parents.remove(&node) else {
            return;
        };
        if !ui_flag("ui.auto_remove_empty_groups", true) {
            return;
        }
        // A parent counts while its own removal has not been reported: the host reports a removal
        // after the tree already changed, so the parent may be gone from the tree by now.
        if !self.known.contains(&parent) {
            return;
        }
        let mut api = host();
        let parent_id = NodeId(parent);
        let locked = HostTree::get(&mut *api, parent_id).is_some_and(|view| view.locked);
        if locked || !HostTree::children(&mut *api, parent_id).is_empty() {
            return;
        }
        let bookmarked = HostTree::metadata(&mut *api, parent_id)
            .is_some_and(|bytes| bytes.first() == Some(&BOOKMARK_BYTE));
        if bookmarked {
            return;
        }
        submissions.push(Submission::keyed(
            Intent::CloseNode(parent_id),
            format!("close-empty-{parent}"),
        ));
    }

    /// The tree as the move rules read it.
    fn snapshot() -> Snapshot {
        let mut api = host();
        let mut views = Vec::new();
        let mut stack: Vec<NodeId> = HostTree::roots(&mut *api).into_iter().rev().collect();
        while let Some(id) = stack.pop() {
            if views.len() >= TREE_READ_LIMIT {
                break;
            }
            let Some(view) = HostTree::get(&mut *api, id) else {
                continue;
            };
            views.push(view);
            stack.extend(HostTree::children(&mut *api, id).into_iter().rev());
        }
        Snapshot::from_views(&views)
    }

    /// Sets or clears the bookmark byte of a node and returns the mirroring intent.
    fn set_bookmark(node: u64, on: bool) -> Result<Submission, PluginError> {
        let mut api = host();
        let id = NodeId(node);
        if HostTree::get(&mut *api, id).is_none() {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                format!("no node {node}"),
            ));
        }
        let bytes = if on { vec![BOOKMARK_BYTE] } else { Vec::new() };
        HostTree::set_metadata(&mut *api, id, bytes)?;
        Ok(Submission::new(Intent::SetBookmarked(SetBookmarkedSpec {
            node: id,
            bookmarked: on,
        })))
    }

    /// The bookmarked nodes, in tree order.
    fn bookmarks() -> Vec<u64> {
        let mut api = host();
        let mut found = Vec::new();
        let mut stack: Vec<NodeId> = HostTree::roots(&mut *api).into_iter().rev().collect();
        let mut visited = 0;
        while let Some(id) = stack.pop() {
            visited += 1;
            if visited > TREE_READ_LIMIT {
                break;
            }
            if HostTree::metadata(&mut *api, id)
                .is_some_and(|bytes| bytes.first() == Some(&BOOKMARK_BYTE))
            {
                found.push(id.0);
            }
            stack.extend(HostTree::children(&mut *api, id).into_iter().rev());
        }
        found
    }

    /// Runs one command; `None` when it is not a tree command or has nothing to do.
    fn command(event: &CommandInvokedEvent) -> Option<Submission> {
        let bare = event
            .command
            .strip_prefix(PLUGIN_ID)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(&event.command);
        let argument_node = serde_json::from_str::<Value>(&event.arguments_json)
            .ok()
            .and_then(|arguments| arguments.get("node").and_then(Value::as_u64));
        match bare {
            "indent" | "outdent" => {
                let node = event.context.pane.map(|pane| pane.0).or(argument_node)?;
                let intent = node_ops::indent_intent(node, bare == "outdent", &Self::snapshot())?;
                Some(Submission::new(intent))
            }
            _ => {
                let (operation, node) =
                    NodeOp::from_command(&event.command, &event.arguments_json).ok()?;
                if let NodeOp::Bookmark(on) = operation {
                    return Self::set_bookmark(node, on).ok();
                }
                let snapshot = match operation {
                    NodeOp::MoveUp | NodeOp::MoveDown => Self::snapshot(),
                    _ => Snapshot::default(),
                };
                node_ops::intent_for(operation, node, &snapshot).map(Submission::new)
            }
        }
    }

    /// Answers one bookmarks call: `{"op": "list" | "add" | "remove", "node"}`, or the op as the method.
    fn bookmarks_call(&mut self, method: &str, payload_json: &str) -> String {
        let request: Value = serde_json::from_str(payload_json).unwrap_or(Value::Null);
        let operation = request
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or(method)
            .to_string();
        let node = request.get("node").and_then(Value::as_u64);
        let (on, node) = match (operation.as_str(), node) {
            ("list", _) => return json!({ "bookmarks": Self::bookmarks() }).to_string(),
            ("add", Some(node)) => (true, node),
            ("remove", Some(node)) => (false, node),
            ("add" | "remove", None) => return error_payload("`node` is required"),
            (other, _) => return error_payload(&format!("unknown operation `{other}`")),
        };
        match Self::set_bookmark(node, on) {
            Ok(submission) => {
                let mut api = host();
                if HostIntentsHost::submit(&mut *api, submission.clone()).is_err() {
                    self.pending.push(submission);
                }
                json!({ "ok": true, "node": node, "bookmarked": on }).to_string()
            }
            Err(error) => error_payload(&error.message),
        }
    }
}

/// The error answer of a service call: the plugin cannot return an `Err`, so it answers JSON.
fn error_payload(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// The `order` decoration of a node: a key and no visible text, never persisted.
fn order_decoration(node: NodeId, key: i64) -> SetDecorationSpec {
    SetDecorationSpec {
        node,
        slot: ORDER_SLOT.to_string(),
        content: Vec::new(),
        icon: None,
        sort_key: Some(key),
        persist: false,
        spans: None,
        tooltip: None,
        frames: Vec::new(),
        frame_ms: 0,
        trailing: false,
        visibility: DecorationVisibility::Always,
        tone: None,
    }
}

impl Handler for TreeServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::TreeChanged(nodes) => self.observe(nodes, &mut submissions),
                Event::NodeRemoved(node) => self.removed(node.0, &mut submissions),
                Event::CommandInvoked(command) => {
                    submissions.extend(Self::command(&command));
                }
                Event::Message(message) => {
                    if let Some(submission) =
                        creation_from_message(&message.channel, &message.payload)
                            .or_else(|| dialog_creation(&message.channel, &message.payload))
                    {
                        submissions.push(submission);
                    }
                }
                Event::RequestCompleted(result) => {
                    if let Outcome::Refused(error) = result.outcome {
                        if error.code == ErrorCode::Conflict {
                            ilium_plugin_sdk::log::log(
                                Level::Info,
                                "manual name kept; not retrying",
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        submissions.append(&mut self.pending);
        submissions
    }
}

impl ServiceProvider for TreeServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        // `ilium.core.tree/bookmarks` is the only service this plugin provides.
        ServiceReply::Done(self.bookmarks_call(&call.method, &call.payload_json))
    }
}

/// Turns a `new-group` / `new-folder` message (8 little-endian bytes: the parent node) into a
/// creation keyed `<channel>-<parent>`.
fn creation_from_message(channel: &str, payload: &[u8]) -> Option<Submission> {
    let bytes: [u8; 8] = payload.try_into().ok()?;
    let parent = u64::from_le_bytes(bytes);
    let intent = match channel {
        "new-group" => Intent::CreateGroup(CreateGroupSpec {
            parent: Some(NodeId(parent)),
            title: "New group".to_string(),
            placement: Placement::InsideLast,
            children: Vec::new(),
        }),
        "new-folder" => Intent::CreateFolder(CreateFolderSpec {
            parent: Some(NodeId(parent)),
            title: "New folder".to_string(),
            path: None,
        }),
        _ => return None,
    };
    Some(Submission::keyed(intent, format!("{channel}-{parent}")))
}

/// Turns the `create-group` answer of the "New group" dialog (JSON `{"parent", "name"}`) into a
/// group under that parent. Anything else is not for this handler.
fn dialog_creation(channel: &str, payload: &[u8]) -> Option<Submission> {
    if channel != new_group::CHANNEL {
        return None;
    }
    let answer = new_group::Answer::from_payload(payload)?;
    Some(Submission::new(Intent::CreateGroup(CreateGroupSpec {
        parent: Some(NodeId(answer.parent)),
        title: answer.name,
        placement: Placement::InsideLast,
        children: Vec::new(),
    })))
}

thread_local! {
    /// The open "New group" dialog of this client, if any.
    static NEW_GROUP: RefCell<Option<new_group::Dialog>> = const { RefCell::new(None) };
}

/// Most nodes the dialog reads from the tree; a larger tree offers its first groups only.
const TREE_READ_LIMIT_DIALOG: usize = 4096;

/// Reads the tree in drawn order as the facts the dialog needs. Without the `tree.read` grant
/// the tree reads empty and the dialog offers no destination.
fn group_facts() -> Vec<new_group::GroupFact> {
    let mut api = host();
    let mut facts = Vec::new();
    let mut stack: Vec<NodeId> = HostTree::roots(&mut *api).into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        if facts.len() >= TREE_READ_LIMIT_DIALOG {
            break;
        }
        let Some(view) = HostTree::get(&mut *api, id) else {
            continue;
        };
        facts.push(new_group::GroupFact::from_view(&view));
        stack.extend(HostTree::children(&mut *api, id).into_iter().rev());
    }
    facts
}

/// A message to the server side of this plugin.
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

/// Opens the "New group" dialog for the `new-group` command. Without a `parent` argument the
/// top level is preselected.
fn open_new_group(arguments_json: &str) -> Vec<Submission> {
    let facts = group_facts();
    let preselected = new_group::parent_argument(arguments_json)
        .or_else(|| facts.first().map(|fact| fact.id))
        .unwrap_or(0);
    let dialog = new_group::Dialog::from_facts(preselected, &facts);
    NEW_GROUP.with(|cell| *cell.borrow_mut() = Some(dialog));
    vec![Submission::new(Intent::OpenView(OpenViewSpec {
        view: new_group::view(),
        focus: true,
    }))]
}

/// Applies one action of the "New group" dialog; when it ends the dialog, returns the message
/// for the server side (if a group is wanted) and the intent that closes the popup.
fn new_group_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = NEW_GROUP.with(|cell| {
        let mut state = cell.borrow_mut();
        let step = state
            .as_mut()
            .map(|dialog| dialog.act(key, action_id, kind, value));
        if matches!(
            step,
            Some(new_group::Step::Create(_) | new_group::Step::Cancel)
        ) {
            *state = None;
        }
        step
    });
    let close = || Submission::new(Intent::CloseView(new_group::view()));
    match step {
        Some(new_group::Step::Create(answer)) => {
            vec![
                message_to_server(new_group::CHANNEL, answer.to_payload()),
                close(),
            ]
        }
        // The kernel already removed a popup it closed itself; every other end closes it here.
        Some(new_group::Step::Cancel) if kind != ActionKind::Closed => vec![close()],
        _ => Vec::new(),
    }
}

/// The client side of `ilium.core.tree` (core-plugin-tree.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct TreeClient;

impl Lifecycle for TreeClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        NEW_GROUP.with(|cell| *cell.borrow_mut() = None);
        Ok(())
    }
}

impl Views for TreeClient {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        NEW_GROUP.with(|cell| match cell.borrow().as_ref() {
            Some(dialog) if view == new_group::view() => dialog.render(),
            _ => new_group::blank(),
        })
    }
}

impl Handler for TreeClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let action = match event {
                Event::CommandInvoked(invoked) => {
                    let bare = invoked
                        .command
                        .strip_prefix(PLUGIN_ID)
                        .and_then(|rest| rest.strip_prefix('/'))
                        .unwrap_or(&invoked.command);
                    if bare == new_group::POPUP {
                        submissions.extend(open_new_group(&invoked.arguments_json));
                    }
                    continue;
                }
                Event::UiAction(action) if action.view == new_group::view() => {
                    submissions.extend(new_group_action(
                        &action.key,
                        &action.action_id,
                        action.kind,
                        action.value,
                    ));
                    continue;
                }
                Event::UiAction(action) => action,
                _ => continue,
            };
            if action.kind != ActionKind::Activated {
                continue;
            }
            let Some(node) = action
                .key
                .strip_prefix("row-")
                .and_then(|digits| digits.parse::<u64>().ok())
            else {
                continue;
            };
            let Some(ActionValue::Text(item)) = action.value else {
                continue;
            };
            let intent = match item.as_str() {
                "lock" if ui_flag("ui.lock_closed_enabled", true) => {
                    Intent::SetNodeLocked(SetNodeLockedSpec {
                        node: NodeId(node),
                        locked: true,
                    })
                }
                "unlock" => Intent::SetNodeLocked(SetNodeLockedSpec {
                    node: NodeId(node),
                    locked: false,
                }),
                "close" => Intent::CloseNode(NodeId(node)),
                _ => continue,
            };
            submissions.push(Submission::new(intent));
        }
        submissions
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// The four essential core plugins that cannot be disabled (LIF-183, LIF-276).
    const ESSENTIAL_PLUGIN_IDS: [&str; 4] = [
        "ilium.core.tree",
        "ilium.core.layout",
        "ilium.core.settings",
        "ilium.core.sessions",
    ];

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()
            .and_then(|points| points.iter().find(|point| point["id"] == point_id))
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identity_matches_core_page() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.tree");
        assert_eq!(manifest["name"], "Tree");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["license"], "MIT");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_sides_and_worlds_match_core_page() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn server_grants_match_core_page() {
        let mut grants = string_list(&manifest()["capabilities"]["server"]);
        grants.sort();
        let mut expected = vec![
            "tree.read",
            "tree.write",
            "tree.metadata",
            "tree.decorate",
            "tree.restructure",
            "extensions.provide:ilium.core.tree/before-move",
            "extensions.provide:ilium.core.tree/before-close",
            "extensions.provide:ilium.core.tree/node-sorts",
            "services.provide:ilium.core.tree/bookmarks",
        ];
        expected.sort();
        assert_eq!(grants, expected);
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_are_the_tree_read_and_the_popup() {
        let grants = string_list(&manifest()["capabilities"]["client"]);
        assert_eq!(grants, vec!["tree.read".to_owned(), "ui.panels".to_owned()]);
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec!["startup".to_owned()]
        );
    }

    /// REQ: EXT-052
    #[test]
    fn veto_hooks_declare_mode_deadline_and_limit() {
        let manifest = manifest();
        for point_id in ["before-move", "before-close"] {
            let point = extension_point(&manifest, point_id)
                .unwrap_or_else(|| panic!("extension point {point_id} is declared"));
            assert_eq!(point["kind"], "hook");
            assert_eq!(point["mode"], "veto");
            assert_eq!(point["version"], "0.1.0");
            assert_eq!(point["deadline_ms"], 20);
            assert_eq!(point["max_handlers"], 8);
        }
    }

    /// REQ: EXT-052
    #[test]
    fn node_sorts_is_static_collect_with_64_entries() {
        let manifest = manifest();
        let point = extension_point(&manifest, "node-sorts").expect("node-sorts is declared");
        assert_eq!(point["kind"], "collect");
        assert_eq!(point["version"], "0.1.0");
        assert_eq!(point["max_entries"], 64);
        assert_eq!(point["dynamic"], false);
    }

    /// REQ: LIF-183
    #[test]
    fn tree_plugin_is_in_the_essential_set() {
        assert_eq!(ESSENTIAL_PLUGIN_IDS.len(), 4);
        let manifest = manifest();
        let plugin_id = manifest["id"].as_str().expect("manifest id is a string");
        assert!(ESSENTIAL_PLUGIN_IDS.contains(&plugin_id));
    }

    /// REQ: EXT-052
    #[test]
    fn undeclared_extension_point_is_absent() {
        assert!(extension_point(&manifest(), "after-close").is_none());
    }

    /// REQ: F-006
    #[test]
    fn the_empty_group_flag_defaults_to_on_and_reads_a_boolean() {
        let key = "ui.auto_remove_empty_groups";
        assert!(flag_from_json("{}", key, true));
        assert!(flag_from_json("not json", key, true));
        assert!(flag_from_json(
            r#"{"ui.auto_remove_empty_groups": "yes"}"#,
            key,
            true
        ));
        assert!(!flag_from_json(
            r#"{"ui.auto_remove_empty_groups": false}"#,
            key,
            true
        ));
        assert!(flag_from_json(
            r#"{"ui.auto_remove_empty_groups": true}"#,
            key,
            false
        ));
    }

    #[test]
    fn empty_event_batch_submits_nothing() {
        let submissions = TreeServer::default().on_events(Vec::new());
        assert!(submissions.is_empty());
    }
}
