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

use crate::agent_debug::AgentDebugRecorder;
use crate::config::{DetectionConfig, NotificationsConfig};
use crate::pane::PaneResource;
use crate::persistence::SessionSnapshot;
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
    pub agent_debug_menu_enabled: bool,
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
    /// Server-owned, pane-keyed semantic debug history. It is separate from
    /// the hot tree snapshot transported after structural changes.
    pub agent_debug: AgentDebugRecorder,
    /// Most recently selected terminal launch directory in this session.
    pub last_terminal_working_directory: Mutex<Option<PathBuf>>,
    pub pending_session_recovery: Mutex<Option<SessionSnapshot>>,
    /// One pre-restructure snapshot per project. A project-scoped revert
    /// restores only that project's subtree, leaving concurrent work in
    /// every other project intact.
    pub restructure_undo: Mutex<HashMap<NodeId, Tree>>,
    pub snapshot_write_lock: Mutex<()>,
    /// Serializes schedule replacement with the executor's final freshness
    /// check and PTY write. Lock ordering is this mutex, then `tree`, then
    /// `panes`; no other workflow acquires it, so a replaced timer cannot fire
    /// after its replacement was accepted.
    pub scheduled_input_transaction: Mutex<()>,
    /// Serializes enqueue/clear mutations with completion-driven delivery so
    /// a successful PTY write advances exactly the FIFO head it observed.
    pub prompt_queue_transaction: Mutex<()>,
    /// `true` when the on-disk crash-recovery snapshot no longer matches
    /// `tree`/`panes` and needs rewriting. Request handlers
    /// (`crate::ipc::handlers`) set this via `request_snapshot_save`
    /// instead of writing to disk inline on the request path (see
    /// `crate::persistence::spawn_snapshot_writer`); the background writer
    /// consumes it with an atomic swap so a request that arrives mid-write
    /// is never lost, only coalesced into the next pass.
    pub snapshot_dirty: AtomicBool,
    /// Set once by `ipc::handlers::handle_kill_session` and never cleared.
    /// `crate::run`'s shutdown grace period (see `SHUTDOWN_GRACE_PERIOD`)
    /// deliberately keeps other attached connections alive for a short
    /// window after one connection's `KillSession` request has already
    /// reset `tree`/`panes` to empty and deleted the on-disk snapshot --
    /// so, without this flag, a second connection's ordinary mutation
    /// (e.g. `NewPane`) landing in that same window would call
    /// `request_snapshot_save` again and resurrect a snapshot file for a
    /// session `handle_kill_session` already decided has nothing worth
    /// recovering. Checked by `request_snapshot_save` itself -- the one
    /// chokepoint every snapshot-dirtying call site already goes through
    /// -- rather than by every individual mutation handler.
    pub session_killed: AtomicBool,
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
    /// Wakes the deadline-driven detection loop when a new pane or a user
    /// interaction pulls a pane's next check earlier than its current sleep.
    pub detection_schedule_changed: Notify,
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
        let mut tree = Tree::new();
        // A brand-new session always starts with the launch directory as its
        // first top-level project; the server's `session_cwd` remains the
        // host for session-wide socket/log/snapshot state.
        tree.ensure_launch_project(options.session_cwd.clone())
            .expect("a new tree always accepts its launch project");
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
            tree: RwLock::new(tree),
            panes: RwLock::new(HashMap::new()),
            agent_debug: AgentDebugRecorder::new(options.agent_debug_menu_enabled),
            last_terminal_working_directory: Mutex::new(None),
            pending_session_recovery: Mutex::new(None),
            restructure_undo: Mutex::new(HashMap::new()),
            snapshot_write_lock: Mutex::new(()),
            scheduled_input_transaction: Mutex::new(()),
            prompt_queue_transaction: Mutex::new(()),
            snapshot_dirty: AtomicBool::new(false),
            session_killed: AtomicBool::new(false),
            snapshot_requested: Notify::new(),
            scheduled_input_changed: Notify::new(),
            detection_schedule_changed: Notify::new(),
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

    /// Aborts every tracked connection task. Called explicitly, once, on
    /// `run`'s own clean-shutdown path; also invoked automatically by
    /// `Drop` below as a safety net for the abnormal path where this
    /// `ServerState` is torn down without `run` ever reaching that code
    /// (see `crate::task_guard`'s module doc for the matching guard on the
    /// background tasks that otherwise keep this state's `Arc` alive).
    ///
    /// Recovers from a poisoned lock instead of unwrapping it: this is the
    /// body of `Drop::drop` below, which runs on exactly the abnormal
    /// teardown path where some other holder of this lock may have
    /// panicked -- unwrapping here would panic *again* while already
    /// unwinding, which aborts the whole process instead of letting this
    /// safety net do its one job. A `Vec<JoinHandle<()>>` behind a poisoned
    /// lock is never left in a logically invalid state (no invariant spans
    /// more than one `Vec` method call), so recovering the guard and
    /// proceeding to abort every handle is safe.
    pub fn abort_all_connection_tasks(&self) {
        let tasks = self
            .connection_tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    ///
    /// A no-op once `session_killed` is set: a killed session must never
    /// get a snapshot written for it again, no matter which still-attached
    /// connection or background task calls this afterward (see that
    /// field's doc comment).
    ///
    /// Re-checks `session_killed` after marking dirty, and undoes the mark
    /// if it lands true: `session_killed` and `snapshot_dirty` are two
    /// separate atomics with no lock spanning both, so `handle_kill_session`
    /// (mark killed, then clear dirty) can interleave with this function's
    /// own two steps. Without the re-check, a `session_killed` load here
    /// that lands just before `handle_kill_session`'s store, followed by
    /// this function's `snapshot_dirty` store landing just *after*
    /// `handle_kill_session`'s own `snapshot_dirty = false`, would leave the
    /// flag stuck dirty for the killed session -- exactly the resurrected
    /// snapshot this no-op exists to prevent. The re-check closes that
    /// window: whichever of the two threads' `session_killed` store or load
    /// actually happens last, the losing side's view of `snapshot_dirty` is
    /// reconciled back to `false` by the check below the same call it
    /// happened in.
    pub fn request_snapshot_save(&self) {
        if self.session_killed.load(Ordering::Acquire) {
            return;
        }
        self.snapshot_dirty.store(true, Ordering::Release);
        if self.session_killed.load(Ordering::Acquire) {
            self.snapshot_dirty.store(false, Ordering::Release);
            return;
        }
        self.snapshot_requested.notify_one();
    }

    /// Drops every `restructure_undo` entry whose project no longer exists
    /// in the live tree. A project's undo slot is only ever consumed by a
    /// successful revert (`handle_revert_project_restructure`); nothing else
    /// removes it, so a project closed via `ClosePane` (or discarded by a
    /// session-recovery overwrite) would otherwise leave its full pre-
    /// restructure `Tree` clone dangling in the map for the rest of the
    /// server process's life. `NodeId`s are never reused (see
    /// `ilium_core::Tree`'s id allocator), so a stale key can never
    /// accidentally collide with a live project and get pruned by mistake.
    ///
    /// Takes its own fresh `tree` read lock rather than a caller-supplied
    /// snapshot: every `restructure_undo` insert happens strictly after the
    /// tree write lock that committed the corresponding project is dropped
    /// (see `handle_apply_project_restructure_plan`), so a project can never
    /// appear in `restructure_undo` before it appears in the live tree.
    /// Reading the live tree here, at whatever moment this call actually
    /// runs, therefore can never observe an inserted entry's project as
    /// missing -- reusing an already-cloned snapshot from moments earlier
    /// could, if a concurrent restructure's tree commit and undo-insert
    /// straddled that snapshot's read lock.
    ///
    /// Called from `broadcast_and_persist`, the shared tail of every
    /// structural-mutation handler, so every path that can remove a project
    /// -- present or future -- is covered without each handler needing to
    /// remember this map exists.
    pub async fn prune_stale_restructure_undo(&self) {
        let tree = self.tree.read().await;
        let mut undo = self.restructure_undo.lock().await;
        undo.retain(|project_id, _| tree.get(*project_id).is_some());
    }
}

impl Drop for ServerState {
    /// Last-resort cleanup for every still-tracked per-connection task.
    /// `run`'s own `select!` cleanup already calls
    /// [`ServerState::abort_all_connection_tasks`] on a clean shutdown, at
    /// which point this state still has other `Arc` owners alive and this
    /// `Drop` does not run yet; it only fires once the very last
    /// `Arc<ServerState>` owner goes away. That only happens on the
    /// abnormal path this exists to guard: `run`'s own task cancelled from
    /// the outside before it reaches its normal cleanup, whose background
    /// tasks (detection loop, scheduled-input executor, snapshot writer,
    /// sound actor, config watcher) are each the *other* `Arc` owners --
    /// once `crate::task_guard::AbortOnDropHandle` stops every one of
    /// those on `run`'s own cancellation, their `Arc<ServerState>` clones
    /// release, and this `Drop` is what finally reaches the one resource
    /// (`connection_tasks`) that was never one of `run`'s own local
    /// variables and so could not be covered by that same guard.
    fn drop(&mut self) {
        self.abort_all_connection_tasks();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::config::{DetectionConfig, NotificationsConfig};
    use crate::NoopSoundPlayer;

    use super::*;

    fn test_state(directory: &tempfile::TempDir) -> ServerState {
        let (sound_requests, _playback_task) = crate::sounds::spawn(Arc::new(NoopSoundPlayer));
        ServerState::new(ServerStateOptions {
            session_name: "state-test".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("state-test.snapshot.json"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        })
    }

    /// Reproduces the `KillSession` grace-period race
    /// `ServerState::session_killed` exists to close: once a session is
    /// marked killed, `request_snapshot_save` must stay a no-op regardless
    /// of how many more times a still-attached connection calls it during
    /// `crate::run`'s shutdown grace period -- otherwise a mutation that
    /// slips in during that window would resurrect a snapshot for a
    /// session `ipc::handlers::handle_kill_session` already decided has
    /// nothing worth recovering.
    #[tokio::test]
    async fn request_snapshot_save_is_a_no_op_once_the_session_is_marked_killed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = test_state(&directory);

        state.request_snapshot_save();
        assert!(
            state.snapshot_dirty.load(Ordering::Acquire),
            "an ordinary request must still be able to dirty the snapshot before any kill"
        );

        // Mirrors `handle_kill_session`'s own ordering: mark killed, then
        // clear the flag it may have just set.
        state.session_killed.store(true, Ordering::Release);
        state.snapshot_dirty.store(false, Ordering::Release);

        // Stands in for a second, still-attached connection's ordinary
        // mutation (e.g. `NewPane`) landing during the shutdown grace
        // period after this session was killed.
        state.request_snapshot_save();

        assert!(
            !state.snapshot_dirty.load(Ordering::Acquire),
            "request_snapshot_save must not re-dirty the snapshot for an already-killed session"
        );
    }

    /// Stress-reproduces the same race with a genuinely concurrent
    /// `request_snapshot_save` and kill sequence on real OS threads, rather
    /// than the hand-sequenced interleaving above: `session_killed` and
    /// `snapshot_dirty` are two independent atomics with no lock spanning
    /// both, so `handle_kill_session`'s own two-step sequence (mark killed,
    /// then clear dirty) can land at any point relative to
    /// `request_snapshot_save`'s two steps (check killed, then mark dirty).
    /// Whichever interleaving occurs, the killer thread's own
    /// `snapshot_dirty = false` is always the last write to actually
    /// persist unless `request_snapshot_save` observes the kill and undoes
    /// its own mark -- run enough iterations under `loom`-style repetition
    /// to make a regression in that re-check reliably observable rather
    /// than relying on a single lucky (or unlucky) scheduling.
    #[tokio::test]
    async fn request_snapshot_save_never_leaves_dirty_set_after_a_concurrent_kill() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = Arc::new(test_state(&directory));

        for _ in 0..1_000 {
            state.snapshot_dirty.store(false, Ordering::Release);
            state.session_killed.store(false, Ordering::Release);

            let saver = Arc::clone(&state);
            let saver_thread = std::thread::spawn(move || saver.request_snapshot_save());

            let killer = Arc::clone(&state);
            let killer_thread = std::thread::spawn(move || {
                killer.session_killed.store(true, Ordering::Release);
                killer.snapshot_dirty.store(false, Ordering::Release);
            });

            saver_thread.join().expect("saver thread must not panic");
            killer_thread.join().expect("killer thread must not panic");

            assert!(
                !state.snapshot_dirty.load(Ordering::Acquire),
                "a concurrent kill must never leave the dirty flag stuck true"
            );
        }
    }
}
