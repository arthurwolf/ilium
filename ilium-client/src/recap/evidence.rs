//! Goal-aware, bounded evidence for one agent session, and bounded screen
//! evidence for shell panes.
//!
//! The reader never loads a whole transcript. It keeps a head window and a
//! tail window, drops oversize lines, and reports when the middle was skipped.
//! Each tier has its own character budget and is trimmed from the oldest end,
//! so goal records are never cut by a lower tier. Every string leaves this
//! module redacted.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use ilium_core::AgentClass;
use serde_json::Value;

use super::custody::{redact_secrets, Redacted};
use crate::transcript_context::{extract_transcript_entries, TranscriptEntryKind};

const KIB_CHARACTERS: usize = 1_024;
const MIB_BYTES: u64 = 1_024 * 1_024;

/// Bytes read from the start of a transcript (goal history, first prompts).
pub const HEAD_WINDOW_BYTES: u64 = 8 * MIB_BYTES;
/// Bytes read from the end of a transcript (current state).
pub const TAIL_WINDOW_BYTES: u64 = 32 * MIB_BYTES;
/// Lines longer than this are skipped instead of loaded.
pub const MAX_LINE_BYTES: usize = 4 * 1_024 * 1_024;

/// Tier A: goals, in characters.
pub const GOAL_BUDGET_CHARACTERS: usize = 8 * KIB_CHARACTERS;
/// Characters kept of each goal objective when the goal list is trimmed.
pub const GOAL_OBJECTIVE_CLIP_CHARACTERS: usize = 2_000;
/// Tier B: agent-written summaries, in characters.
pub const SUMMARY_BUDGET_CHARACTERS: usize = 4 * KIB_CHARACTERS;
/// Tier C: genuine user prompts.
pub const PROMPT_COUNT: usize = 12;
pub const PROMPT_CLIP_CHARACTERS: usize = 400;
pub const PROMPT_BUDGET_CHARACTERS: usize = 5 * KIB_CHARACTERS;
/// Tier D: agent answers.
pub const ANSWER_COUNT: usize = 6;
pub const ANSWER_CLIP_CHARACTERS: usize = 1_536;
pub const ANSWER_BUDGET_CHARACTERS: usize = 9 * KIB_CHARACTERS;
/// Tier E: screen text, for agents and shell panes.
pub const SCREEN_BUDGET_CHARACTERS: usize = 4 * KIB_CHARACTERS;
pub const SCREEN_LINE_LIMIT: usize = 40;

/// Recorded goal state. Mirrors the phases Claude and Codex record; Claude
/// records only active and reached goals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecapGoalState {
    Active,
    Paused,
    Blocked,
    Done,
    /// The session recorded this goal but no final state, or a later goal
    /// replaced it. Never presented as finished or as running.
    Unknown,
}

impl RecapGoalState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }

    fn from_codex_status(status: &str) -> Self {
        match status {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "blocked" => Self::Blocked,
            "complete" => Self::Done,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalRecord {
    pub objective: String,
    pub state: RecapGoalState,
}

/// Bounded, redacted evidence for one agent session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDigest {
    /// One entry per objective, oldest first, newest last.
    pub goals: Vec<GoalRecord>,
    /// Goals removed from the middle of the list by the tier A budget.
    pub goals_omitted: usize,
    pub summaries: Vec<String>,
    pub prompts: Vec<String>,
    pub answers: Vec<String>,
    pub screen: Option<String>,
    /// The middle of the transcript was skipped by the window reader.
    pub window_truncated: bool,
    pub oversize_lines_skipped: usize,
    pub redactions: usize,
}

impl AgentDigest {
    /// True when the digest contains no text the model could use.
    pub fn is_empty(&self) -> bool {
        self.goals.is_empty()
            && self.summaries.is_empty()
            && self.prompts.is_empty()
            && self.answers.is_empty()
            && self.screen.is_none()
    }
}

/// Bounded bytes read from one transcript, parsed into JSON-line strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptWindow {
    pub lines: Vec<String>,
    pub truncated: bool,
    pub oversize_lines_skipped: usize,
}

/// Reads at most `head_bytes` from the start and `tail_bytes` from the end.
/// A file no larger than both windows is read whole. Partial lines at a
/// window edge are dropped, never parsed.
pub fn read_transcript_window(
    path: &Path,
    head_bytes: u64,
    tail_bytes: u64,
    max_line_bytes: usize,
) -> std::io::Result<TranscriptWindow> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut window = TranscriptWindow {
        lines: Vec::new(),
        truncated: false,
        oversize_lines_skipped: 0,
    };
    if length <= head_bytes.saturating_add(tail_bytes) {
        let mut reader = BufReader::new(file);
        read_lines(&mut reader, max_line_bytes, &mut window, false)?;
        return Ok(window);
    }

    window.truncated = true;
    let head_reader = file.try_clone()?;
    let mut head = BufReader::new(std::io::Read::take(head_reader, head_bytes));
    read_lines(&mut head, max_line_bytes, &mut window, true)?;

    file.seek(SeekFrom::Start(length - tail_bytes))?;
    let mut tail = BufReader::new(file);
    // The first line of the tail window starts mid-record: discard it.
    let mut partial = Vec::new();
    tail.read_until(b'\n', &mut partial)?;
    read_lines(&mut tail, max_line_bytes, &mut window, false)?;
    Ok(window)
}

fn read_lines(
    reader: &mut impl BufRead,
    max_line_bytes: usize,
    window: &mut TranscriptWindow,
    drop_unterminated_last: bool,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        let outcome = read_bounded_line(reader, &mut buffer, max_line_bytes)?;
        match outcome {
            LineOutcome::End => return Ok(()),
            LineOutcome::Oversize => window.oversize_lines_skipped += 1,
            LineOutcome::Line => {
                if drop_unterminated_last && buffer.last() != Some(&b'\n') {
                    return Ok(());
                }
                let text = String::from_utf8_lossy(&buffer);
                let text = text.trim_end_matches(['\r', '\n']);
                if !text.is_empty() {
                    window.lines.push(text.to_string());
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LineOutcome {
    Line,
    Oversize,
    End,
}

/// Reads one `\n`-terminated line into `buffer`. A longer line is consumed to
/// its end and reported as `Oversize`, so memory stays bounded.
fn read_bounded_line(
    reader: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    max_line_bytes: usize,
) -> std::io::Result<LineOutcome> {
    buffer.clear();
    let mut oversize = false;
    let mut consumed_any = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(match (consumed_any, oversize) {
                (false, _) => LineOutcome::End,
                (true, true) => LineOutcome::Oversize,
                (true, false) => LineOutcome::Line,
            });
        }
        consumed_any = true;
        let (take, finished) = match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (available.len(), false),
        };
        if !oversize {
            if buffer.len() + take > max_line_bytes {
                oversize = true;
                buffer.clear();
            } else {
                buffer.extend_from_slice(&available[..take]);
            }
        }
        reader.consume(take);
        if finished {
            return Ok(if oversize {
                LineOutcome::Oversize
            } else {
                LineOutcome::Line
            });
        }
    }
}

/// Builds the bounded digest from parsed transcript lines.
pub fn build_agent_digest(class: &AgentClass, window: &TranscriptWindow) -> AgentDigest {
    let mut digest = AgentDigest {
        window_truncated: window.truncated,
        oversize_lines_skipped: window.oversize_lines_skipped,
        ..AgentDigest::default()
    };
    let mut goals = Vec::new();
    let mut summaries = VecDeque::new();
    for line in &window.lines {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match class {
            AgentClass::Claude => {
                claude_goal_record(&record, &mut goals);
                claude_summary_record(&record, &mut summaries);
            }
            AgentClass::Codex => {
                codex_goal_record(&record, &mut goals);
                codex_summary_record(&record, &mut summaries);
            }
            _ => {}
        }
    }
    let (goal_records, goals_omitted) = trim_goals(goals);
    digest.goals = goal_records;
    digest.goals_omitted = goals_omitted;
    digest.summaries = trim_newest_within(summaries.into(), SUMMARY_BUDGET_CHARACTERS);

    let entries = extract_transcript_entries(class, window.lines.clone().into_iter());
    let users: Vec<String> = entries
        .iter()
        .filter(|entry| entry.kind == TranscriptEntryKind::User)
        .map(|entry| clip_middle(&entry.content, PROMPT_CLIP_CHARACTERS))
        .collect();
    let answers: Vec<String> = entries
        .iter()
        .filter(|entry| entry.kind == TranscriptEntryKind::Assistant)
        .map(|entry| clip_middle(&entry.content, ANSWER_CLIP_CHARACTERS))
        .collect();
    digest.prompts = trim_newest_within(last_n(users, PROMPT_COUNT), PROMPT_BUDGET_CHARACTERS);
    digest.answers = trim_newest_within(last_n(answers, ANSWER_COUNT), ANSWER_BUDGET_CHARACTERS);
    redact_digest(digest)
}

/// Builds screen-only evidence for a shell pane or an agent without transcript.
pub fn build_screen_digest(screen_text: &str) -> AgentDigest {
    let tail = screen_tail(screen_text, SCREEN_LINE_LIMIT, SCREEN_BUDGET_CHARACTERS);
    redact_digest(AgentDigest {
        screen: (!tail.is_empty()).then_some(tail),
        ..AgentDigest::default()
    })
}

/// Applies redaction to every string and records how many values were hidden.
fn redact_digest(mut digest: AgentDigest) -> AgentDigest {
    let mut redactions = 0;
    let mut scrub = |text: &mut String| {
        let Redacted { text: clean, count } = redact_secrets(text);
        redactions += count;
        *text = clean;
    };
    for goal in &mut digest.goals {
        scrub(&mut goal.objective);
    }
    for text in digest
        .summaries
        .iter_mut()
        .chain(digest.prompts.iter_mut())
        .chain(digest.answers.iter_mut())
    {
        scrub(text);
    }
    if let Some(screen) = digest.screen.as_mut() {
        scrub(screen);
    }
    digest.redactions = redactions;
    digest
}

/// Keeps the newest `count` items.
fn last_n(items: Vec<String>, count: usize) -> Vec<String> {
    let skip = items.len().saturating_sub(count);
    items.into_iter().skip(skip).collect()
}

/// Keeps the newest items whose total length fits `budget` characters.
/// Items are given oldest first; the result is oldest first too.
fn trim_newest_within(items: Vec<String>, budget: usize) -> Vec<String> {
    let mut kept = Vec::new();
    let mut used = 0;
    for item in items.into_iter().rev() {
        let length = item.chars().count();
        if used + length > budget {
            break;
        }
        used += length;
        kept.push(item);
    }
    kept.reverse();
    kept
}

/// Head-and-tail clip. The marker names the omitted count, so the model sees
/// that the text was cut.
pub fn clip_middle(value: &str, limit: usize) -> String {
    let value = value.trim();
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= limit {
        return value.to_string();
    }
    let edge = limit / 2;
    let head: String = characters[..edge].iter().collect();
    let tail: String = characters[characters.len() - edge..].iter().collect();
    let omitted = characters.len() - edge * 2;
    format!("{head}\n[... {omitted} characters omitted ...]\n{tail}")
}

/// The last `line_limit` lines of `screen_text`, trimmed to `budget` characters
/// from the newest end.
pub fn screen_tail(screen_text: &str, line_limit: usize, budget: usize) -> String {
    let lines: Vec<&str> = screen_text.lines().collect();
    let skip = lines.len().saturating_sub(line_limit);
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0;
    for line in lines[skip..].iter().rev() {
        let length = line.chars().count() + 1;
        if used + length > budget {
            break;
        }
        used += length;
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n").trim_end().to_string()
}

fn upsert_goal(goals: &mut Vec<GoalRecord>, objective: &str, state: RecapGoalState) {
    let objective = objective.trim();
    if objective.is_empty() {
        return;
    }
    let is_new = !goals.iter().any(|goal| goal.objective == objective);
    if is_new {
        // A new goal replaces whatever was running; its final state is
        // unknown unless it was already reached.
        for goal in goals.iter_mut() {
            if goal.state == RecapGoalState::Active {
                goal.state = RecapGoalState::Unknown;
            }
        }
    }
    goals.retain(|goal| goal.objective != objective);
    goals.push(GoalRecord {
        objective: objective.to_string(),
        state,
    });
}

/// Claude: `/goal` invocations carry the objective in `<command-args>`, and
/// `goal_status` attachments carry the condition plus `met` (goal reached).
fn claude_goal_record(record: &Value, goals: &mut Vec<GoalRecord>) {
    match record.get("type").and_then(Value::as_str) {
        Some("user") => {
            let Some(content) = record
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
            else {
                return;
            };
            if let Some(objective) = goal_command_arguments(content) {
                upsert_goal(goals, &objective, RecapGoalState::Active);
            }
        }
        Some("attachment") => {
            let Some(attachment) = record.get("attachment") else {
                return;
            };
            if attachment.get("type").and_then(Value::as_str) != Some("goal_status") {
                return;
            }
            let Some(condition) = attachment.get("condition").and_then(Value::as_str) else {
                return;
            };
            let state = if attachment.get("met").and_then(Value::as_bool) == Some(true) {
                RecapGoalState::Done
            } else {
                RecapGoalState::Active
            };
            upsert_goal(goals, condition, state);
        }
        _ => {}
    }
}

/// Extracts the objective from a `/goal` command record, or `None` when the
/// text is not one.
pub fn goal_command_arguments(content: &str) -> Option<String> {
    if !content.contains("<command-name>/goal</command-name>") {
        return None;
    }
    let start_tag = "<command-args>";
    let end_tag = "</command-args>";
    let start = content.find(start_tag)? + start_tag.len();
    let end = start + content[start..].find(end_tag)?;
    let objective = content[start..end].trim();
    (!objective.is_empty()).then(|| objective.to_string())
}

/// Codex: every `thread_goal_updated` event carries the goal's objective and
/// current status, so the newest event for an objective wins.
fn codex_goal_record(record: &Value, goals: &mut Vec<GoalRecord>) {
    if record.get("type").and_then(Value::as_str) != Some("event_msg") {
        return;
    }
    let Some(payload) = record.get("payload") else {
        return;
    };
    if payload.get("type").and_then(Value::as_str) != Some("thread_goal_updated") {
        return;
    }
    let Some(goal) = payload.get("goal") else {
        return;
    };
    let Some(objective) = goal.get("objective").and_then(Value::as_str) else {
        return;
    };
    let state = goal
        .get("status")
        .and_then(Value::as_str)
        .map_or(RecapGoalState::Unknown, RecapGoalState::from_codex_status);
    upsert_goal(goals, objective, state);
}

/// Tier A budget. Keeps the first objective and the newest, and writes an
/// omitted count for everything between them.
fn trim_goals(goals: Vec<GoalRecord>) -> (Vec<GoalRecord>, usize) {
    let original_total: usize = goals
        .iter()
        .map(|goal| goal.objective.chars().count())
        .sum();
    let goals: Vec<_> = goals
        .into_iter()
        .map(|goal| GoalRecord {
            objective: clip_middle(&goal.objective, GOAL_OBJECTIVE_CLIP_CHARACTERS),
            state: goal.state,
        })
        .collect();
    // Clipping markers are evidence too; account for them in the aggregate
    // bound without clipping a second time and losing the true omitted count.
    let clipped_total: usize = goals
        .iter()
        .map(|goal| goal.objective.chars().count())
        .sum();
    if original_total <= GOAL_BUDGET_CHARACTERS && clipped_total <= GOAL_BUDGET_CHARACTERS {
        return (goals, 0);
    }
    let count = goals.len();
    let mut kept = Vec::new();
    if let Some(first) = goals.first() {
        kept.push(first.clone());
    }
    if count > 1 {
        if let Some(newest) = goals.last() {
            kept.push(newest.clone());
        }
    }
    let omitted = count.saturating_sub(kept.len());
    (kept, omitted)
}

/// Summaries the agent wrote itself: compaction summaries, away summaries,
/// Codex compacted history, and the last completed-turn message.
fn claude_summary_record(record: &Value, summaries: &mut VecDeque<String>) {
    let summary = match record.get("type").and_then(Value::as_str) {
        Some("user") if record.get("isCompactSummary").and_then(Value::as_bool) == Some(true) => {
            crate::transcript_context::textual_value(
                record.get("message").and_then(|m| m.get("content")),
            )
        }
        Some("system") if record.get("subtype").and_then(Value::as_str) == Some("away_summary") => {
            crate::transcript_context::textual_value(record.get("content"))
        }
        _ => None,
    };
    push_summary(summaries, summary);
}

fn codex_summary_record(record: &Value, summaries: &mut VecDeque<String>) {
    let Some(payload) = record.get("payload") else {
        return;
    };
    let summary = match record.get("type").and_then(Value::as_str) {
        Some("compacted") => {
            crate::transcript_context::textual_value(payload.get("replacement_history"))
        }
        Some("event_msg")
            if payload.get("type").and_then(Value::as_str) == Some("task_complete") =>
        {
            crate::transcript_context::textual_value(payload.get("last_agent_message"))
        }
        _ => None,
    };
    push_summary(summaries, summary);
}

fn push_summary(summaries: &mut VecDeque<String>, summary: Option<String>) {
    let Some(summary) = summary else {
        return;
    };
    summaries.push_back(clip_middle(&summary, SUMMARY_BUDGET_CHARACTERS / 2));
    while summaries.len() > 8 {
        summaries.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_user(content: &str) -> String {
        serde_json::json!({"type": "user", "message": {"role": "user", "content": content}})
            .to_string()
    }

    fn claude_goal_status(condition: &str, met: bool) -> String {
        serde_json::json!({"type": "attachment", "attachment": {"type": "goal_status", "condition": condition, "met": met}}).to_string()
    }

    fn codex_goal(objective: &str, status: &str) -> String {
        serde_json::json!({"type": "event_msg", "payload": {"type": "thread_goal_updated", "goal": {"objective": objective, "status": status}}}).to_string()
    }

    fn window(lines: Vec<String>) -> TranscriptWindow {
        TranscriptWindow {
            lines,
            truncated: false,
            oversize_lines_skipped: 0,
        }
    }

    #[test]
    fn parses_goal_command_arguments_only_from_goal_records() {
        let record =
            "<command-name>/goal</command-name><command-args>ship the recap</command-args>";
        assert_eq!(
            goal_command_arguments(record).as_deref(),
            Some("ship the recap")
        );
        assert_eq!(
            goal_command_arguments("<command-args>x</command-args>"),
            None
        );
        assert_eq!(
            goal_command_arguments(
                "<command-name>/goal</command-name><command-args>  </command-args>"
            ),
            None
        );
    }

    #[test]
    fn claude_goal_state_comes_from_met_flag_and_newer_goal_supersedes_active() {
        let digest = build_agent_digest(
            &AgentClass::Claude,
            &window(vec![
                claude_user(
                    "<command-name>/goal</command-name><command-args>first objective</command-args>",
                ),
                claude_goal_status("first objective", false),
                claude_user(
                    "<command-name>/goal</command-name><command-args>second objective</command-args>",
                ),
                claude_goal_status("second objective", true),
            ]),
        );
        assert_eq!(digest.goals.len(), 2);
        assert_eq!(digest.goals[0].objective, "first objective");
        assert_eq!(digest.goals[0].state, RecapGoalState::Unknown);
        assert_eq!(digest.goals[1].objective, "second objective");
        assert_eq!(digest.goals[1].state, RecapGoalState::Done);
    }

    #[test]
    fn codex_goal_status_maps_recorded_phases() {
        let digest = build_agent_digest(
            &AgentClass::Codex,
            &window(vec![
                codex_goal("objective a", "active"),
                codex_goal("objective a", "paused"),
                codex_goal("objective b", "complete"),
            ]),
        );
        assert_eq!(digest.goals.len(), 2);
        // Objective a was paused, not active, so the newer goal keeps it Paused.
        assert_eq!(digest.goals[0].state, RecapGoalState::Paused);
        assert_eq!(digest.goals[1].state, RecapGoalState::Done);
    }

    #[test]
    fn oversize_goal_list_keeps_first_and_newest_and_counts_omitted() {
        let mut lines = Vec::new();
        for index in 0..40 {
            let objective = format!("objective {index} {}", "x".repeat(400));
            lines.push(codex_goal(&objective, "active"));
        }
        let digest = build_agent_digest(&AgentClass::Codex, &window(lines));
        assert_eq!(digest.goals.len(), 2);
        assert_eq!(digest.goals_omitted, 38);
        assert!(digest.goals[0].objective.starts_with("objective 0 "));
        assert!(digest.goals[1].objective.starts_with("objective 39 "));
        assert!(digest
            .goals
            .iter()
            .all(|goal| goal.objective.chars().count() <= GOAL_OBJECTIVE_CLIP_CHARACTERS + 64));
    }

    #[test]
    fn oversized_individual_goal_is_clipped_even_below_aggregate_budget() {
        let objective = format!(
            "first {} last",
            "x".repeat(GOAL_OBJECTIVE_CLIP_CHARACTERS + 100)
        );
        let digest = build_agent_digest(
            &AgentClass::Codex,
            &window(vec![codex_goal(&objective, "active")]),
        );
        assert_eq!(digest.goals_omitted, 0);
        assert_eq!(digest.goals.len(), 1);
        let kept = &digest.goals[0].objective;
        assert!(kept.starts_with("first ") && kept.ends_with(" last"));
        assert!(kept.contains("characters omitted"));
        assert!(kept.chars().count() <= GOAL_OBJECTIVE_CLIP_CHARACTERS + 64);
    }

    #[test]
    fn two_oversized_goals_keep_first_and_newest() {
        let first = format!("first {} first-tail", "x".repeat(GOAL_BUDGET_CHARACTERS));
        let newest = format!("newest {} newest-tail", "y".repeat(GOAL_BUDGET_CHARACTERS));
        let digest = build_agent_digest(
            &AgentClass::Codex,
            &window(vec![
                codex_goal(&first, "paused"),
                codex_goal(&newest, "active"),
            ]),
        );
        assert_eq!(digest.goals.len(), 2);
        assert_eq!(digest.goals_omitted, 0);
        assert!(digest.goals[0].objective.starts_with("first "));
        assert!(digest.goals[1].objective.starts_with("newest "));
        assert!(digest
            .goals
            .iter()
            .all(|goal| goal.objective.contains("characters omitted")));
    }

    #[test]
    fn clipping_markers_count_towards_the_aggregate_goal_budget() {
        let size = GOAL_OBJECTIVE_CLIP_CHARACTERS + 1;
        let objectives = [
            "a".repeat(size),
            "b".repeat(size),
            "c".repeat(size),
            "d".repeat(size),
            "z".repeat(GOAL_BUDGET_CHARACTERS - 4 * size - 1),
        ];
        let digest = build_agent_digest(
            &AgentClass::Codex,
            &window(
                objectives
                    .iter()
                    .map(|objective| codex_goal(objective, "active"))
                    .collect(),
            ),
        );
        assert_eq!(digest.goals.len(), 2);
        assert_eq!(digest.goals_omitted, 3);
        assert!(digest.goals[0].objective.starts_with('a'));
        assert!(digest.goals[1].objective.starts_with('z'));
        assert!(
            digest
                .goals
                .iter()
                .map(|goal| goal.objective.chars().count())
                .sum::<usize>()
                <= GOAL_BUDGET_CHARACTERS
        );
    }

    #[test]
    fn injected_notifications_are_not_prompts() {
        let digest = build_agent_digest(
            &AgentClass::Claude,
            &window(vec![
                claude_user("<system-reminder>internal note</system-reminder>"),
                claude_user("real question about the parser"),
            ]),
        );
        assert_eq!(
            digest.prompts,
            vec!["real question about the parser".to_string()]
        );
    }

    #[test]
    fn prompts_are_bounded_and_secrets_are_redacted_before_leaving() {
        // The secret sits in the kept head; a secret in the clipped middle is never read.
        let long = format!(
            "api_key=abcd1234efgh5678 {} {}",
            "p".repeat(900),
            "q".repeat(900)
        );
        let digest = build_agent_digest(&AgentClass::Claude, &window(vec![claude_user(&long)]));
        assert_eq!(digest.prompts.len(), 1);
        assert!(digest.prompts[0].chars().count() <= PROMPT_CLIP_CHARACTERS + 64);
        assert!(!digest.prompts[0].contains("abcd1234efgh5678"));
        assert!(digest.redactions >= 1);
    }

    #[test]
    fn screen_tail_keeps_newest_lines_within_budget() {
        let screen = (0..100)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = screen_tail(&screen, 5, SCREEN_BUDGET_CHARACTERS);
        assert_eq!(tail.lines().count(), 5);
        assert!(tail.ends_with("line 99"));
        assert!(!tail.contains("line 94"));
    }

    #[test]
    fn screen_digest_redacts_and_reports_empty_screen_as_absent() {
        let digest = build_screen_digest("export GITHUB_TOKEN=supersecretvalue123\n$ ls");
        assert!(digest.screen.as_deref().unwrap().contains("[redacted]"));
        assert!(!digest
            .screen
            .as_deref()
            .unwrap()
            .contains("supersecretvalue123"));
        assert!(build_screen_digest("   \n  ").is_empty());
    }

    #[test]
    fn window_reader_reads_whole_small_file_and_skips_oversize_lines() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("t.jsonl");
        let oversize = "y".repeat(300);
        std::fs::write(&path, format!("first\n{oversize}\nthird\n")).expect("write");
        let window = read_transcript_window(&path, 1_000_000, 1_000_000, 100).expect("read");
        assert_eq!(window.lines, vec!["first".to_string(), "third".to_string()]);
        assert_eq!(window.oversize_lines_skipped, 1);
        assert!(!window.truncated);
    }

    #[test]
    fn window_reader_drops_partial_edge_lines_when_middle_is_skipped() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("t.jsonl");
        let mut contents = String::new();
        for index in 0..200 {
            contents.push_str(&format!("line-{index:04}-padding-padding\n"));
        }
        std::fs::write(&path, &contents).expect("write");
        let window = read_transcript_window(&path, 200, 200, 1_000).expect("read");
        assert!(window.truncated);
        assert!(window
            .lines
            .iter()
            .all(|line| line.starts_with("line-") && line.ends_with("padding-padding")));
        assert!(window
            .lines
            .iter()
            .any(|line| line.starts_with("line-0000")));
        assert!(window
            .lines
            .iter()
            .any(|line| line.starts_with("line-0199")));
        assert!(!window
            .lines
            .iter()
            .any(|line| line.starts_with("line-0100")));
    }
}
