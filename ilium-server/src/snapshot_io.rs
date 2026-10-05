//! Session-owned ordered snapshot disk service. Only this OS thread serializes
//! and touches snapshot files; a cancelled async waiter cannot release a live
//! write guard before its admitted disk operation finishes.
use crate::{
    error::{ServerError, SnapshotError},
    persistence::{self, SessionSnapshot},
};
use ilium_platform::owned_worker::{OwnedWorker, StopToken, WorkerKind};
use std::{
    io,
    ops::ControlFlow,
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
const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
// serde_json 1.0.149 IoRead pushes decoded bytes into one scratch Vec.
// Escapes cannot produce more bytes than the capped encoded input. Installed
// RawVec doubles capacity; 4x input covers old/new backing coexistence during
// growth, plus the BufReader. This does not bound Norway's eager event graph.
const MAX_NATIVE_READ_SCRATCH_BYTES: usize =
    4 * (persistence::MAX_ENCODED_SNAPSHOT_BYTES as usize + 1) + 64 * 1024;

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
pub(crate) fn admission_error(
    path: &Path,
    operation: &'static str,
    message: impl Into<String>,
) -> ServerError {
    ServerError::Snapshot {
        operation,
        path: path.to_owned(),
        source: SnapshotError::ResourceAdmission(message.into()),
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
/// A decoded result retains its independent process storage admission after
/// the operation slot is released. Pending recovery and restored state share
/// this same lease; there is no second charge for immutable custody handles.
pub(crate) struct LoadedSnapshot {
    snapshot: Option<SessionSnapshot>,
    storage: Arc<ilium_execution::StorageAdmission>,
    retirement: Option<ilium_execution::RetirementReservation<SessionSnapshot>>,
    restore_retirement:
        Option<ilium_execution::RetirementReservation<crate::SnapshotRestoreFuture>>,
}
impl std::ops::Deref for LoadedSnapshot {
    type Target = SessionSnapshot;
    fn deref(&self) -> &Self::Target {
        // Every live result owns its original until the consuming transfer.
        self.snapshot.as_ref().expect("live loaded snapshot")
    }
}
impl LoadedSnapshot {
    pub(crate) fn into_restore_future(
        mut self,
        state: Arc<crate::state::ServerState>,
    ) -> ControlFlow<Self, ilium_execution::Retiring<crate::SnapshotRestoreFuture>> {
        let Some(retirement) = self.restore_retirement.take() else {
            return ControlFlow::Break(self);
        };
        // Every allocation of the actual future frame was admitted before
        // decoding. Transfer the exact original; neither tree nor pane history
        // is cloned. The same lease covers partial state and remaining input.
        state.retain_snapshot_read_storage(Arc::clone(&self.storage));
        let snapshot = self.snapshot.take().expect("live loaded snapshot");
        let frame = crate::boxed_snapshot_restore_future(state, snapshot);
        let mut frame = retirement.attach(frame);
        frame.set_storage_guard(Arc::clone(&self.storage));
        ControlFlow::Continue(frame)
    }
    #[cfg(test)]
    pub(crate) fn into_parts(
        mut self,
    ) -> (SessionSnapshot, Arc<ilium_execution::StorageAdmission>) {
        // Ownership transfer is constant work; restored state keeps the same
        // storage lease. The unused destruction envelope releases metadata.
        (
            self.snapshot.take().expect("live loaded snapshot"),
            Arc::clone(&self.storage),
        )
    }
    #[cfg(test)]
    pub(crate) fn into_test_snapshot(mut self) -> SessionSnapshot {
        self.snapshot.take().expect("live loaded snapshot")
    }
}
impl Drop for LoadedSnapshot {
    fn drop(&mut self) {
        if let Some(retirement) = self.retirement.take() {
            if let Some(snapshot) = self.snapshot.take() {
                let mut retiring = retirement.attach(snapshot);
                retiring.set_storage_guard(Arc::clone(&self.storage));
                // The preadmitted envelope owns the original through actual
                // CPU destruction, including an exceptional failed handoff.
                drop(retiring);
            }
        }
        // Only independent no-bank fixtures use direct destruction. Every
        // production constructor supplies the existing bank's retirement owner.
    }
}
enum Command {
    CaptureWrite {
        sources: persistence::SnapshotSources,
        runtime: tokio::runtime::Handle,
        // Variable-size progress metadata should not inflate every queued
        // read/flush command. Its allocation follows retained-byte admission.
        progress_override: Option<Box<persistence::PersistedProgressMonitor>>,
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
        result_storage: Arc<ilium_execution::StorageAdmission>,
        parser_storage: ilium_execution::StorageAdmission,
        retirement: Option<ilium_execution::RetirementReservation<SessionSnapshot>>,
        restore_retirement:
            Option<ilium_execution::RetirementReservation<crate::SnapshotRestoreFuture>>,
        ack: oneshot::Sender<Completion<Option<LoadedSnapshot>>>,
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
    quota: ilium_execution::QuotaGroup,
    retirement: Option<ilium_execution::RetirementHandle>,
    _worker: OwnedWorker,
}
impl SnapshotIo {
    #[cfg(test)]
    pub(crate) fn new(path: PathBuf) -> io::Result<Self> {
        Self::start(path, || {})
    }
    pub(crate) fn new_with_execution(
        path: PathBuf,
        execution: &crate::execution::ServerExecution,
    ) -> io::Result<Self> {
        Self::start_with_retirement(
            path,
            &execution.quota_group(),
            Some(execution.client.foundation.retirement()),
            || {},
        )
    }
    #[cfg(test)]
    pub(crate) fn new_with_quota(
        path: PathBuf,
        quota: &ilium_execution::QuotaGroup,
    ) -> io::Result<Self> {
        Self::start_with_quota(path, quota, || {})
    }
    #[cfg(test)]
    fn start(path: PathBuf, before_run: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        // Independent fixtures do not designate a second process supervisor.
        Self::start_with_quota(
            path,
            &crate::execution::ServerResources::new().quota_group(),
            before_run,
        )
    }
    #[cfg(test)]
    fn start_with_quota(
        path: PathBuf,
        quota: &ilium_execution::QuotaGroup,
        before_run: impl FnOnce() + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_with_retirement(path, quota, None, before_run)
    }
    fn start_with_retirement(
        path: PathBuf,
        quota: &ilium_execution::QuotaGroup,
        retirement: Option<ilium_execution::RetirementHandle>,
        before_run: impl FnOnce() + Send + 'static,
    ) -> io::Result<Self> {
        // Reserve before constructing channels/captures. Physical custody keeps
        // the role charged after logical drop until the native thread is joined.
        let resident_bytes = MAX_SNAPSHOT_RETAINED_BYTES
            .checked_add(MAX_COMMANDS * std::mem::size_of::<Command>())
            .and_then(|bytes| bytes.checked_add(path.capacity().saturating_mul(2)))
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or_else(|| io::Error::other("snapshot worker declaration overflow"))?;
        let worker_slot =
            ilium_execution::reserve_admitted_worker(quota, WORKER_STACK_BYTES, resident_bytes)
                .map_err(io::Error::other)?;
        let (sender, receiver) = mpsc::sync_channel(MAX_COMMANDS);
        let worker_path = path.clone();
        let worker = worker_slot.spawn(
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
                                    progress_override.as_deref(),
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
                            result_storage,
                            parser_storage,
                            retirement,
                            restore_retirement,
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
                                    return Err(admission_error(
                                        &worker_path,
                                        "load admission",
                                        "decoded snapshot exceeds retained-memory limit",
                                    ));
                                }
                                Ok(snapshot.map(|snapshot| LoadedSnapshot {
                                    snapshot: Some(snapshot),
                                    storage: result_storage,
                                    retirement,
                                    restore_retirement,
                                }))
                            });
                            // The decoder and its scratch have actually returned;
                            // only the independent decoded-result lease survives.
                            drop(parser_storage);
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
            quota: quota.clone(),
            retirement,
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
                admission_error(
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
            Err(TrySendError::Full(_)) => Err(admission_error(
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
                progress_override: progress_override.cloned().map(Box::new),
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
    ) -> Result<Option<LoadedSnapshot>, ServerError> {
        // Decode can expand strings/containers beyond encoded size. Reserve the
        // full service budget for its sole result until the receiver consumes it.
        let reservation = self.reserve(MAX_SNAPSHOT_RETAINED_BYTES)?;
        // This independent storage lease survives operation completion and
        // physical worker shutdown. Reserve before decoding or queueing data.
        let result_storage = Arc::new(
            self.quota
                .reserve_external_storage(MAX_SNAPSHOT_RETAINED_BYTES)
                .map_err(|reason| {
                    admission_error(&self.path, "read storage admission", format!("{reason:?}"))
                })?,
        );
        let parser_storage = self
            .quota
            .reserve_external_storage(MAX_NATIVE_READ_SCRATCH_BYTES)
            .map_err(|reason| {
                admission_error(&self.path, "read parser admission", format!("{reason:?}"))
            })?;
        let retirement = self
            .retirement
            .as_ref()
            .map(|owner| {
                owner
                    .try_reserve::<SessionSnapshot>(4096)
                    .map_err(|reason| {
                        admission_error(
                            &self.path,
                            "read retirement admission",
                            format!("{reason:?}"),
                        )
                    })
            })
            .transpose()?;
        let restore_retirement = self
            .retirement
            .as_ref()
            .map(|owner| {
                let frame_bytes = crate::snapshot_restore_frame_bytes()
                    .checked_add(4096)
                    .ok_or_else(|| {
                        admission_error(
                            &self.path,
                            "restore frame admission",
                            "future frame declaration overflow",
                        )
                    })?;
                owner
                    .try_reserve::<crate::SnapshotRestoreFuture>(frame_bytes)
                    .map_err(|reason| {
                        admission_error(
                            &self.path,
                            "restore frame admission",
                            format!("{reason:?}"),
                        )
                    })
            })
            .transpose()?;
        let (ack, receiver) = oneshot::channel();
        self.admit(
            Command::Read {
                migration,
                result_storage,
                parser_storage,
                retirement,
                restore_retirement,
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

    #[test]
    fn snapshot_native_role_remains_charged_until_blocked_worker_is_joined() {
        let directory = tempfile::tempdir().expect("directory");
        let quota = crate::execution::ServerResources::new().quota_group();
        let (release, gate) = mpsc::channel();
        let (started, running) = mpsc::channel();
        let service = SnapshotIo::start_with_quota(
            directory.path().join("retiring.json"),
            &quota,
            move || {
                started.send(()).expect("started");
                gate.recv().expect("release");
            },
        )
        .expect("worker");
        running.recv_timeout(Duration::from_secs(5)).expect("entry");
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert!(quota.snapshot().worker_bytes >= WORKER_STACK_BYTES + MAX_SNAPSHOT_RETAINED_BYTES);
        let ticket = service._worker.ticket();
        drop(service);
        assert_eq!(quota.snapshot().worker_threads, 1);
        assert!(ticket.join_until(Instant::now()).is_err());
        release.send(()).expect("release");
        ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .expect("joined");
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn snapshot_refuses_exhausted_process_roles_before_entering_worker() {
        let directory = tempfile::tempdir().expect("directory");
        let quota = crate::execution::ServerResources::new().quota_group();
        let limits = quota.snapshot().limits;
        let occupied = quota
            .reserve_external_worker(limits.worker_threads, 1)
            .expect("roles");
        let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&entered);
        let result = SnapshotIo::start_with_quota(
            directory.path().join("refused.json"),
            &quota,
            move || {
                observed.store(true, Ordering::Release);
            },
        );
        assert!(result.is_err());
        assert!(!entered.load(Ordering::Acquire));
        assert_eq!(quota.snapshot().worker_threads, limits.worker_threads);
        assert_eq!(quota.snapshot().worker_bytes, 1);
        drop(occupied);
    }

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
    async fn discarded_loaded_snapshot_keeps_original_storage_until_cpu_destruction() {
        let directory = tempfile::tempdir().expect("directory");
        let owner = crate::execution::ServerExecution::start().expect("bank");
        let quota = owner.quota_group();
        let service =
            SnapshotIo::new_with_execution(directory.path().join("discarded.json"), &owner)
                .expect("disk owner");
        service.write_boot(snapshot(31)).await.expect("write");
        let (entered, started) = mpsc::channel();
        let mut releases = Vec::new();
        let mut receipts = Vec::new();
        for _ in 0..2 {
            let (release, gate) = mpsc::channel();
            let entered = entered.clone();
            let receipt = owner
                .client
                .foundation
                .try_reserve(
                    ilium_execution::Lane::Cpu,
                    ilium_execution::JobCost {
                        input_bytes: 4096,
                        result_bytes: 4096,
                    },
                )
                .expect("CPU admission")
                .submit(move |_: ilium_execution::JobContext| {
                    entered.send(()).expect("entered");
                    gate.recv().expect("release");
                    Ok::<(), ()>(())
                })
                .unwrap_or_else(|_| panic!("CPU submit refused"));
            releases.push(release);
            receipts.push(receipt);
        }
        for _ in 0..2 {
            started
                .recv_timeout(Duration::from_secs(5))
                .expect("both CPU owners parked");
        }
        let baseline = quota.snapshot().worker_bytes;
        let loaded = service.read(None).await.expect("read").expect("snapshot");
        assert_eq!(loaded.version, 31);
        let retained = quota.snapshot().worker_bytes;
        assert!(retained >= baseline + MAX_SNAPSHOT_RETAINED_BYTES);
        drop(loaded);
        // The unused restore-frame reservation contains no original and may
        // release its small empty envelope. The decoded original still owns
        // its independent body lease and CPU destruction envelope.
        assert!(
            quota.snapshot().worker_bytes > baseline + MAX_SNAPSHOT_RETAINED_BYTES,
            "discard must not destroy or uncharge the original on the caller"
        );
        for release in releases {
            release.send(()).expect("release CPU");
        }
        let wake = owner.client.completion_notification();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = wake.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if quota.snapshot().worker_bytes == baseline {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("retirement completion");
        drop(receipts);
        service.shutdown().await.expect("disk shutdown");
    }

    #[tokio::test]
    async fn cancelled_pending_restore_keeps_original_frame_state_and_storage_until_cpu_drop() {
        let directory = tempfile::tempdir().expect("directory");
        let owner = crate::execution::ServerExecution::start().expect("bank");
        let quota = owner.quota_group();
        let service =
            SnapshotIo::new_with_execution(directory.path().join("restore-frame.json"), &owner)
                .expect("disk owner");
        service
            .write_boot(snapshot(35))
            .await
            .expect("fixture write");
        let (entered, started) = mpsc::channel();
        let mut releases = Vec::new();
        let mut receipts = Vec::new();
        for _ in 0..2 {
            let (release, gate) = mpsc::channel();
            let entered = entered.clone();
            let receipt = owner
                .client
                .foundation
                .try_reserve(
                    ilium_execution::Lane::Cpu,
                    ilium_execution::JobCost {
                        input_bytes: 4096,
                        result_bytes: 4096,
                    },
                )
                .expect("CPU admission")
                .submit(move |_: ilium_execution::JobContext| {
                    entered.send(()).expect("entered");
                    gate.recv().expect("release");
                    Ok::<(), ()>(())
                })
                .unwrap_or_else(|_| panic!("CPU submit refused"));
            releases.push(release);
            receipts.push(receipt);
        }
        for _ in 0..2 {
            started
                .recv_timeout(Duration::from_secs(5))
                .expect("CPU owners parked");
        }
        let baseline = quota.snapshot().worker_bytes;
        let loaded = service.read(None).await.expect("read").expect("original");
        let state = Arc::new(capture_state(directory.path()));
        let weak = Arc::downgrade(&state);
        let publish_fence = state.workspace_spawn_lock.lock().await;
        let mut frame = match loaded.into_restore_future(Arc::clone(&state)) {
            ControlFlow::Continue(frame) => frame,
            ControlFlow::Break(_) => panic!("admitted restore frame"),
        };
        std::future::poll_fn(|context| {
            assert!(
                frame.as_mut().poll(context).is_pending(),
                "actual restore must wait for its publish fence"
            );
            std::task::Poll::Ready(())
        })
        .await;
        drop(publish_fence);
        drop(state);
        let retained = quota.snapshot().worker_bytes;
        assert!(retained > baseline + MAX_SNAPSHOT_RETAINED_BYTES);
        drop(frame);
        assert!(
            weak.upgrade().is_some(),
            "cancelled future retains its actual state on CPU retirement queue"
        );
        assert_eq!(
            quota.snapshot().worker_bytes,
            retained,
            "neither original nor frame credit released by caller"
        );
        for release in releases {
            release.send(()).expect("release CPU");
        }
        let wake = owner.client.completion_notification();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = wake.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if weak.upgrade().is_none() && quota.snapshot().worker_bytes == baseline {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("actual frame/state retirement");
        drop(receipts);
        service.shutdown().await.expect("disk shutdown");
    }

    #[tokio::test]
    async fn restore_frame_refusal_precedes_decode_and_releases_the_snapshot_envelope() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("frame-refused.json");
        let owner = crate::execution::ServerExecution::start().expect("bank");
        let quota = owner.quota_group();
        let service = SnapshotIo::new_with_execution(path.clone(), &owner).expect("disk owner");
        service
            .write_boot(snapshot(36))
            .await
            .expect("fixture write");
        let original = std::fs::read(&path).expect("original bytes");
        let retirement = owner.client.foundation.retirement();
        let permits: Vec<_> = (0..63)
            .map(|_| {
                retirement
                    .try_reserve::<SessionSnapshot>(4096)
                    .expect("slot pressure")
            })
            .collect();
        let baseline = quota.snapshot().worker_bytes;
        assert!(matches!(
            service.read(None).await,
            Err(ServerError::Snapshot {
                operation: "restore frame admission",
                source: SnapshotError::ResourceAdmission(_),
                ..
            })
        ));
        assert_eq!(
            quota.snapshot().worker_bytes,
            baseline,
            "failed frame admission releases prior result, scratch and envelope credits"
        );
        assert_eq!(service.budget.bytes.load(Ordering::Acquire), 0);
        assert_eq!(
            std::fs::read(&path).expect("authoritative readback"),
            original
        );
        drop(permits);
        let loaded = service.read(None).await.expect("retry").expect("snapshot");
        assert_eq!(loaded.version, 36);
        drop(loaded);
        service.shutdown().await.expect("disk shutdown");
    }

    #[tokio::test]
    async fn exhausted_retirement_slots_refuse_read_before_decoding_and_preserve_disk() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("retirement-refused.json");
        let owner = crate::execution::ServerExecution::start().expect("bank");
        let quota = owner.quota_group();
        let service = SnapshotIo::new_with_execution(path.clone(), &owner).expect("disk owner");
        service.write_boot(snapshot(32)).await.expect("write");
        let original = std::fs::read(&path).expect("original");
        let retirement = owner.client.foundation.retirement();
        let permits: Vec<_> = (0..ilium_execution::RETIREMENT_SLOTS)
            .map(|_| {
                retirement
                    .try_reserve::<Vec<u8>>(4096)
                    .expect("retirement slot")
            })
            .collect();
        let before = quota.snapshot().worker_bytes;
        assert!(service.read(None).await.is_err());
        assert_eq!(quota.snapshot().worker_bytes, before);
        assert_eq!(service.budget.bytes.load(Ordering::Acquire), 0);
        assert_eq!(std::fs::read(&path).expect("readback"), original);
        drop(permits);
        let loaded = service.read(None).await.expect("retry").expect("snapshot");
        assert_eq!(loaded.version, 32);
        drop(loaded);
        service.shutdown().await.expect("disk shutdown");
    }

    #[tokio::test]
    async fn loaded_result_and_captured_sources_retain_storage_after_worker_join() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("loaded.json");
        let quota = crate::execution::ServerResources::new().quota_group();
        let service = SnapshotIo::new_with_quota(path, &quota).expect("worker");
        service
            .write_boot(snapshot(11))
            .await
            .expect("initial write");
        let worker_bytes = quota.snapshot().worker_bytes;
        let loaded = service.read(None).await.expect("read").expect("snapshot");
        assert_eq!(loaded.version, 11);
        assert_eq!(
            quota.snapshot().worker_bytes,
            worker_bytes + MAX_SNAPSHOT_RETAINED_BYTES
        );
        // A retained read does not occupy the operation mailbox budget: later
        // ordered writes must remain usable while a recovery decision waits.
        service.write_boot(snapshot(12)).await.expect("later write");
        service.shutdown().await.expect("shutdown");
        let ticket = service._worker.ticket();
        drop(service);
        ticket
            .join_until(Instant::now() + Duration::from_secs(5))
            .expect("join");
        drop(ticket);
        assert_eq!(quota.snapshot().worker_bytes, MAX_SNAPSHOT_RETAINED_BYTES);
        let state = capture_state(directory.path());
        let (snapshot, storage) = loaded.into_parts();
        state.retain_snapshot_read_storage(storage);
        *state.tree.write().await = snapshot.tree;
        let sources = persistence::SnapshotSources::new(&state);
        drop(state);
        assert_eq!(quota.snapshot().worker_bytes, MAX_SNAPSHOT_RETAINED_BYTES);
        drop(sources);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[tokio::test]
    async fn parser_storage_refusal_preserves_file_and_releases_result_and_operation_credit() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("parser-refused.json");
        let quota = crate::execution::ServerResources::new().quota_group();
        let service = SnapshotIo::new_with_quota(path.clone(), &quota).expect("worker");
        service
            .write_boot(snapshot(23))
            .await
            .expect("fixture write");
        let original = std::fs::read(&path).expect("original");
        let baseline = quota.snapshot().worker_bytes;
        let available = quota.snapshot().limits.worker_bytes - baseline;
        let occupied_bytes =
            available - (MAX_SNAPSHOT_RETAINED_BYTES + MAX_NATIVE_READ_SCRATCH_BYTES - 1);
        let occupied = quota
            .reserve_external_storage(occupied_bytes)
            .expect("parser pressure");
        assert!(matches!(
            service.read(None).await,
            Err(ServerError::Snapshot {
                operation: "read parser admission",
                source: SnapshotError::ResourceAdmission(_),
                ..
            })
        ));
        assert_eq!(service.budget.bytes.load(Ordering::Acquire), 0);
        assert_eq!(quota.snapshot().worker_bytes, baseline + occupied_bytes);
        assert_eq!(
            std::fs::read(&path).expect("authoritative readback"),
            original
        );
        drop(occupied);
        let loaded = service.read(None).await.expect("retry").expect("snapshot");
        assert_eq!(loaded.version, 23);
        // Scratch is physically gone at receipt delivery; result stays charged.
        assert_eq!(
            quota.snapshot().worker_bytes,
            baseline + MAX_SNAPSHOT_RETAINED_BYTES
        );
        drop(loaded);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn read_operation_pressure_is_typed_and_preserves_original_before_enqueue() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("operation-refused.json");
        let service = SnapshotIo::new(path.clone()).expect("worker");
        service
            .write_boot(snapshot(24))
            .await
            .expect("fixture write");
        let original = std::fs::read(&path).expect("original");
        let occupied = service
            .reserve(MAX_SNAPSHOT_RETAINED_BYTES)
            .expect("operation pressure");
        assert!(matches!(
            service.read(None).await,
            Err(ServerError::Snapshot {
                operation: "admission",
                source: SnapshotError::ResourceAdmission(_),
                ..
            })
        ));
        assert_eq!(
            service.budget.bytes.load(Ordering::Acquire),
            MAX_SNAPSHOT_RETAINED_BYTES
        );
        assert_eq!(
            std::fs::read(&path).expect("authoritative readback"),
            original
        );
        drop(occupied);
        assert_eq!(
            service
                .read(None)
                .await
                .expect("retry")
                .expect("snapshot")
                .version,
            24
        );
        service.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn read_storage_refusal_precedes_decode_and_releases_operation_capacity() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("refused-read.json");
        let quota = crate::execution::ServerResources::new().quota_group();
        let service = SnapshotIo::new_with_quota(path.clone(), &quota).expect("worker");
        service
            .write_boot(snapshot(21))
            .await
            .expect("initial write");
        let original = std::fs::read(&path).expect("original");
        let available = quota.snapshot().limits.worker_bytes - quota.snapshot().worker_bytes;
        let occupied = quota.reserve_external_storage(available).expect("occupied");
        assert!(service.read(None).await.is_err());
        assert_eq!(service.budget.bytes.load(Ordering::Acquire), 0);
        assert_eq!(std::fs::read(&path).expect("readback"), original);
        drop(occupied);
        let loaded = service.read(None).await.expect("retry").expect("snapshot");
        assert_eq!(loaded.version, 21);
        drop(loaded);
        service.shutdown().await.expect("shutdown");
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
        let (sound_requests, _) = crate::sounds::test_channel(1);
        ServerState::new(ServerStateOptions {
            session_name: "capture-test".into(),
            session_cwd: directory.into(),
            home_dir: directory.into(),
            snapshot_path: directory.join("capture.json"),
            socket_path: directory.join("isolated.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: true,
            progress_monitor_enabled: true,
        })
    }

    #[tokio::test]
    async fn shutdown_drains_queued_semantic_cpu_work_and_durable_snapshot_before_bank_stop() {
        let directory = tempfile::tempdir().expect("directory");
        let state = Arc::new(capture_state(directory.path()));
        let owner = crate::execution::ServerExecution::start().expect("bank");
        assert!(state.execution.set(owner).is_ok());
        let execution = state.execution.get().expect("owner");
        let foundation = execution.client.foundation.clone();
        let wake = execution.client.completion_notification();
        let (entered, started) = mpsc::channel();
        let mut releases = Vec::new();
        let mut parked = Vec::new();
        for _ in 0..2 {
            let (release, gate) = mpsc::channel();
            let entered = entered.clone();
            parked.push(
                foundation
                    .try_reserve(
                        ilium_execution::Lane::Cpu,
                        ilium_execution::JobCost {
                            input_bytes: 4096,
                            result_bytes: 4096,
                        },
                    )
                    .expect("park admission")
                    .submit(move |_: ilium_execution::JobContext| {
                        entered.send(()).expect("entered");
                        gate.recv().expect("release");
                        Ok::<(), ()>(())
                    })
                    .unwrap_or_else(|_| panic!("park submit")),
            );
            releases.push(release);
        }
        for _ in 0..2 {
            started
                .recv_timeout(Duration::from_secs(5))
                .expect("CPU owners parked");
        }
        let mut receipt = foundation
            .try_reserve(
                ilium_execution::Lane::Cpu,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 4096,
                },
            )
            .expect("semantic admission")
            .submit(|context: ilium_execution::JobContext| {
                Ok::<bool, ()>(!context.stop_requested())
            })
            .unwrap_or_else(|_| panic!("semantic submit"));
        let semantic_state = Arc::clone(&state);
        let task = tokio::spawn(async move {
            loop {
                let notified = wake.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                match receipt.try_take() {
                    ilium_execution::JobPoll::Pending => notified.await,
                    ilium_execution::JobPoll::Ready(outcome) => {
                        assert!(
                            matches!(
                                outcome.view(),
                                ilium_execution::JobOutcome::Finished(Ok(true))
                            ),
                            "accepted semantic CPU work must run before cancellation"
                        );
                        break;
                    }
                    _ => panic!("semantic work lost"),
                }
            }
            semantic_state
                .tree
                .write()
                .await
                .rename_node(ROOT_ID, "accepted work drained", None, None)
                .expect("mutation");
            semantic_state.request_snapshot_save();
        });
        assert!(state.track_workspace_mutation_task(task));
        let mut writer = crate::task_guard::AbortOnDropHandle::new(
            persistence::spawn_snapshot_writer(Arc::clone(&state)),
        );
        let mut shutdown = Box::pin(crate::drain_session_work(&state, &mut writer));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(shutdown.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(
            foundation.is_open(),
            "semantic drain must retain bank admission"
        );
        for release in releases {
            release.send(()).expect("release CPU");
        }
        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown drain")
            .expect("successful semantic and durable shutdown");
        assert!(
            !foundation.is_open(),
            "bank stops after semantic and durable work"
        );
        let persisted = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("authoritative readback")
            .expect("snapshot");
        assert_eq!(
            persisted.tree.get(ROOT_ID).expect("root").name,
            "accepted work drained"
        );
        drop(parked);
        let _ = writer.settle().await;
    }

    #[tokio::test]
    async fn cancelled_snapshot_attempt_preserves_original_dirty_claim() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = Arc::new(capture_state(directory.path()));
        let execution = crate::execution::ServerExecution::start().expect("actual bank");
        assert!(state.execution.set(execution).is_ok());
        let write_guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        state.request_snapshot_save();
        let mut attempt = Box::pin(persistence::flush_pending_snapshot(&state));
        std::future::poll_fn(|context| {
            assert!(
                std::future::Future::poll(attempt.as_mut(), context).is_pending(),
                "actual write must remain blocked behind the held write owner"
            );
            std::task::Poll::Ready(())
        })
        .await;
        // Cancel only this test-owned future after the production flush took
        // the dirty claim but before native write admission can occur.
        drop(attempt);
        let still_owed = state.take_pending_snapshot();
        drop(write_guard);
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("ordered native service cleanup");
        state
            .execution
            .get()
            .expect("actual owner")
            .request_shutdown();
        assert!(
            still_owed,
            "cancelling an actual pending snapshot attempt must preserve its dirty claim"
        );
        assert!(
            !state.snapshot_path.exists(),
            "a cancelled pre-admission write must not create a snapshot"
        );
    }

    #[tokio::test]
    async fn shutdown_retries_dirty_claim_returned_by_inflight_failed_write() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = Arc::new(capture_state(directory.path()));
        let execution = crate::execution::ServerExecution::start().expect("actual bank");
        assert!(state.execution.set(execution).is_ok());
        state
            .tree
            .write()
            .await
            .rename_node(ROOT_ID, "latest state after failed write", None, None)
            .expect("actual semantic mutation");
        std::fs::create_dir(&state.snapshot_path).expect("isolated native write obstruction");
        let write_guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        let mut writer = crate::task_guard::AbortOnDropHandle::new(
            persistence::spawn_snapshot_writer(Arc::clone(&state)),
        );
        state.request_snapshot_save();
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.is_snapshot_dirty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("actual background writer consumed the dirty claim");
        let mut shutdown = Box::pin(crate::drain_session_work(&state, &mut writer));
        std::future::poll_fn(|context| {
            assert!(
                std::future::Future::poll(shutdown.as_mut(), context).is_pending(),
                "shutdown waits behind the actual in-flight writer"
            );
            std::task::Poll::Ready(())
        })
        .await;
        drop(write_guard);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !state.is_snapshot_dirty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("actual native failure returned the dirty claim");
        // Freeze only this fixture's now-idle debounced coordinator, after the
        // real write acknowledgement and failure bookkeeping. This prevents a
        // later ordinary retry from hiding a missing final shutdown retry.
        state.stop_snapshot_writer();
        assert_eq!(state.snapshot_path.parent(), Some(directory.path()));
        std::fs::remove_dir(&state.snapshot_path).expect("remove only isolated empty obstruction");
        let _shutdown_outcome = tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown completes cleanup after actual write failure");
        let _ = writer.settle().await;
        assert!(!state
            .execution
            .get()
            .expect("actual bank")
            .client
            .foundation
            .is_open());
        let persisted = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("authoritative final readback")
            .expect("shutdown must retry the dirty claim returned by its in-flight failed write");
        assert_eq!(
            persisted.tree.get(ROOT_ID).expect("saved root").name,
            "latest state after failed write"
        );
    }

    #[tokio::test]
    async fn cancelled_shutdown_preserves_original_writer_failure_across_retries() {
        let directory = tempfile::tempdir().expect("isolated snapshot directory");
        let state = Arc::new(capture_state(directory.path()));
        let execution = crate::execution::ServerExecution::start().expect("actual bank");
        assert!(state.execution.set(execution).is_ok());
        state
            .tree
            .write()
            .await
            .rename_node(
                ROOT_ID,
                "dirty state survives cancelled shutdown",
                None,
                None,
            )
            .expect("actual semantic mutation");
        state.request_snapshot_save();
        let write_guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        let mut writer: crate::task_guard::AbortOnDropHandle<()> =
            crate::task_guard::AbortOnDropHandle::new(tokio::spawn(async {
                panic!("isolated snapshot coordinator failure");
            }));
        let mut first = Box::pin(crate::drain_session_work(&state, &mut writer));
        tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|context| {
                let outcome = std::future::Future::poll(first.as_mut(), context);
                assert!(
                    outcome.is_pending(),
                    "final persistence must wait for the held write owner"
                );
                if state.snapshot_writer_failed() {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            }),
        )
        .await
        .expect("actual writer failure was reaped and latched");
        drop(first);
        assert!(
            state.is_snapshot_dirty(),
            "cancelled final persistence returns the original dirty claim"
        );
        assert!(
            state.snapshot_writer_failed(),
            "cancellation must not erase the physical join failure"
        );
        drop(write_guard);
        let second = tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer),
        )
        .await
        .expect("retry settles actual persistence and shutdown");
        assert!(
            second.is_err(),
            "retry must report the original coordinator failure"
        );
        let persisted = persistence::load_snapshot_blocking(&state.snapshot_path)
            .expect("authoritative final readback")
            .expect("dirty state was durably retried");
        assert_eq!(
            persisted.tree.get(ROOT_ID).expect("saved root").name,
            "dirty state survives cancelled shutdown"
        );
        let third = tokio::time::timeout(
            Duration::from_secs(5),
            crate::drain_session_work(&state, &mut writer),
        )
        .await
        .expect("already reaped shutdown retry finishes");
        assert!(
            third.is_err(),
            "an already reaped failed coordinator never becomes successful"
        );
        assert!(!state
            .execution
            .get()
            .expect("actual bank")
            .client
            .foundation
            .is_open());
    }

    #[tokio::test]
    async fn snapshot_worker_creation_refusal_is_typed_and_retains_recovery_file() {
        let directory = tempfile::tempdir().expect("directory");
        let state = capture_state(directory.path());
        let owner = crate::execution::ServerExecution::start().expect("existing bank");
        assert!(state.execution.set(owner).is_ok());
        persistence::write_snapshot_blocking(&state.snapshot_path, &snapshot(25)).expect("fixture");
        let original = std::fs::read(&state.snapshot_path).expect("original bytes");
        let quota = state.execution.get().expect("actual owner").quota_group();
        let available = quota.snapshot().limits.worker_bytes - quota.snapshot().worker_bytes;
        let occupied = quota
            .reserve_external_storage(available)
            .expect("worker pressure");
        assert!(matches!(
            persistence::load_snapshot_for_state(&state).await,
            Err(ServerError::Snapshot {
                operation: "start worker",
                source: SnapshotError::ResourceAdmission(_),
                ..
            })
        ));
        assert_eq!(
            std::fs::read(&state.snapshot_path).expect("readback"),
            original
        );
        assert!(state.snapshot_io.get().is_none());
        drop(occupied);
        let loaded = persistence::snapshot_service(&state)
            .await
            .expect("retry owner")
            .read(None)
            .await
            .expect("retry read")
            .expect("snapshot");
        assert_eq!(loaded.version, 25);
        drop(loaded);
        persistence::shutdown_snapshot_service(&state)
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn failed_recovery_write_remains_pending_and_retry_persists_without_another_mutation() {
        let directory = tempfile::tempdir().expect("directory");
        let state = capture_state(directory.path());
        // A directory at the exact destination makes atomic file replacement
        // fail on every platform without relying on user permission bits.
        std::fs::create_dir(&state.snapshot_path).expect("destination obstruction");
        state.request_snapshot_save();
        persistence::flush_pending_snapshot(&state).await;
        assert!(
            state.is_snapshot_dirty(),
            "failed save still owes durability"
        );
        std::fs::remove_dir(&state.snapshot_path).expect("remove owned empty obstruction");
        persistence::flush_pending_snapshot(&state).await;
        assert!(
            !state.is_snapshot_dirty(),
            "successful retry settles the claim"
        );
        let saved = persistence::snapshot_service(&state)
            .await
            .expect("disk owner")
            .read(None)
            .await
            .expect("readback")
            .expect("durable snapshot");
        assert_eq!(saved.tree, *state.tree.read().await);
        // Ordered flush retains the earlier failure receipt even though the
        // retry/readback succeeded; it must not retrospectively hide errors.
        assert!(persistence::shutdown_snapshot_service(&state)
            .await
            .is_err());
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
        let (sound_requests, _sound_receiver) = crate::sounds::test_channel(1);
        let state = ServerState::new(ServerStateOptions {
            session_name: "snapshot-kill-test".into(),
            session_cwd: directory.path().to_owned(),
            home_dir: directory.path().to_owned(),
            snapshot_path: directory.path().join("killed.json"),
            socket_path: directory.path().join("isolated.sock"),
            detection_config: DetectionConfig::default(),
            notifications_config: NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
