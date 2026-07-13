//! Discovers the session/thread ID an agent CLI (Claude Code, Codex)
//! assigned itself, so `ilium-client`'s background title-inference worker
//! (`naming_workers::spawn_session_title_worker`) knows which transcript
//! file to read prompts out of. Deliberately app-level orchestration, not
//! part of `ilium-detect`: it needs `/proc/<pid>/fd` reads, command-line
//! and environment inspection, and transcript-directory scans, none of
//! which belong in a pure classification crate (see
//! `ilium_detect::AgentIdentity`'s doc comment).
//!
//! Five tiers, tried in order, cheapest/most-certain first. Tiers 1-3 are
//! inherently unambiguous (they read something scoped to this exact pid);
//! tiers 4-5 exist for when all three miss (most commonly: two or more
//! Claude Code panes open on the *same project directory* at once -- in
//! practice tier 3 essentially never fires for Claude Code at all, since it
//! does not keep its transcript file's fd open between writes (confirmed by
//! polling `/proc/<pid>/fd` against a real running session), so a fresh,
//! not-yet-resumed pane depends on tiers 4-5 from the moment it starts).
//!
//! Every tier here upholds one invariant: return the right session ID, or
//! return none at all -- **never** a wrong one. A tier that is merely
//! "probably right" is not good enough, because a wrong answer here doesn't
//! surface as an error, it silently mislabels a pane with a *different*
//! agent's title. Tiers 1-3 satisfy this by construction (each reads
//! something scoped to this exact pid). Tiers 4-5 satisfy it by declining
//! (falling through to `None`) whenever more than one transcript is a
//! plausible candidate, rather than picking one -- an earlier version of
//! tier 5 picked the most-recently-modified candidate when several were
//! eligible, which is a guess, and a demonstrably wrong one: whichever
//! pane's discovery happens to run first in an unordered per-tick batch
//! would claim whatever is newest in the directory, even when that file
//! belongs to a different, not-yet-resolved sibling pane. Both tiers 4 and
//! 5 lean on one thing the caller (`crate::detection::run_due_panes`)
//! guarantees across the whole detection loop: no session ID already
//! claimed by a *different* pane (`excluded_session_ids`, rebuilt every
//! tick from every pane's *persisted* session ID, not just this tick's) is
//! ever handed to another one. That, plus each tier declining rather than
//! guessing on ambiguity, makes the whole discovery process self-healing:
//! a batch of several panes that all start at once will not all resolve on
//! the very first tick, but each subsequent tick narrows the field (the
//! pane with the tightest/most-recent start time always has the smallest
//! candidate set and resolves first, excluding its ID for everyone else),
//! converging to every pane correctly identified within a handful of ticks
//! -- rather than any of them ever being wrong even transiently.
//!
//! 1. [`from_arguments`] -- an explicit `--resume <id>`/`resume <id>` (or a
//!    handful of generic `--session`/`--thread` spellings) on the command
//!    line, present only when this pane resumed a prior session.
//! 2. [`from_environment`] -- a known session-ID environment variable.
//! 3. [`from_open_files`] (Linux only, via `/proc/<pid>/fd`) -- whichever
//!    transcript file this exact process currently holds open. Unambiguous
//!    even with several concurrent panes of the same class, since it reads
//!    *this* pid's own file descriptors rather than guessing from directory
//!    contents. In practice rarely the tier that actually resolves a Claude
//!    Code pane (see above), but harmless and unambiguous when it does fire
//!    (e.g. for CLIs that do keep the fd open), so it stays first of the
//!    two directory-scanning tiers.
//! 4. [`from_content_match`] -- Claude Code only: this exact pane's own
//!    most recently submitted line, reconstructed from raw PTY input
//!    (`pane::TerminalPaneRuntime::input_fingerprint_tracker`, the same
//!    line-editing reconstruction `shell_title::ShellCommandTracker` uses
//!    for shell command titles, but never reset by a foreground-process
//!    change -- an agent CLI's own input box is exactly what it needs to
//!    see through). Checked against every transcript modified since this
//!    process started: what a specific pane's user just typed can only
//!    ever appear in that pane's own transcript, so a match is positive
//!    identification, not a guess. Falls through (rather than guessing) if
//!    nothing was typed recently enough, the candidate text is too short
//!    to be a trustworthy fingerprint, or more than one transcript
//!    contains it.
//! 5. [`from_sole_unclaimed_project_transcript`] -- Claude Code only,
//!    genuine last resort: the *one* eligible transcript under this
//!    project's `~/.claude/projects/<slug>/` directory that isn't excluded
//!    and was modified no earlier than this process's own start time (so
//!    an older, unrelated session already on disk can never be picked up
//!    by mistake). Declines whenever more than one candidate is eligible --
//!    see the module-level invariant above for why guessing among several
//!    is never acceptable here, even as a last resort.
//!
//! Codex has no per-project session directory (`~/.codex/sessions/` is
//! organized by date, not by cwd), so tiers 4-5 are skipped for it; tier 3
//! fires for both classes equally on Linux the moment either CLI opens its
//! own transcript file, and the caller retries discovery every tick rather
//! than giving up after one miss.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ilium_core::AgentClass;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Below this length, a fingerprint from `from_content_match` is rejected
/// rather than searched for -- a short, generic line (a bare "y" or "ok"
/// approval reply) is exactly the kind of text that could coincidentally
/// appear in an unrelated transcript, which is the "random" failure mode
/// this tier exists to eliminate, not reintroduce.
const MIN_CONTENT_MATCH_CHARS: usize = 12;

/// Refreshes exactly the process fields discovery needs (command line,
/// environment, working directory) for `pids`, and only `pids` -- these
/// fields are not part of the cheap name-only refresh the detection loop
/// already does every tick for every process on the machine
/// (`ilium_detect::refresh`), and fetching them for every process just to
/// discover a session ID for a handful of agent panes would scale with
/// total system load for no benefit. A no-op on an empty slice.
pub fn refresh_for_discovery(system: &mut System, pids: &[Pid]) {
    if pids.is_empty() {
        return;
    }
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        false,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
}

/// Attempts to discover `pid`'s session ID via tiers 1-5 (see module
/// docs). `system` must already have been refreshed for `pid` via
/// [`refresh_for_discovery`] -- this function does no refreshing itself,
/// so it stays cheap, pure, in-memory iteration given its inputs. `home`
/// is the user's home directory, passed in rather than resolved
/// internally (the caller uses `directories::BaseDirs`, once per tick
/// rather than once per pane) so tests can point tiers 4-5 at a scratch
/// directory instead of the real `~/.claude`.
///
/// `recent_input` is this exact pane's `input_fingerprint_tracker`
/// snapshot (see `pane::TerminalPaneRuntime`), already age-filtered by the
/// caller -- `None` when nothing was typed recently enough to trust.
/// `excluded_session_ids` is every session ID a *different* pane has
/// already claimed this tick; tiers 4-5 never return one of these (see
/// module docs on why that invariant alone rules out the
/// same-project-directory misattribution both tiers exist to avoid).
///
/// `already_resolved` is this pane's currently-known session ID, if any.
/// When set, tiers 4-5 are skipped entirely: they're guesses (directory
/// content, not something scoped to this exact pid), and a guess must
/// never be allowed to flip a pane away from an ID already positively
/// established. Without this guard, a single tick where tiers 1-3
/// transiently miss (e.g. a race on `/proc/<pid>/fd`) would fall through
/// to guessing, and on a project directory shared by several panes that
/// guess can land on a *different* transcript than the one already
/// resolved -- silently reassigning the pane and re-triggering title
/// inference for no real reason. Tiers 1-3 still run unconditionally, so a
/// genuine resume to a different session (unambiguous evidence tied to
/// this exact pid) is still picked up immediately.
pub fn discover(
    system: &System,
    pid: Pid,
    class: &AgentClass,
    home: &Path,
    recent_input: Option<&str>,
    excluded_session_ids: &HashSet<String>,
    already_resolved: Option<&str>,
) -> Option<String> {
    let process = system.process(pid)?;
    let arguments: Vec<String> = process
        .cmd()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();

    from_arguments(&arguments)
        .or_else(|| {
            process
                .environ()
                .iter()
                .filter_map(|entry| entry.to_str())
                .find_map(from_environment)
        })
        .or_else(|| from_open_files(pid.as_u32()))
        .or_else(|| {
            if !guess_tiers_eligible(class, already_resolved) {
                return None;
            }
            let cwd = process.cwd()?;
            let started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(process.start_time());
            recent_input
                .and_then(|line| {
                    from_content_match(home, cwd, started_at, line, excluded_session_ids)
                })
                .or_else(|| {
                    from_sole_unclaimed_project_transcript(
                        home,
                        cwd,
                        started_at,
                        excluded_session_ids,
                    )
                })
        })
}

/// Whether `discover`'s guess tiers (4-5) should even be attempted: only
/// for Claude Code (the only class those tiers support) and only when this
/// pane has no already-resolved session ID to protect -- see `discover`'s
/// `already_resolved` doc comment for why an established ID must never be
/// overwritten by a guess. Pulled out as its own pure function so this
/// decision is unit-testable without a live `sysinfo::System`/process,
/// which the rest of `discover` requires.
fn guess_tiers_eligible(class: &AgentClass, already_resolved: Option<&str>) -> bool {
    *class == AgentClass::Claude && already_resolved.is_none()
}

/// Extracts a UUID-like session/thread ID visibly reported by an agent
/// TUI on its own screen -- a fallback for a fresh session tiers 1-4 all
/// missed (e.g. a short-lived timing gap right after spawn), cheap enough
/// to try unconditionally every tick since it's pure string scanning of
/// a screen dump the detection loop already has in hand. Requiring the
/// same line to mention "session"/"thread" (case-insensitively) keeps an
/// unrelated UUID printed in normal output from being mistaken for one.
pub fn from_screen(screen_text: &str) -> Option<String> {
    screen_text.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("session") || lower.contains("thread")) {
            return None;
        }
        line.split(|character: char| !character.is_ascii_hexdigit() && character != '-')
            .find(|token| looks_like_uuid(token))
            .map(str::to_string)
    })
}

/// Parses the resume/session flags exposed by Claude Code and Codex, plus
/// generic session/thread spellings used by other supported CLIs.
fn from_arguments(arguments: &[String]) -> Option<String> {
    const SESSION_FLAGS: &[&str] = &[
        "--resume",
        "-r",
        "--session",
        "--session-id",
        "--thread",
        "--thread-id",
        "--conversation-id",
    ];

    for (index, argument) in arguments.iter().enumerate() {
        if let Some((flag, value)) = argument.split_once('=') {
            if SESSION_FLAGS.contains(&flag) && is_session_identifier(value) {
                return Some(value.to_string());
            }
        }
        if SESSION_FLAGS.contains(&argument.as_str()) {
            if let Some(value) = arguments.get(index + 1) {
                if is_session_identifier(value) {
                    return Some(value.clone());
                }
            }
        }
        // `codex resume <SESSION_ID>` is a positional form, unlike Claude's
        // `--resume <SESSION_ID>`.
        if argument == "resume" {
            if let Some(value) = arguments.get(index + 1) {
                if is_session_identifier(value) {
                    return Some(value.clone());
                }
            }
        }
    }
    None
}

/// Reads session/thread IDs propagated explicitly through the environment.
fn from_environment(entry: &str) -> Option<String> {
    const SESSION_ENVIRONMENT_NAMES: &[&str] = &[
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_SESSION_ID",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "AGENT_SESSION_ID",
    ];
    let (name, value) = entry.split_once('=')?;
    (SESSION_ENVIRONMENT_NAMES.contains(&name) && is_session_identifier(value))
        .then(|| value.to_string())
}

/// Scans `/proc/<pid>/fd` for an open file matching a known agent CLI's
/// session-file layout, and extracts the session ID from its filename.
/// Linux-only (procfs); yields nothing on other platforms, the same
/// graceful-degradation posture as the rest of this module.
#[cfg(target_os = "linux")]
fn from_open_files(pid: u32) -> Option<String> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .find_map(|target| session_id_from_known_transcript_path(&target))
}

#[cfg(not(target_os = "linux"))]
fn from_open_files(_pid: u32) -> Option<String> {
    None
}

/// Recognizes Claude Code's `<uuid>.jsonl` transcripts (anywhere under a
/// `.claude/projects/` directory) and Codex's `rollout-<timestamp>-<uuid>`
/// session files (anywhere under a `.codex/sessions/` directory), and
/// extracts the UUID from the filename. Requiring the known parent
/// directory (not just the filename shape) keeps an unrelated file that
/// happens to be UUID-named from being mistaken for a session transcript.
fn session_id_from_known_transcript_path(path: &Path) -> Option<String> {
    let path_text = path.to_string_lossy();
    let stem = path.file_stem()?.to_str()?;

    if path_text.contains(".claude/projects/") {
        return session_id_from_transcript_stem(&AgentClass::Claude, stem);
    }
    if path_text.contains(".codex/sessions/") {
        return session_id_from_transcript_stem(&AgentClass::Codex, stem);
    }
    None
}

/// Parses a transcript filename's stem (no directory, no extension) into a
/// session ID, given which CLI's naming convention to expect.
fn session_id_from_transcript_stem(class: &AgentClass, stem: &str) -> Option<String> {
    match class {
        // Claude Code: the whole stem is the session UUID.
        AgentClass::Claude => looks_like_uuid(stem).then(|| stem.to_string()),
        // Codex rollout filenames have changed from a date-only prefix and
        // `.json` extension to a timestamped, nested `.jsonl` layout. The
        // stable contract is the final UUID, so accept either layout
        // rather than coupling recovery to the timestamp representation.
        AgentClass::Codex => {
            let after_prefix = stem.strip_prefix("rollout-")?;
            let uuid_start = after_prefix.len().checked_sub(36)?;
            let candidate = after_prefix.get(uuid_start..)?;
            (uuid_start > 0
                && after_prefix.as_bytes().get(uuid_start - 1) == Some(&b'-')
                && looks_like_uuid(candidate))
            .then(|| candidate.to_string())
        }
        AgentClass::Other(_) => None,
    }
}

/// This project's Claude Code transcripts modified no earlier than
/// `not_before`, minus any whose session ID is in `excluded_session_ids`
/// (already claimed by a different pane this tick) -- the shared candidate
/// set both [`from_content_match`] and
/// [`from_sole_unclaimed_project_transcript`] start from.
fn eligible_project_transcripts(
    home: &Path,
    cwd: &Path,
    not_before: SystemTime,
    excluded_session_ids: &HashSet<String>,
) -> Vec<(SystemTime, String, PathBuf)> {
    let project_dir = home
        .join(".claude")
        .join("projects")
        .join(slugify_claude_project_path(cwd));

    std::fs::read_dir(project_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_string();
            let is_candidate_transcript = path.extension().and_then(|extension| extension.to_str())
                == Some("jsonl")
                && looks_like_uuid(&stem)
                && !excluded_session_ids.contains(&stem);
            if !is_candidate_transcript {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            (modified >= not_before).then_some((modified, stem, path))
        })
        .collect()
}

/// Tier 4: positively identifies this pane's own transcript by content --
/// the one (and only one) eligible transcript with a genuine user-typed
/// message containing `recent_input`, the line this exact pane's user most
/// recently submitted. See the module docs for why a match here is
/// trustworthy rather than a guess, and why a too-short needle or more
/// than one match both fall through empty instead of picking arbitrarily.
fn from_content_match(
    home: &Path,
    cwd: &Path,
    not_before: SystemTime,
    recent_input: &str,
    excluded_session_ids: &HashSet<String>,
) -> Option<String> {
    // `ShellCommandTracker::observe` appends '…' when it truncated a long
    // line -- strip it so the needle stays a genuine prefix of whatever
    // the transcript actually recorded. Whitespace-normalized like
    // `transcript_message_contains` normalizes the transcript side, so a
    // wrapped or double-spaced typed line still matches the single-line,
    // single-spaced text a Claude Code transcript record actually stores.
    let needle = normalize_whitespace(recent_input.trim_end_matches('…').trim());
    if needle.chars().count() < MIN_CONTENT_MATCH_CHARS {
        return None;
    }

    let mut matches = eligible_project_transcripts(home, cwd, not_before, excluded_session_ids)
        .into_iter()
        .filter(|(_, _, path)| transcript_message_contains(path, &needle))
        .map(|(_, stem, _)| stem);

    let first_match = matches.next()?;
    // More than one transcript contains the same text (e.g. an identical
    // short prompt sent to two panes) -- ambiguous, so this tier declines
    // rather than guessing; tier 5 still gets a chance below.
    if matches.next().is_some() {
        return None;
    }
    Some(first_match)
}

/// True if some genuine user-typed message in the Claude Code transcript
/// at `path` contains `needle` (already whitespace-normalized by the
/// caller). Matches against JSON-*decoded* message text, not raw file
/// bytes -- a typed `"` is stored on disk as `\"`, so a raw
/// `file_contents.contains(needle)` would silently miss any prompt
/// containing a quote, backslash, or other character JSON escapes. Applies
/// the same "genuine user turn" filter `transcript_prompts::claude_user_prompts`
/// (the client-side title-inference reader) uses -- `"type":"user"`,
/// non-sidechain, plain-string `message.content` -- so this only ever
/// matches text the user actually typed, not a tool result or an
/// assistant's echo of it.
fn transcript_message_contains(path: &Path, needle: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents.lines().any(|line| {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        if entry.get("type").and_then(serde_json::Value::as_str) != Some("user") {
            return false;
        }
        if entry
            .get("isSidechain")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return false;
        }
        let Some(text) = entry
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
        else {
            return false;
        };
        normalize_whitespace(text).contains(needle)
    })
}

/// Collapses whitespace runs (including newlines) to single spaces, the
/// same normalization `shell_title::normalize_title` applies to a typed
/// line -- applied to both the typed-input needle and the transcript's
/// recorded text so a multi-line paste or double-spaced typo doesn't
/// defeat an otherwise-genuine match.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tier 5, last resort: the *sole* eligible transcript, with no content
/// signal to disambiguate by. Deliberately declines (returns `None`) unless
/// exactly one candidate remains -- picking among several by recency (an
/// earlier version of this function did, via "newest modified wins") is a
/// guess that can be *wrong*, not just uncertain: whichever pane's discovery
/// happens to run first in an unordered per-tick batch would claim whatever
/// is newest in the directory, even when that file genuinely belongs to a
/// different, not-yet-resolved sibling pane that simply hasn't been
/// processed yet this tick. See module docs for why "return the right
/// answer or no answer, never a wrong one" is the invariant every tier here
/// must uphold, and why this one is safe now: `excluded_session_ids` only
/// grows and only persists once a pane resolves (`ilium-server`'s
/// `run_due_panes` recomputes it from every pane's *persisted* session ID,
/// not just this tick's), so a batch of N panes that all start within the
/// same tick or two converges to every pane correctly resolved within a
/// handful of ticks -- the pane with the tightest (latest) `not_before`
/// always has the smallest candidate set and resolves first, which then
/// narrows the next pane's candidate set on a later tick, and so on --
/// rather than ever handing out a guess that has to be silently wrong for
/// someone.
fn from_sole_unclaimed_project_transcript(
    home: &Path,
    cwd: &Path,
    not_before: SystemTime,
    excluded_session_ids: &HashSet<String>,
) -> Option<String> {
    let mut candidates =
        eligible_project_transcripts(home, cwd, not_before, excluded_session_ids).into_iter();
    let (_, stem, _) = candidates.next()?;
    // More than one still-eligible transcript -- which one is genuinely
    // this pane's own can't be told apart without a content signal, so
    // decline rather than guess; a later tick (once a sibling resolves and
    // starts being excluded) gets another chance.
    if candidates.next().is_some() {
        return None;
    }
    Some(stem)
}

/// Reproduces Claude Code's project-directory slug: every character that
/// isn't an ASCII letter or digit becomes `-` (e.g. `/home/developer/.claude-docs`
/// -> `-home-arthur--claude-docs`, `/tmp/e2e_project` -> `-tmp-e2e-project`).
/// Verified against 103 real `~/.claude/projects/<slug>` directories on this
/// machine against each one's own recorded `cwd`: Claude Code replaces every
/// non-alphanumeric byte, not just `/` and `.` -- an earlier version of this
/// function that replaced only those two left underscores (and any other
/// punctuation) untouched, so it silently computed a project directory that
/// never existed for any cwd containing one, and tiers 4-5 discovery
/// permanently missed for those projects.
fn slugify_claude_project_path(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// True for the canonical UUID layout used by Claude and Codex session IDs.
fn looks_like_uuid(value: &str) -> bool {
    const HYPHEN_INDICES: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if HYPHEN_INDICES.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

/// Conservatively accepts UUID-like identifiers and named sessions, while
/// excluding flags such as `--last` and other option tokens.
fn is_session_identifier(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_title::ShellCommandTracker;
    use std::thread;

    /// A fresh, thread-unique scratch directory under the OS temp dir, so
    /// parallel test runs never collide with each other or with a real
    /// `~/.claude`/`~/.codex`.
    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-server-session-id-tests")
            .join(format!("{:?}", thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn arguments_cover_claude_and_codex_resume_forms() {
        let id = "95fd0645-3331-408b-a7e5-36e6007bfb78";
        assert_eq!(
            from_arguments(&[
                "claude".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--resume".to_string(),
                id.to_string(),
            ]),
            Some(id.to_string())
        );
        assert_eq!(
            from_arguments(&["codex".to_string(), "resume".to_string(), id.to_string(),]),
            Some(id.to_string())
        );
    }

    #[test]
    fn arguments_ignore_resume_picker_flags() {
        assert_eq!(
            from_arguments(&[
                "codex".to_string(),
                "resume".to_string(),
                "--last".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn environment_reads_known_variable_names_only() {
        assert_eq!(
            from_environment("CLAUDE_CODE_SESSION_ID=95fd0645-3331-408b-a7e5-36e6007bfb78"),
            Some("95fd0645-3331-408b-a7e5-36e6007bfb78".to_string())
        );
        assert_eq!(from_environment("PATH=/usr/bin"), None);
    }

    #[test]
    fn screen_fallback_requires_session_context_and_a_real_uuid() {
        let id = "6bc4a1a6-2a2d-4f2d-b170-70fd81284a1c";
        assert_eq!(
            from_screen(&format!("Session ID: {id}")),
            Some(id.to_string())
        );
        assert_eq!(from_screen(id), None);
    }

    #[test]
    fn open_file_path_recognizes_claude_transcript() {
        let id = "22079c73-778b-40e6-8491-44848552d771";
        let path = Path::new("/home/developer/.claude/projects/-home-arthur-dev-ai-ilium")
            .join(format!("{id}.jsonl"));
        assert_eq!(
            session_id_from_known_transcript_path(&path),
            Some(id.to_string())
        );
    }

    #[test]
    fn open_file_path_recognizes_current_codex_rollout() {
        let id = "4e8767e0-8b01-4329-bfc6-a6087b1b1f9e";
        let path = Path::new("/home/developer/.codex/sessions/2026/07/11")
            .join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl"));
        assert_eq!(
            session_id_from_known_transcript_path(&path),
            Some(id.to_string())
        );
    }

    #[test]
    fn open_file_path_ignores_files_outside_known_session_directories() {
        let id = "22079c73-778b-40e6-8491-44848552d771";
        let path = Path::new("/tmp/random").join(format!("{id}.jsonl"));
        assert_eq!(session_id_from_known_transcript_path(&path), None);
    }

    /// Builds `<home>/.claude/projects/<slug-of-cwd>/`, the directory
    /// `eligible_project_transcripts` actually scans, so tests can drop
    /// fixture transcripts exactly where the real function looks instead
    /// of guessing.
    fn claude_project_dir(home: &Path, cwd: &Path) -> std::path::PathBuf {
        let dir = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sole_unclaimed_project_transcript_is_gated_by_start_time() {
        // `Other`/`Codex` never reach this tier via `discover` (checked
        // there, not here) -- this test exercises the function directly.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let old_id = "11111111-1111-4111-8111-111111111111";
        let new_id = "22222222-2222-4222-8222-222222222222";
        std::fs::write(project_dir.join(format!("{old_id}.jsonl")), "old").unwrap();
        // Ensure distinct mtimes are observable on filesystems with coarse
        // resolution.
        std::thread::sleep(Duration::from_millis(20));
        let not_before = SystemTime::now();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(project_dir.join(format!("{new_id}.jsonl")), "new").unwrap();

        // `old_id` predates `not_before` so it's excluded by the mtime gate,
        // leaving exactly one eligible candidate -- the case this tier is
        // allowed to resolve.
        assert_eq!(
            from_sole_unclaimed_project_transcript(&home, cwd, not_before, &HashSet::new()),
            Some(new_id.to_string())
        );
    }

    #[test]
    fn sole_unclaimed_project_transcript_ignores_files_older_than_not_before() {
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "33333333-3333-4333-8333-333333333333";
        std::fs::write(project_dir.join(format!("{id}.jsonl")), "content").unwrap();
        let not_before = SystemTime::now() + Duration::from_secs(60);

        assert_eq!(
            from_sole_unclaimed_project_transcript(&home, cwd, not_before, &HashSet::new()),
            None
        );
    }

    #[test]
    fn sole_unclaimed_project_transcript_declines_rather_than_guessing_between_two_fresh_panes() {
        // Two Claude Code panes opened seconds apart in the *same* project
        // directory, neither has typed anything yet (so tier 4 can't help)
        // and neither has an already-resolved session ID. Pane A started
        // first; by the time discovery runs for A, pane B (started later)
        // has *already* created its own transcript file. A pre-fix version
        // of this tier picked whichever transcript was most recently
        // modified, which here is pane B's -- a real, demonstrated
        // misattribution (pane A silently gets titled from pane B's
        // conversation), not a hypothetical one. The fix: with two eligible
        // candidates and no way to tell them apart, decline instead of
        // guessing.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let pane_a_id = "aaaaaaaa-0000-4000-8000-000000000000";
        let pane_b_id = "bbbbbbbb-0000-4000-8000-000000000000";

        let pane_a_start = SystemTime::now();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(project_dir.join(format!("{pane_a_id}.jsonl")), "a").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // Pane B starts later and its transcript is created later still --
        // both real timestamps, both after pane A's own start time, so both
        // are eligible candidates from pane A's point of view.
        std::fs::write(project_dir.join(format!("{pane_b_id}.jsonl")), "b").unwrap();

        assert_eq!(
            from_sole_unclaimed_project_transcript(&home, cwd, pane_a_start, &HashSet::new()),
            None,
            "two eligible candidates and no content signal -- must decline, never guess"
        );
    }

    #[test]
    fn sole_unclaimed_project_transcript_converges_once_the_other_pane_is_excluded() {
        // Continues the previous scenario one tick later: pane B has since
        // resolved (via some other tier, or a later retry of this same
        // one) and is now excluded, so pane A's candidate set narrows to
        // exactly one and it can finally, correctly resolve -- this is the
        // self-healing behavior the module docs describe: ambiguous now
        // does not mean wrong forever.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let pane_a_id = "aaaaaaaa-0000-4000-8000-000000000000";
        let pane_b_id = "bbbbbbbb-0000-4000-8000-000000000000";

        let pane_a_start = SystemTime::now();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(project_dir.join(format!("{pane_a_id}.jsonl")), "a").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(project_dir.join(format!("{pane_b_id}.jsonl")), "b").unwrap();

        let excluded: HashSet<String> = [pane_b_id.to_string()].into_iter().collect();
        assert_eq!(
            from_sole_unclaimed_project_transcript(&home, cwd, pane_a_start, &excluded),
            Some(pane_a_id.to_string())
        );
    }

    #[test]
    fn sole_unclaimed_project_transcript_skips_ids_another_pane_already_claimed() {
        // The exact scenario the bug report described: two concurrent
        // Claude Code panes in the same project, the newest transcript
        // belongs to the *other* pane (already claimed), and with no
        // content signal to go on this tier must fall back to the only
        // remaining candidate instead of handing out a duplicate.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let this_panes_id = "44444444-4444-4444-8444-444444444444";
        let other_panes_id = "55555555-5555-4555-8555-555555555555";
        std::fs::write(project_dir.join(format!("{this_panes_id}.jsonl")), "mine").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(
            project_dir.join(format!("{other_panes_id}.jsonl")),
            "theirs, and more recently written",
        )
        .unwrap();
        let not_before = SystemTime::UNIX_EPOCH;
        let excluded: HashSet<String> = [other_panes_id.to_string()].into_iter().collect();

        assert_eq!(
            from_sole_unclaimed_project_transcript(&home, cwd, not_before, &excluded),
            Some(this_panes_id.to_string())
        );
    }

    #[test]
    fn content_match_identifies_the_one_transcript_containing_the_typed_line() {
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let this_panes_id = "66666666-6666-4666-8666-666666666666";
        let other_panes_id = "77777777-7777-4777-8777-777777777777";
        std::fs::write(
            project_dir.join(format!("{this_panes_id}.jsonl")),
            r#"{"type":"user","message":{"content":"fix the auth token refresh bug"}}"#,
        )
        .unwrap();
        std::fs::write(
            project_dir.join(format!("{other_panes_id}.jsonl")),
            r#"{"type":"user","message":{"content":"add pagination to the search results"}}"#,
        )
        .unwrap();
        let not_before = SystemTime::UNIX_EPOCH;

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                not_before,
                "fix the auth token refresh bug",
                &HashSet::new(),
            ),
            Some(this_panes_id.to_string())
        );
    }

    #[test]
    fn content_match_rejects_a_too_short_fingerprint() {
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "88888888-8888-4888-8888-888888888888";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            r#"{"type":"user","message":{"content":"y"}}"#,
        )
        .unwrap();

        assert_eq!(
            from_content_match(&home, cwd, SystemTime::UNIX_EPOCH, "y", &HashSet::new()),
            None
        );
    }

    #[test]
    fn content_match_declines_when_more_than_one_transcript_matches() {
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let first_id = "99999999-9999-4999-8999-999999999999";
        let second_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let shared_line = "run the full regression suite before merging";
        for id in [first_id, second_id] {
            std::fs::write(
                project_dir.join(format!("{id}.jsonl")),
                format!(r#"{{"type":"user","message":{{"content":"{shared_line}"}}}}"#),
            )
            .unwrap();
        }

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                SystemTime::UNIX_EPOCH,
                shared_line,
                &HashSet::new(),
            ),
            None
        );
    }

    #[test]
    fn content_match_strips_the_truncation_ellipsis_before_searching() {
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            r#"{"type":"user","message":{"content":"refactor the connection pool sizing logic"}}"#,
        )
        .unwrap();

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                SystemTime::UNIX_EPOCH,
                "refactor the connection pool sizing…",
                &HashSet::new(),
            ),
            Some(id.to_string())
        );
    }

    #[test]
    fn content_match_finds_a_typed_quote_through_json_escaping() {
        // The transcript stores a typed `"` as `\"` on disk -- a raw
        // `file_contents.contains(needle)` (the pre-fix implementation)
        // would miss this. Matching against JSON-decoded text, as
        // `from_content_match` does now, must not.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "message": {"content": "add a \"retry\" wrapper around the fetch call"}
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                SystemTime::UNIX_EPOCH,
                "add a \"retry\" wrapper around the fetch call",
                &HashSet::new(),
            ),
            Some(id.to_string())
        );
    }

    #[test]
    fn content_match_ignores_whitespace_differences_between_typed_and_recorded_text() {
        // A multi-line paste collapses to one committed line in
        // `ShellCommandTracker`'s reconstruction, and a double space typed
        // by the user is exactly what `normalize_title` compacts away --
        // the transcript keeps whatever Claude Code itself recorded. Both
        // sides must be normalized the same way for a genuine match to
        // still succeed.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "message": {"content": "fix   the   flaky\nupload test"}
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                SystemTime::UNIX_EPOCH,
                "fix the flaky upload test",
                &HashSet::new(),
            ),
            Some(id.to_string())
        );
    }

    #[test]
    fn content_match_never_matches_a_tool_result_or_sidechain_turn() {
        // Both are `"type":"user"` in a real transcript but were never
        // typed by the human at the keyboard -- a tool result's `content`
        // is a JSON array (so `.as_str()` already excludes it) and a
        // sidechain turn is a sub-agent's own internal prompt, not this
        // pane's user's. Neither should ever produce a positive
        // fingerprint match.
        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
        let needle = "investigate the intermittent timeout";
        let lines = [
            serde_json::json!({
                "type": "user",
                "message": {"content": [{"type": "tool_result", "content": needle}]}
            })
            .to_string(),
            serde_json::json!({
                "type": "user",
                "isSidechain": true,
                "message": {"content": needle}
            })
            .to_string(),
        ];
        std::fs::write(project_dir.join(format!("{id}.jsonl")), lines.join("\n")).unwrap();

        assert_eq!(
            from_content_match(&home, cwd, SystemTime::UNIX_EPOCH, needle, &HashSet::new()),
            None
        );
    }

    #[test]
    fn content_match_finds_a_real_tracker_commit_not_just_a_hand_built_needle() {
        // Every other content-match test hands `from_content_match` a
        // pre-built string, which never actually exercises the seam
        // `crate::detection` drives in production: raw PTY bytes ->
        // `ShellCommandTracker::observe` -> `TerminalPaneRuntime::last_submitted_line`
        // -> here. This test drives that exact reconstruction step (typed
        // characters, including a quote and a doubled space, terminated
        // by carriage return, the same as `handle_key_input` forwards) so
        // a future change to `normalize_title`'s output shape (its 80-char
        // cap, its truncation character) that broke compatibility with
        // this tier would fail here, not just look plausible.
        let mut tracker = ShellCommandTracker::default();
        let committed = tracker
            .observe(b"fix the  \"flaky\" upload test\r")
            .expect("a non-empty typed line commits on \\r");

        let home = scratch_dir();
        let cwd = Path::new("/some/project");
        let project_dir = claude_project_dir(&home, cwd);
        let id = "ffffffff-ffff-4fff-8fff-ffffffffffff";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "message": {"content": "fix the \"flaky\" upload test"}
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            from_content_match(
                &home,
                cwd,
                SystemTime::UNIX_EPOCH,
                &committed,
                &HashSet::new()
            ),
            Some(id.to_string())
        );
    }

    #[test]
    fn slugify_matches_claude_codes_own_convention() {
        assert_eq!(
            slugify_claude_project_path(Path::new("/home/developer/.claude-docs")),
            "-home-arthur--claude-docs"
        );
    }

    #[test]
    fn slugify_replaces_underscores_and_other_punctuation_too() {
        // Confirmed against real `~/.claude/projects/` directories: Claude
        // Code's own slug replaces every non-alphanumeric character, not
        // just `/` and `.` -- e.g. a real recorded cwd of
        // `/media/arthur/Big/Old_Disk_Extracted` is stored under
        // `-media-arthur-Big-Old-Disk-Extracted`.
        assert_eq!(
            slugify_claude_project_path(Path::new("/media/arthur/Big/Old_Disk_Extracted")),
            "-media-arthur-Big-Old-Disk-Extracted"
        );
    }

    #[test]
    fn guess_tiers_are_skipped_once_a_session_id_is_already_resolved() {
        assert!(!guess_tiers_eligible(
            &AgentClass::Claude,
            Some("95fd0645-3331-408b-a7e5-36e6007bfb78"),
        ));
    }

    #[test]
    fn guess_tiers_are_available_for_claude_with_nothing_resolved_yet() {
        assert!(guess_tiers_eligible(&AgentClass::Claude, None));
    }

    #[test]
    fn guess_tiers_never_apply_outside_claude() {
        assert!(!guess_tiers_eligible(&AgentClass::Codex, None));
    }

    #[test]
    fn is_session_identifier_rejects_flags_and_empty_values() {
        assert!(is_session_identifier(
            "95fd0645-3331-408b-a7e5-36e6007bfb78"
        ));
        assert!(!is_session_identifier("--last"));
        assert!(!is_session_identifier(""));
    }
}
