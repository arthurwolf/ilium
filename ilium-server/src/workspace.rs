//! Server-owned Git workspace validation and lifecycle coordination.
//!
//! Every Git command awaits outside the tree and pane locks. A snapshot's
//! saved worktree path is only provenance until Git confirms it still names
//! the same registered repository checkout.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ilium_core::{
    AgentProvider, BuiltinAgentProvider, NodeId, PaneContentKind, PaneWorkspace, ROOT_ID,
};
use ilium_execution::{JobCost, Lane, Retained};
use ilium_ipc::{
    RepoFacts, ServerEvent, WorkspaceClosePolicy, WorkspaceCreateSpec, WorkspaceCreateStage,
    WorkspaceGitStatus, WorkspaceGitVersion, WorkspaceWorktreeFact,
};
use ilium_platform::{paths, secure_fs};

use crate::execution::ExecutionClient;
use crate::ipc::handlers::{
    broadcast_and_persist, spawn_and_register_pane_in_directory, RegisterPaneError,
};
use crate::pane::{PaneSnapshotKind, TerminalOrigin};
use crate::state::{ServerState, WorkspaceClosePreference};
use crate::workspace_prune as prune;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreTarget {
    Ready,
    Missing(String),
}

/// Validates a saved workspace before any provider resume or automatic input
/// can run there. A missing or replaced worktree must get a harmless fallback
/// shell; its original agent command stays in the snapshot for later recovery.
pub(crate) async fn restore_target(
    client: &ExecutionClient,
    saved_cwd: &Path,
    workspace: &PaneWorkspace,
) -> RestoreTarget {
    let subpath = match saved_cwd.strip_prefix(&workspace.worktree_root) {
        Ok(subpath) => subpath,
        Err(_) => {
            return RestoreTarget::Missing(
                "saved launch directory is outside the recorded worktree".into(),
            );
        }
    };
    if subpath.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return RestoreTarget::Missing(
            "saved launch directory contains an unsafe path component".into(),
        );
    }

    let root = match crate::workspace_owner::canonical_path(client, &workspace.worktree_root).await
    {
        Ok(root) => root,
        Err(error) => {
            return RestoreTarget::Missing(format!("worktree directory is unavailable: {error}"));
        }
    };
    if root.view() != &workspace.worktree_root {
        return RestoreTarget::Missing(
            "worktree directory no longer resolves to its saved path".into(),
        );
    }

    let cwd = match crate::workspace_owner::canonical_path(client, saved_cwd).await {
        Ok(cwd) => cwd,
        Err(error) => {
            return RestoreTarget::Missing(format!("launch directory is unavailable: {error}"));
        }
    };
    if !cwd.view().starts_with(root.view()) {
        return RestoreTarget::Missing("launch directory now resolves outside the worktree".into());
    }
    match crate::workspace_owner::is_directory(client, cwd.view()).await {
        Ok(true) => {}
        Ok(false) => {
            return RestoreTarget::Missing("launch directory is not a directory".into());
        }
        Err(error) => {
            return RestoreTarget::Missing(format!("launch directory is unavailable: {error}"));
        }
    }

    let repository = match ilium_git::discover(root.view()).await {
        Ok(repository) => repository,
        Err(error) => {
            return RestoreTarget::Missing(format!(
                "worktree Git identity cannot be verified: {error}"
            ));
        }
    };
    if repository.is_bare
        || repository.worktree_root != *root.view()
        || repository.common_dir != workspace.repo_common_dir
    {
        return RestoreTarget::Missing(
            "worktree Git identity differs from the saved repository".into(),
        );
    }

    let entries = match ilium_git::list_worktrees(root.view()).await {
        Ok(entries) => entries,
        Err(error) => {
            return RestoreTarget::Missing(format!(
                "Git worktree registration cannot be verified: {error}"
            ));
        }
    };
    if let Err(error) = crate::workspace_owner::validate_registered_worktree_count(entries.len()) {
        return RestoreTarget::Missing(format!(
            "Git worktree registration cannot be verified: {error}"
        ));
    }
    let mut is_listed = false;
    for entry in entries.iter().filter(|entry| !entry.is_bare) {
        if crate::workspace_owner::canonical_path(client, &entry.path)
            .await
            .ok()
            .is_some_and(|candidate| candidate.view() == root.view())
        {
            is_listed = true;
            break;
        }
    }
    if !is_listed {
        return RestoreTarget::Missing("directory is absent from Git's worktree list".into());
    }

    RestoreTarget::Ready
}

pub(crate) async fn project_directory(
    state: &ServerState,
    node_id: NodeId,
) -> Result<PathBuf, String> {
    let tree = state.tree.read().await;
    if node_id == ROOT_ID {
        return Ok(state.session_cwd.clone());
    }
    tree.project_path_for(node_id)
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("node {} is not in a project", node_id.0))
}

/// Derive the fixed sibling checkout location for an API caller. The HTTP
/// transport never accepts a caller-chosen filesystem path.
pub(crate) async fn default_new_worktree_spec(
    project_directory: &Path,
    branch: String,
    base: Option<String>,
) -> Result<WorkspaceCreateSpec, String> {
    ilium_core::validate_branch_name(&branch)
        .map_err(|error| format!("invalid worktree branch: {error}"))?;
    let repository = ilium_git::discover(project_directory)
        .await
        .map_err(|error| format!("source repository: {error}"))?;
    if repository.is_bare {
        return Err("bare repositories cannot host agent panes".into());
    }
    let worktrees = ilium_git::list_worktrees(project_directory)
        .await
        .map_err(|error| format!("cannot list repository worktrees: {error}"))?;
    let main = worktrees
        .first()
        .ok_or_else(|| "repository has no registered main worktree".to_string())?;
    let parent = main
        .path
        .parent()
        .ok_or_else(|| "main worktree has no parent directory".to_string())?;
    let mut directory_name = main
        .path
        .file_name()
        .ok_or_else(|| "main worktree has no directory name".to_string())?
        .to_os_string();
    directory_name.push(".worktrees");
    let path = parent
        .join(directory_name)
        .join(ilium_core::slugify_branch(&branch));
    let base_ref = match base {
        Some(base) if !base.trim().is_empty() => base,
        Some(_) => return Err("workspace base must not be empty".into()),
        None => ilium_git::default_base_ref(project_directory)
            .await
            .map_err(|error| format!("cannot choose base branch: {error}"))?,
    };
    Ok(WorkspaceCreateSpec::New {
        branch,
        base_ref,
        path,
    })
}

/// Collects one bounded dialog snapshot. Every mutation is revalidated later.
pub(crate) async fn repo_facts(state: &ServerState, project: NodeId) -> Result<RepoFacts, String> {
    let execution = state
        .execution
        .get()
        .ok_or_else(|| "workspace execution service is unavailable".to_string())?;
    if !secure_fs::supports_nofollow_directories() {
        return Err("agent worktrees are unavailable: this platform cannot secure workspace ownership paths".into());
    }
    let project_cwd = project_directory(state, project).await?;
    let repository = ilium_git::discover(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    if repository.is_bare {
        return Err("bare repositories cannot host agent panes".into());
    }
    // Keep the Git registration and ownership-marker snapshot together while
    // another request may be creating or removing a worktree.
    let repository_lock = state
        .workspace_repository_lock(&repository.common_dir)
        .await;
    let _guard = repository_lock.lock().await;
    let default_base_ref = ilium_git::default_base_ref(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    let default_base_commit = ilium_git::resolve_commit(&project_cwd, &default_base_ref)
        .await
        .map_err(|error| error.to_string())?;
    let local_branches = ilium_git::list_branches(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    let listed = ilium_git::list_worktrees(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    crate::workspace_owner::validate_registered_worktree_count(listed.len())
        .map_err(|error| error.to_string())?;
    let current_branch = ilium_git::head_probe(&project_cwd)
        .await
        .map_err(|error| error.to_string())?
        .branch;
    let source_status = ilium_git::status(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    let source_dirty_count = source_status
        .staged
        .saturating_add(source_status.modified)
        .saturating_add(source_status.untracked)
        .saturating_add(source_status.conflicted);
    let main_directory = listed
        .first()
        .map(|entry| entry.path.clone())
        .ok_or("Git reported no worktrees")?;
    let main_status = if main_directory == repository.worktree_root {
        source_status
    } else {
        ilium_git::status(&main_directory)
            .await
            .map_err(|error| format!("cannot read main checkout status: {error}"))?
    };
    let main_dirty_count = main_status
        .staged
        .saturating_add(main_status.modified)
        .saturating_add(main_status.untracked)
        .saturating_add(main_status.conflicted);
    let gitmodules_path = repository.worktree_root.join(".gitmodules");
    let (worktree_directories, has_gitmodules) =
        repo_facts_path_metadata(&execution.client, &listed, &gitmodules_path).await?;
    let occupied = {
        let tree = state.tree.read().await;
        tree.panes()
            .filter_map(|pane| {
                tree.pane_cwd(pane.id)
                    .map(|directory| (pane.id, directory.to_path_buf()))
            })
            .collect::<Vec<_>>()
    };
    let mut worktrees = Vec::with_capacity(listed.len());
    for (entry, is_directory) in listed.into_iter().zip(worktree_directories) {
        let existing = occupied
            .iter()
            .find(|(_, directory)| directory.starts_with(&entry.path));
        let is_dirty = if entry.is_prunable || !is_directory {
            true
        } else {
            ilium_git::status(&entry.path)
                .await
                .map(|status| !status.is_clean())
                .unwrap_or(true)
        };
        let created_by_ilium = if entry.path == main_directory || entry.is_prunable || !is_directory
        {
            false
        } else {
            crate::workspace_owner::read_registered_marker(
                &execution.client,
                &repository.common_dir,
                &entry.path,
            )
            .await
            .map_err(|error| {
                format!(
                    "cannot inspect worktree ownership at {}: {error}",
                    entry.path.display()
                )
            })?
            .is_some()
        };
        worktrees.push(WorkspaceWorktreeFact {
            path: entry.path,
            branch: entry.branch,
            created_by_ilium,
            is_dirty,
            occupied_pane_id: existing.map(|(pane_id, _)| *pane_id),
        });
    }
    Ok(RepoFacts {
        repo_common_dir: repository.common_dir,
        checkout_root: repository.worktree_root,
        project_subpath: repository.project_subpath,
        current_branch,
        default_base_ref,
        default_base_commit,
        local_branches,
        worktrees,
        source_dirty_count,
        main_dirty_count,
        has_gitmodules,
        git_version: WorkspaceGitVersion {
            major: repository.git_version.major,
            minor: repository.git_version.minor,
            patch: repository.git_version.patch,
        },
    })
}

async fn progress(
    direct_tx: Option<&crate::ipc::EventReply<'_>>,
    request_id: u64,
    stage: WorkspaceCreateStage,
) {
    if let Some(direct_tx) = direct_tx {
        // Progress is advisory; a stalled client must not strand a checkout
        // between Git mutation and the pane commit or exact rollback.
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            direct_tx.send(ServerEvent::WorkspaceCreateProgress { request_id, stage }),
        )
        .await;
    }
}

const MAX_NEW_WORKTREE_PATH_BYTES: usize = 4096;
const REPO_FACTS_METADATA_BATCH_SIZE: usize = 32;

fn canonical_new_path_blocking(path: &Path) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "new worktree path must be absolute and normalized",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "new worktree path has no parent",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "new worktree path has no final name",
        )
    })?;
    let (canonical_parent, created_parent) = match parent.symlink_metadata() {
        Ok(_) => (
            paths::canonicalize(parent).map_err(|error| {
                std::io::Error::other(format!("worktree parent is unavailable: {error}"))
            })?,
            None,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let grandparent = parent.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "worktree parent has no parent",
                )
            })?;
            let parent_name = parent.file_name().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "worktree parent has no name",
                )
            })?;
            let canonical_grandparent = paths::canonicalize(grandparent).map_err(|error| {
                std::io::Error::other(format!("worktree parent base is unavailable: {error}"))
            })?;
            if canonical_grandparent.join(parent_name) != parent {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "worktree parent base is not canonical",
                ));
            }
            std::fs::create_dir(parent).map_err(|error| {
                std::io::Error::other(format!("cannot create worktree parent: {error}"))
            })?;
            (parent.to_path_buf(), Some(parent.to_path_buf()))
        }
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "cannot inspect worktree parent: {error}"
            )));
        }
    };
    if !canonical_parent.is_dir() || canonical_parent.join(file_name) != path {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "new worktree path has a noncanonical parent",
        ));
    }
    match path.symlink_metadata() {
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("new worktree path already exists: {}", path.display()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "cannot inspect worktree path: {error}"
            )));
        }
    }
    Ok((path.to_path_buf(), created_parent))
}

async fn run_worktree_path_io<T: Send + 'static>(
    client: &ExecutionClient,
    paths: Vec<PathBuf>,
    result_bytes: usize,
    operation: impl FnOnce(Vec<PathBuf>, ilium_execution::JobContext) -> std::io::Result<T>
        + Send
        + 'static,
) -> Result<Retained<T>, String> {
    if paths.is_empty() {
        return Err("worktree path I/O requires at least one path".into());
    }
    let path_bytes = paths.iter().try_fold(0usize, |total, path| {
        let path_bytes = path.as_os_str().as_encoded_bytes().len();
        if path_bytes > MAX_NEW_WORKTREE_PATH_BYTES {
            return None;
        }
        total.checked_add(path_bytes.max(1))
    });
    let Some(path_bytes) = path_bytes else {
        return Err("worktree path input exceeds the 4096-byte bounded I/O limit".into());
    };
    let declared_bytes = path_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(4096usize.saturating_mul(paths.len().max(1))))
        .ok_or("worktree path I/O admission size overflow")?;
    let reservation = client
        .reserve(
            Lane::Io,
            JobCost {
                input_bytes: declared_bytes.max(1),
                result_bytes: result_bytes.max(1),
            },
        )
        .await
        .map_err(|reason| format!("worktree path I/O admission failed: {reason:?}"))?;
    client
        .run_reserved(reservation, move |context| operation(paths, context))
        .await
        .map_err(|error| match error {
            crate::execution::ExecutionError::Failed(error) => error.view().to_string(),
            other => format!("worktree path I/O failed: {other:?}"),
        })
}

async fn canonical_new_path(
    client: &ExecutionClient,
    path: PathBuf,
) -> Result<Retained<(PathBuf, Option<PathBuf>)>, String> {
    let path_bytes = path.as_os_str().as_encoded_bytes().len();
    let result_bytes = path_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(4096))
        .ok_or("new worktree path result size overflow")?;
    run_worktree_path_io(client, vec![path], result_bytes, |mut paths, _| {
        canonical_new_path_blocking(&paths.remove(0))
    })
    .await
}

async fn worktree_paths_absent(
    client: &ExecutionClient,
    paths: Vec<PathBuf>,
) -> Result<Retained<Vec<bool>>, String> {
    let result_bytes = paths.len().max(1);
    run_worktree_path_io(client, paths, result_bytes, |paths, _| {
        paths
            .iter()
            .map(|path| match path.symlink_metadata() {
                Ok(_) => Ok(false),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
                Err(error) => Err(error),
            })
            .collect()
    })
    .await
}

async fn remove_created_parent(client: &ExecutionClient, parent: PathBuf) -> Result<(), String> {
    let removed = run_worktree_path_io(client, vec![parent], 1, |mut paths, _| {
        std::fs::remove_dir(paths.remove(0))
    })
    .await?;
    // The retained charge covers only the removal; the unit result carries nothing.
    drop(removed);
    Ok(())
}

/// Probe checkout directories and `.gitmodules` in fixed-size batches. Paths
/// remain borrowed until admission succeeds, so a large Git worktree list
/// cannot create an uncharged duplicate path vector or one job per checkout.
async fn repo_facts_path_metadata(
    client: &ExecutionClient,
    worktrees: &[ilium_git::Worktree],
    gitmodules: &Path,
) -> Result<(Vec<bool>, bool), String> {
    let mut directories = Vec::new();
    directories
        .try_reserve_exact(worktrees.len())
        .map_err(|error| format!("cannot allocate worktree metadata results: {error}"))?;
    let mut has_gitmodules = false;

    for (batch_index, batch) in worktrees.chunks(REPO_FACTS_METADATA_BATCH_SIZE).enumerate() {
        let includes_gitmodules = batch_index == 0;
        let path_count = batch.len() + usize::from(includes_gitmodules);
        let encoded_path_bytes = batch
            .iter()
            .map(|worktree| worktree.path.as_os_str().as_encoded_bytes().len())
            .chain(includes_gitmodules.then_some(gitmodules.as_os_str().as_encoded_bytes().len()))
            .try_fold(0usize, |total, bytes| {
                (bytes <= MAX_NEW_WORKTREE_PATH_BYTES)
                    .then(|| total.checked_add(bytes.max(1)))
                    .flatten()
            })
            .ok_or("repository metadata path batch exceeds the 4096-byte per-path limit")?;
        let input_bytes = encoded_path_bytes
            .checked_mul(4)
            .and_then(|bytes| {
                4096usize
                    .checked_mul(path_count)
                    .and_then(|overhead| bytes.checked_add(overhead))
            })
            .ok_or("repository metadata admission size overflow")?;
        let result_bytes = path_count
            .checked_mul(std::mem::size_of::<(bool, bool)>())
            .ok_or("repository metadata result size overflow")?;
        let reservation = client
            .reserve(
                Lane::Io,
                JobCost {
                    input_bytes: input_bytes.max(1),
                    result_bytes: result_bytes.max(1),
                },
            )
            .await
            .map_err(|reason| format!("repository metadata admission failed: {reason:?}"))?;

        // Clone only this admitted, fixed-size batch.
        let mut paths = batch
            .iter()
            .map(|worktree| worktree.path.clone())
            .collect::<Vec<_>>();
        if includes_gitmodules {
            paths.push(gitmodules.to_path_buf());
        }
        let worktree_count = batch.len();
        let retained = client
            .run_reserved(reservation, move |context: ilium_execution::JobContext| {
                let mut results = Vec::with_capacity(paths.len());
                for path in &paths {
                    if context.stop_requested() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "repository metadata scan cancelled",
                        ));
                    }
                    results.push(
                        std::fs::metadata(path)
                            .map(|metadata| (metadata.is_dir(), metadata.is_file()))
                            .unwrap_or((false, false)),
                    );
                }
                Ok(results)
            })
            .await
            .map_err(|error| match error {
                crate::execution::ExecutionError::Failed(error) => {
                    format!("repository metadata I/O failed: {}", error.view())
                }
                other => format!("repository metadata I/O failed: {other:?}"),
            })?;
        let values = retained.view();
        if values.len() != path_count {
            return Err("repository metadata worker returned an incomplete batch".into());
        }
        directories.extend(
            values
                .iter()
                .take(worktree_count)
                .map(|(is_dir, _)| *is_dir),
        );
        if includes_gitmodules {
            has_gitmodules = values[worktree_count].1;
        }
    }
    if worktrees.is_empty() {
        let retained = run_worktree_path_io(
            client,
            vec![gitmodules.to_path_buf()],
            2,
            |paths, context| {
                if context.stop_requested() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "repository metadata scan cancelled",
                    ));
                }
                let is_file = std::fs::metadata(&paths[0]).is_ok_and(|metadata| metadata.is_file());
                Ok(vec![(false, is_file)])
            },
        )
        .await?;
        has_gitmodules = retained.view()[0].1;
    }
    Ok((directories, has_gitmodules))
}

/// Roll back only when include preparation wrote no path and Git finds no
/// other changes. File identity does not prove copied contents are unchanged.
enum CreatedIncludes {
    None,
    Complete(Retained<crate::worktree_include::IncludeCopyReport>),
    Partial(Retained<crate::worktree_include::IncludeCopyError>),
}

async fn rollback_pre_pane_workspace(
    state: &ServerState,
    control_directory: &Path,
    workspace: &PaneWorkspace,
    created_parent: Option<&Path>,
    created_includes: CreatedIncludes,
) -> String {
    let path = workspace.worktree_root.clone();
    let copied_paths_exist = match &created_includes {
        CreatedIncludes::None => false,
        CreatedIncludes::Complete(report) => !report.view().created_paths.is_empty(),
        CreatedIncludes::Partial(error) => !error.view().created_paths.is_empty(),
    };
    if copied_paths_exist {
        return format!(
            "worktree retained at {}: include preparation wrote files whose contents may have changed; no copied files were removed",
            path.display()
        );
    }
    let Some(execution) = state.execution.get() else {
        return format!(
            "worktree retained at {}: execution service unavailable for include rollback",
            path.display()
        );
    };
    let cleanup_path = path.clone();
    match execution
        .client
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 1,
                result_bytes: 1,
            },
            move |_| match created_includes {
                CreatedIncludes::None => Ok(()),
                CreatedIncludes::Complete(report) => {
                    crate::worktree_include::remove_created_report(&cleanup_path, report.view())
                }
                CreatedIncludes::Partial(error) => {
                    crate::worktree_include::remove_created_files(&cleanup_path, error.view())
                }
            },
        )
        .await
    {
        Ok(_result) => {}
        Err(problem) => {
            return format!(
                "worktree retained at {}: copied-file rollback job failed: {problem}",
                path.display()
            );
        }
    }
    match ilium_git::is_worktree_pristine_including_ignored(&path).await {
        Ok(true) => {}
        Ok(false) => {
            return format!(
                "worktree retained at {}: it has tracked, untracked, or ignored changes after copy rollback",
                path.display()
            );
        }
        Err(problem) => {
            return format!(
                "worktree retained at {}: Git status cannot be checked: {problem}",
                path.display()
            );
        }
    }
    if ilium_git::branch_tip(control_directory, &workspace.branch)
        .await
        .ok()
        .flatten()
        .as_deref()
        != Some(workspace.base_commit.as_str())
    {
        return format!(
            "worktree retained at {}: branch tip changed or cannot be checked",
            path.display()
        );
    }
    let Some(execution) = state.execution.get() else {
        return format!(
            "worktree retained at {}: execution service unavailable for ownership verification",
            path.display()
        );
    };
    if let Err(problem) = crate::workspace_owner::verify_marker(&execution.client, workspace).await
    {
        return format!(
            "worktree retained at {}: ownership cannot be verified: {problem}",
            path.display()
        );
    }
    let metadata_directory = match ilium_git::head_paths(&path).await {
        Ok(paths) => match paths.head.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                return format!(
                    "worktree retained at {}: Git metadata path has no parent",
                    path.display()
                );
            }
        },
        Err(problem) => {
            return format!(
                "worktree retained at {}: Git metadata cannot be located: {problem}",
                path.display()
            );
        }
    };
    // Setup hooks and filters may leave descendants behind even when Git and
    // the checkout are pristine. Keep the checkout unless the OS can prove
    // that no process still uses it.
    match crate::workspace_prune::directory_users(&execution.client, &path).await {
        Ok(users) if users.view().is_empty() => {}
        Ok(users) => {
            return format!(
                "worktree retained at {}: processes still use its directory: {:?}",
                path.display(),
                users.view()
            );
        }
        Err(problem) => {
            return format!(
                "worktree retained at {}: process custody cannot be proved: {problem}",
                path.display()
            );
        }
    }
    // Git removes its per-worktree metadata with the checkout. Leaving the
    // marker in place avoids an ownerless checkout if removal fails.
    let command_result = ilium_git::remove_worktree(control_directory, &path, false).await;
    let absence = worktree_paths_absent(
        &execution.client,
        vec![path.clone(), metadata_directory.clone()],
    )
    .await;
    let (path_absent, metadata_absent) = match &absence {
        Ok(absence) => (absence.view()[0], absence.view()[1]),
        Err(_) => (false, false),
    };
    let registration_absent = ilium_git::list_worktrees(control_directory)
        .await
        .ok()
        .is_some_and(|entries| !entries.iter().any(|entry| entry.path == path));
    if !path_absent || !metadata_absent || !registration_absent || command_result.is_err() {
        return format!(
            "worktree removal outcome uncertain at {}: Git result: {command_result:?}; path absent: {path_absent}; metadata absent: {metadata_absent}; registration absent: {registration_absent}; branch retained",
            path.display()
        );
    }
    if let Some(parent) = created_parent {
        let parent_cleanup = remove_created_parent(&execution.client, parent.to_path_buf()).await;
        if let Err(error) = parent_cleanup {
            return format!("new worktree removed; empty parent directory retained: {error}");
        }
    }
    match ilium_git::delete_branch_if_tip(
        control_directory,
        &workspace.branch,
        &workspace.base_commit,
    )
    .await
    {
        Ok(()) => "new worktree and branch rolled back".into(),
        Err(problem) => format!(
            "new worktree rolled back; branch {} retained: {problem}",
            workspace.branch
        ),
    }
}

async fn rollback_uncommitted_creation(
    state: &ServerState,
    control_directory: &Path,
    workspace: &PaneWorkspace,
    created_parent: Option<&Path>,
    included_files: Option<Retained<crate::worktree_include::IncludeCopyReport>>,
) -> String {
    let Some(report) = included_files else {
        return "existing worktree left unchanged".into();
    };
    let repo_lock = state
        .workspace_repository_lock(&workspace.repo_common_dir)
        .await;
    let _guard = repo_lock.lock().await;
    let Some(execution) = state.execution.get() else {
        return "worktree retained: execution service unavailable for rollback admission".into();
    };
    let _repository_lease =
        match prune::repository_lease(&execution.client, &workspace.repo_common_dir).await {
            Ok(lease) => lease,
            Err(error) => {
                return format!("worktree retained: rollback repository admission failed: {error}");
            }
        };
    rollback_pre_pane_workspace(
        state,
        control_directory,
        workspace,
        created_parent,
        CreatedIncludes::Complete(report),
    )
    .await
}

/// Creates the Git checkout before inserting a tree node. Failure after Git
/// has entered its mutation phase retains the checkout for explicit recovery;
/// a hook or filter may have written ignored data invisible to `git status`.
pub(crate) struct CreateAgentOptions {
    pub request_id: u64,
    pub parent_group: NodeId,
    pub project_override: Option<PathBuf>,
    pub provider: BuiltinAgentProvider,
    pub spec: WorkspaceCreateSpec,
    pub initial_input: Option<String>,
    pub wait_for_prompt: bool,
}

pub(crate) async fn create_agent_in_workspace(
    state: &Arc<ServerState>,
    options: CreateAgentOptions,
    direct_tx: Option<&crate::ipc::EventReply<'_>>,
) -> Result<NodeId, String> {
    if !secure_fs::supports_nofollow_directories() {
        return Err("agent worktrees are unavailable: this platform cannot secure workspace ownership paths".into());
    }
    let CreateAgentOptions {
        request_id,
        parent_group,
        project_override,
        provider,
        spec,
        initial_input,
        wait_for_prompt,
    } = options;
    let project_cwd = match &project_override {
        Some(path) => path.clone(),
        None => project_directory(state, parent_group).await?,
    };
    let spec = match spec {
        WorkspaceCreateSpec::NewAtDefaultPath { branch, base_ref } => {
            default_new_worktree_spec(&project_cwd, branch, base_ref).await?
        }
        WorkspaceCreateSpec::NewAtDefaultPathWithSetup {
            branch,
            base_ref,
            setup_command,
        } => {
            let resolved = default_new_worktree_spec(&project_cwd, branch, base_ref).await?;
            let WorkspaceCreateSpec::New {
                branch,
                base_ref,
                path,
            } = resolved
            else {
                return Err("default workspace path did not resolve to a new worktree".into());
            };
            WorkspaceCreateSpec::NewWithSetup {
                branch,
                base_ref,
                path,
                setup_command,
            }
        }
        explicit => explicit,
    };
    let (spec, setup_command, close_policy) = match spec {
        WorkspaceCreateSpec::NewWithOptions {
            branch,
            base_ref,
            path,
            setup_command,
            close_policy,
        } => (
            WorkspaceCreateSpec::New {
                branch,
                base_ref,
                path,
            },
            setup_command,
            close_policy,
        ),
        WorkspaceCreateSpec::ExistingWithOptions { path, close_policy } => (
            WorkspaceCreateSpec::Existing { path },
            String::new(),
            close_policy,
        ),
        WorkspaceCreateSpec::NewWithSetup {
            branch,
            base_ref,
            path,
            setup_command,
        } => (
            WorkspaceCreateSpec::New {
                branch,
                base_ref,
                path,
            },
            setup_command,
            WorkspaceClosePolicy::Keep,
        ),
        spec => (spec, String::new(), WorkspaceClosePolicy::Keep),
    };
    let setup_command = crate::workspace_setup::validate(Some(&setup_command))?;
    let setup_started = setup_command.is_some();
    let source = ilium_git::discover(&project_cwd)
        .await
        .map_err(|error| format!("source repository: {error}"))?;
    if source.is_bare {
        return Err("a bare repository cannot host agent panes".into());
    }
    let repo_lock = state.workspace_repository_lock(&source.common_dir).await;
    // Keep this reservation through the tree commit. Releasing it after Git
    // preparation would let another client attach before the pane appears.
    let mut repo_guard = Some(repo_lock.lock().await);
    let execution = state
        .execution
        .get()
        .ok_or_else(|| "execution service unavailable for repository admission".to_string())?;
    let mut repository_lease =
        Some(prune::repository_lease(&execution.client, &source.common_dir).await?);

    // Keep worker-result storage charged while the accepted worktree path is
    // used by Git, include preparation, rollback and pane publication.
    let mut _path_preparation_retention = None;
    let (workspace, launch_cwd, created_parent, included_files) = match spec {
        WorkspaceCreateSpec::New {
            branch,
            base_ref,
            path,
        } => {
            if ilium_git::branch_tip(&project_cwd, &branch)
                .await
                .map_err(|error| format!("branch validation failed: {error}"))?
                .is_some()
            {
                return Err(format!("branch {branch:?} already exists"));
            }
            let (base_ref, base_commit) =
                ilium_git::qualified_creation_base(&project_cwd, &base_ref)
                    .await
                    .map_err(|error| format!("base reference is unavailable: {error}"))?;
            let listed = ilium_git::list_worktrees(&project_cwd)
                .await
                .map_err(|error| format!("cannot validate existing checkouts: {error}"))?;
            if path.starts_with(&source.common_dir)
                || listed.iter().any(|entry| path.starts_with(&entry.path))
            {
                return Err("new worktree must be outside every existing checkout and Git metadata directory".into());
            }
            let prepared_path = canonical_new_path(&execution.client, path).await?;
            let (path, created_parent) = prepared_path.view().clone();
            _path_preparation_retention = Some(prepared_path);
            progress(
                direct_tx,
                request_id,
                WorkspaceCreateStage::CreatingWorktree,
            )
            .await;
            if !state.accepts_workspace_creation()
                || direct_tx.is_some_and(|reply| reply.is_closed())
            {
                if let Some(parent) = created_parent {
                    let _ = remove_created_parent(&execution.client, parent).await;
                }
                return Err("workspace creation cancelled before Git mutation".into());
            }
            if let Err(error) =
                ilium_git::create_worktree(&project_cwd, &path, &branch, &base_commit).await
            {
                let branch_state = match ilium_git::branch_tip(&project_cwd, &branch).await {
                    Ok(Some(tip)) => format!("present at {tip}"),
                    Ok(None) => "absent".into(),
                    Err(probe) => format!("unknown ({probe})"),
                };
                let path_state =
                    match worktree_paths_absent(&execution.client, vec![path.clone()]).await {
                        Ok(absence) if absence.view()[0] => "absent".to_string(),
                        Ok(_) => "present".to_string(),
                        Err(probe) => format!("unknown ({probe})"),
                    };
                let registration = match ilium_git::list_worktrees(&project_cwd).await {
                    Ok(entries) if entries.iter().any(|entry| entry.path == path) => {
                        "present".into()
                    }
                    Ok(_) => "absent".into(),
                    Err(probe) => format!("unknown ({probe})"),
                };
                if path_state == "absent" && registration == "absent" {
                    if let Some(parent) = created_parent {
                        // Remove only an empty exact directory made here.
                        let _ = remove_created_parent(&execution.client, parent).await;
                    }
                }
                return Err(format!(
                    "git worktree add failed for {} ({branch}): {error}; branch {branch_state}, path {path_state}, registration {registration}; inspect these exact effects before retrying",
                    path.display(),
                ));
            }
            progress(direct_tx, request_id, WorkspaceCreateStage::Preparing).await;
            let launch_cwd = path.join(&source.project_subpath);
            let mut workspace = PaneWorkspace {
                workspace_id: None,
                repo_common_dir: source.common_dir.clone(),
                worktree_root: path.clone(),
                branch,
                base_ref,
                base_commit,
                created_by_ilium: true,
                created_at_unix: chrono::Utc::now().timestamp(),
            };
            let execution = state.execution.get().ok_or_else(|| {
                "execution service unavailable before marker creation".to_string()
            })?;
            crate::workspace_owner::create_marker(&execution.client, &mut workspace)
                .await
                .map_err(|error| {
                    format!(
                        "worktree retained at {}: could not record ownership: {error}",
                        path.display()
                    )
                })?;
            if !crate::workspace_owner::is_directory(&execution.client, &launch_cwd)
                .await
                .map_err(|error| format!("cannot inspect project subdirectory: {error}"))?
            {
                let rollback = rollback_pre_pane_workspace(
                    state,
                    &project_cwd,
                    &workspace,
                    created_parent.as_deref(),
                    CreatedIncludes::None,
                )
                .await;
                return Err(format!(
                    "project subdirectory {} is absent at the selected base; {rollback}",
                    source.project_subpath.display()
                ));
            }
            if !state.accepts_workspace_creation()
                || direct_tx.is_some_and(|reply| reply.is_closed())
            {
                let rollback = rollback_pre_pane_workspace(
                    state,
                    &project_cwd,
                    &workspace,
                    created_parent.as_deref(),
                    CreatedIncludes::None,
                )
                .await;
                return Err(format!(
                    "workspace creation cancelled before pane commit; {rollback}"
                ));
            }
            let Some(execution) = state.execution.get() else {
                return Err(format!(
                    "worktree retained at {}: execution service unavailable for include preparation",
                    path.display()
                ));
            };
            let source_project = project_cwd.clone();
            let target_project = launch_cwd.clone();
            let include_result = execution
                .client
                .run(
                    Lane::Io,
                    JobCost {
                        input_bytes: 16 * 1024 * 1024,
                        result_bytes: 16 * 1024 * 1024,
                    },
                    move |context: ilium_execution::JobContext| {
                        let result = crate::worktree_include::copy_worktree_includes_with_stop(
                            &source_project,
                            &target_project,
                            || context.stop_requested(),
                        );
                        if !context.stop_requested() {
                            return result;
                        }
                        match result {
                            Ok(report) => match crate::worktree_include::remove_created_report(
                                &target_project,
                                &report,
                            ) {
                                Ok(()) => Err(crate::worktree_include::cancelled_error()),
                                Err(error) => Err(crate::worktree_include::report_as_error(
                                    format!("cancelled copy rollback failed: {error}"),
                                    report,
                                )),
                            },
                            Err(error) => match crate::worktree_include::remove_created_files(
                                &target_project,
                                &error,
                            ) {
                                Ok(()) => Err(crate::worktree_include::cancelled_error()),
                                Err(_) => Err(error),
                            },
                        }
                    },
                )
                .await;
            let include_report = match include_result {
                Ok(report) => report,
                Err(crate::execution::ExecutionError::Failed(error)) => {
                    let reason = error.view().to_string();
                    let rollback = rollback_pre_pane_workspace(
                        state,
                        &project_cwd,
                        &workspace,
                        created_parent.as_deref(),
                        CreatedIncludes::Partial(error),
                    )
                    .await;
                    return Err(format!("include preparation failed: {reason}; {rollback}"));
                }
                Err(crate::execution::ExecutionError::Rejected(reason)) => {
                    let rollback = rollback_pre_pane_workspace(
                        state,
                        &project_cwd,
                        &workspace,
                        created_parent.as_deref(),
                        CreatedIncludes::None,
                    )
                    .await;
                    return Err(format!(
                        "include preparation was not admitted ({reason:?}); {rollback}"
                    ));
                }
                Err(
                    reason @ (crate::execution::ExecutionError::Panicked
                    | crate::execution::ExecutionError::Lost),
                ) => {
                    return Err(format!(
                        "include preparation outcome is uncertain ({reason}); worktree retained at {}",
                        path.display()
                    ));
                }
                Err(crate::execution::ExecutionError::Cancelled) => {
                    let rollback = rollback_pre_pane_workspace(
                        state,
                        &project_cwd,
                        &workspace,
                        created_parent.as_deref(),
                        CreatedIncludes::None,
                    )
                    .await;
                    return Err(format!("include preparation cancelled; {rollback}"));
                }
            };
            if let Some(command) = setup_command.as_deref() {
                progress(direct_tx, request_id, WorkspaceCreateStage::RunningSetup).await;
                crate::workspace_setup::run(
                    &execution.client,
                    command,
                    &launch_cwd,
                    &workspace.worktree_root,
                    || {
                        !state.accepts_workspace_creation()
                            || direct_tx.is_some_and(|reply| reply.is_closed())
                    },
                )
                .await
                .map_err(|error| {
                    format!(
                        "setup failed: {error}; worktree retained at {} for inspection",
                        path.display()
                    )
                })?;
                crate::workspace_owner::verify_marker(&execution.client, &workspace)
                    .await
                    .map_err(|error| {
                        format!(
                            "setup changed worktree ownership: {error}; worktree retained at {}",
                            path.display()
                        )
                    })?;
                let head = ilium_git::head_probe(&workspace.worktree_root)
                    .await
                    .map_err(|error| {
                        format!(
                            "setup changed Git identity or it cannot be verified: {error}; worktree retained at {}",
                            path.display()
                        )
                    })?;
                if head.branch.as_deref() != Some(workspace.branch.as_str()) {
                    return Err(format!(
                        "setup changed the selected branch; worktree retained at {}",
                        path.display()
                    ));
                }
            }
            (workspace, launch_cwd, created_parent, Some(include_report))
        }
        WorkspaceCreateSpec::Existing { path } => {
            let path = paths::canonicalize(&path)
                .map_err(|error| format!("existing worktree is unavailable: {error}"))?;
            let selected = ilium_git::discover(&path)
                .await
                .map_err(|error| format!("existing worktree is not a Git checkout: {error}"))?;
            if selected.is_bare
                || selected.common_dir != source.common_dir
                || selected.worktree_root != path
            {
                return Err("existing path is not a worktree root of this repository".into());
            }
            let listed = ilium_git::list_worktrees(&project_cwd)
                .await
                .map_err(|error| format!("cannot validate Git worktree list: {error}"))?;
            if listed.first().is_some_and(|main| main.path == path) {
                return Err("select a linked worktree, not the shared main checkout".into());
            }
            if !listed
                .iter()
                .any(|entry| !entry.is_prunable && entry.path == path)
            {
                return Err("existing checkout is absent from Git's worktree list".into());
            }
            {
                let tree = state.tree.read().await;
                if tree.panes().any(|pane| {
                    tree.pane_cwd(pane.id)
                        .is_some_and(|cwd| cwd.starts_with(&path))
                }) {
                    return Err("another Ilium pane already uses this worktree".into());
                }
            }
            let launch_cwd = path.join(&source.project_subpath);
            if !crate::workspace_owner::is_directory(&execution.client, &launch_cwd)
                .await
                .map_err(|error| format!("cannot inspect project subdirectory: {error}"))?
            {
                return Err("project subdirectory is absent in the selected worktree".into());
            }
            let execution = state.execution.get().ok_or_else(|| {
                "execution service unavailable for ownership inspection".to_string()
            })?;
            let marker = crate::workspace_owner::read_registered_marker(
                &execution.client,
                &source.common_dir,
                &path,
            )
            .await
            .map_err(|error| format!("cannot inspect existing worktree ownership: {error}"))?;
            let workspace = if let Some(marker) = marker {
                PaneWorkspace {
                    workspace_id: Some(marker.workspace_id),
                    repo_common_dir: marker.repo_common_dir,
                    worktree_root: marker.worktree_root,
                    branch: marker.branch,
                    base_ref: marker.base_ref,
                    base_commit: marker.base_commit,
                    created_by_ilium: true,
                    created_at_unix: marker.created_at_unix,
                }
            } else {
                let facts = ilium_git::branch_facts(&path)
                    .await
                    .map_err(|error| format!("cannot read existing branch: {error}"))?;
                let branch = facts.head.branch.unwrap_or_else(|| "(detached)".into());
                PaneWorkspace {
                    workspace_id: None,
                    repo_common_dir: source.common_dir.clone(),
                    worktree_root: path.clone(),
                    branch: branch.clone(),
                    base_ref: branch,
                    base_commit: facts.commit_id,
                    created_by_ilium: false,
                    created_at_unix: chrono::Utc::now().timestamp(),
                }
            };
            (workspace, launch_cwd, None, None)
        }
        WorkspaceCreateSpec::NewAtDefaultPath { .. } => {
            return Err("default workspace path was not resolved".into());
        }
        WorkspaceCreateSpec::NewWithSetup { .. } => {
            return Err("setup workspace request was not normalized".into());
        }
        WorkspaceCreateSpec::NewAtDefaultPathWithSetup { .. } => {
            return Err("configured default workspace path was not normalized".into());
        }
        WorkspaceCreateSpec::NewWithOptions { .. }
        | WorkspaceCreateSpec::ExistingWithOptions { .. } => {
            return Err("workspace options were not normalized".into());
        }
    };
    let execution = state
        .execution
        .get()
        .ok_or_else(|| "execution service unavailable for workspace validation".to_string())?;
    if let RestoreTarget::Missing(error) =
        restore_target(&execution.client, &launch_cwd, &workspace).await
    {
        return Err(format!(
            "worktree retained at {}: launch target cannot be verified: {error}",
            workspace.worktree_root.display()
        ));
    }
    progress(direct_tx, request_id, WorkspaceCreateStage::Starting).await;
    if !state.accepts_workspace_creation() || direct_tx.is_some_and(|reply| reply.is_closed()) {
        drop(repository_lease.take());
        drop(repo_guard.take());
        let rollback = if setup_started {
            format!(
                "worktree retained at {} after setup",
                workspace.worktree_root.display()
            )
        } else {
            rollback_uncommitted_creation(
                state,
                &project_cwd,
                &workspace,
                created_parent.as_deref(),
                included_files,
            )
            .await
        };
        return Err(format!(
            "workspace creation cancelled before pane commit; {rollback}"
        ));
    }
    let command_line = provider.command_line().to_string();
    let publish_guard = state.workspace_spawn_lock.lock().await;
    let pane_result: Result<NodeId, String> = async {
        let mut tree = state.tree.write().await;
        if !state.accepts_workspace_creation() || direct_tx.is_some_and(|reply| reply.is_closed()) {
            return Err("workspace creation cancelled before pane commit".into());
        }
        let parent_group = if parent_group == ROOT_ID || project_override.is_some() {
            let project_id = if let Some(path) = &project_override {
                tree.project_ids()
                    .into_iter()
                    .find(|project_id| {
                        tree.get(*project_id)
                            .and_then(ilium_core::Node::project_path)
                            .is_some_and(|existing| existing == path)
                    })
                    .map(Ok)
                    .unwrap_or_else(|| tree.add_project(path.clone()))
                    .map_err(|error| format!("cannot add project: {error}"))?
            } else {
                tree.ensure_launch_project(state.session_cwd.clone())
                    .map_err(|error| format!("cannot find launch project: {error}"))?
            };
            tree.ensure_project_default_group(project_id, "default")
                .map_err(|error| format!("cannot prepare project group: {error}"))?
        } else {
            parent_group
        };
        let pane_id = tree
            .add_pane(
                parent_group,
                command_line.clone(),
                PaneContentKind::Terminal,
            )
            .map_err(|error| format!("cannot add pane: {error}"))?;
        if let Err(error) = tree.set_pane_launch_cwd(pane_id, launch_cwd.clone()) {
            let _ = tree.remove_node(pane_id);
            return Err(format!("cannot record pane directory: {error}"));
        }
        if let Err(error) = tree.set_pane_workspace(pane_id, Some(workspace.clone())) {
            let _ = tree.remove_node(pane_id);
            return Err(format!("cannot record pane workspace: {error}"));
        }
        state.workspace_close_preferences.write().await.insert(
            pane_id,
            WorkspaceClosePreference {
                pane_id,
                workspace_id: workspace.workspace_id.clone(),
                worktree_root: workspace.worktree_root.clone(),
                policy: close_policy,
            },
        );
        Ok(pane_id)
    }
    .await;
    drop(publish_guard);
    let pane_id = match pane_result {
        Ok(pane_id) => pane_id,
        Err(reason) => {
            drop(repository_lease.take()); // Release cross-process admission before re-entering rollback or spawn.
            drop(repo_guard.take()); // Release the process-local repository reservation.
            let rollback = if setup_started {
                format!(
                    "worktree retained at {} after setup",
                    workspace.worktree_root.display()
                )
            } else {
                rollback_uncommitted_creation(
                    state,
                    &project_cwd,
                    &workspace,
                    created_parent.as_deref(),
                    included_files,
                )
                .await
            };
            return Err(format!("{reason}; {rollback}"));
        }
    };
    drop(repository_lease.take()); // Release cross-process admission before re-entering rollback or spawn.
    drop(repo_guard.take()); // Release the process-local repository reservation.
    let origin = TerminalOrigin::Command(command_line);
    match spawn_and_register_pane_in_directory(
        state,
        pane_id,
        PaneSnapshotKind::Terminal(origin),
        &launch_cwd,
    )
    .await
    {
        Ok(()) => {}
        Err(RegisterPaneError::NodeRemoved(_)) => {
            state
                .workspace_close_preferences
                .write()
                .await
                .remove(&pane_id);
            return Err("worktree retained: pane was removed before startup".into());
        }
        Err(RegisterPaneError::Spawn(error)) => {
            let mut tree = state.tree.write().await;
            let _ = tree.remove_node(pane_id);
            drop(tree);
            state
                .workspace_close_preferences
                .write()
                .await
                .remove(&pane_id);
            broadcast_and_persist(state).await;
            return Err(format!("worktree retained: pane could not start: {error}"));
        }
    }
    broadcast_and_persist(state).await;
    if let Some(initial_input) = initial_input {
        let completion =
            crate::initial_prompt::start(Arc::clone(state), pane_id, initial_input).await;
        if wait_for_prompt {
            match tokio::time::timeout(std::time::Duration::from_secs(120), completion).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => {
                    return Err(format!(
                        "pane {} created but prompt was not delivered: {error}",
                        pane_id.0
                    ));
                }
                Ok(Err(_)) => {
                    return Err(format!(
                        "pane {} created but prompt delivery was cancelled",
                        pane_id.0
                    ));
                }
                Err(_) => {
                    return Err(format!(
                        "pane {} created but prompt delivery timed out",
                        pane_id.0
                    ));
                }
            }
        }
    }
    Ok(pane_id)
}

pub(crate) async fn refresh_pane_git_status(
    state: &ServerState,
    pane_id: NodeId,
) -> Result<WorkspaceGitStatus, String> {
    let (workspace, cwd) = {
        let tree = state.tree.read().await;
        let workspace = tree
            .pane_workspace(pane_id)
            .cloned()
            .ok_or_else(|| format!("pane {} has no worktree", pane_id.0))?;
        let cwd = tree
            .pane_cwd(pane_id)
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("pane {} has no launch directory", pane_id.0))?;
        (workspace, cwd)
    };
    let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let execution = state
        .execution
        .get()
        .ok_or_else(|| "workspace execution service is unavailable".to_string())?;
    if let RestoreTarget::Missing(_) = restore_target(&execution.client, &cwd, &workspace).await {
        return Ok(WorkspaceGitStatus {
            branch: Some(workspace.branch),
            detached: false,
            ahead: 0,
            behind: 0,
            staged: 0,
            modified: 0,
            untracked: 0,
            conflicted: 0,
            upstream: None,
            last_commit_subject: None,
            checked_at_unix_millis: now,
            full_checked_at_unix_millis: None,
            missing: true,
        });
    }
    let status = ilium_git::status(&cwd)
        .await
        .map_err(|error| format!("cannot refresh Git status: {error}"))?;
    let subject = ilium_git::branch_facts(&cwd)
        .await
        .ok()
        .map(|facts| facts.last_commit_subject);
    Ok(WorkspaceGitStatus {
        branch: status.branch,
        detached: status.detached,
        ahead: status.ahead,
        behind: status.behind,
        staged: status.staged,
        modified: status.modified,
        untracked: status.untracked,
        conflicted: status.conflicted,
        upstream: status.upstream,
        last_commit_subject: subject,
        checked_at_unix_millis: now,
        full_checked_at_unix_millis: Some(now),
        missing: false,
    })
}

pub(crate) enum WorkspaceRemovalOutcome {
    Removed {
        branch_warning: Option<String>,
    },
    Blocked {
        reasons: Vec<String>,
        pane_closed: bool,
    },
    Uncertain {
        reasons: Vec<String>,
        pane_closed: bool,
    },
}

/// Legacy pane-addressed caller. Retained checkout pruning uses the
/// identity-fenced path transaction in `prune` directly.
pub(crate) async fn remove_workspace(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    force_path: Option<PathBuf>,
    remove_branch: bool,
) -> WorkspaceRemovalOutcome {
    let (result, pane_closed) = prune::remove_pane(state, pane_id, force_path, remove_branch).await;
    match result.outcome {
        ilium_ipc::WorkspacePruneOutcome::Removed => WorkspaceRemovalOutcome::Removed {
            branch_warning: (!result.reasons.is_empty()).then(|| result.reasons.join("; ")),
        },
        ilium_ipc::WorkspacePruneOutcome::Blocked => WorkspaceRemovalOutcome::Blocked {
            reasons: result.reasons,
            pane_closed,
        },
        ilium_ipc::WorkspacePruneOutcome::Uncertain => WorkspaceRemovalOutcome::Uncertain {
            reasons: result.reasons,
            pane_closed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    #[tokio::test]
    async fn new_worktree_path_preparation_waits_for_bounded_io_admission() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let client = execution.client.clone();
        let started = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        let mut blockers = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..2 {
            let reservation = client
                .reserve(
                    Lane::Io,
                    JobCost {
                        input_bytes: 1,
                        result_bytes: 1,
                    },
                )
                .await
                .expect("I/O reservation");
            let worker_started = std::sync::Arc::clone(&started);
            let worker_client = client.clone();
            let (release, wait) = std::sync::mpsc::sync_channel(1);
            releases.push(release);
            blockers.push(tokio::spawn(async move {
                let _ = worker_client
                    .run_reserved(reservation, move |_| {
                        worker_started.add_permits(1);
                        let _ = wait.recv();
                        Ok::<_, std::convert::Infallible>(())
                    })
                    .await
                    .expect("blocking I/O job");
            }));
        }
        let _started = started
            .acquire_many(2)
            .await
            .expect("both I/O workers started");

        let temporary = TempDir::new().expect("temporary worktree parent");
        let parent = temporary.path().join("new-parent");
        let requested = parent.join("worktree");
        let expected_path = requested.clone();
        let preparation_client = client.clone();
        let mut preparation =
            tokio::spawn(async move { canonical_new_path(&preparation_client, requested).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut preparation)
                .await
                .is_err(),
            "path preparation bypassed the saturated I/O bank"
        );
        assert!(!parent.exists(), "parent was created before I/O admission");

        for release in releases {
            let _ = release.send(());
        }
        for blocker in blockers {
            blocker.await.expect("blocking job task");
        }
        let prepared = tokio::time::timeout(std::time::Duration::from_secs(3), preparation)
            .await
            .expect("path preparation completed after admission")
            .expect("path preparation task")
            .expect("valid new worktree path");
        assert_eq!(prepared.view().0, expected_path);
        assert_eq!(prepared.view().1.as_deref(), Some(parent.as_path()));
        assert!(parent.is_dir());
    }

    #[tokio::test]
    async fn repo_facts_metadata_waits_for_bounded_io_admission() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let client = execution.client.clone();
        let started = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        let mut blockers = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..2 {
            let reservation = client
                .reserve(
                    Lane::Io,
                    JobCost {
                        input_bytes: 1,
                        result_bytes: 1,
                    },
                )
                .await
                .expect("I/O reservation");
            let worker_started = std::sync::Arc::clone(&started);
            let worker_client = client.clone();
            let (release, wait) = std::sync::mpsc::sync_channel(1);
            releases.push(release);
            blockers.push(tokio::spawn(async move {
                let _ = worker_client
                    .run_reserved(reservation, move |_| {
                        worker_started.add_permits(1);
                        let _ = wait.recv();
                        Ok::<_, std::convert::Infallible>(())
                    })
                    .await
                    .expect("blocking I/O job");
            }));
        }
        let _started = started
            .acquire_many(2)
            .await
            .expect("both I/O workers started");

        let temporary = TempDir::new().expect("repository metadata fixture");
        let worktree_path = temporary.path().join("checkout");
        std::fs::create_dir(&worktree_path).expect("checkout directory");
        let gitmodules = temporary.path().join(".gitmodules");
        std::fs::write(&gitmodules, "[submodule \"example\"]\n").expect("Git metadata file");
        let worktrees = vec![ilium_git::Worktree {
            path: worktree_path,
            head: None,
            branch: None,
            is_bare: false,
            is_detached: false,
            is_locked: false,
            is_prunable: false,
        }];
        let metadata_client = client.clone();
        let mut metadata = tokio::spawn(async move {
            repo_facts_path_metadata(&metadata_client, &worktrees, &gitmodules).await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut metadata)
                .await
                .is_err(),
            "repository metadata bypassed the saturated I/O bank"
        );

        for release in releases {
            let _ = release.send(());
        }
        for blocker in blockers {
            blocker.await.expect("blocking job task");
        }
        let (directories, has_gitmodules) =
            tokio::time::timeout(std::time::Duration::from_secs(3), metadata)
                .await
                .expect("metadata completed after I/O admission")
                .expect("metadata task")
                .expect("metadata probe");
        assert_eq!(directories, vec![true]);
        assert!(has_gitmodules);
    }

    async fn restore_target(saved_cwd: &Path, workspace: &PaneWorkspace) -> RestoreTarget {
        let client = crate::execution::test_general_client();
        super::restore_target(&client, saved_cwd, workspace).await
    }

    fn git(directory: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .expect("git is installed");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn api_default_path_uses_main_checkout_even_from_a_linked_project() {
        let temporary = TempDir::new().unwrap();
        let fixture_root = paths::canonicalize(temporary.path()).unwrap();
        let main = fixture_root.join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.name", "Ilium Test"]);
        git(&main, &["config", "user.email", "ilium@example.invalid"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "initial"]);
        let linked = paths::canonicalize(temporary.path())
            .unwrap()
            .join("linked");
        ilium_git::create_worktree(&main, &linked, "agent/old", "main")
            .await
            .unwrap();
        let spec = default_new_worktree_spec(&linked, "agent/new-task".into(), Some("main".into()))
            .await
            .unwrap();
        assert_eq!(
            spec,
            WorkspaceCreateSpec::New {
                branch: "agent/new-task".into(),
                base_ref: "main".into(),
                path: paths::canonicalize(temporary.path())
                    .unwrap()
                    .join("main.worktrees/agent-new-task"),
            }
        );
    }

    #[tokio::test]
    async fn restore_accepts_only_the_saved_registered_worktree() {
        let temporary = TempDir::new().unwrap();
        let fixture_root = paths::canonicalize(temporary.path()).unwrap();
        let main = &fixture_root.join("main");
        std::fs::create_dir(main).unwrap();
        git(main, &["init", "-q", "-b", "main"]);
        git(main, &["config", "user.name", "Ilium Test"]);
        git(main, &["config", "user.email", "ilium@example.invalid"]);
        git(main, &["commit", "-q", "--allow-empty", "-m", "initial"]);

        let linked = paths::canonicalize(temporary.path())
            .unwrap()
            .join("linked");
        ilium_git::create_worktree(main, &linked, "agent/restore", "main")
            .await
            .unwrap();
        let cwd = linked.join("src");
        std::fs::create_dir(&cwd).unwrap();
        let repository = ilium_git::discover(main).await.unwrap();
        let workspace = PaneWorkspace {
            workspace_id: Some("test-workspace".into()),
            repo_common_dir: repository.common_dir,
            worktree_root: linked.clone(),
            branch: "agent/restore".into(),
            base_ref: "main".into(),
            base_commit: ilium_git::resolve_commit(main, "main").await.unwrap(),
            created_by_ilium: true,
            created_at_unix: 1_790_380_800,
        };
        assert_eq!(restore_target(&cwd, &workspace).await, RestoreTarget::Ready);
        assert!(matches!(
            restore_target(main, &workspace).await,
            RestoreTarget::Missing(_)
        ));

        ilium_git::remove_worktree(main, &linked, false)
            .await
            .unwrap();
        assert!(matches!(
            restore_target(&cwd, &workspace).await,
            RestoreTarget::Missing(_)
        ));

        std::fs::create_dir(&linked).unwrap();
        git(&linked, &["init", "-q", "-b", "other"]);
        std::fs::create_dir(&cwd).unwrap();
        assert!(matches!(
            restore_target(&cwd, &workspace).await,
            RestoreTarget::Missing(_)
        ));
    }
}
