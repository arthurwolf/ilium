//! Session-owned ordered snapshot disk service. Only this OS thread serializes
//! and touches snapshot files; a cancelled async waiter cannot release a live
//! write guard before its admitted disk operation finishes.
use crate::{
    error::{ServerError, SnapshotError},
    persistence::{self, SessionSnapshot},
};
use ilium_platform::owned_worker::{spawn_owned, OwnedWorker, StopToken, WorkerKind};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{oneshot, OwnedMutexGuard};

pub(crate) const MAX_SNAPSHOT_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_COMMANDS: usize = 8;

pub(crate) fn error(
    path: &Path,
    operation: &'static str,
    message: impl Into<String>,
) -> ServerError {
    ServerError::Snapshot {
        operation,
        path: path.to_owned(),
        source: SnapshotError::Io(io::Error::other(message.into())),
    }
}
struct Budget {
    bytes: AtomicUsize,
}
struct Reservation {
    bytes: usize,
    budget: Arc<Budget>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
struct Completion<T> {
    result: Result<T, ServerError>,
    _reservation: Reservation,
}
enum Command {
    CaptureWrite {
        sources: persistence::SnapshotSources,
        runtime: tokio::runtime::Handle,
        progress_override: Option<persistence::PersistedProgressMonitor>,
        guard: OwnedMutexGuard<()>,
        ack: oneshot::Sender<Completion<OwnedMutexGuard<()>>>,
        reservation: Reservation,
    },
    #[cfg(test)]
    Write {
        snapshot: Box<SessionSnapshot>,
        guard: Option<OwnedMutexGuard<()>>,
        ack: oneshot::Sender<Completion<Option<OwnedMutexGuard<()>>>>,
        reservation: Reservation,
    },
    Read {
        migration: Option<(PathBuf, PathBuf)>,
        ack: oneshot::Sender<Completion<Option<SessionSnapshot>>>,
        reservation: Reservation,
    },
    Remove {
        guard: OwnedMutexGuard<()>,
        ack: oneshot::Sender<Completion<OwnedMutexGuard<()>>>,
        reservation: Reservation,
    },
    Flush {
        shutdown: bool,
        ack: oneshot::Sender<Completion<()>>,
        reservation: Reservation,
    },
}
struct Admission {
    closed: bool,
}
pub(crate) struct SnapshotIo {
    path: PathBuf,
    sender: SyncSender<Command>,
    admission: Mutex<Admission>,
    budget: Arc<Budget>,
    _worker: OwnedWorker,
}
impl SnapshotIo {
    pub(crate) fn new(path: PathBuf) -> io::Result<Self> {
        Self::start(path, || {})
    }
    fn start(path: PathBuf, before_run: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_COMMANDS);
        let worker_path = path.clone();
        let worker = spawn_owned(
            "ilium-snapshot-io",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            || {},
            move |stop| {
                before_run();
                let mut write_failed = false;
                while !stop.is_stopped() {
                    let command = match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    match command {
                        Command::CaptureWrite {
                            sources,
                            runtime,
                            progress_override,
                            guard,
                            ack,
                            reservation,
                        } => {
                            let result = runtime
                                .block_on(persistence::capture_snapshot(
                                    &sources,
                                    &worker_path,
                                    progress_override.as_ref(),
                                ))
                                .and_then(|snapshot| {
                                    if estimated_bytes(&snapshot) > MAX_SNAPSHOT_RETAINED_BYTES {
                                        return Err(error(
                                            &worker_path,
                                            "capture admission",
                                            "captured snapshot exceeds retained-memory limit",
                                        ));
                                    }
                                    persistence::write_snapshot_blocking(&worker_path, &snapshot)
                                })
                                .map(|()| guard);
                            write_failed |= result.is_err();
                            let _ = ack.send(Completion {
                                result,
                                _reservation: reservation,
                            });
                        }
                        #[cfg(test)]
                        Command::Write {
                            snapshot,
                            guard,
                            ack,
                            reservation,
                        } => {
                            let result =
                                persistence::write_snapshot_blocking(&worker_path, &snapshot)
                                    .map(|()| guard);
                            write_failed |= result.is_err();
                            drop(snapshot);
                            let _ = ack.send(Completion {
                                result,
                                _reservation: reservation,
                            });
                        }
                        Command::Read {
                            migration,
                            ack,
                            reservation,
                        } => {
                            let result = match migration {
                                Some((cwd, home)) => {
                                    persistence::load_snapshot_or_migrate_blocking(
                                        &worker_path,
                                        &cwd,
                                        &home,
                                    )
                                }
                                None => persistence::load_snapshot_blocking(&worker_path),
                            }
                            .and_then(|snapshot| {
                                if snapshot.as_ref().is_some_and(|snapshot| {
                                    estimated_bytes(snapshot) > MAX_SNAPSHOT_RETAINED_BYTES
                                }) {
                                    return Err(error(
                                        &worker_path,
                                        "load admission",
                                        "decoded snapshot exceeds retained-memory limit",
                                    ));
                                }
                                Ok(snapshot)
                            });
                            let _ = ack.send(Completion {
                                result,
                                _reservation: reservation,
                            });
                        }
                        Command::Remove {
                            guard,
                            ack,
                            reservation,
                        } => {
                            let result = match std::fs::remove_file(&worker_path) {
                                Ok(()) => Ok(guard),
                                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                                    Ok(guard)
                                }
                                Err(source) => Err(ServerError::Snapshot {
                                    operation: "remove",
                                    path: worker_path.clone(),
                                    source: SnapshotError::Io(source),
                                }),
                            };
                            write_failed |= result.is_err();
                            let _ = ack.send(Completion {
                                result,
                                _reservation: reservation,
                            });
                        }
                        Command::Flush {
                            shutdown,
                            ack,
                            reservation,
                        } => {
                            // Each preceding write already syncs/closes before rename.
                            let _ = ack.send(Completion {
                                result: if write_failed {
                                    Err(error(
                                        &worker_path,
                                        "flush",
                                        "an earlier ordered snapshot write or removal failed",
                                    ))
                                } else {
                                    Ok(())
                                },
                                _reservation: reservation,
                            });
                            if shutdown {
                                break;
                            }
                        }
                    }
                }
                // Dropping the receiver cancels queued jobs, dropping their guards
                // and reservations only after any active disk operation completed.
            },
        )?;
        Ok(Self {
            path,
            sender,
            admission: Mutex::new(Admission { closed: false }),
            budget: Arc::new(Budget {
                bytes: AtomicUsize::new(0),
            }),
            _worker: worker,
        })
    }
    fn reserve(&self, bytes: usize) -> Result<Reservation, ServerError> {
        self.budget
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|total| *total <= MAX_SNAPSHOT_RETAINED_BYTES)
            })
            .map_err(|_| {
                error(
                    &self.path,
                    "admission",
                    "snapshot retained-memory limit exhausted",
                )
            })?;
        Ok(Reservation {
            bytes,
            budget: Arc::clone(&self.budget),
        })
    }
    fn admit(&self, command: Command, shutdown: bool) -> Result<(), ServerError> {
        // Bookkeeping only: no filesystem operation or serialization under this lock.
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if admission.closed {
            return Err(error(
                &self.path,
                "admission",
                "snapshot service is shut down",
            ));
        }
        match self.sender.try_send(command) {
            Ok(()) => {
                admission.closed |= shutdown;
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(error(
                &self.path,
                "admission",
                "snapshot command capacity exhausted",
            )),
            Err(TrySendError::Disconnected(_)) => {
                Err(error(&self.path, "admission", "snapshot worker stopped"))
            }
        }
    }
    async fn receive<T>(
        &self,
        receiver: oneshot::Receiver<Completion<T>>,
    ) -> Result<T, ServerError> {
        receiver
            .await
            .map_err(|_| {
                error(
                    &self.path,
                    "acknowledgement",
                    "snapshot worker stopped before completing the operation",
                )
            })?
            .result
    }
    pub(crate) async fn capture_write(
        &self,
        sources: persistence::SnapshotSources,
        progress_override: Option<&persistence::PersistedProgressMonitor>,
        guard: OwnedMutexGuard<()>,
    ) -> Result<OwnedMutexGuard<()>, ServerError> {
        self.receive(self.enqueue_capture(sources, progress_override, guard)?)
            .await
    }
    fn enqueue_capture(
        &self,
        sources: persistence::SnapshotSources,
        progress_override: Option<&persistence::PersistedProgressMonitor>,
        guard: OwnedMutexGuard<()>,
    ) -> Result<oneshot::Receiver<Completion<OwnedMutexGuard<()>>>, ServerError> {
        if progress_override.is_some_and(|monitor| {
            persistence::estimated_monitor_bytes(monitor) > MAX_SNAPSHOT_RETAINED_BYTES
        }) {
            return Err(error(
                &self.path,
                "capture admission",
                "progress override exceeds retained-memory limit",
            ));
        }
        // Reserve before the bounded staging clone; deep session capture is
        // performed on the worker after dequeue, not during admission.
        let reservation = self.reserve(MAX_SNAPSHOT_RETAINED_BYTES)?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::CaptureWrite {
                sources,
                runtime: tokio::runtime::Handle::current(),
                progress_override: progress_override.cloned(),
                guard,
                ack,
                reservation,
            },
            false,
        )?;
        Ok(receiver)
    }
    #[cfg(test)]
    pub(crate) async fn write(
        &self,
        snapshot: SessionSnapshot,
        guard: OwnedMutexGuard<()>,
    ) -> Result<OwnedMutexGuard<()>, ServerError> {
        self.write_inner(snapshot, Some(guard))
            .await?
            .ok_or_else(|| {
                error(
                    &self.path,
                    "acknowledgement",
                    "snapshot write guard missing",
                )
            })
    }
    #[cfg(test)]
    async fn write_inner(
        &self,
        snapshot: SessionSnapshot,
        guard: Option<OwnedMutexGuard<()>>,
    ) -> Result<Option<OwnedMutexGuard<()>>, ServerError> {
        self.receive(self.enqueue_write(snapshot, guard)?).await
    }
    #[cfg(test)]
    fn enqueue_write(
        &self,
        snapshot: SessionSnapshot,
        guard: Option<OwnedMutexGuard<()>>,
    ) -> Result<oneshot::Receiver<Completion<Option<OwnedMutexGuard<()>>>>, ServerError> {
        let reservation = self.reserve(estimated_bytes(&snapshot))?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::Write {
                snapshot: Box::new(snapshot),
                guard,
                ack,
                reservation,
            },
            false,
        )?;
        Ok(receiver)
    }
    #[cfg(test)]
    pub(crate) async fn write_boot(&self, snapshot: SessionSnapshot) -> Result<(), ServerError> {
        self.write_inner(snapshot, None).await.map(|_| ())
    }
    pub(crate) async fn read(
        &self,
        migration: Option<(PathBuf, PathBuf)>,
    ) -> Result<Option<SessionSnapshot>, ServerError> {
        // Decode can expand strings/containers beyond encoded size. Reserve the
        // full service budget for its sole result until the receiver consumes it.
        let reservation = self.reserve(MAX_SNAPSHOT_RETAINED_BYTES)?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::Read {
                migration,
                ack,
                reservation,
            },
            false,
        )?;
        self.receive(receiver).await
    }
    pub(crate) async fn remove(
        &self,
        guard: OwnedMutexGuard<()>,
    ) -> Result<OwnedMutexGuard<()>, ServerError> {
        let reservation = self.reserve(0)?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::Remove {
                guard,
                ack,
                reservation,
            },
            false,
        )?;
        self.receive(receiver).await
    }
    pub(crate) async fn shutdown(&self) -> Result<(), ServerError> {
        let reservation = self.reserve(0)?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::Flush {
                shutdown: true,
                ack,
                reservation,
            },
            true,
        )?;
        self.receive(receiver).await
    }
}

/// Cheap allocation-capacity walk: no JSON encoding or traversal of string
/// contents. Fixed per-object margins account for private tree map buckets.
fn estimated_bytes(snapshot: &SessionSnapshot) -> usize {
    use crate::pane::{PaneSnapshotKind, TerminalOrigin};
    // HashMap's logical capacity excludes spare control bytes and rounding.
    // Two node-sized slots plus a control margin conservatively charge each
    // retained slot, including sparse capacity after removals. This is a
    // retained-resource estimate, not a measurement of allocator RSS.
    let mut bytes =
        std::mem::size_of::<SessionSnapshot>().saturating_add(estimated_tree_bytes(&snapshot.tree));
    let mut add = |amount: usize| {
        bytes = bytes.saturating_add(amount);
    };
    let optional = |text: &Option<String>| text.as_ref().map_or(0, String::capacity);
    let path_size = |path: &PathBuf| path.capacity().saturating_mul(2);
    let progress_size = estimated_progress_bytes;
    add(snapshot.panes.capacity() * std::mem::size_of::<persistence::PaneSnapshot>());
    for pane in &snapshot.panes {
        match &pane.kind {
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) => add(command.capacity()),
            PaneSnapshotKind::Editor { path: Some(path) } => add(path_size(path)),
            _ => {}
        }
    }
    add(snapshot.agent_debug_logs.capacity()
        * std::mem::size_of::<crate::agent_debug::PaneDebugLogSnapshot>());
    for pane in &snapshot.agent_debug_logs {
        add(estimated_debug_log_bytes(&pane.log));
    }
    add(snapshot.progress_monitors.capacity()
        * std::mem::size_of::<persistence::PersistedProgressMonitor>());
    for monitor in &snapshot.progress_monitors {
        add(monitor.command.capacity());
        add(progress_size(&monitor.latest_progress));
    }
    add(snapshot.workspace_close_preferences.capacity()
        * std::mem::size_of::<persistence::PersistedWorkspaceClosePreference>());
    for preference in &snapshot.workspace_close_preferences {
        add(optional(&preference.workspace_id));
        add(path_size(&preference.worktree_root));
    }
    bytes
}

pub(crate) fn estimated_tree_bytes(tree: &ilium_core::Tree) -> usize {
    use ilium_core::{AgentClass, ContainerKind, NodeKind, PaneStatus};
    let mut bytes = std::mem::size_of::<ilium_core::Tree>().saturating_add(
        tree.retained_node_capacity()
            .saturating_mul(std::mem::size_of::<ilium_core::Node>() * 2 + 32),
    );
    let mut add = |amount: usize| {
        bytes = bytes.saturating_add(amount);
    };
    let optional = |text: &Option<String>| text.as_ref().map_or(0, String::capacity);
    let path_size = |path: &PathBuf| path.capacity().saturating_mul(2);
    let class_size = |class: &AgentClass| match class {
        AgentClass::Other(value) => value.capacity(),
        _ => 0,
    };
    let progress_size = |progress: &ilium_core::PaneProgress| {
        progress
            .report
            .job_id
            .capacity()
            .saturating_add(progress.report.message.capacity())
            .saturating_add(optional(&progress.report.error))
            .saturating_add(512)
    };
    for node_id in tree.all_ids() {
        let Some(node) = tree.get(node_id) else {
            continue;
        };
        add(node.name.capacity());
        add(optional(&node.short_name));
        add(optional(&node.inferred_icon));
        if let Some(animation) = &node.inferred_animation {
            add(animation.kind.capacity());
            add(animation.parameters.capacity()
                * std::mem::size_of::<ilium_core::animation_recommendation::AnimationParameter>());
            for parameter in &animation.parameters {
                add(parameter.id.capacity());
                if let ilium_core::animation_recommendation::AnimationValue::Choice {
                    label, ..
                } = &parameter.value
                {
                    add(label.capacity());
                }
            }
        }
        match &node.kind {
            NodeKind::Container(container) => {
                add(container.children.capacity() * std::mem::size_of::<ilium_core::NodeId>());
                if let ContainerKind::Project { path } = &container.kind {
                    add(path_size(path));
                }
            }
            NodeKind::Folder { path, .. } => add(path_size(path)),
            NodeKind::Pane {
                status,
                board_storage,
                scheduled_input,
                prompt_queue,
                last_prompt,
                progress,
                launch_cwd,
                workspace,
                ..
            } => {
                match status {
                    PaneStatus::Agent(state) => add(class_size(&state.class)),
                    PaneStatus::AgentUnavailable(recovery) => {
                        add(std::mem::size_of_val(recovery.as_ref()));
                        add(class_size(&recovery.last_known_state.class));
                        add(optional(&recovery.signal_name));
                        add(optional(&recovery.session_id));
                        add(optional(&recovery.last_prompt));
                        add(optional(&recovery.previous_exact_prompt));
                    }
                    _ => {}
                }
                if let Some(storage) = board_storage {
                    add(storage.path().as_os_str().len().saturating_mul(2));
                }
                if let Some(input) = scheduled_input {
                    add(input.text.capacity());
                }
                add(prompt_queue.capacity() * std::mem::size_of::<ilium_core::QueuedPrompt>());
                for prompt in prompt_queue {
                    add(prompt.text.capacity());
                }
                add(optional(last_prompt));
                if let Some(progress) = progress {
                    add(progress_size(progress));
                }
                if let Some(path) = launch_cwd {
                    add(path_size(path));
                }
                if let Some(workspace) = workspace {
                    add(std::mem::size_of_val(workspace.as_ref()));
                    add(optional(&workspace.workspace_id));
                    add(path_size(&workspace.repo_common_dir));
                    add(path_size(&workspace.worktree_root));
                    add(workspace.branch.capacity());
                    add(workspace.base_ref.capacity());
                    add(workspace.base_commit.capacity());
                }
            }
        }
    }
    bytes
}
pub(crate) fn estimated_progress_bytes(progress: &ilium_core::PaneProgress) -> usize {
    progress
        .report
        .job_id
        .capacity()
        .saturating_add(progress.report.message.capacity())
        .saturating_add(progress.report.error.as_ref().map_or(0, String::capacity))
        .saturating_add(512)
}
pub(crate) fn estimated_debug_log_bytes(log: &ilium_agent_debug::PaneDebugLog) -> usize {
    let mut bytes = log
        .entries
        .capacity()
        .saturating_mul(std::mem::size_of::<ilium_agent_debug::AgentDebugEntry>());
    for entry in &log.entries {
        for amount in [
            entry.summary.capacity(),
            entry.correlation_id.as_ref().map_or(0, String::capacity),
            entry
                .context
                .session_id
                .as_ref()
                .map_or(0, String::capacity),
            entry
                .fields
                .capacity()
                .saturating_mul(std::mem::size_of::<ilium_agent_debug::AgentDebugField>()),
        ] {
            bytes = bytes.saturating_add(amount);
        }
        if let Some(ilium_core::AgentClass::Other(class)) = &entry.context.class {
            bytes = bytes.saturating_add(class.capacity());
        }
        for field in &entry.fields {
            bytes = bytes
                .saturating_add(field.label.capacity())
                .saturating_add(field.value.capacity());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{Tree, ROOT_ID};
    use std::time::Instant;

    fn snapshot(version: u32) -> SessionSnapshot {
        SessionSnapshot {
            version,
            tree: Tree::new(),
            panes: Vec::new(),
            agent_debug_logs: Vec::new(),
            progress_monitors: Vec::new(),
            workspace_close_preferences: Vec::new(),
        }
    }
    fn parked(
        path: PathBuf,
    ) -> (
        SnapshotIo,
        std::sync::mpsc::Sender<()>,
        std::thread::ThreadId,
    ) {
        let (release, gate) = mpsc::channel();
        let (started, identity) = mpsc::channel();
        let service = SnapshotIo::start(path, move || {
            started.send(std::thread::current().id()).expect("identity");
            gate.recv().expect("release");
        })
        .expect("worker");
        let worker = identity
            .recv_timeout(Duration::from_secs(5))
            .expect("started");
        (service, release, worker)
    }

    #[tokio::test]
    async fn snapshot_worker_orders_real_atomic_writes_and_shutdown_acknowledges_both() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("ordered.json");
        let (service, release, worker) = parked(path.clone());
        assert_ne!(worker, std::thread::current().id());
        let first = service
            .enqueue_write(snapshot(10), None)
            .expect("first admission");
        let second = service
            .enqueue_write(snapshot(20), None)
            .expect("second admission");
        release.send(()).expect("release");
        service.receive(first).await.expect("first ack");
        service.receive(second).await.expect("second ack");
        service.shutdown().await.expect("shutdown");
        let loaded = persistence::load_snapshot_blocking(&path)
            .expect("readback")
            .expect("snapshot");
        assert_eq!(loaded.version, 20);
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("directory")
                .count(),
            1
        );
        assert_eq!(service.budget.bytes.load(Ordering::Acquire), 0);
        assert!(service.write_boot(snapshot(30)).await.is_err());
    }

    #[tokio::test]
    async fn cancelling_snapshot_waiter_keeps_write_guard_until_disk_job_finishes() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("cancel.json");
        let (service, release, _) = parked(path.clone());
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let guard = Arc::clone(&lock).lock_owned().await;
        let receiver = service
            .enqueue_write(snapshot(31), Some(guard))
            .expect("admission");
        drop(receiver); // Exactly the receiver cancellation performed by a dropped async future.
        assert!(
            lock.try_lock().is_err(),
            "queued disk owner retains cancellation fence"
        );
        release.send(()).expect("release");
        let guard = tokio::time::timeout(Duration::from_secs(5), Arc::clone(&lock).lock_owned())
            .await
            .expect("write finished");
        assert_eq!(
            persistence::load_snapshot_blocking(&path)
                .expect("read")
                .expect("snapshot")
                .version,
            31
        );
        let guard = service.remove(guard).await.expect("ordered removal");
        assert!(!path.exists());
        drop(guard);
        service.shutdown().await.expect("shutdown");
        assert!(
            !path.exists(),
            "completed prior writes cannot recreate removed snapshot"
        );
    }

    #[tokio::test]
    async fn progress_style_ack_returns_write_guard_for_subsequent_live_commit() {
        let directory = tempfile::tempdir().expect("directory");
        let service = SnapshotIo::new(directory.path().join("barrier.json")).expect("worker");
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let guard = service
            .write(snapshot(40), Arc::clone(&lock).lock_owned().await)
            .await
            .expect("durability ack");
        assert!(
            lock.try_lock().is_err(),
            "barrier guard remains held through live commit"
        );
        drop(guard);
        assert!(lock.try_lock().is_ok());
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn rejected_snapshot_capacity_and_file_error_leave_original_bytes_untouched() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("snapshot.json");
        std::fs::write(&path, b"original").expect("fixture");
        let service = SnapshotIo::new(path.clone()).expect("worker");
        let mut oversized = snapshot(50);
        let mut name = String::with_capacity(MAX_SNAPSHOT_RETAINED_BYTES + 1);
        name.push('x');
        oversized
            .tree
            .rename_node(ROOT_ID, name, None, None)
            .expect("name");
        assert!(service.write_boot(oversized).await.is_err());
        assert_eq!(std::fs::read(&path).expect("unchanged"), b"original");
        service.shutdown().await.expect("shutdown");
        let failing = SnapshotIo::new(path.join("not-a-directory.json")).expect("worker");
        assert!(failing.write_boot(snapshot(51)).await.is_err());
        assert!(
            failing.shutdown().await.is_err(),
            "shutdown cannot conceal an earlier write failure"
        );
        assert_eq!(
            std::fs::read(path).expect("unchanged after I/O failure"),
            b"original"
        );
    }

    #[tokio::test]
    async fn oversized_encoded_load_is_rejected_before_reading_its_body() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("large.json");
        std::fs::File::create(&path)
            .expect("fixture")
            .set_len(persistence::MAX_ENCODED_SNAPSHOT_BYTES + 1)
            .expect("sparse length");
        let service = SnapshotIo::new(path).expect("worker");
        assert!(service.read(None).await.is_err());
        service.shutdown().await.expect("shutdown");
    }

    fn capture_state(directory: &Path) -> crate::state::ServerState {
        use crate::{
            config::{DetectionConfig, NotificationsConfig},
            state::{ServerState, ServerStateOptions},
        };
        let (sound_requests, _) = tokio::sync::mpsc::channel(1);
        ServerState::new(ServerStateOptions {
            session_name: "capture-test".into(),
            session_cwd: directory.into(),
            home_dir: directory.into(),
            snapshot_path: directory.join("capture.json"),
            socket_path: directory.join("isolated.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: true,
            progress_monitor_enabled: true,
        })
    }

    #[tokio::test]
    async fn queued_capture_holds_no_broad_locks_and_cancellation_retains_durability_guard() {
        let directory = tempfile::tempdir().expect("directory");
        let state = capture_state(directory.path());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let caller_thread = std::thread::current().id();
        let service = SnapshotIo::start(state.snapshot_path.clone(), move || {
            let _ = started_tx.send(std::thread::current().id());
            release_rx.recv().expect("release fixture");
        })
        .expect("worker");
        assert_ne!(started_rx.await.expect("started"), caller_thread);
        let receiver = service
            .enqueue_capture(
                persistence::SnapshotSources::new(&state),
                None,
                Arc::clone(&state.snapshot_write_lock).lock_owned().await,
            )
            .expect("capture admission");
        // A stopped disk owner cannot make a queued capture monopolize state.
        state
            .tree
            .try_write()
            .expect("tree remains writable")
            .rename_node(ROOT_ID, "latest before dequeue", None, None)
            .expect("rename");
        drop(state.panes.try_write().expect("panes remain writable"));
        drop(
            state
                .workspace_close_preferences
                .try_write()
                .expect("preferences remain writable"),
        );
        drop(receiver);
        assert!(state.snapshot_write_lock.try_lock().is_err());
        release_tx.send(()).expect("release");
        // Wait for the underlying capture/write, even though its waiter vanished.
        let guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        let persisted = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("readback")
            .expect("snapshot");
        assert_eq!(
            persisted.tree.get(ROOT_ID).expect("root").name,
            "latest before dequeue"
        );
        let guard = service.remove(guard).await.expect("ordered remove");
        drop(guard);
        service.shutdown().await.expect("shutdown");
        assert!(!state.snapshot_path.exists());
    }

    #[tokio::test]
    async fn capture_rejects_oversized_source_before_clone_and_keeps_previous_file() {
        let directory = tempfile::tempdir().expect("directory");
        let state = capture_state(directory.path());
        std::fs::write(&state.snapshot_path, b"previous snapshot").expect("fixture");
        let mut name = String::with_capacity(MAX_SNAPSHOT_RETAINED_BYTES + 1);
        name.push('x');
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, name, None, None)
            .expect("rename");
        assert!(persistence::save_snapshot(&state).await.is_err());
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("readback"),
            b"previous snapshot"
        );
        assert!(state.snapshot_write_lock.try_lock().is_ok());
        // Explicit failed capture is visible at drain rather than fake durability.
        assert!(persistence::shutdown_snapshot_service(&state)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn killed_session_rejects_later_capture_and_cannot_recreate_removed_snapshot() {
        use crate::{
            config::{DetectionConfig, NotificationsConfig},
            state::{ServerState, ServerStateOptions},
        };
        let directory = tempfile::tempdir().expect("directory");
        let (sound_requests, _sound_receiver) = tokio::sync::mpsc::channel(1);
        let state = ServerState::new(ServerStateOptions {
            session_name: "snapshot-kill-test".into(),
            session_cwd: directory.path().to_owned(),
            home_dir: directory.path().to_owned(),
            snapshot_path: directory.path().join("killed.json"),
            socket_path: directory.path().join("isolated.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        });
        persistence::save_snapshot(&state)
            .await
            .expect("initial durable save");
        assert!(state.snapshot_path.exists());
        state.mark_session_killed();
        let guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        let guard = persistence::remove_snapshot_ordered(&state, guard)
            .await
            .expect("ordered remove");
        drop(guard);
        assert!(persistence::save_snapshot(&state).await.is_err());
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("drain");
        assert!(!state.snapshot_path.exists());
    }

    #[test]
    fn queued_snapshots_and_control_commands_are_bounded_and_drop_does_not_join() {
        let directory = tempfile::tempdir().expect("directory");
        let (service, release, _) = parked(directory.path().join("bounded.json"));
        let mut acknowledgements = Vec::new();
        for index in 0..MAX_COMMANDS {
            acknowledgements.push(
                service
                    .enqueue_write(snapshot(index as u32), None)
                    .expect("admission"),
            );
        }
        assert!(service.enqueue_write(snapshot(99), None).is_err());
        assert!(service.budget.bytes.load(Ordering::Acquire) <= MAX_SNAPSHOT_RETAINED_BYTES);
        let ticket = service._worker.ticket();
        let started = Instant::now();
        drop(service);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(ticket.join_until(Instant::now()).is_err());
        release.send(()).expect("release");
        assert!(ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .is_ok());
        assert!(acknowledgements.into_iter().all(|mut receiver| matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        )));
    }
}
