//! Screen classification vendored from `ilium-detect` (pure text rules): activity, evidence, prompt readiness,
//! interstitial answers and goal phases. The plugin crate is a wasm component and cannot link the host-side crate.

use unicode_width::UnicodeWidthChar;

/// The agent classes the screen rules distinguish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentClass {
    Claude,
    Codex,
    Antigravity,
    Other(String),
}

impl AgentClass {
    /// The class of a registry signature id.
    pub fn from_signature(signature: &str) -> AgentClass {
        match signature {
            "claude" => AgentClass::Claude,
            "codex" => AgentClass::Codex,
            "antigravity" => AgentClass::Antigravity,
            other => AgentClass::Other(other.to_string()),
        }
    }
}

/// The provider turn phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurn {
    Working,
    WaitingApproval,
    WaitingSubagents,
    Settling,
    Idle,
}

/// The phase of a provider goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalState {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    Reached,
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
    FolderTrustPrompt,
    NoActiveMarker,
}

impl ActivityEvidence {
    /// Names the predicate that actually selected this activity. These are
    /// structural and substring rules, not regular expressions.
    pub const fn description(self) -> &'static str {
        match self {
            Self::InterruptMarker => "screen line contains the exact marker esc to interrupt",
            Self::GenericLiveStatus => {
                "generic live status line contains an ellipsis and elapsed-time token"
            }
            Self::ClaudeLiveStatus => {
                "Claude live status line contains an ellipsis and elapsed-time token"
            }
            Self::CodexLiveStatus => {
                "Codex present-tense working status line contains an elapsed-time token"
            }
            Self::BackgroundWait => "newest eligible status line starts with waiting for and contains background and agent or task",
            Self::BackgroundTaskWait => {
                "completed-turn summary line contains a numbered still running suffix"
            }
            Self::ConfirmationPrompt => "visible confirmation prompt has a yes/no choice",
            Self::SelectionPrompt => {
                "visible interactive menu has selection or select/cancel chrome"
            }
            Self::FolderTrustPrompt => "Claude folder trust prompt is visible",
            Self::NoActiveMarker => {
                "none of the interrupt marker, provider live-status, background-wait, post-turn running-suffix, confirmation, Claude folder-trust, or selection predicates matched"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityClassification {
    pub turn: AgentTurn,
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
pub const WORKING_MARKER: &str = "esc to interrupt";

/// Scans a pane's plain-text screen contents (as returned by
/// `vt100::Screen::contents()`) for activity markers and classifies it.
///
/// A bottom-anchored question dialog wins over stale activity chrome.
/// Otherwise working signals precede background waits. A recognized Claude
/// completed-turn summary supersedes earlier activity rows while retaining
/// its own still-running suffix and every later row. Approval checks retain
/// the whole screen. Next, a background-wait line
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
pub fn classify_activity(screen_text: &str) -> AgentTurn {
    classify_activity_detailed(screen_text).turn
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
pub fn classify_activity_for_agent(class: &AgentClass, screen_text: &str) -> AgentTurn {
    classify_activity_for_agent_detailed(class, screen_text).turn
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
pub fn classify_screen_activity(
    class: Option<&AgentClass>,
    screen_text: &str,
) -> ActivityClassification {
    let status_text = current_status_text(class, screen_text);
    let (turn, evidence) = if looks_like_bottom_anchored_question_dialog(screen_text) {
        // A modal question dialog blocks the turn no matter what stale status
        // chrome ("esc to interrupt", a spinner row) is still drawn above it.
        (
            AgentTurn::WaitingApproval,
            ActivityEvidence::SelectionPrompt,
        )
    } else if status_text.contains(WORKING_MARKER) {
        (AgentTurn::Working, ActivityEvidence::InterruptMarker)
    } else if let Some(live_status) = live_status_evidence(class, status_text) {
        (AgentTurn::Working, live_status)
    } else if looks_like_background_wait_line(status_text) {
        (
            AgentTurn::WaitingSubagents,
            ActivityEvidence::BackgroundWait,
        )
    } else if looks_like_background_task_wait_line(status_text) {
        (AgentTurn::Settling, ActivityEvidence::BackgroundTaskWait)
    } else if looks_like_confirmation_prompt(screen_text) {
        (
            AgentTurn::WaitingApproval,
            ActivityEvidence::ConfirmationPrompt,
        )
    } else if looks_like_claude_folder_trust_prompt(screen_text) {
        (
            AgentTurn::WaitingApproval,
            ActivityEvidence::FolderTrustPrompt,
        )
    } else if looks_like_selection_prompt(screen_text) {
        (
            AgentTurn::WaitingApproval,
            ActivityEvidence::SelectionPrompt,
        )
    } else {
        (AgentTurn::Idle, ActivityEvidence::NoActiveMarker)
    };
    let evidence_text = match evidence {
        ActivityEvidence::ConfirmationPrompt
        | ActivityEvidence::SelectionPrompt
        | ActivityEvidence::FolderTrustPrompt => screen_text,
        _ => status_text,
    };
    ActivityClassification {
        turn,
        evidence,
        matched_line: activity_evidence_line(evidence, evidence_text),
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
pub fn live_status_evidence(class: Option<&AgentClass>, screen_text: &str) -> Option<ActivityEvidence> {
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
pub fn activity_evidence_line(evidence: ActivityEvidence, screen_text: &str) -> Option<String> {
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
        ActivityEvidence::FolderTrustPrompt => is_claude_folder_trust_choice_line,
        ActivityEvidence::NoActiveMarker => return None,
    };
    let mut matching_lines = screen_text
        .lines()
        .filter(|line| line_matches_evidence(line));
    let matched_line = match evidence {
        ActivityEvidence::ConfirmationPrompt
        | ActivityEvidence::SelectionPrompt
        | ActivityEvidence::FolderTrustPrompt => matching_lines.next(),
        // Report the newest eligible activity row, matching the chronology rule.
        _ => matching_lines.next_back(),
    };
    matched_line.map(bounded_terminal_evidence)
}

/// Keeps durable diagnostic excerpts useful and safe for terminal rendering.
/// The excerpt is evidence, not a transcript dump.
pub fn bounded_terminal_evidence(line: &str) -> String {
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
            AgentTurn::Working | AgentTurn::WaitingSubagents | AgentTurn::WaitingApproval
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
/// present in `dimmed_cells`. Antigravity exposes an empty composer as `>`;
/// its cursor must be immediately after that reserved prefix. Rows and columns
/// are zero-based, matching `vt100::Screen::cursor_position`. Claude and other
/// providers retain their text-only contracts.
pub fn is_agent_prompt_ready_at_cursor(
    class: &AgentClass,
    screen_text: &str,
    cursor_row: u16,
    cursor_column: u16,
    dimmed_cells: &[(u16, u16)],
) -> bool {
    if matches!(class, AgentClass::Antigravity) {
        let Some(composer_line) = screen_text.lines().nth(usize::from(cursor_row)) else {
            return false;
        };
        let leading_spaces = composer_line.len() - composer_line.trim_start_matches(' ').len();
        let expected_cursor_column = leading_spaces
            .checked_add(2)
            .and_then(|column| u16::try_from(column).ok());
        let composer_is_empty = composer_line[leading_spaces..].trim_end() == ">"
            && expected_cursor_column == Some(cursor_column)
            && !looks_like_selection_prompt(screen_text);
        return composer_is_empty
            && !matches!(
                classify_activity_for_agent(class, screen_text),
                AgentTurn::Working | AgentTurn::WaitingSubagents | AgentTurn::WaitingApproval
            );
    }
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
            AgentTurn::Working | AgentTurn::WaitingSubagents | AgentTurn::WaitingApproval
        )
}

/// One known one-time interstitial dialog a first-party agent CLI shows
/// outside its normal turn lifecycle -- distinct from
/// `classify_activity_for_agent`'s ongoing working/idle/approval states
/// because dialogs like this appear once, only at session (re)start, before
/// any turn exists to classify. A new known dialog is a new entry in
/// `INTERSTITIAL_PROMPTS`, not a new branch in an if/else chain -- same
/// registry shape as `GENERIC_AGENT_SIGNATURES`.
pub struct InterstitialPrompt {
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
pub const INTERSTITIAL_PROMPT_ANCHOR_ROWS: usize = 10;

pub const INTERSTITIAL_PROMPTS: &[InterstitialPrompt] = &[
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
pub fn screen_has_unambiguously_empty_codex_composer(screen_text: &str) -> bool {
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
pub fn screen_has_empty_codex_composer_at_cursor(
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

pub fn codex_legacy_composer_box_is_empty(lines_before_label: &[&str]) -> bool {
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

pub fn is_codex_box_border(line: &str, left_corner: char, right_corner: char) -> bool {
    line.starts_with(left_corner)
        && line.ends_with(right_corner)
        && line.chars().all(|character| {
            matches!(character, '─' | ' ') || character == left_corner || character == right_corner
        })
}

pub fn is_empty_codex_box_row(line: &str) -> bool {
    line.starts_with('│')
        && line.ends_with('│')
        && line.chars().all(|character| matches!(character, '│' | ' '))
}

/// Claude's empty composer is a bordered terminal field whose content row is
/// just `>` (older builds) or `❯` (current builds). Requiring that isolated
/// row avoids confusing a prose quote or a numbered selector with a composer.
pub fn screen_has_claude_composer_cursor(screen_text: &str) -> bool {
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
pub fn screen_has_only_empty_composer_chrome(screen_text: &str, composer_label: &str) -> bool {
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
    /// Exact predicate family that selected the reported provider-owned line.
    /// `None` means the screen supplied no decisive goal evidence.
    pub matched_rule: Option<GoalEvidenceRule>,
    /// Literal marker tested inside the structurally selected provider row.
    /// This is `None` for negative or inconclusive evidence.
    pub matched_pattern: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalEvidenceRule {
    CodexFooterGoalSegment,
    CodexMetadataFooterWithoutGoal,
    ClaudeGoalIndicator,
    ClaudePausedNotice,
    ClaudeAchievedNotice,
    ClaudeFailedNotice,
    ClaudeClearedNotice,
    ClaudeIndicatorWithoutGoal,
}

impl GoalEvidenceRule {
    pub const fn description(self) -> &'static str {
        match self {
            Self::CodexFooterGoalSegment => "rightmost Codex goal segment in the status footer below its composer",
            Self::CodexMetadataFooterWithoutGoal => "Codex metadata footer below its composer contains no goal segment",
            Self::ClaudeGoalIndicator => "Claude /goal active indicator directly above its composer",
            Self::ClaudePausedNotice => "newest current-turn Claude goal notice starts with Goal paused",
            Self::ClaudeAchievedNotice => "newest current-turn Claude goal notice starts with Goal achieved",
            Self::ClaudeFailedNotice => "newest current-turn Claude goal notice starts with Goal could not be achieved",
            Self::ClaudeClearedNotice => "newest current-turn Claude goal notice starts with Goal cleared",
            Self::ClaudeIndicatorWithoutGoal => "Claude indicator row directly above its composer has no goal and no current-turn goal notice",
        }
    }
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
            matched_rule: None,
            matched_pattern: None,
        };
    };
    let lines: Vec<&str> = screen_text.lines().collect();
    goal_evidence_of_screen(&lines)
}

/// The provider-owned goal reader, or `None` for a provider that renders no
/// goal status at all.
pub fn provider_goal_reader(class: &AgentClass) -> Option<fn(&[&str]) -> GoalClassification> {
    match class {
        AgentClass::Codex => Some(codex_goal_classification),
        AgentClass::Claude => Some(claude_goal_classification),
        AgentClass::Antigravity | AgentClass::Other(_) => None,
    }
}

pub fn goal_classification(
    evidence: GoalEvidence,
    line: Option<&str>,
    matched_rule: Option<GoalEvidenceRule>,
    matched_pattern: Option<&'static str>,
) -> GoalClassification {
    GoalClassification {
        evidence,
        matched_line: line.map(bounded_terminal_evidence),
        matched_rule,
        matched_pattern,
    }
}

/// How far above Codex's footer its composer may start. The composer grows
/// with a multi-line draft; anything further away is not the live composer.
pub const CODEX_COMPOSER_SEARCH_ROWS: usize = 10;

/// Codex renders its status footer below the `›` composer. A separate
/// `? for shortcuts` hint row may follow it, so the status row is not always
/// the last non-blank screen row. The goal segment is right-pinned in the
/// status row: wide layouts truncate metadata on its left with `…`.
pub fn codex_goal_classification(lines: &[&str]) -> GoalClassification {
    let Some(footer) = codex_footer_row(lines) else {
        return goal_classification(GoalEvidence::Unknown, None, None, None);
    };
    let normalized_footer = footer.to_lowercase();
    if let Some((goal_state, marker)) = codex_footer_goal_state(&normalized_footer) {
        return goal_classification(
            GoalEvidence::State(goal_state),
            Some(footer),
            Some(GoalEvidenceRule::CodexFooterGoalSegment),
            Some(marker),
        );
    }
    // A goal-free footer is decisive only when it is recognizably Codex's
    // metadata status line. Popups that replace the footer (slash-command
    // lists, key hints under a selection menu) stay inconclusive so a
    // transient overlay never clears a confirmed goal.
    if is_codex_metadata_footer(&normalized_footer) {
        return goal_classification(
            GoalEvidence::Inactive,
            Some(footer),
            Some(GoalEvidenceRule::CodexMetadataFooterWithoutGoal),
            None,
        );
    }
    goal_classification(GoalEvidence::Unknown, None, None, None)
}

/// The status footer row, provided the live composer sits a few rows above it.
pub fn codex_footer_row<'screen>(lines: &[&'screen str]) -> Option<&'screen str> {
    let last_index = lines.iter().rposition(|line| !line.trim().is_empty())?;
    let footer_index = if lines[last_index]
        .trim_start()
        .starts_with("? for shortcuts")
    {
        (0..last_index)
            .rev()
            .find(|&index| !lines[index].trim().is_empty())?
    } else {
        last_index
    };
    let search_start = footer_index.saturating_sub(CODEX_COMPOSER_SEARCH_ROWS);
    lines[search_start..footer_index]
        .iter()
        .any(|line| is_codex_composer_row(line))
        .then_some(lines[footer_index])
}

/// The composer row starts with `›`. Numbered menu options use the same
/// selection glyph (`› 1. Trust and continue`) and are rejected so a modal's
/// hint row is not mistaken for the footer.
pub fn is_codex_composer_row(line: &str) -> bool {
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
pub fn codex_footer_goal_state(normalized_footer: &str) -> Option<(GoalState, &'static str)> {
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
            .map(|index| (index, *goal_state, *marker))
    });
    let exact_matches = EXACT_MARKERS.iter().filter_map(|(marker, goal_state)| {
        rightmost_segment_start(normalized_footer, marker)
            .map(|index| (index, *goal_state, *marker))
    });
    elapsed_matches
        .chain(exact_matches)
        .max_by_key(|(index, _, _)| *index)
        .map(|(_, goal_state, marker)| (goal_state, marker))
}

/// The last occurrence of `marker` that begins a word, so `repursuing goal (`
/// cannot match inside a longer token.
pub fn rightmost_segment_start(line: &str, marker: &str) -> Option<usize> {
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
pub fn has_elapsed_token_after(line: &str, duration_start: usize) -> bool {
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
pub fn is_codex_metadata_footer(normalized_footer: &str) -> bool {
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
pub const CLAUDE_TURN_END_SEARCH_ROWS: usize = 30;

/// Claude Code renders `◎ /goal active (2m)` right-aligned on the indicator
/// row directly above the composer's top rule for as long as a goal is set.
/// That row cannot express any other phase, so the closing notices of the
/// current turn — the rows between the indicator and the last user prompt —
/// refine it: `Goal paused · <reason>` while the indicator persists, and
/// `Goal achieved` / `Goal could not be achieved` / `Goal cleared` once it is
/// gone.
pub fn claude_goal_classification(lines: &[&str]) -> GoalClassification {
    let Some(indicator_index) = claude_indicator_row_index(lines) else {
        return goal_classification(GoalEvidence::Unknown, None, None, None);
    };
    let indicator = lines[indicator_index];
    let turn_end = claude_turn_end_goal_notice(lines, indicator_index);
    if claude_indicator_shows_goal(indicator) {
        return match turn_end {
            Some((ClaudeGoalNotice::Paused(goal_state), line)) => goal_classification(
                GoalEvidence::State(goal_state),
                Some(line),
                Some(GoalEvidenceRule::ClaudePausedNotice),
                Some("goal paused"),
            ),
            _ => goal_classification(
                GoalEvidence::State(GoalState::Active),
                Some(indicator),
                Some(GoalEvidenceRule::ClaudeGoalIndicator),
                Some("/goal active"),
            ),
        };
    }
    match turn_end {
        Some((ClaudeGoalNotice::Achieved, line)) => goal_classification(
            GoalEvidence::State(GoalState::Reached),
            Some(line),
            Some(GoalEvidenceRule::ClaudeAchievedNotice),
            Some("goal achieved"),
        ),
        Some((ClaudeGoalNotice::Failed, line)) => goal_classification(
            GoalEvidence::State(GoalState::Blocked),
            Some(line),
            Some(GoalEvidenceRule::ClaudeFailedNotice),
            Some("goal could not be achieved"),
        ),
        Some((ClaudeGoalNotice::Cleared, line)) => goal_classification(
            GoalEvidence::Inactive,
            Some(line),
            Some(GoalEvidenceRule::ClaudeClearedNotice),
            Some("goal cleared"),
        ),
        // A notice that the goal is still running (or paused) without its
        // indicator is a redraw in progress, not proof the goal ended.
        Some((ClaudeGoalNotice::Running | ClaudeGoalNotice::Paused(_), _)) => {
            goal_classification(GoalEvidence::Unknown, None, None, None)
        }
        None => goal_classification(
            GoalEvidence::Inactive,
            Some(indicator),
            Some(GoalEvidenceRule::ClaudeIndicatorWithoutGoal),
            None,
        ),
    }
}

/// The row immediately above the rule that tops the last `❯` composer.
pub fn claude_indicator_row_index(lines: &[&str]) -> Option<usize> {
    let composer_index = (2..lines.len()).rev().find(|&index| {
        lines[index].trim_start().starts_with('❯') && is_horizontal_rule(lines[index - 1])
    })?;
    Some(composer_index - 2)
}

pub fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.chars().count() >= 10 && trimmed.chars().all(|character| character == '─')
}

/// The indicator may share its row with other right-aligned status items
/// (`● high · /effort · ◎ /goal active (3m)`), so it is matched as that row's
/// final segment.
pub fn claude_indicator_shows_goal(indicator: &str) -> bool {
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
pub enum ClaudeGoalNotice {
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
pub fn claude_turn_end_goal_notice<'screen>(
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

pub fn claude_goal_notice(normalized: &str) -> Option<ClaudeGoalNotice> {
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
pub fn normalize_goal_status_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

/// Requires a marker to begin at the normalized line start or after an actual
/// footer-segment delimiter. Whitespace alone is intentionally insufficient:
/// normal transcript prose can quote the exact visible status phrase.
pub fn has_status_boundary_before(line: &str, marker_index: usize) -> bool {
    marker_index == 0
        || line[..marker_index]
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|character| matches!(character, '·' | '│' | '|' | '•' | '—' | '–' | '…'))
}

/// A completed Claude turn supersedes status chrome above its summary, not
/// the summary itself: it can still report background work after the turn.
/// This is a text-order boundary, not a goal-state or task-delivery decision.
/// Other identified providers retain their own existing status semantics.
pub fn current_status_text<'screen>(
    class: Option<&AgentClass>,
    screen_text: &'screen str,
) -> &'screen str {
    if !matches!(class, None | Some(AgentClass::Claude)) {
        return screen_text;
    }
    // The composer is separated from conversation/status rows by a horizontal
    // rule. Rows below it are configurable footer content, not live activity.
    // Requiring that separator avoids treating an approval menu's cursor as
    // the composer. Approval recognition still receives the complete screen.
    let mut previous_was_separator = false;
    let mut offset = 0;
    let mut footer_start = None;
    for line in screen_text.split_inclusive('\n') {
        if previous_was_separator && line.trim_start().starts_with('❯') {
            footer_start = Some(offset);
        }
        let trimmed = line.trim();
        previous_was_separator =
            trimmed.chars().count() >= 3 && trimmed.chars().all(|character| character == '─');
        offset += line.len();
    }
    let screen_text = &screen_text[..footer_start.unwrap_or(screen_text.len())];
    let mut start = screen_text.len();
    for line in screen_text.split_inclusive('\n').rev() {
        start -= line.len();
        if is_claude_completed_turn_status_line(line) {
            return &screen_text[start..];
        }
    }
    screen_text
}

/// Recognizes the supplied Claude summary shape, without enumerating verbs:
/// `✻ <word> for <duration> [· done <12-hour clock>] [· N ... still running]`.
/// A prose mention, an individual `● Agent ... finished` event, or an
/// incomplete/unknown suffix is not evidence that earlier waits ended.
/// An exact verbatim copy of terminal chrome is indistinguishable in plain
/// text; this predicate is not proof of provider provenance by itself.
pub fn is_claude_completed_turn_status_line(line: &str) -> bool {
    let Some(body) = line.trim().strip_prefix("✻ ") else {
        return false;
    };
    let mut segments = body.split('·');
    let Some(header) = segments.next() else {
        return false;
    };
    let mut words = header.split_whitespace();
    let Some(verb) = words.next() else {
        return false;
    };
    if !verb.bytes().all(|byte| byte.is_ascii_alphabetic()) || words.next() != Some("for") {
        return false;
    }
    let Some(first_duration) = words.next() else {
        return false;
    };
    if !is_claude_summary_duration(first_duration) || !words.all(is_claude_summary_duration) {
        return false;
    }
    segments.all(|segment| {
        let segment = segment.trim();
        is_claude_summary_done_time(segment) || is_background_task_wait_line(segment)
    })
}

pub fn is_claude_summary_duration(token: &str) -> bool {
    let Some(digits) = token.strip_suffix(['h', 'm', 's']) else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn is_claude_summary_done_time(segment: &str) -> bool {
    let mut words = segment.split_whitespace();
    if words.next() != Some("done") {
        return false;
    }
    let Some(clock) = words.next() else {
        return false;
    };
    let Some((hour, minute)) = clock.split_once(':') else {
        return false;
    };
    let Some(period) = words.next() else {
        return false;
    };
    (1..=2).contains(&hour.len())
        && hour.bytes().all(|byte| byte.is_ascii_digit())
        && hour
            .parse::<u8>()
            .is_ok_and(|hour| (1..=12).contains(&hour))
        && minute.len() == 2
        && minute.bytes().all(|byte| byte.is_ascii_digit())
        && minute.parse::<u8>().is_ok_and(|minute| minute < 60)
        && (period.eq_ignore_ascii_case("AM") || period.eq_ignore_ascii_case("PM"))
        && words.next().is_none()
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
pub fn looks_like_background_wait_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_background_wait_line)
}

/// The single-line rule behind [`looks_like_background_wait_line`], shared
/// with `activity_evidence_line` so the reported evidence line is always the
/// line the classifier itself matched.
pub fn is_background_wait_line(line: &str) -> bool {
    // Use to_ascii_lowercase() for ASCII terminal output (faster than to_lowercase() for typical case)
    let lower = line.to_ascii_lowercase();
    normalize_goal_status_line(line).starts_with("waiting for ")
        && lower.contains("background")
        && (lower.contains("agent") || lower.contains("task"))
}

/// The `WORKING_MARKER` rule as a per-line predicate, so evidence recovery
/// runs the same test the classifier ran over the whole screen. The marker
/// contains no newline, so a screen-wide `contains` match always lies inside
/// exactly one line.
pub fn is_interrupt_marker_line(line: &str) -> bool {
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
pub fn looks_like_background_task_wait_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_background_task_wait_line)
}

/// The single-line rule behind [`looks_like_background_task_wait_line`].
pub fn is_background_task_wait_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("still running")
        && lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
}

/// True if any line looks like an in-progress Claude status line: a spinner
/// glyph, an activity label ending in an ellipsis, and parenthesized metadata
/// containing an elapsed-time token (e.g. "6s", "12m"). This is the structural
/// convention observed in real Claude Code output -- a present-tense
/// whimsical verb ending in '…' plus a live elapsed-time counter, e.g.
/// `"✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)"` -- and
/// it survives whichever silly verb happens to be showing, unlike trying
/// to match exact wording. It's also distinct from the *finished*-turn
/// summary line Claude Code prints once a turn completes (e.g.
/// `"✻ Cogitated for 10s"`), which uses past tense "for Ns" with no
/// ellipsis, so it won't be mistaken for still-working.
pub fn looks_like_live_status_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_live_status_line)
}

/// The single-line rule behind [`looks_like_live_status_line`].
pub fn is_live_status_line(line: &str) -> bool {
    // Claude's spinner is terminal chrome, not an arbitrary prose ellipsis.
    // In particular, custom status footers can contain both truncated text
    // and ages such as `first seen 4m …` while the composer is idle.
    let mut characters = line.trim().chars();
    if !characters
        .next()
        .is_some_and(|character| matches!(character, '·' | '✢' | '✳' | '∗' | '✻' | '✽'))
    {
        return false;
    }
    let body = characters.as_str();
    if !body.starts_with(char::is_whitespace) {
        return false;
    }
    let Some((label, metadata)) = body.trim_start().split_once('…') else {
        return false;
    };
    let metadata = metadata.trim();
    !label.is_empty()
        && label
            .chars()
            .all(|character| character.is_alphabetic() || character == ' ')
        && metadata.starts_with('(')
        && metadata
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
pub fn looks_like_codex_live_status_line(screen_text: &str) -> bool {
    screen_text.lines().any(is_codex_live_status_line)
}

/// The single-line rule behind [`looks_like_codex_live_status_line`].
pub fn is_codex_live_status_line(line: &str) -> bool {
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
pub fn is_elapsed_time_token(token: &str) -> bool {
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
pub fn looks_like_confirmation_prompt(screen_text: &str) -> bool {
    screen_text.lines().any(is_confirmation_prompt_line)
}

/// The single-line rule behind [`looks_like_confirmation_prompt`].
pub fn is_confirmation_prompt_line(line: &str) -> bool {
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

/// Captured live from Claude Code 2.1.283 in a fresh folder with a harmless
/// project permission rule. Both choices and the safety-check heading are
/// required before declaring a blocked interstitial. No input is ever sent
/// to answer it automatically.
pub fn looks_like_claude_folder_trust_prompt(screen_text: &str) -> bool {
    screen_text.contains("Quick safety check:")
        && screen_text.contains("Yes, I trust this folder")
        && screen_text.contains("No, continue without these permissions")
}

pub fn is_claude_folder_trust_choice_line(line: &str) -> bool {
    line.contains("Yes, I trust this folder")
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
pub const SELECTION_CURSORS: &[char] = &['\u{276f}', '\u{203a}'];

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
pub fn looks_like_selection_prompt(screen_text: &str) -> bool {
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

/// How many rows above the footer hint a dialog's "Chat about this" option may
/// sit. Claude Code's question dialog is a header, a question, up to four
/// options with descriptions, then the two fixed trailing options.
pub const QUESTION_DIALOG_OPTION_WINDOW: usize = 12;

/// True if the screen ends in Claude Code's question dialog (AskUserQuestion):
/// the last non-blank row is the "Enter to select ... Esc to cancel" hint and
/// a numbered "Chat about this" option sits just above it. Both are fixed
/// dialog chrome, so together they are strong enough to outrank stale working
/// markers; anchoring to the bottom keeps a transcript that merely quotes the
/// dialog from matching.
pub fn looks_like_bottom_anchored_question_dialog(screen_text: &str) -> bool {
    let mut rows = screen_text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty());
    if !rows.next().is_some_and(is_selection_footer_line) {
        return false;
    }
    rows.take(QUESTION_DIALOG_OPTION_WINDOW).any(|line| {
        is_numbered_option_line(line) && line.to_ascii_lowercase().contains("chat about this")
    })
}

/// The per-line half of [`looks_like_selection_prompt`]: either signal, on
/// this one line. Used to recover the evidence line for a
/// `SelectionPrompt` classification.
pub fn is_selection_prompt_line(line: &str) -> bool {
    is_selection_footer_line(line)
        || (is_numbered_option_line(line) && starts_with_selection_cursor(line))
}

/// True if `line` is an interactive prompt's footer hint -- it names both a
/// confirm/select action and a cancel action.
pub fn is_selection_footer_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let names_a_confirm_action =
        lower.contains("to select") || lower.contains("to confirm") || lower.contains("to choose");
    names_a_confirm_action && lower.contains("cancel")
}

/// True if `line`'s first non-blank character is a selection cursor.
pub fn starts_with_selection_cursor(line: &str) -> bool {
    line.trim_start().starts_with(SELECTION_CURSORS)
}

/// True if `line` starts (after an optional selection cursor and leading
/// whitespace) with a small integer followed by `". "` -- e.g. "1. Source
/// only" or "  2. Write full list to file".
pub fn is_numbered_option_line(line: &str) -> bool {
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
