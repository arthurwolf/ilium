//! `ilium-server`: owns all PTYs and the session tree for one session,
//! runs the adaptive agent-detection loop, and speaks the `ilium-ipc`
//! protocol over that session's Unix domain socket. See README
//! "Architecture" and `CLAUDE.md`'s layering rules -- this crate is the
//! one place those pieces are wired together; each piece's actual logic
//! lives in its own module (or, for the tree/pty/detection primitives
//! themselves, in `ilium-core`/`ilium-pty`/`ilium-detect`).
//!
//! One process serves exactly one session (see `CLAUDE.md`: one UDS
//! socket per session, never multiplexed) -- there is no multi-session
//! registry here, unlike a hypothetical single "server for the whole
//! machine." [`ServerOptions`]/[`run`] take that session's resolved paths
//! directly rather than resolving them internally, so tests can point a
//! server at a tempdir without touching `~/.config`/`~/.local/share` (see
//! `crate::paths` for how `main` resolves the real paths).

pub mod config;
mod detection;
pub mod error;
mod ipc;
mod mouse;
mod notifications;
mod pane;
pub mod paths;
mod persistence;
mod prompt_queue;
mod scheduled_input;
mod session_id;
mod shell_title;
mod sounds;
mod state;
mod task_guard;

use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UnixListener;

use ilium_detect::AgentSignature;

use crate::config::{DetectionConfig, NotificationsConfig, SessionRecoveryConfig};
use crate::error::ServerError;
use crate::state::{ServerState, ServerStateOptions};
use crate::task_guard::AbortOnDropHandle;

pub use crate::sounds::{NoopSoundPlayer, SoundPlayer, SystemSoundPlayer};

/// Everything [`run`] needs to serve one session. Constructed by `main`
/// from CLI-resolved project paths, or directly
/// by tests with tempdir paths -- either way `run` itself never touches
/// `directories::ProjectDirs`.
pub struct ServerOptions {
    pub session_name: String,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
    /// Project directory the CLI used to create this session. It is needed
    /// as the hard boundary for pane cwd and agent transcript ownership.
    pub session_cwd: PathBuf,
    /// User home containing the local built-in provider transcript stores.
    pub home_dir: PathBuf,
    pub detection_config: DetectionConfig,
    pub notifications_config: NotificationsConfig,
    pub sound_settings: ilium_sound::SoundSettings,
    /// Path watched by detached servers so changes made from a different
    /// project session also apply. Tests pass `None` to stay hermetic.
    pub sound_config_path: Option<PathBuf>,
    pub sound_player: Arc<dyn SoundPlayer>,
    /// User-configured agent signatures (`config::ServerConfig::custom_signatures`)
    /// to check alongside `ilium-detect`'s built-in registry -- threaded
    /// straight through to `ServerState` and from there into every
    /// detection-loop tick's `identify_agent_with_extra` call.
    pub custom_signatures: Vec<AgentSignature>,
    pub session_recovery: SessionRecoveryConfig,
}

/// How long `run` waits after a `KillSession` shutdown signal before
/// aborting still-open connection tasks and returning. Long enough for
/// each attached connection's writer loop to wake on the just-broadcast
/// final `TreeSnapshot` and flush it (a single `write_frame` call over a
/// local UDS), short enough that a deliberate shutdown still completes
/// promptly.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_millis(200);

/// Runs the server until a `KillSession` request is handled or the UDS
/// listener itself fails unrecoverably. It removes only a stale socket file;
/// a socket that accepts a connection belongs to a live server and is never
/// displaced.
pub async fn run(options: ServerOptions) -> Result<(), ServerError> {
    prepare_socket_path(&options.socket_path)?;
    let listener =
        UnixListener::bind(&options.socket_path).map_err(|source| ServerError::SocketBind {
            path: options.socket_path.clone(),
            source,
        })?;

    let (sound_requests, sound_playback_task) = sounds::spawn(Arc::clone(&options.sound_player));
    // See `task_guard`'s module doc: wrapping every long-lived child task's
    // handle here, immediately at the spawn site, ties each task's lifetime
    // to this `run` future's own stack frame -- so cancelling `run`'s task
    // from the outside (rather than only this function's own post-`select!`
    // cleanup below) is now guaranteed to stop all of them too.
    let sound_playback_task = AbortOnDropHandle::new(sound_playback_task);
    let state = Arc::new(ServerState::new(ServerStateOptions {
        session_name: options.session_name,
        session_cwd: options.session_cwd,
        home_dir: options.home_dir,
        snapshot_path: options.snapshot_path,
        detection_config: options.detection_config,
        notifications_config: options.notifications_config,
        sound_settings: options.sound_settings,
        sound_requests,
        custom_signatures: options.custom_signatures,
    }));

    if !matches!(options.session_recovery, SessionRecoveryConfig::StartFresh) {
        match persistence::load_snapshot_or_migrate(
            &state.snapshot_path,
            &state.session_cwd,
            &state.home_dir,
        )
        .await
        {
            Ok(Some(snapshot))
                if matches!(
                    options.session_recovery,
                    SessionRecoveryConfig::AskBeforeRestore
                ) =>
            {
                *state.pending_session_recovery.lock().await = Some(snapshot);
            }
            Ok(Some(snapshot)) => restore_snapshot(&state, snapshot).await,
            Ok(None) => {}
            Err(error) => tracing::warn!("failed to load crash-recovery snapshot: {error}"),
        }
    }

    let detection_task = AbortOnDropHandle::new(detection::spawn(Arc::clone(&state)));
    let scheduled_input_task = AbortOnDropHandle::new(scheduled_input::spawn(Arc::clone(&state)));
    let snapshot_writer_task =
        AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
    let sound_config_watcher_task = options
        .sound_config_path
        .clone()
        .map(|path| AbortOnDropHandle::new(sounds::spawn_config_watcher(Arc::clone(&state), path)));

    tokio::select! {
        () = ipc::accept_loop(Arc::clone(&state), listener) => {}
        () = state.shutdown.notified() => {
            tracing::info!("session {:?} received a shutdown signal", state.session_name);
        }
    }

    detection_task.abort();
    scheduled_input_task.abort();
    if let Some(task) = sound_config_watcher_task {
        task.abort();
    }
    // Gives already-broadcast events (notably `KillSession`'s final
    // `TreeSnapshot`) a chance to actually reach attached clients before
    // their connection tasks are cancelled -- see this constant's doc
    // comment.
    tokio::time::sleep(SHUTDOWN_GRACE_PERIOD).await;
    state.abort_all_connection_tasks();

    // Flush any crash-recovery mutation too recent for the background
    // debounced writer's window to have picked up yet (a `KillSession`
    // shutdown will usually have already cleared the dirty flag itself
    // and removed the snapshot file -- see `ipc::handlers::handle_kill_session`
    // -- making this a no-op in that case), then wait for the shared
    // write lock to confirm any write that writer already had in flight
    // has fully finished, before finally stopping it. See
    // `persistence::spawn_snapshot_writer`'s doc comment for why this
    // task is never aborted any earlier than this.
    persistence::flush_pending_snapshot(&state).await;
    {
        let _write_guard = state.snapshot_write_lock.lock().await;
    }
    snapshot_writer_task.abort();
    sound_playback_task.abort();

    // Best-effort: a clean `KillSession` shutdown has usually already
    // removed this file itself, so `NotFound` is the common, silent case --
    // but any other failure (e.g. a permissions problem) is worth a warning
    // rather than being discarded, matching `ipc::handlers::handle_kill_session`'s
    // identical cleanup.
    match std::fs::remove_file(&options.socket_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            "failed to remove session socket file {:?} on shutdown: {error}",
            options.socket_path
        ),
    }
    Ok(())
}

/// Refuses to replace a live session listener while allowing a daemon to
/// recover from the stale filesystem entry a crashed predecessor leaves.
fn prepare_socket_path(socket_path: &Path) -> Result<(), ServerError> {
    if !socket_path.exists() {
        return Ok(());
    }

    match StdUnixStream::connect(socket_path) {
        Ok(_) => Err(ServerError::SessionAlreadyRunning(
            socket_path.to_path_buf(),
        )),
        Err(_) => {
            std::fs::remove_file(socket_path)?;
            Ok(())
        }
    }
}

/// Applies a crash-recovery snapshot: replaces `state.tree` wholesale (the
/// snapshot already *is* the tree -- no need to rebuild it node-by-node) and
/// respawns/registers every snapshotted pane's resource via
/// `ipc::handlers::spawn_and_register_pane`, the same function a live
/// client's `NewPane` request uses.
///
/// This function has two callers: `run`'s own boot sequence (when
/// `SessionRecoveryConfig` is not `AskBeforeRestore`, the tree is still
/// empty at this point so there is nothing to orphan) and
/// `ipc::handlers::handle_session_recovery_resolution` (when it *is*
/// `AskBeforeRestore`, the server serves requests -- including `NewPane` --
/// against an empty tree for as long as the restore decision is pending,
/// so by the time this runs `state.tree`/`state.panes` may already hold
/// live panes the snapshot knows nothing about). Because `state.tree` is
/// about to be replaced wholesale by `snapshot.tree` -- a tree built by an
/// unrelated `Tree::new()` node-id counter in a previous process, so its
/// ids collide with the live tree's ids by coincidence, not by identity --
/// every `PaneResource` already registered in `state.panes` at this instant
/// is stale regardless of whether its node id happens to also appear in
/// `snapshot.tree`. All of them are torn down here before the tree is
/// overwritten, exactly like the unconditional `panes.drain()` in
/// `ipc::handlers::handle_kill_session` -- otherwise a `PaneResource` (PTY
/// child process, reader thread, output journal, forwarder task) could stay
/// registered in `state.panes` forever, keyed by a node id that either no
/// longer resolves to a pane in the new tree or, worse, now resolves to an
/// unrelated node the snapshot introduced.
///
/// A pane that fails to respawn (most commonly: its command's binary no
/// longer exists) is logged and dropped from the restored tree rather than
/// kept as a node with no resource behind it -- `ilium_core::PaneStatus`
/// has no "broken" variant to represent that state, and every other place
/// in this crate that ends up with a tree node it cannot back with a live
/// resource (see `handle_new_pane`'s own failure path) removes the node
/// rather than inventing one, so this does the same for consistency rather
/// than introducing a new kind of pane status only this one caller would
/// ever produce. A single pane failing to respawn must never abort the
/// rest of the restore, matching this crate's top-level error-boundary
/// rule.
pub(crate) async fn restore_snapshot(
    state: &Arc<ServerState>,
    snapshot: persistence::SessionSnapshot,
) {
    let pane_count = snapshot.panes.len();
    {
        let mut tree = state.tree.write().await;
        *tree = snapshot.tree;
        // Keep the write guard held across the unconditional pane-registry
        // drain below -- see `ipc::handlers::spawn_and_register_pane_in_directory`'s
        // doc comment for why this tree-overwrite-then-sweep pair must stay
        // atomic with respect to that function's own tree-check-then-
        // panes-insert pair, under the "tree before panes" ordering
        // `state.rs` documents. Only dropped once this whole block ends,
        // which is also before the per-pane respawn loop below takes its
        // own `tree` lock per iteration.
        //
        // Every entry in `state.panes` at this point predates the snapshot
        // (see this function's doc comment for why matching node ids across
        // the two trees would be coincidental, not meaningful), so this
        // drains the whole registry rather than filtering it -- mirroring
        // `ipc::handlers::handle_kill_session`.
        let mut panes = state.panes.write().await;
        for (pane_id, resource) in panes.drain() {
            ipc::handlers::teardown_pane_resource(pane_id, resource);
        }
        drop(panes);
    }

    let mut failed_pane_ids = Vec::new();
    for pane_snapshot in snapshot.panes {
        let node_id = pane_snapshot.node_id;
        match ipc::handlers::spawn_and_register_pane(state, node_id, pane_snapshot.kind).await {
            Ok(()) => {}
            Err(ipc::handlers::RegisterPaneError::NodeRemoved(_)) => {
                // A concurrent request removed `node_id` from the tree
                // (e.g. `ClosePane`) while this snapshot's own respawn loop
                // was mid-flight on it; `spawn_and_register_pane` already
                // tore the freshly-spawned resource back down, and the tree
                // node is already gone, so -- unlike the `Spawn` failure
                // case below -- there is no tree cleanup left to do and
                // this is not a respawn failure worth reporting to the
                // user.
            }
            Err(ipc::handlers::RegisterPaneError::Spawn(error)) => {
                tracing::warn!(
                    "failed to respawn pane {node_id:?} from crash-recovery snapshot, dropping \
                     it from the restored tree: {error}"
                );
                let mut tree = state.tree.write().await;
                // `node_id` was just loaded as part of this same snapshot's
                // tree above, so it is expected to always be present here;
                // still logged rather than silently discarded in case that
                // invariant is ever violated by a future bug.
                if let Err(remove_error) = tree.remove_node(node_id) {
                    tracing::warn!(
                        "crash-recovery restore could not remove failed pane {node_id:?} from \
                         the tree: {remove_error}"
                    );
                }
                failed_pane_ids.push(node_id);
            }
        }
    }

    if failed_pane_ids.is_empty() {
        tracing::info!(
            "restored {pane_count} pane(s) from crash-recovery snapshot for session {:?}",
            state.session_name
        );
    } else {
        tracing::warn!(
            "restored {} of {pane_count} pane(s) from crash-recovery snapshot for session {:?}; \
             {} failed to respawn and were dropped: {failed_pane_ids:?}",
            pane_count - failed_pane_ids.len(),
            state.session_name,
            failed_pane_ids.len()
        );
        // The in-memory tree just diverged from the on-disk snapshot (it
        // no longer contains the pane(s) that failed to respawn), and
        // nothing else on this startup path mutates the tree again to
        // naturally trigger a save. Without this, a server that crashes
        // again before any user-driven mutation would leave the stale,
        // still-failing pane(s) in the persisted snapshot forever,
        // repeating this same failed respawn on every future restart.
        state.request_snapshot_save();
    }
}

/// End-to-end test of the crash-recovery restore path: writes a
/// [`persistence::SessionSnapshot`] fixture straight to a tempdir path (no
/// dependency on `persistence::save_snapshot`'s own atomic-write plumbing,
/// which is already covered by `persistence`'s own tests), points a real
/// [`run`] at that path, and asserts over the wire (via `ilium-ipc`, the
/// same way an attached client would) that the restored tree matches the
/// fixture *and* that the restored terminal pane has a live pty behind it,
/// not just a tree node. Lives here rather than in `tests/` because it
/// needs to construct `persistence::SessionSnapshot`/`pane::PaneSnapshotKind`
/// directly, and both are crate-private (see this crate's module docs on
/// why the client/server boundary only ever goes through `ilium-ipc`).
#[cfg(test)]
mod restore_tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use ilium_core::{PaneContentKind, Tree, ROOT_ID};
    use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
    use tokio::net::UnixStream;

    use super::*;
    use crate::pane::{PaneSnapshotKind, TerminalOrigin};
    use crate::persistence::{PaneSnapshot, SessionSnapshot};

    /// Mirrors `ilium-server/tests/smoke.rs`'s helper of the same name --
    /// binding a UDS socket is not instant, and a fixed sleep would be
    /// either flaky (too short) or slow (long enough to never flake).
    async fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if condition() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Mirrors `ilium-server/tests/smoke.rs`'s helper of the same name --
    /// reads frames until one matches `predicate`, so an interleaved,
    /// unrelated broadcast can never make this a flaky "next frame must
    /// match" assertion.
    async fn expect_event(
        stream: &mut UnixStream,
        timeout: Duration,
        predicate: impl Fn(&ServerEvent) -> bool,
    ) -> ServerEvent {
        tokio::time::timeout(timeout, async {
            loop {
                let event: ServerEvent = read_frame(stream).await.expect("read a server event");
                if predicate(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for the expected server event")
    }

    #[tokio::test]
    async fn startup_respawns_panes_from_a_crash_recovery_snapshot() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let socket_path = dir.path().join("restore-test.sock");
        let snapshot_path = dir.path().join("restore-test.snapshot.json");

        // The tree a previous, now-dead server process would have
        // persisted: one group holding a terminal pane and an editor pane.
        // `cat` (not `$SHELL`/a login shell) is the terminal's command so
        // this test proves a real pty round-trip without depending on any
        // particular shell's prompt/echo behavior -- only that bytes
        // written to it come back out, exactly like
        // `ilium-pty/tests/pty_integration.rs`'s own `cat` tests.
        let mut tree = Tree::new();
        let project = tree
            .ensure_launch_project(dir.path().to_path_buf())
            .expect("snapshot fixture accepts its launch project");
        let group = tree.add_group(project, "restored").unwrap();
        let terminal_pane_id = tree
            .add_pane(group, "cat", PaneContentKind::Terminal)
            .unwrap();
        let editor_pane_id = tree
            .add_pane(group, "restored-notes.md", PaneContentKind::Editor)
            .unwrap();

        let snapshot = SessionSnapshot {
            version: 1,
            tree: tree.clone(),
            panes: vec![
                PaneSnapshot {
                    node_id: terminal_pane_id,
                    kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command("cat".to_string())),
                },
                PaneSnapshot {
                    node_id: editor_pane_id,
                    kind: PaneSnapshotKind::Editor {
                        path: Some(PathBuf::from("/tmp/restored-notes.md")),
                    },
                },
            ],
        };
        let json = serde_json::to_vec_pretty(&snapshot).expect("serialize snapshot fixture");
        tokio::fs::write(&snapshot_path, &json)
            .await
            .expect("write snapshot fixture");

        let options = ServerOptions {
            session_name: "restore-test".to_string(),
            socket_path: socket_path.clone(),
            snapshot_path,
            session_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            detection_config: DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_config_path: None,
            sound_player: Arc::new(NoopSoundPlayer),
            custom_signatures: Vec::new(),
            session_recovery: SessionRecoveryConfig::RestoreAutomatically,
        };
        let server_task = tokio::spawn(run(options));

        let bound = wait_until(|| socket_path.exists(), Duration::from_secs(5)).await;
        assert!(bound, "server did not bind its socket in time");

        let mut client = UnixStream::connect(&socket_path)
            .await
            .expect("connect to the session socket");
        write_frame(
            &mut client,
            &ClientRequest::Attach {
                session: "restore-test".to_string(),
            },
        )
        .await
        .expect("write Attach request");

        let event = expect_event(&mut client, Duration::from_secs(5), |_| true).await;
        let ServerEvent::TreeSnapshot(restored_tree) = event else {
            panic!("expected TreeSnapshot on attach, got {event:?}");
        };
        assert_eq!(
            restored_tree, tree,
            "the tree the server attaches with should exactly match the saved snapshot's tree"
        );

        // Prove the terminal pane is a live, respawned pty -- not just a
        // tree node -- by writing to it and reading its own bytes echoed
        // back as a `ScreenUpdate`.
        write_frame(
            &mut client,
            &ClientRequest::KeyInput {
                pane_id: terminal_pane_id,
                bytes: b"restore-marker\n".to_vec(),
                submission: None,
            },
        )
        .await
        .expect("write KeyInput request");

        // `expect_event`'s predicate already only returns once it has seen a
        // `ScreenUpdate` for this exact pane containing the marker, so
        // reaching this point (rather than the timeout panic inside
        // `expect_event`) *is* the assertion -- a further `matches!` on
        // `event` here would be a tautology.
        let _event = expect_event(&mut client, Duration::from_secs(5), |event| {
            matches!(
                event,
                ServerEvent::ScreenUpdate {
                    pane_id,
                    bytes,
                    ..
                }
                    if *pane_id == terminal_pane_id
                        && String::from_utf8_lossy(bytes).contains("restore-marker")
            )
        })
        .await;

        write_frame(&mut client, &ClientRequest::KillSession)
            .await
            .expect("write KillSession request");
        let _ = tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("server did not exit after KillSession")
            .expect("server task panicked");
    }

    /// Reproduces the `AskBeforeRestore` orphan-leak scenario directly
    /// against `restore_snapshot` (no socket needed): a pane gets
    /// registered in `state.panes`/`state.tree` -- exactly what a live
    /// client's `NewPane` request does while a recovery decision is still
    /// pending, see `crate::run`'s doc comment on
    /// `SessionRecoveryConfig::AskBeforeRestore` -- and then a snapshot
    /// that never knew about that pane overwrites the tree. Asserts the
    /// orphaned pane's registry entry (and therefore its pty child, reader
    /// thread, and forwarder task) does not survive the overwrite.
    #[tokio::test]
    async fn restore_snapshot_tears_down_a_pane_orphaned_by_the_tree_overwrite() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let (sound_requests, _playback_task) = crate::sounds::spawn(Arc::new(NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "orphan-test".to_string(),
            session_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            snapshot_path: dir.path().join("orphan-test.snapshot.json"),
            detection_config: DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
        }));

        let orphan_pane_id = {
            let mut tree = state.tree.write().await;
            let group = tree
                .add_group(ROOT_ID, "leftover-group")
                .expect("add the soon-to-be-orphaned pane's parent group");
            tree.add_pane(group, "leftover", PaneContentKind::Terminal)
                .expect("add the soon-to-be-orphaned pane's tree node")
        };
        crate::ipc::handlers::spawn_and_register_pane(
            &state,
            orphan_pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command("cat".to_string())),
        )
        .await
        .expect("spawn the soon-to-be-orphaned pane's live pty");
        assert!(
            state.panes.read().await.contains_key(&orphan_pane_id),
            "test setup: the pane must be live before the overwrite it's meant to survive"
        );

        // The snapshot being restored predates the pane above entirely --
        // it was created after this snapshot was written to disk, during
        // the `AskBeforeRestore` window.
        let snapshot = SessionSnapshot {
            version: 1,
            tree: Tree::new(),
            panes: Vec::new(),
        };
        restore_snapshot(&state, snapshot).await;

        assert!(
            !state.panes.read().await.contains_key(&orphan_pane_id),
            "restore_snapshot must tear down a pane orphaned by the tree overwrite, not leak it"
        );
        assert!(
            state.tree.read().await.get(orphan_pane_id).is_none(),
            "the orphaned pane's node must not survive in the overwritten tree either"
        );
    }

    /// Reproduces the id-collision variant of the same `AskBeforeRestore`
    /// leak: the orphaned live pane and the incoming snapshot's tree both
    /// allocate node ids from their own independent `Tree::new()` counter
    /// (this process's live tree vs. the previous process's persisted
    /// tree), so it is pure coincidence whenever a node id in one also
    /// appears in the other. Before the fix this test guards, `restore_snapshot`
    /// mistook that coincidence for identity: it only tore down a
    /// pre-existing live pane whose id was entirely absent from
    /// `snapshot.tree`, so an orphaned pane whose id happened to collide
    /// with an unrelated *group* node in the snapshot was skipped by the
    /// sweep and left registered in `state.panes` forever -- unreachable by
    /// any later close, since `collect_pane_descendants` only ever returns
    /// pane nodes, never the group itself. Asserts the orphaned pane's
    /// registry entry does not survive the overwrite even though its id
    /// resolves to a live (group) node in the post-overwrite tree.
    #[tokio::test]
    async fn restore_snapshot_tears_down_an_orphaned_pane_whose_id_collides_with_a_snapshot_group()
    {
        let dir = tempfile::tempdir().expect("create tempdir");
        let (sound_requests, _playback_task) = crate::sounds::spawn(Arc::new(NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "collision-test".to_string(),
            session_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            snapshot_path: dir.path().join("collision-test.snapshot.json"),
            detection_config: DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
        }));

        // Same shape as the plain orphan-teardown test above: one launch
        // project, one group, then one pane. The pane lands at `NodeId(3)`.
        let orphan_pane_id = {
            let mut tree = state.tree.write().await;
            let project = tree
                .project_ids()
                .into_iter()
                .next()
                .expect("fresh state has one launch project");
            let group = tree
                .add_group(project, "leftover-group")
                .expect("add the soon-to-be-orphaned pane's parent group");
            tree.add_pane(group, "leftover", PaneContentKind::Terminal)
                .expect("add the soon-to-be-orphaned pane's tree node")
        };
        crate::ipc::handlers::spawn_and_register_pane(
            &state,
            orphan_pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command("cat".to_string())),
        )
        .await
        .expect("spawn the soon-to-be-orphaned pane's live pty");
        assert!(
            state.panes.read().await.contains_key(&orphan_pane_id),
            "test setup: the pane must be live before the overwrite it's meant to survive"
        );

        // Independently allocates a project and two groups from its own
        // `Tree::new()` counter, so the second group lands on the exact same `NodeId` as
        // `orphan_pane_id` above -- by coincidence, not by any shared
        // history with the live tree.
        let mut snapshot_tree = Tree::new();
        let snapshot_project = snapshot_tree
            .ensure_launch_project(PathBuf::from("/tmp/snapshot-project"))
            .expect("snapshot tree accepts its project");
        let _first_group = snapshot_tree
            .add_group(snapshot_project, "a")
            .expect("add the snapshot's first group");
        let colliding_group = snapshot_tree
            .add_group(snapshot_project, "b")
            .expect("add the snapshot's second group, colliding with orphan_pane_id");
        assert_eq!(
            colliding_group, orphan_pane_id,
            "test setup: the snapshot group must land on the same id as the orphaned pane"
        );

        let snapshot = SessionSnapshot {
            version: 1,
            tree: snapshot_tree,
            panes: Vec::new(),
        };
        restore_snapshot(&state, snapshot).await;

        assert!(
            !state.panes.read().await.contains_key(&orphan_pane_id),
            "restore_snapshot must tear down an orphaned pane even when its id collides with an \
             unrelated node in the snapshot tree, not leak it"
        );
        assert!(
            state
                .tree
                .read()
                .await
                .get(orphan_pane_id)
                .is_some_and(|node| node.is_group()),
            "test setup: the colliding id must resolve to the snapshot's group node after the \
             overwrite, proving the teardown above did not merely happen because the id was gone"
        );
    }

    /// Reproduces the race `spawn_and_register_pane_in_directory`'s "tree
    /// before panes" locking now closes: a concurrent structural removal
    /// (here, standing in for any of `handle_close_pane`,
    /// `handle_revert_last_restructure`, or `restore_snapshot`) can remove
    /// `pane_id` from the tree while this function's own pty spawn is
    /// still in flight, i.e. before its resource is inserted into
    /// `state.panes`. Simulates "the removal already happened" by removing
    /// the node from `state.tree` before calling
    /// `spawn_and_register_pane_in_directory`, then asserts the call
    /// notices, tears its own freshly-spawned resource back down, and
    /// returns `NodeRemoved` -- rather than leaking a live pty (child
    /// process, reader thread, forwarder task) keyed by a node id no
    /// longer reachable from any tree, which no later sweep could ever
    /// find again.
    #[tokio::test]
    async fn spawn_and_register_pane_tears_down_its_own_resource_if_the_node_was_removed_first() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let (sound_requests, _playback_task) = crate::sounds::spawn(Arc::new(NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "new-pane-race-test".to_string(),
            session_cwd: dir.path().to_path_buf(),
            home_dir: dir.path().to_path_buf(),
            snapshot_path: dir.path().join("new-pane-race-test.snapshot.json"),
            detection_config: DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
        }));

        let pane_id = {
            let mut tree = state.tree.write().await;
            let group = tree
                .add_group(ROOT_ID, "about-to-be-closed")
                .expect("add the pane's parent group");
            tree.add_pane(group, "cat", PaneContentKind::Terminal)
                .expect("add the pane's tree node")
        };

        // Stand in for a second connection's `ClosePane` (or
        // `RevertLastRestructure`, or a session-recovery restore) landing
        // while this pane's spawn is still in flight: the node is already
        // gone from the tree by the time
        // `spawn_and_register_pane_in_directory`'s own presence check runs.
        state
            .tree
            .write()
            .await
            .remove_node(pane_id)
            .expect("remove the pane's node, simulating a concurrent close");

        let cwd = dir.path().to_path_buf();
        let result = crate::ipc::handlers::spawn_and_register_pane_in_directory(
            &state,
            pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command("cat".to_string())),
            &cwd,
        )
        .await;

        assert!(
            matches!(
                result,
                Err(crate::ipc::handlers::RegisterPaneError::NodeRemoved(id)) if id == pane_id
            ),
            "expected NodeRemoved for a node the tree no longer has, got {result:?}"
        );
        assert!(
            !state.panes.read().await.contains_key(&pane_id),
            "a resource must never be registered under a node id absent from the tree -- that \
             is the leak this test guards against"
        );
    }

    #[tokio::test]
    async fn restart_request_flushes_a_recoverable_snapshot_before_the_server_exits() {
        let directory = tempfile::tempdir().expect("tempdir");
        let socket_path = directory.path().join("restart-test.sock");
        let snapshot_path = directory.path().join("restart-test.snapshot.json");
        let options = ServerOptions {
            session_name: "restart-test".to_string(),
            socket_path: socket_path.clone(),
            snapshot_path: snapshot_path.clone(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            detection_config: DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_config_path: None,
            sound_player: Arc::new(NoopSoundPlayer),
            custom_signatures: Vec::new(),
            session_recovery: SessionRecoveryConfig::RestoreAutomatically,
        };
        let server_task = tokio::spawn(run(options));

        assert!(
            wait_until(|| socket_path.exists(), Duration::from_secs(5)).await,
            "server did not bind its socket"
        );

        let mut client = UnixStream::connect(&socket_path)
            .await
            .expect("connect to the session socket");
        write_frame(&mut client, &ClientRequest::RestartServer)
            .await
            .expect("request a snapshot-preserving restart");

        let result = tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("server did not exit after RestartServer")
            .expect("server task panicked");
        assert!(
            result.is_ok(),
            "server restart should exit cleanly: {result:?}"
        );
        assert!(
            persistence::load_snapshot(&snapshot_path)
                .await
                .expect("read snapshot")
                .is_some(),
            "RestartServer must leave a snapshot for the replacement server"
        );
    }
}

#[cfg(test)]
mod socket_tests {
    use super::*;

    #[test]
    fn refuses_to_replace_a_live_session_socket() {
        let directory = tempfile::tempdir().expect("tempdir");
        let socket_path = directory.path().join("live.sock");
        let _listener =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind listener");

        let result = prepare_socket_path(&socket_path);

        assert!(
            matches!(result, Err(ServerError::SessionAlreadyRunning(path)) if path == socket_path)
        );
        assert!(socket_path.exists(), "live socket must remain in place");
    }
}
