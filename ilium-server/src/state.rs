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

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use ilium_core::{NodeId, Tree};
use ilium_detect::AgentSignature;
use ilium_ipc::ServerEvent;
use tokio::sync::{broadcast, watch, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::agent_debug::AgentDebugRecorder;
use crate::config::{DetectionConfig, NotificationsConfig};
use crate::pane::PaneResource;
use crate::persistence::SessionSnapshot;
use crate::snapshot_state::SnapshotState;
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

pub(crate) const MAXIMUM_CACHED_PROGRESS_SET_REQUESTS: usize = 512;
pub(crate) type ProgressSetResult =
    Result<ilium_ipc::ProgressMonitorAccepted, ilium_ipc::ProgressMonitorRejection>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProgressSetRequestIdentity {
    pub pane_id: NodeId,
    pub command: String,
    pub interval_seconds: u32,
    pub goal_policy: ilium_ipc::ProgressGoalPolicy,
}

#[derive(Debug, Clone)]
pub(crate) enum ProgressSetRequestOutcome {
    Pending(tokio::sync::watch::Sender<Option<ProgressSetResult>>),
    Complete(ProgressSetResult),
}

#[derive(Debug, Clone)]
pub(crate) struct ProgressSetRequestRecord {
    pub identity: ProgressSetRequestIdentity,
    pub outcome: ProgressSetRequestOutcome,
}

#[derive(Default)]
pub(crate) struct ProgressSetRequestCache {
    pub records: HashMap<u64, ProgressSetRequestRecord>,
    pub completed_order: VecDeque<u64>,
}

/// One accepted rule set and its execution generation. A queued delivery
/// belongs to the generation that matched it, even if settings later cycle
/// back to identical values.
#[derive(Default)]
pub struct VersionedTextTriggerSettings {
    pub settings: ilium_ipc::TextTriggerSettings,
    pub revision: u64,
}

#[derive(Default)]
struct TerminalSubscriptionCounts {
    all_panes: usize,
    panes: HashMap<NodeId, usize>,
}

/// Construction-time values for one project-session server. Keeping this as
/// one explicit contract prevents positional path/config arguments from being
/// swapped as the state gains another project-scoped dependency.
pub struct ServerStateOptions {
    pub session_name: String,
    pub session_cwd: PathBuf,
    pub home_dir: PathBuf,
    pub snapshot_path: PathBuf,
    /// This session's Unix domain socket path. Injected as
    /// `ILIUM_SESSION_SOCKET` into every spawned terminal pane's environment
    /// (see `crate::pane::spawn_terminal_session`) so a process running
    /// inside a pane -- e.g. the `ilium progress set` CLI subcommand -- can
    /// address this exact server without the caller needing to already know
    /// the session's runtime-directory layout.
    pub socket_path: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub sound_settings: ilium_sound::SoundSettings,
    pub sound_requests: tokio::sync::mpsc::Sender<PlaybackRequest>,
    pub custom_signatures: Vec<AgentSignature>,
    pub agent_debug_menu_enabled: bool,
    pub progress_monitor_enabled: bool,
}

pub struct ServerState {
    pub session_name: String,
    /// Canonical project boundary shared by every pane in this server.
    pub session_cwd: PathBuf,
    /// Home containing the local built-in provider transcript stores.
    pub home_dir: PathBuf,
    pub snapshot_path: PathBuf,
    pub socket_path: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub sound_settings: RwLock<ilium_sound::SoundSettings>,
    /// Last server-accepted Text Trigger configuration. Execution state is
    /// added separately so a rejected candidate never replaces this value.
    pub text_trigger_settings: RwLock<VersionedTextTriggerSettings>,
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
    pub snapshot_write_lock: std::sync::Arc<Mutex<()>>,
    /// Serializes schedule replacement with the executor's final freshness
    /// check and PTY write. Lock ordering is this mutex, then `tree`, then
    /// `panes`; no other workflow acquires it, so a replaced timer cannot fire
    /// after its replacement was accepted.
    pub scheduled_input_transaction: Mutex<()>,
    /// Serializes enqueue/clear mutations with completion-driven delivery so
    /// a successful PTY write advances exactly the FIFO head it observed.
    pub prompt_queue_transaction: Mutex<()>,
    /// Whether the on-disk crash-recovery snapshot still matches
    /// `tree`/`panes`, and whether this session wants one at all. Request
    /// Ordinary request handlers (`crate::ipc::handlers`) mark it through
    /// [`ServerState::request_snapshot_save`] instead of writing to disk
    /// inline on the request path (see
    /// `crate::persistence::spawn_snapshot_writer`). Explicit durability
    /// barriers serialize on `snapshot_write_lock` independently of this
    /// best-effort dirty flag.
    ///
    /// Private, and reached only through the methods below: its two
    /// interesting transitions -- "a mutation needs persisting" and "this
    /// session was killed and must never be persisted again" -- are a single
    /// atomic state machine precisely so no call site can sequence them by
    /// hand. See [`crate::snapshot_state`] for why that is not optional.
    snapshot_state: SnapshotState,
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
    /// Aggregate of connection-local right-panel subscriptions. PTY
    /// forwarders consult this before building and broadcasting a raw-output
    /// frame; the PTY journal remains authoritative when no client displays a
    /// pane. A synchronous mutex is appropriate because updates are tiny,
    /// infrequent focus transitions and the read check never awaits.
    terminal_subscription_counts: std::sync::Mutex<TerminalSubscriptionCounts>,
    /// Monotonic invalidation token for PTY forwarders' connection-demand
    /// caches. Output is orders of magnitude more frequent than focus or
    /// attachment changes, so forwarders pay one relaxed-sized atomic load
    /// per chunk and consult `terminal_subscription_counts` only after this
    /// revision changes.
    terminal_subscription_revision: std::sync::atomic::AtomicU64,
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
    /// Live policy for whether `SetPaneProgressMonitor` is accepted at all --
    /// see `ClientRequest::UpdateProgressMonitorEnabled`. A plain
    /// `AtomicBool` (rather than going through `tree`/`panes`) because this
    /// is a session-wide switch with no per-pane state of its own, checked
    /// on every `SetPaneProgressMonitor` and updated only by its own
    /// handler.
    progress_monitor_enabled: std::sync::atomic::AtomicBool,
    /// Live setting observed by the owned rolling-backup task. A watch
    /// channel lets disabling stop future captures promptly without adding
    /// a versioned IPC request that older servers could not understand.
    session_backups_enabled: watch::Sender<bool>,
    /// Allocates process-local monitor generations. Zero is reserved for
    /// "no monitor"; persisted registrations receive a fresh generation
    /// when restored so stale clients can never clear their successor.
    next_progress_monitor_id: std::sync::atomic::AtomicU64,
    /// Bounded session-scoped replay cache for `SetPaneProgressMonitor`.
    /// Request IDs remain meaningful across a CLI reconnect to this server;
    /// argument collisions are rejected instead of replacing a monitor twice.
    pub(crate) progress_set_requests: Mutex<ProgressSetRequestCache>,
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
            socket_path: options.socket_path,
            detection_config: options.detection_config,
            notifications_config: options.notifications_config,
            sound_settings: RwLock::new(options.sound_settings),
            text_trigger_settings: RwLock::new(VersionedTextTriggerSettings::default()),
            sound_requests: options.sound_requests,
            custom_signatures: options.custom_signatures,
            tree: RwLock::new(tree),
            panes: RwLock::new(HashMap::new()),
            agent_debug: AgentDebugRecorder::new(options.agent_debug_menu_enabled),
            last_terminal_working_directory: Mutex::new(None),
            pending_session_recovery: Mutex::new(None),
            restructure_undo: Mutex::new(HashMap::new()),
            snapshot_write_lock: std::sync::Arc::new(Mutex::new(())),
            scheduled_input_transaction: Mutex::new(()),
            prompt_queue_transaction: Mutex::new(()),
            snapshot_state: SnapshotState::new(),
            snapshot_requested: Notify::new(),
            scheduled_input_changed: Notify::new(),
            detection_schedule_changed: Notify::new(),
            events,
            terminal_subscription_counts: std::sync::Mutex::new(
                TerminalSubscriptionCounts::default(),
            ),
            terminal_subscription_revision: std::sync::atomic::AtomicU64::new(0),
            shutdown: Notify::new(),
            connection_tasks: std::sync::Mutex::new(Vec::new()),
            progress_monitor_enabled: std::sync::atomic::AtomicBool::new(
                options.progress_monitor_enabled,
            ),
            session_backups_enabled: watch::channel(true).0,
            next_progress_monitor_id: std::sync::atomic::AtomicU64::new(1),
            progress_set_requests: Mutex::new(ProgressSetRequestCache::default()),
        }
    }

    /// Whether `SetPaneProgressMonitor` is currently accepted.
    pub fn is_progress_monitor_enabled(&self) -> bool {
        self.progress_monitor_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Applies a live `UpdateProgressMonitorEnabled` toggle. `Relaxed` is
    /// sufficient: every accepting/rejecting read of this flag
    /// (`handle_set_pane_progress_monitor`) has no other memory it must
    /// stay ordered with, unlike `snapshot_state`'s kill/dirty pair.
    pub fn set_progress_monitor_enabled(&self, enabled: bool) {
        self.progress_monitor_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn session_backups_enabled(&self) -> bool {
        *self.session_backups_enabled.borrow()
    }

    pub fn set_session_backups_enabled(&self, enabled: bool) {
        if self.session_backups_enabled() != enabled {
            self.session_backups_enabled.send_replace(enabled);
        }
    }

    pub fn watch_session_backups_enabled(&self) -> watch::Receiver<bool> {
        self.session_backups_enabled.subscribe()
    }

    /// Returns a non-zero, monotonically increasing generation used to fence
    /// replacement, terminal delivery, and clear requests for one monitor.
    pub fn allocate_progress_monitor_id(&self) -> u64 {
        self.next_progress_monitor_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

    /// Atomically replaces one connection's contribution to the aggregate
    /// terminal subscription counts.
    pub(crate) fn replace_terminal_subscriptions(
        &self,
        previous_all: bool,
        previous_panes: &std::collections::HashSet<NodeId>,
        next_all: bool,
        next_panes: &std::collections::HashSet<NodeId>,
    ) {
        let mut counts = self.terminal_subscription_counts.lock().unwrap();
        if previous_all {
            counts.all_panes = counts.all_panes.saturating_sub(1);
        }
        for pane_id in previous_panes {
            let should_remove = if let Some(count) = counts.panes.get_mut(pane_id) {
                *count = count.saturating_sub(1);
                *count == 0
            } else {
                false
            };
            if should_remove {
                counts.panes.remove(pane_id);
            }
        }
        if next_all {
            counts.all_panes = counts.all_panes.saturating_add(1);
        }
        for pane_id in next_panes {
            counts
                .panes
                .entry(*pane_id)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
        drop(counts);
        self.terminal_subscription_revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// Returns the invalidation token used by each PTY forwarder's local
    /// demand cache. Acquire pairs with the release increment after a
    /// connection changes the authoritative counts.
    pub(crate) fn terminal_subscription_revision(&self) -> u64 {
        self.terminal_subscription_revision
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Whether at least one attached connection currently displays this pane
    /// or is a legacy full-stream consumer.
    pub(crate) fn has_terminal_subscribers(&self, pane_id: NodeId) -> bool {
        let counts = self.terminal_subscription_counts.lock().unwrap();
        counts.all_panes > 0 || counts.panes.contains_key(&pane_id)
    }

    /// Marks the crash-recovery snapshot dirty and wakes the background
    /// debounced writer (`crate::persistence::spawn_snapshot_writer`).
    /// Cheap and non-blocking -- call sites are request handlers that must
    /// never `.await` a full disk write inline on the request path (see
    /// `crate::persistence` module docs).
    ///
    /// A no-op once the session has been killed: a killed session must never
    /// get a snapshot written for it again, no matter which still-attached
    /// connection or background task calls this afterward. The refusal is
    /// part of the same atomic step that would otherwise mark the snapshot
    /// dirty, so no interleaving can leave a killed session owing a write
    /// (see [`crate::snapshot_state`]).
    pub fn request_snapshot_save(&self) {
        if self.snapshot_state.mark_dirty() {
            self.snapshot_requested.notify_one();
        }
    }

    /// Claims a pending snapshot write for the caller, returning whether
    /// there was one to claim. Used only by `crate::persistence`'s writer.
    pub fn take_pending_snapshot(&self) -> bool {
        self.snapshot_state.take_dirty()
    }

    /// Marks this session unrecoverable, discarding any pending write.
    /// Terminal: nothing can mark the snapshot dirty afterward.
    ///
    /// `crate::run`'s shutdown grace period (see `SHUTDOWN_GRACE_PERIOD`)
    /// deliberately keeps other attached connections alive for a short
    /// window after one connection's `KillSession` request has already reset
    /// `tree`/`panes` to empty and deleted the on-disk snapshot. Without
    /// this, a second connection's ordinary mutation (e.g. `NewPane`)
    /// landing in that window would resurrect a snapshot file for a session
    /// `handle_kill_session` already decided has nothing worth recovering.
    pub fn mark_session_killed(&self) {
        self.snapshot_state.mark_session_killed();
    }

    /// Whether a snapshot write is currently owed. Tests only -- a writer
    /// must claim the work with `take_pending_snapshot` rather than acting
    /// on this observation.
    #[cfg(test)]
    pub fn is_snapshot_dirty(&self) -> bool {
        self.snapshot_state.is_dirty()
    }

    /// Whether this session has been killed. Tests only, for the same
    /// reason as above.
    #[cfg(test)]
    pub fn is_session_killed(&self) -> bool {
        self.snapshot_state.is_session_killed()
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
            socket_path: directory.path().join("state-test.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
            state.is_snapshot_dirty(),
            "an ordinary request must still be able to dirty the snapshot before any kill"
        );

        state.mark_session_killed();

        // Stands in for a second, still-attached connection's ordinary
        // mutation (e.g. `NewPane`) landing during the shutdown grace
        // period after this session was killed.
        state.request_snapshot_save();

        assert!(
            !state.is_snapshot_dirty(),
            "request_snapshot_save must not re-dirty the snapshot for an already-killed session"
        );
        assert!(
            state.is_session_killed(),
            "a refused request must leave the session killed, not merely clean"
        );
        assert!(
            !state.take_pending_snapshot(),
            "and the background writer must find no work for a killed session"
        );
    }

    #[tokio::test]
    async fn terminal_subscription_counts_follow_multi_client_replacement() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = test_state(&directory);
        let first = NodeId(11);
        let second = NodeId(12);
        let none = std::collections::HashSet::new();
        let first_only = std::collections::HashSet::from([first]);
        let both = std::collections::HashSet::from([first, second]);

        state.replace_terminal_subscriptions(false, &none, false, &first_only);
        state.replace_terminal_subscriptions(false, &none, false, &both);
        assert!(state.has_terminal_subscribers(first));
        assert!(state.has_terminal_subscribers(second));

        state.replace_terminal_subscriptions(false, &first_only, false, &none);
        assert!(state.has_terminal_subscribers(first));
        state.replace_terminal_subscriptions(false, &both, false, &none);
        assert!(!state.has_terminal_subscribers(first));
        assert!(!state.has_terminal_subscribers(second));

        state.replace_terminal_subscriptions(false, &none, true, &none);
        assert!(state.has_terminal_subscribers(NodeId(999)));
        state.replace_terminal_subscriptions(true, &none, false, &none);
        assert!(!state.has_terminal_subscribers(NodeId(999)));
    }

    // The concurrent request-versus-kill stress lives with the state machine
    // itself, in `crate::snapshot_state`: it is a property of that one
    // atomic, and testing it there needs neither a tempdir nor a whole
    // `ServerState` per iteration.
}
