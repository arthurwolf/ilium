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
use ilium_ipc::{
    RepoFacts, ServerEvent, WorkspaceClosePolicy, WorkspaceCreateSpec, WorkspaceCreateStage,
    WorkspaceGitStatus, WorkspaceGitVersion, WorkspaceWorktreeFact,
};
use ilium_platform::{paths, secure_fs};
use tokio::sync::mpsc;

use crate::ipc::handlers::{
    broadcast_and_persist, spawn_and_register_pane_in_directory, RegisterPaneError,
};
use crate::pane::{PaneSnapshotKind, TerminalOrigin};
use crate::state::{ServerState, WorkspaceClosePreference};

#[path = "workspace_prune.rs"]
pub(crate) mod prune;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreTarget {
    Ready(PathBuf),
    Missing(String),
}

/// Validates a saved workspace before any provider resume or automatic input
/// can run there. A missing or replaced worktree must get a harmless fallback
/// shell; its original agent command stays in the snapshot for later recovery.
pub(crate) async fn restore_target(saved_cwd: &Path, workspace: &PaneWorkspace) -> RestoreTarget {
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

    let root = match paths::canonicalize(&workspace.worktree_root) {
        Ok(root) => root,
        Err(error) => {
            return RestoreTarget::Missing(format!("worktree directory is unavailable: {error}"));
        }
    };
    if root != workspace.worktree_root {
        return RestoreTarget::Missing(
            "worktree directory no longer resolves to its saved path".into(),
        );
    }

    let cwd = match paths::canonicalize(saved_cwd) {
        Ok(cwd) if cwd.is_dir() && cwd.starts_with(&root) => cwd,
        Ok(_) => {
            return RestoreTarget::Missing(
                "launch directory now resolves outside the worktree".into(),
            );
        }
        Err(error) => {
            return RestoreTarget::Missing(format!("launch directory is unavailable: {error}"));
        }
    };

    let repository = match ilium_git::discover(&root).await {
        Ok(repository) => repository,
        Err(error) => {
            return RestoreTarget::Missing(format!(
                "worktree Git identity cannot be verified: {error}"
            ));
        }
    };
    if repository.is_bare
        || repository.worktree_root != root
        || repository.common_dir != workspace.repo_common_dir
    {
        return RestoreTarget::Missing(
            "worktree Git identity differs from the saved repository".into(),
        );
    }

    let entries = match ilium_git::list_worktrees(&root).await {
        Ok(entries) => entries,
        Err(error) => {
            return RestoreTarget::Missing(format!(
                "Git worktree registration cannot be verified: {error}"
            ));
        }
    };
    let is_listed = entries
        .iter()
        .filter(|entry| !entry.is_bare)
        .any(|entry| paths::canonicalize(&entry.path).ok().as_deref() == Some(root.as_path()));
    if !is_listed {
        return RestoreTarget::Missing("directory is absent from Git's worktree list".into());
    }

    RestoreTarget::Ready(cwd)
}

async fn project_directory(state: &ServerState, node_id: NodeId) -> Result<PathBuf, String> {
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
    for entry in listed {
        let existing = occupied
            .iter()
            .find(|(_, directory)| directory.starts_with(&entry.path));
        let is_dirty = if entry.is_prunable || !entry.path.is_dir() {
            true
        } else {
            ilium_git::status(&entry.path)
                .await
                .map(|status| !status.is_clean())
                .unwrap_or(true)
        };
        let created_by_ilium =
            if entry.path == main_directory || entry.is_prunable || !entry.path.is_dir() {
                false
            } else {
                crate::workspace_owner::read_registered_marker(&repository.common_dir, &entry.path)
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
    let has_gitmodules = repository.worktree_root.join(".gitmodules").is_file();
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
    direct_tx: Option<&mpsc::Sender<ServerEvent>>,
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

fn canonical_new_path(path: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("new worktree path must be absolute and normalized".into());
    }
    let parent = path.parent().ok_or("new worktree path has no parent")?;
    let file_name = path
        .file_name()
        .ok_or("new worktree path has no final name")?;
    let (canonical_parent, created_parent) = match parent.symlink_metadata() {
        Ok(_) => (
            paths::canonicalize(parent)
                .map_err(|error| format!("worktree parent is unavailable: {error}"))?,
            None,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let grandparent = parent.parent().ok_or("worktree parent has no parent")?;
            let parent_name = parent.file_name().ok_or("worktree parent has no name")?;
            let canonical_grandparent = paths::canonicalize(grandparent)
                .map_err(|error| format!("worktree parent base is unavailable: {error}"))?;
            if canonical_grandparent.join(parent_name) != parent {
                return Err("worktree parent base is not canonical".into());
            }
            std::fs::create_dir(parent)
                .map_err(|error| format!("cannot create worktree parent: {error}"))?;
            (parent.to_path_buf(), Some(parent.to_path_buf()))
        }
        Err(error) => return Err(format!("cannot inspect worktree parent: {error}")),
    };
    if !canonical_parent.is_dir() || canonical_parent.join(file_name) != path {
        return Err("new worktree path has a noncanonical parent".into());
    }
    match path.symlink_metadata() {
        Ok(_) => {
            return Err(format!(
                "new worktree path already exists: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect worktree path: {error}")),
    }
    Ok((path.to_path_buf(), created_parent))
}

/// Roll back only when include preparation wrote no path and Git finds no
/// other changes. File identity does not prove copied contents are unchanged.
enum CreatedIncludes {
    Complete(crate::worktree_include::IncludeCopyReport),
    Partial(crate::worktree_include::IncludeCopyError),
}

async fn rollback_pre_pane_workspace(
    control_directory: &Path,
    workspace: &PaneWorkspace,
    created_parent: Option<&Path>,
    created_includes: CreatedIncludes,
) -> String {
    let path = workspace.worktree_root.clone();
    let copied_paths_exist = match &created_includes {
        CreatedIncludes::Complete(report) => !report.created_paths.is_empty(),
        CreatedIncludes::Partial(error) => !error.created_paths.is_empty(),
    };
    if copied_paths_exist {
        return format!(
            "worktree retained at {}: include preparation wrote files whose contents may have changed; no copied files were removed",
            path.display()
        );
    }
    let cleanup_path = path.clone();
    match tokio::task::spawn_blocking(move || match created_includes {
        CreatedIncludes::Complete(report) => {
            crate::worktree_include::remove_created_report(&cleanup_path, &report)
        }
        CreatedIncludes::Partial(error) => {
            crate::worktree_include::remove_created_files(&cleanup_path, &error)
        }
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(problem)) => {
            return format!(
                "worktree retained at {}: copied-file rollback is unsafe: {problem}",
                path.display()
            );
        }
        Err(problem) => {
            return format!(
                "worktree retained at {}: copied-file rollback task failed: {problem}",
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
    if let Err(problem) = crate::workspace_owner::verify_marker(workspace).await {
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
    match directory_users(path.clone()).await {
        Ok(users) if users.is_empty() => {}
        Ok(users) => {
            return format!(
                "worktree retained at {}: processes still use its directory: {users:?}",
                path.display()
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
    let path_absent = matches!(path.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
    let metadata_absent = matches!(metadata_directory.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
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
        let _ = std::fs::remove_dir(parent);
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
    included_files: Option<crate::worktree_include::IncludeCopyReport>,
) -> String {
    let Some(report) = included_files else {
        return "existing worktree left unchanged".into();
    };
    let repo_lock = state
        .workspace_repository_lock(&workspace.repo_common_dir)
        .await;
    let _guard = repo_lock.lock().await;
    let _repository_lease = match prune::repository_lease(&workspace.repo_common_dir).await {
        Ok(lease) => lease,
        Err(error) => {
            return format!("worktree retained: rollback repository admission failed: {error}");
        }
    };
    rollback_pre_pane_workspace(
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
    direct_tx: Option<&mpsc::Sender<ServerEvent>>,
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
    let mut repository_lease = Some(prune::repository_lease(&source.common_dir).await?);

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
            let (path, created_parent) = canonical_new_path(&path)?;
            progress(
                direct_tx,
                request_id,
                WorkspaceCreateStage::CreatingWorktree,
            )
            .await;
            if !state.accepts_workspace_creation() || direct_tx.is_some_and(mpsc::Sender::is_closed)
            {
                if let Some(parent) = created_parent {
                    let _ = std::fs::remove_dir(parent);
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
                let path_state = match path.symlink_metadata() {
                    Ok(_) => "present".to_string(),
                    Err(probe) if probe.kind() == std::io::ErrorKind::NotFound => "absent".into(),
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
                        let _ = std::fs::remove_dir(parent);
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
            crate::workspace_owner::create_marker(&mut workspace)
                .await
                .map_err(|error| {
                    format!(
                        "worktree retained at {}: could not record ownership: {error}",
                        path.display()
                    )
                })?;
            if !launch_cwd.is_dir() {
                let rollback = rollback_pre_pane_workspace(
                    &project_cwd,
                    &workspace,
                    created_parent.as_deref(),
                    CreatedIncludes::Complete(Default::default()),
                )
                .await;
                return Err(format!(
                    "project subdirectory {} is absent at the selected base; {rollback}",
                    source.project_subpath.display()
                ));
            }
            if !state.accepts_workspace_creation() || direct_tx.is_some_and(mpsc::Sender::is_closed)
            {
                let rollback = rollback_pre_pane_workspace(
                    &project_cwd,
                    &workspace,
                    created_parent.as_deref(),
                    CreatedIncludes::Complete(Default::default()),
                )
                .await;
                return Err(format!(
                    "workspace creation cancelled before pane commit; {rollback}"
                ));
            }
            let source_project = project_cwd.clone();
            let target_project = launch_cwd.clone();
            let include_result = tokio::task::spawn_blocking(move || {
                crate::worktree_include::copy_worktree_includes(&source_project, &target_project)
            })
            .await
            .map_err(|error| {
                format!(
                    "worktree retained at {}: include task failed: {error}",
                    path.display()
                )
            })?;
            let include_report = match include_result {
                Ok(report) => report,
                Err(error) => {
                    let reason = error.to_string();
                    let rollback = rollback_pre_pane_workspace(
                        &project_cwd,
                        &workspace,
                        created_parent.as_deref(),
                        CreatedIncludes::Partial(error),
                    )
                    .await;
                    return Err(format!("include preparation failed: {reason}; {rollback}"));
                }
            };
            if let Some(command) = setup_command.as_deref() {
                progress(direct_tx, request_id, WorkspaceCreateStage::RunningSetup).await;
                crate::workspace_setup::run(command, &launch_cwd, &workspace.worktree_root, || {
                    !state.accepts_workspace_creation()
                        || direct_tx.is_some_and(mpsc::Sender::is_closed)
                })
                .await
                .map_err(|error| {
                    format!(
                        "setup failed: {error}; worktree retained at {} for inspection",
                        path.display()
                    )
                })?;
                crate::workspace_owner::verify_marker(&workspace)
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
            if !launch_cwd.is_dir() {
                return Err("project subdirectory is absent in the selected worktree".into());
            }
            let marker = crate::workspace_owner::read_registered_marker(&source.common_dir, &path)
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
    if let RestoreTarget::Missing(error) = restore_target(&launch_cwd, &workspace).await {
        return Err(format!(
            "worktree retained at {}: launch target cannot be verified: {error}",
            workspace.worktree_root.display()
        ));
    }
    progress(direct_tx, request_id, WorkspaceCreateStage::Starting).await;
    if !state.accepts_workspace_creation() || direct_tx.is_some_and(mpsc::Sender::is_closed) {
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
        if !state.accepts_workspace_creation() || direct_tx.is_some_and(mpsc::Sender::is_closed) {
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
    if let RestoreTarget::Missing(_) = restore_target(&cwd, &workspace).await {
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

async fn directory_users(path: PathBuf) -> Result<Vec<u32>, String> {
    tokio::task::spawn_blocking(move || {
        ilium_platform::process_control::processes_using_directory(&path)
    })
    .await
    .map_err(|error| format!("process directory probe failed: {error}"))?
    .map_err(|error| format!("process directory probe is unavailable: {error}"))
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
        assert_eq!(
            restore_target(&cwd, &workspace).await,
            RestoreTarget::Ready(cwd.clone())
        );
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
