//! Retained-worktree inventory and identity-fenced removal, independent of pane lifetime.
//! Lock order is repository, spawn admission, tree, panes. No Git runs under tree/panes.
//! The spawn fence is process-local; external writers must be quiescent during removal.
use crate::pane::PaneResource;
use crate::state::ServerState;
use crate::workspace_owner::OwnershipMarker;
use ilium_core::{NodeId, NodeKind, PaneWorkspace};
use ilium_ipc::ServerEvent;
use ilium_ipc::{
    WorkspaceInventory, WorkspaceInventoryEntry, WorkspaceInventoryOwner,
    WorkspacePruneBranchOutcome, WorkspacePruneBranchPolicy, WorkspacePruneMode,
    WorkspacePruneOutcome, WorkspacePruneResult, WorkspacePruneTarget,
};
use ilium_platform::paths; // Canonical path resolution remains platform-owned.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
/// Acquire after the process-local repository mutex and before spawn/tree/pane locks.
pub(crate) async fn repository_lease(
    common_dir: &Path,
) -> Result<ilium_platform::process_control::WorkspaceRepositoryLease, String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let common_dir = common_dir.to_path_buf();
        let lease = tokio::task::spawn_blocking(move || {
            ilium_platform::process_control::try_workspace_repository_lease(&common_dir)
        })
        .await
        .map_err(|error| format!("repository lease worker failed: {error}"))?
        .map_err(|error| format!("repository lease unavailable: {error}"))?;
        if let Some(lease) = lease {
            return Ok(lease);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "repository is busy in another Ilium operation; retry after it finishes".into(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
/// Holding both guards makes spawn and prune share the same cross-process order.
pub(crate) struct SpawnRepositoryAdmission {
    _repository_guard: tokio::sync::OwnedMutexGuard<()>,
    _repository_lease: ilium_platform::process_control::WorkspaceRepositoryLease,
    owned_marker: Option<OwnershipMarker>,
}
impl SpawnRepositoryAdmission {
    pub(crate) async fn begin_custody(
        &self,
    ) -> Result<Option<crate::workspace_custody::CustodyTicket>, String> {
        match &self.owned_marker {
            Some(marker) => crate::workspace_custody::begin(marker).await.map(Some),
            None => Ok(None),
        }
    }
}
/// Non-Git plain panes remain unchanged. A present but unreadable Git root fails closed.
pub(crate) async fn spawn_repository_admission(
    state: &ServerState,
    cwd: &Path,
    is_terminal: bool,
) -> Result<Option<SpawnRepositoryAdmission>, String> {
    let canonical_cwd = ilium_platform::paths::canonicalize(cwd)
        .map_err(|error| format!("launch directory cannot be resolved: {error}"))?;
    if canonical_cwd != cwd {
        return Err("launch directory is not canonical".into());
    }
    let mut git_boundary_present = false;
    for ancestor in canonical_cwd.ancestors() {
        match std::fs::symlink_metadata(ancestor.join(".git")) {
            Ok(_) => {
                git_boundary_present = true;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect launch repository boundary: {error}"
                ))
            }
        }
    }
    if !git_boundary_present {
        return Ok(None);
    }
    let repository = ilium_git::discover(&canonical_cwd)
        .await
        .map_err(|error| format!("launch repository cannot be verified: {error}"))?;
    let repository_guard = state
        .workspace_repository_lock(&repository.common_dir)
        .await
        .lock_owned()
        .await;
    let repository_lease = repository_lease(&repository.common_dir).await?;
    let current = ilium_git::discover(&canonical_cwd)
        .await
        .map_err(|error| format!("launch repository changed while waiting: {error}"))?;
    if current.common_dir != repository.common_dir
        || current.worktree_root != repository.worktree_root
    {
        return Err("launch repository identity changed during admission".into());
    }
    let owned_marker = if is_terminal {
        let listed = ilium_git::list_worktrees(&canonical_cwd)
            .await
            .map_err(|error| format!("launch worktrees cannot be listed: {error}"))?;
        if listed
            .first()
            .is_some_and(|main| main.path == current.worktree_root)
        {
            None
        } else if listed
            .iter()
            .any(|entry| entry.path == current.worktree_root)
        {
            crate::workspace_owner::read_registered_marker(
                &current.common_dir,
                &current.worktree_root,
            )
            .await
            .map_err(|error| format!("launch ownership is unavailable: {error}"))?
        } else {
            return Err("launch worktree registration changed during admission".into());
        }
    } else {
        None
    };
    Ok(Some(SpawnRepositoryAdmission {
        _repository_guard: repository_guard,
        _repository_lease: repository_lease,
        owned_marker,
    }))
}

const MAX_INVENTORY_ROWS: usize = 128;
/// A refusal before the removal command is not a claim about concurrent external edits.
pub(crate) fn blocked(reason: impl Into<String>) -> WorkspacePruneResult {
    WorkspacePruneResult {
        outcome: WorkspacePruneOutcome::Blocked,
        mutation_attempted: false,
        path_present: None,
        registration_present: None,
        metadata_present: None,
        branch_outcome: WorkspacePruneBranchOutcome::Kept,
        reasons: vec![reason.into()],
    }
}
fn workspace_from_marker(marker: OwnershipMarker) -> PaneWorkspace {
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
}
/// A directory generation is a stale-target fence, not ownership by itself.
pub(crate) fn directory_generation(path: &Path) -> Result<(u64, u64), String> {
    ilium_platform::secure_fs::directory_generation(path)
        .map_err(|error| format!("directory generation unavailable: {error}"))
}
async fn target_for(workspace: &PaneWorkspace) -> Result<WorkspacePruneTarget, String> {
    crate::workspace_owner::verify_marker(workspace)
        .await
        .map_err(|error| error.to_string())?;
    let dot_git = std::fs::symlink_metadata(workspace.worktree_root.join(".git"))
        .map_err(|error| error.to_string())?;
    if !dot_git.file_type().is_file() {
        return Err("linked checkout .git is not a regular non-symlink file".into());
    }
    let head_paths = ilium_git::head_paths(&workspace.worktree_root)
        .await
        .map_err(|error| error.to_string())?;
    let metadata_directory = head_paths
        .head
        .parent()
        .ok_or("HEAD has no metadata directory")?
        .to_path_buf();
    let (root_device, root_inode) = directory_generation(&workspace.worktree_root)?;
    let (metadata_device, metadata_inode) = directory_generation(&metadata_directory)?;
    let expected_head = ilium_git::resolve_commit(&workspace.worktree_root, "HEAD")
        .await
        .map_err(|error| error.to_string())?;
    Ok(WorkspacePruneTarget {
        repo_common_dir: workspace.repo_common_dir.clone(),
        worktree_root: workspace.worktree_root.clone(),
        workspace_id: workspace
            .workspace_id
            .clone()
            .ok_or("ownership has no UUID")?,
        creation_branch: workspace.branch.clone(),
        base_ref: workspace.base_ref.clone(),
        base_commit: workspace.base_commit.clone(),
        created_at_unix: workspace.created_at_unix,
        metadata_directory,
        root_device,
        root_inode,
        metadata_device,
        metadata_inode,
        expected_head,
    })
}
async fn verify_target(target: &WorkspacePruneTarget) -> Result<PaneWorkspace, String> {
    let marker = crate::workspace_owner::read_registered_marker(
        &target.repo_common_dir,
        &target.worktree_root,
    )
    .await
    .map_err(|error| format!("ownership is unavailable: {error}"))?
    .ok_or("worktree was not created by Ilium")?;
    let workspace = workspace_from_marker(marker);
    if target_for(&workspace).await? != *target {
        return Err("stale target: ownership, directory generation, or HEAD changed; refresh and confirm again".into());
    }
    Ok(workspace)
}
async fn control_directory(
    workspace: &PaneWorkspace,
) -> Result<(PathBuf, Vec<ilium_git::Worktree>), String> {
    let listed = ilium_git::list_worktrees(&workspace.worktree_root)
        .await
        .map_err(|error| error.to_string())?;
    let main = listed.first().ok_or("repository has no main checkout")?;
    let control = paths::canonicalize(&main.path)
        .map_err(|error| format!("main checkout unavailable: {error}"))?;
    if main.is_bare
        || control == workspace.worktree_root
        || control.starts_with(&workspace.worktree_root)
    {
        return Err("cannot identify a separate protected main checkout".into());
    }
    let discovered = ilium_git::discover(&control)
        .await
        .map_err(|error| error.to_string())?;
    if discovered.is_bare
        || discovered.common_dir != workspace.repo_common_dir
        || discovered.worktree_root != control
    {
        return Err("main checkout repository identity changed".into());
    }
    Ok((control, listed))
}
fn path_uses_root(path: &Path, root: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        || path.starts_with(root)
    {
        return true;
    }
    let mut ancestor = path;
    loop {
        match paths::canonicalize(ancestor) {
            Ok(canonical) => return canonical.starts_with(root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A missing editor file can still be below a symlinked parent.
                // A broken symlink cannot prove that the path is outside.
                match ancestor.symlink_metadata() {
                    Ok(_) => return true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return true,
                }
                match ancestor.parent() {
                    Some(parent) => ancestor = parent,
                    None => return true,
                }
            }
            Err(_) => return true,
        }
    }
}
/// Includes ordinary panes, open editor files, and project/folder roots,
/// not only panes carrying worktree ownership.
async fn admission_facts(
    state: &ServerState,
    root: &Path,
    permitted_pane: Option<NodeId>,
) -> (Vec<NodeId>, Vec<PathBuf>, Vec<String>) {
    let tree = state.tree.read().await;
    let mut occupied = Vec::new();
    let mut protected = Vec::new();
    let mut reasons = Vec::new();
    for pane in tree.panes() {
        if Some(pane.id) == permitted_pane {
            continue;
        }
        let cwd_uses_root = tree
            .pane_cwd(pane.id)
            .is_some_and(|cwd| path_uses_root(cwd, root));
        let owns_root = tree
            .pane_workspace(pane.id)
            .is_some_and(|workspace| path_uses_root(&workspace.worktree_root, root));
        if cwd_uses_root || owns_root {
            occupied.push(pane.id);
        }
    }
    for project_id in tree.project_ids() {
        if let Some(path) = tree
            .get(project_id)
            .and_then(ilium_core::Node::project_path)
        {
            if path_uses_root(path, root) {
                protected.push(path.to_path_buf());
            }
        }
    }
    for node_id in tree.all_ids() {
        if let Some(node) = tree.get(node_id) {
            if let NodeKind::Folder { path, .. } = &node.kind {
                if path_uses_root(path, root) {
                    protected.push(path.clone());
                }
            }
        }
    }
    if path_uses_root(&state.session_cwd, root) {
        protected.push(state.session_cwd.clone());
    }
    let panes = state.panes.read().await;
    for (pane_id, resource) in panes.iter() {
        if let PaneResource::Editor { path: Some(path) } = resource {
            if path_uses_root(path, root) && !occupied.contains(pane_id) {
                occupied.push(*pane_id);
            }
        }
    }
    if panes.keys().any(|pane_id| tree.get(*pane_id).is_none()) {
        reasons.push("a pane resource has no tree node; process custody requires repair".into());
    }
    drop(panes);
    drop(tree);
    protected.sort();
    protected.dedup();
    if !occupied.is_empty() {
        reasons.push(format!("Ilium panes still use this worktree: {occupied:?}"));
    }
    if !protected.is_empty() {
        reasons.push(format!(
            "worktree contains protected session/project paths: {protected:?}"
        ));
    }
    if state.pending_session_recovery.lock().await.is_some() {
        reasons.push("resolve the pending session recovery before removing worktrees".into());
    }
    (occupied, protected, reasons)
}
fn registration_gate(
    workspace: &PaneWorkspace,
    listed: &[ilium_git::Worktree],
) -> Result<(), String> {
    if workspace
        .worktree_root
        .starts_with(&workspace.repo_common_dir)
        || workspace
            .repo_common_dir
            .starts_with(&workspace.worktree_root)
    {
        return Err("worktree overlaps shared Git metadata".into());
    }
    let matches = listed
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.path == workspace.worktree_root)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("worktree registration is missing or ambiguous".into());
    }
    let (index, entry) = matches[0];
    if index == 0 || entry.is_bare || entry.is_locked || entry.is_prunable {
        return Err("main, bare, locked, or prunable registrations cannot be removed here".into());
    }
    if entry.is_detached || entry.branch.as_deref() != Some(workspace.branch.as_str()) {
        return Err("worktree switched away from its recorded creation branch".into());
    }
    if listed.iter().any(|entry| {
        entry.path != workspace.worktree_root
            && path_uses_root(&entry.path, &workspace.worktree_root)
    }) {
        return Err("another registered checkout is nested under this worktree".into());
    }
    Ok(())
}
async fn clean_gate(workspace: &PaneWorkspace) -> Result<(), String> {
    let status = ilium_git::status(&workspace.worktree_root)
        .await
        .map_err(|error| format!("cannot inspect worktree changes: {error}"))?;
    if !status.is_clean() {
        return Err(format!(
            "worktree has staged {}, modified {}, untracked {}, conflicted {} entries",
            status.staged, status.modified, status.untracked, status.conflicted
        ));
    }
    if !ilium_git::is_worktree_pristine_including_ignored(&workspace.worktree_root)
        .await
        .map_err(|error| format!("ignored-inclusive inspection failed: {error}"))?
    {
        return Err("worktree has untracked or ignored files".into());
    }
    Ok(())
}
async fn snapshot_gate(
    state: &ServerState,
    target: &WorkspacePruneTarget,
    mode: &WorkspacePruneMode,
    permitted_pane: Option<NodeId>,
) -> Result<(PaneWorkspace, PathBuf), String> {
    let workspace = verify_target(target).await?;
    let (control, listed) = control_directory(&workspace).await?;
    registration_gate(&workspace, &listed)?;
    let (_, _, reasons) = admission_facts(state, &workspace.worktree_root, permitted_pane).await;
    if !reasons.is_empty() {
        return Err(reasons.join("; "));
    }
    if permitted_pane.is_none() {
        crate::workspace_custody::require_no_tickets(
            &workspace.repo_common_dir,
            &workspace.worktree_root,
        )
        .await?;
    }
    let head = ilium_git::head_probe(&workspace.worktree_root)
        .await
        .map_err(|error| error.to_string())?;
    if head.detached
        || head.branch.as_deref() != Some(workspace.branch.as_str())
        || ilium_git::branch_tip(&control, &workspace.branch)
            .await
            .map_err(|error| error.to_string())?
            .as_deref()
            != Some(target.expected_head.as_str())
    {
        return Err("stale target: creation branch and current HEAD no longer agree".into());
    }
    ilium_git::verify_removal_index(&workspace.worktree_root)
        .await
        .map_err(|error| error.to_string())?;
    match mode {
        WorkspacePruneMode::Safe => {
            clean_gate(&workspace).await?;
            ilium_git::verify_removal_branch(
                &control,
                &workspace.branch,
                &target.expected_head,
                &workspace.base_ref,
                &workspace.base_commit,
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        WorkspacePruneMode::DiscardFiles { confirmed_path }
            if *confirmed_path == target.worktree_root => {}
        WorkspacePruneMode::DiscardFiles { .. } => {
            return Err("discard confirmation does not name the exact worktree path".into())
        }
    }
    Ok((workspace, control))
}
/// This observation never kills an unrelated process and force never bypasses failure.
async fn unused_directory(root: &Path) -> Result<(), String> {
    let users = super::directory_users(root.to_path_buf()).await?;
    if !users.is_empty() {
        return Err(format!("processes still use the worktree: {users:?}"));
    }
    Ok(())
}
/// Retained inventory is read-only and intentionally reports invalid/foreign rows.
pub(crate) async fn inventory(
    state: &ServerState,
    project: NodeId,
) -> Result<WorkspaceInventory, String> {
    let project_cwd = super::project_directory(state, project).await?;
    let source = ilium_git::discover(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    if source.is_bare {
        return Err("bare repositories are not supported by the worktree manager".into());
    }
    let repository_lock = state.workspace_repository_lock(&source.common_dir).await;
    let _repository_guard = repository_lock.lock().await;
    let current_project = super::project_directory(state, project).await?;
    if current_project != project_cwd
        || ilium_git::discover(&current_project)
            .await
            .map_err(|error| error.to_string())?
            .common_dir
            != source.common_dir
    {
        return Err("project repository changed during inventory; refresh".into());
    }
    let listed = ilium_git::list_worktrees(&project_cwd)
        .await
        .map_err(|error| error.to_string())?;
    let control_directory = listed
        .first()
        .ok_or("Git reported no worktrees")?
        .path
        .clone();
    let total_worktrees = listed.len();
    let mut entries = Vec::new();
    for (index, entry) in listed.iter().take(MAX_INVENTORY_ROWS).enumerate() {
        let (occupied_pane_ids, protected_paths, mut common_blockers) =
            admission_facts(state, &entry.path, None).await;
        let mut row = WorkspaceInventoryEntry {
            path: entry.path.clone(),
            branch: entry.branch.clone(),
            head: entry.head.clone(),
            is_main: index == 0,
            is_locked: entry.is_locked,
            is_prunable: entry.is_prunable,
            owner: WorkspaceInventoryOwner::Foreign,
            target: None,
            occupied_pane_ids,
            protected_paths,
            safe_blockers: Vec::new(),
            discard_blockers: Vec::new(),
            merge_target: None,
        };
        if index == 0 || entry.is_bare || entry.is_prunable || !entry.path.is_dir() {
            row.owner = WorkspaceInventoryOwner::Unavailable {
                reason: "main/bare/missing/prunable entry is protected or requires manual repair"
                    .into(),
            };
            common_blockers.push("this registration is not an eligible linked checkout".into());
            row.safe_blockers = common_blockers.clone();
            row.discard_blockers = common_blockers;
            entries.push(row);
            continue;
        }
        let marker =
            crate::workspace_owner::read_registered_marker(&source.common_dir, &entry.path).await;
        let workspace = match marker {
            Ok(Some(marker)) => workspace_from_marker(marker),
            Ok(None) => {
                common_blockers.push(
                    "foreign worktree: use existing is allowed, ownership/removal is not adopted"
                        .into(),
                );
                row.safe_blockers = common_blockers.clone();
                row.discard_blockers = common_blockers;
                entries.push(row);
                continue;
            }
            Err(error) => {
                let reason = format!("ownership unavailable: {error}");
                row.owner = WorkspaceInventoryOwner::Unavailable {
                    reason: reason.clone(),
                };
                common_blockers.push(reason);
                row.safe_blockers = common_blockers.clone();
                row.discard_blockers = common_blockers;
                entries.push(row);
                continue;
            }
        };
        row.owner = WorkspaceInventoryOwner::Owned;
        match target_for(&workspace).await {
            Ok(target) => row.target = Some(target),
            Err(error) => common_blockers.push(error),
        }
        if let Err(error) = registration_gate(&workspace, &listed) {
            common_blockers.push(error);
        }
        if let Err(error) = ilium_git::verify_removal_index(&workspace.worktree_root).await {
            common_blockers.push(error.to_string());
        }
        if let Err(error) = unused_directory(&workspace.worktree_root).await {
            common_blockers.push(error);
        }
        if let Err(error) = crate::workspace_custody::require_no_tickets(
            &workspace.repo_common_dir,
            &workspace.worktree_root,
        )
        .await
        {
            common_blockers.push(error);
        }
        row.discard_blockers = common_blockers.clone();
        row.safe_blockers = common_blockers;
        if let Err(error) = clean_gate(&workspace).await {
            row.safe_blockers.push(error);
        }
        if let Some(target) = &row.target {
            match ilium_git::removal_base(
                &control_directory,
                &workspace.base_ref,
                &workspace.base_commit,
            )
            .await
            {
                Ok((label, _)) => row.merge_target = Some(label),
                Err(error) => row.safe_blockers.push(error.to_string()),
            }
            if let Err(error) = ilium_git::verify_removal_branch(
                &control_directory,
                &workspace.branch,
                &target.expected_head,
                &workspace.base_ref,
                &workspace.base_commit,
            )
            .await
            {
                row.safe_blockers.push(error.to_string());
            }
        }
        entries.push(row);
    }
    Ok(WorkspaceInventory {
        repo_common_dir: source.common_dir,
        control_directory,
        total_worktrees,
        truncated: total_worktrees > MAX_INVENTORY_ROWS,
        entries,
    })
}
/// A failed remove can have partial effects; absence must be observed independently.
fn path_presence(path: &Path) -> Option<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}
fn removal_observed(
    path_present: Option<bool>,
    registered: Option<bool>,
    metadata_present: Option<bool>,
) -> WorkspacePruneOutcome {
    if path_present == Some(false) && registered == Some(false) && metadata_present == Some(false) {
        return WorkspacePruneOutcome::Removed;
    }
    WorkspacePruneOutcome::Uncertain
}
async fn mutate_exact(
    target: &WorkspacePruneTarget,
    workspace: &PaneWorkspace,
    control: &Path,
    mode: &WorkspacePruneMode,
    branch_policy: WorkspacePruneBranchPolicy,
) -> WorkspacePruneResult {
    let forced = matches!(mode, WorkspacePruneMode::DiscardFiles { .. });
    // Leave the marker in Git metadata. Git removes that metadata with the checkout.
    // This avoids both an ownerless live-checkout window and unsafe marker restoration.
    let command_result = ilium_git::remove_worktree(control, &target.worktree_root, forced).await;
    let path_present = path_presence(&target.worktree_root);
    let metadata_present = path_presence(&target.metadata_directory);
    let registration_present = ilium_git::list_worktrees(control)
        .await
        .ok()
        .map(|entries| {
            entries
                .iter()
                .any(|entry| entry.path == target.worktree_root)
        });
    // An unavailable command-exit proof cannot be upgraded by a momentary absence scan.
    let settled = matches!(
        &command_result,
        Ok(()) | Err(ilium_git::GitError::Command { .. })
    );
    let outcome = if settled {
        removal_observed(path_present, registration_present, metadata_present)
    } else {
        WorkspacePruneOutcome::Uncertain
    };
    let mut result = WorkspacePruneResult {
        outcome,
        mutation_attempted: true,
        path_present,
        registration_present,
        metadata_present,
        branch_outcome: WorkspacePruneBranchOutcome::Kept,
        reasons: Vec::new(),
    };
    if let Err(error) = &command_result {
        result.reasons.push(format!(
            "Git remove returned an error: {error}; do not retry automatically"
        ));
    }
    if outcome != WorkspacePruneOutcome::Removed {
        result.reasons.push("removal effects are uncertain; inspect path, registration, metadata, and files; no branch deletion or ownership rewrite was attempted".into());
        return result;
    }
    if command_result.is_err() || branch_policy == WorkspacePruneBranchPolicy::Keep {
        return result;
    }
    let deletion = ilium_git::delete_removal_branch(
        control,
        &workspace.branch,
        &target.expected_head,
        &workspace.base_ref,
        &workspace.base_commit,
    )
    .await;
    let branch_tip = ilium_git::branch_tip(control, &workspace.branch).await;
    result.branch_outcome = match (&deletion, branch_tip) {
        (Ok(()), Ok(None)) => WorkspacePruneBranchOutcome::Deleted,
        (Err(_), Ok(None)) => WorkspacePruneBranchOutcome::Absent,
        (_, Ok(Some(_))) => WorkspacePruneBranchOutcome::Kept,
        (_, Err(_)) => WorkspacePruneBranchOutcome::Unknown,
    };
    if deletion.is_ok() && result.branch_outcome == WorkspacePruneBranchOutcome::Kept {
        result.reasons.push("branch is present after the deletion attempt, possibly recreated; no retry was performed".into());
    }
    if let Err(error) = deletion {
        result.reasons.push(format!(
            "worktree removal was observed; safe branch deletion did not complete: {error}"
        ));
    }
    if result.branch_outcome == WorkspacePruneBranchOutcome::Unknown {
        result
            .reasons
            .push("branch postcondition is unavailable; inspect the exact creation branch".into());
    }
    result
}
/// This function never holds a tree or pane lock while waiting for PTY exit.
async fn stop_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    workspace: &PaneWorkspace,
) -> Result<(), (String, bool)> {
    let tree = state.tree.read().await;
    if tree.pane_workspace(pane_id) != Some(workspace) {
        return Err((
            "pane ownership changed before process termination".into(),
            false,
        ));
    }
    let mut panes = state.panes.write().await;
    let resource = match panes.remove(&pane_id) {
        Some(resource @ PaneResource::Terminal(_)) => resource,
        Some(resource) => {
            panes.insert(pane_id, resource);
            return Err(("workspace pane has no terminal process".into(), false));
        }
        None => return Err(("workspace terminal process is unavailable".into(), false)),
    };
    drop(panes);
    drop(tree);
    let joined = tokio::task::spawn_blocking(move || {
        let mut resource = resource;
        let termination = match &mut resource {
            PaneResource::Terminal(runtime) => runtime
                .session
                .terminate_process_tree(std::time::Duration::from_secs(5)),
            _ => unreachable!("resource was checked before transfer"),
        };
        (resource, termination)
    })
    .await;
    let (mut resource, termination) =
        joined.map_err(|error| (format!("PTY custody worker failed: {error}"), true))?;
    if let Err(error) = termination {
        let tree = state.tree.read().await;
        let mut panes = state.panes.write().await;
        if tree.pane_workspace(pane_id) == Some(workspace) && !panes.contains_key(&pane_id) {
            panes.insert(pane_id, resource);
            return Err((
                format!("PTY descendants cannot be proven stopped: {error}"),
                false,
            ));
        }
        drop(panes);
        drop(tree);
        resource.abort_background_tasks();
        return Err((
            format!("pane disappeared and PTY custody remains uncertain: {error}"),
            true,
        ));
    }
    if let PaneResource::Terminal(runtime) = &mut resource {
        if let Some(ticket) = runtime.custody_ticket.take() {
            if let Err(error) = unused_directory(ticket.worktree_root()).await {
                resource.abort_background_tasks();
                return Err((format!("PTY stopped but custody remains: {error}"), true));
            }
            let cleared = tokio::task::spawn_blocking(move || ticket.clear_after_proof()).await;
            if let Err(error) = cleared
                .map_err(|error| format!("custody clear worker failed: {error}"))
                .and_then(|result| result)
            {
                resource.abort_background_tasks();
                return Err((format!("PTY stopped but custody remains: {error}"), true));
            }
        }
    }
    resource.abort_background_tasks();
    drop(resource);
    Ok(())
}
fn cancelled(state: &ServerState, reply: Option<&mpsc::Sender<ServerEvent>>) -> bool {
    !state.accepts_workspace_creation() || reply.is_some_and(mpsc::Sender::is_closed)
}
/// Caller owns repository then spawn guards for the whole transaction, including postconditions.
async fn execute_locked(
    state: &Arc<ServerState>,
    target: &WorkspacePruneTarget,
    mode: &WorkspacePruneMode,
    branch_policy: WorkspacePruneBranchPolicy,
    permitted_pane: Option<NodeId>,
    reply: Option<&mpsc::Sender<ServerEvent>>,
) -> (WorkspacePruneResult, bool) {
    if cancelled(state, reply) {
        return (blocked("removal cancelled before mutation"), false);
    }
    let (workspace, _) = match snapshot_gate(state, target, mode, permitted_pane).await {
        Ok(value) => value,
        Err(error) => return (blocked(error), false),
    };
    // With a live owned pane, only availability is checked before stopping it.
    // Its own process is expected to appear. The final scan requires an empty result.
    if let Err(error) = super::directory_users(target.worktree_root.clone()).await {
        return (blocked(error), false);
    }
    let pane_closed = if let Some(pane_id) = permitted_pane {
        match stop_pane(state, pane_id, &workspace).await {
            Ok(()) => true,
            Err((error, closed)) => return (blocked(error), closed),
        }
    } else {
        false
    };
    if cancelled(state, reply) {
        return (
            blocked("removal cancelled before Git mutation; no automatic retry"),
            pane_closed,
        );
    }
    let (workspace, control) = match snapshot_gate(state, target, mode, permitted_pane).await {
        Ok(value) => value,
        Err(error) => return (blocked(error), pane_closed),
    };
    if let Err(error) = unused_directory(&target.worktree_root).await {
        return (blocked(error), pane_closed);
    }
    if let Err(error) = crate::workspace_custody::require_no_tickets(
        &workspace.repo_common_dir,
        &workspace.worktree_root,
    )
    .await
    {
        return (blocked(error), pane_closed);
    }
    // Last ownership/generation recheck is inside the process-admission fence.
    if let Err(error) = verify_target(target).await {
        return (blocked(error), pane_closed);
    }
    if cancelled(state, reply) {
        return (
            blocked("removal cancelled at the final mutation boundary"),
            pane_closed,
        );
    }
    (
        mutate_exact(target, &workspace, &control, mode, branch_policy).await,
        pane_closed,
    )
}
pub(crate) async fn remove_retained(
    state: &Arc<ServerState>,
    project: NodeId,
    target: &WorkspacePruneTarget,
    mode: &WorkspacePruneMode,
    branch_policy: WorkspacePruneBranchPolicy,
    reply: Option<&mpsc::Sender<ServerEvent>>,
) -> WorkspacePruneResult {
    let project_cwd = match super::project_directory(state, project).await {
        Ok(cwd) => cwd,
        Err(error) => return blocked(error),
    };
    let source = match ilium_git::discover(&project_cwd).await {
        Ok(repository)
            if !repository.is_bare && repository.common_dir == target.repo_common_dir =>
        {
            repository
        }
        _ => return blocked("target does not match the selected project's repository"),
    };
    let repository_lock = state.workspace_repository_lock(&source.common_dir).await;
    let _repository_guard = repository_lock.lock().await;
    let _repository_lease = match repository_lease(&source.common_dir).await {
        Ok(lease) => lease,
        Err(error) => return blocked(error),
    };
    let _spawn_guard = state.workspace_spawn_lock.lock().await;
    let current_project = match super::project_directory(state, project).await {
        Ok(cwd) if cwd == project_cwd => cwd,
        _ => return blocked("project changed while waiting for removal admission"),
    };
    match ilium_git::discover(&current_project).await {
        Ok(repository)
            if !repository.is_bare && repository.common_dir == target.repo_common_dir => {}
        _ => return blocked("target is not in the selected project's current repository"),
    }
    execute_locked(state, target, mode, branch_policy, None, reply)
        .await
        .0
}
/// Legacy live-pane removal shares the same gates and postcondition engine.
pub(crate) async fn remove_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    force_path: Option<PathBuf>,
    remove_branch: bool,
) -> (WorkspacePruneResult, bool) {
    let workspace = match state.tree.read().await.pane_workspace(pane_id).cloned() {
        Some(workspace) if workspace.created_by_ilium => workspace,
        Some(_) => return (blocked("worktree was not created by Ilium"), false),
        None => return (blocked("pane has no worktree"), false),
    };
    let repository_lock = state
        .workspace_repository_lock(&workspace.repo_common_dir)
        .await;
    let _repository_guard = repository_lock.lock().await;
    let _repository_lease = match repository_lease(&workspace.repo_common_dir).await {
        Ok(lease) => lease,
        Err(error) => return (blocked(error), false),
    };
    let _spawn_guard = state.workspace_spawn_lock.lock().await;
    let target = match target_for(&workspace).await {
        Ok(target) => target,
        Err(error) => return (blocked(error), false),
    };
    let mode = match force_path {
        Some(confirmed_path) => WorkspacePruneMode::DiscardFiles { confirmed_path },
        None => WorkspacePruneMode::Safe,
    };
    let branch_policy = if remove_branch {
        WorkspacePruneBranchPolicy::DeleteIfSafe
    } else {
        WorkspacePruneBranchPolicy::Keep
    };
    execute_locked(state, &target, &mode, branch_policy, Some(pane_id), None).await
}

/// Read-only close hint. The later disposition request repeats every removal
/// gate while holding the repository lease and spawn fence.
pub(crate) async fn can_offer_close(state: &Arc<ServerState>, pane_id: NodeId) -> bool {
    let workspace = {
        let tree = state.tree.read().await;
        match tree.pane_workspace(pane_id) {
            Some(workspace) if workspace.created_by_ilium => workspace.clone(),
            _ => return false,
        }
    };
    let preference = {
        let preferences = state.workspace_close_preferences.read().await;
        match preferences.get(&pane_id) {
            Some(preference)
                if preference.matches_workspace(&workspace)
                    && preference.policy
                        == ilium_ipc::WorkspaceClosePolicy::OfferRemovalWhenSafe =>
            {
                preference.clone()
            }
            _ => return false,
        }
    };
    let ticket = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => match &runtime.custody_ticket {
                Some(ticket) => ticket.clone(),
                None => return false,
            },
            _ => return false,
        }
    };
    let repository_lock = state
        .workspace_repository_lock(&workspace.repo_common_dir)
        .await;
    let _repository_guard = repository_lock.lock().await;
    let marker = match crate::workspace_owner::verify_marker(&workspace).await {
        Ok(marker) => marker,
        Err(_) => return false,
    };
    if crate::workspace_custody::require_only_ticket(&marker, &ticket)
        .await
        .is_err()
        || super::directory_users(workspace.worktree_root.clone())
            .await
            .is_err()
    {
        return false;
    }
    let target = match target_for(&workspace).await {
        Ok(target) => target,
        Err(_) => return false,
    };
    if snapshot_gate(state, &target, &WorkspacePruneMode::Safe, Some(pane_id))
        .await
        .is_err()
    {
        return false;
    }
    let tree = state.tree.read().await;
    let preferences = state.workspace_close_preferences.read().await;
    tree.pane_workspace(pane_id) == Some(&workspace)
        && preferences.get(&pane_id) == Some(&preference)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_requires_all_three_absence_observations() {
        assert_eq!(
            removal_observed(Some(false), Some(false), Some(false)),
            WorkspacePruneOutcome::Removed
        );
        for value in [None, Some(true)] {
            assert_eq!(
                removal_observed(value, Some(false), Some(false)),
                WorkspacePruneOutcome::Uncertain
            );
            assert_eq!(
                removal_observed(Some(false), value, Some(false)),
                WorkspacePruneOutcome::Uncertain
            );
            assert_eq!(
                removal_observed(Some(false), Some(false), value),
                WorkspacePruneOutcome::Uncertain
            );
        }
    }
    #[test]
    fn refusal_does_not_claim_observed_path_or_registration_state() {
        let result = blocked("stale");
        assert!(!result.mutation_attempted);
        assert_eq!(result.path_present, None);
        assert_eq!(result.registration_present, None);
        assert_eq!(result.metadata_present, None);
    }
}
