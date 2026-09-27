//! Durable, per-spawn refusal tokens for worktree deletion.
//! Every token is created and fsynced before a PTY may start. Only a
//! proven-stopped lineage and a successful directory-user scan may erase it.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ilium_platform::secure_fs::NoFollowDirectory;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workspace_owner::{self, OwnershipMarker};

const PREFIX: &str = "ilium-workspace-custody-";
const SUFFIX: &str = ".json";
const MAX_TICKET_BYTES: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CustodyTicket {
    metadata_directory: PathBuf,
    worktree_root: PathBuf,
    file_name: String,
    workspace_id: String,
    ticket_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketContents {
    workspace_id: String,
    ticket_id: String,
}

fn io_error(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}: {error}")
}

fn exact_bytes(workspace_id: &str, ticket_id: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&TicketContents {
        workspace_id: workspace_id.to_owned(),
        ticket_id: ticket_id.to_owned(),
    })
    .map_err(|error| io_error("cannot encode worktree custody ticket", error))
}

fn read_exact_ticket(
    directory: &NoFollowDirectory,
    ticket: &CustodyTicket,
) -> Result<std::fs::File, String> {
    let mut file = directory
        .open_regular(OsStr::new(&ticket.file_name))
        .map_err(|error| io_error("cannot open worktree custody ticket", error))?;
    if file
        .metadata()
        .map_err(|error| io_error("cannot stat custody ticket", error))?
        .len()
        > MAX_TICKET_BYTES
    {
        return Err("worktree custody ticket is too large".into());
    }
    let mut actual = Vec::new();
    (&mut file)
        .take(MAX_TICKET_BYTES + 1)
        .read_to_end(&mut actual)
        .map_err(|error| io_error("cannot read custody ticket", error))?;
    if actual != exact_bytes(&ticket.workspace_id, &ticket.ticket_id)? {
        return Err("worktree custody ticket content changed".into());
    }
    Ok(file)
}

pub(crate) async fn begin(marker: &OwnershipMarker) -> Result<CustodyTicket, String> {
    if marker.custody_revision != 1 {
        return Err("worktree ownership predates custody tracking".into());
    }
    let marker_path =
        workspace_owner::validated_marker_path_for(&marker.repo_common_dir, &marker.worktree_root)
            .await
            .map_err(|error| io_error("worktree ownership path changed", error))?;
    let metadata_directory = marker_path
        .parent()
        .ok_or("worktree marker has no metadata parent")?
        .to_path_buf();
    let ticket_id = Uuid::new_v4().to_string();
    let ticket = CustodyTicket {
        metadata_directory,
        worktree_root: marker.worktree_root.clone(),
        file_name: format!("{PREFIX}{ticket_id}{SUFFIX}"),
        workspace_id: marker.workspace_id.clone(),
        ticket_id,
    };
    let ticket_for_worker = ticket.clone();
    tokio::task::spawn_blocking(move || {
        let directory = NoFollowDirectory::open_root(&ticket_for_worker.metadata_directory)
            .map_err(|error| io_error("cannot pin worktree metadata directory", error))?;
        let mut file = directory
            .create_regular(OsStr::new(&ticket_for_worker.file_name))
            .map_err(|error| io_error("cannot reserve worktree custody ticket", error))?;
        // A failed write or sync leaves the ticket in place. That is a
        // refusal token, even when no PTY was launched.
        file.write_all(&exact_bytes(
            &ticket_for_worker.workspace_id,
            &ticket_for_worker.ticket_id,
        )?)
        .and_then(|()| file.sync_all())
        .and_then(|()| directory.sync_all())
        .map_err(|error| io_error("cannot durably publish custody ticket", error))
    })
    .await
    .map_err(|error| io_error("custody-ticket worker failed", error))??;
    Ok(ticket)
}

impl CustodyTicket {
    pub(crate) fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Call only after PTY lineage termination returned proof AND
    /// `processes_using_directory(root)` returned an empty list. Keep the
    /// token on every error, including worker cancellation or disk errors.
    pub(crate) fn clear_after_proof(self) -> Result<(), String> {
        let directory = NoFollowDirectory::open_root(&self.metadata_directory)
            .map_err(|error| io_error("cannot pin custody directory", error))?;
        let file = read_exact_ticket(&directory, &self)?;
        directory
            .remove_regular(OsStr::new(&self.file_name), &file)
            .and_then(|()| directory.sync_all())
            .map_err(|error| io_error("cannot clear proven custody ticket", error))
    }
}

/// Any matching name blocks deletion, including malformed, unreadable,
/// symlinked, or partial ticket files. Git's metadata directory is verified
/// through the same path routine as the immutable owner marker.
pub(crate) async fn require_no_tickets(
    repo_common_dir: &Path,
    worktree_root: &Path,
) -> Result<(), String> {
    let marker_path = workspace_owner::validated_marker_path_for(repo_common_dir, worktree_root)
        .await
        .map_err(|error| io_error("worktree custody path changed", error))?;
    let metadata_directory = marker_path
        .parent()
        .ok_or("worktree marker has no metadata parent")?
        .to_path_buf();
    tokio::task::spawn_blocking(move || {
        let before = ilium_platform::secure_fs::directory_generation(&metadata_directory)
            .map_err(|error| io_error("custody directory identity unavailable", error))?;
        let entries = std::fs::read_dir(&metadata_directory)
            .map_err(|error| io_error("cannot list custody directory", error))?;
        for entry in entries {
            let entry = entry.map_err(|error| io_error("cannot inspect custody entry", error))?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(PREFIX) {
                return Err(format!(
                    "worktree process custody is unresolved ({})",
                    name.to_string_lossy()
                ));
            }
        }
        let after = ilium_platform::secure_fs::directory_generation(&metadata_directory)
            .map_err(|error| io_error("custody directory identity changed", error))?;
        if before != after {
            return Err("custody metadata directory changed during inspection".into());
        }
        Ok(())
    })
    .await
    .map_err(|error| io_error("custody inspection worker failed", error))?
}

/// A close hint may disregard only the current pane's still-live ticket.
/// Any earlier crash or another pane's ticket makes the hint unavailable.
pub(crate) async fn require_only_ticket(
    marker: &OwnershipMarker,
    ticket: &CustodyTicket,
) -> Result<(), String> {
    let marker_path =
        workspace_owner::validated_marker_path_for(&marker.repo_common_dir, &marker.worktree_root)
            .await
            .map_err(|error| io_error("worktree custody path changed", error))?;
    if marker_path.parent() != Some(ticket.metadata_directory.as_path())
        || marker.worktree_root != ticket.worktree_root
        || marker.workspace_id != ticket.workspace_id
    {
        return Err("current pane custody ticket does not match worktree ownership".into());
    }
    let ticket = ticket.clone();
    tokio::task::spawn_blocking(move || {
        let directory = NoFollowDirectory::open_root(&ticket.metadata_directory)
            .map_err(|error| io_error("cannot pin custody directory", error))?;
        let before = ilium_platform::secure_fs::directory_generation(&ticket.metadata_directory)
            .map_err(|error| io_error("custody directory identity unavailable", error))?;
        let _opened = read_exact_ticket(&directory, &ticket)?;
        let mut count = 0;
        for entry in std::fs::read_dir(&ticket.metadata_directory)
            .map_err(|error| io_error("cannot list custody directory", error))?
        {
            let entry = entry.map_err(|error| io_error("cannot inspect custody entry", error))?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(PREFIX) {
                count += 1;
                if name != OsStr::new(&ticket.file_name) {
                    return Err("another unresolved worktree custody ticket exists".into());
                }
            }
        }
        let after = ilium_platform::secure_fs::directory_generation(&ticket.metadata_directory)
            .map_err(|error| io_error("custody directory identity changed", error))?;
        if before != after || count != 1 {
            return Err("current pane custody ticket set changed".into());
        }
        Ok(())
    })
    .await
    .map_err(|error| io_error("custody inspection worker failed", error))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneWorkspace;
    use tempfile::TempDir;

    fn git(directory: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git installed");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn owned_worktree() -> (TempDir, OwnershipMarker) {
        let temporary = TempDir::new().unwrap();
        let main = temporary.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.name", "Ilium Test"]);
        git(&main, &["config", "user.email", "ilium@example.invalid"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "initial"]);
        let base_commit = ilium_git::resolve_commit(&main, "main").await.unwrap();
        let linked = temporary.path().join("linked");
        ilium_git::create_worktree(&main, &linked, "agent/test", "main")
            .await
            .unwrap();
        let repository = ilium_git::discover(&linked).await.unwrap();
        let mut workspace = PaneWorkspace {
            workspace_id: None,
            repo_common_dir: repository.common_dir,
            worktree_root: repository.worktree_root,
            branch: "agent/test".into(),
            base_ref: "main".into(),
            base_commit,
            created_by_ilium: true,
            created_at_unix: 1_700_000_000,
        };
        let marker = workspace_owner::create_marker(&mut workspace)
            .await
            .unwrap();
        (temporary, marker)
    }

    #[tokio::test]
    async fn two_spawn_tickets_block_prune_until_each_is_proven_cleared() {
        let (_temporary, marker) = owned_worktree().await;
        let first = begin(&marker).await.unwrap();
        assert!(require_only_ticket(&marker, &first).await.is_ok());
        let second = begin(&marker).await.unwrap();
        assert!(require_only_ticket(&marker, &first).await.is_err());
        assert!(
            require_no_tickets(&marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
        first.clear_after_proof().unwrap();
        assert!(require_only_ticket(&marker, &second).await.is_ok());
        assert!(
            require_no_tickets(&marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
        second.clear_after_proof().unwrap();
        assert!(
            require_no_tickets(&marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn malformed_ticket_name_blocks_prune() {
        let (_temporary, marker) = owned_worktree().await;
        let marker_path = workspace_owner::validated_marker_path_for(
            &marker.repo_common_dir,
            &marker.worktree_root,
        )
        .await
        .unwrap();
        let malformed = marker_path
            .parent()
            .unwrap()
            .join("ilium-workspace-custody-malformed.json");
        std::fs::write(malformed, b"broken").unwrap();
        assert!(
            require_no_tickets(&marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
    }
}
