//! One session's pending crash-recovery decision and accepted resolution.
//! The slot owns exactly one accepted coordination task independently of its
//! requesting socket. Snapshot I/O and CPU retirement remain with their
//! existing execution owners.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ilium_execution::StorageAdmission;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::snapshot_io::LoadedSnapshot;
use crate::state::ServerState;

// Completion watches retain only fixed static diagnostics; their copies
// cannot outlive a released allocation charge or grow with native path text.
pub(crate) type Resolution = Result<(), &'static str>;
pub(crate) type Completion = watch::Receiver<Option<Resolution>>;

#[derive(Default)]
struct Slot {
    closed: bool,
    pending: Option<LoadedSnapshot>,
    active: Option<Active>,
    failure: Option<&'static str>,
}

struct Active {
    handle: JoinHandle<TaskResult>,
    completion: Completion,
}

/// On a refused operation, the exact decoded original returns in the join
/// result. The task never has to acquire its owner's slot while a drainer is
/// awaiting its handle under that same slot lock.
struct TaskResult {
    outcome: Resolution,
    retry: Option<LoadedSnapshot>,
}

pub(crate) struct RecoveryOwner {
    admission_closed: AtomicBool,
    // Installed once during startup, before writers/clients start. Only a
    // completed semantic resolution reopens ordinary snapshot replacement.
    preserve_original: AtomicBool,
    slot: Mutex<Slot>,
    #[cfg(test)]
    pub(crate) before_restore_publication: Notify,
    #[cfg(test)]
    pub(crate) after_restore_publication: Notify,
}

impl Default for RecoveryOwner {
    fn default() -> Self {
        Self {
            admission_closed: AtomicBool::new(false),
            preserve_original: AtomicBool::new(false),
            slot: Mutex::new(Slot::default()),
            #[cfg(test)]
            before_restore_publication: Notify::new(),
            #[cfg(test)]
            after_restore_publication: Notify::new(),
        }
    }
}

pub(crate) enum AttachStatus {
    Pending { pane_count: usize },
    Resolving(Completion),
    Failed(&'static str),
    Settled,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Closed,
    Busy,
    NoPending,
    Failed(&'static str),
    Unavailable(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("session recovery is closing"),
            Self::Busy => formatter.write_str("session recovery is already resolving"),
            Self::NoPending => formatter.write_str("no session recovery decision is pending"),
            Self::Failed(message) => formatter.write_str(message),
            Self::Unavailable(message) => {
                write!(formatter, "session recovery admission refused: {message}")
            }
        }
    }
}

/// A completed task is joined without removing its handle first. If the
/// caller is cancelled while awaiting that join, a later drainer can resume
/// it. The task never takes this lock, so holding the guard cannot deadlock.
async fn settle_if_completed(slot: &mut Slot) {
    let should_join = slot
        .active
        .as_ref()
        .is_some_and(|active| active.handle.is_finished() || active.completion.borrow().is_some());
    if should_join {
        if let Some(active) = slot.active.as_mut() {
            let joined = (&mut active.handle).await;
            slot.active.take();
            settle_join(slot, joined);
        }
    }
}

fn settle_join(slot: &mut Slot, joined: Result<TaskResult, tokio::task::JoinError>) {
    match joined {
        Ok(TaskResult { outcome, retry }) => {
            slot.failure = outcome.err();
            if let Err(message) = outcome {
                tracing::warn!(%message, "session recovery resolution failed");
            }
            if let Some(snapshot) = retry {
                if slot.pending.is_none() {
                    slot.pending = Some(snapshot);
                } else {
                    // Startup installs at most one result and this active
                    // task consumed that result. A collision is an invariant
                    // breach; the disk original remains the recovery source.
                    slot.failure = Some("session recovery retry slot was occupied");
                    tracing::error!("session recovery retry slot was occupied");
                    drop(snapshot);
                }
            }
        }
        Err(error) => {
            // The consumed original may already have been partially published
            // or CPU-retired. Do not fabricate a decoded retry or settled state.
            slot.failure =
                Some("session recovery task failed; stored snapshot replacement remains blocked");
            tracing::error!(%error, "session recovery resolution task failed");
        }
    }
}

impl RecoveryOwner {
    /// Startup installs its sole decoded original before accepting clients.
    /// A rejected original stays with the caller and retains CPU retirement.
    pub(crate) async fn install_initial(
        &self,
        snapshot: LoadedSnapshot,
    ) -> Result<(), LoadedSnapshot> {
        let mut slot = self.slot.lock().await;
        if self.admission_closed.load(Ordering::Acquire)
            || slot.closed
            || slot.pending.is_some()
            || slot.active.is_some()
            || slot.failure.is_some()
        {
            return Err(snapshot);
        }
        self.preserve_original.store(true, Ordering::Release);
        slot.pending = Some(snapshot);
        Ok(())
    }

    /// Ordinary snapshot replacement must not overwrite an undecided,
    /// in-progress or failed recovery. This read cannot join/take the slot:
    /// a drainer may hold that slot while discard awaits the write lock.
    pub(crate) fn preserves_original(&self) -> bool {
        self.preserve_original.load(Ordering::Acquire)
    }

    /// Worktree pruning must retain its protection while either the exact
    /// pending original or an accepted resolution still belongs to this owner.
    /// Settle an already completed task before inspecting; no new task is begun.
    pub(crate) async fn is_unresolved(&self) -> bool {
        let mut slot = self.slot.lock().await;
        settle_if_completed(&mut slot).await;
        self.preserves_original()
            || slot.pending.is_some()
            || slot.active.is_some()
            || slot.failure.is_some()
    }

    pub(crate) async fn attach_status(&self) -> AttachStatus {
        let mut slot = self.slot.lock().await;
        settle_if_completed(&mut slot).await;
        if let Some(snapshot) = slot.pending.as_ref() {
            return AttachStatus::Pending {
                pane_count: snapshot.panes.len(),
            };
        }
        if let Some(message) = slot.failure {
            return AttachStatus::Failed(message);
        }
        if let Some(active) = slot.active.as_ref() {
            return match *active.completion.borrow() {
                Some(Ok(())) => AttachStatus::Settled,
                Some(Err(message)) => AttachStatus::Failed(message),
                None => AttachStatus::Resolving(active.completion.clone()),
            };
        }
        if self.preserves_original() {
            return AttachStatus::Failed(
                "session recovery closed without resolving the stored snapshot",
            );
        }
        AttachStatus::Settled
    }

    /// Admission, the pending-original transfer and handle registration form
    /// one locked transition. No filesystem call or expensive work runs under
    /// the slot. A finished task is joined before any retry consumes its input.
    pub(crate) async fn admit(
        &self,
        state: Arc<ServerState>,
        restore: bool,
    ) -> Result<Completion, Refusal> {
        if self.admission_closed.load(Ordering::Acquire) {
            return Err(Refusal::Closed);
        }
        let mut slot = self.slot.lock().await;
        if slot.closed || self.admission_closed.load(Ordering::Acquire) {
            return Err(Refusal::Closed);
        }
        settle_if_completed(&mut slot).await;
        if slot.active.is_some() {
            return Err(Refusal::Busy);
        }
        if slot.pending.is_none() {
            return Err(slot.failure.map_or(Refusal::NoPending, Refusal::Failed));
        }
        let frame_bytes = semantic_frame_bytes()
            .ok_or_else(|| Refusal::Unavailable("resolution frame size overflow".to_string()))?;
        let frame_storage = if let Some(execution) = state.execution.get() {
            Some(
                execution
                    .client
                    .try_reserve_storage(frame_bytes)
                    .map_err(|reason| Refusal::Unavailable(format!("{reason:?}")))?,
            )
        } else {
            #[cfg(test)]
            {
                // Older no-bank semantic fixtures may still exercise their
                // isolated paths. Forcing owner tests install the real bank.
                None
            }
            #[cfg(not(test))]
            {
                return Err(Refusal::Unavailable(
                    "server execution owner is unavailable".to_string(),
                ));
            }
        };
        let (sender, completion) = watch::channel(None);
        // The sole original is transferred only after frame admission succeeds.
        // The handle is stored before another claimant can acquire the slot.
        let Some(snapshot) = slot.pending.take() else {
            return Err(Refusal::NoPending);
        };
        slot.failure = None;
        let handle = tokio::spawn(resolve(state, snapshot, restore, sender, frame_storage));
        slot.active = Some(Active {
            handle,
            completion: completion.clone(),
        });
        Ok(completion)
    }

    /// Fences new decisions, joins the one accepted semantic task, and only
    /// then releases an unchosen or failed original to CPU retirement.
    /// Holding the slot lock through join makes simultaneous drains linear;
    /// cancellation leaves the handle in the slot for the next drainer.
    pub(crate) async fn close_and_drain(&self) -> Resolution {
        // Fence new claims immediately, including while another drainer
        // holds the slot lock waiting for the accepted task to finish.
        self.admission_closed.store(true, Ordering::Release);
        let mut slot = self.slot.lock().await;
        slot.closed = true;
        if let Some(active) = slot.active.as_mut() {
            let joined = (&mut active.handle).await;
            slot.active.take();
            settle_join(&mut slot, joined);
        }
        let outcome = slot.failure.map_or(Ok(()), Err);
        let pending = slot.pending.take();
        drop(slot);
        drop(pending);
        // Retirement is not successful semantic resolution. In particular,
        // neither the disk fence nor the latched failure is cleared here.
        outcome
    }

    #[cfg(test)]
    pub(crate) fn notify_restore_prepublication(&self) {
        self.before_restore_publication.notify_waiters();
    }
}

/// The task's actual compiler frame plus fixed runtime/channel metadata is
/// charged against the existing root before consuming the original. This
/// does not replace SnapshotIo's separate decoded-result lease or the
/// restore future's preadmitted CPU retirement envelope.
fn semantic_frame_bytes() -> Option<usize> {
    type Resolver<F> = fn(
        Arc<ServerState>,
        LoadedSnapshot,
        bool,
        watch::Sender<Option<Resolution>>,
        Option<Arc<StorageAdmission>>,
    ) -> F;
    fn frame_size<F>(_: Resolver<F>) -> usize {
        std::mem::size_of::<F>()
    }
    frame_size(resolve).checked_add(4096)
}

pub(crate) async fn wait_for_result(mut completion: Completion) -> Resolution {
    loop {
        if let Some(result) = *completion.borrow_and_update() {
            return result;
        }
        if completion.changed().await.is_err() {
            return Err("session recovery task ended without a semantic result");
        }
    }
}

async fn resolve(
    state: Arc<ServerState>,
    snapshot: LoadedSnapshot,
    restore: bool,
    completion: watch::Sender<Option<Resolution>>,
    frame_storage: Option<Arc<StorageAdmission>>,
) -> TaskResult {
    let result = if restore {
        match crate::restore_snapshot(&state, snapshot).await {
            std::ops::ControlFlow::Continue(()) => {
                crate::ipc::handlers::broadcast_and_persist(&state).await;
                TaskResult {
                    outcome: Ok(()),
                    retry: None,
                }
            }
            std::ops::ControlFlow::Break(snapshot) => TaskResult {
                outcome: Err(
                    "could not restore snapshot: admitted CPU destruction owner is unavailable; original recovery data preserved for retry",
                ),
                retry: Some(snapshot),
            },
        }
    } else {
        let guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        match crate::persistence::remove_snapshot_ordered(&state, guard).await {
            Ok(_guard) => {
                // LoadedSnapshot's Drop sends the decoded original to its
                // preadmitted CPU retirement, independently of socket reply.
                drop(snapshot);
                TaskResult {
                    outcome: Ok(()),
                    retry: None,
                }
            }
            Err(error) => {
                tracing::warn!(%error, "ordered recovery snapshot discard failed");
                TaskResult {
                    outcome: Err("could not discard stored session snapshot; original recovery data preserved for retry"),
                    retry: Some(snapshot),
                }
            }
        }
    };
    if result.outcome.is_ok() {
        // No await separates completed semantic work, reopening writes and
        // publishing its result. Aborting an earlier await leaves the fence
        // in place. Wake the existing writer without manufacturing a dirty
        // snapshot after a successful discard.
        state
            .recovery
            .preserve_original
            .store(false, Ordering::Release);
        state.snapshot_requested.notify_one();
    }
    completion.send_replace(Some(result.outcome));
    drop(frame_storage);
    result
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Poll;
    use std::time::Duration;

    use ilium_core::ROOT_ID;
    use ilium_ipc::ServerEvent;
    use tokio::sync::mpsc;

    use super::{AttachStatus, Refusal};
    use crate::config::{DetectionConfig, NotificationsConfig};
    use crate::execution::ServerExecution;
    use crate::ipc::handlers::handle_session_recovery_resolution;
    use crate::persistence;
    use crate::state::{ServerState, ServerStateOptions};
    use crate::task_guard::AbortOnDropHandle;

    fn state(directory: &tempfile::TempDir) -> Arc<ServerState> {
        let (sound_requests, _sound_receiver) = crate::sounds::test_channel(1);
        let state = Arc::new(ServerState::new(ServerStateOptions {
            session_name: "recovery-owner-test".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("session.json"),
            socket_path: directory.path().join("session.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        assert!(state
            .execution
            .set(ServerExecution::start().expect("real bank"))
            .is_ok());
        state
    }

    async fn install_saved_tree(state: &Arc<ServerState>) {
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, "saved recovery", None, None)
            .expect("rename saved tree");
        persistence::save_snapshot(state)
            .await
            .expect("saved snapshot");
        let loaded = persistence::load_snapshot_for_state(state)
            .await
            .expect("read snapshot")
            .expect("saved original");
        assert!(state.recovery.install_initial(loaded).await.is_ok());
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, "live before restore", None, None)
            .expect("rename live tree");
    }

    #[tokio::test]
    async fn lost_accepted_resolution_is_failed_and_still_protects_recovery() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let original_bytes = std::fs::read(&state.snapshot_path).expect("original disk snapshot");
        let publication_guard = state.workspace_spawn_lock.lock().await;
        let reached = state.recovery.before_restore_publication.notified();
        tokio::pin!(reached);
        reached.as_mut().enable();
        let completion = state
            .recovery
            .admit(Arc::clone(&state), true)
            .await
            .expect("real semantic task admitted");
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .expect("real restore parked before publication");
        {
            // Fault injection affects only this fixture's accepted task. It
            // exercises the owner's actual JoinError boundary, not a fake
            // completion message or a production cancellation API.
            let slot = state.recovery.slot.lock().await;
            slot.active
                .as_ref()
                .expect("tracked real task")
                .handle
                .abort();
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(5), super::wait_for_result(completion),)
                .await
                .expect("failed semantic completion")
                .is_err()
        );
        let attach_status = state.recovery.attach_status().await;
        let protected = state.recovery.is_unresolved().await;
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("retained original disk snapshot"),
            original_bytes,
        );
        assert_eq!(
            state
                .tree
                .read()
                .await
                .get(ROOT_ID)
                .expect("live root")
                .name,
            "live before restore",
        );
        drop(publication_guard);
        assert!(state.recovery.close_and_drain().await.is_err());
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("isolated disk owner shutdown");
        state.execution.get().expect("bank").request_shutdown();
        assert!(
            matches!(attach_status, AttachStatus::Failed(_)),
            "a joined failed recovery task must not report settled",
        );
        assert!(
            protected,
            "failed recovery must keep worktree pruning protection"
        );
    }

    #[tokio::test]
    async fn accepted_restore_survives_request_abort_and_drains_to_disk_before_bank_shutdown() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        assert!(state.recovery.is_unresolved().await);
        let guard = state.workspace_spawn_lock.lock().await;
        let reached = state.recovery.before_restore_publication.notified();
        tokio::pin!(reached);
        reached.as_mut().enable();
        let (direct_tx, _direct_rx) = crate::ipc::DirectEventSender::channel(1);
        let request_state = Arc::clone(&state);
        let request = tokio::spawn(async move {
            handle_session_recovery_resolution(&request_state, true, &direct_tx).await;
        });
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .expect("real restore reached the held publication fence");
        assert_eq!(
            state.tree.read().await.get(ROOT_ID).expect("root").name,
            "live before restore"
        );
        assert!(
            state.recovery.is_unresolved().await,
            "accepted restore must protect worktrees while publication is blocked"
        );
        request.abort();
        assert!(request.await.expect_err("request aborted").is_cancelled());
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        let mut drain = Box::pin(crate::drain_session_work(&state, &mut writer));
        std::future::poll_fn(|context| {
            assert!(Future::poll(drain.as_mut(), context).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
        drop(guard);
        tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .expect("semantic and durable drain")
            .expect("successful recovery and final persistence");
        assert!(!state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
        assert!(
            !state.recovery.is_unresolved().await,
            "actual recovery drain releases pruning protection"
        );
        let saved = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("disk readback")
            .expect("durable snapshot");
        assert_eq!(
            saved.tree.get(ROOT_ID).expect("saved root").name,
            "saved recovery"
        );
        assert_eq!(
            state
                .tree
                .read()
                .await
                .get(ROOT_ID)
                .expect("live root")
                .name,
            "saved recovery"
        );
    }

    #[tokio::test]
    async fn frame_refusal_preserves_original_and_duplicate_and_close_are_fenced() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let execution = state.execution.get().expect("bank");
        let quota = execution.quota_group().snapshot();
        let available = quota.limits.worker_bytes - quota.worker_bytes;
        let occupied = execution
            .client
            .try_reserve_storage(available)
            .expect("occupy root storage");
        assert!(matches!(
            state.recovery.admit(Arc::clone(&state), true).await,
            Err(Refusal::Unavailable(_))
        ));
        assert!(matches!(
            state.recovery.attach_status().await,
            AttachStatus::Pending { .. }
        ));
        drop(occupied);
        let guard = state.workspace_spawn_lock.lock().await;
        let reached = state.recovery.before_restore_publication.notified();
        tokio::pin!(reached);
        reached.as_mut().enable();
        let completion = state
            .recovery
            .admit(Arc::clone(&state), true)
            .await
            .expect("admitted");
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .expect("actual restore await");
        assert!(matches!(
            state.recovery.admit(Arc::clone(&state), false).await,
            Err(Refusal::Busy)
        ));
        let mut close = Box::pin(state.recovery.close_and_drain());
        std::future::poll_fn(|context| {
            assert!(Future::poll(close.as_mut(), context).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(matches!(
            state.recovery.admit(Arc::clone(&state), false).await,
            Err(Refusal::Closed)
        ));
        let mut second_close = Box::pin(state.recovery.close_and_drain());
        std::future::poll_fn(|context| {
            assert!(Future::poll(second_close.as_mut(), context).is_pending());
            Poll::Ready(())
        })
        .await;
        // Cancelling the first drainer cannot detach the tracked handle or
        // let the second drainer pass before restoration completes.
        drop(close);
        drop(guard);
        tokio::time::timeout(Duration::from_secs(5), second_close)
            .await
            .expect("second close joins the original restore")
            .expect("restore succeeded");
        assert!(super::wait_for_result(completion).await.is_ok());
        persistence::flush_pending_snapshot(&state).await;
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("disk shutdown");
        execution.request_shutdown();
    }

    #[tokio::test]
    async fn failed_discard_keeps_bytes_and_pending_retry_for_next_session_owner() {
        let directory = tempfile::tempdir().expect("directory");
        let first = state(&directory);
        install_saved_tree(&first).await;
        let original_bytes = std::fs::read(&first.snapshot_path).expect("original bytes");
        persistence::shutdown_snapshot_service(&first)
            .await
            .expect("close disk owner");
        let (direct_tx, mut direct_rx) = crate::ipc::DirectEventSender::channel(128);
        handle_session_recovery_resolution(&first, false, &direct_tx).await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::Error { .. })
        ));
        assert!(
            direct_rx.try_recv().is_err(),
            "failed discard cannot claim sync completion"
        );
        assert!(matches!(
            first.recovery.attach_status().await,
            AttachStatus::Pending { .. }
        ));
        assert_eq!(
            std::fs::read(&first.snapshot_path).expect("retained bytes"),
            original_bytes
        );
        assert!(first.recovery.close_and_drain().await.is_err());
        first.execution.get().expect("bank").request_shutdown();
        drop(first);

        let next = state(&directory);
        let loaded = persistence::load_snapshot_for_state(&next)
            .await
            .expect("new owner reads original")
            .expect("unchanged recovery original");
        assert!(next.recovery.install_initial(loaded).await.is_ok());
        let completion = next
            .recovery
            .admit(Arc::clone(&next), false)
            .await
            .expect("retry admitted");
        assert!(super::wait_for_result(completion).await.is_ok());
        next.recovery
            .close_and_drain()
            .await
            .expect("discard succeeded");
        assert!(!next.snapshot_path.exists());
        persistence::shutdown_snapshot_service(&next)
            .await
            .expect("disk shutdown");
        next.execution.get().expect("bank").request_shutdown();
    }

    #[tokio::test]
    async fn slow_and_closed_direct_replies_do_not_hold_the_semantic_owner() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let (direct_tx, mut direct_rx) = crate::ipc::DirectEventSender::channel(1);
        let request_state = Arc::clone(&state);
        let request = tokio::spawn(async move {
            handle_session_recovery_resolution(&request_state, true, &direct_tx).await;
        });
        let first = tokio::time::timeout(Duration::from_secs(5), direct_rx.recv())
            .await
            .expect("semantic result reached direct reply")
            .expect("first state event");
        assert!(matches!(first, ServerEvent::PaneStateSnapshot { .. }));
        tokio::time::timeout(Duration::from_secs(5), state.recovery.close_and_drain())
            .await
            .expect("owner drains while direct response is backpressured")
            .expect("restore succeeded");
        assert!(
            !request.is_finished(),
            "request is still sending direct state"
        );
        drop(direct_rx);
        tokio::time::timeout(Duration::from_secs(5), request)
            .await
            .expect("closed reply ends request")
            .expect("request task");
        persistence::flush_pending_snapshot(&state).await;
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("disk shutdown");
        state.execution.get().expect("bank").request_shutdown();
    }

    async fn abort_resolution(state: &Arc<ServerState>, completion: super::Completion) {
        {
            let slot = state.recovery.slot.lock().await;
            slot.active.as_ref().expect("accepted task").handle.abort();
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(5), super::wait_for_result(completion))
                .await
                .expect("failed completion")
                .is_err()
        );
        assert!(matches!(
            state.recovery.attach_status().await,
            AttachStatus::Failed(_)
        ));
    }

    fn progress_fixture() -> persistence::PersistedProgressMonitor {
        persistence::PersistedProgressMonitor {
            pane_id: ROOT_ID,
            command: "synthetic-recovery-probe".to_string(),
            interval_seconds: 15,
            latest_progress: ilium_core::PaneProgress::new(
                41,
                ilium_core::ProgressTaskReport::new(
                    "synthetic-recovery-failure".to_string(),
                    ilium_core::ProgressTaskStatus::Running,
                    50.0,
                    "synthetic recovery fixture".to_string(),
                    String::new(),
                    None,
                )
                .expect("report"),
                1_700_000_000_000,
            )
            .expect("progress"),
            result_delivery: persistence::PersistedProgressDeliveryState::NotQueued,
        }
    }

    #[tokio::test]
    async fn failed_restore_blocks_all_snapshot_replacements_and_reports_shutdown_failure() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let original_bytes = std::fs::read(&state.snapshot_path).expect("original bytes");
        let publication_guard = state.workspace_spawn_lock.lock().await;
        let reached = state.recovery.before_restore_publication.notified();
        tokio::pin!(reached);
        reached.as_mut().enable();
        let completion = state
            .recovery
            .admit(Arc::clone(&state), true)
            .await
            .expect("admitted");
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .expect("before publication");
        abort_resolution(&state, completion).await;
        drop(publication_guard);
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, "live mutation after failure", None, None)
            .expect("mutation");
        state.request_snapshot_save();
        persistence::flush_pending_snapshot(&state).await;
        assert!(
            state.is_snapshot_dirty(),
            "blocked background save must retain its claim"
        );
        assert!(persistence::save_snapshot(&state).await.is_err());
        assert!(persistence::await_snapshot_durability_barrier(&state)
            .await
            .is_err());
        assert!(persistence::await_progress_monitor_durability_barrier(
            &state,
            &progress_fixture()
        )
        .await
        .is_err());
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("original retained"),
            original_bytes
        );
        assert!(matches!(
            state.recovery.admit(Arc::clone(&state), false).await,
            Err(Refusal::Failed(_))
        ));
        let (direct_tx, mut direct_rx) = crate::ipc::DirectEventSender::channel(8);
        assert!(
            !crate::ipc::handlers::handle_request(
                &state,
                ilium_ipc::ClientRequest::KillSession,
                &direct_tx,
            )
            .await,
            "failed kill must keep the requesting connection open for its error"
        );
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::Error { .. })
        ));
        assert!(
            direct_rx.try_recv().is_err(),
            "no success synchronization on failure"
        );
        assert!(!state.is_session_killed());
        assert_eq!(
            state.tree.read().await.get(ROOT_ID).expect("root").name,
            "live mutation after failure"
        );
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        assert!(tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer)
        )
        .await
        .expect("drain completes cleanup")
        .is_err());
        assert!(
            state.recovery.close_and_drain().await.is_err(),
            "failure survives repeated drains"
        );
        assert!(
            state.recovery.is_unresolved().await,
            "pruning stays protected after retirement"
        );
        assert!(matches!(
            state.recovery.attach_status().await,
            AttachStatus::Failed(_)
        ));
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("disk after shutdown"),
            original_bytes
        );
        assert!(!state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
    }

    #[tokio::test]
    async fn failure_after_tree_publication_preserves_disk_without_claiming_live_rollback() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let original_bytes = std::fs::read(&state.snapshot_path).expect("original bytes");
        // This existing lock is acquired immediately after tree publication.
        // Holding it forces a real pending restore after its first live commit.
        let cache_guard = state.workspace_git_status_cache.write().await;
        let reached = state.recovery.after_restore_publication.notified();
        tokio::pin!(reached);
        reached.as_mut().enable();
        let completion = state
            .recovery
            .admit(Arc::clone(&state), true)
            .await
            .expect("admitted");
        tokio::time::timeout(Duration::from_secs(5), reached)
            .await
            .expect("tree published");
        assert_eq!(
            state.tree.read().await.get(ROOT_ID).expect("root").name,
            "saved recovery"
        );
        abort_resolution(&state, completion).await;
        drop(cache_guard);
        // A later ordinary mutation cannot replace the original with partial
        // recovery state, and it is not silently rolled back either.
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, "partial live state", None, None)
            .expect("mutation");
        state.request_snapshot_save();
        assert!(persistence::save_snapshot(&state).await.is_err());
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        assert!(tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer)
        )
        .await
        .expect("failed restore still drains")
        .is_err());
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("original after partial restore"),
            original_bytes
        );
        assert_eq!(
            state.tree.read().await.get(ROOT_ID).expect("root").name,
            "partial live state"
        );
        assert!(state.recovery.is_unresolved().await);
        assert!(matches!(
            state.recovery.attach_status().await,
            AttachStatus::Failed(_)
        ));
    }

    #[tokio::test]
    async fn pending_recovery_keeps_dirty_claim_then_successful_discard_reopens_saves() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        let original_bytes = std::fs::read(&state.snapshot_path).expect("original bytes");
        state.request_snapshot_save();
        persistence::flush_pending_snapshot(&state).await;
        assert!(state.is_snapshot_dirty());
        assert!(persistence::save_snapshot(&state).await.is_err());
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("pending original"),
            original_bytes
        );
        let completion = state
            .recovery
            .admit(Arc::clone(&state), false)
            .await
            .expect("discard");
        super::wait_for_result(completion)
            .await
            .expect("discard completed");
        state
            .recovery
            .close_and_drain()
            .await
            .expect("successful discard drain");
        assert!(!state.recovery.preserves_original());
        persistence::try_flush_pending_snapshot(&state)
            .await
            .expect("live dirty claim writes");
        assert!(!state.is_snapshot_dirty());
        let saved = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("readback")
            .expect("saved live snapshot");
        assert_eq!(
            saved.tree.get(ROOT_ID).expect("root").name,
            "live before restore"
        );
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("shutdown");
        state.execution.get().expect("bank").request_shutdown();
    }

    #[tokio::test]
    async fn clean_discard_shutdown_does_not_recreate_original_snapshot() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = state(&directory);
        install_saved_tree(&state).await;
        assert!(
            !state.is_snapshot_dirty(),
            "fixture has no later live mutation"
        );
        let completion = state
            .recovery
            .admit(Arc::clone(&state), false)
            .await
            .expect("actual discard accepted");
        super::wait_for_result(completion)
            .await
            .expect("ordered native discard acknowledged");
        assert!(!state.snapshot_path.exists());
        assert!(!state.is_snapshot_dirty());
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer),
        )
        .await
        .expect("actual shutdown finishes")
        .expect("clean shutdown succeeds");
        let _ = writer.settle().await;
        assert!(
            !state.snapshot_path.exists(),
            "clean discard shutdown must not recreate the removed snapshot"
        );
        assert!(!state.is_snapshot_dirty());
        assert!(!state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
    }

    #[tokio::test]
    async fn clean_start_fresh_shutdown_does_not_create_snapshot() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = state(&directory);
        assert!(!state.is_snapshot_dirty());
        assert!(!state.snapshot_path.exists());
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer),
        )
        .await
        .expect("actual shutdown finishes")
        .expect("clean shutdown succeeds");
        let _ = writer.settle().await;
        assert!(
            !state.snapshot_path.exists(),
            "clean startup shutdown must not manufacture a recovery snapshot"
        );
        assert!(!state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
    }

    #[tokio::test]
    async fn final_snapshot_failure_is_returned_after_bank_cleanup() {
        let directory = tempfile::tempdir().expect("directory");
        let state = state(&directory);
        // Native SnapshotIo rejects attempts to atomically replace a directory
        // with a regular snapshot file. No recovery decision is involved.
        std::fs::create_dir(&state.snapshot_path).expect("unwritable snapshot target");
        state.request_snapshot_save();
        let mut writer =
            AbortOnDropHandle::new(persistence::spawn_snapshot_writer(Arc::clone(&state)));
        assert!(tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer)
        )
        .await
        .expect("cleanup despite failed write")
        .is_err());
        assert!(state.snapshot_path.is_dir());
        assert!(
            state.is_snapshot_dirty(),
            "failed final write cannot claim durability"
        );
        assert!(!state
            .execution
            .get()
            .expect("bank")
            .client
            .foundation
            .is_open());
    }
}
