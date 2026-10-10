//! Durable, per-spawn refusal tokens for worktree deletion.
//! Every token is created and fsynced before a PTY may start. Only a
//! proven-stopped lineage and a successful directory-user scan may erase it.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ilium_execution::{JobCost, Lane};
use ilium_platform::secure_fs::NoFollowDirectory;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::execution::ExecutionClient;
use crate::workspace_owner::{self, OwnershipMarker};

const PREFIX: &str = "ilium-workspace-custody-";
const SUFFIX: &str = ".json";
const MAX_TICKET_BYTES: u64 = 1024;
const MAX_CUSTODY_PATH_BYTES: usize = 64 * 1024;
const MAX_CUSTODY_ENTRIES: usize = 1024;
const MAX_RESULT_ERROR_BYTES: usize = 4096;
const JOB_WORKING_BYTES: usize = 16 * 1024;

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
    let mut message = format!("{context}: {error}");
    if message.len() > MAX_RESULT_ERROR_BYTES {
        let mut boundary = MAX_RESULT_ERROR_BYTES - 3;
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
        message.push_str("...");
    }
    message
}

/// Cost includes two copies of captured path/identity data while the caller
/// still owns its ticket, plus bounded serialization and directory-entry work.
fn io_cost(paths: &[&Path], identity_bytes: usize) -> Result<JobCost, String> {
    let mut captured_bytes = identity_bytes;
    if identity_bytes > MAX_TICKET_BYTES as usize {
        return Err("worktree custody identity is too large".into());
    }
    for path in paths {
        let length = path.as_os_str().as_encoded_bytes().len();
        if length > MAX_CUSTODY_PATH_BYTES {
            return Err("worktree custody path is too long".into());
        }
        captured_bytes = captured_bytes
            .checked_add(length)
            .ok_or("worktree custody input size overflow")?;
    }
    let input_bytes = captured_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(JOB_WORKING_BYTES + 2 * MAX_TICKET_BYTES as usize))
        .ok_or("worktree custody input size overflow")?;
    Ok(JobCost {
        input_bytes,
        result_bytes: MAX_RESULT_ERROR_BYTES + std::mem::size_of::<String>(),
    })
}

async fn reserve_io(
    client: &ExecutionClient,
    cost: JobCost,
) -> Result<ilium_execution::Reservation, String> {
    client.reserve(Lane::Io, cost).await.map_err(|error| {
        io_error(
            "worktree custody I/O was not admitted",
            format!("{error:?}"),
        )
    })
}

fn exact_bytes(workspace_id: &str, ticket_id: &str) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(&TicketContents {
        workspace_id: workspace_id.to_owned(),
        ticket_id: ticket_id.to_owned(),
    })
    .map_err(|error| io_error("cannot encode worktree custody ticket", error))?;
    if bytes.len() > MAX_TICKET_BYTES as usize {
        return Err("worktree custody ticket is too large".into());
    }
    Ok(bytes)
}

fn publish_ticket(
    directory: &NoFollowDirectory,
    file_name: &str,
    bytes: &[u8],
    sync_directory: impl FnOnce(&NoFollowDirectory) -> std::io::Result<()>,
) -> Result<(), String> {
    let mut file = directory
        .create_regular(OsStr::new(file_name))
        .map_err(|error| io_error("cannot reserve worktree custody ticket", error))?;
    // A failed write or sync leaves the exact file as a refusal token.
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| sync_directory(directory))
        .map_err(|error| io_error("cannot durably publish custody ticket", error))
}

fn read_exact_ticket(
    directory: &NoFollowDirectory,
    file_name: &str,
    expected_bytes: &[u8],
) -> Result<std::fs::File, String> {
    let mut file = directory
        .open_regular(OsStr::new(file_name))
        .map_err(|error| io_error("cannot open worktree custody ticket", error))?;
    if file
        .metadata()
        .map_err(|error| io_error("cannot stat custody ticket", error))?
        .len()
        > MAX_TICKET_BYTES
    {
        return Err("worktree custody ticket is too large".into());
    }
    let mut actual = Vec::with_capacity(MAX_TICKET_BYTES as usize + 1);
    (&mut file)
        .take(MAX_TICKET_BYTES + 1)
        .read_to_end(&mut actual)
        .map_err(|error| io_error("cannot read custody ticket", error))?;
    if actual != expected_bytes {
        return Err("worktree custody ticket content changed".into());
    }
    Ok(file)
}

pub(crate) async fn begin(
    client: &ExecutionClient,
    marker: &OwnershipMarker,
) -> Result<CustodyTicket, String> {
    if marker.custody_revision != 1 {
        return Err("worktree ownership predates custody tracking".into());
    }
    let marker_path = workspace_owner::validated_marker_path_for(
        client,
        &marker.repo_common_dir,
        &marker.worktree_root,
    )
    .await
    .map_err(|error| io_error("worktree ownership path changed", error))?;
    let metadata_directory = marker_path
        .view()
        .parent()
        .ok_or("worktree marker has no metadata parent")?;
    let reservation = reserve_io(
        client,
        io_cost(
            &[metadata_directory, &marker.worktree_root],
            marker.workspace_id.len(),
        )?,
    )
    .await?;
    let ticket_id = Uuid::new_v4().to_string();
    let ticket = CustodyTicket {
        metadata_directory: metadata_directory.to_path_buf(),
        worktree_root: marker.worktree_root.clone(),
        file_name: format!("{PREFIX}{ticket_id}{SUFFIX}"),
        workspace_id: marker.workspace_id.clone(),
        ticket_id,
    };
    let bytes = exact_bytes(&ticket.workspace_id, &ticket.ticket_id)?;
    let worker_directory = ticket.metadata_directory.clone();
    let worker_file_name = ticket.file_name.clone();
    let result = client
        .run_reserved(
            reservation,
            move |context: ilium_execution::JobContext| -> Result<(), String> {
                if context.stop_requested() {
                    return Err("custody ticket creation cancelled before file creation".into());
                }
                let before = ilium_platform::secure_fs::directory_generation(&worker_directory)
                    .map_err(|error| io_error("custody directory identity unavailable", error))?;
                let directory = NoFollowDirectory::open_root(&worker_directory)
                    .map_err(|error| io_error("cannot pin worktree metadata directory", error))?;
                if ilium_platform::secure_fs::directory_generation(&worker_directory)
                    .map_err(|error| io_error("custody directory identity changed", error))?
                    != before
                {
                    return Err("custody metadata directory changed before ticket creation".into());
                }
                // After exclusive creation, finish the durability sequence even if
                // the waiter cancels. A failed write or sync leaves a refusal token.
                publish_ticket(
                    &directory,
                    &worker_file_name,
                    &bytes,
                    NoFollowDirectory::sync_all,
                )?;
                if ilium_platform::secure_fs::directory_generation(&worker_directory)
                    .map_err(|error| io_error("custody directory identity changed", error))?
                    != before
                {
                    return Err("custody metadata directory changed after ticket creation".into());
                }
                Ok(())
            },
        )
        .await
        .map_err(|error| io_error("custody-ticket I/O job failed", error))?;
    drop(result);
    Ok(ticket)
}

impl CustodyTicket {
    pub(crate) fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Call only after PTY lineage termination returned proof AND
    /// `processes_using_directory(root)` returned an empty list.
    /// A failed acknowledgement after unlink has an uncertain disk outcome;
    /// callers must not claim that custody was cleared or retry automatically.
    #[cfg(test)]
    pub(crate) async fn clear_after_proof(&self, client: &ExecutionClient) -> Result<(), String> {
        let reservation = reserve_io(
            client,
            io_cost(&[&self.metadata_directory], self.workspace_id.len())?,
        )
        .await?;
        let ticket = self.clone();
        let result = client
            .run_reserved(reservation, move |context| {
                ticket.clear_after_proof_in_worker(&context)
            })
            .await
            .map_err(|error| io_error("custody-clear I/O job failed", error))?;
        drop(result);
        Ok(())
    }

    /// Clears a proven ticket inside a caller's already-admitted I/O job.
    /// This allows process termination, use scanning and durable marker removal
    /// to share one reservation without nesting admission while retaining a
    /// result lease.
    pub(crate) fn clear_after_proof_in_worker(
        &self,
        context: &ilium_execution::JobContext,
    ) -> Result<(), String> {
        if context.stop_requested() {
            return Err("custody clear cancelled before inspection".into());
        }
        let before = ilium_platform::secure_fs::directory_generation(&self.metadata_directory)
            .map_err(|error| io_error("custody directory identity unavailable", error))?;
        let directory = NoFollowDirectory::open_root(&self.metadata_directory)
            .map_err(|error| io_error("cannot pin custody directory", error))?;
        let expected_bytes = exact_bytes(&self.workspace_id, &self.ticket_id)?;
        let file = read_exact_ticket(&directory, &self.file_name, &expected_bytes)?;
        if context.stop_requested() {
            return Err("custody clear cancelled before removal".into());
        }
        if ilium_platform::secure_fs::directory_generation(&self.metadata_directory)
            .map_err(|error| io_error("custody directory identity changed", error))?
            != before
        {
            return Err("custody metadata directory changed before removal".into());
        }
        // Once unlink starts, complete the directory sync despite a cancelled
        // waiter. The result may otherwise be ambiguous.
        directory
            .remove_regular(OsStr::new(&self.file_name), &file)
            .and_then(|()| directory.sync_all())
            .map_err(|error| io_error("cannot clear proven custody ticket", error))?;
        if ilium_platform::secure_fs::directory_generation(&self.metadata_directory)
            .map_err(|error| io_error("custody directory identity changed", error))?
            != before
        {
            return Err("custody metadata directory changed after removal".into());
        }
        Ok(())
    }
}

/// Any matching name blocks deletion, including malformed, unreadable,
/// symlinked, or partial ticket files. Git's metadata directory is verified
/// through the same path routine as the immutable owner marker.
pub(crate) async fn require_no_tickets(
    client: &ExecutionClient,
    repo_common_dir: &Path,
    worktree_root: &Path,
) -> Result<(), String> {
    let marker_path =
        workspace_owner::validated_marker_path_for(client, repo_common_dir, worktree_root)
            .await
            .map_err(|error| io_error("worktree custody path changed", error))?;
    let metadata_directory = marker_path
        .view()
        .parent()
        .ok_or("worktree marker has no metadata parent")?;
    let reservation = reserve_io(client, io_cost(&[metadata_directory], 0)?).await?;
    let metadata_directory = metadata_directory.to_path_buf();
    let result = client
        .run_reserved(
            reservation,
            move |context: ilium_execution::JobContext| -> Result<(), String> {
                let before = ilium_platform::secure_fs::directory_generation(&metadata_directory)
                    .map_err(|error| {
                    io_error("custody directory identity unavailable", error)
                })?;
                let _directory = NoFollowDirectory::open_root(&metadata_directory)
                    .map_err(|error| io_error("cannot pin custody directory", error))?;
                let entries = std::fs::read_dir(&metadata_directory)
                    .map_err(|error| io_error("cannot list custody directory", error))?;
                for (index, entry) in entries.enumerate() {
                    if context.stop_requested() {
                        return Err("custody directory inspection cancelled".into());
                    }
                    if index >= MAX_CUSTODY_ENTRIES {
                        return Err("custody directory entry limit exceeded".into());
                    }
                    let entry =
                        entry.map_err(|error| io_error("cannot inspect custody entry", error))?;
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
            },
        )
        .await
        .map_err(|error| io_error("custody inspection I/O job failed", error))?;
    drop(result);
    Ok(())
}

/// A close hint may disregard only the current pane's still-live ticket.
/// Any earlier crash or another pane's ticket makes the hint unavailable.
pub(crate) async fn require_only_ticket(
    client: &ExecutionClient,
    marker: &OwnershipMarker,
    ticket: &CustodyTicket,
) -> Result<(), String> {
    let marker_path = workspace_owner::validated_marker_path_for(
        client,
        &marker.repo_common_dir,
        &marker.worktree_root,
    )
    .await
    .map_err(|error| io_error("worktree custody path changed", error))?;
    if marker_path.view().parent() != Some(ticket.metadata_directory.as_path())
        || marker.worktree_root != ticket.worktree_root
        || marker.workspace_id != ticket.workspace_id
    {
        return Err("current pane custody ticket does not match worktree ownership".into());
    }
    let reservation = reserve_io(
        client,
        io_cost(&[&ticket.metadata_directory], ticket.workspace_id.len())?,
    )
    .await?;
    let metadata_directory = ticket.metadata_directory.clone();
    let file_name = ticket.file_name.clone();
    let expected_bytes = exact_bytes(&ticket.workspace_id, &ticket.ticket_id)?;
    let result = client
        .run_reserved(
            reservation,
            move |context: ilium_execution::JobContext| -> Result<(), String> {
                let before = ilium_platform::secure_fs::directory_generation(&metadata_directory)
                    .map_err(|error| {
                    io_error("custody directory identity unavailable", error)
                })?;
                let directory = NoFollowDirectory::open_root(&metadata_directory)
                    .map_err(|error| io_error("cannot pin custody directory", error))?;
                let _opened = read_exact_ticket(&directory, &file_name, &expected_bytes)?;
                let mut count = 0;
                for (index, entry) in std::fs::read_dir(&metadata_directory)
                    .map_err(|error| io_error("cannot list custody directory", error))?
                    .enumerate()
                {
                    if context.stop_requested() {
                        return Err("custody directory inspection cancelled".into());
                    }
                    if index >= MAX_CUSTODY_ENTRIES {
                        return Err("custody directory entry limit exceeded".into());
                    }
                    let entry =
                        entry.map_err(|error| io_error("cannot inspect custody entry", error))?;
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with(PREFIX) {
                        count += 1;
                        if name != OsStr::new(&file_name) {
                            return Err("another unresolved worktree custody ticket exists".into());
                        }
                    }
                }
                let after = ilium_platform::secure_fs::directory_generation(&metadata_directory)
                    .map_err(|error| io_error("custody directory identity changed", error))?;
                if before != after || count != 1 {
                    return Err("current pane custody ticket set changed".into());
                }
                Ok(())
            },
        )
        .await
        .map_err(|error| io_error("custody inspection I/O job failed", error))?;
    drop(result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{ExecutionClient, ServerExecution};
    use ilium_core::PaneWorkspace;
    use ilium_execution::{JobCost, Lane};
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

    async fn owned_worktree(client: &ExecutionClient) -> (TempDir, OwnershipMarker) {
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
        let marker = workspace_owner::create_marker(client, &mut workspace)
            .await
            .unwrap();
        (temporary, marker)
    }

    #[tokio::test]
    async fn two_spawn_tickets_block_prune_until_each_is_proven_cleared() {
        let execution = ServerExecution::start().unwrap();
        let client = &execution.client;
        let (_temporary, marker) = owned_worktree(client).await;
        let first = begin(client, &marker).await.unwrap();
        assert!(require_only_ticket(client, &marker, &first).await.is_ok());
        let second = begin(client, &marker).await.unwrap();
        assert!(require_only_ticket(client, &marker, &first).await.is_err());
        assert!(
            require_no_tickets(client, &marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
        first.clear_after_proof(client).await.unwrap();
        assert!(require_only_ticket(client, &marker, &second).await.is_ok());
        assert!(
            require_no_tickets(client, &marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
        second.clear_after_proof(client).await.unwrap();
        assert!(
            require_no_tickets(client, &marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn malformed_ticket_name_blocks_prune() {
        let execution = ServerExecution::start().unwrap();
        let client = &execution.client;
        let (_temporary, marker) = owned_worktree(client).await;
        let marker_path = workspace_owner::validated_marker_path_for(
            client,
            &marker.repo_common_dir,
            &marker.worktree_root,
        )
        .await
        .unwrap();
        let malformed = marker_path
            .view()
            .parent()
            .unwrap()
            .join("ilium-workspace-custody-malformed.json");
        std::fs::write(malformed, b"broken").unwrap();
        assert!(
            require_no_tickets(client, &marker.repo_common_dir, &marker.worktree_root)
                .await
                .is_err()
        );
    }

    #[test]
    fn failed_directory_sync_keeps_created_ticket_as_refusal_evidence() {
        let temporary = TempDir::new().unwrap();
        let directory = NoFollowDirectory::open_root(temporary.path()).unwrap();
        let file_name = "ilium-workspace-custody-test.json";
        let bytes = exact_bytes("workspace", "ticket").unwrap();
        let error = publish_ticket(&directory, file_name, &bytes, |_| {
            Err(std::io::Error::other("forced directory sync failure"))
        })
        .unwrap_err();
        assert!(error.contains("forced directory sync failure"));
        assert_eq!(
            std::fs::read(temporary.path().join(file_name)).unwrap(),
            bytes
        );
    }

    #[tokio::test]
    async fn tampered_ticket_is_never_cleared() {
        let execution = ServerExecution::start().unwrap();
        let client = &execution.client;
        let (_temporary, marker) = owned_worktree(client).await;
        let ticket = begin(client, &marker).await.unwrap();
        let ticket_path = ticket.metadata_directory.join(&ticket.file_name);
        std::fs::write(&ticket_path, b"foreign content").unwrap();
        assert!(ticket.clear_after_proof(client).await.is_err());
        assert_eq!(std::fs::read(&ticket_path).unwrap(), b"foreign content");
    }

    #[tokio::test]
    async fn clear_waits_for_shared_io_capacity_and_keeps_ticket_until_acknowledged() {
        let execution = ServerExecution::start().unwrap();
        let client = execution.client.clone();
        let (_temporary, marker) = owned_worktree(&client).await;
        let ticket = begin(&client, &marker).await.unwrap();
        let ticket_path = ticket.metadata_directory.join(&ticket.file_name);

        let mut blocked_jobs = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..2 {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker_client = client.clone();
            blocked_jobs.push(tokio::spawn(async move {
                blocker_client
                    .run(
                        Lane::Io,
                        JobCost {
                            input_bytes: 1024,
                            result_bytes: 128,
                        },
                        move |_| -> std::io::Result<()> {
                            let _ = started_tx.send(());
                            release_rx.recv().expect("release blocked I/O job");
                            Ok(())
                        },
                    )
                    .await
            }));
            releases.push(release_tx);
            started_rx.await.expect("blocked I/O job started");
        }

        let clearing_client = client.clone();
        let mut clearing =
            tokio::spawn(async move { ticket.clear_after_proof(&clearing_client).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut clearing)
                .await
                .is_err()
        );
        assert!(
            ticket_path.is_file(),
            "ticket remains until bank acknowledgement"
        );
        let cpu = client
            .run(
                Lane::Cpu,
                JobCost {
                    input_bytes: 64,
                    result_bytes: 64,
                },
                |_| Ok::<_, std::io::Error>(7),
            )
            .await
            .expect("blocked custody I/O must leave CPU work responsive");
        assert_eq!(*cpu.view(), 7);
        drop(cpu);
        for release in releases {
            release.send(()).expect("release blocked I/O job");
        }
        for blocked in blocked_jobs {
            drop(
                blocked
                    .await
                    .expect("blocked I/O caller")
                    .expect("I/O result"),
            );
        }
        clearing
            .await
            .expect("clear caller")
            .expect("durable clear");
        assert!(!ticket_path.exists());
    }

    #[tokio::test]
    async fn oversized_directory_scan_fails_closed() {
        let execution = ServerExecution::start().unwrap();
        let client = &execution.client;
        let (_temporary, marker) = owned_worktree(client).await;
        let marker_path = workspace_owner::validated_marker_path_for(
            client,
            &marker.repo_common_dir,
            &marker.worktree_root,
        )
        .await
        .unwrap();
        let metadata_directory = marker_path.view().parent().unwrap();
        for index in 0..=MAX_CUSTODY_ENTRIES {
            std::fs::write(
                metadata_directory.join(format!("scan-entry-{index:05}")),
                b"",
            )
            .unwrap();
        }
        assert!(
            require_no_tickets(client, &marker.repo_common_dir, &marker.worktree_root)
                .await
                .unwrap_err()
                .contains("entry limit")
        );
    }
}
