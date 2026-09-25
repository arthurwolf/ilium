//! Crash-recovery snapshot: a JSON dump of the session's tree plus enough
//! per-pane metadata to respawn each pane the same way, written to
//! `<project>/.ilium/sessions/<session>.json` after every structural tree
//! change.
//!
//! This is explicitly **not** a database and **not** the source of truth
//! while the server is running -- `ServerState::tree`/`ServerState::panes`
//! are that. It exists so a killed/crashed server can be restarted and the
//! same tree shape (plus what to relaunch in each terminal pane) recovered,
//! matching the role `ilium/src/workspace_file.rs` played in the
//! pre-refactor single-process bin -- that file's `SavedNode`/`SavedAgent`
//! shape is not reused verbatim here, because this crate already has a
//! precise "how to respawn this pane" type in [`crate::pane::TerminalOrigin`]
//! (derived from `ilium_ipc::NewPaneKind`, the same shape a client sends
//! on `NewPane`), so duplicating a second schema for the same concept would
//! violate DRY for no benefit.
//!
//! The provider registry owns every built-in resume form (`claude --resume`,
//! `codex resume`, and `agy --conversation`), so snapshot recovery does not
//! have to duplicate provider-specific command parsing or reconstruction.
//!
//! Loading a snapshot on startup is implemented and tested here.
//! Respawning its panes is `crate::run`'s job: on finding a snapshot, it
//! replaces `ServerState::tree` with the snapshot's tree wholesale (the
//! snapshot already *is* the tree) and calls
//! `crate::ipc::handlers::spawn_and_register_pane` once per
//! [`PaneSnapshot`] to bring each pane's resource back to life -- the same
//! function a live client's `NewPane` request uses, so there is exactly one
//! place that knows how to turn a [`crate::pane::PaneSnapshotKind`] into a
//! running `PaneResource`. A pane whose command can no longer be spawned
//! (e.g. its binary was uninstalled since the snapshot was written) is
//! logged and dropped from the restored tree rather than left as a node
//! with no resource behind it; see `run`'s doc comment for why.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ilium_agent_session::TranscriptLocator;
use ilium_core::{
    AgentProvider, BuiltinAgentProvider, NodeId, PaneContentKind, PaneProgress, Tree,
};
use ilium_ipc::ProgressGoalPolicy;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::task::JoinHandle;

use crate::agent_debug::PaneDebugLogSnapshot;
use crate::error::{ServerError, SnapshotError};
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
use crate::progress_monitor::ProgressMonitorRegistration;
use crate::state::ServerState;

/// How long the background snapshot writer waits, once woken by
/// `ServerState::request_snapshot_save`, before actually writing --
/// enough to coalesce a burst of rapid mutations (e.g. several automatic
/// pane-title updates typed in quick succession) into a single disk
/// write, short enough that crash-recovery data is never more than about
/// a second stale.
const SNAPSHOT_DEBOUNCE_INTERVAL: Duration = Duration::from_millis(750);

/// Bumped whenever `SessionSnapshot`'s shape changes incompatibly. Not
/// currently enforced on load -- see `workspace_file::CURRENT_VERSION`'s
/// identical comment for why that's an acceptable, deliberate choice for a
/// best-effort recovery file.
const CURRENT_SNAPSHOT_VERSION: u32 = 2;

/// Project-local save format written by the single-process precursor. It is
/// intentionally defined at the server persistence boundary: importing it is
/// a one-time storage migration, not client presentation logic.
#[derive(Debug, Deserialize)]
struct LegacyWorkspace {
    root: Vec<LegacyNode>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LegacyNode {
    Group {
        name: String,
        children: Vec<LegacyNode>,
    },
    Terminal {
        name: String,
        agent: Option<LegacyAgent>,
        #[serde(default)]
        title_source: Option<LegacyTitleSource>,
    },
    Editor {
        name: String,
        path: Option<std::path::PathBuf>,
    },
}

#[derive(Debug, Deserialize)]
struct LegacyAgent {
    command: String,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyTitleSource {
    Auto,
    User,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: u32,
    pub tree: Tree,
    /// A `Vec` of `(NodeId, kind)` pairs rather than
    /// `HashMap<NodeId, PaneSnapshotKind>` -- `NodeId` is a newtype over
    /// `u64`, and JSON object keys must be strings, so a map keyed by it
    /// would need a custom key (de)serializer for no real benefit here:
    /// this list is small (one entry per pane) and read/written as a
    /// whole, never looked up by key.
    pub panes: Vec<PaneSnapshot>,
    /// Pane histories are persisted in the same atomic project snapshot but
    /// stay outside the hot `TreeSnapshot` IPC path.
    #[serde(default)]
    pub agent_debug_logs: Vec<PaneDebugLogSnapshot>,
    /// Durable progress-monitor registrations and terminal delivery state.
    /// This stays outside `Tree`: the tree owns presentation state, while the
    /// command, cadence, and recovery bookkeeping belong to the server's I/O
    /// boundary. Older snapshots default to no registrations.
    #[serde(default)]
    pub(crate) progress_monitors: Vec<PersistedProgressMonitor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub node_id: NodeId,
    pub kind: PaneSnapshotKind,
}

/// What is durably known about an automated PTY delivery.
///
/// Only `Queued` is safe to retry after a server restart. `Attempted`,
/// `DeliveredToPty`, and `Uncertain` may already have reached the agent, so
/// replaying any of them could duplicate a result or resume a goal twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PersistedProgressDeliveryState {
    #[default]
    NotQueued,
    Queued,
    Attempted,
    DeliveredToPty,
    Uncertain,
}

impl PersistedProgressDeliveryState {
    pub(crate) const fn may_retry_after_restart(self) -> bool {
        matches!(self, Self::Queued)
    }
}

/// Server-owned progress lifecycle data required to recover after a crash.
/// No top-level registration ID is persisted. `latest_progress` necessarily
/// carries the previous presentation ID, but monitor IDs are process-local
/// generations: restore never reuses that value, allocates a fresh ID, and
/// rewrites the embedded state before publishing or starting work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedProgressMonitor {
    pub pane_id: NodeId,
    pub command: String,
    pub interval_seconds: u64,
    pub goal_policy: ProgressGoalPolicy,
    pub latest_progress: PaneProgress,
    #[serde(default)]
    pub goal_resume_armed: bool,
    #[serde(default)]
    pub result_delivery: PersistedProgressDeliveryState,
    #[serde(default)]
    pub goal_resume_delivery: PersistedProgressDeliveryState,
}

impl PersistedProgressMonitor {
    pub(crate) const fn requires_probe_before_restore(&self) -> bool {
        !self.latest_progress.is_terminal() && !self.latest_progress.monitor_health.is_failed()
    }

    pub(crate) fn accepts_restored_preflight(
        &self,
        preflight: &ilium_ipc::ProgressMonitorPreflight,
    ) -> bool {
        preflight.report.job_id == self.latest_progress.report.job_id
    }

    /// Reconstitutes the monitor specification with a fresh generation.
    /// The caller must still run one preflight probe and require the same
    /// `job_id` before installing a non-terminal recurring coordinator.
    pub(crate) fn restored_registration(
        &self,
        fresh_monitor_id: u64,
    ) -> Result<ProgressMonitorRegistration, crate::progress_monitor::ProgressProbeError> {
        let mut initial_progress = self.latest_progress.clone();
        initial_progress.monitor_id = fresh_monitor_id;
        let registration = ProgressMonitorRegistration {
            monitor_id: fresh_monitor_id,
            pane_id: self.pane_id,
            command: self.command.clone(),
            interval: Duration::from_secs(self.interval_seconds),
            initial_progress,
        };
        registration.validate()?;
        Ok(registration)
    }

    /// Goal ownership cannot be reconstructed from a snapshot alone. A
    /// restored record may retain that the user requested pause/resume for
    /// diagnostics, but automatic resume is always disarmed on boot.
    pub(crate) fn disarm_ambiguous_goal_resume(&mut self) {
        self.goal_resume_armed = false;
        if !matches!(
            self.goal_resume_delivery,
            PersistedProgressDeliveryState::NotQueued
        ) {
            self.goal_resume_delivery = PersistedProgressDeliveryState::Uncertain;
        }
    }
}

/// Loads the native project-local snapshot if present, otherwise imports the
/// preceding project-local YAML workspace. The YAML source is deliberately
/// retained: it is a user-owned recovery artifact, not disposable seed data.
pub async fn load_snapshot_or_migrate(
    snapshot_path: &Path,
    session_cwd: &Path,
    home: &Path,
) -> Result<Option<SessionSnapshot>, ServerError> {
    if let Some(mut snapshot) = load_snapshot(snapshot_path).await? {
        let original_tree = snapshot.tree.clone();
        snapshot
            .tree
            .ensure_launch_project(session_cwd.to_path_buf())?;
        let tree_changed_by_launch_project = snapshot.tree != original_tree;
        // Both sides must always run: `||` short-circuits and would skip
        // `normalize_agent_resumes`'s mutations (repairing invalid resume
        // bindings, fixing their titles) whenever `ensure_launch_project`
        // alone already changed the tree, silently leaving a corrupted or
        // duplicate resume binding in the snapshot this function returns.
        let resume_bindings_changed = normalize_agent_resumes(&mut snapshot, home, session_cwd);
        if tree_changed_by_launch_project || resume_bindings_changed {
            write_snapshot_to(snapshot_path, &snapshot).await?;
        }
        return Ok(Some(snapshot));
    }

    let legacy_path = session_cwd.join(".ilium").join("sessions.yml");
    let legacy_exists = tokio::fs::try_exists(&legacy_path).await.map_err(|error| {
        ServerError::LegacyWorkspace {
            path: legacy_path.clone(),
            message: error.to_string(),
        }
    })?;
    if !legacy_exists {
        return Ok(None);
    }

    let legacy_contents = tokio::fs::read_to_string(&legacy_path)
        .await
        .map_err(|error| ServerError::LegacyWorkspace {
            path: legacy_path.clone(),
            message: error.to_string(),
        })?;
    let legacy_workspace: LegacyWorkspace =
        serde_norway::from_str(&legacy_contents).map_err(|error| ServerError::LegacyWorkspace {
            path: legacy_path.clone(),
            message: error.to_string(),
        })?;
    let mut snapshot = legacy_workspace.into_snapshot(session_cwd)?;
    // Legacy `sessions.yml` files could already carry the same agent
    // session id on several titled panes; collapse those duplicates before
    // ever writing/spawning from this migrated snapshot, the same way a
    // reloaded native snapshot already does above.
    normalize_agent_resumes(&mut snapshot, home, session_cwd);

    write_snapshot_to(snapshot_path, &snapshot).await?;
    tracing::info!("imported legacy workspace into {}", snapshot_path.display());
    Ok(Some(snapshot))
}

/// Keeps a persisted resume binding only when its transcript independently
/// proves the same agent class, session UUID, and canonical project cwd. This
/// also repairs older snapshots corrupted by inherited cross-project IDs and
/// prevents one valid session from being resumed by multiple panes.
fn normalize_agent_resumes(
    snapshot: &mut SessionSnapshot,
    home: &Path,
    session_cwd: &Path,
) -> bool {
    let mut seen_resumes = HashSet::new();
    let mut invalid_automatic_titles = Vec::new();
    let mut changed = false;
    for pane in &mut snapshot.panes {
        let PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) = &mut pane.kind else {
            continue;
        };
        let Some(binding) = persisted_resume_binding(command) else {
            continue;
        };
        let project_cwd = snapshot
            .tree
            .project_path_for(pane.node_id)
            .unwrap_or(session_cwd);
        let locator = TranscriptLocator::new(home, project_cwd);
        let is_project_verified = locator
            .transcript_for_session(&binding.provider.class(), &binding.session_id)
            .is_some();
        // Only a project-verified session id occupies the "already resumed"
        // slot. Inserting unconditionally would let an unverifiable (e.g.
        // cross-project) binding claim a session id first and then falsely
        // flag a *different*, later-processed pane's legitimately verified
        // resume of that same session as a duplicate -- destroying the one
        // binding this function exists to protect, purely because of Vec
        // iteration order.
        let is_unique = if is_project_verified {
            seen_resumes.insert(binding.session_id)
        } else {
            true
        };
        if is_project_verified && is_unique {
            continue;
        }
        *command = binding.provider.command_line().to_string();
        invalid_automatic_titles.push((pane.node_id, binding.provider.command_line()));
        changed = true;
    }
    // A generated title is downstream of the same session binding. Once the
    // binding fails provenance validation, retaining that automatic title
    // would continue showing content from the wrong project even though the
    // unsafe resume command itself was removed. User-specified titles remain
    // protected by `set_automatic_pane_title`.
    for (pane_id, bare_command) in invalid_automatic_titles {
        let _ = snapshot
            .tree
            .set_automatic_pane_title(pane_id, bare_command, None, None);
    }
    changed
}

struct PersistedResumeBinding {
    provider: BuiltinAgentProvider,
    session_id: String,
}

/// Parses only the exact resume commands ilium itself persists. Arbitrary
/// shell commands containing similar words remain user-owned commands.
fn persisted_resume_binding(command: &str) -> Option<PersistedResumeBinding> {
    let (provider, session_id) = BuiltinAgentProvider::resume_binding(command)?;
    Some(PersistedResumeBinding {
        provider,
        session_id,
    })
}

impl LegacyWorkspace {
    /// Converts the old YAML tree into the server's native persistence
    /// contract, retaining the exact commands needed to resume known agents.
    fn into_snapshot(self, session_cwd: &Path) -> Result<SessionSnapshot, ServerError> {
        let mut tree = Tree::new();
        let project = tree.ensure_launch_project(session_cwd.to_path_buf())?;
        let mut panes = Vec::new();
        let fallback_group = self
            .root
            .iter()
            .any(|node| !matches!(node, LegacyNode::Group { .. }))
            .then(|| tree.add_group(project, "default"))
            .transpose()?;

        for node in self.root {
            let parent = if matches!(&node, LegacyNode::Group { .. }) {
                project
            } else {
                fallback_group.ok_or_else(|| ServerError::LegacyWorkspace {
                    path: PathBuf::from(".ilium/sessions.yml"),
                    message: "legacy root pane had no generated fallback group".to_string(),
                })?
            };
            append_legacy_node(&mut tree, &mut panes, parent, node)?;
        }

        Ok(SessionSnapshot {
            version: CURRENT_SNAPSHOT_VERSION,
            tree,
            panes,
            agent_debug_logs: Vec::new(),
            progress_monitors: Vec::new(),
        })
    }
}

/// Adds one legacy node recursively and records a native resource descriptor
/// for every resulting pane.
fn append_legacy_node(
    tree: &mut Tree,
    panes: &mut Vec<PaneSnapshot>,
    parent: NodeId,
    node: LegacyNode,
) -> Result<(), ServerError> {
    match node {
        LegacyNode::Group { name, children } => {
            let group_id = tree.add_group(parent, name)?;
            for child in children {
                append_legacy_node(tree, panes, group_id, child)?;
            }
        }
        LegacyNode::Terminal {
            name,
            agent,
            title_source,
        } => {
            let pane_id = tree.add_pane(parent, name.clone(), PaneContentKind::Terminal)?;
            if matches!(title_source, Some(LegacyTitleSource::User)) {
                tree.rename_node(pane_id, name, None, None)?;
            }
            let origin = agent.map_or(TerminalOrigin::PlainShell, legacy_agent_origin);
            panes.push(PaneSnapshot {
                node_id: pane_id,
                kind: PaneSnapshotKind::Terminal(origin),
            });
        }
        LegacyNode::Editor { name, path } => {
            let pane_id = tree.add_pane(parent, name, PaneContentKind::Editor)?;
            panes.push(PaneSnapshot {
                node_id: pane_id,
                kind: PaneSnapshotKind::Editor { path },
            });
        }
    }
    Ok(())
}

/// Builds the original CLI command for an agent pane, using its stored
/// session identifier when the old workspace had one.
fn legacy_agent_origin(agent: LegacyAgent) -> TerminalOrigin {
    let Some(session_id) = agent.session_id else {
        return TerminalOrigin::Command(agent.command);
    };
    if uuid::Uuid::try_parse(&session_id).is_err() {
        return TerminalOrigin::Command(agent.command);
    }
    let quoted_session_id = shell_quote(&session_id);
    let command_line = BuiltinAgentProvider::from_command_line(&agent.command)
        .map(|provider| provider.resume_command(&quoted_session_id))
        .unwrap_or(agent.command);
    TerminalOrigin::Command(command_line)
}

/// Quotes one value for the shell command string accepted by `PtyCommand`.
fn shell_quote(value: &str) -> String {
    // Close the current single-quoted segment, emit a backslash-escaped
    // single quote, then reopen single-quoting -- the standard POSIX
    // technique for embedding a literal `'` inside a `'...'` string.
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Builds a snapshot of `state`'s current tree and pane registry. Takes
/// the `tree` read lock before `panes`, per `ServerState`'s documented
/// lock ordering.
async fn build_snapshot(state: &ServerState) -> SessionSnapshot {
    build_snapshot_with_progress_override(state, None).await
}

/// Builds the current snapshot while substituting one already-validated
/// progress registration for the live runtime state of the same pane.
///
/// This is used by `SetPaneProgressMonitor` while holding that pane's effect
/// gate: the previous live monitor remains untouched until the complete
/// replacement snapshot is durable. A failed write can therefore reject the
/// replacement without cancelling the previously accepted monitor.
async fn build_snapshot_with_progress_override(
    state: &ServerState,
    progress_override: Option<&PersistedProgressMonitor>,
) -> SessionSnapshot {
    let (tree, pane_snapshots, progress_monitors) = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let mut pane_snapshots = Vec::with_capacity(panes.len());
        let mut progress_monitors = Vec::new();
        for (node_id, resource) in panes.iter() {
            let kind = match resource {
                PaneResource::Terminal(runtime) => {
                    let progress_monitor = progress_override
                        .filter(|progress_monitor| progress_monitor.pane_id == *node_id)
                        .cloned()
                        .or_else(|| runtime.progress_monitor_snapshot(*node_id));
                    if let Some(progress_monitor) = progress_monitor {
                        progress_monitors.push(progress_monitor);
                    }
                    PaneSnapshotKind::Terminal(snapshot_terminal_origin(runtime))
                }
                PaneResource::Editor { path } => PaneSnapshotKind::Editor { path: path.clone() },
            };
            pane_snapshots.push(PaneSnapshot {
                node_id: *node_id,
                kind,
            });
        }
        (tree.clone(), pane_snapshots, progress_monitors)
    };
    let agent_debug_logs = state.agent_debug.snapshot().await;
    SessionSnapshot {
        version: CURRENT_SNAPSHOT_VERSION,
        tree,
        panes: pane_snapshots,
        agent_debug_logs,
        progress_monitors,
    }
}

/// Turns a freshly-launched agent command into its resume form once the
/// detection loop has authoritatively discovered that CLI's session id.
fn snapshot_terminal_origin(runtime: &crate::pane::TerminalPaneRuntime) -> TerminalOrigin {
    snapshot_origin_from_identity(
        &runtime.origin,
        runtime.session_id.as_deref(),
        runtime.is_session_identity_invalidated,
    )
}

/// Rebuilds only commands ilium itself owns as agent launch/resume forms.
/// Arbitrary user commands are immutable. During an in-process transition,
/// the previous resume ID is known stale, so a crash can safely restore only
/// a fresh bare agent until the replacement identity is proven.
fn snapshot_origin_from_identity(
    origin: &TerminalOrigin,
    session_id: Option<&str>,
    is_session_identity_invalidated: bool,
) -> TerminalOrigin {
    let TerminalOrigin::Command(command) = origin else {
        return origin.clone();
    };
    let provider = BuiltinAgentProvider::from_command_line(command)
        .or_else(|| persisted_resume_binding(command).map(|binding| binding.provider));
    let Some(provider) = provider else {
        return origin.clone();
    };
    if is_session_identity_invalidated {
        return TerminalOrigin::Command(provider.command_line().to_string());
    }
    let Some(session_id) = session_id else {
        return origin.clone();
    };
    TerminalOrigin::Command(provider.resume_command(&shell_quote(session_id)))
}

/// Builds and writes the current snapshot to `state.snapshot_path`.
/// Ordinary recovery saves are best-effort (see module docs). Operations
/// that fence an irreversible effect or return a durable-acceptance ack call
/// the error-propagating barrier wrappers below instead; every other request
/// uses [`spawn_snapshot_writer`]/[`flush_pending_snapshot`] through
/// `ServerState::request_snapshot_save`.
pub async fn save_snapshot(state: &ServerState) -> Result<(), ServerError> {
    let _write_guard = state.snapshot_write_lock.lock().await;
    let snapshot = build_snapshot(state).await;
    write_snapshot_to(&state.snapshot_path, &snapshot).await
}

/// Durability barrier for an irreversible server-owned effect.
///
/// Unlike [`flush_pending_snapshot`], this always writes a fresh snapshot and
/// propagates errors. It therefore remains a valid barrier when the
/// background writer has already consumed the dirty flag: both paths share
/// `snapshot_write_lock`, and this write is built only after any older write
/// has finished, so an older snapshot cannot overwrite it afterward.
pub(crate) async fn await_snapshot_durability_barrier(
    state: &ServerState,
) -> Result<(), ServerError> {
    save_snapshot(state).await
}

/// Durably stages a validated replacement progress registration without
/// disturbing the currently running monitor. The caller must hold the pane's
/// `progress_effect_gate` from before this call through the subsequent live
/// commit, and must retain the returned write guard through that commit. This
/// makes a persistence failure transactionally harmless to the old monitor
/// and prevents a background writer that already claimed the dirty flag from
/// overwriting the staged registration with pre-commit state.
pub(crate) async fn await_progress_monitor_durability_barrier(
    state: &ServerState,
    progress_monitor: &PersistedProgressMonitor,
) -> Result<tokio::sync::OwnedMutexGuard<()>, ServerError> {
    let write_guard = std::sync::Arc::clone(&state.snapshot_write_lock)
        .lock_owned()
        .await;
    let snapshot = build_snapshot_with_progress_override(state, Some(progress_monitor)).await;
    write_snapshot_to(&state.snapshot_path, &snapshot).await?;
    Ok(write_guard)
}

/// Spawns the background task that owns every crash-recovery snapshot
/// write. `crate::ipc::handlers` never writes to disk itself on the
/// request path -- it calls `ServerState::request_snapshot_save` (a
/// cheap, non-blocking dirty-flag set + wakeup) and returns immediately,
/// so a slow disk (contention from other processes, a busy page cache)
/// never adds latency to an unrelated connection's next request. This
/// task debounces: it waits `SNAPSHOT_DEBOUNCE_INTERVAL` after being
/// woken before actually writing, so a burst of rapid mutations coalesces
/// into one write.
///
/// Deliberately never aborted while a write might be in flight: cancelling
/// a task mid-`tokio::fs::write`/`rename` does not stop the underlying
/// blocking file operation (`tokio::fs` runs it on a blocking-thread-pool
/// task via `spawn_blocking`, which keeps running to completion even if
/// the `.await` waiting on it is dropped) -- it would only detach the
/// *result* from anything, silently losing track of whether the write
/// finished. `crate::run`'s shutdown path instead performs its own final,
/// directly-awaited [`flush_pending_snapshot`] call, then waits for
/// `state.snapshot_write_lock` to confirm any write this task already had
/// in flight has fully completed, before finally stopping this task (see
/// its comments).
pub fn spawn_snapshot_writer(state: Arc<ServerState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            state.snapshot_requested.notified().await;
            tokio::time::sleep(SNAPSHOT_DEBOUNCE_INTERVAL).await;
            flush_pending_snapshot(&state).await;
        }
    })
}

/// Writes the crash-recovery snapshot if (and only if) it is currently
/// marked dirty, consuming the dirty flag with an atomic swap first so a
/// `request_snapshot_save` that lands *during* this write is never lost
/// -- it simply re-dirties the flag for the writer's next pass rather
/// than being silently dropped. Called by [`spawn_snapshot_writer`]'s
/// loop on its normal debounced schedule, and once more, directly, by
/// `crate::run`'s shutdown path so a mutation too recent to have been
/// picked up by the debounce window yet is still persisted before the
/// server exits.
pub async fn flush_pending_snapshot(state: &ServerState) {
    if !state.take_pending_snapshot() {
        return;
    }
    if let Err(error) = save_snapshot(state).await {
        tracing::error!("failed to write crash-recovery snapshot: {error}");
    }
}

/// Writes `snapshot` to `path`, via a temp-file-then-rename in the same
/// directory so a crash or kill mid-write -- the exact scenario this
/// feature exists to survive -- can never leave a half-written,
/// unparseable snapshot behind (the rename is atomic: the file on disk is
/// always either the previous complete snapshot or the new one, mirroring
/// `workspace_file::save`'s identical reasoning).
async fn write_snapshot_to(path: &Path, snapshot: &SessionSnapshot) -> Result<(), ServerError> {
    let json = serde_json::to_vec(snapshot).map_err(|source| ServerError::Snapshot {
        operation: "serialize",
        path: path.to_path_buf(),
        source: SnapshotError::Json(source),
    })?;

    let path_buf = path.to_path_buf();
    let to_snapshot_io_error =
        |operation: &'static str, source: std::io::Error| ServerError::Snapshot {
            operation,
            path: path_buf.clone(),
            source: SnapshotError::Io(source),
        };

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // Create and restrict the directory through the platform adapter in one
    // operation. This avoids briefly creating session data with ambient
    // permissions and refuses pre-planted symlink components.
    ilium_platform::secure_fs::create_private_directory(parent)
        .map_err(|source| to_snapshot_io_error("create private directory for", source))?;
    let temp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "snapshot".to_string()),
        std::process::id()
    ));
    let _ = tokio::fs::remove_file(&temp_path).await;
    // `private_open_options()` also refuses a pre-planted symlink at
    // `temp_path` (`O_NOFOLLOW`) and keeps the descriptor out of any spawned
    // agent CLI (`O_CLOEXEC`) on Unix -- guarantees the old ad hoc
    // `.mode(0o600)` open here didn't have. The temp path already carries
    // this process's pid, so `create_new` still can't collide with a
    // concurrent writer.
    let open_result = ilium_platform::secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temp_path);
    let mut temp_file = match open_result {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => return Err(to_snapshot_io_error("create", error)),
    };
    if let Err(error) = temp_file.write_all(&json).await {
        // A write that fails partway (e.g. disk full, permission change
        // mid-write) can still have created the temp file. This writer
        // loops for the server's entire lifetime (`spawn_snapshot_writer`),
        // so leaving a stray temp file behind on every failed write would
        // accumulate unbounded cruft on disk across a long-running server;
        // clean it up (best-effort) before surfacing the original error.
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(to_snapshot_io_error("write", error));
    }
    if let Err(error) = temp_file.sync_all().await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(to_snapshot_io_error("sync", error));
    }
    drop(temp_file);
    if let Err(error) = tokio::fs::rename(&temp_path, path).await {
        // Same reasoning: don't leave the temp file behind if the rename
        // itself fails.
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(to_snapshot_io_error("rename", error));
    }
    ilium_platform::secure_fs::restrict_file_to_owner(path)
        .map_err(|source| to_snapshot_io_error("secure", source))?;
    Ok(())
}

/// Reads and parses the snapshot at `path`. `Ok(None)` means no snapshot
/// exists yet (a brand-new session) -- distinct from `Err`, which means
/// one exists but couldn't be read or parsed (e.g. hand-edited into
/// invalid JSON), so the caller can log "nothing to recover" separately
/// from "something to warn about."
pub async fn load_snapshot(path: &Path) -> Result<Option<SessionSnapshot>, ServerError> {
    let path_buf = path.to_path_buf();
    // One read, with `NotFound` mapped to "no snapshot yet", instead of a
    // separate existence probe followed by the read: the probe-then-read
    // pair had a TOCTOU window in which a snapshot removed between the two
    // calls surfaced as a spurious read `Err` rather than the documented
    // `Ok(None)`, and the read itself already answers the existence
    // question.
    let contents = match tokio::fs::read(path).await {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ServerError::Snapshot {
                operation: "read",
                path: path_buf,
                source: SnapshotError::Io(source),
            });
        }
    };
    let snapshot: SessionSnapshot =
        serde_json::from_slice(&contents).map_err(|source| ServerError::Snapshot {
            operation: "parse",
            path: path_buf.clone(),
            source: SnapshotError::Json(source),
        })?;
    // `Tree` derives `Deserialize` so it can be loaded wholesale above, but
    // that bypasses every invariant this crate's own tree mutations
    // (`add_pane`, `move_node`, ...) normally enforce -- a hand-edited or
    // crash-truncated snapshot file can deserialize into a structurally
    // invalid tree without a JSON parse error. Reject it here, loudly,
    // rather than let some later, unrelated tree operation panic on it.
    snapshot
        .tree
        .validate()
        .map_err(|source| ServerError::Snapshot {
            operation: "validate",
            path: path_buf,
            source: SnapshotError::InvalidTree(source),
        })?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::pane::TerminalOrigin;
    use ilium_agent_debug::{
        AgentDebugContext, AgentDebugEventDraft, AgentDebugEventKind, AgentDebugSource,
        PaneDebugLog,
    };
    use ilium_core::{
        PaneContentKind, PaneProgress, ProgressTaskReport, ProgressTaskStatus, ROOT_ID,
    };

    fn scratch_snapshot_path() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-server-persistence-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir.join("test-session.snapshot.json")
    }

    fn sample_snapshot() -> SessionSnapshot {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let shell_pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let agent_pane = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        let editor_pane = tree
            .add_pane(group, "notes.md", PaneContentKind::Editor)
            .unwrap();

        SessionSnapshot {
            version: CURRENT_SNAPSHOT_VERSION,
            tree,
            panes: vec![
                PaneSnapshot {
                    node_id: shell_pane,
                    kind: PaneSnapshotKind::Terminal(TerminalOrigin::PlainShell),
                },
                PaneSnapshot {
                    node_id: agent_pane,
                    kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command("claude".to_string())),
                },
                PaneSnapshot {
                    node_id: editor_pane,
                    kind: PaneSnapshotKind::Editor {
                        path: Some(PathBuf::from("/tmp/notes.md")),
                    },
                },
            ],
            agent_debug_logs: Vec::new(),
            progress_monitors: Vec::new(),
        }
    }

    fn sample_persisted_progress(pane_id: NodeId) -> PersistedProgressMonitor {
        PersistedProgressMonitor {
            pane_id,
            command: "/usr/local/bin/progress-probe".to_string(),
            interval_seconds: 15,
            goal_policy: ProgressGoalPolicy::PauseAndResume,
            latest_progress: PaneProgress::new(
                41,
                ProgressTaskReport::new(
                    "render-job-7".to_string(),
                    ProgressTaskStatus::Running,
                    72.5,
                    "frame 725/1000".to_string(),
                    None,
                )
                .unwrap(),
                1_700_000_000_000,
            )
            .unwrap(),
            goal_resume_armed: true,
            result_delivery: PersistedProgressDeliveryState::NotQueued,
            goal_resume_delivery: PersistedProgressDeliveryState::Queued,
        }
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_compact_snapshot_serialization() {
        const ITERATIONS: usize = 200;
        let mut snapshot = sample_snapshot();
        let pane_id = snapshot.panes[1].node_id;
        let mut log = PaneDebugLog::default();
        for sequence in 0..1_000 {
            let _ = log.append(
                1_700_000_000_000 + sequence,
                AgentDebugSource::Pty,
                AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::PromptSubmitted,
                    format!("diagnostic event {sequence} with representative payload"),
                ),
            );
        }
        snapshot
            .agent_debug_logs
            .push(PaneDebugLogSnapshot { pane_id, log });

        let pretty_started_at = std::time::Instant::now();
        let mut pretty_len = 0;
        for _iteration in 0..ITERATIONS {
            pretty_len = serde_json::to_vec_pretty(&snapshot).unwrap().len();
        }
        let pretty_elapsed = pretty_started_at.elapsed();

        let compact_started_at = std::time::Instant::now();
        let mut compact_len = 0;
        for _iteration in 0..ITERATIONS {
            compact_len = serde_json::to_vec(&snapshot).unwrap().len();
        }
        let compact_elapsed = compact_started_at.elapsed();
        println!(
            "PERF server.snapshot_json pretty_ns={} compact_ns={} pretty_bytes={} compact_bytes={}",
            pretty_elapsed.as_nanos() / ITERATIONS as u128,
            compact_elapsed.as_nanos() / ITERATIONS as u128,
            pretty_len,
            compact_len,
        );
    }

    #[tokio::test]
    #[ignore = "manual performance benchmark"]
    async fn benchmark_debug_snapshot_removed_from_tree_lock() {
        let recorder = crate::agent_debug::AgentDebugRecorder::new(true);
        for sequence in 0..5_000 {
            let _ = recorder
                .append(
                    NodeId(7),
                    AgentDebugSource::Pty,
                    AgentDebugContext::default(),
                    AgentDebugEventDraft::information(
                        AgentDebugEventKind::PromptSubmitted,
                        format!("diagnostic event {sequence} with representative payload"),
                    ),
                )
                .await;
        }

        let started_at = std::time::Instant::now();
        let snapshot = recorder.snapshot().await;
        let elapsed = started_at.elapsed();
        std::hint::black_box(snapshot);
        println!(
            "PERF server.snapshot_debug_clone removed_lock_hold_ns={}",
            elapsed.as_nanos(),
        );
    }

    #[tokio::test]
    async fn load_on_a_missing_path_returns_none() {
        let path = scratch_snapshot_path();
        assert_eq!(load_snapshot(&path).await.unwrap(), None);
    }

    #[tokio::test]
    async fn save_then_load_round_trips_the_full_snapshot() {
        let path = scratch_snapshot_path();
        let mut snapshot = sample_snapshot();
        let pane_id = snapshot.panes[1].node_id;
        snapshot.tree.set_node_bookmarked(pane_id, true).unwrap();
        let mut log = PaneDebugLog::default();
        let _ = log.append(
            1_700_000_000_000,
            AgentDebugSource::Pty,
            AgentDebugContext::default(),
            AgentDebugEventDraft::information(
                AgentDebugEventKind::PromptSubmitted,
                "Prompt submitted",
            ),
        );
        snapshot
            .agent_debug_logs
            .push(PaneDebugLogSnapshot { pane_id, log });

        write_snapshot_to(&path, &snapshot).await.unwrap();
        let loaded = load_snapshot(&path).await.unwrap().expect("just wrote it");

        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn snapshots_from_before_agent_debug_history_default_to_empty_logs() {
        let snapshot = sample_snapshot();
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        old_shape
            .as_object_mut()
            .expect("snapshot serializes as an object")
            .remove("agent_debug_logs");

        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();

        assert!(loaded.agent_debug_logs.is_empty());
    }

    #[test]
    fn snapshots_from_before_progress_lifecycle_default_to_no_monitors() {
        let snapshot = sample_snapshot();
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        old_shape
            .as_object_mut()
            .expect("snapshot serializes as an object")
            .remove("progress_monitors");

        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();

        assert!(loaded.progress_monitors.is_empty());
    }

    #[test]
    fn early_progress_snapshots_default_missing_delivery_bookkeeping_safely() {
        let mut snapshot = sample_snapshot();
        let pane_id = snapshot.panes[1].node_id;
        snapshot
            .progress_monitors
            .push(sample_persisted_progress(pane_id));
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        let monitor = old_shape
            .get_mut("progress_monitors")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|monitors| monitors.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .expect("fixture has one serialized progress monitor");
        monitor.remove("goal_resume_armed");
        monitor.remove("result_delivery");
        monitor.remove("goal_resume_delivery");

        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();
        let monitor = &loaded.progress_monitors[0];
        assert!(!monitor.goal_resume_armed);
        assert_eq!(
            monitor.result_delivery,
            PersistedProgressDeliveryState::NotQueued
        );
        assert_eq!(
            monitor.goal_resume_delivery,
            PersistedProgressDeliveryState::NotQueued
        );
    }

    #[tokio::test]
    async fn progress_monitor_state_round_trips_with_the_session_snapshot() {
        let path = scratch_snapshot_path();
        let mut snapshot = sample_snapshot();
        let pane_id = snapshot.panes[1].node_id;
        snapshot
            .progress_monitors
            .push(sample_persisted_progress(pane_id));

        write_snapshot_to(&path, &snapshot).await.unwrap();
        let loaded = load_snapshot(&path).await.unwrap().unwrap();

        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn restore_rewrites_generation_and_disarms_ambiguous_goal_ownership() {
        let mut persisted = sample_persisted_progress(NodeId(17));
        persisted.disarm_ambiguous_goal_resume();
        let restored = persisted.restored_registration(99).unwrap();

        assert_eq!(restored.monitor_id, 99);
        assert_eq!(restored.initial_progress.monitor_id, 99);
        assert_eq!(restored.initial_progress.report.job_id, "render-job-7");
        assert!(!persisted.goal_resume_armed);
        assert_eq!(
            persisted.goal_resume_delivery,
            PersistedProgressDeliveryState::Uncertain
        );
        assert!(!persisted.goal_resume_delivery.may_retry_after_restart());
    }

    #[test]
    fn restore_rejects_invalid_persisted_registration_parameters() {
        let mut persisted = sample_persisted_progress(NodeId(17));
        persisted.interval_seconds = 0;
        assert!(persisted.restored_registration(99).is_err());

        persisted.interval_seconds = 15;
        assert!(persisted.restored_registration(0).is_err());

        persisted.command.clear();
        assert!(persisted.restored_registration(99).is_err());
    }

    #[test]
    fn only_never_attempted_queued_delivery_is_retryable_after_restart() {
        assert!(PersistedProgressDeliveryState::Queued.may_retry_after_restart());
        for state in [
            PersistedProgressDeliveryState::NotQueued,
            PersistedProgressDeliveryState::Attempted,
            PersistedProgressDeliveryState::DeliveredToPty,
            PersistedProgressDeliveryState::Uncertain,
        ] {
            assert!(!state.may_retry_after_restart(), "{state:?}");
        }
    }

    #[test]
    fn restore_probe_is_required_only_for_still_observable_nonterminal_tasks() {
        let mut persisted = sample_persisted_progress(NodeId(17));
        assert!(persisted.requires_probe_before_restore());

        persisted.latest_progress.report.status = ProgressTaskStatus::Done;
        persisted.latest_progress.report.percent = 100.0;
        assert!(!persisted.requires_probe_before_restore());

        persisted.latest_progress.report.status = ProgressTaskStatus::Running;
        persisted.latest_progress.report.percent = 72.5;
        persisted.latest_progress.monitor_health = ilium_core::ProgressMonitorHealth::Failed {
            consecutive_failures: 3,
            last_error: "probe unavailable".to_string(),
        };
        assert!(!persisted.requires_probe_before_restore());
    }

    #[test]
    fn restored_preflight_must_keep_the_same_job_identity() {
        let persisted = sample_persisted_progress(NodeId(17));
        let matching = ilium_ipc::ProgressMonitorPreflight {
            report: ProgressTaskReport::new(
                "render-job-7".to_string(),
                ProgressTaskStatus::Running,
                73.0,
                "frame 730/1000".to_string(),
                None,
            )
            .unwrap(),
            checked_at_unix_millis: 1_700_000_000_100,
        };
        let replacement = ilium_ipc::ProgressMonitorPreflight {
            report: ProgressTaskReport::new(
                "render-job-8".to_string(),
                ProgressTaskStatus::Running,
                1.0,
                "different task".to_string(),
                None,
            )
            .unwrap(),
            checked_at_unix_millis: 1_700_000_000_100,
        };

        assert!(persisted.accepts_restored_preflight(&matching));
        assert!(!persisted.accepts_restored_preflight(&replacement));
    }

    #[test]
    fn snapshots_from_before_bookmarks_default_every_node_to_unbookmarked() {
        fn remove_bookmark_fields(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    fields.remove("is_bookmarked");
                    for child in fields.values_mut() {
                        remove_bookmark_fields(child);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        remove_bookmark_fields(item);
                    }
                }
                _ => {}
            }
        }

        let snapshot = sample_snapshot();
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        remove_bookmark_fields(&mut old_shape);
        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();

        assert!(loaded.tree.all_ids().all(|node_id| {
            !loaded
                .tree
                .get(node_id)
                .expect("all ids resolve to tree nodes")
                .is_bookmarked
        }));
    }

    #[test]
    fn snapshots_from_before_activity_tracking_have_no_trusted_checkpoints() {
        fn remove_activity_fields(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    fields.remove("activity_revision");
                    fields.remove("last_restructure_activity_revision");
                    fields.remove("last_focus_activity_revision");
                    for child in fields.values_mut() {
                        remove_activity_fields(child);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        remove_activity_fields(item);
                    }
                }
                _ => {}
            }
        }

        let snapshot = sample_snapshot();
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        remove_activity_fields(&mut old_shape);
        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();

        assert!(loaded.tree.all_ids().all(|node_id| {
            let node = loaded
                .tree
                .get(node_id)
                .expect("all ids resolve to tree nodes");
            node.activity_revision == 0
                && node.last_restructure_activity_revision.is_none()
                && node.last_focus_activity_revision.is_none()
        }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_directory_and_file_are_private_when_debug_history_is_persisted() {
        use std::os::unix::fs::PermissionsExt;

        let path = scratch_snapshot_path();
        write_snapshot_to(&path, &sample_snapshot()).await.unwrap();

        let directory_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[tokio::test]
    async fn pending_scheduled_input_round_trips_with_the_session_snapshot() {
        let path = scratch_snapshot_path();
        let mut snapshot = sample_snapshot();
        let pane_id = snapshot.panes[0].node_id;
        snapshot
            .tree
            .schedule_pane_input(
                pane_id,
                ilium_core::ScheduledPaneInput {
                    execute_at_unix_millis: 9_999_999,
                    text: "continue".to_string(),
                    send_enter: true,
                },
            )
            .unwrap();

        write_snapshot_to(&path, &snapshot).await.unwrap();
        let loaded = load_snapshot(&path).await.unwrap().unwrap();

        assert_eq!(loaded, snapshot);
        assert_eq!(loaded.tree.scheduled_pane_inputs().count(), 1);
    }

    #[test]
    fn snapshots_from_before_scheduled_input_default_to_no_pending_action() {
        fn remove_scheduled_input_fields(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    fields.remove("scheduled_input");
                    for child in fields.values_mut() {
                        remove_scheduled_input_fields(child);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        remove_scheduled_input_fields(item);
                    }
                }
                _ => {}
            }
        }

        let snapshot = sample_snapshot();
        let mut old_shape = serde_json::to_value(snapshot).unwrap();
        remove_scheduled_input_fields(&mut old_shape);
        let loaded: SessionSnapshot = serde_json::from_value(old_shape).unwrap();

        assert_eq!(loaded.tree.scheduled_pane_inputs().count(), 0);
    }

    #[tokio::test]
    async fn a_second_save_overwrites_the_first_atomically_and_leaves_no_temp_file() {
        let path = scratch_snapshot_path();
        let first = sample_snapshot();
        let mut second = sample_snapshot();
        second
            .tree
            .rename_node(ROOT_ID, "renamed root", None, None)
            .unwrap();

        write_snapshot_to(&path, &first).await.unwrap();
        write_snapshot_to(&path, &second).await.unwrap();

        let loaded = load_snapshot(&path).await.unwrap().unwrap();
        assert_eq!(loaded, second);

        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }

    #[tokio::test]
    async fn corrupt_snapshot_file_is_an_error_not_a_panic() {
        let path = scratch_snapshot_path();
        tokio::fs::write(&path, b"not valid json at all")
            .await
            .unwrap();

        let result = load_snapshot(&path).await;
        assert!(matches!(result, Err(ServerError::Snapshot { .. })));
    }

    /// A snapshot can be syntactically valid JSON that still deserializes
    /// into a structurally invalid `Tree` (e.g. hand-edited, or truncated
    /// mid-write by a crash) -- distinct from
    /// `corrupt_snapshot_file_is_an_error_not_a_panic` above, which only
    /// covers a JSON syntax error. Without `Tree::validate` this loads
    /// successfully and panics much later, in an unrelated tree operation
    /// that assumes the invariant the corruption broke.
    #[tokio::test]
    async fn structurally_corrupt_tree_is_rejected_at_load_not_panicked_on_later() {
        let path = scratch_snapshot_path();
        let mut corrupted = serde_json::to_value(sample_snapshot()).unwrap();
        // `next_id` colliding with an id already in use would let a later
        // `alloc_id` silently overwrite an existing node.
        corrupted["tree"]["next_id"] = serde_json::json!(0);
        tokio::fs::write(&path, serde_json::to_vec(&corrupted).unwrap())
            .await
            .unwrap();

        let result = load_snapshot(&path).await;

        assert!(matches!(
            result,
            Err(ServerError::Snapshot {
                source: SnapshotError::InvalidTree(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn migrates_a_legacy_workspace_and_drops_unverifiable_resume_commands() {
        let directory = tempfile::tempdir().expect("tempdir");
        let legacy_directory = directory.path().join(".ilium");
        let legacy_path = legacy_directory.join("sessions.yml");
        let snapshot_path = directory.path().join("default.snapshot.json");
        std::fs::create_dir_all(&legacy_directory).expect("create legacy directory");
        std::fs::write(
            &legacy_path,
            r#"version: 1
root:
  - kind: group
    name: default
    children:
      - kind: terminal
        name: shell
        agent: null
      - kind: terminal
        name: claude work
        agent:
          command: claude
          session_id: claude-session
        title_source: auto
      - kind: terminal
        name: codex work
        agent:
          command: codex
          session_id: codex-session
        title_source: user
      - kind: editor
        name: notes.md
        path: /tmp/notes.md
"#,
        )
        .expect("write legacy workspace");

        let snapshot = load_snapshot_or_migrate(&snapshot_path, directory.path(), directory.path())
            .await
            .expect("migrate legacy workspace")
            .expect("migrated snapshot");

        assert!(legacy_path.is_file(), "legacy file must remain recoverable");
        assert_eq!(snapshot.panes.len(), 4);
        assert!(snapshot.panes.iter().any(|pane| matches!(
            &pane.kind,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(command))
                if command == "claude"
        )));
        assert!(snapshot.panes.iter().any(|pane| matches!(
            &pane.kind,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(command))
                if command == "codex"
        )));
        assert_eq!(
            load_snapshot(&snapshot_path)
                .await
                .expect("load native snapshot"),
            Some(snapshot)
        );
        assert_eq!(
            load_snapshot_or_migrate(&snapshot_path, directory.path(), directory.path(),)
                .await
                .expect("load existing native snapshot"),
            load_snapshot(&snapshot_path)
                .await
                .expect("reload native snapshot")
        );
    }

    #[test]
    fn cross_project_and_duplicate_resume_bindings_become_fresh_agents() {
        let directory = tempfile::tempdir().unwrap();
        let project_cwd = directory.path().join("money");
        let other_cwd = directory.path().join("ilium");
        std::fs::create_dir_all(&project_cwd).unwrap();
        std::fs::create_dir_all(&other_cwd).unwrap();
        let session_id = "11111111-1111-4111-8111-111111111111";
        let rollout_directory = directory.path().join(".codex/sessions/2026/07/14");
        std::fs::create_dir_all(&rollout_directory).unwrap();
        std::fs::write(
            rollout_directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl")),
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": other_cwd}
            })
            .to_string(),
        )
        .unwrap();
        let mut snapshot = sample_snapshot();
        let restored_group = snapshot.tree.add_group(ROOT_ID, "restored").unwrap();
        let cross_project_pane = snapshot
            .tree
            .add_pane(
                restored_group,
                "Cross Project Title Must Not Survive",
                PaneContentKind::Terminal,
            )
            .unwrap();
        snapshot
            .tree
            .set_automatic_pane_title(
                cross_project_pane,
                "Cross Project Title Must Not Survive",
                Some("Wrong Project".to_string()),
                None,
            )
            .unwrap();
        let user_named_pane = snapshot
            .tree
            .add_pane(restored_group, "codex", PaneContentKind::Terminal)
            .unwrap();
        snapshot
            .tree
            .rename_node(user_named_pane, "My Persistent Name", None, None)
            .unwrap();
        snapshot.panes.push(PaneSnapshot {
            node_id: cross_project_pane,
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                "codex resume '{session_id}'"
            ))),
        });
        snapshot.panes.push(PaneSnapshot {
            node_id: user_named_pane,
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                "codex resume '{session_id}'"
            ))),
        });

        assert!(normalize_agent_resumes(
            &mut snapshot,
            directory.path(),
            &project_cwd
        ));

        // Assert on the two specific node ids rather than `any`/count over the
        // whole pane list so one repaired binding cannot conceal another.
        let command_for = |node_id: NodeId| {
            snapshot
                .panes
                .iter()
                .find(|pane| pane.node_id == node_id)
                .and_then(|pane| match &pane.kind {
                    PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) => {
                        Some(command.clone())
                    }
                    _ => None,
                })
                .expect("pane with this node id exists and is a command")
        };
        assert_eq!(
            command_for(cross_project_pane),
            "codex",
            "a transcript owned by ilium cannot be resumed in money"
        );
        assert_eq!(
            command_for(user_named_pane),
            "codex",
            "a duplicate cross-project binding is also removed"
        );
        let repaired_node = snapshot.tree.get(cross_project_pane).unwrap();
        assert_eq!(repaired_node.name, "codex");
        assert_eq!(repaired_node.short_name, None);
        assert_eq!(
            snapshot.tree.get(user_named_pane).unwrap().name,
            "My Persistent Name",
            "normalization must never overwrite a user-specified title"
        );
    }

    #[test]
    fn keeps_one_project_verified_resume_and_removes_its_duplicate() {
        let directory = tempfile::tempdir().unwrap();
        let project_cwd = directory.path().join("ilium");
        std::fs::create_dir_all(&project_cwd).unwrap();
        let session_id = "22222222-2222-4222-8222-222222222222";
        let rollout_directory = directory.path().join(".codex/sessions/2026/07/14");
        std::fs::create_dir_all(&rollout_directory).unwrap();
        std::fs::write(
            rollout_directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl")),
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": project_cwd}
            })
            .to_string(),
        )
        .unwrap();
        let mut snapshot = sample_snapshot();
        let restored_group = snapshot.tree.add_group(ROOT_ID, "restored").unwrap();
        let verified_pane = snapshot
            .tree
            .add_pane(
                restored_group,
                "Verified Project Title",
                PaneContentKind::Terminal,
            )
            .unwrap();
        let duplicate_pane = snapshot
            .tree
            .add_pane(
                restored_group,
                "Duplicate Project Title",
                PaneContentKind::Terminal,
            )
            .unwrap();
        snapshot.panes.push(PaneSnapshot {
            node_id: verified_pane,
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                "codex resume {session_id}"
            ))),
        });
        snapshot.panes.push(PaneSnapshot {
            node_id: duplicate_pane,
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                "codex resume '{session_id}'"
            ))),
        });

        assert!(normalize_agent_resumes(
            &mut snapshot,
            directory.path(),
            &project_cwd
        ));

        let command_for = |node_id: NodeId| {
            snapshot
                .panes
                .iter()
                .find(|pane| pane.node_id == node_id)
                .and_then(|pane| match &pane.kind {
                    PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) => {
                        Some(command.as_str())
                    }
                    _ => None,
                })
                .expect("pane command")
        };
        assert_eq!(
            command_for(verified_pane),
            format!("codex resume {session_id}")
        );
        assert_eq!(command_for(duplicate_pane), "codex");
        assert_eq!(
            snapshot.tree.get(verified_pane).unwrap().name,
            "Verified Project Title"
        );
        assert_eq!(snapshot.tree.get(duplicate_pane).unwrap().name, "codex");
    }

    #[test]
    fn an_earlier_unverified_duplicate_does_not_block_a_later_verified_resume() {
        // Regression test: `normalize_agent_resumes` must only let a
        // *project-verified* session id occupy the "already claimed" slot.
        // Two panes can end up sharing one corrupted/duplicated session id
        // across two different projects (exactly the scenario this function
        // exists to repair); only one of them can possibly be the real
        // owner. Processing the unverifiable one first must not cause the
        // later, legitimately verified pane to be misdiagnosed as a
        // duplicate and have its valid resume destroyed.
        let directory = tempfile::tempdir().unwrap();
        let owning_project_cwd = directory.path().join("owner");
        let other_project_cwd = directory.path().join("other");
        std::fs::create_dir_all(&owning_project_cwd).unwrap();
        std::fs::create_dir_all(&other_project_cwd).unwrap();
        let session_id = "55555555-5555-4555-8555-555555555555";
        let rollout_directory = directory.path().join(".codex/sessions/2026/07/14");
        std::fs::create_dir_all(&rollout_directory).unwrap();
        std::fs::write(
            rollout_directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl")),
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": owning_project_cwd}
            })
            .to_string(),
        )
        .unwrap();

        let mut tree = Tree::new();
        // The unverified pane's project is added -- and therefore appears
        // first in `snapshot.panes` -- so it is the one that would have
        // wrongly claimed `session_id` under the pre-fix logic.
        let other_project = tree.add_project(other_project_cwd).unwrap();
        let other_group = tree.add_group(other_project, "restored").unwrap();
        let unverified_pane = tree
            .add_pane(other_group, "codex", PaneContentKind::Terminal)
            .unwrap();

        let owning_project = tree.add_project(owning_project_cwd.clone()).unwrap();
        let owning_group = tree.add_group(owning_project, "restored").unwrap();
        let verified_pane = tree
            .add_pane(owning_group, "codex", PaneContentKind::Terminal)
            .unwrap();

        let mut snapshot = SessionSnapshot {
            version: CURRENT_SNAPSHOT_VERSION,
            tree,
            panes: vec![
                PaneSnapshot {
                    node_id: unverified_pane,
                    kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                        "codex resume '{session_id}'"
                    ))),
                },
                PaneSnapshot {
                    node_id: verified_pane,
                    kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                        "codex resume '{session_id}'"
                    ))),
                },
            ],
            agent_debug_logs: Vec::new(),
            progress_monitors: Vec::new(),
        };

        assert!(normalize_agent_resumes(
            &mut snapshot,
            directory.path(),
            &owning_project_cwd,
        ));

        let command_for = |node_id: NodeId| {
            snapshot
                .panes
                .iter()
                .find(|pane| pane.node_id == node_id)
                .and_then(|pane| match &pane.kind {
                    PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) => {
                        Some(command.clone())
                    }
                    _ => None,
                })
                .expect("pane command")
        };

        assert_eq!(
            command_for(unverified_pane),
            "codex",
            "the pane in the non-owning project must not resume another project's session"
        );
        assert_eq!(
            command_for(verified_pane),
            format!("codex resume '{session_id}'"),
            "an earlier unverified duplicate must not block a later pane's verified resume"
        );
    }

    #[test]
    fn snapshot_rewrites_standard_agent_origins_from_current_identity() {
        let old_session_id = "33333333-3333-4333-8333-333333333333";
        let new_session_id = "44444444-4444-4444-8444-444444444444";
        let old_origin =
            TerminalOrigin::Command(format!("codex resume {}", shell_quote(old_session_id)));

        assert_eq!(
            snapshot_origin_from_identity(&old_origin, Some(new_session_id), false),
            TerminalOrigin::Command(format!("codex resume {}", shell_quote(new_session_id)))
        );
        assert_eq!(
            snapshot_origin_from_identity(&old_origin, None, true),
            TerminalOrigin::Command("codex".to_string())
        );
        let old_antigravity_origin = TerminalOrigin::Command(format!(
            "agy --conversation {}",
            shell_quote(old_session_id)
        ));
        assert_eq!(
            snapshot_origin_from_identity(&old_antigravity_origin, Some(new_session_id), false),
            TerminalOrigin::Command(format!(
                "agy --conversation {}",
                shell_quote(new_session_id)
            ))
        );
        assert_eq!(
            snapshot_origin_from_identity(&old_antigravity_origin, None, true),
            TerminalOrigin::Command("agy".to_string())
        );
        assert_eq!(
            snapshot_origin_from_identity(
                &TerminalOrigin::Command("custom-agent --resume value".to_string()),
                Some(new_session_id),
                true,
            ),
            TerminalOrigin::Command("custom-agent --resume value".to_string())
        );
    }
}
