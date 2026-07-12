//! Agent detection: two independent, pure signals about a terminal pane.
//!
//! `identify_agent` walks an already-populated `sysinfo::System` process
//! tree to answer "which agent CLI (if any) is running below this pane's
//! shell" -- the *identity* signal, driven by the [`AGENT_SIGNATURES`]
//! registry table. `classify_activity` scans the pane's rendered
//! plain-text screen contents to answer "is that agent working, blocked on
//! a confirmation, or idle" -- the *activity* signal.
//!
//! Both functions are pure: `classify_activity` takes only a `&str`, and
//! `identify_agent` takes a `&System` the caller has already refreshed
//! (via [`refresh`]) plus a process list it has already scanned. Neither
//! function owns a polling loop, a PTY, or any filesystem/`/proc` access
//! of its own -- that I/O (adaptive-interval scheduling, and the
//! I/O-heavy session-ID discovery that reads `/proc/<pid>/fd` and scans
//! transcript files) belongs to the caller (`illium-server`'s detection
//! loop, currently `illium`'s `app.rs` during the strangler-fig
//! migration), not to this crate.

use std::collections::HashSet;

use illium_core::{AgentActivity, AgentClass};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// One entry in the agent-identity registry: a lowercase substring to
/// match against a process name, and how to build the resulting
/// [`AgentClass`] from the matched (lowercased) name.
///
/// New agent CLI support is a new entry here, not a new branch in an
/// if/else chain -- see `CLAUDE.md`'s layering rule.
struct AgentSignature {
    /// Lowercase substring matched against a process's name.
    name_substring: &'static str,
    /// Builds the `AgentClass` for a match. Receives the matched
    /// (lowercased) process name so `AgentClass::Other` can carry the
    /// exact name that matched.
    class_of: fn(matched_name: &str) -> AgentClass,
}

/// Known agent CLI signatures, in match-priority order (first match wins
/// when a process name could match more than one entry).
const AGENT_SIGNATURES: &[AgentSignature] = &[
    AgentSignature {
        name_substring: "claude",
        class_of: |_matched_name| AgentClass::Claude,
    },
    AgentSignature {
        name_substring: "codex",
        class_of: |_matched_name| AgentClass::Codex,
    },
    AgentSignature {
        name_substring: "opencode",
        class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
    },
    AgentSignature {
        name_substring: "aider",
        class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
    },
];

/// The agent CLI identity found below a pane's shell, and the OS pid of
/// the matched process. Session/thread-ID discovery is deliberately not
/// part of this type -- that's I/O-heavy app-level orchestration (reading
/// `/proc/<pid>/fd`, scanning transcript files) that lives one layer up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIdentity {
    pub class: AgentClass,
    pub pid: u32,
}

/// Substring some agent CLIs render continuously while a turn is in
/// progress (older Claude Code builds, some Codex CLI versions). Kept as
/// one recognized trigger, but NOT the only one: a live probe against a
/// real, current Claude Code session (v2.1.207) showed its actual
/// in-progress line never contains this text at all -- it looks like
/// `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"`. See
/// `looks_like_live_status_line` for the heuristic that actually catches
/// that format.
const WORKING_MARKER: &str = "esc to interrupt";

/// Scans a pane's plain-text screen contents (as returned by
/// `vt100::Screen::contents()`) for activity markers and classifies it.
///
/// Precedence: a "working" signal is checked first because a confirmation
/// prompt never coexists with it in practice, but checking it first keeps
/// the rule unambiguous either way. Absent that, either a y/n-style
/// confirmation box or a general multiple-choice/question prompt (see
/// `looks_like_confirmation_prompt` and `looks_like_selection_prompt`)
/// means the agent is blocked waiting on the user. Anything else is
/// `Idle`.
pub fn classify_activity(screen_text: &str) -> AgentActivity {
    if screen_text.contains(WORKING_MARKER) || looks_like_live_status_line(screen_text) {
        return AgentActivity::Working;
    }

    if looks_like_confirmation_prompt(screen_text) || looks_like_selection_prompt(screen_text) {
        return AgentActivity::WaitingApproval;
    }

    AgentActivity::Idle
}

/// True if any line looks like an in-progress status line: contains an
/// ellipsis ('…') *and* an elapsed-time token (digits immediately
/// followed by 's' or 'm', e.g. "6s", "12m"). This is the structural
/// convention observed in real Claude Code output -- a present-tense
/// whimsical verb ending in '…' plus a live elapsed-time counter, e.g.
/// `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"` -- and
/// it survives whichever silly verb happens to be showing, unlike trying
/// to match exact wording. It's also distinct from the *finished*-turn
/// summary line Claude Code prints once a turn completes (e.g.
/// `"✻ Cogitated for 10s"`), which uses past tense "for Ns" with no
/// ellipsis, so it won't be mistaken for still-working.
fn looks_like_live_status_line(screen_text: &str) -> bool {
    screen_text
        .lines()
        .filter(|line| line.contains('…'))
        .any(|line| {
            line.split(|c: char| c.is_whitespace() || c == '·')
                .any(is_elapsed_time_token)
        })
}

/// True if `token` looks like an elapsed-time reading: one or more ASCII
/// digits immediately followed by a single 's' or 'm' unit suffix and
/// nothing else (so "6s" and "12m" match, but "s" or "class" don't).
fn is_elapsed_time_token(token: &str) -> bool {
    let Some(digits) = token.strip_suffix(['s', 'm']) else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// True if a line looks like a yes/no question: ends in '?' and mentions
/// "yes"/"no" as whole words (case-insensitive), not substrings — so
/// "...does that make sense or not?" (which contains "not", not "no") and
/// "...is that known?" don't false-match.
///
/// Deliberately does NOT treat "some line has the word Yes" plus "some
/// other line has the word No" *anywhere on screen* as a prompt: normal
/// agent prose routinely contains both words in unrelated sentences (a
/// pros/cons recap, a "Yes, ... No further changes needed" aside), and a
/// numbered "1. Yes" / "2. No" style menu is already caught by
/// `looks_like_selection_prompt`, which additionally requires a selection
/// cursor.
fn looks_like_confirmation_prompt(screen_text: &str) -> bool {
    screen_text.lines().any(|line| {
        let trimmed = line.trim_end();
        if !trimmed.ends_with('?') {
            return false;
        }
        let tokens: HashSet<String> = trimmed
            .split(|c: char| !c.is_alphanumeric())
            .map(|token| token.to_lowercase())
            .collect();
        tokens.contains("yes") && tokens.contains("no")
    })
}

/// True if the screen looks like a general multiple-choice / selection
/// prompt -- not necessarily yes/no -- e.g. Claude Code's numbered option
/// menus with a `❯` cursor on the currently-selected line and a footer
/// hint like "Enter to select · ↑/↓ to navigate · Esc to cancel". Either
/// of two independent signals is enough:
///
/// - A footer hint line naming both a confirm/select action and a cancel
///   action -- that exact combination of phrasing only shows up as
///   interactive-prompt chrome, never in normal command output.
/// - At least two numbered option lines (e.g. "1. Source only", "  2. Write
///   full list to file") where at least one of them is itself prefixed by
///   the `❯` selection cursor -- matching how the fixtures actually render
///   it (`"❯ 1. Source only"`). Requiring the cursor to prefix an option
///   line specifically (not merely appear *somewhere* on screen) keeps
///   this from firing when an agent's own numbered analysis (e.g. "1.
///   Findings list page...", "2. Finding detail page...") shares a screen
///   with an unrelated `❯` glyph -- a Starship-themed shell prompt uses
///   that exact character, and it doesn't mean the numbered lines above it
///   are a selection menu.
fn looks_like_selection_prompt(screen_text: &str) -> bool {
    let lines: Vec<&str> = screen_text.lines().collect();

    let has_selection_footer = lines.iter().any(|line| {
        let lower = line.to_lowercase();
        let names_a_confirm_action = lower.contains("to select")
            || lower.contains("to confirm")
            || lower.contains("to choose");
        names_a_confirm_action && lower.contains("cancel")
    });
    if has_selection_footer {
        return true;
    }

    let numbered_option_lines = lines
        .iter()
        .filter(|line| is_numbered_option_line(line))
        .count();
    let has_cursor_on_option_line = lines
        .iter()
        .any(|line| line.trim_start().starts_with('\u{276f}') && is_numbered_option_line(line));
    numbered_option_lines >= 2 && has_cursor_on_option_line
}

/// True if `line` starts (after an optional `❯` cursor and leading
/// whitespace) with a small integer followed by `". "` -- e.g. "1. Source
/// only" or "  2. Write full list to file".
fn is_numbered_option_line(line: &str) -> bool {
    let trimmed = line
        .trim_start()
        .trim_start_matches('\u{276f}')
        .trim_start();
    let digits_end = trimmed.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    if digits_end == 0 {
        return false;
    }
    trimmed[digits_end..].starts_with(". ")
}

/// Refreshes the system-wide process list (pid/parent/name only) on the
/// given `System`. Call this once per detection tick (shared across all
/// panes), not once per pane -- the tick's timing/scheduling/adaptive
/// backoff is the caller's responsibility, not this crate's.
///
/// Deliberately cheap: `identify_agent`'s tree walk only needs pid/parent
/// chains and process names, which sysinfo always populates. `cwd` and
/// `environ` are per-process filesystem reads (`readlink`/`open`+`read`
/// under `/proc`) and cost proportionally to *every* process on the
/// machine if fetched here -- entirely wasted on the >99% of processes
/// that are never an agent CLI. Session-ID discovery (which does need
/// those fields, scoped to just the one matched pid) is app-level
/// orchestration that lives above this crate.
pub fn refresh(system: &mut System) {
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
}

/// Given the OS pid of a pane's directly-spawned child (typically the
/// user's shell), walks the process tree looking for a descendant process
/// whose name matches a known agent CLI signature (see
/// [`AGENT_SIGNATURES`]), and returns the first match found (breadth-first
/// over `system.processes()`, matching each process's `parent()` back up
/// its ancestor chain to `shell_pid`).
///
/// Returns `None` if no descendant process matches.
///
/// Takes `&System` (already refreshed by the caller via [`refresh`], or
/// equivalent) rather than owning any refresh itself -- session-ID
/// discovery, which does need a targeted `cwd`/`environ` refresh on the
/// matched pid, is the caller's job.
pub fn identify_agent(system: &System, shell_pid: Pid) -> Option<AgentIdentity> {
    system
        .processes()
        .values()
        .filter_map(|process| {
            let depth = descendant_depth(system, process.pid(), shell_pid)?;
            let class = classify_process_name(&process.name().to_string_lossy().to_lowercase())?;
            Some((depth, process.pid(), class))
        })
        // The CLI process is closer to the pane shell than its internal
        // helper processes (for example Codex's code-mode host).
        .min_by_key(|(depth, pid, _)| (*depth, pid.as_u32()))
        .map(|(_, pid, class)| AgentIdentity {
            pid: pid.as_u32(),
            class,
        })
}

/// Classifies a single (already-lowercased) process name against the
/// [`AGENT_SIGNATURES`] registry.
fn classify_process_name(lowercase_name: &str) -> Option<AgentClass> {
    AGENT_SIGNATURES
        .iter()
        .find(|signature| lowercase_name.contains(signature.name_substring))
        .map(|signature| (signature.class_of)(lowercase_name))
}

/// Returns the number of parent links from `pid` to `root`, or `None` when
/// it is outside that pane's process tree.
fn descendant_depth(system: &System, pid: Pid, root: Pid) -> Option<usize> {
    let mut current = Some(pid);
    // Process trees are finite and shallow in practice; a visited set
    // guards against any (unexpected) parent cycle from a stale snapshot.
    let mut visited = HashSet::new();
    let mut depth = 0;
    while let Some(current_pid) = current {
        if current_pid == root {
            return Some(depth);
        }
        if !visited.insert(current_pid) {
            return None;
        }
        depth += 1;
        current = system.process(current_pid).and_then(|p| p.parent());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads a captured screen-text fixture from `tests/fixtures/`.
    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read fixture {path}: {error}");
        })
    }

    #[test]
    fn claude_code_mid_turn_is_working() {
        assert_eq!(
            classify_activity(&fixture("claude_code_working.txt")),
            AgentActivity::Working
        );
    }

    #[test]
    fn claude_code_idle_prompt_is_idle() {
        assert_eq!(
            classify_activity(&fixture("claude_code_idle.txt")),
            AgentActivity::Idle
        );
    }

    #[test]
    fn claude_code_awaiting_approval_is_waiting_approval() {
        assert_eq!(
            classify_activity(&fixture("claude_code_awaiting_approval.txt")),
            AgentActivity::WaitingApproval
        );
    }

    #[test]
    fn codex_mid_turn_is_working() {
        assert_eq!(
            classify_activity(&fixture("codex_working.txt")),
            AgentActivity::Working
        );
    }

    #[test]
    fn codex_idle_prompt_is_idle() {
        assert_eq!(
            classify_activity(&fixture("codex_idle.txt")),
            AgentActivity::Idle
        );
    }

    #[test]
    fn codex_awaiting_approval_is_waiting_approval() {
        assert_eq!(
            classify_activity(&fixture("codex_awaiting_approval.txt")),
            AgentActivity::WaitingApproval
        );
    }

    #[test]
    fn prose_mentioning_yes_and_no_is_idle_not_waiting_approval() {
        assert_eq!(
            classify_activity(&fixture("claude_code_prose_with_yes_no.txt")),
            AgentActivity::Idle
        );
    }

    #[test]
    fn rhetorical_question_with_not_is_not_waiting_approval() {
        assert_eq!(
            classify_activity("Does that make sense, or not?"),
            AgentActivity::Idle
        );
    }

    #[test]
    fn numbered_analysis_with_stray_cursor_elsewhere_is_idle() {
        assert_eq!(
            classify_activity(&fixture(
                "claude_code_numbered_analysis_with_stray_cursor.txt"
            )),
            AgentActivity::Idle
        );
    }

    #[test]
    fn plain_shell_prompt_has_no_activity_signal() {
        assert_eq!(
            classify_activity(&fixture("plain_shell.txt")),
            AgentActivity::Idle
        );
    }

    #[test]
    fn classify_process_name_matches_known_signatures() {
        assert_eq!(classify_process_name("claude"), Some(AgentClass::Claude));
        assert_eq!(classify_process_name("codex"), Some(AgentClass::Codex));
        assert_eq!(
            classify_process_name("opencode"),
            Some(AgentClass::Other("opencode".to_string()))
        );
        assert_eq!(
            classify_process_name("aider"),
            Some(AgentClass::Other("aider".to_string()))
        );
        assert_eq!(classify_process_name("bash"), None);
    }

    /// `identify_agent` walks a *real* process tree (sysinfo has no fake
    /// backend to inject a synthetic one), so the meaningful thing this
    /// integration-style test can assert without a real agent CLI
    /// installed is the negative case: the current test process's own
    /// pid has no `claude`/`codex`/etc. descendant, so it must return
    /// `None` rather than panicking or false-matching.
    #[test]
    fn identify_agent_returns_none_when_no_agent_descendant_exists() {
        let mut system = System::new_all();
        system.refresh_all();
        let current_pid = Pid::from_u32(std::process::id());
        assert_eq!(identify_agent(&system, current_pid), None);
    }
}
