//! Bounded, argv-only Git operations for Ilium's agent workspaces.
//!
//! This adapter owns process execution and Git's wire formats. It never
//! chooses a pane, stores ownership, or decides whether user work may be
//! removed; those decisions belong to the server coordinator.

pub mod parse;
mod runner;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::OnceCell;

static GIT_VERSION: OnceCell<GitVersion> = OnceCell::const_new();

#[derive(Debug, Error)]
pub enum GitError {
    #[error("could not spawn git: {0}")]
    Spawn(std::io::Error),
    #[error("git child has no process ID")]
    MissingProcessId,
    #[error("git child has no {0} pipe")]
    MissingPipe(&'static str),
    #[error("could not guard git process tree: {0}")]
    ProcessGuard(std::io::Error),
    #[error("git command exceeded {0:?}")]
    Timeout(Duration),
    #[error("git output exceeded its byte limit")]
    OutputTooLarge,
    #[error("could not read git output: {0}")]
    Output(std::io::Error),
    #[error("could not wait for git: {0}")]
    Wait(std::io::Error),
    #[error("git exited with {exit:?}: {stderr}")]
    Command { exit: Option<i32>, stderr: String },
    #[error("invalid git output: {0}")]
    Parse(String),
    #[error("git {0} is too old; worktrees require 2.17 or newer")]
    UnsupportedVersion(GitVersion),
    #[error("invalid git operation: {0}")]
    InvalidInput(String),
    #[error("could not resolve repository path: {0}")]
    Path(std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Canonical --git-common-dir; stable across every linked worktree.
    pub common_dir: PathBuf,
    /// Canonical root of the current checkout.
    pub worktree_root: PathBuf,
    /// Directory within the checkout from which Ilium was launched.
    pub project_subpath: PathBuf,
    pub is_bare: bool,
    pub git_version: GitVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub detached: bool,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub conflicted: u32,
    pub upstream: Option<String>,
}

impl GitStatus {
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.modified == 0 && self.untracked == 0 && self.conflicted == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadProbe {
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchFacts {
    pub head: HeadProbe,
    pub commit_id: String,
    pub last_commit_subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadPaths {
    pub head: PathBuf,
    pub head_log: PathBuf,
}

/// Checks the installed Git version once per process. Failed checks can be
/// retried when Git becomes available later.
pub async fn git_version() -> Result<GitVersion, GitError> {
    let version = GIT_VERSION
        .get_or_try_init(|| async {
            let output = runner::run(Path::new("."), &runner::args(&["--version"]), true).await?;
            parse::git_version(&output)
        })
        .await?;
    if *version
        < (GitVersion {
            major: 2,
            minor: 17,
            patch: 0,
        })
    {
        return Err(GitError::UnsupportedVersion(*version));
    }
    Ok(*version)
}

pub async fn discover(directory: &Path) -> Result<Repository, GitError> {
    let version = git_version().await?;
    let directory = ilium_platform::paths::canonicalize(directory).map_err(GitError::Path)?;
    let common_dir = single_line(
        runner::run(
            &directory,
            &runner::args(&["rev-parse", "--git-common-dir"]),
            true,
        )
        .await?,
    )?;
    let is_bare = single_line(
        runner::run(
            &directory,
            &runner::args(&["rev-parse", "--is-bare-repository"]),
            true,
        )
        .await?,
    )?;
    if is_bare != "true" && is_bare != "false" {
        return Err(GitError::Parse("invalid bare-repository flag".into()));
    }
    let common_dir = resolve_git_path(&directory, Path::new(&common_dir))?;
    let (worktree_root, project_subpath) = if is_bare == "true" {
        (directory.clone(), PathBuf::new())
    } else {
        let root = single_line(
            runner::run(
                &directory,
                &runner::args(&["rev-parse", "--show-toplevel"]),
                true,
            )
            .await?,
        )?;
        let root = resolve_git_path(&directory, Path::new(&root))?;
        let subpath = directory
            .strip_prefix(&root)
            .map_err(|_| GitError::Parse("project path is outside Git worktree".into()))?
            .to_path_buf();
        (root, subpath)
    };
    Ok(Repository {
        common_dir,
        worktree_root,
        project_subpath,
        is_bare: is_bare == "true",
        git_version: version,
    })
}

/// The current branch by default; fall back to the remote default when the
/// current checkout has detached HEAD.
pub async fn default_base_ref(directory: &Path) -> Result<String, GitError> {
    let current = runner::run(
        directory,
        &runner::args(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
        true,
    )
    .await;
    match current {
        Ok(output) => return single_line(output),
        Err(GitError::Command { exit: Some(1), .. }) => {}
        Err(error) => return Err(error),
    }
    let remote = runner::run(
        directory,
        &runner::args(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ]),
        true,
    )
    .await?;
    single_line(remote)
}

pub async fn list_branches(directory: &Path) -> Result<Vec<String>, GitError> {
    let output = runner::run(
        directory,
        &runner::args(&["for-each-ref", "--format=%(refname:short)", "refs/heads"]),
        true,
    )
    .await?;
    parse::branches(&output)
}

pub async fn list_worktrees(directory: &Path) -> Result<Vec<Worktree>, GitError> {
    let version = git_version().await?;
    if version.major > 2 || version.minor >= 36 {
        let output = runner::run(
            directory,
            &runner::args(&["worktree", "list", "--porcelain", "-z"]),
            true,
        )
        .await?;
        return parse::worktrees(&output);
    }

    let output = runner::run(
        directory,
        &runner::args(&["worktree", "list", "--porcelain"]),
        true,
    )
    .await?;
    let worktrees = parse::worktrees_lines(&output)?;
    let expected_common_dir = discover(directory).await?.common_dir;
    for worktree in &worktrees {
        if worktree.is_prunable && !worktree.path.exists() {
            continue;
        }
        let actual_common_dir = single_line(
            runner::run(
                &worktree.path,
                &runner::args(&["rev-parse", "--git-common-dir"]),
                true,
            )
            .await?,
        )?;
        let actual_common_dir = resolve_git_path(&worktree.path, Path::new(&actual_common_dir))?;
        if actual_common_dir != expected_common_dir {
            return Err(GitError::Parse(
                "ambiguous worktree path in newline-delimited Git output".into(),
            ));
        }
    }
    Ok(worktrees)
}

/// Files to stat for the cheap status tier. Git resolves linked-worktree
/// metadata names itself; they need not equal the worktree directory name.
pub async fn head_paths(directory: &Path) -> Result<HeadPaths, GitError> {
    let head = single_line(
        runner::run(
            directory,
            &runner::args(&["rev-parse", "--git-path", "HEAD"]),
            true,
        )
        .await?,
    )?;
    let head_log = single_line(
        runner::run(
            directory,
            &runner::args(&["rev-parse", "--git-path", "logs/HEAD"]),
            true,
        )
        .await?,
    )?;
    Ok(HeadPaths {
        head: absolute_git_path(directory, Path::new(&head))?,
        head_log: absolute_git_path(directory, Path::new(&head_log))?,
    })
}

/// Creates a new branch and linked worktree. The caller serializes mutations
/// per common Git directory and owns rollback if later pane creation fails.
pub async fn create_worktree(
    directory: &Path,
    path: &Path,
    branch: &str,
    base_ref: &str,
) -> Result<(), GitError> {
    if !path.is_absolute() || path.exists() {
        return Err(GitError::InvalidInput(
            "worktree path must be absolute and not already exist".into(),
        ));
    }
    validate_branch(directory, branch).await?;
    if base_ref.is_empty() || base_ref.starts_with('-') {
        return Err(GitError::InvalidInput("invalid base ref".into()));
    }
    let arguments = vec![
        OsString::from("worktree"),
        OsString::from("add"),
        OsString::from("-b"),
        OsString::from(branch),
        OsString::from("--"),
        path.as_os_str().to_owned(),
        OsString::from(base_ref),
    ];
    runner::run(directory, &arguments, false).await?;
    Ok(())
}

/// Resolves a selected base to a complete commit ID before creating a
/// worktree. The server records this immutable fact in the pane snapshot.
pub async fn resolve_commit(directory: &Path, base_ref: &str) -> Result<String, GitError> {
    if base_ref.is_empty() || base_ref.starts_with('-') {
        return Err(GitError::InvalidInput("invalid base ref".into()));
    }
    let revision = format!("{base_ref}^{{commit}}");
    let commit = single_line(
        runner::run(
            directory,
            &[
                OsString::from("rev-parse"),
                OsString::from("--verify"),
                OsString::from(revision),
            ],
            true,
        )
        .await?,
    )?;
    if !is_full_object_id(&commit) {
        return Err(GitError::Parse(
            "base did not resolve to a full commit ID".into(),
        ));
    }
    Ok(commit)
}

/// Returns the current tip of a local branch, or `None` when it is absent.
pub async fn branch_tip(directory: &Path, branch: &str) -> Result<Option<String>, GitError> {
    validate_branch(directory, branch).await?;
    let reference = format!("refs/heads/{branch}");
    let output = runner::run(
        directory,
        &[
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(reference),
        ],
        true,
    )
    .await;
    match output {
        Ok(output) => {
            let commit = single_line(output)?;
            if !is_full_object_id(&commit) {
                return Err(GitError::Parse("branch tip is not a full object ID".into()));
            }
            Ok(Some(commit))
        }
        Err(GitError::Command { exit: Some(1), .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Compare-and-delete for rollback of a branch created by this request.
/// The caller must first prove creation ownership and that no worktree still
/// uses the branch. A moved tip remains untouched.
pub async fn delete_branch_if_tip(
    directory: &Path,
    branch: &str,
    expected_tip: &str,
) -> Result<(), GitError> {
    validate_branch(directory, branch).await?;
    if !is_full_object_id(expected_tip) {
        return Err(GitError::InvalidInput("invalid expected branch tip".into()));
    }
    runner::run(
        directory,
        &[
            OsString::from("update-ref"),
            OsString::from("-d"),
            OsString::from(format!("refs/heads/{branch}")),
            OsString::from(expected_tip),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Removes one listed linked worktree. The coordinator must first prove the
/// owner, stop every pane process, and apply its dirty/unpushed/merged gates.
pub async fn remove_worktree(directory: &Path, path: &Path, force: bool) -> Result<(), GitError> {
    let repo = discover(directory).await?;
    let requested_path = path;
    let path = ilium_platform::paths::canonicalize(requested_path).map_err(GitError::Path)?;
    if path != requested_path {
        return Err(GitError::InvalidInput(
            "worktree path changed its canonical identity".into(),
        ));
    }
    let worktrees = list_worktrees(directory).await?;
    let main = worktrees
        .first()
        .ok_or_else(|| GitError::Parse("repository has no listed worktree".into()))?;
    let main_path = ilium_platform::paths::canonicalize(&main.path).map_err(GitError::Path)?;
    let is_listed_linked = worktrees.iter().skip(1).any(|worktree| {
        !worktree.is_bare
            && !worktree.is_prunable
            && !worktree.is_locked
            && ilium_platform::paths::canonicalize(&worktree.path)
                .is_ok_and(|listed_path| listed_path == path)
    });
    if path == repo.worktree_root || path == main_path || !is_listed_linked {
        return Err(GitError::InvalidInput(
            "path is not a listed linked worktree".into(),
        ));
    }
    let mut arguments = runner::args(&["worktree", "remove"]);
    if force {
        arguments.push(OsString::from("--force"));
    }
    arguments.push(OsString::from("--"));
    arguments.push(path.into_os_string());
    runner::run(directory, &arguments, false).await?;
    Ok(())
}

/// Git's safe `branch -d`; refuses an unmerged branch.
pub async fn delete_branch_if_merged(directory: &Path, branch: &str) -> Result<(), GitError> {
    validate_branch(directory, branch).await?;
    runner::run(
        directory,
        &[
            OsString::from("branch"),
            OsString::from("-d"),
            OsString::from("--"),
            OsString::from(branch),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Destructive branch removal; callers must require a path-named explicit
/// confirmation and verify the branch is no longer checked out anywhere.
pub async fn delete_branch_force(directory: &Path, branch: &str) -> Result<(), GitError> {
    validate_branch(directory, branch).await?;
    runner::run(
        directory,
        &[
            OsString::from("branch"),
            OsString::from("-D"),
            OsString::from("--"),
            OsString::from(branch),
        ],
        false,
    )
    .await?;
    Ok(())
}

/// Git's ancestry check against the selected base as it exists now. This is
/// independent of whether the branch has been pushed to an upstream.
pub async fn branch_is_merged_into(
    directory: &Path,
    branch: &str,
    base_ref: &str,
) -> Result<bool, GitError> {
    validate_branch(directory, branch).await?;
    let base_commit = resolve_commit(directory, base_ref).await?;
    let reference = format!("refs/heads/{branch}");
    match runner::run(
        directory,
        &[
            OsString::from("merge-base"),
            OsString::from("--is-ancestor"),
            OsString::from(reference),
            OsString::from(base_commit),
        ],
        true,
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(GitError::Command { exit: Some(1), .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Resolve a creation operand once, recording a shared, direct ref where possible.
/// Expressions and checkout-relative operands that cannot be qualified become an OID.
/// Existing ownership markers are never rewritten by this normalization.
pub async fn qualified_creation_base(
    directory: &Path,
    operand: &str,
) -> Result<(String, String), GitError> {
    let commit = resolve_commit(directory, operand).await?;
    let output = runner::run(
        directory,
        &runner::args(&["rev-parse", "--symbolic-full-name", "--verify", operand]),
        true,
    )
    .await?;
    let candidate = parse::utf8(&output)?.trim_end_matches(['\r', '\n']);
    if !is_shared_removal_ref(candidate) {
        return Ok((commit.clone(), commit));
    }
    let reference = direct_reference(directory, candidate).await?;
    if reference.0 != commit {
        return Err(GitError::InvalidInput(
            "base moved while its creation identity was captured".into(),
        ));
    }
    Ok((candidate.to_owned(), commit))
}
/// Only these namespaces have repository-shared, non-checkout-relative semantics.
fn is_shared_removal_ref(reference: &str) -> bool {
    (reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/"))
        && !reference.chars().any(char::is_control)
        && !reference.contains(['~', '^', ':', '@', '*', '?', '[', '\\'])
}
/// Returns one exact direct ref's OID and full upstream name; no DWIM or symrefs.
async fn direct_reference(
    directory: &Path,
    reference: &str,
) -> Result<(String, Option<String>), GitError> {
    if !is_shared_removal_ref(reference) {
        return Err(GitError::InvalidInput(
            "removal requires a fully qualified shared ref".into(),
        ));
    }
    runner::run(
        directory,
        &runner::args(&["check-ref-format", reference]),
        true,
    )
    .await?;
    let output = runner::run(
        directory,
        &runner::args(&[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)%00%(upstream)",
            "--",
            reference,
        ]),
        true,
    )
    .await?;
    let text = parse::utf8(&output)?.trim_end_matches(['\r', '\n']);
    let fields = text.split('\0').collect::<Vec<_>>();
    if fields.len() != 4
        || fields[0] != reference
        || !is_full_object_id(fields[1])
        || !fields[2].is_empty()
        || text.contains(['\r', '\n'])
    {
        return Err(GitError::InvalidInput(
            "exact direct ref is missing, symbolic, or ambiguous".into(),
        ));
    }
    let upstream = (!fields[3].is_empty()).then(|| fields[3].to_owned());
    Ok((fields[1].to_owned(), upstream))
}
/// Pseudo refs, old short names and arbitrary expressions use the recorded OID.
/// This conservative legacy fallback never reinterprets HEAD in the agent checkout.
pub async fn removal_base(
    directory: &Path,
    base_ref: &str,
    creation_commit: &str,
) -> Result<(String, String), GitError> {
    if !is_full_object_id(creation_commit) {
        return Err(GitError::InvalidInput(
            "invalid recorded creation commit".into(),
        ));
    }
    let creation = resolve_commit(directory, creation_commit).await?;
    if !is_shared_removal_ref(base_ref) {
        return Ok((format!("creation commit {creation}"), creation));
    }
    let (tip, _) = direct_reference(directory, base_ref).await?;
    if !commit_is_ancestor(directory, &creation, &tip).await? {
        return Err(GitError::InvalidInput(
            "recorded base ref no longer descends from its creation commit".into(),
        ));
    }
    Ok((base_ref.to_owned(), tip))
}
/// Both operands are full immutable OIDs; branch movement cannot change this query.
pub async fn commit_is_ancestor(
    directory: &Path,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, GitError> {
    if !is_full_object_id(ancestor) || !is_full_object_id(descendant) {
        return Err(GitError::InvalidInput(
            "ancestry requires full commit OIDs".into(),
        ));
    }
    match runner::run(
        directory,
        &runner::args(&["merge-base", "--is-ancestor", ancestor, descendant]),
        true,
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(GitError::Command { exit: Some(1), .. }) => Ok(false),
        Err(error) => Err(error),
    }
}
/// Require the unchanged branch to be locally published and merged into a stable base.
/// No fetch, remote contact, or claim about the remote server's current contents occurs.
pub async fn verify_removal_branch(
    directory: &Path,
    branch: &str,
    expected_tip: &str,
    base_ref: &str,
    creation_commit: &str,
) -> Result<String, GitError> {
    validate_branch(directory, branch).await?;
    let reference = format!("refs/heads/{branch}");
    let (tip, upstream) = direct_reference(directory, &reference).await?;
    if tip != expected_tip {
        return Err(GitError::InvalidInput(
            "creation branch moved since confirmation".into(),
        ));
    }
    match upstream {
        Some(upstream) => {
            let (upstream_tip, _) = direct_reference(directory, &upstream).await?;
            if !commit_is_ancestor(directory, &tip, &upstream_tip).await? {
                return Err(GitError::InvalidInput(
                    "branch has commits absent from its locally recorded upstream".into(),
                ));
            }
        }
        None if tip != creation_commit => {
            return Err(GitError::InvalidInput("branch has commits and no upstream; keep the branch or configure and refresh its upstream".into()));
        }
        None => {}
    }
    let (label, base_tip) = removal_base(directory, base_ref, creation_commit).await?;
    if !commit_is_ancestor(directory, &tip, &base_tip).await? {
        return Err(GitError::InvalidInput(
            "branch is not merged into the removal safety base".into(),
        ));
    }
    Ok(label)
}
/// Status can hide assume-unchanged/skip-worktree edits; reject those indexes.
/// Submodule histories are not covered by a superproject file-discard confirmation.
pub async fn verify_removal_index(directory: &Path) -> Result<(), GitError> {
    let flags = runner::run(directory, &runner::args(&["ls-files", "-v", "-z"]), true).await?;
    if flags
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .any(|record| record[0] == b'S' || record[0].is_ascii_lowercase())
    {
        return Err(GitError::InvalidInput(
            "index has skip-worktree or assume-unchanged entries; inspect them before removal"
                .into(),
        ));
    }
    let stages = runner::run(
        directory,
        &runner::args(&["ls-files", "--stage", "-z"]),
        true,
    )
    .await?;
    if stages
        .split(|byte| *byte == 0)
        .any(|record| record.starts_with(b"160000 "))
    {
        return Err(GitError::InvalidInput("submodule entries require separate history/data review; file discard does not authorize it".into()));
    }
    Ok(())
}

/// Safe ancestry/publication policy plus atomic expected-OID deletion, never branch -D.
/// Branch-specific config is deliberately left untouched; it has no expected-value CAS.
pub async fn delete_removal_branch(
    directory: &Path,
    branch: &str,
    expected_tip: &str,
    base_ref: &str,
    creation_commit: &str,
) -> Result<(), GitError> {
    verify_removal_branch(directory, branch, expected_tip, base_ref, creation_commit).await?;
    if list_worktrees(directory)
        .await?
        .iter()
        .any(|entry| entry.branch.as_deref() == Some(branch))
    {
        return Err(GitError::InvalidInput(
            "branch is still checked out in a registered worktree".into(),
        ));
    }
    let reference = format!("refs/heads/{branch}");
    runner::run(
        directory,
        &runner::args(&["update-ref", "--no-deref", "-d", &reference, expected_tip]),
        false,
    )
    .await?;
    Ok(())
}

pub async fn status(directory: &Path) -> Result<GitStatus, GitError> {
    let output = runner::run(
        directory,
        &runner::args(&["status", "--porcelain=v2", "--branch", "-z"]),
        true,
    )
    .await?;
    parse::status(&output)
}

/// Proves a newly created checkout has no staged, modified, untracked, or
/// ignored files before an automatic rollback. Ordinary porcelain status
/// hides ignored hook output, which must never be removed implicitly.
pub async fn is_worktree_pristine_including_ignored(directory: &Path) -> Result<bool, GitError> {
    let output = runner::run(
        directory,
        &runner::args(&[
            "status",
            "--porcelain=v1",
            "--ignored",
            "--ignore-submodules=none",
            "--untracked-files=all",
            "-z",
        ]),
        true,
    )
    .await?;
    Ok(output.is_empty())
}

pub async fn head_probe(directory: &Path) -> Result<HeadProbe, GitError> {
    let branch = single_line(
        runner::run(
            directory,
            &runner::args(&["rev-parse", "--abbrev-ref", "HEAD"]),
            true,
        )
        .await?,
    )?;
    let upstream_result = runner::run(
        directory,
        &runner::args(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]),
        true,
    )
    .await;
    let (upstream, ahead, behind) = match upstream_result {
        Ok(output) => {
            let upstream = single_line(output)?;
            let counts = single_line(
                runner::run(
                    directory,
                    &runner::args(&["rev-list", "--left-right", "--count", "@{u}...HEAD"]),
                    true,
                )
                .await?,
            )?;
            let mut counts = counts.split_whitespace();
            let behind = parse_count(counts.next(), "behind")?;
            let ahead = parse_count(counts.next(), "ahead")?;
            (Some(upstream), ahead, behind)
        }
        Err(GitError::Command {
            exit: Some(128), ..
        }) => (None, 0, 0),
        Err(error) => return Err(error),
    };
    let detached = branch == "HEAD";
    Ok(HeadProbe {
        branch: (!detached).then_some(branch),
        detached,
        upstream,
        ahead,
        behind,
    })
}

pub async fn branch_facts(directory: &Path) -> Result<BranchFacts, GitError> {
    let head = head_probe(directory).await?;
    let commit_id =
        single_line(runner::run(directory, &runner::args(&["rev-parse", "HEAD"]), true).await?)?;
    let last_commit_subject = single_line(
        runner::run(
            directory,
            &runner::args(&["log", "-1", "--format=%s"]),
            true,
        )
        .await?,
    )?;
    Ok(BranchFacts {
        head,
        commit_id,
        last_commit_subject,
    })
}

async fn validate_branch(directory: &Path, branch: &str) -> Result<(), GitError> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err(GitError::InvalidInput("invalid branch name".into()));
    }
    runner::run(
        directory,
        &runner::args(&["check-ref-format", "--branch", branch]),
        true,
    )
    .await?;
    Ok(())
}

fn resolve_git_path(directory: &Path, path: &Path) -> Result<PathBuf, GitError> {
    let absolute = absolute_git_path(directory, path)?;
    ilium_platform::paths::canonicalize(&absolute).map_err(GitError::Path)
}

fn absolute_git_path(directory: &Path, path: &Path) -> Result<PathBuf, GitError> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        directory.join(path)
    };
    std::path::absolute(path).map_err(GitError::Path)
}

fn single_line(output: Vec<u8>) -> Result<String, GitError> {
    let text = parse::utf8(&output)?.trim_end_matches(['\r', '\n']);
    if text.is_empty() || text.contains('\n') || text.contains('\r') {
        return Err(GitError::Parse("expected one nonempty line".into()));
    }
    Ok(text.to_owned())
}

fn parse_count(value: Option<&str>, name: &str) -> Result<u32, GitError> {
    value
        .ok_or_else(|| GitError::Parse(format!("missing {name} count")))?
        .parse()
        .map_err(|_| GitError::Parse(format!("invalid {name} count")))
}

fn is_full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
