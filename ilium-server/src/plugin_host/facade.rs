//! The server's kernel facade for the plugin host (foundation contract sections 2.3 and 12).
//!
//! Every method here runs on the plugin host's own thread, which is not a tokio worker, so the
//! facade may block: tree reads take the lock with `blocking_read`, and an intent is applied by
//! handing the equivalent `ClientRequest` to the same `handle_request` path an attached client
//! uses, driven through the server's tokio handle. A plugin therefore gets exactly the
//! validation, snapshot scheduling and broadcasts a user action gets, and nothing in this file
//! owns or touches a PTY: a trap or a stalled plugin stops here (COR-218).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ilium_core::{NodeKind, PaneContentKind, SplitOrientation, Tree};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, PaneResizeCause, ServerEvent,
};
use ilium_plugin_api::generated::types::events::MessageEvent;
use ilium_plugin_api::generated::types::intents::{
    CreatePaneSpec, ExpandTarget, MessageSpec, MoveNodeSpec,
};
use ilium_plugin_api::generated::types::tree::NodeClass;
use ilium_plugin_api::generated::types::{
    ErrorCode, Event, Intent, NodeId, Outcome, Placement, PluginError, PluginSide, SplitDirection,
};
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead, WorkspaceAccess,
};
use ilium_plugin_api::platform::ProcessKind;
use ilium_plugin_api::side::Side;
use ilium_plugin_host::imports::basic::{BasicBackend, BasicChange};
use ilium_plugin_host::runtime::RuntimeThread;

use crate::ipc::handlers::handle_request;
use crate::ipc::DirectEventSender;
use crate::state::ServerState;

/// How long one translated request may take before the plugin sees `timed-out`. Pane creation
/// includes spawning the process, so this is far above an ordinary tree change.
const REQUEST_BUDGET: Duration = Duration::from_secs(30);
/// How long one screen read may take; the reader itself bounds a resize in flight.
const SCREEN_BUDGET: Duration = Duration::from_secs(3);
/// Replies one translated request can queue before the handler waits for the drain.
const REPLY_CAPACITY: usize = 16;

/// Content kind names of the built-in pane kinds (`NodeView::content_kind`).
pub(crate) const TERMINAL_KIND: &str = "ilium.terminal";
const EDITOR_KIND: &str = "ilium.editor";
const BOARD_KIND: &str = "ilium.board";

/// Where the runtime thread lives. The facade posts to it (a server-side message), the host
/// handle takes it out at shutdown; nobody blocks on the runtime while holding this lock.
pub(crate) type RuntimeSlot = Arc<Mutex<Option<RuntimeThread>>>;

/// The kernel as server-side plugins see it.
pub(crate) struct ServerFacade {
    // Weak: the state owns the host handle, which owns the runtime, which owns this facade.
    state: Weak<ServerState>,
    tokio: tokio::runtime::Handle,
    runtime: RuntimeSlot,
    epoch: Arc<AtomicU64>,
    next_request: AtomicU64,
    next_envelope: Arc<AtomicU64>,
    project_dir: PathBuf,
    session_name: String,
}

fn to_core(node: NodeId) -> ilium_core::NodeId {
    ilium_core::NodeId(node.0)
}

fn to_plugin(node: ilium_core::NodeId) -> NodeId {
    NodeId(node.0)
}

fn refused(code: ErrorCode, message: impl Into<String>) -> Outcome {
    Outcome::Refused(PluginError::new(code, message.into()))
}

fn failed(code: ErrorCode, message: impl Into<String>) -> Outcome {
    Outcome::Failed(PluginError::new(code, message.into()))
}

fn unsupported(what: &str) -> Outcome {
    refused(
        ErrorCode::Unsupported,
        format!("the server kernel does not apply {what} for plugins yet"),
    )
}

/// One tree node as a plugin sees it.
fn node_view(tree: &Tree, id: ilium_core::NodeId) -> Option<TreeNode> {
    let node = tree.get(id)?;
    let (class, content_kind) = match &node.kind {
        NodeKind::Container(_) => (NodeClass::Container, None),
        NodeKind::Folder { .. } => (NodeClass::Folder, None),
        NodeKind::Pane { content, .. } => (
            NodeClass::Pane,
            Some(
                match content {
                    PaneContentKind::Terminal => TERMINAL_KIND,
                    PaneContentKind::Editor => EDITOR_KIND,
                    PaneContentKind::Board => BOARD_KIND,
                }
                .to_owned(),
            ),
        ),
    };
    Some(TreeNode {
        id: to_plugin(id),
        parent: node.parent.map(to_plugin),
        class,
        content_kind,
        title: node.name.clone(),
        cwd: tree
            .pane_cwd(id)
            .map(|path| path.to_string_lossy().into_owned()),
        // Agent identity reaches plugins through the agents accessor, which is not wired yet
        // (foundation contract section 12.4).
        agent: None,
        // `None` for a pane, which has no closed state to lock.
        locked: node.is_locked_closed().unwrap_or(false),
        revision: node.presentation_revision,
    })
}

/// A POSIX single-quoted word, so an argument vector survives the shell that runs a pane's
/// command line.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// What a translated request produced.
enum RequestEnd {
    Done,
    Created(ilium_core::NodeId),
    Failed(String),
    TimedOut,
}

impl ServerFacade {
    pub(crate) fn new(
        state: &Arc<ServerState>,
        tokio: tokio::runtime::Handle,
        runtime: RuntimeSlot,
        epoch: Arc<AtomicU64>,
        next_envelope: Arc<AtomicU64>,
    ) -> Self {
        Self {
            state: Arc::downgrade(state),
            tokio,
            runtime,
            epoch,
            next_request: AtomicU64::new(1),
            next_envelope,
            project_dir: state.session_cwd.clone(),
            session_name: state.session_name.clone(),
        }
    }

    fn with_tree<R>(&self, read: impl FnOnce(&Tree) -> R) -> Option<R> {
        let state = self.state.upgrade()?;
        // The host thread is not a tokio worker, so the blocking lock is allowed here.
        let tree = state.tree.blocking_read();
        Some(read(&tree))
    }

    /// Runs one client request through the ordinary handler and reads its direct replies.
    /// `created` is the request id of a pane creation whose finish event carries the node.
    fn request(&self, request: ClientRequest, created: Option<u64>) -> RequestEnd {
        let Some(state) = self.state.upgrade() else {
            return RequestEnd::Failed("the server is shutting down".to_owned());
        };
        let Some(execution) = state.execution.get() else {
            return RequestEnd::Failed("the server execution bank is not started".to_owned());
        };
        let (sender, mut receiver) =
            DirectEventSender::admitted_channel(REPLY_CAPACITY, execution.client.clone());
        let work = async {
            let handler = async {
                let sender = sender;
                let _ = handle_request(&state, request, &sender).await;
                // Dropping the last sender ends the drain below for requests that reply inline.
            };
            let drain = async {
                let mut end = RequestEnd::Done;
                while let Some(queued) = receiver.recv_queued().await {
                    match queued.event {
                        ServerEvent::Error { message } => {
                            if matches!(end, RequestEnd::Done) {
                                end = RequestEnd::Failed(message);
                            }
                        }
                        ServerEvent::PaneCreateFinished { request_id, result }
                            if Some(request_id) == created =>
                        {
                            return match result {
                                Ok(node) => RequestEnd::Created(node),
                                Err(message) => RequestEnd::Failed(message),
                            };
                        }
                        _ => {}
                    }
                }
                end
            };
            let ((), end) = tokio::join!(handler, drain);
            end
        };
        self.tokio.block_on(async {
            match tokio::time::timeout(REQUEST_BUDGET, work).await {
                Ok(end) => end,
                Err(_) => RequestEnd::TimedOut,
            }
        })
    }

    fn simple(&self, request: ClientRequest) -> Outcome {
        match self.request(request, None) {
            RequestEnd::Done | RequestEnd::Created(_) => Outcome::Ok,
            RequestEnd::Failed(message) => failed(ErrorCode::Internal, message),
            RequestEnd::TimedOut => failed(
                ErrorCode::TimedOut,
                "the server did not finish the request in time",
            ),
        }
    }

    fn create_pane(&self, spec: CreatePaneSpec) -> Outcome {
        if !spec.content_kind.is_empty() && spec.content_kind != TERMINAL_KIND {
            return unsupported("create-pane for a content kind other than ilium.terminal");
        }
        let working_directory = match &spec.cwd {
            None => NewPaneWorkingDirectory::ProjectRoot,
            Some(path) if path.scope == "project" => {
                // The handler canonicalizes the directory and refuses a missing one.
                NewPaneWorkingDirectory::Directory(self.project_dir.join(&path.path))
            }
            Some(path) => {
                return refused(
                    ErrorCode::Unsupported,
                    format!("a pane cannot start in path scope {:?}", path.scope),
                );
            }
        };
        let command_line = spec.command.as_ref().map(|words| {
            words
                .iter()
                .map(|word| shell_word(word))
                .collect::<Vec<_>>()
                .join(" ")
        });
        let kind = match (command_line, spec.initial_input.clone()) {
            (None, None) => NewPaneKind::PlainShell,
            (Some(command_line), None) => NewPaneKind::Command(command_line),
            (Some(command_line), Some(initial_input)) => NewPaneKind::CommandWithInitialInput {
                command_line,
                initial_input,
            },
            (None, Some(_)) => {
                return refused(ErrorCode::InvalidArgument, "initial-input needs a command");
            }
        };
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request = ClientRequest::NewPane {
            request_id,
            parent_group: spec.parent.map_or(ilium_core::ROOT_ID, to_core),
            kind,
            working_directory,
        };
        match self.request(request, Some(request_id)) {
            RequestEnd::Created(node) => {
                if !spec.title.is_empty() {
                    // A failed title is not a failed pane: the pane exists and is reported.
                    let _ = self.request(
                        ClientRequest::RenameNode {
                            node_id: node,
                            title: spec.title,
                            short_title: None,
                            inferred_icon: None,
                        },
                        None,
                    );
                }
                Outcome::OkNode(to_plugin(node))
            }
            RequestEnd::Done => failed(
                ErrorCode::Internal,
                "the server did not report the created pane",
            ),
            RequestEnd::Failed(message) => failed(ErrorCode::Internal, message),
            RequestEnd::TimedOut => failed(ErrorCode::TimedOut, "pane creation timed out"),
        }
    }

    fn move_node(&self, spec: MoveNodeSpec) -> Outcome {
        let node = to_core(spec.node);
        let target = to_core(spec.target);
        let place = self.with_tree(|tree| match spec.placement {
            Placement::InsideFirst => Some((target, Some(0))),
            Placement::InsideLast => Some((target, None)),
            Placement::Before | Placement::After => {
                let parent = tree.parent_of(target)?;
                let siblings = tree.children_of(parent).ok()?;
                let mut index = siblings.iter().position(|id| *id == target)?;
                // The moved node leaves its slot first when it already sits before the target
                // under the same parent.
                if tree.parent_of(node) == Some(parent)
                    && siblings.iter().position(|id| *id == node) < Some(index)
                {
                    index = index.saturating_sub(1);
                }
                if matches!(spec.placement, Placement::After) {
                    index += 1;
                }
                Some((parent, Some(index)))
            }
        });
        match place {
            Some(Some((new_parent, index))) => self.simple(ClientRequest::ReparentNode {
                node_id: node,
                new_parent,
                index,
            }),
            Some(None) => refused(ErrorCode::NotFound, "the move target has no parent"),
            None => failed(ErrorCode::Internal, "the server is shutting down"),
        }
    }

    fn message(&self, origin: &Origin, spec: MessageSpec) -> Outcome {
        let (from, from_side) = match origin {
            Origin::Plugin { plugin, side, .. } => (
                plugin.clone(),
                match side {
                    Side::Server => PluginSide::Server,
                    _ => PluginSide::Client,
                },
            ),
            _ => return refused(ErrorCode::NotPermitted, "only a plugin sends a message"),
        };
        if spec.to.plugin.0 != from.0 {
            // Plugin-to-plugin messages go through services and topics, not this intent.
            return refused(
                ErrorCode::NotPermitted,
                "a message intent reaches only the sender's own other side",
            );
        }
        match spec.to.side {
            PluginSide::Client => {
                let Some(state) = self.state.upgrade() else {
                    return failed(ErrorCode::Internal, "the server is shutting down");
                };
                state.broadcast(ServerEvent::Plugin(ilium_ipc::plugin::PluginEnvelope {
                    plugin: spec.to.plugin.0.clone(),
                    seq: self.next_envelope.fetch_add(1, Ordering::Relaxed),
                    body: ilium_ipc::plugin::EnvelopeBody::Message(
                        ilium_ipc::plugin::PluginMessage {
                            topic: spec.channel,
                            payload: spec.payload,
                        },
                    ),
                }));
                Outcome::Ok
            }
            PluginSide::Server => {
                let target = spec.to.plugin.0.clone();
                let event = Event::Message(MessageEvent {
                    from,
                    from_side,
                    from_client: None,
                    channel: spec.channel,
                    payload: spec.payload,
                });
                // Queued, never delivered inline: this call already runs inside the runtime.
                let posted = self.runtime.lock().ok().is_some_and(|slot| {
                    slot.as_ref().is_some_and(|thread| {
                        thread.post(move |runtime| {
                            if let Err(error) = runtime.deliver(&target, vec![event]) {
                                tracing::debug!("plugin message not delivered: {error}");
                            }
                            for error in runtime.pump() {
                                tracing::warn!("plugin host: {error}");
                            }
                        })
                    })
                });
                if posted {
                    Outcome::Ok
                } else {
                    failed(ErrorCode::Busy, "the plugin host is stopping")
                }
            }
        }
    }

    fn screen<R: Send>(
        &self,
        pane: NodeId,
        read: impl Fn(&vt100::Screen) -> R + Send,
    ) -> Option<R> {
        let state = self.state.upgrade()?;
        self.tokio.block_on(async {
            tokio::time::timeout(
                SCREEN_BUDGET,
                crate::pane::read_current_terminal_screen(&state, to_core(pane), read),
            )
            .await
            .ok()
            .flatten()
        })
    }
}

impl KernelFacade for ServerFacade {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        match intent {
            Intent::RenameNode(spec) => {
                let node = to_core(spec.node);
                if let Some(expected) = spec.expected_revision {
                    let current = self
                        .with_tree(|tree| tree.get(node).map(|node| node.presentation_revision))
                        .flatten();
                    match current {
                        None => return refused(ErrorCode::NotFound, "no such node"),
                        Some(current) if current != expected => {
                            return refused(
                                ErrorCode::Conflict,
                                "the node changed since the plugin read it",
                            );
                        }
                        Some(_) => {}
                    }
                }
                self.simple(ClientRequest::RenameNode {
                    node_id: node,
                    title: spec.title,
                    short_title: None,
                    inferred_icon: None,
                })
            }
            Intent::MoveNode(spec) => self.move_node(spec),
            Intent::CloseNode(node) => self.simple(ClientRequest::ClosePane {
                pane_id: to_core(node),
            }),
            Intent::SetNodeLocked(spec) => self.simple(ClientRequest::SetNodeLockedClosed {
                node_id: to_core(spec.node),
                locked_closed: spec.locked,
            }),
            Intent::CreatePane(spec) => self.create_pane(spec),
            Intent::ResizePane(spec) => {
                let current = self.panes().size(spec.pane);
                let columns = spec.columns.or(current.map(|size| size.0));
                let rows = spec.rows.or(current.map(|size| size.1));
                match (columns, rows) {
                    (Some(cols), Some(rows)) => self.simple(ClientRequest::ResizePane {
                        pane_id: to_core(spec.pane),
                        rows,
                        cols,
                        cause: PaneResizeCause::HostTerminal,
                    }),
                    _ => refused(ErrorCode::NotFound, "the pane has no terminal to resize"),
                }
            }
            Intent::CreateGroup(spec) => {
                if !spec.children.is_empty() {
                    return unsupported("create-group with initial children");
                }
                self.simple(ClientRequest::NewGroup {
                    parent_group: spec.parent.map_or(ilium_core::ROOT_ID, to_core),
                    name: spec.title,
                })
            }
            Intent::CreateSplit(spec) => {
                let Some(first) = spec.panes.first().copied() else {
                    return refused(ErrorCode::InvalidArgument, "a split needs panes");
                };
                let parent = self
                    .with_tree(|tree| tree.parent_of(to_core(first)))
                    .flatten()
                    .unwrap_or(ilium_core::ROOT_ID);
                let (name, orientation) = match spec.direction {
                    SplitDirection::Horizontal => {
                        ("Horizontal split", SplitOrientation::Horizontal)
                    }
                    SplitDirection::Vertical => ("Vertical split", SplitOrientation::Vertical),
                };
                self.simple(ClientRequest::CreateSplitView {
                    parent_group: parent,
                    name: name.to_owned(),
                    orientation,
                    pane_ids: spec.panes.into_iter().map(to_core).collect(),
                })
            }
            Intent::SetBookmarked(spec) => self.simple(ClientRequest::SetNodeBookmarked {
                node_id: to_core(spec.node),
                is_bookmarked: spec.bookmarked,
            }),
            Intent::SetExpanded(spec) => match spec.target {
                ExpandTarget::Node(node) => self.simple(ClientRequest::SetNodeExpanded {
                    node_id: to_core(node),
                    expanded: spec.expanded,
                }),
                ExpandTarget::VirtualRow(_) => unsupported("set-expanded on a virtual row"),
            },
            Intent::Message(spec) => self.message(origin, spec),
            // The remaining intents belong to features that are not ported yet; each port adds
            // its arm here together with its handler (foundation contract section 12.4).
            _ => unsupported("this intent"),
        }
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Server
    }

    fn workspace(&self) -> Option<&dyn WorkspaceAccess> {
        Some(self)
    }
}

impl TreeRead for ServerFacade {
    fn nodes(&self) -> Vec<TreeNode> {
        self.with_tree(|tree| {
            let mut ids: Vec<_> = tree.all_ids().collect();
            ids.sort_by_key(|id| id.0);
            ids.into_iter()
                .filter_map(|id| node_view(tree, id))
                .collect()
        })
        .unwrap_or_default()
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        self.with_tree(|tree| node_view(tree, to_core(id)))
            .flatten()
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

impl PaneRead for ServerFacade {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        self.screen(pane, |screen| screen.contents())
    }

    fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        // Styled cells and scrollback are served once the terminal primitive is ported
        // (foundation contract section 12.4).
        None
    }

    fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
        None
    }

    fn size(&self, pane: NodeId) -> Option<(u16, u16)> {
        self.screen(pane, |screen| {
            let (rows, columns) = screen.size();
            (columns, rows)
        })
    }
}

impl WorkspaceAccess for ServerFacade {
    fn project_dir(&self) -> Option<String> {
        Some(self.project_dir.to_string_lossy().into_owned())
    }

    fn session_name(&self) -> Option<String> {
        Some(self.session_name.clone())
    }
}

impl BasicBackend for ServerFacade {
    fn apply_basic(
        &self,
        _plugin: &str,
        pane: NodeId,
        change: BasicChange,
    ) -> Result<(), PluginError> {
        match change {
            BasicChange::Title(title) => {
                match self.request(
                    ClientRequest::RenameNode {
                        node_id: to_core(pane),
                        title,
                        short_title: None,
                        inferred_icon: None,
                    },
                    None,
                ) {
                    RequestEnd::Done | RequestEnd::Created(_) => Ok(()),
                    RequestEnd::Failed(message) => {
                        Err(PluginError::new(ErrorCode::Internal, message))
                    }
                    RequestEnd::TimedOut => Err(PluginError::new(
                        ErrorCode::TimedOut,
                        "the server did not finish the rename in time".to_owned(),
                    )),
                }
            }
            // Status text, focus, activity and the dirty mark need pane-kind state that the
            // primitive ports add (foundation contract section 12.4).
            _ => Err(PluginError::new(
                ErrorCode::Unsupported,
                "this basic pane change is not wired on the server yet".to_owned(),
            )),
        }
    }

    fn basic_size(&self, _plugin: &str, pane: NodeId) -> Option<(u16, u16)> {
        PaneRead::size(self, pane)
    }
}

#[cfg(test)]
mod tests {
    use super::shell_word;

    #[test]
    fn plain_words_stay_bare_and_others_are_single_quoted() {
        assert_eq!(shell_word("cargo"), "cargo");
        assert_eq!(shell_word("--flag=1"), "--flag=1");
        assert_eq!(shell_word("two words"), "'two words'");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
    }
}
