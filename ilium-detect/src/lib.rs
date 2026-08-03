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

use ilium_core::{AgentActivity, AgentClass, AgentProvider, BuiltinAgentProvider};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

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
/// the rule unambiguous either way. Next, a background-wait line (see
/// `looks_like_background_wait_line`) means the agent dispatched
/// subagents/background tasks and is waiting on them, not actively
/// streaming foreground output. Absent both, either a y/n-style
/// confirmation box or a general multiple-choice/question prompt (see
/// `looks_like_confirmation_prompt` and `looks_like_selection_prompt`)
/// means the agent is blocked waiting on the user. Anything else is
/// `Idle`.
pub fn classify_activity(screen_text: &str) -> AgentActivity {
    classify_activity_detailed(screen_text).activity
}

pub fn classify_activity_detailed(screen_text: &str) -> ActivityClassification {
    let (activity, evidence) = if screen_text.contains(WORKING_MARKER) {
        (AgentActivity::Working, ActivityEvidence::InterruptMarker)
    } else if looks_like_live_status_line(screen_text) {
        (AgentActivity::Working, ActivityEvidence::GenericLiveStatus)
    } else if looks_like_background_wait_line(screen_text) {
        (
            AgentActivity::WaitingBackground,
            ActivityEvidence::BackgroundWait,
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
    let (activity, evidence) = if screen_text.contains(WORKING_MARKER) {
        (AgentActivity::Working, ActivityEvidence::InterruptMarker)
    } else if matches!(class, AgentClass::Claude) && looks_like_live_status_line(screen_text) {
        (AgentActivity::Working, ActivityEvidence::ClaudeLiveStatus)
    } else if matches!(class, AgentClass::Codex) && looks_like_codex_live_status_line(screen_text) {
        (AgentActivity::Working, ActivityEvidence::CodexLiveStatus)
    } else if looks_like_background_wait_line(screen_text) {
        (
            AgentActivity::WaitingBackground,
            ActivityEvidence::BackgroundWait,
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

/// Recovers the exact short terminal-chrome line behind a positive evidence
/// code. This intentionally runs after the cheap classifier has selected one
/// rule, keeping the public evidence complete without making every predicate
/// allocate while it searches.
fn activity_evidence_line(evidence: ActivityEvidence, screen_text: &str) -> Option<String> {
    let matched_line = match evidence {
        ActivityEvidence::InterruptMarker => screen_text
            .lines()
            .find(|line| line.contains(WORKING_MARKER)),
        ActivityEvidence::GenericLiveStatus | ActivityEvidence::ClaudeLiveStatus => screen_text
            .lines()
            .find(|line| looks_like_live_status_line(line)),
        ActivityEvidence::CodexLiveStatus => screen_text
            .lines()
            .find(|line| looks_like_codex_live_status_line(line)),
        ActivityEvidence::BackgroundWait => screen_text.lines().find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("waiting for")
                && lower.contains("background")
                && (lower.contains("agent") || lower.contains("task"))
        }),
        ActivityEvidence::ConfirmationPrompt => screen_text
            .lines()
            .find(|line| looks_like_confirmation_prompt(line)),
        ActivityEvidence::SelectionPrompt => screen_text.lines().find(|line| {
            let lower = line.to_ascii_lowercase();
            let is_footer = (lower.contains("to select")
                || lower.contains("to confirm")
                || lower.contains("to choose"))
                && lower.contains("cancel");
            is_footer
                || (line.trim_start().starts_with('\u{276f}') && is_numbered_option_line(line))
        }),
        ActivityEvidence::NoActiveMarker => None,
    }?;
    Some(bounded_terminal_evidence(matched_line))
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
pub fn is_agent_prompt_ready(class: &AgentClass, screen_text: &str) -> bool {
    let normalized = screen_text.to_ascii_lowercase();
    let composer_visible = match class {
        AgentClass::Codex => {
            normalized.contains("send a message") || screen_has_codex_composer_cursor(screen_text)
        }
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

/// Current Codex releases rotate contextual placeholder text instead of
/// retaining the older literal `Send a message` label. The stable composer
/// contract is its leading `›` cursor; numbered modal choices are excluded so
/// an approval surface cannot masquerade as the free-form composer.
fn screen_has_codex_composer_cursor(screen_text: &str) -> bool {
    screen_text.lines().any(|line| {
        let Some(composer_content) = line.trim_start().strip_prefix('›') else {
            return false;
        };
        !is_numbered_option_line(composer_content)
    })
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
    Active,
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
/// Goal status may be a standalone row or the final segment of a shared footer.
/// Current wide Codex layouts render metadata first and end the same line with
/// `Pursuing goal (16m)`, so matching only the beginning of a line is
/// width-dependent. Lines are inspected newest-first: a current `Goal achieved`
/// footer must override an older positive row that is still visible above it.
pub fn goal_evidence_for_agent(class: &AgentClass, screen_text: &str) -> GoalEvidence {
    goal_evidence_for_agent_detailed(class, screen_text).evidence
}

pub fn goal_evidence_for_agent_detailed(
    class: &AgentClass,
    screen_text: &str,
) -> GoalClassification {
    for original_line in screen_text.lines().rev() {
        let line = normalize_goal_status_line(original_line);
        let evidence = match class {
            AgentClass::Codex => codex_goal_evidence(&line),
            AgentClass::Claude => claude_goal_evidence(&line),
            AgentClass::Antigravity | AgentClass::Other(_) => GoalEvidence::Unknown,
        };
        if evidence != GoalEvidence::Unknown {
            return GoalClassification {
                evidence,
                matched_line: Some(bounded_terminal_evidence(original_line)),
            };
        }
    }
    GoalClassification {
        evidence: GoalEvidence::Unknown,
        matched_line: None,
    }
}

/// Recognizes Codex's active, paused, blocked, limited, and completed footer
/// phases. Exact elapsed-time suffixes are high-signal terminal chrome and may
/// appear after arbitrary model/cwd/context metadata on the same line.
fn codex_goal_evidence(line: &str) -> GoalEvidence {
    if has_elapsed_status_suffix(line, "goal achieved (")
        || line_ends_with_status(line, "goal achieved")
    {
        return GoalEvidence::Inactive;
    }

    if [
        "pursuing goal (",
        "goal paused (",
        "goal blocked (",
        "goal hit usage limits (",
    ]
    .iter()
    .any(|marker| has_elapsed_status_suffix(line, marker))
        || line.starts_with("goal active objective:")
        || line_ends_with_status(line, "goal paused (/goal resume)")
        || line_ends_with_status(line, "goal blocked (/goal resume)")
        || line_ends_with_status(line, "goal hit usage limits (/goal resume)")
        || line
            .strip_prefix("goal:")
            .is_some_and(|goal_value| !goal_value.trim().is_empty())
    {
        return GoalEvidence::Active;
    }

    GoalEvidence::Unknown
}

/// Recognizes Claude Code's provider-owned active-goal footer forms without
/// requiring them to be the first status segment on the line.
fn claude_goal_evidence(line: &str) -> GoalEvidence {
    if has_elapsed_status_suffix(line, "goal active (") || line.starts_with("goal active:") {
        GoalEvidence::Active
    } else {
        GoalEvidence::Unknown
    }
}

/// Normalizes case and removes decoration only from the beginning of a line.
/// Interior separators remain available to establish status-segment boundaries.
fn normalize_goal_status_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase()
}

/// Matches a status label followed by a well-formed elapsed-time token at the
/// end of a line. This accepts a footer suffix after arbitrary metadata while
/// rejecting ordinary prose such as "the footer says Pursuing goal (16m) here".
fn has_elapsed_status_suffix(line: &str, marker: &str) -> bool {
    let Some(marker_index) = line.rfind(marker) else {
        return false;
    };
    if !has_status_boundary_before(line, marker_index) {
        return false;
    }

    let duration_and_tail = &line[marker_index + marker.len()..];
    let Some(closing_parenthesis) = duration_and_tail.find(')') else {
        return false;
    };
    let duration = &duration_and_tail[..closing_parenthesis];
    let trailing = &duration_and_tail[closing_parenthesis + 1..];
    !duration.is_empty()
        && duration.chars().any(|character| character.is_ascii_digit())
        && duration.chars().all(|character| {
            character.is_ascii_digit()
                || character.is_ascii_whitespace()
                || matches!(character, '.' | ':' | 'd' | 'h' | 'm' | 's')
        })
        && trailing
            .chars()
            .all(|character| character.is_whitespace() || !character.is_alphanumeric())
}

/// Matches an exact terminal status suffix after optional shared-footer chrome.
fn line_ends_with_status(line: &str, marker: &str) -> bool {
    let trimmed = line.trim_end_matches(|character: char| {
        character.is_whitespace() || (!character.is_alphanumeric() && character != ')')
    });
    let Some(marker_index) = trimmed.rfind(marker) else {
        return false;
    };
    has_status_boundary_before(trimmed, marker_index)
        && marker_index + marker.len() == trimmed.len()
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
    screen_text.lines().any(|line| {
        // Use to_ascii_lowercase() for ASCII terminal output (faster than to_lowercase() for typical case)
        let lower = line.to_ascii_lowercase();
        lower.contains("waiting for")
            && lower.contains("background")
            && (lower.contains("agent") || lower.contains("task"))
    })
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

/// True when Codex's visible status line explicitly names an active turn.
///
/// Codex keeps completed timing summaries on screen, often alongside an
/// ellipsis and an elapsed-time token. Requiring a present-tense activity word
/// prevents those historical rows from being mistaken for a live turn.
fn looks_like_codex_live_status_line(screen_text: &str) -> bool {
    screen_text.lines().any(|line| {
        // Use to_ascii_lowercase() for ASCII terminal output; cache once per line
        let lower = line.trim().to_ascii_lowercase();
        let names_active_turn = ["thinking", "working", "generating", "planning", "running"]
            .iter()
            .any(|marker| lower.starts_with(marker));
        names_active_turn
            && (lower.contains("…") || lower.contains("..."))
            && lower
                .split(|character: char| character.is_whitespace() || character == '·')
                .any(is_elapsed_time_token)
    })
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
    screen_text.lines().any(|line| {
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
    // Check for selection footer first (early exit, avoids Vec collection if found)
    if screen_text.lines().any(|line| {
        let lower = line.to_lowercase();
        let names_a_confirm_action = lower.contains("to select")
            || lower.contains("to confirm")
            || lower.contains("to choose");
        names_a_confirm_action && lower.contains("cancel")
    }) {
        return true;
    }

    // Only collect lines if footer wasn't found
    let lines: Vec<&str> = screen_text.lines().collect();
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
/// returns the first match found.
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
pub fn identify_agent_with_extra(
    system: &System,
    shell_pid: Pid,
    children_index: &ProcessChildrenIndex,
    extra_signatures: &[AgentSignature],
) -> Option<AgentIdentity> {
    // Tracks the best match found so far as (depth, pid) plus its class --
    // the CLI process is closer to the pane shell than its internal helper
    // processes (for example Codex's code-mode host), so lower depth (and,
    // tie-broken, lower pid) wins, matching the old `min_by_key((depth,
    // pid))` semantics exactly.
    let mut best: Option<((usize, u32), Pid, String, AgentProcessMatch)> = None;
    let mut queue: VecDeque<(Pid, usize)> = VecDeque::new();
    let mut visited: HashSet<Pid> = HashSet::new();
    queue.push_back((shell_pid, 0));
    visited.insert(shell_pid);

    while let Some((pid, depth)) = queue.pop_front() {
        if let Some(((best_depth, _), _, _, _)) = &best {
            if depth > *best_depth {
                break;
            }
        }
        if let Some(process) = system.process(pid) {
            // Lowercased once per process, avoiding repeated allocation per
            // classification attempt. See `identifying_process_names` for why
            // more than the kernel name has to be considered.
            let matched = identifying_process_names(process)
                .into_iter()
                .find_map(|candidate| {
                    match_process_name_with_extra(&candidate, extra_signatures)
                        .map(|process_match| (candidate, process_match))
                });
            if let Some((matched_name, process_match)) = matched {
                let key = (depth, pid.as_u32());
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
        |((depth, _), pid, process_name, process_match)| AgentIdentity {
            pid: pid.as_u32(),
            class: process_match.class,
            process_name,
            matched_signature: process_match.matched_signature,
            process_tree_depth: depth,
        },
    )
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
/// The first command-line argument closes that gap, because it is the path the
/// process was actually invoked as. Only its file name is considered, and it is
/// only consulted when the kernel name matched nothing, so `sh -c "codex …"`
/// cannot masquerade as the agent: its first argument is the shell itself.
fn identifying_process_names(process: &sysinfo::Process) -> Vec<String> {
    let arguments = process.cmd();
    let invoked = argument_file_name(arguments, 0);
    // When the first argument is an interpreter, the *script* it was handed is
    // the name worth matching. That is exactly how the kernel launches a
    // shebang script: executing `#!/bin/sh /path/codex` builds an argument
    // vector of `["/bin/sh", "/path/codex"]`, so the agent's own name is the
    // second entry, never the first.
    let script = invoked
        .as_deref()
        .is_some_and(is_interpreter_name)
        .then(|| argument_file_name(arguments, 1))
        .flatten();

    let mut candidates = vec![process.name().to_string_lossy().to_lowercase()];
    for candidate in [script, invoked].into_iter().flatten() {
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
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
    fn claude_code_waiting_on_background_agents_is_waiting_background() {
        assert_eq!(
            classify_activity(&fixture("claude_code_waiting_background.txt")),
            AgentActivity::WaitingBackground
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
        assert!(is_agent_prompt_ready(
            &AgentClass::Codex,
            &fixture("codex_dynamic_placeholder_idle.txt")
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
    fn visible_goal_status_is_detected_for_current_codex_and_claude_code() {
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Codex,
                "  • Goal active Objective: Finish the detection pass"
            ),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, "  ◒ Pursuing goal (5m)"),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, "🏁 Goal: keep the goal paused"),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Claude, "◎ /goal active (11s)"),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Claude,
                " /goal active: finish the detection pass"
            ),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, "Goal achieved"),
            GoalEvidence::Inactive
        );
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Codex,
                "This thread does not currently have a goal."
            ),
            GoalEvidence::Unknown
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, "Goal:"),
            GoalEvidence::Unknown
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, "The project's goal: keep tests green."),
            GoalEvidence::Unknown
        );
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Claude,
                "Goal set: an old transcript row is still visible"
            ),
            GoalEvidence::Unknown
        );
    }

    #[test]
    fn codex_goal_footer_is_detected_after_wide_layout_metadata() {
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Codex,
                &fixture("codex_goal_active_wide_footer.txt")
            ),
            GoalEvidence::Active
        );
        assert_eq!(
            goal_evidence_for_agent(
                &AgentClass::Codex,
                "gpt-5.6-sol xhigh · workspace · Waiting · Context … Pursuing goal (3m)"
            ),
            GoalEvidence::Active,
            "a narrow footer can collapse hidden metadata into an ellipsis"
        );
    }

    #[test]
    fn newest_explicit_completion_overrides_an_older_visible_goal_row() {
        let screen = format!(
            "Pursuing goal (5m)\n{}",
            fixture("codex_goal_achieved_wide_footer.txt")
        );
        assert_eq!(
            goal_evidence_for_agent(&AgentClass::Codex, &screen),
            GoalEvidence::Inactive
        );
    }

    #[test]
    fn goal_phases_remain_attached_while_paused_blocked_or_limited() {
        for status in [
            "Goal paused (/goal resume)",
            "Goal blocked (/goal resume)",
            "Goal hit usage limits (/goal resume)",
            "model · workspace · Goal paused (2m)",
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, status),
                GoalEvidence::Active,
                "expected an attached goal for {status:?}"
            );
        }
    }

    #[test]
    fn prose_that_only_mentions_a_goal_footer_is_inconclusive() {
        for prose in [
            "The footer says Pursuing goal (16m) in the bottom-right corner.",
            "The footer says Pursuing goal (16m).",
        ] {
            assert_eq!(
                goal_evidence_for_agent(&AgentClass::Codex, prose),
                GoalEvidence::Unknown
            );
        }
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
            "old output\nmodel · workspace · Pursuing goal (16m)",
        );

        assert_eq!(classification.evidence, GoalEvidence::Active);
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
