//! Phase 1 of the scan: find every transcript file of one agent.
//!
//! Claude Code keeps `<home>/.claude/projects/**.jsonl`, including the
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

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

use ilium_compaction_analysis::AgentKind;

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
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: i64,
    /// Claude: under `subagents/` or `workflows/`. Codex: always false here.
    pub is_subagent: bool,
}

impl ListedFile {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
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

/// The directories an agent's transcripts live in, below `home`.
fn roots(agent: AgentKind, home: &Path) -> Vec<PathBuf> {
    match agent {
        AgentKind::ClaudeCode => vec![home.join(".claude").join("projects")],
        AgentKind::Codex => vec![
            home.join(".codex").join("sessions"),
            home.join(".codex").join("archived_sessions"),
        ],
    }
}

fn is_transcript(agent: AgentKind, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !name.ends_with(".jsonl") {
        return false;
    }
    match agent {
        AgentKind::ClaudeCode => true,
        AgentKind::Codex => name.starts_with("rollout-"),
    }
}

/// Whether `relative` (a path below the projects root) is delegated work.
fn is_claude_subagent(relative: &Path) -> bool {
    relative.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("subagents") | Some("workflows")
        )
    })
}

/// Lists the transcripts of `agent` below `home`. Missing directories are not
/// an error (the agent may never have been used): the listing is just empty.
pub(super) fn list_transcripts(
    agent: AgentKind,
    home: &Path,
    counters: &ScanProgressCounters,
    should_stop: &dyn Fn() -> bool,
) -> Listing {
    let mut listing = Listing::default();
    let mut path_bytes = 0_usize;
    let mut directories = 0_usize;
    for root in roots(agent, home) {
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
            let Ok(children) = std::fs::read_dir(&directory) else {
                continue;
            };
            for child in children.flatten() {
                let Ok(kind) = child.file_type() else {
                    continue;
                };
                let path = child.path();
                if kind.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !kind.is_file() || !is_transcript(agent, &path) {
                    continue;
                }
                let Ok(metadata) = child.metadata() else {
                    continue;
                };
                let Some(mtime_ms) = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|elapsed| elapsed.as_millis() as i64)
                else {
                    continue;
                };
                if metadata.len() == 0 {
                    continue;
                }
                if listing.files.len() >= MAX_LISTED_FILES
                    || path_bytes + path.as_os_str().len() > MAX_PATH_BYTES
                {
                    listing.warnings.push(format!(
                        "Listing stopped at {MAX_LISTED_FILES} files; the remaining sessions are not analysed."
                    ));
                    return listing;
                }
                path_bytes += path.as_os_str().len();
                let is_subagent = agent == AgentKind::ClaudeCode
                    && path.strip_prefix(&root).is_ok_and(is_claude_subagent);
                listing.bytes_total += metadata.len();
                counters.files_found.fetch_add(1, Ordering::Relaxed);
                counters
                    .bytes_total
                    .fetch_add(metadata.len(), Ordering::Relaxed);
                listing.files.push(ListedFile {
                    path,
                    size: metadata.len(),
                    mtime_ms,
                    is_subagent,
                });
            }
        }
    }
    listing
}
