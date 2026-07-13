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
//! Known gap, deliberately deferred: this module does not yet resume an
//! agent CLI's own session (`claude --resume <id>` / `codex resume <id>`)
//! the way `workspace_file.rs`'s `SavedAgent::session_id` did -- that
//! requires the session-ID screen-scraping logic in the pre-refactor bin
//! crate's `agent_detect.rs`, which is app-level orchestration tied to
//! pane-creation flows that don't exist in this crate yet. A pane
//! recovered from a snapshot respawns as a fresh (non-resumed) invocation
//! of the same command.
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
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use ilium_core::{NodeId, PaneContentKind, Tree, ROOT_ID};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::error::{ServerError, SnapshotError};
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
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
const CURRENT_SNAPSHOT_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub node_id: NodeId,
    pub kind: PaneSnapshotKind,
}

/// Loads the native project-local snapshot if present, otherwise imports the
/// preceding project-local YAML workspace. The YAML source is deliberately
/// retained: it is a user-owned recovery artifact, not disposable seed data.
pub async fn load_snapshot_or_migrate(
    snapshot_path: &Path,
    session_cwd: &Path,
) -> Result<Option<SessionSnapshot>, ServerError> {
    if let Some(mut snapshot) = load_snapshot(snapshot_path).await? {
        if normalize_duplicate_agent_resumes(&mut snapshot) {
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
    let mut snapshot = legacy_workspace.into_snapshot()?;
    // Legacy `sessions.yml` files could already carry the same agent
    // session id on several titled panes; collapse those duplicates before
    // ever writing/spawning from this migrated snapshot, the same way a
    // reloaded native snapshot already does above.
    normalize_duplicate_agent_resumes(&mut snapshot);

    write_snapshot_to(snapshot_path, &snapshot).await?;
    tracing::info!("imported legacy workspace into {}", snapshot_path.display());
    Ok(Some(snapshot))
}

/// Ensures one saved agent session is resumed by at most one pane. Legacy
/// saves could incorrectly assign the same session id to several titled
/// panes; resuming each copy opens identical agent conversations. Keep the
/// first verifiable resume binding and start the remaining panes as fresh,
/// independent agents rather than inventing session ids from pane titles.
fn normalize_duplicate_agent_resumes(snapshot: &mut SessionSnapshot) -> bool {
    let mut seen_resumes = HashSet::new();
    let mut changed = false;
    for pane in &mut snapshot.panes {
        let PaneSnapshotKind::Terminal(TerminalOrigin::Command(command)) = &mut pane.kind else {
            continue;
        };
        let Some(agent_command) = resumed_agent_command(command) else {
            continue;
        };
        if seen_resumes.insert(command.clone()) {
            continue;
        }
        *command = agent_command.to_string();
        changed = true;
    }
    changed
}

/// Returns the bare command only for a recognized persisted resume form.
fn resumed_agent_command(command: &str) -> Option<&'static str> {
    command
        .starts_with("claude --resume ")
        .then_some("claude")
        .or_else(|| command.starts_with("codex resume ").then_some("codex"))
}

impl LegacyWorkspace {
    /// Converts the old YAML tree into the server's native persistence
    /// contract, retaining the exact commands needed to resume known agents.
    fn into_snapshot(self) -> Result<SessionSnapshot, ServerError> {
        let mut tree = Tree::new();
        let mut panes = Vec::new();
        let fallback_group = self
            .root
            .iter()
            .any(|node| !matches!(node, LegacyNode::Group { .. }))
            .then(|| tree.add_group(ROOT_ID, "default"))
            .transpose()?;

        for node in self.root {
            let parent = if matches!(&node, LegacyNode::Group { .. }) {
                ROOT_ID
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
                tree.rename_node(pane_id, name, None)?;
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
    let quoted_session_id = shell_quote(&session_id);
    let command_line = match agent.command.as_str() {
        "claude" => format!("claude --resume {quoted_session_id}"),
        "codex" => format!("codex resume {quoted_session_id}"),
        _ => agent.command,
    };
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
    let tree = state.tree.read().await;
    let panes = state.panes.read().await;
    let pane_snapshots = panes
        .iter()
        .map(|(node_id, resource)| PaneSnapshot {
            node_id: *node_id,
            kind: match resource {
                PaneResource::Terminal(runtime) => {
                    PaneSnapshotKind::Terminal(snapshot_terminal_origin(runtime))
                }
                PaneResource::Editor { path } => PaneSnapshotKind::Editor { path: path.clone() },
            },
        })
        .collect();
    SessionSnapshot {
        version: CURRENT_SNAPSHOT_VERSION,
        tree: tree.clone(),
        panes: pane_snapshots,
    }
}

/// Turns a freshly-launched agent command into its resume form once the
/// detection loop has authoritatively discovered that CLI's session id.
fn snapshot_terminal_origin(runtime: &crate::pane::TerminalPaneRuntime) -> TerminalOrigin {
    let Some(session_id) = runtime.session_id.as_deref() else {
        return runtime.origin.clone();
    };
    match &runtime.origin {
        TerminalOrigin::Command(command) if command == "claude" => {
            TerminalOrigin::Command(format!("claude --resume {}", shell_quote(session_id)))
        }
        TerminalOrigin::Command(command) if command == "codex" => {
            TerminalOrigin::Command(format!("codex resume {}", shell_quote(session_id)))
        }
        _ => runtime.origin.clone(),
    }
}

/// Builds and writes the current snapshot to `state.snapshot_path`.
/// Best-effort by design (see module docs): callers are expected to log an
/// `Err` and continue, never to treat a failed snapshot write as a reason
/// to reject a request or to crash the server. Request handlers never
/// call this directly on the request path -- see
/// [`spawn_snapshot_writer`]/[`flush_pending_snapshot`] and
/// `ServerState::request_snapshot_save`.
pub async fn save_snapshot(state: &ServerState) -> Result<(), ServerError> {
    let _write_guard = state.snapshot_write_lock.lock().await;
    let snapshot = build_snapshot(state).await;
    write_snapshot_to(&state.snapshot_path, &snapshot).await
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
    if !state.snapshot_dirty.swap(false, Ordering::AcqRel) {
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
    let json = serde_json::to_vec_pretty(snapshot).map_err(|source| ServerError::Snapshot {
        operation: "serialize",
        path: path.to_path_buf(),
        source: SnapshotError::Json(source),
    })?;

    let to_snapshot_io_error = |source: std::io::Error| ServerError::Snapshot {
        operation: "write",
        path: path.to_path_buf(),
        source: SnapshotError::Io(source),
    };

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(to_snapshot_io_error)?;
    let temp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "snapshot".to_string()),
        std::process::id()
    ));
    tokio::fs::write(&temp_path, &json)
        .await
        .map_err(to_snapshot_io_error)?;
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(to_snapshot_io_error)?;
    Ok(())
}

/// Reads and parses the snapshot at `path`. `Ok(None)` means no snapshot
/// exists yet (a brand-new session) -- distinct from `Err`, which means
/// one exists but couldn't be read or parsed (e.g. hand-edited into
/// invalid JSON), so the caller can log "nothing to recover" separately
/// from "something to warn about."
pub async fn load_snapshot(path: &Path) -> Result<Option<SessionSnapshot>, ServerError> {
    let exists = tokio::fs::try_exists(path)
        .await
        .map_err(|source| ServerError::Snapshot {
            operation: "check existence of",
            path: path.to_path_buf(),
            source: SnapshotError::Io(source),
        })?;
    if !exists {
        return Ok(None);
    }

    let contents = tokio::fs::read(path)
        .await
        .map_err(|source| ServerError::Snapshot {
            operation: "read",
            path: path.to_path_buf(),
            source: SnapshotError::Io(source),
        })?;
    let snapshot: SessionSnapshot =
        serde_json::from_slice(&contents).map_err(|source| ServerError::Snapshot {
            operation: "parse",
            path: path.to_path_buf(),
            source: SnapshotError::Json(source),
        })?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::pane::TerminalOrigin;
    use ilium_core::{PaneContentKind, ROOT_ID};

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
        }
    }

    #[tokio::test]
    async fn load_on_a_missing_path_returns_none() {
        let path = scratch_snapshot_path();
        assert_eq!(load_snapshot(&path).await.unwrap(), None);
    }

    #[tokio::test]
    async fn save_then_load_round_trips_the_full_snapshot() {
        let path = scratch_snapshot_path();
        let snapshot = sample_snapshot();

        write_snapshot_to(&path, &snapshot).await.unwrap();
        let loaded = load_snapshot(&path).await.unwrap().expect("just wrote it");

        assert_eq!(loaded, snapshot);
    }

    #[tokio::test]
    async fn a_second_save_overwrites_the_first_atomically_and_leaves_no_temp_file() {
        let path = scratch_snapshot_path();
        let first = sample_snapshot();
        let mut second = sample_snapshot();
        second
            .tree
            .rename_node(ROOT_ID, "renamed root", None)
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

    #[tokio::test]
    async fn migrates_a_legacy_workspace_once_and_preserves_agent_resume_commands() {
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

        let snapshot = load_snapshot_or_migrate(&snapshot_path, directory.path())
            .await
            .expect("migrate legacy workspace")
            .expect("migrated snapshot");

        assert!(legacy_path.is_file(), "legacy file must remain recoverable");
        assert_eq!(snapshot.panes.len(), 4);
        assert!(snapshot.panes.iter().any(|pane| matches!(
            &pane.kind,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(command))
                if command == "claude --resume 'claude-session'"
        )));
        assert!(snapshot.panes.iter().any(|pane| matches!(
            &pane.kind,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(command))
                if command == "codex resume 'codex-session'"
        )));
        assert_eq!(
            load_snapshot(&snapshot_path)
                .await
                .expect("load native snapshot"),
            Some(snapshot)
        );
        assert_eq!(
            load_snapshot_or_migrate(&snapshot_path, directory.path())
                .await
                .expect("load existing native snapshot"),
            load_snapshot(&snapshot_path)
                .await
                .expect("reload native snapshot")
        );
    }

    #[test]
    fn duplicate_agent_resume_bindings_become_fresh_independent_agents() {
        let mut snapshot = sample_snapshot();
        snapshot.panes.push(PaneSnapshot {
            node_id: NodeId(99),
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(
                "claude --resume 'same-session'".to_string(),
            )),
        });
        snapshot.panes.push(PaneSnapshot {
            node_id: NodeId(100),
            kind: PaneSnapshotKind::Terminal(TerminalOrigin::Command(
                "claude --resume 'same-session'".to_string(),
            )),
        });

        assert!(normalize_duplicate_agent_resumes(&mut snapshot));

        // Assert on the two specific node ids rather than `any`/count over the
        // whole pane list -- this is the only way to actually verify the
        // documented contract ("keep the *first* verifiable binding"): if the
        // function instead kept node 100 and converted node 99, a looser
        // `any`/count-based assertion would still pass.
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
            command_for(NodeId(99)),
            "claude --resume 'same-session'",
            "the first duplicate keeps its resume binding"
        );
        assert_eq!(
            command_for(NodeId(100)),
            "claude",
            "the later duplicate becomes a fresh Claude invocation"
        );
    }
}
