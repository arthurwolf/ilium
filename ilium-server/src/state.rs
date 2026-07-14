//! `ServerState`: the single source of truth one running `ilium-server`
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
use std::sync::atomic::{AtomicBool, Ordering};

use ilium_core::{NodeId, Tree};
use ilium_detect::AgentSignature;
use ilium_ipc::ServerEvent;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::config::{DetectionConfig, NotificationsConfig};
use crate::pane::PaneResource;
use crate::sounds::PlaybackRequest;

/// Capacity of the per-session broadcast channel. Sized generously for
/// terminal output bursts (a `cat` of a large file can emit many
/// `ScreenUpdate` chunks in a tight loop); a client that falls behind by
/// more than this gets `RecvError::Lagged` rather than blocking every
/// other client or unboundedly growing memory -- see
/// `ilium_pty::PtySession`'s own broadcast channel for the identical
/// tradeoff at the pty layer.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

pub type PaneRegistry = HashMap<NodeId, PaneResource>;

/// Construction-time values for one project-session server. Keeping this as
/// one explicit contract prevents positional path/config arguments from being
/// swapped as the state gains another project-scoped dependency.
pub struct ServerStateOptions {
    pub session_name: String,
    pub session_cwd: PathBuf,
    pub home_dir: PathBuf,
    pub snapshot_path: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub sound_settings: ilium_sound::SoundSettings,
    pub sound_requests: tokio::sync::mpsc::Sender<PlaybackRequest>,
    pub custom_signatures: Vec<AgentSignature>,
}

pub struct ServerState {
    pub session_name: String,
    /// Canonical project boundary shared by every pane in this server.
    pub session_cwd: PathBuf,
    /// Home containing the local built-in provider transcript stores.
    pub home_dir: PathBuf,
    pub snapshot_path: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub sound_settings: RwLock<ilium_sound::SoundSettings>,
    pub sound_requests: tokio::sync::mpsc::Sender<PlaybackRequest>,
    /// User-configured agent signatures checked alongside `ilium-detect`'s
    /// built-in registry on every detection-loop tick (see
    /// `ilium_detect::identify_agent_with_extra`). Never mutated after
    /// construction -- there is no "reload config" request yet.
    pub custom_signatures: Vec<AgentSignature>,
    pub tree: RwLock<Tree>,
    pub panes: RwLock<PaneRegistry>,
    pub snapshot_write_lock: Mutex<()>,
    /// Serializes schedule replacement with the executor's final freshness
    /// check and PTY write. Lock ordering is this mutex, then `tree`, then
    /// `panes`; no other workflow acquires it, so a replaced timer cannot fire
    /// after its replacement was accepted.
    pub scheduled_input_transaction: Mutex<()>,
    /// `true` when the on-disk crash-recovery snapshot no longer matches
    /// `tree`/`panes` and needs rewriting. Request handlers
    /// (`crate::ipc::handlers`) set this via `request_snapshot_save`
    /// instead of writing to disk inline on the request path (see
    /// `crate::persistence::spawn_snapshot_writer`); the background writer
    /// consumes it with an atomic swap so a request that arrives mid-write
    /// is never lost, only coalesced into the next pass.
    pub snapshot_dirty: AtomicBool,
    /// Wakes `crate::persistence::spawn_snapshot_writer`'s background
    /// task. `Notify` only ever stores a single outstanding permit, which
    /// is exactly the coalescing behavior wanted here: any number of
    /// `request_snapshot_save` calls that land faster than the writer's
    /// debounce window collapse into one wakeup.
    pub snapshot_requested: Notify,
    /// Wakes the single scheduled-input executor whenever the nearest
    /// deadline may have changed (new schedule, replacement, or pane close).
    /// One coalesced permit is sufficient because the executor always scans
    /// the authoritative tree again before deciding what to wait for.
    pub scheduled_input_changed: Notify,
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
    pub fn new(options: ServerStateOptions) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            session_name: options.session_name,
            session_cwd: options.session_cwd,
            home_dir: options.home_dir,
            snapshot_path: options.snapshot_path,
            detection_config: options.detection_config,
            notifications_config: options.notifications_config,
            sound_settings: RwLock::new(options.sound_settings),
            sound_requests: options.sound_requests,
            custom_signatures: options.custom_signatures,
            tree: RwLock::new(Tree::new()),
            panes: RwLock::new(HashMap::new()),
            snapshot_write_lock: Mutex::new(()),
            scheduled_input_transaction: Mutex::new(()),
            snapshot_dirty: AtomicBool::new(false),
            snapshot_requested: Notify::new(),
            scheduled_input_changed: Notify::new(),
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
        // for the whole server, consistent with how `ilium-pty` treats
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

    /// Marks the crash-recovery snapshot dirty and wakes the background
    /// debounced writer (`crate::persistence::spawn_snapshot_writer`).
    /// Cheap and non-blocking -- call sites are request handlers that must
    /// never `.await` a full disk write inline on the request path (see
    /// `crate::persistence` module docs).
    pub fn request_snapshot_save(&self) {
        self.snapshot_dirty.store(true, Ordering::Release);
        self.snapshot_requested.notify_one();
    }
}
