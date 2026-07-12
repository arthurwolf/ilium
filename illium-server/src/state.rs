//! `ServerState`: the single source of truth one running `illium-server`
//! process owns for the one session it serves (see `CLAUDE.md`: one UDS
//! socket, and therefore one server process, per session -- this is not a
//! multi-session registry).
//!
//! Lock ordering (must be followed everywhere in this crate to avoid
//! deadlock): **`tree` before `panes`**. Both are `tokio::sync::RwLock` so
//! neither can be held across an unrelated `.await` safely assumed away --
//! every call site that needs both takes `tree` first, does its
//! `panes`-locked work, and drops both before returning.

use std::collections::HashMap;
use std::path::PathBuf;

use illium_core::{NodeId, Tree};
use illium_ipc::ServerEvent;
use tokio::sync::{broadcast, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::config::{DetectionConfig, NotificationsConfig};
use crate::pane::PaneResource;

/// Capacity of the per-session broadcast channel. Sized generously for
/// terminal output bursts (a `cat` of a large file can emit many
/// `ScreenUpdate` chunks in a tight loop); a client that falls behind by
/// more than this gets `RecvError::Lagged` rather than blocking every
/// other client or unboundedly growing memory -- see
/// `illium_pty::PtySession`'s own broadcast channel for the identical
/// tradeoff at the pty layer.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

pub type PaneRegistry = HashMap<NodeId, PaneResource>;

pub struct ServerState {
    pub session_name: String,
    pub snapshot_path: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub tree: RwLock<Tree>,
    pub panes: RwLock<PaneRegistry>,
    /// Broadcast to every currently-attached client. Connection tasks each
    /// hold their own `subscribe()`d receiver; this crate never reads from
    /// this sender's own channel, only sends into it.
    pub events: broadcast::Sender<ServerEvent>,
    /// Signaled by the `KillSession` handler; `run`'s top-level select
    /// loop treats this as "stop accepting connections and exit."
    pub shutdown: Notify,
    /// Every spawned per-connection task's handle, so a `KillSession`
    /// shutdown can abort connections that are blocked reading with no
    /// request coming (an idle attached client) instead of leaking them.
    /// Pruned of already-finished handles on each insert rather than
    /// letting it grow unboundedly across a long-lived session with many
    /// short-lived connections.
    pub connection_tasks: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl ServerState {
    pub fn new(
        session_name: String,
        snapshot_path: PathBuf,
        detection_config: DetectionConfig,
        notifications_config: NotificationsConfig,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            session_name,
            snapshot_path,
            detection_config,
            notifications_config,
            tree: RwLock::new(Tree::new()),
            panes: RwLock::new(HashMap::new()),
            events,
            shutdown: Notify::new(),
            connection_tasks: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Registers a spawned connection task's handle for shutdown-time
    /// cancellation, first dropping any handles for connections that have
    /// already ended on their own.
    pub fn track_connection_task(&self, handle: JoinHandle<()>) {
        // Poisoned-lock panic would mean a prior holder panicked while
        // holding this uncontended, in-memory-only lock -- unrecoverable
        // for the whole server, consistent with how `illium-pty` treats
        // its own poisoned locks.
        let mut tasks = self.connection_tasks.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
    }

    /// Aborts every tracked connection task. Called once, at session
    /// shutdown.
    pub fn abort_all_connection_tasks(&self) {
        let tasks = self.connection_tasks.lock().unwrap();
        for task in tasks.iter() {
            task.abort();
        }
    }

    /// Best-effort broadcast: an `Err` here only ever means there are
    /// currently zero attached clients, which is a normal state (no
    /// terminal attached right now), not a failure worth logging.
    pub fn broadcast(&self, event: ServerEvent) {
        let _ = self.events.send(event);
    }
}
