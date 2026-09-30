//! Durable ownership evidence for worktrees created by Ilium.
//!
//! The marker lives in Git's per-worktree metadata, not the checkout. Git
//! chooses that directory name, so always resolve it through `head_paths`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::fs;

use ilium_core::PaneWorkspace;
use ilium_platform::secure_fs::NoFollowDirectory;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const MARKER_FILE: &str = "ilium-workspace-owner.json";
const MAX_MARKER_BYTES: u64 = 4096;

#[derive(Debug, Error)]
pub(crate) enum WorkspaceOwnerError {
    #[error("Git worktree identity could not be verified: {0}")]
    Git(#[from] ilium_git::GitError),
    #[error("worktree ownership path could not be accessed: {0}")]
    Io(#[from] std::io::Error),
    #[error("worktree ownership marker is invalid: {0}")]
    Invalid(&'static str),
    #[error("worktree ownership marker already exists")]
    AlreadyExists,
    #[error("worktree ownership marker does not match the pane")]
    Mismatch,
    #[error("worktree ownership marker could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnershipMarker {
    pub custody_revision: u32,
    pub workspace_id: String,
    pub repo_common_dir: PathBuf,
    pub worktree_root: PathBuf,
    pub branch: String,
    pub base_ref: String,
    pub base_commit: String,
    pub created_at_unix: i64,
}

/// Writes an exclusive marker after `git worktree add`. On success the newly
/// generated ID is copied into the pane snapshot. A failed write leaves the
/// snapshot untouched; callers must handle any incomplete file explicitly.
pub(crate) async fn create_marker(
    workspace: &mut PaneWorkspace,
) -> Result<OwnershipMarker, WorkspaceOwnerError> {
    if workspace.workspace_id.is_some() {
        return Err(WorkspaceOwnerError::Invalid("workspace already has an ID"));
    }
    let marker_path = validated_marker_path(workspace).await?;
    let head = ilium_git::head_probe(&workspace.worktree_root).await?;
    if head.branch.as_deref() != Some(workspace.branch.as_str()) {
        return Err(WorkspaceOwnerError::Invalid(
            "worktree branch differs from creation branch",
        ));
    }
    if ilium_git::branch_tip(&workspace.worktree_root, &workspace.branch).await?
        != Some(workspace.base_commit.clone())
    {
        return Err(WorkspaceOwnerError::Invalid(
            "worktree tip differs from creation base commit",
        ));
    }
    let marker = OwnershipMarker {
        custody_revision: 1,
        workspace_id: Uuid::new_v4().to_string(),
        repo_common_dir: workspace.repo_common_dir.clone(),
        worktree_root: workspace.worktree_root.clone(),
        branch: workspace.branch.clone(),
        base_ref: workspace.base_ref.clone(),
        base_commit: workspace.base_commit.clone(),
        created_at_unix: workspace.created_at_unix,
    };
    validate_marker_identity(
        &marker,
        &workspace.repo_common_dir,
        &workspace.worktree_root,
    )?;
    let bytes = serde_json::to_vec(&marker)?;
    tokio::task::spawn_blocking(move || {
        let parent = marker_path
            .parent()
            .ok_or_else(|| std::io::Error::other("marker has no parent"))?;
        let directory = NoFollowDirectory::open_root(parent)?;
        let mut file = directory.create_regular(MARKER_FILE.as_ref())?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = directory.remove_regular(MARKER_FILE.as_ref(), &file);
            return Err(error);
        }
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|error| std::io::Error::other(format!("marker write task failed: {error}")))?
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            WorkspaceOwnerError::AlreadyExists
        } else {
            WorkspaceOwnerError::Io(error)
        }
    })?;
    workspace.workspace_id = Some(marker.workspace_id.clone());
    Ok(marker)
}

/// Confirms that a saved pane still owns the registered linked worktree.
/// Switching the live branch does not change the creation identity.
pub(crate) async fn verify_marker(
    workspace: &PaneWorkspace,
) -> Result<OwnershipMarker, WorkspaceOwnerError> {
    let marker_path = validated_marker_path(workspace).await?;
    let marker = expected_marker(workspace)?;
    let bytes = read_marker_bytes(&marker_path).await?;
    if bytes != serde_json::to_vec(&marker)? {
        return Err(WorkspaceOwnerError::Mismatch);
    }
    Ok(marker)
}

/// Reads a retained Ilium marker without a pane snapshot. Both input paths
/// must already be canonical. `None` means only that the exact marker file is
/// absent in a registered linked worktree; invalid Git metadata, symlinks,
/// malformed markers, and mismatched identities fail closed.
///
/// Pre-provenance markers (without `base_ref` and `base_commit`) are invalid:
/// their creation base cannot be reconstructed safely from a branch's current
/// tip, so they require explicit manual handling rather than automatic prune.
pub(crate) async fn read_registered_marker(
    repo_common_dir: &Path,
    worktree_root: &Path,
) -> Result<Option<OwnershipMarker>, WorkspaceOwnerError> {
    let marker_path = validated_marker_path_for(repo_common_dir, worktree_root).await?;
    let Some(bytes) = read_marker_bytes_if_present(&marker_path).await? else {
        return Ok(None);
    };
    let marker: OwnershipMarker = serde_json::from_slice(&bytes)
        .map_err(|_| WorkspaceOwnerError::Invalid("marker JSON or schema is invalid"))?;
    validate_marker_identity(&marker, repo_common_dir, worktree_root)?;
    if serde_json::to_vec(&marker)? != bytes {
        return Err(WorkspaceOwnerError::Invalid(
            "marker is not in the original canonical form",
        ));
    }
    Ok(Some(marker))
}

fn expected_marker(workspace: &PaneWorkspace) -> Result<OwnershipMarker, WorkspaceOwnerError> {
    let workspace_id = workspace
        .workspace_id
        .as_deref()
        .ok_or(WorkspaceOwnerError::Invalid("pane has no workspace ID"))?;
    let marker = OwnershipMarker {
        custody_revision: 1,
        workspace_id: workspace_id.to_owned(),
        repo_common_dir: workspace.repo_common_dir.clone(),
        worktree_root: workspace.worktree_root.clone(),
        branch: workspace.branch.clone(),
        base_ref: workspace.base_ref.clone(),
        base_commit: workspace.base_commit.clone(),
        created_at_unix: workspace.created_at_unix,
    };
    validate_marker_identity(
        &marker,
        &workspace.repo_common_dir,
        &workspace.worktree_root,
    )?;
    Ok(marker)
}

async fn validated_marker_path(workspace: &PaneWorkspace) -> Result<PathBuf, WorkspaceOwnerError> {
    if !workspace.created_by_ilium || workspace.branch.is_empty() || workspace.created_at_unix <= 0
    {
        return Err(WorkspaceOwnerError::Invalid(
            "pane has no Ilium creation identity",
        ));
    }
    validated_marker_path_for(&workspace.repo_common_dir, &workspace.worktree_root).await
}

fn validate_marker_identity(
    marker: &OwnershipMarker,
    repo_common_dir: &Path,
    worktree_root: &Path,
) -> Result<(), WorkspaceOwnerError> {
    let workspace_id = Uuid::parse_str(&marker.workspace_id)
        .map_err(|_| WorkspaceOwnerError::Invalid("workspace ID is not a UUID"))?;
    if workspace_id.to_string() != marker.workspace_id || workspace_id.get_version_num() != 4 {
        return Err(WorkspaceOwnerError::Invalid(
            "workspace ID is not a canonical version 4 UUID",
        ));
    }
    if marker.repo_common_dir != repo_common_dir || marker.worktree_root != worktree_root {
        return Err(WorkspaceOwnerError::Mismatch);
    }
    if marker.custody_revision != 1
        || marker.branch.is_empty()
        || marker.branch.starts_with('-')
        || marker.branch.chars().any(char::is_control)
        || marker.base_ref.is_empty()
        || marker.base_ref.starts_with('-')
        || marker.base_ref.chars().any(char::is_control)
        || !matches!(marker.base_commit.len(), 40 | 64)
        || !marker
            .base_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || marker.created_at_unix <= 0
    {
        return Err(WorkspaceOwnerError::Invalid(
            "marker has invalid creation identity",
        ));
    }
    Ok(())
}

/// Canonical form, spelled exactly as `ilium_git` and every saved pane spell
/// it. `std::fs::canonicalize` would return a `\\?\` extended-length path on
/// Windows, which never equals the saved (simplified) form and would make every
/// saved path look non-canonical.
async fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || ilium_platform::paths::canonicalize(&path))
        .await
        .map_err(|error| std::io::Error::other(format!("canonicalize task failed: {error}")))?
}

pub(crate) async fn validated_marker_path_for(
    repo_common_dir: &Path,
    worktree_root: &Path,
) -> Result<PathBuf, WorkspaceOwnerError> {
    let common_dir = canonical(repo_common_dir).await?;
    let root = canonical(worktree_root).await?;
    if common_dir != repo_common_dir || root != worktree_root {
        return Err(WorkspaceOwnerError::Invalid(
            "saved paths are not canonical",
        ));
    }
    let repository = ilium_git::discover(&root).await?;
    if repository.is_bare || repository.common_dir != common_dir || repository.worktree_root != root
    {
        return Err(WorkspaceOwnerError::Invalid("repository identity changed"));
    }
    let listed = ilium_git::list_worktrees(&root).await?;
    let mut registered = false;
    for worktree in listed
        .iter()
        .skip(1)
        .filter(|worktree| !worktree.is_bare && !worktree.is_prunable)
    {
        if canonical(&worktree.path).await.ok().as_deref() == Some(root.as_path()) {
            registered = true;
            break;
        }
    }
    if !registered {
        return Err(WorkspaceOwnerError::Invalid(
            "not a registered linked worktree",
        ));
    }
    let head_path = ilium_git::head_paths(&root).await?.head;
    if head_path.file_name().is_none_or(|name| name != "HEAD") {
        return Err(WorkspaceOwnerError::Invalid("Git HEAD path is unexpected"));
    }
    let head_metadata = fs::symlink_metadata(&head_path).await?;
    if !head_metadata.file_type().is_file() {
        return Err(WorkspaceOwnerError::Invalid(
            "Git HEAD is not a regular file",
        ));
    }
    let metadata_dir = canonical(
        head_path
            .parent()
            .ok_or(WorkspaceOwnerError::Invalid("Git HEAD has no parent"))?,
    )
    .await?;
    let worktrees_dir = canonical(&common_dir.join("worktrees")).await?;
    if worktrees_dir.parent() != Some(common_dir.as_path())
        || metadata_dir.parent() != Some(worktrees_dir.as_path())
    {
        return Err(WorkspaceOwnerError::Invalid(
            "Git metadata is outside worktrees directory",
        ));
    }
    Ok(metadata_dir.join(MARKER_FILE))
}

async fn read_marker_bytes(path: &Path) -> Result<Vec<u8>, WorkspaceOwnerError> {
    read_marker_bytes_if_present(path)
        .await?
        .ok_or_else(|| WorkspaceOwnerError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))
}

async fn read_marker_bytes_if_present(path: &Path) -> Result<Option<Vec<u8>>, WorkspaceOwnerError> {
    let path = path.to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("marker has no parent"))?;
        let directory = NoFollowDirectory::open_root(parent)?;
        let file = match directory.open_regular(MARKER_FILE.as_ref()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if metadata.len() > MAX_MARKER_BYTES {
            return Err(std::io::Error::other("marker is too large"));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_MARKER_BYTES + 1).read_to_end(&mut bytes)?;
        Ok::<Option<Vec<u8>>, std::io::Error>(Some(bytes))
    })
    .await
    .map_err(|error| std::io::Error::other(format!("marker read task failed: {error}")))??;
    if bytes
        .as_ref()
        .is_some_and(|bytes| bytes.len() as u64 > MAX_MARKER_BYTES)
    {
        return Err(WorkspaceOwnerError::Invalid("marker is too large"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as std_fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(directory: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git is installed");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn linked_workspace() -> (TempDir, PaneWorkspace) {
        let temp = TempDir::new().expect("temp directory");
        let main = temp.path().join("main");
        std_fs::create_dir(&main).expect("main directory");
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.name", "Ilium Test"]);
        git(&main, &["config", "user.email", "ilium@example.invalid"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "initial"]);
        let base_commit = ilium_git::resolve_commit(&main, "main")
            .await
            .expect("base commit");
        let linked = temp.path().join("linked");
        ilium_git::create_worktree(&main, &linked, "agent/test", "main")
            .await
            .expect("linked worktree");
        let repo = ilium_git::discover(&linked).await.expect("discover linked");
        let workspace = PaneWorkspace {
            workspace_id: None,
            repo_common_dir: repo.common_dir,
            worktree_root: repo.worktree_root,
            branch: "agent/test".into(),
            base_ref: "main".into(),
            base_commit,
            created_by_ilium: true,
            created_at_unix: 1_700_000_000,
        };
        (temp, workspace)
    }

    #[tokio::test]
    async fn creates_and_verifies_exact_registered_marker() {
        let (_temp, mut workspace) = linked_workspace().await;
        let marker = create_marker(&mut workspace).await.expect("create marker");
        assert_eq!(marker.base_ref, workspace.base_ref);
        assert_eq!(marker.base_commit, workspace.base_commit);
        assert_eq!(
            workspace.workspace_id.as_deref(),
            Some(marker.workspace_id.as_str())
        );
        assert_eq!(
            verify_marker(&workspace).await.expect("verify marker"),
            marker
        );
        assert_eq!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root)
                .await
                .expect("read retained marker"),
            Some(marker.clone())
        );
        let marker_path = validated_marker_path(&workspace)
            .await
            .expect("marker path");
        assert!(marker_path.is_file());
        assert!(workspace.worktree_root.exists());
    }

    #[tokio::test]
    async fn registered_foreign_worktree_without_marker_is_absent() {
        let (_temp, workspace) = linked_workspace().await;
        assert_eq!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root)
                .await
                .expect("absent marker"),
            None
        );
    }

    #[tokio::test]
    async fn retained_read_rejects_tampered_identity_and_old_marker_schema() {
        let (_temp, mut workspace) = linked_workspace().await;
        let marker = create_marker(&mut workspace).await.expect("create marker");
        let marker_path = validated_marker_path(&workspace)
            .await
            .expect("marker path");

        let mut wrong_root = marker.clone();
        wrong_root.worktree_root = workspace.repo_common_dir.clone();
        std_fs::write(&marker_path, serde_json::to_vec(&wrong_root).unwrap())
            .expect("write wrong-root marker");
        assert!(matches!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root).await,
            Err(WorkspaceOwnerError::Mismatch)
        ));

        let mut invalid_base = marker.clone();
        invalid_base.base_commit = "not-a-commit".into();
        std_fs::write(&marker_path, serde_json::to_vec(&invalid_base).unwrap())
            .expect("write invalid-base marker");
        assert!(matches!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root).await,
            Err(WorkspaceOwnerError::Invalid(_))
        ));

        let mut pre_custody = serde_json::to_value(&marker).unwrap();
        pre_custody
            .as_object_mut()
            .unwrap()
            .remove("custody_revision");
        std_fs::write(&marker_path, serde_json::to_vec(&pre_custody).unwrap())
            .expect("write pre-custody marker");
        assert!(matches!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root).await,
            Err(WorkspaceOwnerError::Invalid(_))
        ));

        let mut old_schema = serde_json::to_value(marker).unwrap();
        old_schema.as_object_mut().unwrap().remove("base_ref");
        old_schema.as_object_mut().unwrap().remove("base_commit");
        std_fs::write(&marker_path, serde_json::to_vec(&old_schema).unwrap())
            .expect("write pre-provenance marker");
        assert!(matches!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root).await,
            Err(WorkspaceOwnerError::Invalid(_))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn retained_read_rejects_symlink_marker() {
        use std::os::unix::fs::symlink;

        let (_temp, workspace) = linked_workspace().await;
        let marker_path = validated_marker_path(&workspace)
            .await
            .expect("marker path");
        let target = workspace.worktree_root.join("foreign-marker.json");
        std_fs::write(&target, b"foreign content").expect("write foreign content");
        symlink(&target, &marker_path).expect("link marker");
        assert!(
            read_registered_marker(&workspace.repo_common_dir, &workspace.worktree_root)
                .await
                .is_err()
        );
        assert_eq!(
            std_fs::read(&target).expect("foreign remains"),
            b"foreign content"
        );
    }

    #[tokio::test]
    async fn collision_and_tampering_never_remove_foreign_content() {
        let (_temp, mut workspace) = linked_workspace().await;
        create_marker(&mut workspace).await.expect("create marker");
        let marker_path = validated_marker_path(&workspace)
            .await
            .expect("marker path");
        let mut retry = workspace.clone();
        retry.workspace_id = None;
        assert!(matches!(
            create_marker(&mut retry).await,
            Err(WorkspaceOwnerError::AlreadyExists)
        ));
        assert!(retry.workspace_id.is_none());
        std_fs::write(&marker_path, b"foreign content").expect("tamper marker");
        assert!(matches!(
            verify_marker(&workspace).await,
            Err(WorkspaceOwnerError::Mismatch)
        ));
        assert_eq!(
            std_fs::read(&marker_path).expect("marker remains"),
            b"foreign content"
        );
    }

    #[tokio::test]
    async fn rejects_main_checkout_and_wrong_pane_identity() {
        let (_temp, mut workspace) = linked_workspace().await;
        let marker = create_marker(&mut workspace).await.expect("create marker");
        let mut wrong = workspace.clone();
        wrong.branch = "other".into();
        assert!(matches!(
            verify_marker(&wrong).await,
            Err(WorkspaceOwnerError::Mismatch)
        ));
        let main = workspace
            .worktree_root
            .parent()
            .expect("temp root")
            .join("main");
        let main_repo = ilium_git::discover(&main).await.expect("discover main");
        wrong.worktree_root = main_repo.worktree_root;
        assert!(matches!(
            verify_marker(&wrong).await,
            Err(WorkspaceOwnerError::Invalid(_))
        ));
        assert_eq!(
            verify_marker(&workspace).await.expect("original marker"),
            marker
        );
    }

    #[tokio::test]
    async fn live_branch_switch_does_not_change_creation_ownership() {
        let (_temp, mut workspace) = linked_workspace().await;
        let marker = create_marker(&mut workspace).await.expect("create marker");
        git(
            &workspace.worktree_root,
            &["switch", "-q", "-c", "alternate"],
        );
        assert_eq!(
            verify_marker(&workspace).await.expect("verify marker"),
            marker
        );
    }
}
