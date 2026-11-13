//! Pure Git layer of the worktrees plugin, moved from the `ilium-git` crate.
//!
//! Nothing here spawns `git`: [`parse`] turns captured output into plain data and [`commands`]
//! builds the argument vectors and the decisions that depend only on that data. Running the
//! commands is the `process` host import (gap G10).

pub mod commands;
pub mod parse;

/// Why a Git operation or a parse failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// Git exited unsuccessfully.
    Command { exit: Option<i32>, stderr: String },
    /// Output did not have the documented shape.
    Parse(String),
    /// An argument was refused before any process started.
    InvalidInput(String),
    /// The installed Git is older than 2.17.
    UnsupportedVersion(GitVersion),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command { exit, stderr } => {
                write!(formatter, "git exited with {exit:?}: {stderr}")
            }
            Self::Parse(reason) => write!(formatter, "invalid git output: {reason}"),
            Self::InvalidInput(reason) => write!(formatter, "invalid git operation: {reason}"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "git {version} is too old; worktrees require 2.17 or newer"
                )
            }
        }
    }
}

impl std::error::Error for GitError {}

/// The installed Git version.
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

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}

/// Counts from `git status --porcelain=v2 --branch -z`.
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
    /// True when nothing is staged, modified, untracked or conflicted.
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.modified == 0 && self.untracked == 0 && self.conflicted == 0
    }
}
