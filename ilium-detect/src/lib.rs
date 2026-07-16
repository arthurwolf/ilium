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
//! transcript files) belongs to the caller (`ilium-server`'s detection
//! loop, currently `ilium`'s `app.rs` during the strangler-fig
//! migration), not to this crate.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};

use ilium_core::{AgentActivity, AgentClass, AgentProvider, BuiltinAgentProvider};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

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
/// str` so the same type serves both the compile-time [`AGENT_SIGNATURES`]
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
    if screen_text.contains(WORKING_MARKER) || looks_like_live_status_line(screen_text) {
        return AgentActivity::Working;
    }

    if looks_like_background_wait_line(screen_text) {
        return AgentActivity::WaitingBackground;
    }

    if looks_like_confirmation_prompt(screen_text) || looks_like_selection_prompt(screen_text) {
        return AgentActivity::WaitingApproval;
    }

    AgentActivity::Idle
}

/// Classifies activity with the detected provider's status-line vocabulary.
///
/// Claude Code's current live status uses a whimsical verb, a Unicode ellipsis,
/// and an elapsed-time token. Codex also renders elapsed times in completed
/// transcript rows, so applying that shape to every provider turns finished
/// Codex panes back into `Working`. Provider-specific status recognition keeps
/// the shared activity contract while isolating each CLI's volatile UI text.
pub fn classify_activity_for_agent(class: &AgentClass, screen_text: &str) -> AgentActivity {
    if screen_text.contains(WORKING_MARKER)
        || (matches!(class, AgentClass::Claude) && looks_like_live_status_line(screen_text))
        || (matches!(class, AgentClass::Codex) && looks_like_codex_live_status_line(screen_text))
    {
        return AgentActivity::Working;
    }

    if looks_like_background_wait_line(screen_text) {
        return AgentActivity::WaitingBackground;
    }

    if looks_like_confirmation_prompt(screen_text) || looks_like_selection_prompt(screen_text) {
        return AgentActivity::WaitingApproval;
    }

    AgentActivity::Idle
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

/// Returns whether an agent's persistent task goal is visible on screen.
///
/// Codex represents a configured goal as a non-empty `Goal:` status row.
/// Matching its label rather than any occurrence of "goal" avoids false
/// positives from ordinary agent prose, markdown, and Codex's explicit
/// no-goal response. The server calls this only after a pane has already
/// been identified as an agent process.
pub fn has_visible_goal(screen_text: &str) -> bool {
    screen_text.lines().any(|line| {
        // A terminal may preserve the leading checkered-flag/icon cell from
        // Codex's own row, so ignore decoration but retain the anchored label.
        let trimmed = line
            .trim_start()
            .trim_start_matches(|character: char| !character.is_alphanumeric());
        let Some(goal_value) = trimmed
            .get(..5)
            .filter(|label| label.eq_ignore_ascii_case("goal:"))
            .and_then(|_| trimmed.get(5..))
        else {
            return false;
        };
        !goal_value.trim().is_empty()
    })
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
        let lower = line.to_lowercase();
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
/// [`AGENT_SIGNATURES`]), and returns the first match found.
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
/// the built-in [`AGENT_SIGNATURES`] table -- the registry-driven
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
    let mut best: Option<((usize, u32), Pid, AgentClass)> = None;
    let mut queue: VecDeque<(Pid, usize)> = VecDeque::new();
    let mut visited: HashSet<Pid> = HashSet::new();
    queue.push_back((shell_pid, 0));
    visited.insert(shell_pid);

    while let Some((pid, depth)) = queue.pop_front() {
        if let Some(((best_depth, _), _, _)) = &best {
            if depth > *best_depth {
                break;
            }
        }
        if let Some(process) = system.process(pid) {
            if let Some(class) = classify_process_name_with_extra(
                &process.name().to_string_lossy().to_lowercase(),
                extra_signatures,
            ) {
                let key = (depth, pid.as_u32());
                let is_better = match &best {
                    None => true,
                    Some((existing_key, _, _)) => key < *existing_key,
                };
                if is_better {
                    best = Some((key, pid, class));
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

    best.map(|(_, pid, class)| AgentIdentity {
        pid: pid.as_u32(),
        class,
    })
}

/// Classifies a single (already-lowercased) process name against the shared
/// first-party provider registry, generic built-ins, then `extra_signatures`.
/// That order lets configuration add coverage without silently shadowing an
/// agent whose launch/resume semantics ilium already knows.
fn classify_process_name_with_extra(
    lowercase_name: &str,
    extra_signatures: &[AgentSignature],
) -> Option<AgentClass> {
    BuiltinAgentProvider::ALL
        .into_iter()
        .find(|provider| {
            provider
                .process_name_substrings()
                .iter()
                .any(|substring| lowercase_name.contains(substring))
        })
        .map(AgentProvider::class)
        .or_else(|| {
            GENERIC_AGENT_SIGNATURES
                .iter()
                .find(|signature| lowercase_name.contains(signature.name_substring.as_ref()))
                .map(|signature| (signature.class_of)(lowercase_name))
        })
        .or_else(|| {
            extra_signatures
                .iter()
                .find(|signature| lowercase_name.contains(signature.name_substring.as_ref()))
                .map(|signature| (signature.class_of)(lowercase_name))
        })
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
    fn visible_goal_row_is_detected_without_matching_goal_prose() {
        assert!(has_visible_goal("  🏁 Goal: Finish the detection pass"));
        assert!(has_visible_goal("gOaL: keep the goal paused"));
        assert!(!has_visible_goal(
            "This thread does not currently have a goal."
        ));
        assert!(!has_visible_goal("Goal:"));
        assert!(!has_visible_goal("The project's goal: keep tests green."));
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
