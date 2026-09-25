//! Agent detection: two independent, pure signals about a terminal pane.
//!
//! `identify_agent` walks an already-populated `sysinfo::System` process
//! tree to answer "which agent CLI (if any) is running below this pane's
//! shell" -- the *identity* signal, driven by the first-party
//! `BuiltinAgentProvider::ALL` table plus this crate's own
//! [`GENERIC_AGENT_SIGNATURES`] registry. `classify_activity` scans the
//! pane's rendered
//! plain-text screen contents to answer "is that agent working, blocked on
//! a confirmation, or idle" -- the *activity* signal.
//!
//! Both functions are pure: `classify_activity` takes only a `&str`, and
//! `identify_agent` takes a `&System` the caller has already refreshed
//! (via [`refresh`]) plus a process list it has already scanned. Neither
//! function owns a polling loop, a PTY, or any filesystem/`/proc` access
//! of its own -- that I/O (adaptive-interval scheduling, and the
//! I/O-heavy session-ID discovery that reads `/proc/<pid>/fd` and scans
//! transcript files) belongs to the caller (`ilium-server`'s detection
//! loop, currently `ilium`'s `app.rs` during the strangler-fig
//! migration), not to this crate.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use ilium_core::{AgentActivity, AgentClass, AgentProvider, BuiltinAgentProvider, GoalState};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use unicode_width::UnicodeWidthChar;

/// Ensures process detection never consumes the host's file-descriptor budget.
///
/// `sysinfo` otherwise keeps one `/proc/<pid>/stat` descriptor open for up to
/// half the process limit. A long-lived ilium server only refreshes processes
/// occasionally, so retaining thousands of descriptors between ticks costs
/// substantially more than reopening the handful of files it needs.
pub fn configure_process_refresh() {
    #[cfg(target_os = "linux")]
    {
        let _ = sysinfo::set_open_files_limit(0);
    }
}

/// One entry in the agent-identity registry: a lowercase substring to
/// match against a process name, and how to build the resulting
/// [`AgentClass`] from the matched (lowercased) name.
///
/// New agent CLI support is a new entry here, not a new branch in an
/// if/else chain -- see `CLAUDE.md`'s layering rule. `pub` (and its fields
/// `pub`) so a caller can also build its own signatures at runtime -- see
/// [`identify_agent_with_extra`] -- from user config
/// (`ilium-server/src/config.rs`'s `[[detection.custom_signatures]]`)
/// rather than that config surface needing a parallel matching code path.
///
/// `name_substring` is `Cow<'static, str>` rather than plain `&'static
/// str` so the same type serves both the compile-time [`GENERIC_AGENT_SIGNATURES`]
/// table (`Cow::Borrowed`) and signatures built at runtime from an owned
/// `String` read out of a config file (`Cow::Owned`), with no separate
/// "config signature" type needed. `class_of` stays a plain `fn` pointer
/// (not a boxed closure) since every signature -- built-in or
/// config-provided -- only ever needs one of three fixed shapes (always
/// `Claude`, always `Codex`, or `Other` carrying whatever process name
/// actually matched); a non-capturing closure literal coerces to `fn`
/// automatically, so config-provided signatures build these the same way
/// the built-in ones do, no dynamic dispatch required.
///
/// Deliberately does not derive `PartialEq`/`Eq`: `class_of` is a `fn`
/// pointer, and comparing those is documented as unreliable (their
/// addresses aren't guaranteed stable across codegen units) -- nothing in
/// this crate or its callers needs to compare two signatures for equality,
/// so there's no reason to take on that footgun.
#[derive(Debug, Clone)]
pub struct AgentSignature {
    /// Lowercase substring matched against a process's name.
    pub name_substring: Cow<'static, str>,
    /// Builds the `AgentClass` for a match. Receives the matched
    /// (lowercased) process name so `AgentClass::Other` can carry the
    /// exact name that matched.
    pub class_of: fn(matched_name: &str) -> AgentClass,
}

/// Built-in generic signatures that do not expose a launch/resume/session
/// contract. First-party providers live in `BuiltinAgentProvider::ALL`, so a
/// new supported CLI is registered exactly once in `ilium-core` rather than
/// being copied into detection, launch, and persistence tables.
const GENERIC_AGENT_SIGNATURES: &[AgentSignature] = &[
    AgentSignature {
        name_substring: Cow::Borrowed("opencode"),
        class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
    },
    AgentSignature {
        name_substring: Cow::Borrowed("aider"),
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
    /// Exact executable/process name that matched the registry, retained so
    /// diagnostics can explain the identity decision without re-reading the
    /// live process table later.
    pub process_name: String,
    /// Registry substring that matched `process_name`.
    pub matched_signature: String,
    /// Number of process-tree edges between the pane shell and the matched
    /// agent process (`0` when the directly spawned child is the agent).
    pub process_tree_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityEvidence {
    InterruptMarker,
    GenericLiveStatus,
    ClaudeLiveStatus,
    CodexLiveStatus,
    BackgroundWait,
    BackgroundTaskWait,
    ConfirmationPrompt,
    SelectionPrompt,
    NoActiveMarker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityClassification {
    pub activity: AgentActivity,
    pub evidence: ActivityEvidence,
    /// One bounded, control-character-free terminal-chrome line that produced
    /// the positive classification. `None` for the negative `NoActiveMarker`
    /// conclusion, which is explained by the checked marker families instead.
    pub matched_line: Option<String>,
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
/// the rule unambiguous either way. Next, a background-wait line
/// (`looks_like_background_wait_line`) means the agent dispatched
/// subagents/background tasks and is actively blocked mid-turn waiting on
/// them, not streaming foreground output. Distinct from that,
/// `looks_like_background_task_wait_line` catches Claude Code's
/// completed-turn summary line (e.g. "Cogitated for 3m 11s") growing a
/// "· 1 shell still running" (or "monitor", or any other noun the CLI's
/// wording uses) suffix while something it started in the background is
/// still executing -- the turn itself already wrapped up, so this reads as
/// `BackgroundTaskStillRunning` rather than the actively-blocked
/// `WaitingBackground`; without this check that pane would misreport as
/// finished (and the server's `promote_to_done` would mark it `Done`) while
/// real work is still in flight. Absent both, either a y/n-style
/// confirmation box or a general multiple-choice/question prompt (see
/// `looks_like_confirmation_prompt` and `looks_like_selection_prompt`)
/// means the agent is blocked waiting on the user. Anything else is
/// `Idle`.
pub fn classify_activity(screen_text: &str) -> AgentActivity {
    classify_activity_detailed(screen_text).activity
}

pub fn classify_activity_detailed(screen_text: &str) -> ActivityClassification {
    classify_screen_activity(None, screen_text)
}

/// Classifies activity with the detected provider's status-line vocabulary.
///
/// Claude Code's current live status uses a whimsical verb, a Unicode ellipsis,
/// and an elapsed-time token. Codex also renders elapsed times in completed
/// transcript rows, so applying that shape to every provider turns finished
/// Codex panes back into `Working`. Provider-specific status recognition keeps
/// the shared activity contract while isolating each CLI's volatile UI text.
pub fn classify_activity_for_agent(class: &AgentClass, screen_text: &str) -> AgentActivity {
    classify_activity_for_agent_detailed(class, screen_text).activity
}

pub fn classify_activity_for_agent_detailed(
    class: &AgentClass,
    screen_text: &str,
) -> ActivityClassification {
    classify_screen_activity(Some(class), screen_text)
}

/// The single activity-precedence chain both public entry points run.
///
/// `class` is `None` when no agent CLI has been identified for the pane.
/// Everything except the live-status rule is provider-independent, so the
/// precedence order lives here once: duplicating the chain per entry point is
/// exactly how the generic and provider-aware paths would silently drift apart
/// when a new marker family is added to one of them.
fn classify_screen_activity(
    class: Option<&AgentClass>,
    screen_text: &str,
) -> ActivityClassification {
    let (activity, evidence) = if screen_text.contains(WORKING_MARKER) {
        (AgentActivity::Working, ActivityEvidence::InterruptMarker)
    } else if let Some(live_status) = live_status_evidence(class, screen_text) {
        (AgentActivity::Working, live_status)
    } else if looks_like_background_wait_line(screen_text) {
        (
            AgentActivity::WaitingBackground,
            ActivityEvidence::BackgroundWait,
        )
    } else if looks_like_background_task_wait_line(screen_text) {
        (
            AgentActivity::BackgroundTaskStillRunning,
            ActivityEvidence::BackgroundTaskWait,
        )
    } else if looks_like_confirmation_prompt(screen_text) {
        (
            AgentActivity::WaitingApproval,
            ActivityEvidence::ConfirmationPrompt,
        )
    } else if looks_like_selection_prompt(screen_text) {
        (
            AgentActivity::WaitingApproval,
            ActivityEvidence::SelectionPrompt,
        )
    } else {
        (AgentActivity::Idle, ActivityEvidence::NoActiveMarker)
    };
    ActivityClassification {
        activity,
        evidence,
        matched_line: activity_evidence_line(evidence, screen_text),
    }
}

/// Which "this agent is mid-turn" status-line rule applies, given what is
/// known about the provider rendering the screen.
///
/// The generic ellipsis-plus-elapsed-time shape is the right default when no
/// agent CLI has been identified, but it must never be applied to Codex: Codex
/// keeps completed timing summaries (which carry both an ellipsis and an
/// elapsed-time token) on screen, so the generic shape would turn every
/// finished Codex pane back into `Working`. Providers with no recognized live
/// status line fall through to the shared marker families instead of guessing.
fn live_status_evidence(class: Option<&AgentClass>, screen_text: &str) -> Option<ActivityEvidence> {
    match class {
        None => {
            looks_like_live_status_line(screen_text).then_some(ActivityEvidence::GenericLiveStatus)
        }
        Some(AgentClass::Claude) => {
            looks_like_live_status_line(screen_text).then_some(ActivityEvidence::ClaudeLiveStatus)
        }
        Some(AgentClass::Codex) => looks_like_codex_live_status_line(screen_text)
            .then_some(ActivityEvidence::CodexLiveStatus),
        Some(AgentClass::Antigravity | AgentClass::Other(_)) => None,
    }
}

/// Recovers the exact short terminal-chrome line behind a positive evidence
/// code. This intentionally runs after the cheap classifier has selected one
/// rule, keeping the public evidence complete without making every predicate
/// allocate while it searches.
///
/// Every arm reuses the very predicate the classifier ran, so the reported
/// evidence line can never describe a different rule than the one that
/// actually fired -- re-stating a predicate inline here is how the two copies
/// drift apart.
fn activity_evidence_line(evidence: ActivityEvidence, screen_text: &str) -> Option<String> {
    let line_matches_evidence: fn(&str) -> bool = match evidence {
        ActivityEvidence::InterruptMarker => is_interrupt_marker_line,
        ActivityEvidence::GenericLiveStatus | ActivityEvidence::ClaudeLiveStatus => {
            is_live_status_line
        }
        ActivityEvidence::CodexLiveStatus => is_codex_live_status_line,
        ActivityEvidence::BackgroundWait => is_background_wait_line,
        ActivityEvidence::BackgroundTaskWait => is_background_task_wait_line,
        ActivityEvidence::ConfirmationPrompt => is_confirmation_prompt_line,
        ActivityEvidence::SelectionPrompt => is_selection_prompt_line,
        ActivityEvidence::NoActiveMarker => return None,
    };
    screen_text
        .lines()
        .find(|line| line_matches_evidence(line))
        .map(bounded_terminal_evidence)
}

/// Keeps durable diagnostic excerpts useful and safe for terminal rendering.
/// The excerpt is evidence, not a transcript dump.
fn bounded_terminal_evidence(line: &str) -> String {
    const MAXIMUM_EVIDENCE_CHARACTERS: usize = 240;

    let mut evidence = String::new();
    for character in line.trim().chars().take(MAXIMUM_EVIDENCE_CHARACTERS) {
        evidence.push(if character.is_control() {
            '\u{fffd}'
        } else {
            character
        });
    }
    if line.trim().chars().count() > MAXIMUM_EVIDENCE_CHARACTERS {
        evidence.push('…');
    }
    evidence
}

/// Returns whether a detected first-party agent is visibly at the start of
/// a fresh conversation. This is intentionally stricter than `Idle`: an
/// agent that has merely finished a turn also shows an empty composer, and
/// clearing that pane's useful title would be a false positive.
///
/// The universal cleared/new-conversation notice catches the explicit state
/// each provider renders immediately after `/clear`. Codex also has a stable
/// empty-composer screen with no transcript text, so it remains detectable
/// after that transient notice disappears. Unknown/custom agents fail closed.
pub fn is_fresh_agent_screen(class: &AgentClass, screen_text: &str) -> bool {
    let normalized = screen_text.to_ascii_lowercase();
    if [
        "conversation cleared",
        "conversation reset",
        "new conversation started",
        "started a new conversation",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return matches!(
            class,
            AgentClass::Claude | AgentClass::Codex | AgentClass::Antigravity
        );
    }

    matches!(class, AgentClass::Codex)
        && normalized.contains("send a message")
        && screen_has_only_empty_composer_chrome(screen_text, "send a message")
}

/// Returns whether a detected agent visibly exposes its normal free-form
/// composer. This is deliberately a narrower contract than `Idle`: an agent
/// can be idle while still rendering an onboarding step, a permission choice,
/// or another modal that would consume an injected task as the wrong input.
///
/// The server uses this only for the one-shot initial prompt attached to a
/// newly-created agent pane. Unknown/custom agents fail closed because their
/// prompt chrome is not a stable contract ilium can safely infer.
///
/// The composer-visible check alone is not enough for any provider: some
/// providers (Codex confirmed) keep their composer hint rendered on screen
/// underneath an approval dialog or other modal, so a provider-specific
/// composer marker must still be combined with the shared activity gate --
/// otherwise a one-shot initial prompt gets injected into the modal instead
/// of the composer it was meant for.
///
/// This text-only entry point deliberately accepts only Codex composers whose
/// emptiness is unambiguous in plain text. Current Codex releases draw rotating
/// placeholder text after `›`; once terminal formatting is stripped, that is
/// indistinguishable from a user-authored draft. Callers with a live terminal
/// cursor and cell styling should use [`is_agent_prompt_ready_at_cursor`]
/// instead.
pub fn is_agent_prompt_ready(class: &AgentClass, screen_text: &str) -> bool {
    let normalized = screen_text.to_ascii_lowercase();
    let composer_visible = match class {
        AgentClass::Codex => screen_has_unambiguously_empty_codex_composer(screen_text),
        AgentClass::Claude => screen_has_claude_composer_cursor(screen_text),
        AgentClass::Antigravity => normalized.contains("type a message"),
        AgentClass::Other(_) => return false,
    };
    composer_visible
        && !matches!(
            classify_activity_for_agent(class, screen_text),
            AgentActivity::Working
                | AgentActivity::WaitingBackground
                | AgentActivity::WaitingApproval
        )
}

/// Cursor-aware prompt readiness for callers that own a live terminal screen.
///
/// Codex renders rotating placeholder text in the same cells a typed draft
/// later occupies. Plain text therefore cannot distinguish `› Explain this
/// codebase` as a placeholder from the same words entered by a user. The
/// terminal cursor is one necessary signal: for an empty composer it remains
/// at the first input cell immediately after `› `, while ordinary typed input
/// advances it. It is not sufficient by itself because a user can move a dirty
/// draft back to the first cell. Codex renders placeholder text dim and
/// user-authored text normally, so every visible placeholder cell must also be
/// present in `dimmed_cells`. Rows and columns are zero-based, matching
/// `vt100::Screen::cursor_position`. Other providers retain their existing
/// text-only contracts.
pub fn is_agent_prompt_ready_at_cursor(
    class: &AgentClass,
    screen_text: &str,
    cursor_row: u16,
    cursor_column: u16,
    dimmed_cells: &[(u16, u16)],
) -> bool {
    if !matches!(class, AgentClass::Codex) {
        return is_agent_prompt_ready(class, screen_text);
    }

    let composer_visible = screen_has_unambiguously_empty_codex_composer(screen_text)
        || screen_has_empty_codex_composer_at_cursor(
            screen_text,
            cursor_row,
            cursor_column,
            dimmed_cells,
        );
    composer_visible
        && !matches!(
            classify_activity_for_agent(class, screen_text),
            AgentActivity::Working
                | AgentActivity::WaitingBackground
                | AgentActivity::WaitingApproval
        )
}

/// One known one-time interstitial dialog a first-party agent CLI shows
/// outside its normal turn lifecycle -- distinct from
/// `classify_activity_for_agent`'s ongoing working/idle/approval states
/// because dialogs like this appear once, only at session (re)start, before
/// any turn exists to classify. A new known dialog is a new entry in
/// `INTERSTITIAL_PROMPTS`, not a new branch in an if/else chain -- same
/// registry shape as `GENERIC_AGENT_SIGNATURES`.
struct InterstitialPrompt {
    class: AgentClass,
    /// Every one of these substrings must appear in the bottom
    /// `INTERSTITIAL_PROMPT_ANCHOR_ROWS` rows of the screen for this prompt
    /// to match. Position, not phrasing alone, is what keeps this dialog's
    /// own wording -- which can legitimately appear elsewhere, e.g. quoted in
    /// an agent's transcript -- from producing a false positive.
    anchors: &'static [&'static str],
    /// The literal key sent to the pty to answer this prompt. No Enter
    /// follows -- Claude Code's numbered-choice prompts commit on the digit
    /// alone (verified live: pressing `2` with no Enter instantly resumed a
    /// full session).
    key_to_send: &'static str,
}

/// How many trailing screen rows are searched for interstitial-prompt
/// anchors. Wide enough to comfortably hold the whole dialog box (currently
/// 8 rows for the resume-session prompt) with margin for terminal-size
/// variance, narrow enough to exclude scrolled-off transcript text that
/// happens to quote the same wording.
const INTERSTITIAL_PROMPT_ANCHOR_ROWS: usize = 10;

const INTERSTITIAL_PROMPTS: &[InterstitialPrompt] = &[
    // Claude Code's "resume a large/old session" dialog, shown when resuming
    // a saved transcript that would consume a large share of usage limits.
    // Captured live via `claude --resume` on a 470k-token session
    // (2026-08-07); see `tests/fixtures/claude_code_resume_full_session_prompt.txt`.
    InterstitialPrompt {
        class: AgentClass::Claude,
        anchors: &[
            "Resume full session as-is",
            "Don't ask me again",
            "Enter to confirm",
        ],
        key_to_send: "2",
    },
];

/// Returns the key to send to answer a known one-time interstitial dialog
/// currently on screen for `class`, or `None` if no known dialog matches.
/// Callers own actually writing the key to the pty and any one-shot/latch
/// bookkeeping needed to send it at most once per dialog appearance -- this
/// function is a pure screen-text query with no memory of what it answered
/// last tick.
pub fn interstitial_prompt_response(class: &AgentClass, screen_text: &str) -> Option<&'static str> {
    let lines: Vec<&str> = screen_text.lines().collect();
    let tail_start = lines.len().saturating_sub(INTERSTITIAL_PROMPT_ANCHOR_ROWS);
    let tail = lines[tail_start..].join("\n");
    INTERSTITIAL_PROMPTS
        .iter()
        .find(|prompt| {
            prompt.class == *class && prompt.anchors.iter().all(|anchor| tail.contains(anchor))
        })
        .map(|prompt| prompt.key_to_send)
}

/// Recognizes Codex composer forms whose emptiness survives conversion to
/// plain text: a bare modern `›` prompt or the older empty bordered field.
fn screen_has_unambiguously_empty_codex_composer(screen_text: &str) -> bool {
    let lines: Vec<&str> = screen_text.lines().collect();
    lines.iter().enumerate().any(|(line_index, line)| {
        let trimmed = line.trim_start();
        if let Some(composer_content) = trimmed.strip_prefix('›') {
            return composer_content.trim().is_empty();
        }
        trimmed.eq_ignore_ascii_case("send a message")
            && codex_legacy_composer_box_is_empty(&lines[..line_index])
    })
}

/// Recognizes an empty modern Codex composer by requiring the live cursor to
/// remain at its first input cell. Placeholder wording is intentionally not
/// enumerated because Codex rotates it between releases.
fn screen_has_empty_codex_composer_at_cursor(
    screen_text: &str,
    cursor_row: u16,
    cursor_column: u16,
    dimmed_cells: &[(u16, u16)],
) -> bool {
    let Some(line) = screen_text.lines().nth(usize::from(cursor_row)) else {
        return false;
    };
    let leading_space_columns = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let Some(composer_content) = line
        .get(leading_space_columns..)
        .and_then(|trimmed| trimmed.strip_prefix('›'))
    else {
        return false;
    };
    if is_numbered_option_line(composer_content) {
        return false;
    }

    // `Screen::contents()` drops trailing blank cells. Accept either the cell
    // directly after `›` or the conventional first input cell after `› `.
    let first_column_after_marker = leading_space_columns.saturating_add(1);
    let first_input_column = first_column_after_marker.saturating_add(1);
    let cursor_is_at_first_input = usize::from(cursor_column) == first_column_after_marker
        || usize::from(cursor_column) == first_input_column;
    if !cursor_is_at_first_input {
        return false;
    }

    let mut column = first_column_after_marker;
    let mut saw_visible_placeholder_cell = false;
    for character in composer_content.chars() {
        let character_width = character.width().unwrap_or(0);
        if !character.is_whitespace() && character_width > 0 {
            saw_visible_placeholder_cell = true;
            let Ok(column) = u16::try_from(column) else {
                return false;
            };
            if !dimmed_cells.contains(&(cursor_row, column)) {
                return false;
            }
        }
        column = column.saturating_add(character_width);
    }

    saw_visible_placeholder_cell
}

fn codex_legacy_composer_box_is_empty(lines_before_label: &[&str]) -> bool {
    let mut preceding_lines = lines_before_label
        .iter()
        .rev()
        .map(|line| line.trim())
        .skip_while(|line| line.is_empty());
    let Some(bottom_border) = preceding_lines.next() else {
        return false;
    };
    if !is_codex_box_border(bottom_border, '╰', '╯') {
        return false;
    }

    let mut saw_empty_content_row = false;
    for line in preceding_lines {
        if is_codex_box_border(line, '╭', '╮') {
            return saw_empty_content_row;
        }
        if !is_empty_codex_box_row(line) {
            return false;
        }
        saw_empty_content_row = true;
    }
    false
}

fn is_codex_box_border(line: &str, left_corner: char, right_corner: char) -> bool {
    line.starts_with(left_corner)
        && line.ends_with(right_corner)
        && line.chars().all(|character| {
            matches!(character, '─' | ' ') || character == left_corner || character == right_corner
        })
}

fn is_empty_codex_box_row(line: &str) -> bool {
    line.starts_with('│')
        && line.ends_with('│')
        && line.chars().all(|character| matches!(character, '│' | ' '))
}

/// Claude's empty composer is a bordered terminal field whose content row is
/// just `>` (older builds) or `❯` (current builds). Requiring that isolated
/// row avoids confusing a prose quote or a numbered selector with a composer.
fn screen_has_claude_composer_cursor(screen_text: &str) -> bool {
    screen_text.lines().any(|line| {
        let content = line
            .trim()
            .trim_matches(|character| matches!(character, '│' | '|' | ' '))
            .trim();
        matches!(content, ">" | "❯")
    })
}

/// Ignores the box drawing around a known empty composer, then rejects any
/// remaining user/agent text. A completed Codex turn keeps transcript text
/// above its composer, so it cannot satisfy this deliberately narrow rule.
fn screen_has_only_empty_composer_chrome(screen_text: &str, composer_label: &str) -> bool {
    screen_text.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case(composer_label)
            || trimmed
                .chars()
                .all(|character| matches!(character, '╭' | '╮' | '╰' | '╯' | '─' | '│' | ' '))
    })
}

/// Provider evidence about a persistent task goal in one visible-screen sample.
///
/// `Unknown` is deliberately distinct from `Inactive`: a provider can clear or
/// replace its footer while an approval dialog, help overlay, resize, or
/// multi-chunk redraw is in progress. The server retains an already-confirmed
/// goal for the same agent process across those inconclusive samples and clears
/// it only on explicit terminal evidence or process replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalEvidence {
    State(GoalState),
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalClassification {
    pub evidence: GoalEvidence,
    pub matched_line: Option<String>,
}

/// Extracts provider-owned goal evidence from the current visible screen.
///
/// Goal status is read only from the provider's own status chrome next to its
/// composer, never from arbitrary transcript text: agent prose and tool output
/// routinely quote goal wording (`Goal paused`, `Pursuing goal (5m)`), and a
/// stale transcript row must not outrank the live footer. Each provider has an
/// anchor row located structurally relative to its composer; when the anchor
/// cannot be located (overlay, approval dialog, partial redraw) the result is
/// `Unknown` so the server can retain the previously confirmed phase.
///
/// See `docs/research/agent-goal-indicators.md` for the captured layouts.
pub fn goal_evidence_for_agent(class: &AgentClass, screen_text: &str) -> GoalEvidence {
    goal_evidence_for_agent_detailed(class, screen_text).evidence
}

pub fn goal_evidence_for_agent_detailed(
    class: &AgentClass,
    screen_text: &str,
) -> GoalClassification {
    // Resolved once per screen: a provider with no goal surface skips the
    // line collection entirely on every detection tick.
    let Some(goal_evidence_of_screen) = provider_goal_reader(class) else {
        return GoalClassification {
            evidence: GoalEvidence::Unknown,
            matched_line: None,
        };
    };
    let lines: Vec<&str> = screen_text.lines().collect();
    goal_evidence_of_screen(&lines)
}

/// The provider-owned goal reader, or `None` for a provider that renders no
/// goal status at all.
fn provider_goal_reader(class: &AgentClass) -> Option<fn(&[&str]) -> GoalClassification> {
    match class {
        AgentClass::Codex => Some(codex_goal_classification),
        AgentClass::Claude => Some(claude_goal_classification),
        AgentClass::Antigravity | AgentClass::Other(_) => None,
    }
}

fn goal_classification(evidence: GoalEvidence, line: Option<&str>) -> GoalClassification {
    GoalClassification {
        evidence,
        matched_line: line.map(bounded_terminal_evidence),
    }
}

/// How far above Codex's footer its composer may start. The composer grows
/// with a multi-line draft; anything further away is not the live composer.
const CODEX_COMPOSER_SEARCH_ROWS: usize = 10;

/// Codex renders its status footer as the last non-blank screen row, directly
/// below the `›` composer. The goal segment is right-pinned in that row: wide
/// layouts truncate the metadata on its left with `…` and may append a
/// `⚠ 1 warning · f2 to view` notice to its right, so the segment is found by
/// marker, not by line position.
fn codex_goal_classification(lines: &[&str]) -> GoalClassification {
    let Some(footer) = codex_footer_row(lines) else {
        return goal_classification(GoalEvidence::Unknown, None);
    };
    let normalized_footer = footer.to_lowercase();
    if let Some(goal_state) = codex_footer_goal_state(&normalized_footer) {
        return goal_classification(GoalEvidence::State(goal_state), Some(footer));
    }
    // A goal-free footer is decisive only when it is recognizably Codex's
    // metadata status line. Popups that replace the footer (slash-command
    // lists, key hints under a selection menu) stay inconclusive so a
    // transient overlay never clears a confirmed goal.
    if is_codex_metadata_footer(&normalized_footer) {
        return goal_classification(GoalEvidence::Inactive, Some(footer));
    }
    goal_classification(GoalEvidence::Unknown, None)
}

/// The footer row, provided the live composer sits a few rows above it.
fn codex_footer_row<'screen>(lines: &[&'screen str]) -> Option<&'screen str> {
    let footer_index = lines.iter().rposition(|line| !line.trim().is_empty())?;
    let search_start = footer_index.saturating_sub(CODEX_COMPOSER_SEARCH_ROWS);
    lines[search_start..footer_index]
        .iter()
        .any(|line| is_codex_composer_row(line))
        .then_some(lines[footer_index])
}

/// The composer row starts with `›`. Numbered menu options use the same
/// selection glyph (`› 1. Trust and continue`) and are rejected so a modal's
/// hint row is not mistaken for the footer.
fn is_codex_composer_row(line: &str) -> bool {
    let Some(after_glyph) = line.trim_start().strip_prefix('›') else {
        return false;
    };
    let content = after_glyph.trim_start();
    let digit_count = content.chars().take_while(char::is_ascii_digit).count();
    !(digit_count > 0 && content[digit_count..].starts_with('.'))
}

/// Every goal phase Codex renders in its footer, as extracted from the Codex
/// CLI 0.156 binary and confirmed live where noted in the research document.
/// Only the rightmost marker counts: the goal segment is the footer's final
/// status segment.
fn codex_footer_goal_state(normalized_footer: &str) -> Option<GoalState> {
    const ELAPSED_MARKERS: [(&str, GoalState); 3] = [
        ("pursuing goal (", GoalState::Active),
        ("goal achieved (", GoalState::Reached),
        // Budget-limited goals: the thread's token budget ran out.
        ("goal unmet (", GoalState::UsageLimited),
    ];
    const EXACT_MARKERS: [(&str, GoalState); 4] = [
        ("goal paused (/goal resume)", GoalState::Paused),
        ("goal stalled (/goal resume)", GoalState::Blocked),
        // Older Codex releases named the stalled phase "blocked".
        ("goal blocked (/goal resume)", GoalState::Blocked),
        (
            "goal hit usage limits (/goal resume)",
            GoalState::UsageLimited,
        ),
    ];

    let elapsed_matches = ELAPSED_MARKERS.iter().filter_map(|(marker, goal_state)| {
        rightmost_segment_start(normalized_footer, marker)
            .filter(|&index| has_elapsed_token_after(normalized_footer, index + marker.len()))
            .map(|index| (index, *goal_state))
    });
    let exact_matches = EXACT_MARKERS.iter().filter_map(|(marker, goal_state)| {
        rightmost_segment_start(normalized_footer, marker).map(|index| (index, *goal_state))
    });
    elapsed_matches
        .chain(exact_matches)
        .max_by_key(|(index, _)| *index)
        .map(|(_, goal_state)| goal_state)
}

/// The last occurrence of `marker` that begins a word, so `repursuing goal (`
/// cannot match inside a longer token.
fn rightmost_segment_start(line: &str, marker: &str) -> Option<usize> {
    line.rmatch_indices(marker)
        .map(|(index, _)| index)
        .find(|&index| {
            line[..index]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_alphanumeric())
        })
}

/// Accepts `16m)`, `1h 5m)`, `45s)` — a digit-bearing elapsed duration closed
/// by a parenthesis.
fn has_elapsed_token_after(line: &str, duration_start: usize) -> bool {
    let tail = &line[duration_start..];
    let Some(closing_parenthesis) = tail.find(')') else {
        return false;
    };
    let duration = &tail[..closing_parenthesis];
    duration.chars().any(|character| character.is_ascii_digit())
        && duration.chars().all(|character| {
            character.is_ascii_digit()
                || character.is_ascii_whitespace()
                || matches!(character, '.' | ':' | 'd' | 'h' | 'm' | 's')
        })
}

/// Codex's status line is a `·`-separated metadata row that carries at least
/// one of its run-state or context items.
fn is_codex_metadata_footer(normalized_footer: &str) -> bool {
    const STATUS_ITEMS: [&str; 7] = [
        "context", "% left", "ready", "working", "idle", "waiting", "thinking",
    ];
    normalized_footer.contains(" · ")
        && STATUS_ITEMS
            .iter()
            .any(|item| normalized_footer.contains(item))
}

/// How far above Claude Code's indicator row the current turn's closing
/// notices are searched. The turn summary, a goal notice, and an optional
/// feedback box fit well inside this bound.
const CLAUDE_TURN_END_SEARCH_ROWS: usize = 30;

/// Claude Code renders `◎ /goal active (2m)` right-aligned on the indicator
/// row directly above the composer's top rule for as long as a goal is set.
/// That row cannot express any other phase, so the closing notices of the
/// current turn — the rows between the indicator and the last user prompt —
/// refine it: `Goal paused · <reason>` while the indicator persists, and
/// `Goal achieved` / `Goal could not be achieved` / `Goal cleared` once it is
/// gone.
fn claude_goal_classification(lines: &[&str]) -> GoalClassification {
    let Some(indicator_index) = claude_indicator_row_index(lines) else {
        return goal_classification(GoalEvidence::Unknown, None);
    };
    let indicator = lines[indicator_index];
    let turn_end = claude_turn_end_goal_notice(lines, indicator_index);
    if claude_indicator_shows_goal(indicator) {
        return match turn_end {
            Some((ClaudeGoalNotice::Paused(goal_state), line)) => {
                goal_classification(GoalEvidence::State(goal_state), Some(line))
            }
            _ => goal_classification(GoalEvidence::State(GoalState::Active), Some(indicator)),
        };
    }
    match turn_end {
        Some((ClaudeGoalNotice::Achieved, line)) => {
            goal_classification(GoalEvidence::State(GoalState::Reached), Some(line))
        }
        Some((ClaudeGoalNotice::Failed, line)) => {
            goal_classification(GoalEvidence::State(GoalState::Blocked), Some(line))
        }
        Some((ClaudeGoalNotice::Cleared, line)) => {
            goal_classification(GoalEvidence::Inactive, Some(line))
        }
        // A notice that the goal is still running (or paused) without its
        // indicator is a redraw in progress, not proof the goal ended.
        Some((ClaudeGoalNotice::Running | ClaudeGoalNotice::Paused(_), _)) => {
            goal_classification(GoalEvidence::Unknown, None)
        }
        None => goal_classification(GoalEvidence::Inactive, Some(indicator)),
    }
}

/// The row immediately above the rule that tops the last `❯` composer.
fn claude_indicator_row_index(lines: &[&str]) -> Option<usize> {
    let composer_index = (2..lines.len()).rev().find(|&index| {
        lines[index].trim_start().starts_with('❯') && is_horizontal_rule(lines[index - 1])
    })?;
    Some(composer_index - 2)
}

fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.chars().count() >= 10 && trimmed.chars().all(|character| character == '─')
}

/// The indicator may share its row with other right-aligned status items
/// (`● high · /effort · ◎ /goal active (3m)`), so it is matched as that row's
/// final segment.
fn claude_indicator_shows_goal(indicator: &str) -> bool {
    let normalized = indicator.trim_end().to_lowercase();
    if normalized.ends_with("/goal active") {
        return true;
    }
    rightmost_segment_start(&normalized, "/goal active (").is_some_and(|index| {
        let duration_start = index + "/goal active (".len();
        has_elapsed_token_after(&normalized, duration_start)
            && normalized[duration_start..]
                .find(')')
                .is_some_and(|offset| normalized[duration_start + offset + 1..].trim().is_empty())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeGoalNotice {
    Paused(GoalState),
    Achieved,
    Failed,
    Cleared,
    /// `Goal set:` or `Goal not yet met… continuing`: the goal is running.
    Running,
}

/// The newest goal notice of the current turn, scanning upward from the
/// indicator row. The scan stops at the previous user prompt (`❯ text`), at
/// any later assistant message (`● text` that is not itself a goal notice),
/// and at a live spinner row, because each of those proves the agent has
/// moved past an older notice.
fn claude_turn_end_goal_notice<'screen>(
    lines: &[&'screen str],
    indicator_index: usize,
) -> Option<(ClaudeGoalNotice, &'screen str)> {
    let search_start = indicator_index.saturating_sub(CLAUDE_TURN_END_SEARCH_ROWS);
    for line in lines[search_start..indicator_index].iter().rev() {
        let trimmed = line.trim_start();
        let normalized = normalize_goal_status_line(line);
        if let Some(notice) = claude_goal_notice(&normalized) {
            return Some((notice, line));
        }
        let is_user_prompt = trimmed.starts_with('❯');
        let is_assistant_message = trimmed.starts_with('●');
        if is_user_prompt || is_assistant_message || is_live_status_line(line) {
            return None;
        }
    }
    None
}

fn claude_goal_notice(normalized: &str) -> Option<ClaudeGoalNotice> {
    if normalized.starts_with("goal paused") {
        let goal_state = if normalized.contains("usage limit") {
            GoalState::UsageLimited
        } else {
            GoalState::Paused
        };
        return Some(ClaudeGoalNotice::Paused(goal_state));
    }
    if normalized.starts_with("goal achieved") {
        return Some(ClaudeGoalNotice::Achieved);
    }
    if normalized.starts_with("goal could not be achieved") {
        return Some(ClaudeGoalNotice::Failed);
    }
    if normalized.starts_with("goal cleared") {
        return Some(ClaudeGoalNotice::Cleared);
    }
    if normalized.starts_with("goal set:") || normalized.starts_with("goal not yet met") {
        return Some(ClaudeGoalNotice::Running);
    }
    None
}

/// Normalizes case and removes decoration only from the beginning of a line.
fn normalize_goal_status_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

/// Requires a marker to begin at the normalized line start or after an actual
/// footer-segment delimiter. Whitespace alone is intentionally insufficient:
/// normal transcript prose can quote the exact visible status phrase.
fn has_status_boundary_before(line: &str, marker_index: usize) -> bool {
    marker_index == 0
        || line[..marker_index]
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|character| matches!(character, '·' | '│' | '|' | '•' | '—' | '–' | '…'))
}

/// True if a line reads as "the agent is waiting on background
/// subagents/tasks it dispatched" -- e.g. Claude Code's
/// `"✻ Waiting for 2 background agents to finish"`. Requires "waiting for"
/// together with "background" and either "agent" or "task" (all
/// case-insensitive) rather than any one of those words alone: normal agent
/// prose routinely says "waiting for" or "background" in an unrelated
/// sentence, but the combination only shows up as this specific status
/// line. Not tied to a particular agent CLI's exact wording -- any agent
/// (Claude Code today; Codex CLI's own subagent feature may grow an
/// equivalent status line) that renders this combination is classified the
/// same way, per this crate's registry-over-branching convention.
fn looks_like_background_wait_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_background_wait_line)
}

/// The single-line rule behind [`looks_like_background_wait_line`], shared
/// with `activity_evidence_line` so the reported evidence line is always the
/// line the classifier itself matched.
fn is_background_wait_line(line: &str) -> bool {
    // Use to_ascii_lowercase() for ASCII terminal output (faster than to_lowercase() for typical case)
    let lower = line.to_ascii_lowercase();
    lower.contains("waiting for")
        && lower.contains("background")
        && (lower.contains("agent") || lower.contains("task"))
}

/// The `WORKING_MARKER` rule as a per-line predicate, so evidence recovery
/// runs the same test the classifier ran over the whole screen. The marker
/// contains no newline, so a screen-wide `contains` match always lies inside
/// exactly one line.
fn is_interrupt_marker_line(line: &str) -> bool {
    line.contains(WORKING_MARKER)
}

/// True if a line reads as Claude Code's own "N background thing(s) still
/// executing" indicator -- e.g. `"✻ Cogitated for 3m 11s · 1 shell still
/// running"`, or `"✻ Cooked for 3m 6s · done 7:00 PM · 1 monitor still
/// running"`. Claude Code appends this suffix to its completed-turn summary
/// line when something it started in the background (a shell command, a
/// monitor, ...) is still executing after the foreground turn ended; that
/// summary line otherwise reads as finished/idle (see
/// `looks_like_live_status_line`'s doc comment on the same "Cogitated for
/// Ns" shape). Without this check a pane with real work still in flight
/// would misreport as idle, and the server's `promote_to_done` would mark
/// it `Done` -- exactly the "shows finished in the sidebar while still
/// running" report this exists to fix.
///
/// Deliberately noun-agnostic (checks for a bare digit count alongside
/// "still running", not a specific word like "shell") rather than a
/// hardcoded keyword list: Claude Code has already been observed using more
/// than one noun for this same shape ("shell", "monitor"), and a fixed
/// keyword list would need a code change every time its wording drifts
/// again. Requires the literal phrase "still running" together with a bare
/// numeric token (e.g. "1", "3") rather than either alone, since "running"
/// alone appears constantly in ordinary agent prose -- same residual-risk
/// tradeoff `looks_like_background_wait_line` already accepts for its own
/// word combination.
fn looks_like_background_task_wait_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_background_task_wait_line)
}

/// The single-line rule behind [`looks_like_background_task_wait_line`].
fn is_background_task_wait_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("still running")
        && lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
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
    screen_text.lines().any(is_live_status_line)
}

/// The single-line rule behind [`looks_like_live_status_line`].
fn is_live_status_line(line: &str) -> bool {
    line.contains('…')
        && line
            .split(|character: char| character.is_whitespace() || character == '·')
            .any(is_elapsed_time_token)
}

/// True when Codex's visible status line explicitly names an active turn.
///
/// Codex keeps completed timing summaries on screen, often alongside an
/// ellipsis and an elapsed-time token. Requiring a present-tense activity word
/// prevents those historical rows from being mistaken for a live turn.
///
/// The activity word must open a footer *segment*, not just the whole line:
/// real captured Codex chrome renders it either flush left
/// (`"Thinking... (esc to interrupt) · 12s"`) or after a leading glyph
/// (`"• Working (18m 26s • esc to interrupt)"`, see
/// `tests/fixtures/codex_goal_active_wide_footer.txt`). A bare `starts_with`
/// check only catches the first shape; reusing `has_status_boundary_before`
/// (the footer-segment boundary rule shared with other status chrome)
/// catches both while still rejecting the word mid-sentence or embedded in a
/// longer word (e.g. "regenerating").
fn looks_like_codex_live_status_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_codex_live_status_line)
}

/// The single-line rule behind [`looks_like_codex_live_status_line`].
fn is_codex_live_status_line(line: &str) -> bool {
    // Use to_ascii_lowercase() for ASCII terminal output; cache once per line
    let lower = line.trim().to_ascii_lowercase();
    let names_active_turn = ["thinking", "working", "generating", "planning", "running"]
        .iter()
        .any(|marker| {
            // Every occurrence is checked, not just the first: prose earlier
            // on the same line can legitimately contain the activity word
            // without a segment boundary ("tests are running fine ·
            // Running… (5m)"), and stopping at that first failed occurrence
            // would misclassify a live turn as idle.
            lower
                .match_indices(*marker)
                .any(|(marker_index, _)| has_status_boundary_before(&lower, marker_index))
        });
    names_active_turn
        && (lower.contains('…') || lower.contains("..."))
        && lower
            .split(|character: char| character.is_whitespace() || character == '·')
            .any(is_elapsed_time_token)
}

/// True if `token` looks like an elapsed-time reading: one or more ASCII
/// digits immediately followed by a single 's' or 'm' unit suffix and
/// nothing else (so "6s" and "12m" match, but "s" or "class" don't).
fn is_elapsed_time_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| !c.is_alphanumeric());
    let Some(digits) = trimmed.strip_suffix(['s', 'm']) else {
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
    screen_text.lines().any(is_confirmation_prompt_line)
}

/// The single-line rule behind [`looks_like_confirmation_prompt`].
fn is_confirmation_prompt_line(line: &str) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('?') {
        return false;
    }
    // Split on non-alphanumeric boundaries so "yes"/"no" are matched as whole
    // words -- a naive substring check (" yes"/" no") false-positives on
    // ordinary words like "yesterday" or "nothing"/"not"/"now"/"north".
    let mut has_yes_word = false;
    let mut has_no_word = false;
    for word in trimmed.split(|character: char| !character.is_alphanumeric()) {
        if word.eq_ignore_ascii_case("yes") {
            has_yes_word = true;
        } else if word.eq_ignore_ascii_case("no") {
            has_no_word = true;
        }
    }
    has_yes_word && has_no_word
}

/// Every glyph an agent CLI marks its currently-selected option with.
///
/// `❯` (U+276F) is what Claude Code and Codex's approval dialogs render;
/// `›` (U+203A) is what Codex's numbered-choice modals use -- the same glyph
/// its composer draws, which is precisely why a numbered option line has to be
/// distinguished from a composer line rather than from the glyph alone (see
/// `screen_has_empty_codex_composer_at_cursor`). Recognizing only the first
/// glyph left a whole family of real Codex modals classified `Idle`, i.e.
/// silently not reported as blocked on the user, whenever their footer hint
/// was absent or scrolled away.
const SELECTION_CURSORS: &[char] = &['\u{276f}', '\u{203a}'];

/// True if the screen looks like a general multiple-choice / selection
/// prompt -- not necessarily yes/no -- e.g. Claude Code's numbered option
/// menus with a selection cursor on the currently-selected line and a footer
/// hint like "Enter to select · ↑/↓ to navigate · Esc to cancel". Either
/// of two independent signals is enough:
///
/// - A footer hint line naming both a confirm/select action and a cancel
///   action -- that exact combination of phrasing only shows up as
///   interactive-prompt chrome, never in normal command output.
/// - At least two numbered option lines (e.g. "1. Source only", "  2. Write
///   full list to file") where at least one of them is itself prefixed by a
///   [`SELECTION_CURSORS`] cursor -- matching how the fixtures actually render
///   it (`"❯ 1. Source only"`). Requiring the cursor to prefix an option
///   line specifically (not merely appear *somewhere* on screen) keeps
///   this from firing when an agent's own numbered analysis (e.g. "1.
///   Findings list page...", "2. Finding detail page...") shares a screen
///   with an unrelated `❯` glyph -- a Starship-themed shell prompt uses
///   that exact character, and it doesn't mean the numbered lines above it
///   are a selection menu.
fn looks_like_selection_prompt(screen_text: &str) -> bool {
    // Check for the selection footer first: it is a single-line signal, so it
    // can answer without counting option lines across the whole screen.
    if screen_text.lines().any(is_selection_footer_line) {
        return true;
    }

    let mut numbered_option_lines = 0_usize;
    let mut has_cursor_on_option_line = false;
    for line in screen_text.lines() {
        if !is_numbered_option_line(line) {
            continue;
        }
        numbered_option_lines += 1;
        has_cursor_on_option_line |= starts_with_selection_cursor(line);
    }
    numbered_option_lines >= 2 && has_cursor_on_option_line
}

/// The per-line half of [`looks_like_selection_prompt`]: either signal, on
/// this one line. Used to recover the evidence line for a
/// `SelectionPrompt` classification.
fn is_selection_prompt_line(line: &str) -> bool {
    is_selection_footer_line(line)
        || (is_numbered_option_line(line) && starts_with_selection_cursor(line))
}

/// True if `line` is an interactive prompt's footer hint -- it names both a
/// confirm/select action and a cancel action.
fn is_selection_footer_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let names_a_confirm_action =
        lower.contains("to select") || lower.contains("to confirm") || lower.contains("to choose");
    names_a_confirm_action && lower.contains("cancel")
}

/// True if `line`'s first non-blank character is a selection cursor.
fn starts_with_selection_cursor(line: &str) -> bool {
    line.trim_start().starts_with(SELECTION_CURSORS)
}

/// True if `line` starts (after an optional selection cursor and leading
/// whitespace) with a small integer followed by `". "` -- e.g. "1. Source
/// only" or "  2. Write full list to file".
fn is_numbered_option_line(line: &str) -> bool {
    let trimmed = line
        .trim_start()
        .trim_start_matches(SELECTION_CURSORS)
        .trim_start();
    // `None` here means the line is nothing but digits, which cannot be
    // followed by ". " either -- both cases correctly answer "not an option".
    let digits_end = trimmed
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(0);
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
/// Deliberately cheap: `identify_agent`'s tree walk needs pid/parent chains,
/// process names, and the command line. `cwd` and `environ` are per-process
/// filesystem reads (`readlink`/`open`+`read` under `/proc`) and cost
/// proportionally to *every* process on the machine if fetched here --
/// entirely wasted on the >99% of processes that are never an agent CLI.
/// Session-ID discovery (which does need those fields, scoped to just the one
/// matched pid) is app-level orchestration that lives above this crate.
///
/// The command line is the exception, and it is not optional: a CLI installed
/// as a shebang script is reported by macOS under its *interpreter's* name, so
/// without arguments there is nothing left to recognise `claude` or `codex` by
/// there (see `identifying_process_names`). `OnlyIfNotSet` keeps that cheap --
/// a process's arguments never change, so this reads them once per process
/// rather than on every tick.
pub fn refresh(system: &mut System) {
    configure_process_refresh();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
    );
}

/// A parent-pid -> direct-children-pids adjacency, built once from an
/// already-[`refresh`]ed `System` and then reused across every pane's
/// [`identify_agent_with_extra`] call within the same detection tick.
///
/// Building this costs one pass over every process on the machine --
/// exactly the same total cost `identify_agent_with_extra` used to pay on
/// its own, *per pane*, every tick (it used to scan
/// `system.processes().values()` in full, once for each due pane). Since
/// the caller (`ilium-server`'s detection loop) builds one of these per
/// tick and shares it across every due pane, the per-tick cost drops from
/// O(due_panes * total_processes) to O(total_processes + due_panes *
/// average_descendant_count) -- each pane's own walk (see
/// [`identify_agent_with_extra`]) only ever visits processes actually
/// reachable as descendants of that pane's shell, not the whole system
/// table.
pub struct ProcessChildrenIndex(HashMap<Pid, Vec<Pid>>);

/// One registry match with the exact signature retained for diagnostics.
struct AgentProcessMatch {
    class: AgentClass,
    matched_signature: String,
}

impl ProcessChildrenIndex {
    /// Builds the index from `system`'s current process snapshot. `system`
    /// must already have been [`refresh`]ed -- this reads whatever
    /// pid/parent pairs are already populated, it does not refresh
    /// anything itself.
    pub fn build(system: &System) -> Self {
        let mut index: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for process in system.processes().values() {
            if let Some(parent_pid) = process.parent() {
                index.entry(parent_pid).or_default().push(process.pid());
            }
        }
        Self(index)
    }

    /// The direct children of `pid` per this snapshot, or an empty slice
    /// if `pid` has none (or isn't a parent of anything in this snapshot).
    fn children_of(&self, pid: Pid) -> &[Pid] {
        self.0.get(&pid).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Given the OS pid of a pane's directly-spawned child (typically the
/// user's shell), walks the process tree looking for a descendant process
/// whose name matches a known agent CLI signature (see
/// `BuiltinAgentProvider::ALL` and [`GENERIC_AGENT_SIGNATURES`]), and
/// returns the best-ranked match: the shallowest one, preferring a process
/// recognized by its own kernel name over one recognized only through an
/// interpreter's argv, and breaking a remaining tie by lowest pid. See
/// [`identify_agent_with_extra`] for exactly how that ranking bounds the walk.
///
/// Returns `None` if no descendant process matches.
///
/// Takes `&System` (already refreshed by the caller via [`refresh`], or
/// equivalent) rather than owning any refresh itself -- session-ID
/// discovery, which does need a targeted `cwd`/`environ` refresh on the
/// matched pid, is the caller's job.
///
/// Builds its own one-shot [`ProcessChildrenIndex`] internally -- fine for
/// a single call (e.g. this crate's own tests, or a caller checking just
/// one pane), but a caller classifying many panes against the same
/// refreshed `System` in one pass should build a `ProcessChildrenIndex`
/// once and call [`identify_agent_with_extra`] directly instead of this
/// wrapper, so that index-building cost is only paid once, not once per
/// pane. See `ilium-server::detection::run_due_panes`.
pub fn identify_agent(system: &System, shell_pid: Pid) -> Option<AgentIdentity> {
    let children_index = ProcessChildrenIndex::build(system);
    identify_agent_with_extra(system, shell_pid, &children_index, &[])
}

/// Same as [`identify_agent`], but also checks `extra_signatures` (e.g.
/// user-configured `[[detection.custom_signatures]]` entries) alongside
/// the built-in `BuiltinAgentProvider::ALL` and [`GENERIC_AGENT_SIGNATURES`]
/// tables -- the registry-driven
/// extension point `CLAUDE.md`'s layering rule calls for, rather than a
/// second, parallel matching code path for config-provided signatures.
///
/// Walks *only* the descendants of `shell_pid`, breadth-first over
/// `children_index` (see [`ProcessChildrenIndex`]) -- never the full
/// `system.processes()` table -- so this call's cost scales with how many
/// processes actually descend from this one pane's shell, not with how
/// many processes are running on the machine as a whole.
/// Ranking key plus payload for the best candidate seen so far in
/// [`identify_agent_with_extra`]'s walk: `(is_interpreted, depth, pid)` is
/// compared lexicographically, then the matched process's pid, its matched
/// name, and the registry match that recognized it.
type BestAgentMatch = ((bool, usize, u32), Pid, String, AgentProcessMatch);

pub fn identify_agent_with_extra(
    system: &System,
    shell_pid: Pid,
    children_index: &ProcessChildrenIndex,
    extra_signatures: &[AgentSignature],
) -> Option<AgentIdentity> {
    // Tracks the best match found so far as (is_interpreted, depth, pid)
    // plus its class -- the CLI process is closer to the pane shell than
    // its internal helper processes (for example Codex's code-mode host),
    // so lower depth (and, tie-broken, lower pid) wins among matches of the
    // same provenance, matching the old `min_by_key((depth, pid))`
    // semantics exactly.
    //
    // `is_interpreted` breaks that tie the other way for one specific
    // shape: some installs (e.g. Bun's global bin shim,
    // `node /home/.../bin/codex`) put a JS *launcher* directly on the
    // pane's shell -- matched only by unwrapping its argv (see
    // `identifying_process_names`), never by its own kernel name -- which
    // then spawns the real native CLI binary as a further child instead of
    // exec-replacing itself. That binary's kernel name matches the
    // signature directly and is the process actually holding the
    // transcript file open, so a same-signature *native* match found one
    // level past an interpreted one wins regardless of depth: the launcher
    // was never the CLI, just its spawner. The search only extends past an
    // interpreted match's own depth, never past a native one, so this
    // cannot turn into an unbounded deep search for an unrelated process
    // that happens to share a name.
    let mut best: Option<BestAgentMatch> = None;
    let mut queue: VecDeque<(Pid, usize)> = VecDeque::new();
    let mut visited: HashSet<Pid> = HashSet::new();
    queue.push_back((shell_pid, 0));
    visited.insert(shell_pid);

    while let Some((pid, depth)) = queue.pop_front() {
        if let Some(((best_is_interpreted, best_depth, _), _, _, _)) = &best {
            let search_limit = if *best_is_interpreted {
                best_depth + 1
            } else {
                *best_depth
            };
            if depth > search_limit {
                break;
            }
        }
        if let Some(process) = system.process(pid) {
            // Lowercased once per process, avoiding repeated allocation per
            // classification attempt. See `identifying_process_names` for why
            // more than the kernel name has to be considered. Candidate index
            // 0 is always the process's own kernel name (see that function);
            // anything matched further down the list was only inferred from
            // an interpreter's argv.
            let matched = identifying_process_names(process)
                .into_iter()
                .enumerate()
                .find_map(|(index, candidate)| {
                    match_process_name_with_extra(&candidate, extra_signatures)
                        .map(|process_match| (index > 0, candidate, process_match))
                });
            if let Some((is_interpreted, matched_name, process_match)) = matched {
                let key = (is_interpreted, depth, pid.as_u32());
                let is_better = match &best {
                    None => true,
                    Some((existing_key, _, _, _)) => key < *existing_key,
                };
                if is_better {
                    best = Some((key, pid, matched_name, process_match));
                }
            }
        }
        for &child_pid in children_index.children_of(pid) {
            // A visited set guards against any (unexpected) cycle in a
            // stale/inconsistent snapshot -- process trees are finite in
            // practice, but nothing here depends on that being guaranteed.
            if visited.insert(child_pid) {
                queue.push_back((child_pid, depth + 1));
            }
        }
    }

    best.map(
        |((_, depth, _), pid, process_name, process_match)| AgentIdentity {
            pid: pid.as_u32(),
            class: process_match.class,
            process_name,
            matched_signature: process_match.matched_signature,
            process_tree_depth: depth,
        },
    )
}

/// A process's arguments as the program itself would see them.
///
/// A pane runs its command through `$SHELL -c "<command line>"`. Linux replaces
/// the shell image on exec, so the process ends up reporting the program's own
/// tokenised argv. macOS keeps the wrapper's: the entire command line stays a
/// single argument, and nothing in it ever equals `--resume` or `--session-id`.
/// Anything scanning arguments for a flag therefore works on one platform and
/// silently finds nothing on the other.
///
/// Splitting on whitespace is deliberately simple. It is enough for the flags
/// this is used to find, and a path containing spaces would already be
/// ambiguous in a shell command line without quoting this cannot see.
pub fn effective_arguments(process: &sysinfo::Process) -> Vec<String> {
    let arguments: Vec<String> = process
        .cmd()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    expand_shell_command_line(&arguments)
}

/// The pure half of [`effective_arguments`], so the expansion can be tested
/// without a live process.
fn expand_shell_command_line(arguments: &[String]) -> Vec<String> {
    let is_interpreter = arguments
        .first()
        .and_then(|argument| Path::new(argument).file_name())
        .is_some_and(|name| is_interpreter_name(&name.to_string_lossy().to_lowercase()));
    if !is_interpreter {
        return arguments.to_vec();
    }
    let Some(command_index) = arguments
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, argument)| !argument.starts_with('-'))
        .map(|(index, _)| index)
    else {
        return arguments.to_vec();
    };
    let mut expanded: Vec<String> = arguments[..command_index].to_vec();
    for argument in &arguments[command_index..] {
        expanded.extend(argument.split_whitespace().map(str::to_owned));
    }
    expanded
}

/// Every lowercase name a process may legitimately be recognised by, most
/// authoritative first.
///
/// The kernel's process name is normally right, but it is not portable for the
/// shape agent CLIs most often take. A CLI installed as a script with a shebang
/// -- `#!/usr/bin/env node`, or a shell wrapper -- is reported by Linux under
/// the script's own name, while macOS reports the *interpreter*: `node`, or
/// `sh`. Matching the kernel name alone therefore recognises the same installed
/// `claude` or `codex` on Linux and silently misses it on macOS.
///
/// The command line closes that gap. `argv[0]`'s file name is the path the
/// process was actually invoked as, and when that name is itself an
/// interpreter, the program it was handed (see `interpreted_program_name`,
/// which reduces `sh -c "vim codex.md"` to `vim` rather than to the whole
/// command line) is a candidate too -- that is the only thing left to
/// recognise a shebang-installed `claude` or `codex` by on macOS.
///
/// Both are strictly *fallback* candidates: the kernel name is returned first,
/// and the caller stops at the first candidate that matches, so an
/// interpreter-inferred name is only ever consulted when the kernel name
/// matched nothing. [`identify_agent_with_extra`] additionally treats such a
/// match as weaker evidence than a natively-named descendant, because the
/// launcher a shim runs is usually not the process actually doing the work.
pub fn identifying_process_names(process: &sysinfo::Process) -> Vec<String> {
    let arguments = process.cmd();
    let invoked = argument_file_name(arguments, 0);
    // When the first argument is an interpreter, the program it was handed is
    // the name worth matching. Where that program sits is not fixed: a shebang
    // launch produces `["/bin/sh", "/path/codex"]`, while a shell running a
    // command line produces `["/bin/sh", "-c", "/path/codex"]`. Both occur --
    // Linux replaces the process name with the script's on exec, so the
    // distinction only becomes visible on macOS, where a pane's command stays
    // `sh -c ...` forever.
    let script = invoked
        .as_deref()
        .is_some_and(is_interpreter_name)
        .then(|| interpreted_program_name(arguments))
        .flatten();

    let mut candidates = vec![process.name().to_string_lossy().to_lowercase()];
    for candidate in [script, invoked].into_iter().flatten() {
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// The lowercase file name of the program an interpreter was asked to run.
///
/// Skips the interpreter's own flags, then takes the first whitespace-separated
/// token of what follows. The token matters: `sh -c` receives an entire command
/// line as one argument, so `sh -c "vim codex.md"` would otherwise be read as a
/// program named `vim codex.md` -- which contains `codex` and would be matched
/// as the agent by a registry that works on substrings.
fn interpreted_program_name(arguments: &[std::ffi::OsString]) -> Option<String> {
    let program = arguments
        .iter()
        .skip(1)
        .find(|argument| !argument.to_string_lossy().starts_with('-'))?;
    let first_token = program
        .to_string_lossy()
        .split_whitespace()
        .next()?
        .to_owned();
    Path::new(&first_token)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

/// File name of the argument at `index`, lowercased.
fn argument_file_name(arguments: &[std::ffi::OsString], index: usize) -> Option<String> {
    let argument = arguments.get(index)?;
    Path::new(argument)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

/// Whether a program merely *runs* another program named after it.
///
/// Deliberately a closed list. Consulting the second argument for any process
/// would misread `vim codex.md` as the agent itself; consulting it only behind
/// a known interpreter keeps the rule to the case it exists for, and leaves
/// `sh -c "codex …"` alone because that command's second argument is `-c`.
fn is_interpreter_name(name: &str) -> bool {
    const INTERPRETERS: &[&str] = &[
        "sh", "bash", "dash", "zsh", "ksh", "fish", "env", "node", "nodejs", "deno", "bun",
        "python", "python3", "ruby", "perl",
    ];
    INTERPRETERS.contains(&name) || INTERPRETERS.contains(&name.trim_end_matches(".exe"))
}

/// Classifies a single (already-lowercased) process name against the shared
/// first-party provider registry, generic built-ins, then `extra_signatures`.
/// That order lets configuration add coverage without silently shadowing an
/// agent whose launch/resume semantics ilium already knows.
#[cfg(test)]
fn classify_process_name_with_extra(
    lowercase_name: &str,
    extra_signatures: &[AgentSignature],
) -> Option<AgentClass> {
    match_process_name_with_extra(lowercase_name, extra_signatures).map(|matched| matched.class)
}

/// Matches one process while retaining which registry signature justified the
/// class. Built-ins keep priority over generic/custom entries so configuration
/// cannot shadow first-party provider behavior.
fn match_process_name_with_extra(
    lowercase_name: &str,
    extra_signatures: &[AgentSignature],
) -> Option<AgentProcessMatch> {
    if let Some((provider, signature)) =
        BuiltinAgentProvider::ALL.into_iter().find_map(|provider| {
            provider
                .process_name_substrings()
                .iter()
                .find(|substring| lowercase_name.contains(**substring))
                .map(|signature| (provider, *signature))
        })
    {
        return Some(AgentProcessMatch {
            class: provider.class(),
            matched_signature: signature.to_string(),
        });
    }

    GENERIC_AGENT_SIGNATURES
        .iter()
        .chain(extra_signatures)
        .find(|signature| lowercase_name.contains(signature.name_substring.as_ref()))
        .map(|signature| AgentProcessMatch {
            class: (signature.class_of)(lowercase_name),
            matched_signature: signature.name_substring.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shebang case this exists for: the kernel puts the interpreter in
    /// the first argument and the agent's own script in the second, so the
    /// script is the name that must be reachable.
    #[test]
    fn an_interpreter_hands_its_script_name_on() {
        assert!(is_interpreter_name("sh"));
        assert!(is_interpreter_name("node"));
        assert!(is_interpreter_name("python3"));
        // Windows spells the same interpreters with an extension.
        assert!(is_interpreter_name("node.exe"));
    }

    /// The list is closed on purpose: consulting a second argument for any
    /// process at all would read `vim codex.md` as the agent itself.
    #[test]
    fn an_ordinary_program_is_not_treated_as_an_interpreter() {
        assert!(!is_interpreter_name("vim"));
        assert!(!is_interpreter_name("codex"));
        assert!(!is_interpreter_name("claude"));
        assert!(!is_interpreter_name(""));
    }

    /// Both shapes a real pane produces. A shebang launch puts the script
    /// straight after the interpreter; a shell running a command line puts it
    /// after `-c`, which is what macOS keeps reporting for a live pane.
    #[test]
    fn an_interpreter_is_followed_to_its_program_whichever_shape_it_takes() {
        let shebang = [
            std::ffi::OsString::from("/bin/sh"),
            std::ffi::OsString::from("/tmp/fixtures/codex"),
        ];
        let command_line = [
            std::ffi::OsString::from("/bin/sh"),
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("/tmp/fixtures/codex"),
        ];

        assert_eq!(interpreted_program_name(&shebang).as_deref(), Some("codex"));
        assert_eq!(
            interpreted_program_name(&command_line).as_deref(),
            Some("codex")
        );
    }

    /// `sh -c` receives a whole command line as one argument, so the program is
    /// its first token. Without that, `vim codex.md` reads as a program whose
    /// name contains `codex`, and a substring registry would call it the agent.
    #[test]
    fn a_command_line_is_reduced_to_the_program_it_runs() {
        let editing_a_file = [
            std::ffi::OsString::from("/bin/sh"),
            std::ffi::OsString::from("-c"),
            std::ffi::OsString::from("vim codex.md"),
        ];

        assert_eq!(
            interpreted_program_name(&editing_a_file).as_deref(),
            Some("vim"),
            "the program is the first token, not the whole command line"
        );
        assert!(
            classify_process_name_with_extra("vim", &[]).is_none(),
            "editing a file named after an agent must not look like the agent"
        );
    }

    /// The shape macOS reports for a pane: the shell keeps its own argv and
    /// the program's whole command line stays one argument, so a flag scan
    /// finds nothing until it is expanded.
    #[test]
    fn a_shell_wrapped_command_line_expands_into_the_tokens_the_program_sees() {
        let wrapped = [
            "/bin/sh".to_string(),
            "-c".to_string(),
            "/tmp/claude --resume abc123 /tmp/session.jsonl".to_string(),
        ];

        let expanded = expand_shell_command_line(&wrapped);

        assert_eq!(
            expanded,
            vec![
                "/bin/sh",
                "-c",
                "/tmp/claude",
                "--resume",
                "abc123",
                "/tmp/session.jsonl"
            ]
        );
    }

    /// A program that is not an interpreter already reports its own argv, and
    /// splitting it again would corrupt an argument that legitimately contains
    /// a space.
    #[test]
    fn a_direct_invocation_is_left_alone() {
        let direct = [
            "/tmp/claude".to_string(),
            "--resume".to_string(),
            "abc 123".to_string(),
        ];

        assert_eq!(expand_shell_command_line(&direct), direct.to_vec());
    }

    #[test]
    fn an_argument_is_reduced_to_its_lowercase_file_name() {
        let arguments = [
            std::ffi::OsString::from("/bin/sh"),
            std::ffi::OsString::from("/Tmp/Fixtures/Codex"),
        ];

        assert_eq!(argument_file_name(&arguments, 0).as_deref(), Some("sh"));
        assert_eq!(argument_file_name(&arguments, 1).as_deref(), Some("codex"));
        // Past the end is "unknown", not a panic: a process can be inspected
        // while its argument vector is still being read.
        assert_eq!(argument_file_name(&arguments, 2), None);
    }

    /// Loads a captured screen-text fixture from `tests/fixtures/`.
    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read fixture {path}: {error}");
        })
    }

    fn dimmed_ascii_cells(row: u16, start_column: u16, text: &str) -> Vec<(u16, u16)> {
        (0..text.len())
            .map(|offset| {
                (
                    row,
                    start_column.saturating_add(u16::try_from(offset).expect("short fixture")),
                )
            })
            .collect()
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
    fn claude_code_resume_full_session_prompt_yields_key_2() {
        assert_eq!(
            interstitial_prompt_response(
                &AgentClass::Claude,
                &fixture("claude_code_resume_full_session_prompt.txt")
            ),
            Some("2")
        );
    }

    #[test]
    fn resume_prompt_wording_quoted_in_transcript_does_not_match() {
        assert_eq!(
            interstitial_prompt_response(
                &AgentClass::Claude,
                &fixture("claude_code_resume_prompt_quoted_in_transcript.txt")
            ),
            None
        );
    }

    #[test]
    fn resume_prompt_does_not_match_for_codex() {
        assert_eq!(
            interstitial_prompt_response(
                &AgentClass::Codex,
                &fixture("claude_code_resume_full_session_prompt.txt")
            ),
            None
        );
    }

    #[test]
    fn plain_shell_has_no_interstitial_prompt() {
        assert_eq!(
            interstitial_prompt_response(&AgentClass::Claude, &fixture("plain_shell.txt")),
            None
        );
    }

    #[test]
    fn claude_code_waiting_on_background_agents_is_waiting_background() {
        assert_eq!(
            classify_activity(&fixture("claude_code_waiting_background.txt")),
            AgentActivity::WaitingBackground
        );
    }

    #[test]
    fn claude_code_completed_turn_with_shell_still_running_is_background_task_still_running() {
        let fixture_text = fixture("claude_code_shell_still_running.txt");
        assert_eq!(
            classify_activity(&fixture_text),
            AgentActivity::BackgroundTaskStillRunning
        );
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Claude, &fixture_text),
            AgentActivity::BackgroundTaskStillRunning
        );
    }

    #[test]
    fn claude_code_completed_turn_with_monitor_still_running_is_background_task_still_running() {
        let fixture_text = fixture("claude_code_monitor_still_running.txt");
        assert_eq!(
            classify_activity(&fixture_text),
            AgentActivity::BackgroundTaskStillRunning
        );
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Claude, &fixture_text),
            AgentActivity::BackgroundTaskStillRunning
        );
    }

    #[test]
    fn prose_mentioning_shell_or_running_alone_is_idle_not_waiting_background() {
        assert_eq!(
            classify_activity("I opened a new shell for the migration."),
            AgentActivity::Idle
        );
        assert_eq!(
            classify_activity("The dev server is running now."),
            AgentActivity::Idle
        );
    }

    #[test]
    fn codex_mid_turn_is_working() {
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Codex, &fixture("codex_working.txt"),),
            AgentActivity::Working
        );
    }

    #[test]
    fn codex_working_status_behind_a_leading_glyph_is_working() {
        // Real captured Codex chrome (see
        // `codex_goal_active_wide_footer.txt`) prefixes the activity word
        // with a bullet instead of putting it at column 0. Strip the
        // "esc to interrupt" hint that fixture also carries so this
        // exercises `looks_like_codex_live_status_line` itself rather than
        // being short-circuited by the interrupt-marker check.
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Codex, "• Working… (5m)"),
            AgentActivity::Working
        );
    }

    /// The activity word can legitimately appear earlier on the same line as
    /// ordinary prose (no segment boundary) and again as the real status
    /// segment. Every occurrence must be boundary-checked -- stopping at the
    /// first (prose) occurrence would misreport a live turn as idle.
    #[test]
    fn codex_status_segment_after_a_prose_occurrence_of_the_same_word_is_working() {
        assert_eq!(
            classify_activity_for_agent(
                &AgentClass::Codex,
                "tests are running fine · Running… (5m)"
            ),
            AgentActivity::Working
        );
    }

    #[test]
    fn codex_activity_word_embedded_in_a_longer_word_is_not_working() {
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Codex, "Regenerating… (5m) is not a status"),
            AgentActivity::Idle
        );
    }

    #[test]
    fn codex_completed_timing_summary_is_idle() {
        assert_eq!(
            classify_activity_for_agent(
                &AgentClass::Codex,
                "Implemented the requested change… 12s\n\nSend a message",
            ),
            AgentActivity::Idle
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
    fn prose_mentioning_background_or_waiting_alone_is_idle_not_waiting_background() {
        assert_eq!(
            classify_activity("I'll run this in the background and keep waiting for input."),
            AgentActivity::Idle
        );
        assert_eq!(
            classify_activity("Waiting for the build to finish."),
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
    fn question_with_yes_no_as_substrings_of_other_words_is_not_waiting_approval() {
        // "yesterday" contains " yes" and "nothing" contains " no" as raw
        // substrings -- confirms the whole-word tokenizer in
        // `looks_like_confirmation_prompt` doesn't false-positive on them.
        assert_eq!(
            classify_activity("Did yesterday's changes land, or is there nothing new?"),
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
    fn rename_plan_confirmation_menu_is_waiting_approval() {
        assert_eq!(
            classify_activity(&fixture("claude_code_rename_confirm_prompt.txt")),
            AgentActivity::WaitingApproval
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
    fn fresh_screen_detection_covers_all_builtin_agents_without_treating_idle_as_fresh() {
        for class in [
            AgentClass::Claude,
            AgentClass::Codex,
            AgentClass::Antigravity,
        ] {
            assert!(is_fresh_agent_screen(
                &class,
                "Conversation cleared\n\nStart a new task"
            ));
        }
        assert!(is_fresh_agent_screen(
            &AgentClass::Codex,
            &fixture("codex_idle.txt")
        ));
        assert!(!is_fresh_agent_screen(
            &AgentClass::Claude,
            &fixture("claude_code_idle.txt")
        ));
        assert!(!is_fresh_agent_screen(
            &AgentClass::Antigravity,
            "Finished the requested task\nType a message"
        ));
    }

    #[test]
    fn prompt_readiness_requires_each_provider_visible_composer() {
        assert!(is_agent_prompt_ready(
            &AgentClass::Codex,
            &fixture("codex_idle.txt")
        ));
        assert!(is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &fixture("codex_dynamic_placeholder_idle.txt"),
            10,
            2,
            &dimmed_ascii_cells(10, 2, "Explain this codebase"),
        ));
        assert!(is_agent_prompt_ready(
            &AgentClass::Claude,
            &fixture("claude_code_idle.txt")
        ));
        assert!(is_agent_prompt_ready(
            &AgentClass::Antigravity,
            "Welcome to Antigravity\nType a message"
        ));
        assert!(!is_agent_prompt_ready(
            &AgentClass::Codex,
            &fixture("codex_awaiting_approval.txt")
        ));
        assert!(!is_agent_prompt_ready(
            &AgentClass::Claude,
            &fixture("claude_code_awaiting_approval.txt")
        ));
        assert!(!is_agent_prompt_ready(
            &AgentClass::Other("opencode".to_owned()),
            "Type a message"
        ));
    }

    #[test]
    fn codex_composer_hint_under_a_live_modal_is_not_prompt_ready() {
        // Codex can keep its composer hint rendered on screen underneath an
        // approval dialog or other modal; prompt-readiness must gate on the
        // shared activity classification too, not just the composer marker,
        // or a one-shot initial prompt gets injected into the modal instead.
        let screen = "Allow this command?\n❯ 1. Yes\n  2. No\n\nSend a message";
        assert!(!is_agent_prompt_ready(&AgentClass::Codex, screen));
    }

    #[test]
    fn codex_numbered_choice_cursor_is_not_a_free_form_composer() {
        let screen = "Select a mode\n› 1. Read only\n  2. Full access";
        assert!(!is_agent_prompt_ready(&AgentClass::Codex, screen));
    }

    #[test]
    fn codex_dirty_composer_is_not_prompt_ready() {
        let screen = fixture("codex_dirty_composer.txt");
        let draft_column = screen
            .lines()
            .nth(2)
            .expect("dirty composer row")
            .chars()
            .count();
        assert!(!is_agent_prompt_ready(&AgentClass::Codex, &screen));
        assert!(!is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &screen,
            2,
            draft_column.try_into().expect("draft column fits in u16"),
            &[],
        ));
        assert!(!is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &screen,
            2,
            2,
            &[],
        ));
    }

    #[test]
    fn codex_cursor_aware_readiness_requires_the_first_input_cell() {
        let placeholder_screen = fixture("codex_dynamic_placeholder_idle.txt");
        assert!(!is_agent_prompt_ready(
            &AgentClass::Codex,
            &placeholder_screen
        ));
        assert!(is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &placeholder_screen,
            10,
            2,
            &dimmed_ascii_cells(10, 2, "Explain this codebase"),
        ));
        assert!(!is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &placeholder_screen,
            10,
            8,
            &dimmed_ascii_cells(10, 2, "Explain this codebase"),
        ));
        assert!(!is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &placeholder_screen,
            9,
            2,
            &dimmed_ascii_cells(10, 2, "Explain this codebase"),
        ));

        assert!(is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            "  › rotating placeholder\nReady",
            0,
            4,
            &dimmed_ascii_cells(0, 4, "rotating placeholder"),
        ));
        assert!(is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            "›\nReady",
            0,
            1,
            &[],
        ));
        assert!(is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            &fixture("codex_goal_achieved_wide_footer.txt"),
            2,
            2,
            &dimmed_ascii_cells(2, 2, "Send a message"),
        ));
        assert!(!is_agent_prompt_ready_at_cursor(
            &AgentClass::Codex,
            "Select a mode\n› 1. Read only\n  2. Full access",
            1,
            2,
            &dimmed_ascii_cells(1, 2, "1. Read only"),
        ));
    }

    #[test]
    fn legacy_codex_box_must_be_visibly_empty() {
        assert!(is_agent_prompt_ready(
            &AgentClass::Codex,
            &fixture("codex_idle.txt")
        ));
        assert!(!is_agent_prompt_ready(
            &AgentClass::Codex,
            "╭────╮\n│draft│\n╰────╯\nSend a message"
        ));
    }

    /// Codex marks the highlighted entry of its numbered modals with `›`, not
    /// with Claude Code's `❯`. Recognizing only the latter left that whole
    /// family of dialogs classified `Idle` -- the sidebar never reported the
    /// agent as blocked on the user -- unless a footer hint happened to be
    /// visible too.
    #[test]
    fn a_numbered_modal_using_codex_own_cursor_glyph_is_waiting_approval() {
        let screen = "Select a mode\n› 1. Read only\n  2. Full access";

        assert_eq!(
            classify_activity_for_agent(&AgentClass::Codex, screen),
            AgentActivity::WaitingApproval
        );
        assert_eq!(classify_activity(screen), AgentActivity::WaitingApproval);
        assert_eq!(
            classify_activity_for_agent_detailed(&AgentClass::Codex, screen)
                .matched_line
                .as_deref(),
            Some("› 1. Read only"),
            "the evidence line must be the cursor-marked option the rule matched"
        );
    }

    /// The same glyph opens Codex's free-form composer, where it introduces a
    /// rotating placeholder rather than a menu entry. An idle composer must
    /// stay idle.
    #[test]
    fn codex_composer_placeholder_is_not_a_selection_prompt() {
        assert_eq!(
            classify_activity_for_agent(
                &AgentClass::Codex,
                &fixture("codex_dynamic_placeholder_idle.txt")
            ),
            AgentActivity::Idle
        );
    }

    /// Every fixture below is a real `tmux capture-pane` frame from the
    /// recorded sessions described in `docs/research/agent-goal-indicators.md`.
    #[test]
    fn captured_codex_goal_footers_map_to_their_goal_phase() {
        for (fixture_name, expected) in [
            (
                "codex_goal_pursuing_warning_footer.txt",
                GoalEvidence::State(GoalState::Active),
            ),
            // The originally reported bug: the footer ends with a warning
            // notice after the goal segment, and the turn is still Working.
            (
                "codex_goal_paused_while_turn_finishes.txt",
                GoalEvidence::State(GoalState::Paused),
            ),
            (
                "codex_goal_paused_after_interrupt.txt",
                GoalEvidence::State(GoalState::Paused),
            ),
            (
                "codex_goal_stalled_narrow.txt",
                GoalEvidence::State(GoalState::Blocked),
            ),
            (
                "codex_goal_achieved_warning_footer.txt",
                GoalEvidence::State(GoalState::Reached),
            ),
            ("codex_goal_cleared.txt", GoalEvidence::Inactive),
            ("codex_no_goal_metadata_footer.txt", GoalEvidence::Inactive),
            (
                "codex_goal_active_wide_footer.txt",
                GoalEvidence::State(GoalState::Active),
            ),
            (
                "codex_goal_achieved_wide_footer.txt",
                GoalEvidence::State(GoalState::Reached),
            ),
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, &fixture(fixture_name)),
                expected,
                "{fixture_name}"
            );
        }
    }

    #[test]
    fn every_codex_footer_goal_phase_is_recognized_between_other_segments() {
        for (segment, expected_state) in [
            ("Pursuing goal (1h 5m)", GoalState::Active),
            ("Goal paused (/goal resume)", GoalState::Paused),
            ("Goal stalled (/goal resume)", GoalState::Blocked),
            ("Goal blocked (/goal resume)", GoalState::Blocked),
            (
                "Goal hit usage limits (/goal resume)",
                GoalState::UsageLimited,
            ),
            ("Goal unmet (12m)", GoalState::UsageLimited),
            ("Goal achieved (20m)", GoalState::Reached),
        ] {
            let screen = format!(
                "• Done\n\n› Ask Codex to do anything\n\n  model · workspace · Ready · Conte… {segment}    ⚠ 1 warning · f2 to view"
            );
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, &screen),
                GoalEvidence::State(expected_state),
                "{segment}"
            );
        }
    }

    /// Transcript rows, command output, and prose that quote goal wording are
    /// never goal evidence: only the footer below the composer is.
    #[test]
    fn codex_goal_wording_outside_the_footer_is_ignored() {
        let transcript_mentions = "\
• Goal paused Objective: Create files one at a time. Time: 1m.
The footer says Pursuing goal (16m) in the bottom-right corner.
Goal achieved (3m)
• Goal cleared

› Ask Codex to do anything

  model · workspace · Working · Context 88% left … Pursuing goal (19s)";
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, transcript_mentions),
            GoalEvidence::State(GoalState::Active)
        );

        // No composer above the last row: that row is not the footer, even
        // when it spells a goal phase.
        for screen in [
            "Goal paused (/goal resume)",
            "model · workspace · Pursuing goal (5m)\nCogitating (esc to interrupt)",
            "",
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, screen),
                GoalEvidence::Unknown,
                "{screen:?}"
            );
        }
    }

    /// A popup that replaces the footer (selection-menu hints, slash-command
    /// lists) is inconclusive rather than proof that the goal ended.
    #[test]
    fn codex_overlays_that_replace_the_footer_are_inconclusive() {
        for screen in [
            "› 1. Trust and continue\n  2. Quit\n\n  enter continue · esc quit",
            "› /goal\n\n  /goal   set or view the goal for a long-running task",
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, screen),
                GoalEvidence::Unknown,
                "{screen:?}"
            );
        }
    }

    #[test]
    fn captured_claude_code_goal_screens_map_to_their_goal_phase() {
        for (fixture_name, expected) in [
            (
                "claude_goal_active_working.txt",
                GoalEvidence::State(GoalState::Active),
            ),
            (
                "claude_goal_active_shared_indicator_row.txt",
                GoalEvidence::State(GoalState::Active),
            ),
            // Esc does not pause a Claude Code goal: the indicator stays.
            (
                "claude_goal_interrupted_still_active.txt",
                GoalEvidence::State(GoalState::Active),
            ),
            (
                "claude_goal_paused_checks_capped.txt",
                GoalEvidence::State(GoalState::Paused),
            ),
            (
                "claude_goal_achieved.txt",
                GoalEvidence::State(GoalState::Reached),
            ),
            ("claude_goal_cleared.txt", GoalEvidence::Inactive),
            ("claude_no_goal_idle.txt", GoalEvidence::Inactive),
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Claude, &fixture(fixture_name)),
                expected,
                "{fixture_name}"
            );
        }
    }

    const CLAUDE_COMPOSER: &str = "\
────────────────────────────────────────
❯
────────────────────────────────────────
  ⏵⏵ auto mode on (shift+tab to cycle)";

    #[test]
    fn claude_code_goal_notices_only_count_inside_the_current_turn() {
        let resumed_after_pause = format!(
            "● Goal paused · usage limit reached · send a message after it resets to continue\n\
             ❯ continue\n\
             ● Working on it again.\n\
             {:>40}\n{CLAUDE_COMPOSER}",
            "◎ /goal active (4m)"
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, &resumed_after_pause),
            GoalEvidence::State(GoalState::Active)
        );

        let usage_limited = format!(
            "● Goal paused · usage limit reached · continues automatically when it resets\n\
             ✻ Baked for 1m 59s · done 12:45 AM\n\
             {:>40}\n{CLAUDE_COMPOSER}",
            "◎ /goal active (4m)"
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, &usage_limited),
            GoalEvidence::State(GoalState::UsageLimited)
        );

        let failed = format!(
            "✗ Goal could not be achieved (3m · 2 turns)\n\
             ✻ Worked for 3m · done 12:45 AM\n\n{CLAUDE_COMPOSER}"
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, &failed),
            GoalEvidence::State(GoalState::Blocked)
        );

        // An achievement from an earlier turn is not current.
        let later_prompt = format!(
            "✔ Goal achieved (3m · 2 turns · 2.6k tokens)\n❯ thanks\n● You're welcome.\n\n{CLAUDE_COMPOSER}"
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, &later_prompt),
            GoalEvidence::Inactive
        );
    }

    #[test]
    fn claude_code_without_a_visible_composer_is_inconclusive() {
        for screen in [
            "◎ /goal active (11s)",
            "● Goal paused · a hook ended the turn · send a message to continue",
            "Do you want to proceed?\n❯ 1. Yes\n  2. No",
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Claude, screen),
                GoalEvidence::Unknown,
                "{screen:?}"
            );
        }
    }

    #[test]
    fn claude_code_goal_wording_outside_the_indicator_row_is_ignored() {
        let screen = format!(
            "● The footer shows ◎ /goal active (11s) while a goal runs.\n{:>40}\n{CLAUDE_COMPOSER}",
            "● high · /effort"
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, &screen),
            GoalEvidence::Inactive
        );
    }

    #[test]
    fn classify_process_name_matches_known_signatures() {
        assert_eq!(
            classify_process_name_with_extra("claude", &[]),
            Some(AgentClass::Claude)
        );
        assert_eq!(
            classify_process_name_with_extra("codex", &[]),
            Some(AgentClass::Codex)
        );
        assert_eq!(
            classify_process_name_with_extra("agy", &[]),
            Some(AgentClass::Antigravity)
        );
        assert_eq!(
            classify_process_name_with_extra("antimatter", &[]),
            Some(AgentClass::Antigravity)
        );
        assert_eq!(
            classify_process_name_with_extra("opencode", &[]),
            Some(AgentClass::Other("opencode".to_string()))
        );
        assert_eq!(
            classify_process_name_with_extra("aider", &[]),
            Some(AgentClass::Other("aider".to_string()))
        );
        assert_eq!(classify_process_name_with_extra("bash", &[]), None);
    }

    #[test]
    fn detailed_classification_retains_the_bounded_line_that_justified_it() {
        let classification = classify_activity_for_agent_detailed(
            &AgentClass::Codex,
            "older transcript\nWorking (esc to interrupt)\u{7}\nSend a message",
        );

        assert_eq!(classification.activity, AgentActivity::Working);
        assert_eq!(classification.evidence, ActivityEvidence::InterruptMarker);
        assert_eq!(
            classification.matched_line.as_deref(),
            Some("Working (esc to interrupt)�")
        );

        let idle = classify_activity_for_agent_detailed(
            &AgentClass::Codex,
            "completed transcript\nSend a message",
        );
        assert_eq!(idle.evidence, ActivityEvidence::NoActiveMarker);
        assert_eq!(idle.matched_line, None);
    }

    #[test]
    fn detailed_goal_classification_retains_the_provider_status_line() {
        let classification = goal_evidence_for_agent_detailed(
            &AgentClass::Codex,
            "old output\n› Send a message\nmodel · workspace · Pursuing goal (16m)",
        );

        assert_eq!(
            classification.evidence,
            GoalEvidence::State(GoalState::Active)
        );
        assert_eq!(
            classification.matched_line.as_deref(),
            Some("model · workspace · Pursuing goal (16m)")
        );
    }

    #[test]
    fn process_matches_retain_the_exact_registry_signature() {
        let matched = match_process_name_with_extra("codex-host", &[])
            .expect("the built-in Codex substring should match");

        assert_eq!(matched.class, AgentClass::Codex);
        assert_eq!(matched.matched_signature, "codex");
    }

    /// `identify_agent` walks a *real* process tree (sysinfo has no fake
    /// backend to inject a synthetic one), so the meaningful thing this
    /// integration-style test can assert without a real agent CLI
    /// installed is the negative case: a plain child with no
    /// `claude`/`codex`/etc. descendant of its own returns `None` rather
    /// than panicking or false-matching.
    ///
    /// Deliberately scoped to a controlled child's own pid rather than
    /// `std::process::id()` -- the *test binary's* pid is shared ambient
    /// state every test in this module runs under, and cargo's default
    /// parallel test execution means another test's own spawned
    /// subprocess (e.g. the wrapper/native pair below) can be alive as a
    /// descendant of the test binary at the same moment this assertion
    /// runs, which would make this test flaky through no fault of its own.
    #[test]
    fn identify_agent_returns_none_when_no_agent_descendant_exists() {
        let mut plain_child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .expect("spawn plain child");

        let mut system = System::new_all();
        system.refresh_all();
        assert_eq!(
            identify_agent(&system, Pid::from_u32(plain_child.id())),
            None
        );

        let _ = plain_child.kill();
        let _ = plain_child.wait();
    }

    /// Reproduces a real installer shape (Bun's global bin shim): a launcher
    /// invoked as `node <path-ending-in-codex>` sits directly on the pane's
    /// shell, matched only by unwrapping its argv (`identifying_process_names`),
    /// and it *spawns* the real native CLI as a further child rather than
    /// exec-replacing itself. The launcher's own kernel name is "node" -- it
    /// never held a transcript file open, so picking it left every later
    /// session-ID discovery permanently empty. The native child, one level
    /// deeper, must win even though it's not the shallowest match.
    ///
    /// Unix-only: the wrapper and native binaries here are `#!/bin/sh`
    /// scripts made executable via `PermissionsExt::from_mode`, which
    /// Windows has no equivalent of -- same justification as
    /// `ilium-detect/tests/script_agent_identity.rs`'s file-level
    /// `#![cfg(unix)]`.
    #[test]
    #[cfg(unix)]
    fn identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        let node_path = tmp.path().join("node");
        let native_path = tmp.path().join("codex-native");
        std::fs::write(&native_path, "#!/bin/sh\nsleep 5\n").expect("write native script");
        std::fs::write(
            &node_path,
            format!("#!/bin/sh\n\"{}\" &\nwait\n", native_path.display()),
        )
        .expect("write wrapper script");
        std::fs::set_permissions(&node_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod wrapper");
        std::fs::set_permissions(&native_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod native");

        // The argument is never executed -- it only needs a file name
        // containing "codex" so the wrapper matches via
        // `interpreted_program_name`, exactly like Bun's shim being handed
        // its own script path as `node`'s argument.
        let mut wrapper = std::process::Command::new(&node_path)
            .arg(tmp.path().join("codex-shim"))
            .spawn()
            .expect("spawn wrapper");

        // Give the wrapper's own shell body time to actually fork its child
        // before the process table is sampled.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut system = System::new_all();
        system.refresh_all();
        let identity = identify_agent(&system, Pid::from_u32(wrapper.id()))
            .expect("the native child must be found as a codex-matching descendant");

        assert_eq!(identity.class, AgentClass::Codex);
        assert_eq!(identity.process_name, "codex-native");

        let _ = wrapper.kill();
        let _ = wrapper.wait();
    }

    /// A name that matches no built-in signature is only classified once a
    /// matching extra (e.g. user-configured) signature is supplied
    /// alongside the built-in table -- this is the registry extension
    /// point `ilium-server/src/config.rs`'s custom signatures ride on.
    #[test]
    fn classify_process_name_with_extra_matches_a_caller_supplied_signature() {
        let custom = AgentSignature {
            name_substring: Cow::Owned("mytool".to_string()),
            class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
        };

        assert_eq!(classify_process_name_with_extra("mytool", &[]), None);
        assert_eq!(
            classify_process_name_with_extra("mytool", &[custom]),
            Some(AgentClass::Other("mytool".to_string()))
        );
    }

    /// A built-in signature always wins over an extra one for the same
    /// substring -- extras extend the registry, they never shadow it.
    #[test]
    fn classify_process_name_with_extra_never_shadows_a_built_in_signature() {
        let custom = AgentSignature {
            name_substring: Cow::Owned("claude".to_string()),
            class_of: |matched_name| AgentClass::Other(matched_name.to_string()),
        };

        assert_eq!(
            classify_process_name_with_extra("claude", &[custom]),
            Some(AgentClass::Claude)
        );
    }
}
