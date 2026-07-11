//! Agent detection glue for the illium bin crate: app-level orchestration
//! *around* `illium-detect`'s two pure signals, plus the I/O-heavy
//! session-ID discovery that doesn't belong in a pure classification
//! crate.
//!
//! `illium-detect::identify_agent`/`classify_activity` are re-exported
//! (via thin wrappers below) so `app.rs`'s call sites keep working
//! unchanged. What stays here, and only here:
//!
//! - `identify_agent`/`refresh`: thin wrappers that also perform the
//!   targeted `cwd`/`environ` refresh and 5-tier session-ID discovery
//!   below -- work that reads `/proc/<pid>/fd` and scans transcript files
//!   on disk, i.e. app-level orchestration, not pure classification.
//! - The 5-tier session-ID discovery itself (`extract_session_id` and its
//!   tiers): command-line args, environment, `/proc/<pid>/fd`, newest
//!   project transcript, and (via `extract_session_id_from_recent_prompt_match`)
//!   a submitted-prompt content match.
//! - `next_activity`: folds a raw per-tick `classify_activity` reading
//!   together with the previous displayed activity and pane-focus state --
//!   this depends on tree/UI state (focus, prior activity) that only
//!   `app.rs` owns, so it isn't a pure classification concern either.

use std::path::Path;
use std::time::{Duration, SystemTime};

use illium_core::{AgentActivity, AgentClass};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Runtime metadata for the actual agent process discovered below a pane's
/// shell. This remains outside `illium-core`: OS PIDs and CLI session IDs
/// are volatile infrastructure details, not logical tree state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessInfo {
    pub class: AgentClass,
    pub pid: u32,
    /// Present when the process explicitly exposes a session/thread ID in
    /// its command line or environment. Fresh sessions often do not.
    pub session_id: Option<String>,
}

/// Scans a pane's plain-text screen contents for activity markers and
/// classifies it. Pure classification -- delegates entirely to
/// `illium-detect`.
pub fn classify_activity(screen_text: &str) -> AgentActivity {
    illium_detect::classify_activity(screen_text)
}

/// Turns one tick's raw `classify_activity` reading into the activity
/// that should actually be displayed, given what was displayed last tick
/// and whether the user is currently looking at this pane (it's the
/// focused pane).
///
/// `raw` is expected to only ever be `Working`, `WaitingApproval`, or
/// `Idle` (what `classify_activity` produces), but `Done` is handled too
/// rather than relying on that as an unenforced cross-module invariant --
/// if it ever arrived as `Done`, that's treated the same as `Idle` (no
/// live working signal this tick).
///
/// Rules:
/// - A live `Working` or `WaitingApproval` signal always wins outright --
///   sending a fresh prompt (or hitting a new confirmation) immediately
///   supersedes whatever was showing before, including a lingering
///   `Done`.
/// - Otherwise (no live signal this tick): if the pane is focused right
///   now, it's `Idle` -- either it was already calm, or the user is
///   watching it finish in real time, so there's nothing to flag. If it's
///   *not* focused and it just went quiet after `Working`/`WaitingApproval`/
///   `Done`, it becomes (or stays) `Done` -- flagged for attention until
///   the user looks. If it was already plain `Idle` and unfocused, it
///   stays `Idle`.
pub fn next_activity(
    previous: AgentActivity,
    raw: AgentActivity,
    is_focused: bool,
) -> AgentActivity {
    match raw {
        AgentActivity::Working => AgentActivity::Working,
        AgentActivity::WaitingApproval => AgentActivity::WaitingApproval,
        AgentActivity::Idle | AgentActivity::Done => {
            if is_focused {
                AgentActivity::Idle
            } else {
                match previous {
                    AgentActivity::Working
                    | AgentActivity::WaitingApproval
                    | AgentActivity::Done => AgentActivity::Done,
                    AgentActivity::Idle => AgentActivity::Idle,
                }
            }
        }
    }
}

/// Refreshes the system-wide process list (pid/parent/name only) on the
/// given System. Call this once per detection tick (shared across all
/// panes), not once per pane. Delegates to `illium-detect`, which owns
/// the field set its identity walk actually needs.
pub fn refresh(system: &mut System) {
    illium_detect::refresh(system);
}

/// Given the OS pid of a pane's directly-spawned child (typically the
/// user's shell), finds the agent CLI identity below it (via
/// `illium_detect::identify_agent`'s process-tree walk against the
/// registry of known signatures) and, if one is found, discovers its
/// session ID -- the app-level orchestration `illium-detect` deliberately
/// doesn't own: a targeted `cwd`/`environ` refresh scoped to just the
/// matched pid, followed by the 5-tier `extract_session_id` lookup below
/// (command-line args, environment, `/proc/<pid>/fd`, and transcript
/// scanning).
///
/// Returns `None` if no descendant process matches a known agent CLI
/// signature.
pub fn identify_agent(system: &mut System, shell_pid: Pid) -> Option<AgentProcessInfo> {
    let identity = illium_detect::identify_agent(system, shell_pid)?;
    let matched_pid = Pid::from_u32(identity.pid);

    // `cwd` can change over a process's life (only tier 4 needs a fresh
    // read); `environ` is fixed at exec time, so it's fetched once and
    // cached thereafter. Scoped to just this one matched pid -- see
    // `illium_detect::refresh`'s doc comment for why those fields aren't
    // fetched for every process up front.
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[matched_pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::Always)
            .with_environ(UpdateKind::OnlyIfNotSet),
    );
    let process = system.process(matched_pid)?;
    Some(AgentProcessInfo {
        session_id: extract_session_id(process, &identity.class),
        class: identity.class,
        pid: identity.pid,
    })
}

/// Extracts a CLI session/thread ID from explicit, auditable sources, in
/// order from most to least direct -- each tier only runs if the previous
/// one found nothing:
///
/// 1. **Command-line arguments** (`extract_session_id_from_arguments`) --
///    only present when the session was explicitly resumed
///    (`claude --resume <id>`, `codex resume <id>`), not for a fresh one.
/// 2. **Environment variables** (`extract_session_id_from_environment`) --
///    Claude Code sets `CLAUDE_CODE_SESSION_ID` in its own process
///    environment from the moment it starts (confirmed by direct
///    inspection of a live session), covering the fresh-session case with
///    no user action needed. Codex's equivalent, if any, is unconfirmed --
///    this tier costs nothing to try and simply finds nothing if absent.
/// 3. **Open transcript file** (`extract_session_id_from_open_files`) --
///    reads `/proc/<pid>/fd` (Linux only) for the session file the CLI is
///    itself holding open (Claude's per-session `.jsonl`, Codex's
///    `rollout-*.json`) and pulls the ID out of its filename. Tied to this
///    exact process, so it can't be confused by another concurrent session
///    even in the same project -- the strongest fallback when tier 2 comes
///    up empty (as it will for Codex until confirmed otherwise).
/// 4. **Newest project transcript** (
///    `extract_session_id_from_newest_project_transcript`) -- last resort:
///    scans `~/.claude/projects/<slugified-cwd>/` for the most recently
///    modified transcript created after this process started. Claude-only
///    (Codex's session directory isn't project-scoped, so this can't
///    disambiguate concurrent Codex panes) and only reached if the fd scan
///    above found nothing, e.g. a non-Linux host or a fd snapshot that
///    happened to land between writes. Gated on `class == AgentClass::Claude`
///    -- without that check, a Codex pane whose tiers 1-3 all miss (e.g. a
///    timing gap right after spawn, before Codex opens its rollout file)
///    would otherwise pick up an unrelated Claude pane's session ID from the
///    same project directory.
///
/// We do not infer a fresh session from history files with no recency or
/// process-identity check at all, because multiple live panes can share a
/// working directory and such a guess would be wrong.
fn extract_session_id(process: &Process, class: &AgentClass) -> Option<String> {
    let arguments: Vec<String> = process
        .cmd()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();

    extract_session_id_from_arguments(&arguments)
        .or_else(|| {
            process
                .environ()
                .iter()
                .filter_map(|entry| entry.to_str())
                .find_map(extract_session_id_from_environment)
        })
        .or_else(|| extract_session_id_from_open_files(process.pid().as_u32()))
        .or_else(|| {
            if *class != AgentClass::Claude {
                return None;
            }
            let cwd = process.cwd()?;
            let started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(process.start_time());
            extract_session_id_from_newest_project_transcript(cwd, started_at)
        })
}

/// Scans `/proc/<pid>/fd` for an open file matching a known agent CLI's
/// session-file layout, and extracts the session ID from its filename.
/// Linux-only (procfs); yields nothing on other platforms, the same
/// graceful-degradation posture as the rest of this module.
///
/// A running CLI holds its own transcript file open for the session's
/// duration (Claude Code's per-session `.jsonl`, Codex's
/// `rollout-*.jsonl`), so whichever session file *this exact pid* has open
/// is unambiguous -- unlike guessing from directory contents, it can't be
/// confused by a second concurrent session in the same project.
#[cfg(target_os = "linux")]
fn extract_session_id_from_open_files(pid: u32) -> Option<String> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .find_map(|target| session_id_from_known_transcript_path(&target))
}

#[cfg(not(target_os = "linux"))]
fn extract_session_id_from_open_files(_pid: u32) -> Option<String> {
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
/// session ID, given which CLI's naming convention to expect. Shared by
/// the path-based (`session_id_from_known_transcript_path`) and
/// content-based (`extract_session_id_from_recent_prompt_match`) lookups
/// so both agree on exactly what counts as a valid transcript filename.
fn session_id_from_transcript_stem(class: &AgentClass, stem: &str) -> Option<String> {
    match class {
        // Claude Code: the whole stem is the session UUID.
        AgentClass::Claude => looks_like_uuid(stem).then(|| stem.to_string()),
        // Codex rollout filenames have changed from a date-only prefix and
        // `.json` extension to a timestamped, nested `.jsonl` layout. The
        // stable contract is the final UUID, so accept either layout rather
        // than coupling recovery to the timestamp representation.
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

/// Last-resort fallback: the most recently modified Claude Code transcript
/// under this project's `~/.claude/projects/<slug>/` directory, but only
/// if it was modified at or after `not_before` (the pane's own process
/// start time) -- so an older, unrelated session already on disk before
/// this process even started can never be picked up by mistake. Codex has
/// no per-project session directory, so this fallback is Claude-only.
fn extract_session_id_from_newest_project_transcript(
    cwd: &Path,
    not_before: SystemTime,
) -> Option<String> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let project_dir = home
        .join(".claude")
        .join("projects")
        .join(slugify_claude_project_path(cwd));

    std::fs::read_dir(project_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_string();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && looks_like_uuid(&stem))
            .then_some(())?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            (modified >= not_before).then_some((modified, stem))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, stem)| stem)
}

/// Reproduces Claude Code's project-directory slug: every `/` and `.` in
/// the absolute path becomes `-` (e.g. `/home/developer/.claude-docs` ->
/// `-home-arthur--claude-docs`, verified against this machine's own
/// `~/.claude/projects/` layout -- the doubled `-` is exactly where the
/// leading `.` of a dotfile-named directory sits).
fn slugify_claude_project_path(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character == '/' || character == '.' {
                '-'
            } else {
                character
            }
        })
        .collect()
}

/// Best-effort extraction of "what the user just typed" from a pane's
/// screen right before `Enter` is forwarded to it: the last non-blank
/// screen line, with a leading prompt decoration (a cursor glyph, shell
/// prompt character, box-drawing border, and surrounding whitespace)
/// stripped off. Feeds `extract_session_id_from_recent_prompt_match` --
/// the caller captures this the moment Enter is pressed, before the
/// screen has a chance to change.
///
/// Imperfect for a prompt that wraps across multiple screen lines (only
/// the last visual line is seen), but the common case -- a short prompt
/// on one line -- is what matters most in practice; a too-short result
/// (fewer than 3 characters) is rejected as too weak a signal to search
/// transcripts for, since it would match almost anything.
pub fn capture_submitted_prompt_text(screen_text: &str) -> Option<String> {
    const LEADING_DECORATION: &[char] = &['\u{276f}', '>', '$', '#', '\u{2502}', '|'];
    let line = screen_text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())?;
    let trimmed = line
        .trim_start_matches(|character: char| {
            character.is_whitespace() || LEADING_DECORATION.contains(&character)
        })
        .trim();
    (trimmed.chars().count() >= 3).then(|| trimmed.to_string())
}

/// Tier 5, cross-platform (no procfs): triggered right after the user
/// submits a prompt in a pane whose session ID is still unknown --
/// searches this project's very recently modified transcript files for
/// one that now contains that exact prompt text, and takes its filename
/// as the session ID.
///
/// Unlike `extract_session_id_from_open_files` (Linux-only, works the
/// instant the process starts but needs `/proc`), this is plain
/// `std::fs` -- directory listing, file metadata, file reads -- so it
/// works identically on macOS and Windows. The tradeoff is it can only
/// fire once the user has actually sent a prompt, since that's the only
/// text guaranteed to land in the transcript that illium can also read
/// directly off the screen.
///
/// `not_before` should be the moment the prompt was captured (not
/// process start): only files modified at or after that point are even
/// opened, both to avoid matching a stale transcript from a previous
/// session and to keep this cheap -- Codex's `~/.codex/sessions/` in
/// particular can hold thousands of historical rollouts, but only ones
/// touched in the last few seconds are ever read.
///
/// `home` is the user's home directory, passed in rather than resolved
/// internally (the caller uses `directories::BaseDirs`) so tests can
/// point this at a scratch directory instead of the real `~/.claude` or
/// `~/.codex`.
pub fn extract_session_id_from_recent_prompt_match(
    class: &AgentClass,
    home: &Path,
    cwd: &Path,
    prompt_text: &str,
    not_before: SystemTime,
) -> Option<String> {
    let needle = prompt_text.trim();
    if needle.chars().count() < 3 {
        return None;
    }

    let (dir, expected_extension) = transcript_dir_and_extension(class, home, cwd)?;

    transcript_files_under(&dir, class.clone(), expected_extension)
        .into_iter()
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?;
            let id = session_id_from_transcript_stem(class, stem)?;
            let modified = path.metadata().ok()?.modified().ok()?;
            (modified >= not_before).then_some((path, id))
        })
        .find_map(|(path, id)| {
            std::fs::read_to_string(&path)
                .ok()?
                .contains(needle)
                .then_some(id)
        })
}

/// Locates the on-disk transcript file for a session ID illium already
/// knows with certainty (as opposed to the tiers above, which are trying to
/// *discover* an unknown ID). Shared by anything that needs to read a
/// session's transcript content once its ID is settled -- currently only
/// `session_naming::infer_pane_title`, which reads recent user prompts out
/// of it -- so that caller never has to re-derive Claude/Codex's own
/// transcript-path conventions itself.
pub(crate) fn transcript_path_for_session(
    class: &AgentClass,
    home: &Path,
    cwd: &Path,
    session_id: &str,
) -> Option<std::path::PathBuf> {
    let (dir, expected_extension) = transcript_dir_and_extension(class, home, cwd)?;
    transcript_files_under(&dir, class.clone(), expected_extension)
        .into_iter()
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| session_id_from_transcript_stem(class, stem))
                .as_deref()
                == Some(session_id)
        })
}

/// The directory to search and the file extension to expect for a given
/// agent's transcripts -- `None` for `AgentClass::Other`, which has no known
/// transcript format.
fn transcript_dir_and_extension(
    class: &AgentClass,
    home: &Path,
    cwd: &Path,
) -> Option<(std::path::PathBuf, &'static str)> {
    match class {
        AgentClass::Claude => Some((
            home.join(".claude")
                .join("projects")
                .join(slugify_claude_project_path(cwd)),
            "jsonl",
        )),
        AgentClass::Codex => Some((home.join(".codex").join("sessions"), "jsonl")),
        AgentClass::Other(_) => None,
    }
}

/// Returns transcript files in `directory` that match `expected_extension`.
/// Codex stores current rollouts under date directories while Claude keeps a
/// project's transcripts flat, so only Codex needs recursive traversal here.
/// The caller still applies the prompt-time mtime gate before reading content.
fn transcript_files_under(
    directory: &Path,
    class: AgentClass,
    expected_extension: &str,
) -> Vec<std::path::PathBuf> {
    let mut directories = vec![directory.to_path_buf()];
    let mut transcript_paths = Vec::new();

    while let Some(current_directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(current_directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                if class == AgentClass::Codex {
                    directories.push(path);
                }
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) == Some(expected_extension)
            {
                transcript_paths.push(path);
            }
        }
    }

    transcript_paths
}

/// Parses the resume/session flags exposed by Claude Code and Codex, plus
/// generic session/thread spellings used by other supported CLIs.
fn extract_session_id_from_arguments(arguments: &[String]) -> Option<String> {
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
fn extract_session_id_from_environment(entry: &str) -> Option<String> {
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

/// Extracts a UUID-like session/thread ID visibly reported by an agent TUI.
/// This is a fallback for fresh sessions that do not put an ID in their
/// process arguments. Requiring the same line to name a session or thread
/// prevents arbitrary UUIDs in normal terminal output from becoming titles.
pub fn extract_session_id_from_screen(screen_text: &str) -> Option<String> {
    screen_text.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if lower.contains("session") || lower.contains("thread") {
            line.split(|character: char| !character.is_ascii_hexdigit() && character != '-')
                .find(|token| looks_like_uuid(token))
                .map(str::to_string)
        } else {
            None
        }
    })
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
    use std::thread;

    /// A fresh, thread-unique scratch directory under the OS temp dir, so
    /// parallel test runs never collide with each other or with a real
    /// `~/.claude`/`~/.codex` -- mirrors `editor_pane`'s test convention.
    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-agent-detect-tests")
            .join(format!("{:?}", thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    // `classify_activity`'s own behavior (working/idle/waiting-approval
    // marker detection) and the process-name signature registry are now
    // owned and tested by `illium-detect` -- see its fixture tests under
    // `illium-detect/tests/fixtures/`. What remains here is app-level
    // orchestration: session-ID discovery and prompt-focus tracking.

    #[test]
    fn session_id_parsing_covers_claude_and_codex_resume_forms() {
        let id = "95fd0645-3331-408b-a7e5-36e6007bfb78";
        assert_eq!(
            extract_session_id_from_arguments(&[
                "claude".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--resume".to_string(),
                id.to_string(),
            ]),
            Some(id.to_string())
        );
        assert_eq!(
            extract_session_id_from_arguments(&[
                "codex".to_string(),
                "resume".to_string(),
                id.to_string(),
            ]),
            Some(id.to_string())
        );
    }

    #[test]
    fn session_id_parsing_ignores_resume_picker_flags() {
        assert_eq!(
            extract_session_id_from_arguments(&[
                "codex".to_string(),
                "resume".to_string(),
                "--last".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn session_id_screen_fallback_requires_session_context_and_uuid() {
        let id = "6bc4a1a6-2a2d-4f2d-b170-70fd81284a1c";
        assert_eq!(
            extract_session_id_from_screen(&format!("Session ID: {id}")),
            Some(id.to_string())
        );
        assert_eq!(extract_session_id_from_screen(id), None);
    }

    #[test]
    fn open_file_path_recognizes_claude_transcript() {
        let id = "22079c73-778b-40e6-8491-44848552d771";
        let path = Path::new("/home/developer/.claude/projects/-home-arthur-dev-ai-illium")
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
        let path = Path::new("/tmp/some-other-place").join(format!("{id}.jsonl"));
        assert_eq!(session_id_from_known_transcript_path(&path), None);
    }

    #[test]
    fn open_file_path_ignores_non_uuid_claude_looking_files() {
        let path =
            Path::new("/home/developer/.claude/projects/-home-arthur-dev-ai-illium/notes.jsonl");
        assert_eq!(session_id_from_known_transcript_path(path), None);
    }

    #[test]
    fn slugify_matches_observed_claude_project_directory_naming() {
        assert_eq!(
            slugify_claude_project_path(Path::new("/home/developer/dev/ai/illium")),
            "-home-arthur-dev-ai-illium"
        );
        // A dotfile-named directory doubles up the '-': one for the '/',
        // one for the '.' -- verified against this machine's own
        // `~/.claude/projects/-home-arthur--claude-docs` entry.
        assert_eq!(
            slugify_claude_project_path(Path::new("/home/developer/.claude-docs")),
            "-home-arthur--claude-docs"
        );
    }

    #[test]
    fn capture_strips_prompt_decoration_and_picks_the_last_line() {
        let screen = "some earlier output\nmore output\n\u{276f} hello from the user";
        assert_eq!(
            capture_submitted_prompt_text(screen),
            Some("hello from the user".to_string())
        );
    }

    #[test]
    fn capture_ignores_trailing_blank_lines() {
        let screen = "$ my actual prompt\n\n   \n";
        assert_eq!(
            capture_submitted_prompt_text(screen),
            Some("my actual prompt".to_string())
        );
    }

    #[test]
    fn capture_rejects_too_short_a_result() {
        assert_eq!(capture_submitted_prompt_text("\u{276f} "), None);
        assert_eq!(capture_submitted_prompt_text(""), None);
    }

    /// End-to-end, real filesystem: writes a fake Claude transcript into a
    /// scratch "home", advances its mtime past `not_before`, and confirms
    /// the tier-5 lookup finds it purely by matching the submitted prompt
    /// text -- with NO project-directory recency assumption beyond "was
    /// touched after the prompt was captured". This exercises the exact
    /// path the real function takes, not just its pure sub-pieces.
    #[test]
    fn recent_prompt_match_finds_claude_transcript_by_content() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/illium");
        let project_dir = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&project_dir).unwrap();

        let captured_at = SystemTime::now();
        // A file already on disk before the prompt was even sent must
        // never be picked up, even if its content happens to match --
        // `not_before` is the whole point of this tier being safe to fire
        // repeatedly.
        let stale_id = "00000000-0000-4000-8000-000000000000";
        std::fs::write(
            project_dir.join(format!("{stale_id}.jsonl")),
            r#"{"role":"user","content":"please refactor the login flow"}"#,
        )
        .unwrap();
        filetime_set_mtime_before(&project_dir.join(format!("{stale_id}.jsonl")), captured_at);

        std::thread::sleep(Duration::from_millis(10));

        let fresh_id = "11111111-1111-4111-8111-111111111111";
        std::fs::write(
            project_dir.join(format!("{fresh_id}.jsonl")),
            r#"{"role":"user","content":"please refactor the login flow"}"#,
        )
        .unwrap();

        let result = extract_session_id_from_recent_prompt_match(
            &AgentClass::Claude,
            &home,
            cwd,
            "please refactor the login flow",
            captured_at,
        );
        assert_eq!(result, Some(fresh_id.to_string()));
    }

    #[test]
    fn recent_prompt_match_finds_codex_rollout_by_content_in_nested_directory() {
        let home = scratch_dir();
        // Deliberately a different cwd than the transcript's own project --
        // proving Codex's non-project-scoped session directory resolves
        // correctly via content alone.
        let cwd = Path::new("/some/other/project");
        let sessions_dir = home
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("07")
            .join("11");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let captured_at = SystemTime::now();
        // Filesystem mtime resolution can be coarser than `SystemTime`'s;
        // a real prompt-then-write always has genuine separation (the CLI
        // takes at least a few milliseconds to process input), so this
        // sleep just reproduces that ordering instead of racing it.
        std::thread::sleep(Duration::from_millis(10));
        let id = "22222222-2222-4222-8222-222222222222";
        std::fs::write(
            sessions_dir.join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl")),
            r#"{"role":"user","content":"write a fibonacci function"}"#,
        )
        .unwrap();

        let result = extract_session_id_from_recent_prompt_match(
            &AgentClass::Codex,
            &home,
            cwd,
            "write a fibonacci function",
            captured_at,
        );
        assert_eq!(result, Some(id.to_string()));
    }

    #[test]
    fn recent_prompt_match_returns_none_when_no_file_contains_the_prompt() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/illium");
        let project_dir = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&project_dir).unwrap();

        let captured_at = SystemTime::now();
        let id = "33333333-3333-4333-8333-333333333333";
        std::fs::write(
            project_dir.join(format!("{id}.jsonl")),
            r#"{"role":"user","content":"a completely unrelated message"}"#,
        )
        .unwrap();

        let result = extract_session_id_from_recent_prompt_match(
            &AgentClass::Claude,
            &home,
            cwd,
            "please refactor the login flow",
            captured_at,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn recent_prompt_match_rejects_a_too_short_prompt() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/illium");
        assert_eq!(
            extract_session_id_from_recent_prompt_match(
                &AgentClass::Claude,
                &home,
                cwd,
                "hi",
                SystemTime::now()
            ),
            None
        );
    }

    #[test]
    fn transcript_path_for_session_finds_the_matching_claude_file_among_several() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/illium");
        let project_dir = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&project_dir).unwrap();

        let other_id = "44444444-4444-4444-8444-444444444444";
        let target_id = "55555555-5555-4555-8555-555555555555";
        std::fs::write(project_dir.join(format!("{other_id}.jsonl")), "{}").unwrap();
        std::fs::write(project_dir.join(format!("{target_id}.jsonl")), "{}").unwrap();

        let path = transcript_path_for_session(&AgentClass::Claude, &home, cwd, target_id)
            .expect("target transcript should be found");
        assert_eq!(path, project_dir.join(format!("{target_id}.jsonl")));
    }

    #[test]
    fn transcript_path_for_session_finds_the_matching_codex_rollout_in_nested_directories() {
        let home = scratch_dir();
        let cwd = Path::new("/some/other/project");
        let sessions_dir = home
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("07")
            .join("11");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let id = "66666666-6666-4666-8666-666666666666";
        std::fs::write(
            sessions_dir.join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl")),
            "{}",
        )
        .unwrap();

        let path = transcript_path_for_session(&AgentClass::Codex, &home, cwd, id)
            .expect("target rollout should be found");
        assert_eq!(
            path,
            sessions_dir.join(format!("rollout-2026-07-11T22-32-39-{id}.jsonl"))
        );
    }

    #[test]
    fn transcript_path_for_session_returns_none_for_unknown_agent_class() {
        let home = scratch_dir();
        let cwd = Path::new("/home/developer/dev/ai/illium");
        assert_eq!(
            transcript_path_for_session(
                &AgentClass::Other("opencode".to_string()),
                &home,
                cwd,
                "any-id"
            ),
            None
        );
    }

    /// Backdates a file's mtime slightly before `reference`, so it reads as
    /// "already existed before the prompt was captured" -- `SystemTime`
    /// has no public constructor for an arbitrary past instant, so this
    /// goes through the filesystem the same way a real stale file would.
    fn filetime_set_mtime_before(path: &Path, reference: SystemTime) {
        let older = reference - Duration::from_secs(60);
        let file = std::fs::File::open(path).unwrap();
        file.set_modified(older).expect("backdate mtime");
    }

    #[test]
    fn live_working_signal_always_wins() {
        for previous in [
            AgentActivity::Idle,
            AgentActivity::Done,
            AgentActivity::WaitingApproval,
        ] {
            assert_eq!(
                next_activity(previous, AgentActivity::Working, false),
                AgentActivity::Working
            );
            assert_eq!(
                next_activity(previous, AgentActivity::Working, true),
                AgentActivity::Working
            );
        }
    }

    #[test]
    fn finishing_while_unfocused_becomes_done() {
        assert_eq!(
            next_activity(AgentActivity::Working, AgentActivity::Idle, false),
            AgentActivity::Done
        );
    }

    #[test]
    fn finishing_while_focused_stays_idle() {
        assert_eq!(
            next_activity(AgentActivity::Working, AgentActivity::Idle, true),
            AgentActivity::Idle
        );
    }

    #[test]
    fn done_stays_done_until_focused() {
        assert_eq!(
            next_activity(AgentActivity::Done, AgentActivity::Idle, false),
            AgentActivity::Done
        );
        assert_eq!(
            next_activity(AgentActivity::Done, AgentActivity::Idle, true),
            AgentActivity::Idle
        );
    }

    #[test]
    fn plain_idle_stays_idle_when_unfocused() {
        assert_eq!(
            next_activity(AgentActivity::Idle, AgentActivity::Idle, false),
            AgentActivity::Idle
        );
    }

    #[test]
    fn waiting_approval_that_resolves_while_unfocused_becomes_done() {
        assert_eq!(
            next_activity(AgentActivity::WaitingApproval, AgentActivity::Idle, false),
            AgentActivity::Done
        );
    }
}
