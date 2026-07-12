//! Crash-recovery snapshot: a JSON dump of the session's tree plus enough
//! per-pane metadata to respawn each pane the same way, written to
//! `<data_dir>/<session>.snapshot.json` after every structural tree change.
//!
//! This is explicitly **not** a database and **not** the source of truth
//! while the server is running -- `ServerState::tree`/`ServerState::panes`
//! are that. It exists so a killed/crashed server can be restarted and the
//! same tree shape (plus what to relaunch in each terminal pane) recovered,
//! matching the role `illium/src/workspace_file.rs` played in the
//! pre-refactor single-process bin -- that file's `SavedNode`/`SavedAgent`
//! shape is not reused verbatim here, because this crate already has a
//! precise "how to respawn this pane" type in [`crate::pane::TerminalOrigin`]
//! (derived from `illium_ipc::NewPaneKind`, the same shape a client sends
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

use std::path::Path;

use illium_core::{NodeId, Tree};
use serde::{Deserialize, Serialize};

use crate::error::{ServerError, SnapshotError};
use crate::pane::{PaneResource, PaneSnapshotKind};
use crate::state::ServerState;

/// Bumped whenever `SessionSnapshot`'s shape changes incompatibly. Not
/// currently enforced on load -- see `workspace_file::CURRENT_VERSION`'s
/// identical comment for why that's an acceptable, deliberate choice for a
/// best-effort recovery file.
const CURRENT_SNAPSHOT_VERSION: u32 = 1;

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
                    PaneSnapshotKind::Terminal(runtime.origin.clone())
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

/// Builds and writes the current snapshot to `state.snapshot_path`.
/// Best-effort by design (see module docs): callers (the IPC request
/// handlers, after every structural change) are expected to log an `Err`
/// and continue, never to treat a failed snapshot write as a reason to
/// reject the request that triggered it or to crash the server.
pub async fn save_snapshot(state: &ServerState) -> Result<(), ServerError> {
    let snapshot = build_snapshot(state).await;
    write_snapshot_to(&state.snapshot_path, &snapshot).await
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
    use illium_core::{PaneContentKind, ROOT_ID};

    fn scratch_snapshot_path() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-server-persistence-tests")
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
        second.tree.rename_node(ROOT_ID, "renamed root").unwrap();

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
}
