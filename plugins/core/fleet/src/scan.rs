//! What `ilium panes` and `ilium broadcast` share: which sessions are running and which panes
//! each one holds.
//!
//! A pane list comes from one of two places. The session the command runs in is read from the
//! host tree (`ilium broadcast` acts on it directly through intents); every other session is
//! asked with a `list-panes` cross-session request and answers one JSON record per pane.

use std::collections::HashMap;
use std::path::PathBuf;

use ilium_plugin_sdk::host::{CrossRequest, HostSessions, HostTree, LiveSession};
use ilium_plugin_sdk::types::{AgentActivity, NodeClass, NodeId, NodeQuery, NodeView, Outcome};
use ilium_plugin_sdk::HostApi;
use serde_json::Value;

use crate::pane_filter::{AgentStateName, PaneFacts, PaneKind};

/// A running session, as the host lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The session name (`default`, `review`, ...).
    pub name: String,
    /// The endpoint identity cross-session requests are addressed to.
    pub socket_id: String,
    /// The project directory; empty when the host does not know it.
    pub project: String,
    /// Whether the command runs in this session.
    pub is_self: bool,
}

impl From<LiveSession> for SessionRef {
    fn from(live: LiveSession) -> Self {
        Self {
            name: live.name,
            socket_id: live.socket_id,
            project: live.project,
            is_self: live.is_self,
        }
    }
}

/// Every live session, the command's own first.
pub fn live_sessions(api: &mut dyn HostApi) -> Vec<SessionRef> {
    let mut sessions: Vec<SessionRef> = HostSessions::list_live(api)
        .into_iter()
        .map(SessionRef::from)
        .collect();
    sessions.sort_by_key(|session| !session.is_self);
    sessions
}

/// The panes of one session, or why they could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionPanes {
    Panes(Vec<PaneFacts>),
    Unreachable(String),
}

/// The pane lists of a set of sessions, filled in as the replies arrive.
#[derive(Debug)]
pub struct Scan {
    pub sessions: Vec<SessionRef>,
    pub panes: Vec<Option<SessionPanes>>,
    /// Cross-session request id -> index into `sessions`.
    waiting: HashMap<u64, usize>,
    caller: Option<NodeId>,
}

impl Scan {
    /// Starts reading every session. `own_from_tree` reads the command's own session from the
    /// host tree instead of asking it; `ask_others` false leaves the other sessions out (they
    /// hold no panes the caller named).
    pub fn start(
        api: &mut dyn HostApi,
        sessions: Vec<SessionRef>,
        caller: Option<NodeId>,
        own_from_tree: bool,
        ask_others: bool,
    ) -> Self {
        let mut scan = Self {
            panes: vec![None; sessions.len()],
            sessions,
            waiting: HashMap::new(),
            caller,
        };
        for index in 0..scan.sessions.len() {
            let session = scan.sessions[index].clone();
            if session.is_self && own_from_tree {
                scan.panes[index] = Some(SessionPanes::Panes(tree_panes(api, &session, caller)));
                continue;
            }
            if !session.is_self && !ask_others {
                scan.panes[index] = Some(SessionPanes::Panes(Vec::new()));
                continue;
            }
            let sent = HostSessions::send(
                api,
                CrossRequest {
                    session: session.socket_id.clone(),
                    command: "list-panes".to_owned(),
                    arguments_json: "{}".to_owned(),
                },
            );
            match sent {
                Ok(request) => {
                    scan.waiting.insert(request.0, index);
                }
                Err(error) => {
                    scan.panes[index] = Some(SessionPanes::Unreachable(error.message));
                }
            }
        }
        scan
    }

    /// The ids of the requests still unanswered.
    pub fn pending_requests(&self) -> Vec<u64> {
        self.waiting.keys().copied().collect()
    }

    /// Whether every session has an answer.
    pub fn is_complete(&self) -> bool {
        self.panes.iter().all(Option::is_some)
    }

    /// Takes in the completion of `request`; false when it is none of this scan's.
    pub fn on_completed(&mut self, api: &mut dyn HostApi, request: u64, outcome: &Outcome) -> bool {
        let Some(index) = self.waiting.remove(&request) else {
            return false;
        };
        let session = self.sessions[index].clone();
        let reply = HostSessions::take_reply(api, ilium_plugin_sdk::types::RequestId(request));
        self.panes[index] = Some(match outcome {
            Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => {
                parse_reply(reply.as_deref(), &session, self.caller)
            }
            Outcome::Failed(error) | Outcome::Refused(error) => {
                SessionPanes::Unreachable(error.message.clone())
            }
            Outcome::Duplicate(_) | Outcome::Cancelled => {
                SessionPanes::Unreachable("the request was not carried out".to_owned())
            }
        });
        true
    }
}

fn parse_reply(reply: Option<&str>, session: &SessionRef, caller: Option<NodeId>) -> SessionPanes {
    let Some(text) = reply else {
        return SessionPanes::Unreachable("the session sent no answer".to_owned());
    };
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return SessionPanes::Unreachable(format!("the session answered nonsense: {error}"))
        }
    };
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return SessionPanes::Unreachable(message.to_owned());
    }
    let Some(records) = value.as_array() else {
        return SessionPanes::Unreachable(
            "the session answered something else than a list".to_owned(),
        );
    };
    let panes = records
        .iter()
        .filter_map(|record| PaneFacts::from_record(record, &session.name, &session.socket_id))
        .map(|mut facts| {
            facts.is_self = session.is_self && caller.is_some_and(|pane| pane.0 == facts.pane_id);
            facts
        })
        .collect();
    SessionPanes::Panes(panes)
}

/// The panes of the command's own session, read from the host tree.
pub fn tree_panes(
    api: &mut dyn HostApi,
    session: &SessionRef,
    caller: Option<NodeId>,
) -> Vec<PaneFacts> {
    let mut panes = Vec::new();
    for id in HostTree::find(api, NodeQuery::default()) {
        let Some(node) = HostTree::get(api, id) else {
            continue;
        };
        if node.class != NodeClass::Pane {
            continue;
        }
        panes.push(facts_of(&node, session, caller));
    }
    panes
}

fn facts_of(node: &NodeView, session: &SessionRef, caller: Option<NodeId>) -> PaneFacts {
    let content_kind = node.content_kind.as_deref().unwrap_or("");
    let kind = match (content_kind, &node.agent) {
        ("ilium.terminal", Some(_)) => PaneKind::Agent,
        ("ilium.terminal", None) => PaneKind::Shell,
        ("ilium.editor", _) => PaneKind::Editor,
        ("ilium.board", _) => PaneKind::Board,
        _ => PaneKind::Plugin,
    };
    let agent = node
        .agent
        .as_ref()
        .map(|agent| display_name(&agent.signature));
    let state = node.agent.as_ref().map(|agent| match agent.activity {
        AgentActivity::Working => AgentStateName::Working,
        AgentActivity::AwaitingApproval | AgentActivity::Menu => AgentStateName::WaitingApproval,
        AgentActivity::Done => AgentStateName::Done,
        AgentActivity::Idle | AgentActivity::None => AgentStateName::Idle,
    });
    let project = (!session.project.is_empty()).then(|| PathBuf::from(&session.project));
    PaneFacts {
        session_name: session.name.clone(),
        session_id: session.socket_id.clone(),
        pane_id: node.id.0,
        name: node.title.clone(),
        short_name: None,
        project: project.clone(),
        cwd: node.cwd.as_ref().map(PathBuf::from).or(project),
        kind,
        agent,
        state,
        is_self: caller == Some(node.id),
    }
}

/// `claude` -> `Claude`: the signature with a capital first letter.
fn display_name(signature: &str) -> String {
    let mut characters = signature.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_gets_a_capital_first_letter() {
        assert_eq!(display_name("claude"), "Claude");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn a_reply_with_an_error_is_an_unreachable_session() {
        let session = SessionRef {
            name: "review".to_owned(),
            socket_id: "review".to_owned(),
            project: String::new(),
            is_self: false,
        };
        assert_eq!(
            parse_reply(Some(r#"{"error":"gone"}"#), &session, None),
            SessionPanes::Unreachable("gone".to_owned())
        );
        assert!(matches!(
            parse_reply(None, &session, None),
            SessionPanes::Unreachable(_)
        ));
        assert!(matches!(
            parse_reply(Some("not json"), &session, None),
            SessionPanes::Unreachable(_)
        ));
    }

    #[test]
    fn a_pane_list_reply_marks_the_callers_pane_in_its_own_session_only() {
        let reply =
            r#"[{"pane_id":3,"name":"a","kind":"agent"},{"pane_id":4,"name":"b","kind":"shell"}]"#;
        let own = SessionRef {
            name: "default".to_owned(),
            socket_id: "default".to_owned(),
            project: String::new(),
            is_self: true,
        };
        let SessionPanes::Panes(panes) = parse_reply(Some(reply), &own, Some(NodeId(3))) else {
            panic!("a list is panes");
        };
        assert!(panes[0].is_self && !panes[1].is_self);
        let other = SessionRef {
            is_self: false,
            ..own
        };
        let SessionPanes::Panes(panes) = parse_reply(Some(reply), &other, Some(NodeId(3))) else {
            panic!("a list is panes");
        };
        assert!(panes.iter().all(|pane| !pane.is_self));
    }
}
