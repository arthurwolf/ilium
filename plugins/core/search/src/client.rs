//! The client side of `ilium.core.search`: the search panel's state and what its events do.
//!
//! The panel has a query field and a grouped result list. Typing only edits the draft; the
//! kernel debounces the field (150 ms) and reports the settled text as `FilterChanged`, which
//! sends one `query` message to the server. An answer carries the revision of the query it
//! answers and is shown only when that revision is the newest one asked. Activating a result
//! jumps to it (a pane is focused, a file opens in the editor) and closes the panel. State
//! lives in a thread-local cell because the plugin structs are unit structs.

use std::cell::RefCell;

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, EditorOpenSpec, EditorPlacement, Event, FsPath, Intent,
    InvokeCommandSpec, MessageEvent, MessageSpec, MessageTarget, OpenViewSpec, PluginId,
    PluginSide, Submission, UiAction, ViewTarget,
};
use serde_json::{json, Value};

use crate::server::{PLUGIN_ID, QUERY_CHANNEL, RESULTS_CHANNEL};

/// The id of the panel the manifest declares.
pub const PANEL_ID: &str = "search";
/// The command that opens the panel.
pub const OPEN_COMMAND: &str = "ilium.core.search/open";
/// The kernel command that focuses a node; its argument is `{ "node": <id> }`.
const FOCUS_NODE_COMMAND: &str = "kernel.focus-node";
/// Correlation of the submission that opens the panel.
const OPEN_CORRELATION: u64 = 1;
/// Correlation of the submission that closes the panel after a jump.
const CLOSE_CORRELATION: u64 = 2;
/// The built-in groups in the order the panel shows them.
pub const BUILT_IN_KINDS: [&str; 4] = ["agent", "terminal", "file", "meaning"];

/// One result as the server sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub score: Option<f64>,
    pub before: String,
    pub matched: String,
    pub after: String,
    pub line: Option<u32>,
    pub last_command: Option<String>,
    pub pane: Option<String>,
}

impl Hit {
    fn parse(value: &Value) -> Option<Hit> {
        let id = value["id"].as_str()?.to_string();
        let kind = value["kind"].as_str().unwrap_or("file").to_string();
        let title = value["title"].as_str().unwrap_or(&id).to_string();
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
        Some(Hit {
            score: value["score"].as_f64(),
            before: text("before"),
            matched: text("matched"),
            after: text("after"),
            line: value["line"]
                .as_u64()
                .and_then(|line| u32::try_from(line).ok()),
            last_command: value["last_command"].as_str().map(str::to_string),
            pane: value["pane"].as_str().map(str::to_string),
            id,
            kind,
            title,
        })
    }

    /// True when the hit carries the text around a match.
    pub fn has_context(&self) -> bool {
        !self.matched.is_empty()
    }

    /// The text around the match on one line.
    pub fn context_line(&self) -> String {
        format!("{}{}{}", self.before, self.matched, self.after)
    }
}

/// What the panel shows and remembers.
#[derive(Debug, Default)]
pub struct PanelState {
    pub open: bool,
    pub draft: String,
    last_sent: Option<String>,
    revision: u64,
    pub pending: bool,
    pub answered: bool,
    /// Hits in display order (grouped), see `group_rank`.
    pub hits: Vec<Hit>,
    pub selected: usize,
}

thread_local! {
    static STATE: RefCell<PanelState> = RefCell::new(PanelState::default());
}

/// Forgets everything; called on activation.
pub fn reset() {
    STATE.with(|state| *state.borrow_mut() = PanelState::default());
}

/// Runs `read` on the panel state.
pub fn with_state<R>(read: impl FnOnce(&PanelState) -> R) -> R {
    STATE.with(|state| read(&state.borrow()))
}

fn panel_target() -> ViewTarget {
    ViewTarget::Panel(PANEL_ID.to_string())
}

/// Position of a kind in the display order: the built-in groups first, other kinds after.
fn group_rank(kind: &str) -> usize {
    BUILT_IN_KINDS
        .iter()
        .position(|built_in| *built_in == kind)
        .unwrap_or(BUILT_IN_KINDS.len())
}

impl PanelState {
    /// The id the result list uses for the hit at `index`: the hit id, made unique by a suffix
    /// when an earlier hit has the same id (a file with several matches).
    pub fn item_id(&self, index: usize) -> String {
        let hit = &self.hits[index];
        let earlier = self.hits[..index].iter().any(|other| other.id == hit.id);
        if earlier {
            format!("{}\u{1f}{index}", hit.id)
        } else {
            hit.id.clone()
        }
    }

    fn index_of_item(&self, item: &str) -> Option<usize> {
        (0..self.hits.len()).find(|index| self.item_id(*index) == item)
    }

    /// The revision an answer must carry to be shown: the newest one sent, or 1 while the
    /// first query is still only a typed draft.
    fn expected_revision(&self) -> Option<u64> {
        if self.revision > 0 {
            Some(self.revision)
        } else if !self.draft.is_empty() {
            Some(1)
        } else {
            None
        }
    }

    fn on_open(&mut self) {
        *self = PanelState {
            open: true,
            ..PanelState::default()
        };
    }

    fn on_closed(&mut self) {
        self.open = false;
        self.pending = false;
    }

    fn on_filter_changed(&mut self, text: &str) -> Option<Submission> {
        self.draft = text.to_string();
        let changed = match &self.last_sent {
            Some(last) => last != text,
            None => !text.is_empty(),
        };
        if !changed {
            return None;
        }
        self.revision += 1;
        self.last_sent = Some(text.to_string());
        self.pending = true;
        self.answered = false;
        self.hits.clear();
        self.selected = 0;
        let payload = json!({ "revision": self.revision, "query": text })
            .to_string()
            .into_bytes();
        Some(Submission::new(Intent::Message(MessageSpec {
            to: MessageTarget {
                plugin: PluginId(PLUGIN_ID.to_string()),
                side: PluginSide::Server,
                client: None,
            },
            channel: QUERY_CHANNEL.to_string(),
            payload,
        })))
    }

    fn on_results(&mut self, message: &MessageEvent) {
        if !self.open {
            return;
        }
        let Ok(payload) = serde_json::from_slice::<Value>(&message.payload) else {
            return;
        };
        let Some(revision) = payload["revision"].as_u64() else {
            return;
        };
        if self.expected_revision() != Some(revision) {
            return;
        }
        if self.revision == 0 {
            // The answer to a query the panel typed but never sent as a separate revision:
            // it counts as revision 1, so the next settled text is revision 2.
            self.revision = revision;
            self.last_sent = Some(self.draft.clone());
        }
        let mut hits: Vec<Hit> = payload["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Hit::parse)
            .collect();
        // A stable sort keeps the server's order inside every group; sources of other kinds
        // follow the built-in groups, one group per kind.
        hits.sort_by_key(|hit| {
            let rank = group_rank(&hit.kind);
            let custom = if rank == BUILT_IN_KINDS.len() {
                hit.kind.clone()
            } else {
                String::new()
            };
            (rank, custom)
        });
        self.hits = hits;
        self.selected = 0;
        self.pending = false;
        self.answered = true;
    }

    /// Jumps to the hit at `index` and closes the panel.
    fn activate(&mut self, index: usize) -> Vec<Submission> {
        let Some(hit) = self.hits.get(index).cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match hit.kind.as_str() {
            "agent" | "terminal" | "meaning" => {
                if let Ok(node) = hit.id.parse::<u64>() {
                    out.push(Submission::new(Intent::InvokeCommand(InvokeCommandSpec {
                        command: FOCUS_NODE_COMMAND.to_string(),
                        arguments_json: json!({ "node": node }).to_string(),
                    })));
                }
            }
            "file" => out.push(Submission::new(Intent::EditorOpen(EditorOpenSpec {
                path: FsPath {
                    scope: "project".to_string(),
                    path: hit.id.clone(),
                },
                line: hit.line,
                placement: EditorPlacement::NewPane,
            }))),
            _ => {}
        }
        out.push(Submission {
            intent: Intent::CloseView(panel_target()),
            correlation: CLOSE_CORRELATION,
            dedup_key: None,
        });
        self.on_closed();
        out
    }

    fn on_action(&mut self, action: &UiAction) -> Vec<Submission> {
        if action.view != panel_target() {
            return Vec::new();
        }
        if action.kind == ActionKind::Closed {
            self.on_closed();
            return Vec::new();
        }
        if !self.open {
            return Vec::new();
        }
        match (action.key.as_str(), action.kind, &action.value) {
            ("query", ActionKind::Changed, Some(ActionValue::Text(text))) => {
                self.draft = text.clone();
                Vec::new()
            }
            ("query", ActionKind::FilterChanged, Some(ActionValue::Text(text))) => {
                self.on_filter_changed(text).into_iter().collect()
            }
            ("hits", ActionKind::Activated, Some(ActionValue::Index(index))) => {
                self.activate(*index as usize)
            }
            ("hits", ActionKind::Selected, Some(ActionValue::Index(index))) => {
                if (*index as usize) < self.hits.len() {
                    self.selected = *index as usize;
                }
                Vec::new()
            }
            ("results", ActionKind::Activated, Some(ActionValue::Text(item))) => {
                match self.index_of_item(item) {
                    Some(index) => self.activate(index),
                    None => Vec::new(),
                }
            }
            ("results", ActionKind::Selected, Some(ActionValue::Text(item))) => {
                if let Some(index) = self.index_of_item(item) {
                    self.selected = index;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

/// Handles one batch of events.
pub fn on_events(batch: Vec<Event>) -> Vec<Submission> {
    let mut out = Vec::new();
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        for event in batch {
            match event {
                Event::CommandInvoked(invoked) if invoked.command == OPEN_COMMAND => {
                    state.on_open();
                    out.push(Submission {
                        intent: Intent::OpenView(OpenViewSpec {
                            view: panel_target(),
                            focus: true,
                        }),
                        correlation: OPEN_CORRELATION,
                        dedup_key: None,
                    });
                }
                Event::UiAction(action) => out.extend(state.on_action(&action)),
                Event::Message(message) if message.channel == RESULTS_CHANNEL => {
                    state.on_results(&message);
                }
                _ => {}
            }
        }
    });
    out
}
