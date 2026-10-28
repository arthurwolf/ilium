//! Finite native PTY creation, with request-cancellation custody until publication.
//! Durable worktree tickets protect orphaned children after shutdown is requested;
//! rollback and pruning must both consult those tickets before filesystem mutation.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::execution::{ExecutionError, ServerExecution};
use crate::pane::{self, PaneIdentityEnv, SpawnedTerminalSession, TerminalOrigin};
use crate::state::ServerState;
use crate::workspace_custody::CustodyTicket;
use crate::workspace_prune::SpawnRepositoryAdmission;
use ilium_core::NodeId;
use ilium_execution::{Job, JobContext, JobCost, Lane, StorageAdmission};
use ilium_pty::PtyError;
use tokio::sync::OwnedMutexGuard;

const ADMISSION_TIMEOUT: Duration = Duration::from_secs(10);
const NATIVE_WORKING_BYTES: usize = 1024 * 1024;
type DirectoryGeneration = Option<(u64, u64)>;

pub(crate) struct NativeSpawnRequest {
    pub pane_id: NodeId,
    pub origin: TerminalOrigin,
    pub cwd: PathBuf,
    pub before_generation: DirectoryGeneration,
    pub repository: Option<SpawnRepositoryAdmission>,
    pub custody_ticket: Option<CustodyTicket>,
    pub deferred_workspace: bool,
}

/// The owning handler retains this through pane registration. The completed
/// General job has already retired; only independently admitted storage remains.
pub(crate) struct NativeSpawnScope {
    _spawn_guard: OwnedMutexGuard<()>,
    _repository: Option<SpawnRepositoryAdmission>,
    _storage: Arc<StorageAdmission>,
}

/// Declaration order is a cancellation contract: request native shutdown by
/// dropping the session BEFORE releasing repository/spawn admission. The
/// durable ticket is never cleared by Drop; the existing reaper owns the child.
pub(crate) struct NativeSpawned {
    pub spawned: SpawnedTerminalSession,
    pub origin: TerminalOrigin,
    pub custody_ticket: Option<CustodyTicket>,
    pub scope: NativeSpawnScope,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NativeSpawnError {
    #[error("{0}")]
    Admission(String),
    #[error(transparent)]
    Spawn(#[from] PtyError),
    #[error("pane node {0:?} was removed before native spawn")]
    NodeRemoved(NodeId),
}

struct NativeSpawnJob {
    request: NativeSpawnRequest,
    session_name: String,
    socket_path: PathBuf,
    worktree_root: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    quota: ilium_execution::QuotaGroup,
    scope: NativeSpawnScope,
}

impl Job for NativeSpawnJob {
    type Output = NativeSpawned;
    type Error = NativeSpawnError;

    fn run(mut self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err(NativeSpawnError::Admission(
                "terminal launch cancelled before directory verification".into(),
            ));
        }
        // Keep the exact final generation observation and process creation in
        // one admitted callback under the original spawn/repository fence.
        let after = ilium_platform::secure_fs::spawn_directory_generation(&self.request.cwd)
            .map_err(|error| {
                NativeSpawnError::Admission(format!(
                    "launch directory final identity verification failed: {error}"
                ))
            })?;
        if after != self.request.before_generation {
            return Err(NativeSpawnError::Admission(
                "launch directory changed identity during admission".into(),
            ));
        }
        if context.stop_requested() {
            return Err(NativeSpawnError::Admission(
                "terminal launch cancelled before native process creation".into(),
            ));
        }
        let identity = PaneIdentityEnv {
            pane_id: self.request.pane_id,
            session_name: &self.session_name,
            socket_path: &self.socket_path,
            worktree_root: self.worktree_root.as_deref(),
        };
        ilium_platform::startup_timing::record("spawn_begin", 0, self.request.pane_id.0, 0, 0);
        let resolved = match &self.request.origin {
            TerminalOrigin::CustomAgent {
                identity,
                command_line,
                frozen: false,
            } => {
                let provider = ilium_core::BuiltinAgentProvider::from_command_line(command_line)
                    .or_else(|| {
                        ilium_core::BuiltinAgentProvider::resume_binding(command_line)
                            .map(|(provider, _)| provider)
                    })
                    .or_else(|| {
                        pane::generated_claude_session_id(command_line)
                            .map(|_| ilium_core::BuiltinAgentProvider::Claude)
                    })
                    .or_else(|| {
                        (identity.base_provider.resume_picker_command()
                            == Some(command_line.as_str()))
                        .then_some(identity.base_provider)
                    });
                if provider != Some(identity.base_provider) {
                    return Err(NativeSpawnError::Admission(
                        "custom agent resume command does not match its provider".into(),
                    ));
                }
                let directory = self.config_dir.as_deref().ok_or_else(|| {
                    NativeSpawnError::Admission(
                        "custom agent configuration directory is unavailable".into(),
                    )
                })?;
                let profile = crate::custom_agent_config::resolve(directory, identity)
                    .map_err(|error| NativeSpawnError::Admission(error.to_string()))?;
                Some(
                    profile
                        .launch_input()
                        .map_err(|error| NativeSpawnError::Admission(error.to_string()))?,
                )
            }
            _ => None,
        };
        let spawned = pane::spawn_terminal_session_with_environment(
            &self.request.origin,
            &self.request.cwd,
            &identity,
            &self.quota,
            resolved
                .as_ref()
                .map_or(&[], |launch| launch.environment.as_slice()),
        )?;
        ilium_platform::startup_timing::record(
            "pty_ready",
            0,
            self.request.pane_id.0,
            spawned.session.process_id().unwrap_or(0),
            0,
        );
        // Any cancelled/lost receipt drops this exact session-first envelope.
        // A surviving child remains under platform supervision and its durable
        // ticket keeps every Ilium worktree remover fail-closed.
        Ok(NativeSpawned {
            spawned,
            origin: self.request.origin,
            custody_ticket: self.request.custody_ticket.take(),
            scope: self.scope,
        })
    }
}

fn add_bytes(total: usize, bytes: usize) -> Result<usize, NativeSpawnError> {
    total
        .checked_add(bytes)
        .ok_or_else(|| NativeSpawnError::Admission("terminal launch storage size overflow".into()))
}

fn spawn_cost(
    state: &ServerState,
    request: &NativeSpawnRequest,
    worktree_root: Option<&PathBuf>,
) -> Result<(JobCost, usize), NativeSpawnError> {
    let origin_bytes = match &request.origin {
        TerminalOrigin::PlainShell => 0,
        TerminalOrigin::Command(command) => command.capacity(),
        TerminalOrigin::Frozen { resume_command } => resume_command.capacity(),
        TerminalOrigin::CustomAgent {
            identity,
            command_line,
            ..
        } => identity
            .profile_id
            .capacity()
            .saturating_add(command_line.capacity()),
    };
    let mut owned = add_bytes(std::mem::size_of::<NativeSpawnJob>(), origin_bytes)?;
    for bytes in [
        request.cwd.capacity(),
        state.session_name.len(),
        state.socket_path.as_os_str().len(),
        state
            .text_trigger_config_path
            .get()
            .map_or(0, |path| path.capacity()),
        worktree_root.map_or(0, |path| path.capacity()),
        request
            .repository
            .as_ref()
            .map_or(0, |repository| repository.retained_bytes()),
        request
            .custody_ticket
            .as_ref()
            .map_or(0, |ticket| ticket.retained_bytes()),
    ] {
        owned = add_bytes(owned, bytes)?;
    }
    // The job's command/env construction can overlap its captured strings.
    // Persistent parser/journal/PTY workers retain their existing PTY admission;
    // the General job does not lend its credits to those surviving workers.
    let working = owned
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(NATIVE_WORKING_BYTES))
        .ok_or_else(|| {
            NativeSpawnError::Admission("terminal launch working size overflow".into())
        })?;
    let completion_bytes = add_bytes(owned, std::mem::size_of::<NativeSpawned>())?;
    Ok((
        JobCost {
            input_bytes: working,
            result_bytes: completion_bytes,
        },
        completion_bytes,
    ))
}

pub(crate) async fn spawn(
    state: &Arc<ServerState>,
    mut request: NativeSpawnRequest,
) -> Result<NativeSpawned, NativeSpawnError> {
    let execution: &ServerExecution = state.execution.get().ok_or_else(|| {
        NativeSpawnError::Admission("terminal launch execution service is unavailable".into())
    })?;
    let deadline = tokio::time::Instant::now() + ADMISSION_TIMEOUT;
    // This snapshot occurs before General admission; no job credit is held
    // across a tree wait. Validate it again using try_read once fenced.
    let worktree_root = if request.deferred_workspace {
        None
    } else {
        let tree = tokio::time::timeout_at(deadline, state.tree.read())
            .await
            .map_err(|_| {
                NativeSpawnError::Admission("terminal spawn tree snapshot timed out".into())
            })?;
        tree.pane_workspace(request.pane_id)
            .map(|workspace| workspace.worktree_root.clone())
    };
    let (cost, completion_bytes) = spawn_cost(state, &request, worktree_root.as_ref())?;
    // Acquire General and owned completion storage as a conjunction. Neither
    // credit may wait while retaining the other: a queue holder can itself
    // need owned storage before releasing its General result.
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(NativeSpawnError::Admission(
                "terminal spawn admission timed out".into(),
            ));
        }
        let reservation =
            tokio::time::timeout_at(deadline, execution.client.reserve(Lane::Io, cost))
                .await
                .map_err(|_| {
                    NativeSpawnError::Admission("terminal spawn I/O admission timed out".into())
                })?
                .map_err(|error| {
                    NativeSpawnError::Admission(format!(
                        "terminal spawn I/O admission failed: {error:?}"
                    ))
                })?;
        reservation
            .validate_job_type::<NativeSpawnJob>()
            .map_err(|error| {
                NativeSpawnError::Admission(format!(
                    "terminal spawn job cost is invalid: {error:?}"
                ))
            })?;
        let storage = match execution.client.try_reserve_storage(completion_bytes) {
            Ok(storage) => storage,
            Err(
                ilium_execution::RejectReason::Busy | ilium_execution::RejectReason::WorkerBytes,
            ) => {
                drop(reservation);
                let turn = tokio::time::timeout_at(
                    deadline,
                    execution.client.reserve_storage(completion_bytes),
                )
                .await
                .map_err(|_| {
                    NativeSpawnError::Admission("terminal completion storage wait timed out".into())
                })?
                .map_err(|error| {
                    NativeSpawnError::Admission(format!(
                        "terminal completion storage admission failed: {error:?}"
                    ))
                })?;
                drop(turn);
                continue;
            }
            Err(error) => {
                return Err(NativeSpawnError::Admission(format!(
                    "terminal completion storage admission failed: {error:?}"
                )))
            }
        };
        let guard = match Arc::clone(&state.workspace_spawn_lock).try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => {
                drop(reservation);
                drop(storage);
                let turn = tokio::time::timeout_at(deadline, state.workspace_spawn_lock.lock())
                    .await
                    .map_err(|_| {
                        NativeSpawnError::Admission("terminal spawn fence wait timed out".into())
                    })?;
                drop(turn);
                continue;
            }
        };
        let tree = match state.tree.try_read() {
            Ok(tree) => tree,
            Err(_) => {
                drop(guard);
                drop(reservation);
                drop(storage);
                let turn = tokio::time::timeout_at(deadline, state.tree.read())
                    .await
                    .map_err(|_| {
                        NativeSpawnError::Admission("terminal spawn tree wait timed out".into())
                    })?;
                drop(turn);
                continue;
            }
        };
        if tree.get(request.pane_id).is_none() {
            return Err(NativeSpawnError::NodeRemoved(request.pane_id));
        }
        let current_root = if request.deferred_workspace {
            None
        } else {
            tree.pane_workspace(request.pane_id)
                .map(|workspace| &workspace.worktree_root)
        };
        if current_root != worktree_root.as_ref() {
            return Err(NativeSpawnError::Admission(
                "terminal worktree changed during native-spawn admission; retry launch".into(),
            ));
        }
        drop(tree);
        let scope = NativeSpawnScope {
            _spawn_guard: guard,
            _repository: request.repository.take(),
            _storage: storage,
        };
        let job = NativeSpawnJob {
            request,
            session_name: state.session_name.clone(),
            socket_path: state.socket_path.clone(),
            worktree_root,
            config_dir: state
                .text_trigger_config_path
                .get()
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                .or_else(|| crate::paths::config_dir().ok()),
            quota: execution.pty_quota_group(),
            scope,
        };
        return match execution.client.run_reserved(reservation, job).await {
            Ok(retained) => {
                let (spawned, general_retention) = retained.into_parts();
                drop(general_retention);
                Ok(spawned)
            }
            Err(ExecutionError::Failed(error)) => {
                let (error, general_retention) = error.into_parts();
                drop(general_retention);
                Err(error)
            }
            Err(error) => Err(NativeSpawnError::Admission(format!(
                "terminal spawn worker failed: {error}"
            ))),
        };
    }
}

#[cfg(all(test, unix))]
#[path = "native_terminal_spawn_tests.rs"]
mod tests;
