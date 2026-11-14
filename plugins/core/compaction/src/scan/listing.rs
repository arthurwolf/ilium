//! Phase 1 of the scan: find every transcript file of one agent.
//!
//! Claude Code keeps `agent-sessions:claude` (`~/.claude/projects/**.jsonl`), including the
//! `<session>/subagents/**` and `workflows/**` transcripts of delegated work;
//! those are flagged as subagent files. Codex keeps
//! `<home>/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (and moves old ones to
//! `<home>/.codex/archived_sessions`); its subagent status is read from the
//! file by the parser, not from the path.
//!
//! The walk is iterative (no recursion depth to blow), never follows
//! symlinks, and is bounded in entries and path bytes like the cost-history
//! walker. Hitting a bound lists what was found and returns a warning instead
//! of failing the scan.

use std::sync::atomic::Ordering;

use crate::analysis::AgentKind;

use super::source::{ScanPath, TranscriptFs};
use super::ScanProgressCounters;

/// Upper bound on listed files; a larger corpus is truncated with a warning.
pub(super) const MAX_LISTED_FILES: usize = 400_000;
/// Upper bound on the bytes the listed paths occupy.
const MAX_PATH_BYTES: usize = 96 * 1024 * 1024;
/// Upper bound on directories visited.
const MAX_DIRECTORIES: usize = 1_000_000;

/// One transcript file found by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedFile {
    pub path: ScanPath,
    pub size: u64,
    pub mtime_ms: i64,
    /// Claude: under `subagents/` or `workflows/`. Codex: always false here.
    pub is_subagent: bool,
}

impl ListedFile {
    pub fn file_name(&self) -> String {
        self.path.file_name().to_string()
    }
}

/// What the walk produced.
#[derive(Debug, Default)]
pub(super) struct Listing {
    pub files: Vec<ListedFile>,
    pub bytes_total: u64,
    pub warnings: Vec<String>,
    /// The stop request arrived during the walk.
    pub was_stopped: bool,
}

/// The directories an agent's transcripts live in: the read-only scopes the host opens for the agent's session
/// folders (Claude `~/.claude/projects`, Codex `~/.codex/sessions` and `~/.codex/archived_sessions`).
fn roots(agent: AgentKind) -> Vec<ScanPath> {
    match agent {
        AgentKind::ClaudeCode => vec![ScanPath::new("agent-sessions:claude", "")],
        AgentKind::Codex => vec![
            ScanPath::new("agent-sessions:codex", ""),
            ScanPath::new("agent-config:codex", "archived_sessions"),
        ],
    }
}

fn is_transcript(agent: AgentKind, name: &str) -> bool {
    if !name.ends_with(".jsonl") {
        return false;
    }
    match agent {
        AgentKind::ClaudeCode => true,
        AgentKind::Codex => name.starts_with("rollout-"),
    }
}

/// Whether `relative` (a path below the projects root) is delegated work.
fn is_claude_subagent(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| matches!(part, "subagents" | "workflows"))
}

/// Lists the transcripts of `agent`. Missing directories are not an error (the agent may never have been used):
/// the listing is just empty.
pub(super) fn list_transcripts(
    agent: AgentKind,
    fs: &dyn TranscriptFs,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> Listing {
    let mut listing = Listing::default();
    let mut path_bytes = 0_usize;
    let mut directories = 0_usize;
    for root in roots(agent) {
        let mut pending = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            if should_stop() {
                listing.was_stopped = true;
                return listing;
            }
            directories += 1;
            if directories > MAX_DIRECTORIES {
                listing.warnings.push(format!(
                    "Listing stopped after {MAX_DIRECTORIES} directories; older sessions may be missing."
                ));
                return listing;
            }
            let Ok(children) = fs.list_dir(&directory) else {
                continue;
            };
            for child in children {
                let path = directory.child(&child.name);
                if child.info.is_dir {
                    pending.push(path);
                    continue;
                }
                if !child.info.is_file || !is_transcript(agent, &child.name) {
                    continue;
                }
                let Some(mtime_ms) = child.info.modified_ms else {
                    continue;
                };
                if child.info.length == 0 {
                    continue;
                }
                if listing.files.len() >= MAX_LISTED_FILES
                    || path_bytes + path.path.len() > MAX_PATH_BYTES
                {
                    listing.warnings.push(format!(
                        "Listing stopped at {MAX_LISTED_FILES} files; the remaining sessions are not analysed."
                    ));
                    return listing;
                }
                path_bytes += path.path.len();
                let relative = path
                    .path
                    .strip_prefix(root.path.as_str())
                    .unwrap_or(&path.path)
                    .trim_start_matches('/');
                let is_subagent = agent == AgentKind::ClaudeCode && is_claude_subagent(relative);
                listing.bytes_total += child.info.length;
                counters.files_found.fetch_add(1, Ordering::Relaxed);
                counters
                    .bytes_total
                    .fetch_add(child.info.length, Ordering::Relaxed);
                listing.files.push(ListedFile {
                    path,
                    size: child.info.length,
                    mtime_ms,
                    is_subagent,
                });
            }
        }
    }
    listing
}
