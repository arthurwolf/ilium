//! Argument vectors and pure gates for the Git operations the worktrees plugin issues.
//!
//! Each builder returns the exact `git` arguments (without the program name) the kernel
//! `ilium-git` crate used, after the same validation. The `process` host import (gap G10) runs
//! them; every decision that only needs captured output lives here so it can be tested.

use super::{GitError, GitVersion, Worktree};

/// True for a full SHA-1 (40) or SHA-256 (64) hexadecimal object id.
pub fn is_full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Shared refs may be recorded as a removal base; everything else is pinned to an object id.
pub fn is_shared_removal_ref(reference: &str) -> bool {
    (reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/"))
        && !reference.chars().any(char::is_control)
        && !reference.contains(['~', '^', ':', '@', '*', '?', '[', '\\'])
}

/// Git older than 2.17 cannot list or manage worktrees reliably.
pub fn supports_worktrees(version: GitVersion) -> bool {
    version.major > 2 || (version.major == 2 && version.minor >= 17)
}

/// `worktree list -z` exists from Git 2.36.
pub fn supports_nul_worktree_list(version: GitVersion) -> bool {
    version.major > 2 || version.minor >= 36
}

fn owned(arguments: &[&str]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

fn checked_branch(branch: &str) -> Result<(), GitError> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err(GitError::InvalidInput("invalid branch name".into()));
    }
    Ok(())
}

/// `git --version`.
pub fn version() -> Vec<String> {
    owned(&["--version"])
}

/// Arguments that make Git validate a branch name; run before any command that uses it.
pub fn check_branch_name(branch: &str) -> Result<Vec<String>, GitError> {
    checked_branch(branch)?;
    Ok(owned(&["check-ref-format", "--branch", branch]))
}

/// `worktree list`, NUL separated when the installed Git supports it.
pub fn list_worktrees(version: GitVersion) -> Vec<String> {
    if supports_nul_worktree_list(version) {
        owned(&["worktree", "list", "--porcelain", "-z"])
    } else {
        owned(&["worktree", "list", "--porcelain"])
    }
}

/// `git for-each-ref` over local branches.
pub fn list_branches() -> Vec<String> {
    owned(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
}

/// Status with branch headers, NUL separated.
pub fn status() -> Vec<String> {
    owned(&["status", "--porcelain=v2", "--branch", "-z"])
}

/// Status that also lists ignored files, used to prove a checkout is pristine.
pub fn status_including_ignored() -> Vec<String> {
    owned(&[
        "status",
        "--porcelain=v1",
        "--ignored",
        "--ignore-submodules=none",
        "--untracked-files=all",
        "-z",
    ])
}

/// Branch creation plus checkout in a new linked worktree.
pub fn create_worktree(path: &str, branch: &str, base_ref: &str) -> Result<Vec<String>, GitError> {
    if !path.starts_with('/') {
        return Err(GitError::InvalidInput(
            "worktree path must be absolute and not already exist".into(),
        ));
    }
    checked_branch(branch)?;
    if base_ref.is_empty() || base_ref.starts_with('-') {
        return Err(GitError::InvalidInput("invalid base ref".into()));
    }
    Ok(owned(&[
        "worktree", "add", "-b", branch, "--", path, base_ref,
    ]))
}

/// Resolve a base to a full commit id.
pub fn resolve_commit(base_ref: &str) -> Result<Vec<String>, GitError> {
    if base_ref.is_empty() || base_ref.starts_with('-') {
        return Err(GitError::InvalidInput("invalid base ref".into()));
    }
    Ok(owned(&[
        "rev-parse",
        "--verify",
        &format!("{base_ref}^{{commit}}"),
    ]))
}

/// Tip of a local branch; exit status 1 means the branch is absent.
pub fn branch_tip(branch: &str) -> Result<Vec<String>, GitError> {
    checked_branch(branch)?;
    Ok(owned(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]))
}

/// Compare-and-delete of a branch this request created.
pub fn delete_branch_if_tip(branch: &str, expected_tip: &str) -> Result<Vec<String>, GitError> {
    checked_branch(branch)?;
    if !is_full_object_id(expected_tip) {
        return Err(GitError::InvalidInput("invalid expected branch tip".into()));
    }
    Ok(owned(&[
        "update-ref",
        "-d",
        &format!("refs/heads/{branch}"),
        expected_tip,
    ]))
}

/// Safe `branch -d`; Git refuses an unmerged branch.
pub fn delete_branch_if_merged(branch: &str) -> Result<Vec<String>, GitError> {
    checked_branch(branch)?;
    Ok(owned(&["branch", "-d", "--", branch]))
}

/// Destructive `branch -D`; the caller requires an explicit path-named confirmation first.
pub fn delete_branch_force(branch: &str) -> Result<Vec<String>, GitError> {
    checked_branch(branch)?;
    Ok(owned(&["branch", "-D", "--", branch]))
}

/// Ancestry between two full commit ids; exit status 1 means "not an ancestor".
pub fn commit_is_ancestor(ancestor: &str, descendant: &str) -> Result<Vec<String>, GitError> {
    if !is_full_object_id(ancestor) || !is_full_object_id(descendant) {
        return Err(GitError::InvalidInput(
            "ancestry requires full commit OIDs".into(),
        ));
    }
    Ok(owned(&[
        "merge-base",
        "--is-ancestor",
        ancestor,
        descendant,
    ]))
}

/// One exact direct ref with its object id, symbolic target and upstream (NUL separated).
pub fn direct_reference(reference: &str) -> Result<Vec<String>, GitError> {
    if !is_shared_removal_ref(reference) {
        return Err(GitError::InvalidInput(
            "removal requires a fully qualified shared ref".into(),
        ));
    }
    Ok(owned(&[
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(symref)%00%(upstream)",
        "--",
        reference,
    ]))
}

/// Parses the output of [`direct_reference`] into the object id and optional upstream.
pub fn parse_direct_reference(
    reference: &str,
    output: &str,
) -> Result<(String, Option<String>), GitError> {
    let text = output.trim_end_matches(['\r', '\n']);
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

/// Decides whether `path` may be removed as a linked worktree and builds the command.
///
/// `listed` is the parsed worktree list (first entry is the main checkout) and `path` and
/// `current_root` are already canonical. The main checkout, the checkout hosting the request,
/// and any locked, prunable, bare or unlisted path are refused.
pub fn remove_worktree(
    listed: &[Worktree],
    path: &str,
    current_root: &str,
    force: bool,
) -> Result<Vec<String>, GitError> {
    let main = listed
        .first()
        .ok_or_else(|| GitError::Parse("repository has no listed worktree".into()))?;
    let is_listed_linked = listed.iter().skip(1).any(|worktree| {
        !worktree.is_bare && !worktree.is_prunable && !worktree.is_locked && worktree.path == path
    });
    if path == current_root || path == main.path || !is_listed_linked {
        return Err(GitError::InvalidInput(
            "path is not a listed linked worktree".into(),
        ));
    }
    let mut arguments = owned(&["worktree", "remove"]);
    if force {
        arguments.push("--force".into());
    }
    arguments.push("--".into());
    arguments.push(path.to_owned());
    Ok(arguments)
}

/// Facts gathered by the caller to decide whether a creation branch may be removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalFacts {
    /// Current tip of the branch.
    pub tip: String,
    /// Tip the user confirmed.
    pub expected_tip: String,
    /// Commit the branch was created at.
    pub creation_commit: String,
    /// Tip of the locally recorded upstream, when the branch has one.
    pub upstream_tip: Option<String>,
    /// Whether `tip` descends from `upstream_tip`.
    pub contained_in_upstream: bool,
    /// Whether `tip` is an ancestor of the stable base.
    pub merged_into_base: bool,
}

/// The pure part of `verify_removal_branch`: the gate on already gathered facts.
pub fn verify_removal(facts: &RemovalFacts, has_upstream: bool) -> Result<(), GitError> {
    if facts.tip != facts.expected_tip {
        return Err(GitError::InvalidInput(
            "creation branch moved since confirmation".into(),
        ));
    }
    if has_upstream {
        if !facts.contained_in_upstream {
            return Err(GitError::InvalidInput(
                "branch has commits absent from its locally recorded upstream".into(),
            ));
        }
    } else if facts.tip != facts.creation_commit {
        return Err(GitError::InvalidInput(
            "branch has commits and no upstream; keep the branch or configure and refresh its upstream"
                .into(),
        ));
    }
    if !facts.merged_into_base {
        return Err(GitError::InvalidInput(
            "branch is not merged into the removal safety base".into(),
        ));
    }
    Ok(())
}

/// Pure check over `ls-files -v -z` output: refuses hidden skip-worktree/assume-unchanged edits.
pub fn verify_index_flags(output: &[u8]) -> Result<(), GitError> {
    if output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .any(|record| record[0] == b'S' || record[0].is_ascii_lowercase())
    {
        return Err(GitError::InvalidInput(
            "index has skip-worktree or assume-unchanged entries; inspect them before removal"
                .into(),
        ));
    }
    Ok(())
}

/// Pure check over `ls-files --stage -z` output: refuses submodule (gitlink) entries.
pub fn verify_index_stages(output: &[u8]) -> Result<(), GitError> {
    if output
        .split(|byte| *byte == 0)
        .any(|record| record.starts_with(b"160000 "))
    {
        return Err(GitError::InvalidInput(
            "submodule entries require separate history/data review; file discard does not authorize it".into(),
        ));
    }
    Ok(())
}

/// A checkout is pristine, ignored files included, only when the ignored-aware status is empty.
pub fn is_pristine_including_ignored(status_output: &[u8]) -> bool {
    status_output.is_empty()
}
