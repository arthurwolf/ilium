//! "Ask for update" and "Send message to all" (core-plugin-fleet.md "Ask for update").
//!
//! The client side owns the dialog (one message field, Send and Cancel) and the commands the
//! tree menus invoke; the server side decides who receives the text. A row with a scope sends
//! the JSON channel [`SCOPED_CHANNEL`]; the command palette sends the raw-text channel
//! [`CHANNEL`] and reaches every idle agent. Scope rules: a project or group reaches every agent
//! below it, a folder reaches the agents of its project whose working directory lies inside the
//! folder (compared by path components, so `src-extra` is not inside `src`), a pane reaches
//! itself. The fixed status request skips agents that are not idle, except below a group or
//! folder, where the user chose the place explicitly.

use std::path::{Component, Path, PathBuf};

use ilium_plugin_sdk::generated::host::tree::HostTree;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, AgentActivity, ButtonRole, Constraint, ContainerKind, Delivery,
    DialogButton, DialogKind, DialogSpec, Intent, MessageSpec, MessageTarget, NodeClass, NodeFlags,
    NodeId, NodeView, ParagraphSpec, PluginId, PluginSide, SendInputSpec, Span, Style, Submission,
    TextInputSpec, ToastSpec, Tone, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

/// The plugin id, also the target of the client-to-server messages.
const PLUGIN_ID: &str = "ilium.core.fleet";
/// Raw-text channel: the payload is the message; it reaches every idle agent.
pub const CHANNEL: &str = "ask-for-update";
/// JSON channel: `{"node": <id>, "text": <string, absent for the fixed status request>}`.
pub const SCOPED_CHANNEL: &str = "ask-for-update-scoped";
/// Popup id of the dialog (also the command that opens it).
pub const POPUP: &str = "ask-for-update";
/// Command that sends the fixed status request without a dialog.
pub const STATUS_COMMAND: &str = "ask-for-update-status";
/// Key of the message field.
pub const TEXT_KEY: &str = "update-text";
/// Id of the Send button.
pub const SEND_ID: &str = "update-send";
/// Id of the Cancel button.
pub const CANCEL_ID: &str = "update-cancel";
/// The fixed status request.
pub const STATUS_REQUEST: &str = "please remind me, in a very compact way, what you were doing, what I asked you to do, how it went, etc, remind me what's going on";
/// Shown when nobody idle is in scope.
pub const NO_IDLE_NOTICE: &str = "No idle agent to ask for an update";
/// Shown when a scope holds no agent at all.
pub const NO_AGENT_NOTICE: &str = "No agent in this folder";
/// Most levels of ancestors and most nodes one scope walk visits.
const WALK_LIMIT: usize = 4096;

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

// ---------------------------------------------------------------------------------------------
// Client side
// ---------------------------------------------------------------------------------------------

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dialog {
    /// The row the dialog was opened for; `None` for the command palette.
    pub node: Option<u64>,
    /// Who receives the text, as the dialog says it.
    pub target: String,
    pub text: String,
    pub notice: Option<String>,
}

/// What an action did to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Stay,
    /// Finished: send this message to the server side.
    Send {
        channel: &'static str,
        payload: Vec<u8>,
    },
    Cancel,
}

/// The row from the command arguments `{"node": id}`; `None` for `{}` or anything else.
pub fn node_argument(arguments_json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(arguments_json).ok()?;
    value.get("node")?.as_u64()
}

/// The message that carries one ask to the server side of this plugin.
pub fn message_to_server(channel: &str, payload: Vec<u8>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_owned()),
            side: PluginSide::Server,
            client: None,
        },
        channel: channel.to_owned(),
        payload,
    }))
}

/// The ask the command `ask-for-update-status` sends: the fixed request for one row, or for
/// every idle agent when the command has no row.
pub fn status_message(node: Option<u64>) -> Submission {
    match node {
        Some(node) => message_to_server(
            SCOPED_CHANNEL,
            json!({ "node": node }).to_string().into_bytes(),
        ),
        None => message_to_server(CHANNEL, STATUS_REQUEST.as_bytes().to_vec()),
    }
}

impl Dialog {
    pub fn new(node: Option<u64>, target: String) -> Self {
        Self {
            node,
            target,
            text: String::new(),
            notice: None,
        }
    }

    fn send(&mut self) -> Step {
        if self.text.trim().is_empty() {
            self.notice = Some("Enter a message".to_owned());
            return Step::Stay;
        }
        match self.node {
            Some(node) => Step::Send {
                channel: SCOPED_CHANNEL,
                payload: json!({ "node": node, "text": self.text })
                    .to_string()
                    .into_bytes(),
            },
            None => Step::Send {
                channel: CHANNEL,
                payload: self.text.clone().into_bytes(),
            },
        }
    }

    /// Applies one action of the popup. A button reports its id as `action_id`; a host that
    /// reports it as the key is read the same way.
    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        let id = if action_id.is_empty() { key } else { action_id };
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed, Some(ActionValue::Text(text))) if key == TEXT_KEY => {
                self.text = text;
                self.notice = None;
                Step::Stay
            }
            (ActionKind::Activated, _) if id == CANCEL_ID => Step::Cancel,
            (ActionKind::Activated, _) if id == SEND_ID || key == TEXT_KEY => self.send(),
            _ => Step::Stay,
        }
    }

    /// The popup view.
    pub fn render(&self) -> UiTree {
        let mut message = vec![format!("Sends to: {}", self.target)];
        message.extend(self.notice.clone());
        let dialog = DialogSpec {
            title: vec![span("Ask for update")],
            kind: DialogKind::Plain,
            buttons: vec![
                button(SEND_ID, "Send", ButtonRole::Primary, false),
                button(CANCEL_ID, "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Max(60),
            height: Constraint::Min(0),
            message: Some(message.iter().map(|line| vec![span(line)]).collect()),
        };
        let input = TextInputSpec {
            label: "Message".to_owned(),
            initial: self.text.clone(),
            placeholder: None,
            secret: false,
            reveal_last: 0,
            reveal: false,
            max_length: 0,
            submit_on_enter: true,
            debounce_ms: 0,
            validation: None,
            suggestions: None,
        };
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                node(TEXT_KEY, Widget::TextInput(input)),
            ],
            root: 0,
        }
    }

    /// An empty paragraph tree, drawn when the popup view is asked for with no dialog open.
    pub fn closed_view() -> UiTree {
        UiTree {
            version: 1,
            nodes: vec![node(
                "empty",
                Widget::Paragraph(ParagraphSpec {
                    lines: Vec::new(),
                    rich: None,
                    wrap: None,
                    alignment: ilium_plugin_sdk::types::Alignment::Left,
                    style: Style::default(),
                    scrollable: false,
                }),
            )],
            root: 0,
        }
    }
}

fn span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

fn button(id: &str, label: &str, role: ButtonRole, is_cancel: bool) -> DialogButton {
    DialogButton {
        id: id.to_owned(),
        label: label.to_owned(),
        role,
        is_cancel,
    }
}

fn node(key: &str, widget: Widget) -> UiNode {
    UiNode {
        key: Some(key.to_owned()),
        widget,
        children: Vec::new(),
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Server side
// ---------------------------------------------------------------------------------------------

/// What a scope covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every agent below `root` (inclusive). `explicit` is true for a group, where the fixed
    /// request also reaches agents that are not idle.
    Structural { root: u64, explicit: bool },
    /// The agents of the project `boundary` whose working directory lies inside `path`.
    Filesystem { boundary: u64, path: PathBuf },
}

fn terminal_agent(view: &NodeView) -> Option<AgentActivity> {
    if view.class != NodeClass::Pane || view.content_kind.as_deref() != Some("ilium.terminal") {
        return None;
    }
    view.agent
        .as_ref()
        .map(|agent| agent.activity)
        .filter(|activity| *activity != AgentActivity::None)
}

fn normalized(path: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// The project a node belongs to: the nearest ancestor (or the node) that is a project, else the
/// top-level container above it.
fn project_of(api: &mut dyn HostApi, id: NodeId) -> Option<NodeId> {
    let mut top = None;
    let mut current = Some(id);
    for _ in 0..WALK_LIMIT {
        let Some(view) = current.and_then(|node| HostTree::get(api, node)) else {
            break;
        };
        if view.container == Some(ContainerKind::Project) {
            return Some(view.id);
        }
        if view.class == NodeClass::Container {
            top = Some(view.id);
        }
        current = view.parent;
    }
    top
}

/// Captures the scope of the row `node`, or says why there is none.
pub fn capture(api: &mut dyn HostApi, node: NodeId) -> Result<Scope, String> {
    let Some(view) = HostTree::get(api, node) else {
        return Err("The selected project or folder no longer exists".to_owned());
    };
    match view.class {
        NodeClass::Pane => Ok(Scope::Structural {
            root: node.0,
            explicit: false,
        }),
        NodeClass::Container => Ok(Scope::Structural {
            root: node.0,
            explicit: view.container != Some(ContainerKind::Project),
        }),
        NodeClass::Folder => {
            let boundary =
                project_of(api, node).ok_or("The folder has no containing project or group")?;
            let path = view
                .cwd
                .as_deref()
                .map(normalized)
                .ok_or("The folder path cannot be resolved; reopen from its project")?;
            Ok(Scope::Filesystem {
                boundary: boundary.0,
                path,
            })
        }
    }
}

/// The agent panes of a scope with their activity, in tree order.
pub fn recipients(api: &mut dyn HostApi, scope: &Scope) -> Vec<(NodeId, AgentActivity)> {
    let root = match scope {
        Scope::Structural { root, .. } => *root,
        Scope::Filesystem { boundary, .. } => *boundary,
    };
    let mut found = Vec::new();
    let mut stack = vec![NodeId(root)];
    let mut visited = 0;
    while let Some(id) = stack.pop() {
        visited += 1;
        if visited > WALK_LIMIT {
            break;
        }
        let Some(view) = HostTree::get(api, id) else {
            continue;
        };
        if let Some(activity) = terminal_agent(&view) {
            let inside = match scope {
                Scope::Structural { .. } => true,
                Scope::Filesystem { boundary, path } => {
                    project_of(api, id) == Some(NodeId(*boundary))
                        && view
                            .cwd
                            .as_deref()
                            .map(normalized)
                            .is_some_and(|cwd| cwd.starts_with(path))
                }
            };
            if inside {
                found.push((id, activity));
            }
        }
        let mut children = HostTree::children(api, id);
        children.reverse();
        stack.extend(children);
    }
    found
}

fn input(pane: NodeId, text: &str, delivery: Delivery) -> Submission {
    Submission::new(Intent::SendInput(SendInputSpec {
        pane,
        text: text.to_owned(),
        delivery,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }))
}

fn notice(text: &str) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("fleet-ask-for-update".to_owned()),
        tone: Tone::Info,
        title: text.chars().take(60).collect(),
        body: None,
        actions: Vec::new(),
        duration_ms: None,
    }))
}

fn report(count: usize) -> Submission {
    notice(&if count == 1 {
        "Asked for an update".to_owned()
    } else {
        format!("Asked {count} agents for an update")
    })
}

/// Handles the raw-text channel: the text goes to every idle agent. Blank text sends nothing; a
/// multi-line text is pasted, a single line is submitted.
pub fn handle_text(api: &mut dyn HostApi, payload: &[u8]) -> Vec<Submission> {
    let Ok(text) = std::str::from_utf8(payload) else {
        return Vec::new();
    };
    if text.trim().is_empty() {
        return Vec::new();
    }
    let delivery = if text.contains('\n') {
        Delivery::Pasted
    } else {
        Delivery::Submitted
    };
    let mut out = Vec::new();
    for id in HostTree::find(api, ilium_plugin_sdk::types::NodeQuery::default()) {
        let Some(view) = HostTree::get(api, id) else {
            continue;
        };
        if terminal_agent(&view) == Some(AgentActivity::Idle) {
            out.push(input(id, text, delivery));
        }
    }
    if out.is_empty() {
        return vec![notice(NO_IDLE_NOTICE)];
    }
    let count = out.len();
    out.push(report(count));
    out
}

/// Handles the scoped channel `{"node": id, "text": string?}`.
pub fn handle_scoped(api: &mut dyn HostApi, payload: &[u8]) -> Vec<Submission> {
    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        return Vec::new();
    };
    let Some(node) = value.get("node").and_then(Value::as_u64) else {
        return Vec::new();
    };
    let text = value.get("text").and_then(Value::as_str);
    if text.is_some_and(|text| text.trim().is_empty()) {
        return Vec::new();
    }
    let scope = match capture(api, NodeId(node)) {
        Ok(scope) => scope,
        Err(error) => return vec![notice(&error)],
    };
    // A typed message goes to every agent in scope. The fixed request skips agents that are not
    // idle unless the scope is a group or a folder.
    let everyone = text.is_some()
        || matches!(
            scope,
            Scope::Filesystem { .. } | Scope::Structural { explicit: true, .. }
        );
    let found = recipients(api, &scope);
    let mut out: Vec<Submission> = found
        .iter()
        .filter(|(_, activity)| everyone || *activity == AgentActivity::Idle)
        .map(|(pane, _)| input(*pane, text.unwrap_or(STATUS_REQUEST), Delivery::Submitted))
        .collect();
    if out.is_empty() {
        let message = if found.is_empty() && matches!(scope, Scope::Filesystem { .. }) {
            NO_AGENT_NOTICE
        } else if text.is_some() {
            "No agent to message"
        } else {
            NO_IDLE_NOTICE
        };
        return vec![notice(message)];
    }
    let count = out.len();
    out.push(if text.is_some() {
        notice(&format!("Sent the message to {count} agents"))
    } else {
        report(count)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_argument_reads_the_row_and_nothing_else() {
        assert_eq!(node_argument(r#"{"node": 7}"#), Some(7));
        assert_eq!(node_argument("{}"), None);
        assert_eq!(node_argument("not json"), None);
    }

    #[test]
    fn components_not_prefixes_decide_membership() {
        assert!(normalized("/a/src/deep").starts_with(normalized("/a/src")));
        assert!(!normalized("/a/src-extra").starts_with(normalized("/a/src")));
        assert_eq!(normalized("/a/b/../c/./d"), PathBuf::from("/a/c/d"));
    }

    #[test]
    fn a_blank_message_stays_in_the_dialog() {
        let mut dialog = Dialog::new(None, "All agents".to_owned());
        dialog.act(
            TEXT_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Text("  ".to_owned())),
        );
        let step = dialog.act(SEND_ID, "", ActionKind::Activated, Some(ActionValue::None));
        assert_eq!(step, Step::Stay);
        assert!(dialog.notice.is_some());
    }

    #[test]
    fn a_row_dialog_sends_the_scoped_json_and_the_palette_dialog_sends_raw_text() {
        let mut row = Dialog::new(Some(7), "app".to_owned());
        row.text = "hello".to_owned();
        let Step::Send { channel, payload } = row.send() else {
            panic!("a message is sent");
        };
        assert_eq!(channel, SCOPED_CHANNEL);
        assert_eq!(
            serde_json::from_slice::<Value>(&payload).expect("JSON"),
            json!({"node": 7, "text": "hello"})
        );
        let mut palette = Dialog::new(None, "All agents".to_owned());
        palette.text = "hi".to_owned();
        assert_eq!(
            palette.send(),
            Step::Send {
                channel: CHANNEL,
                payload: b"hi".to_vec()
            }
        );
    }
}
