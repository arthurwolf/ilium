//! Provider goal evidence, independent of transcript I/O and presentation.

use serde_json::Value;

/// Maximum retained objective size in bytes; larger objectives are unavailable,
/// never silently truncated into a misleading partial goal.
const MAX_OBJECTIVE_BYTES: usize = 1024 * 1024;

/// A provider-reported phase, separate from the agent's turn activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalPhase {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Reached,
}

/// A successful provider lookup. An unavailable or invalid lookup is `None`.
#[derive(Clone, PartialEq, Eq)]
pub enum GoalLookup {
    Absent,
    Present { phase: GoalPhase, objective: String },
}

/// The caller changes this token whenever a pane's provider, process,
/// transcript binding or project changes, including a later ABA reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoalGeneration(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Current,
    Stale,
}

/// A screen-only phase does not establish objective text or absence.
#[derive(Clone, PartialEq, Eq)]
pub struct GoalObservation {
    pub phase: Option<GoalPhase>,
    pub objective: Option<String>,
    pub freshness: Freshness,
    pub absent: bool,
}

/// Reconciles one current identity. Failed reads may retain same-generation
/// text only when clearly marked stale; a successful no-row result wins.
pub struct GoalReconciler {
    generation: GoalGeneration,
    observation: GoalObservation,
}

impl GoalReconciler {
    pub fn new(generation: GoalGeneration) -> Self {
        Self {
            generation,
            observation: GoalObservation {
                phase: None,
                objective: None,
                freshness: Freshness::Current,
                absent: false,
            },
        }
    }

    pub fn observation(&self) -> &GoalObservation {
        &self.observation
    }

    /// Discard text when provider-home ownership is lost or changes.
    pub fn invalidate_provider_identity(&mut self, screen: Option<GoalPhase>) {
        self.observation = GoalObservation {
            phase: screen,
            objective: None,
            freshness: Freshness::Stale,
            absent: false,
        };
    }

    /// Screen evidence only replaces the phase while the provider read is
    /// unavailable; a successful provider response remains authoritative.
    pub fn update_screen(&mut self, screen: Option<GoalPhase>) -> bool {
        if self.observation.freshness != Freshness::Stale || self.observation.phase == screen {
            return false;
        }
        self.observation.phase = screen;
        true
    }

    /// Reject a completion from any earlier identity, even if its thread ID
    /// has since been reused. `None` means read failure, never cleared.
    pub fn apply_codex(
        &mut self,
        generation: GoalGeneration,
        lookup: Option<GoalLookup>,
        screen: Option<GoalPhase>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        self.observation = match lookup {
            Some(GoalLookup::Present { phase, objective }) => GoalObservation {
                phase: Some(phase),
                objective: Some(objective),
                freshness: Freshness::Current,
                absent: false,
            },
            Some(GoalLookup::Absent) => GoalObservation {
                phase: None,
                objective: None,
                freshness: Freshness::Current,
                absent: true,
            },
            None => GoalObservation {
                phase: screen,
                objective: self.observation.objective.take(),
                freshness: Freshness::Stale,
                absent: false,
            },
        };
        true
    }
}

/// Interpret the result of `thread/goal/get` only after the caller has
/// matched the JSON-RPC request and verified its thread binding.
pub fn codex_goal_response(thread_id: &str, result: &Value) -> Option<GoalLookup> {
    if thread_id.is_empty() {
        return None;
    }
    let goal = result.get("goal")?;
    if goal.is_null() {
        return Some(GoalLookup::Absent);
    }
    if goal.get("threadId")?.as_str()? != thread_id {
        return None;
    }
    let phase = match goal.get("status")?.as_str()? {
        "active" => GoalPhase::Active,
        "paused" => GoalPhase::Paused,
        "blocked" => GoalPhase::Blocked,
        "usageLimited" => GoalPhase::UsageLimited,
        "budgetLimited" => GoalPhase::BudgetLimited,
        "complete" => GoalPhase::Reached,
        _ => return None,
    };
    let objective = goal.get("objective")?.as_str()?;
    if objective.len() > MAX_OBJECTIVE_BYTES {
        return None;
    }
    Some(GoalLookup::Present {
        phase,
        objective: objective.to_owned(),
    })
}

/// The observed SQLx migration set for the installed Codex goal database.
/// An unknown, incomplete, failed or modified migration must fail closed;
/// this check does not replace validation of the table and its columns.
pub fn codex_goal_migrations_supported(rows: &[(i64, bool, &str)]) -> bool {
    const EXPECTED: [(i64, &str); 2] = [
        (1, "8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713"),
        (2, "F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584"),
    ];
    rows.len() == EXPECTED.len()
        && EXPECTED.iter().all(|(version, checksum)| {
            rows.iter()
                .filter(|(actual, _, _)| actual == version)
                .count()
                == 1
                && rows.iter().any(|(actual, success, actual_checksum)| {
                    actual == version && *success && actual_checksum.eq_ignore_ascii_case(checksum)
                })
        })
}

/// Decode one bounded database transaction after the caller verifies its
/// sandbox and process-to-session binding. Invalid or changed schema is an
/// unavailable lookup, never evidence that the goal is absent.
pub fn codex_database_snapshot(thread_id: &str, snapshot: &Value) -> Option<GoalLookup> {
    if thread_id.is_empty() || snapshot.get("user_version")?.as_i64()? != 0 {
        return None;
    }
    let migrations = snapshot.get("migrations")?.as_array()?;
    if migrations.len() != 2 {
        return None;
    }
    let mut parsed = Vec::with_capacity(2);
    for migration in migrations {
        let fields = migration.as_array()?;
        if fields.len() != 3 {
            return None;
        }
        let success = match fields[1].as_i64()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        parsed.push((fields[0].as_i64()?, success, fields[2].as_str()?));
    }
    if !codex_goal_migrations_supported(&parsed) {
        return None;
    }
    let expected = [
        "thread_id",
        "goal_id",
        "objective",
        "status",
        "token_budget",
        "tokens_used",
        "time_used_seconds",
        "created_at_ms",
        "updated_at_ms",
    ];
    let columns = snapshot.get("columns")?.as_array()?;
    if columns.len() != expected.len() {
        return None;
    }
    for (index, (column, name)) in columns.iter().zip(expected).enumerate() {
        let fields = column.as_array()?;
        if fields.len() != 6 || fields[0].as_u64()? != index as u64 || fields[1].as_str()? != name {
            return None;
        }
        let kind = if index < 4 { "TEXT" } else { "INTEGER" };
        if !fields[2].as_str()?.eq_ignore_ascii_case(kind)
            || fields[3].as_i64()? != i64::from(index != 4)
            || fields[5].as_i64()? != i64::from(index == 0)
        {
            return None;
        }
    }
    let rows = snapshot.get("rows")?.as_array()?;
    if rows.is_empty() {
        return Some(GoalLookup::Absent);
    }
    if rows.len() != 1 {
        return None;
    }
    let row = rows[0].as_array()?;
    if row.len() != 3 {
        return None;
    }
    let objective = row[2].as_str()?;
    if objective.len() > MAX_OBJECTIVE_BYTES {
        return None;
    }
    codex_database_goal_row(
        thread_id,
        row[0].as_str()?,
        row[1].as_str()?,
        objective.to_owned(),
    )
}

/// Interpret a row from the observed Codex goal database schema after the
/// caller has verified the database, thread binding, and read transaction.
/// Storage uses snake_case status names, unlike app-server's JSON response.
pub fn codex_database_goal_row(
    thread_id: &str,
    row_thread_id: &str,
    status: &str,
    objective: String,
) -> Option<GoalLookup> {
    if thread_id.is_empty() || thread_id != row_thread_id || objective.len() > MAX_OBJECTIVE_BYTES {
        return None;
    }
    let phase = match status {
        "active" => GoalPhase::Active,
        "paused" => GoalPhase::Paused,
        "blocked" => GoalPhase::Blocked,
        "usage_limited" => GoalPhase::UsageLimited,
        "budget_limited" => GoalPhase::BudgetLimited,
        "complete" => GoalPhase::Reached,
        _ => return None,
    };
    Some(GoalLookup::Present { phase, objective })
}

#[derive(Clone, Default, PartialEq, Eq)]
pub enum ClaudeGoalState {
    #[default]
    Unknown,
    Cleared,
    Objective(String),
}

#[derive(Clone)]
enum Pending {
    Set(String),
    Clear,
}

/// Evidence from ordered complete records in one verified main transcript.
/// Lifecycle still requires independent live screen evidence.
#[derive(Clone, Default)]
pub struct ClaudeGoalEvidence {
    state: ClaudeGoalState,
    pending: Option<Pending>,
}

impl ClaudeGoalEvidence {
    pub fn state(&self) -> &ClaudeGoalState {
        &self.state
    }

    pub fn objective(&self) -> Option<&str> {
        match &self.state {
            ClaudeGoalState::Objective(text) => Some(text),
            ClaudeGoalState::Unknown | ClaudeGoalState::Cleared => None,
        }
    }

    /// A caller must buffer partial JSONL and reset this reducer on file or
    /// identity replacement. Ambiguous ordering invalidates pending evidence.
    pub fn observe_line(&mut self, line: &str) {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            self.pending = None;
            self.state = ClaudeGoalState::Unknown;
            return;
        };
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || record.get("agentId").and_then(Value::as_str).is_some()
        {
            return;
        }
        if record.get("type").and_then(Value::as_str) != Some("user") {
            self.pending = None;
            return;
        }
        let Some(text) = record.pointer("/message/content").and_then(Value::as_str) else {
            self.pending = None;
            return;
        };
        if let Some(rest) = text
            .trim_start()
            .strip_prefix("<command-name>/goal</command-name>")
        {
            let args = rest
                .split_once("<command-args>")
                .and_then(|(_, value)| value.split_once("</command-args>"))
                .map(|(value, _)| value);
            self.pending = match args {
                Some(value) if value.trim() == "clear" => Some(Pending::Clear),
                Some(value)
                    if matches!(value.trim(), "" | "pause" | "resume" | "status" | "stop") =>
                {
                    None
                }
                None => None,
                Some(objective) => Some(Pending::Set(objective.to_owned())),
            };
            if self.pending.is_some() {
                self.state = ClaudeGoalState::Unknown;
            }
            return;
        }
        let pending = self.pending.take();
        let Some(output) = text
            .trim()
            .strip_prefix("<local-command-stdout>")
            .and_then(|text| text.strip_suffix("</local-command-stdout>"))
        else {
            return;
        };
        match pending {
            Some(Pending::Set(objective))
                if output.strip_prefix("Goal set: ") == Some(objective.as_str()) =>
            {
                self.state = ClaudeGoalState::Objective(objective);
            }
            Some(Pending::Clear) if matches!(output.trim(), "Goal cleared" | "Goal cleared.") => {
                self.state = ClaudeGoalState::Cleared;
            }
            _ => {}
        }
    }
}

/// Incremental complete-line framing for a verified main transcript. The
/// caller owns the file offset and recreates this on truncation or rotation.
/// Oversized records invalidate evidence and are skipped without retaining an
/// unbounded frame. Later complete records can establish a new goal.
#[derive(Default)]
pub struct ClaudeGoalTranscript {
    evidence: ClaudeGoalEvidence,
    partial: Vec<u8>,
    discarding_record: bool,
}

impl ClaudeGoalTranscript {
    pub fn evidence(&self) -> &ClaudeGoalEvidence {
        &self.evidence
    }

    /// A trailing partial record may contain an unobserved goal change.
    pub fn has_partial_record(&self) -> bool {
        !self.partial.is_empty() || self.discarding_record
    }

    /// Retained allocation, used by the caller's process-wide admission.
    pub fn retained_bytes(&self) -> usize {
        let objective = match &self.evidence.state {
            ClaudeGoalState::Objective(text) => text.capacity(),
            ClaudeGoalState::Unknown | ClaudeGoalState::Cleared => 0,
        };
        let pending = match &self.evidence.pending {
            Some(Pending::Set(text)) => text.capacity(),
            Some(Pending::Clear) | None => 0,
        };
        self.partial
            .capacity()
            .saturating_add(objective)
            .saturating_add(pending)
    }

    /// Feed a bounded chunk, retaining at most `record_limit` bytes for an
    /// unfinished record. An oversized frame invalidates cached text and is
    /// skipped through its newline, allowing later evidence to be processed.
    pub fn push(&mut self, bytes: &[u8], record_limit: usize) -> Result<(), TranscriptFrameError> {
        for piece in bytes.split_inclusive(|byte| *byte == b'\n') {
            if self.discarding_record {
                if piece.last() == Some(&b'\n') {
                    self.discarding_record = false;
                }
                continue;
            }
            if self.partial.len().saturating_add(piece.len()) > record_limit {
                self.partial.clear();
                self.evidence = ClaudeGoalEvidence::default();
                self.discarding_record = piece.last() != Some(&b'\n');
                continue;
            }
            self.partial.extend_from_slice(piece);
            if piece.last() == Some(&b'\n') {
                match std::str::from_utf8(&self.partial) {
                    Ok(line) => self.evidence.observe_line(line),
                    Err(_) => {
                        self.evidence.pending = None;
                        self.evidence.state = ClaudeGoalState::Unknown;
                    }
                }
                self.partial.clear();
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscriptFrameError;

#[cfg(test)]
mod tests {
    use super::*;

    fn claude(text: &str) -> String {
        serde_json::json!({"type":"user", "message":{"content":text}}).to_string()
    }

    #[test]
    fn claude_accepts_only_a_matched_main_session_confirmation() {
        let mut evidence = ClaudeGoalEvidence::default();
        evidence.observe_line(&claude("A quoted <command-name>/goal</command-name>"));
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal set: invented</local-command-stdout>",
        ));
        assert_eq!(evidence.objective(), None);
        evidence.observe_line(&claude(
            "<command-name>/goal</command-name>\n<command-args>écrire\nune histoire</command-args>",
        ));
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal set: écrire\nune histoire</local-command-stdout>",
        ));
        assert_eq!(evidence.objective(), Some("écrire\nune histoire"));
    }

    #[test]
    fn claude_replacement_is_unknown_until_confirmed_and_clear_is_confirmed() {
        let mut evidence = ClaudeGoalEvidence::default();
        evidence.observe_line(&claude(
            "<command-name>/goal</command-name><command-args>first</command-args>",
        ));
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal set: first</local-command-stdout>",
        ));
        evidence.observe_line(&claude(
            "<command-name>/goal</command-name><command-args>second</command-args>",
        ));
        assert_eq!(evidence.objective(), None);
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal set: wrong</local-command-stdout>",
        ));
        assert_eq!(evidence.objective(), None);
        evidence.observe_line(&claude(
            "<command-name>/goal</command-name><command-args>clear</command-args>",
        ));
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal cleared.</local-command-stdout>",
        ));
        assert!(matches!(evidence.state(), ClaudeGoalState::Cleared));
    }

    #[test]
    fn claude_subagent_and_malformed_records_cannot_supply_evidence() {
        let mut evidence = ClaudeGoalEvidence::default();
        evidence.observe_line("{partial");
        evidence.observe_line(&serde_json::json!({"type":"user","isSidechain":true,"message":{"content":"<command-name>/goal</command-name><command-args>secret</command-args>"}}).to_string());
        evidence.observe_line(&claude(
            "<local-command-stdout>Goal set: secret</local-command-stdout>",
        ));
        assert_eq!(evidence.objective(), None);
    }

    #[test]
    fn codex_requires_matching_thread_and_maps_all_provider_phases() {
        for (status, phase) in [
            ("active", GoalPhase::Active),
            ("paused", GoalPhase::Paused),
            ("blocked", GoalPhase::Blocked),
            ("usageLimited", GoalPhase::UsageLimited),
            ("budgetLimited", GoalPhase::BudgetLimited),
            ("complete", GoalPhase::Reached),
        ] {
            let response = serde_json::json!({"goal":{"threadId":"one","objective":"do work","status":status}});
            assert!(
                codex_goal_response("one", &response)
                    == Some(GoalLookup::Present {
                        phase,
                        objective: "do work".into()
                    })
            );
            assert!(codex_goal_response("other", &response).is_none());
        }
        assert!(
            codex_goal_response("one", &serde_json::json!({"goal":null}))
                == Some(GoalLookup::Absent)
        );
        assert!(codex_goal_response("", &serde_json::json!({"goal":null})).is_none());
        assert!(codex_goal_response(
            "",
            &serde_json::json!({"goal":{"threadId":"","objective":"text","status":"active"}})
        )
        .is_none());
        assert!(codex_goal_response("one", &serde_json::json!({"error":"failed"})).is_none());
        assert!(codex_goal_response("one", &serde_json::json!({"goal":{"threadId":"one","objective":"text","status":"futureStatus"}})).is_none());
        assert!(codex_goal_response(
            "one",
            &serde_json::json!({"goal":{"threadId":"one","status":"active"}})
        )
        .is_none());
        assert!(codex_goal_response(
            "one",
            &serde_json::json!({"goal":{"threadId":"one","objective":null,"status":"active"}})
        )
        .is_none());
    }

    #[test]
    fn codex_response_rejects_oversized_objective_without_truncation() {
        let within_limit = "é".repeat(MAX_OBJECTIVE_BYTES / 2);
        let response = serde_json::json!({"goal":{"threadId":"one","objective":within_limit,"status":"active"}});
        assert!(
            matches!(codex_goal_response("one", &response), Some(GoalLookup::Present { objective, .. }) if objective.len() == MAX_OBJECTIVE_BYTES)
        );
        let oversized = "é".repeat(MAX_OBJECTIVE_BYTES / 2 + 1);
        let response =
            serde_json::json!({"goal":{"threadId":"one","objective":oversized,"status":"active"}});
        assert!(codex_goal_response("one", &response).is_none());
        assert!(codex_database_goal_row(
            "one",
            "one",
            "active",
            "x".repeat(MAX_OBJECTIVE_BYTES + 1)
        )
        .is_none());
    }

    #[test]
    fn codex_query_and_decoder_agree_on_a_synthetic_database() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("goals_1.sqlite");
        let setup = r#"
import sqlite3, sys
d = sqlite3.connect(sys.argv[1])
d.execute('create table _sqlx_migrations (version integer, success integer, checksum blob)')
d.executemany('insert into _sqlx_migrations values (?, 1, ?)', [(1, bytes.fromhex('8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713')), (2, bytes.fromhex('F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584'))])
d.execute('create table thread_goals (thread_id text primary key not null, goal_id text not null, objective text not null, status text not null, token_budget integer, tokens_used integer not null default 0, time_used_seconds integer not null default 0, created_at_ms integer not null, updated_at_ms integer not null)')
d.execute('insert into thread_goals (thread_id, goal_id, objective, status, created_at_ms, updated_at_ms) values (?, ?, ?, ?, 1, 1)', ('thread', 'goal', 'synthetic objective', 'paused'))
d.commit()
d.close()
"#;
        assert!(Command::new("python3")
            .args(["-c", setup])
            .arg(&database)
            .status()
            .unwrap()
            .success());
        // This synthetic fixture runs without the production sandbox and does
        // not qualify privacy, WAL concurrency or process-to-session binding.
        let query = include_str!("../../ilium-platform/src/codex_goal_query.py").replace(
            "file:/goal/goals_1.sqlite?mode=ro",
            &format!("file:{}?mode=ro", database.display()),
        );
        let mut child = Command::new("python3")
            .args(["-I", "-c", &query])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"thread_id":"thread"}"#)
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            codex_database_snapshot("thread", &snapshot)
                == Some(GoalLookup::Present {
                    phase: GoalPhase::Paused,
                    objective: "synthetic objective".into()
                })
        );
        assert!(codex_database_snapshot("other", &snapshot).is_none());
    }

    #[test]
    fn codex_database_snapshot_requires_valid_schema_before_absence_or_objective() {
        let mut snapshot = serde_json::json!({
            "user_version": 0,
            "migrations": [
                [1, 1, "8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713"],
                [2, 1, "F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584"]
            ],
            "columns": [
                [0,"thread_id","TEXT",1,null,1], [1,"goal_id","TEXT",1,null,0],
                [2,"objective","TEXT",1,null,0], [3,"status","TEXT",1,null,0],
                [4,"token_budget","INTEGER",0,null,0], [5,"tokens_used","INTEGER",1,"0",0],
                [6,"time_used_seconds","INTEGER",1,"0",0], [7,"created_at_ms","INTEGER",1,null,0],
                [8,"updated_at_ms","INTEGER",1,null,0]
            ],
            "rows": []
        });
        assert!(codex_database_snapshot("thread", &snapshot) == Some(GoalLookup::Absent));
        snapshot["rows"] = serde_json::json!([["thread", "paused", "synthetic"]]);
        assert!(
            codex_database_snapshot("thread", &snapshot)
                == Some(GoalLookup::Present {
                    phase: GoalPhase::Paused,
                    objective: "synthetic".into()
                })
        );
        assert!(codex_database_snapshot("other", &snapshot).is_none());
        snapshot["columns"][0][1] = serde_json::json!("wrong");
        assert!(codex_database_snapshot("thread", &snapshot).is_none());
        snapshot["rows"] = serde_json::json!([]);
        assert!(codex_database_snapshot("thread", &snapshot).is_none());
    }

    #[test]
    fn codex_goal_migrations_fail_closed_for_unknown_or_changed_schema() {
        let first = "8CE6280244A0B4B39C7C37C7F67AFF1670EC963D2401CDF2ABCFEA9904102DB6CD99B7BEDF366267791A45EB12F17713";
        let second = "F3FF7DE2FD89914820BEF371081E885FD17DF18A6FBDC432399A83255A8A01AA32FB7EEA5E4DE21A17BDA3DA16C49584";
        assert!(codex_goal_migrations_supported(&[
            (2, true, second),
            (1, true, first)
        ]));
        assert!(!codex_goal_migrations_supported(&[(1, true, first)]));
        assert!(!codex_goal_migrations_supported(&[
            (1, true, first),
            (2, false, second)
        ]));
        assert!(!codex_goal_migrations_supported(&[
            (1, true, first),
            (2, true, "changed")
        ]));
        assert!(!codex_goal_migrations_supported(&[
            (1, true, first),
            (1, true, first)
        ]));
        assert!(!codex_goal_migrations_supported(&[
            (1, true, first),
            (2, true, second),
            (3, true, "new")
        ]));
    }

    #[test]
    fn codex_database_rows_require_the_thread_and_storage_status_names() {
        for (status, phase) in [
            ("active", GoalPhase::Active),
            ("paused", GoalPhase::Paused),
            ("blocked", GoalPhase::Blocked),
            ("usage_limited", GoalPhase::UsageLimited),
            ("budget_limited", GoalPhase::BudgetLimited),
            ("complete", GoalPhase::Reached),
        ] {
            assert!(
                codex_database_goal_row("one", "one", status, "synthetic".into())
                    == Some(GoalLookup::Present {
                        phase,
                        objective: "synthetic".into()
                    })
            );
            assert!(codex_database_goal_row("one", "other", status, "synthetic".into()).is_none());
        }
        assert!(
            codex_database_goal_row("one", "one", "usageLimited", "synthetic".into()).is_none()
        );
        assert!(
            codex_database_goal_row("one", "one", "future_status", "synthetic".into()).is_none()
        );
        assert!(codex_database_goal_row("", "", "active", "synthetic".into()).is_none());
    }

    #[test]
    fn codex_read_failure_is_stale_and_generation_fences_late_results() {
        let mut evidence = GoalReconciler::new(GoalGeneration(7));
        assert!(evidence.apply_codex(
            GoalGeneration(7),
            Some(GoalLookup::Present {
                phase: GoalPhase::Paused,
                objective: "original".into()
            }),
            Some(GoalPhase::Active)
        ));
        assert!(evidence.observation().phase == Some(GoalPhase::Paused));
        assert!(!evidence.update_screen(Some(GoalPhase::Blocked)));
        assert!(evidence.observation().phase == Some(GoalPhase::Paused));
        assert!(evidence.apply_codex(GoalGeneration(7), None, Some(GoalPhase::Active)));
        assert!(evidence.observation().freshness == Freshness::Stale);
        assert!(evidence.observation().objective.as_deref() == Some("original"));
        assert!(evidence.update_screen(Some(GoalPhase::Blocked)));
        assert!(evidence.observation().phase == Some(GoalPhase::Blocked));
        let mut replacement = GoalReconciler::new(GoalGeneration(8));
        assert!(!replacement.apply_codex(
            GoalGeneration(7),
            Some(GoalLookup::Present {
                phase: GoalPhase::Active,
                objective: "old".into()
            }),
            None
        ));
        assert!(replacement.observation().objective.is_none());
        assert!(evidence.apply_codex(
            GoalGeneration(7),
            Some(GoalLookup::Absent),
            Some(GoalPhase::Active)
        ));
        assert!(evidence.observation().absent && evidence.observation().objective.is_none());
        assert!(evidence.apply_codex(GoalGeneration(7), None, None));
        assert!(!evidence.observation().absent && evidence.observation().objective.is_none());
        assert!(evidence.apply_codex(
            GoalGeneration(7),
            Some(GoalLookup::Present {
                phase: GoalPhase::BudgetLimited,
                objective: "replacement".into()
            }),
            Some(GoalPhase::Active)
        ));
        assert!(evidence.observation().freshness == Freshness::Current);
        assert!(evidence.observation().phase == Some(GoalPhase::BudgetLimited));
        assert!(evidence.observation().objective.as_deref() == Some("replacement"));
        evidence.invalidate_provider_identity(Some(GoalPhase::Paused));
        assert!(evidence.observation().objective.is_none());
        assert!(evidence.observation().freshness == Freshness::Stale);
        assert!(evidence.observation().phase == Some(GoalPhase::Paused));
    }

    #[test]
    fn transcript_waits_for_complete_lines_and_recovers_after_oversized_records() {
        let command =
            claude("<command-name>/goal</command-name><command-args>hello 世界</command-args>");
        let confirmation =
            claude("<local-command-stdout>Goal set: hello 世界</local-command-stdout>");
        let input = format!("{command}\n{confirmation}\n");
        let mut transcript = ClaudeGoalTranscript::default();
        for chunk in input.as_bytes().chunks(3) {
            assert!(transcript.push(chunk, 1024).is_ok());
        }
        assert_eq!(transcript.evidence().objective(), Some("hello 世界"));
        assert!(transcript.push(&[b'x'; 20], 10).is_ok());
        assert_eq!(transcript.evidence().objective(), None);
        assert!(transcript.has_partial_record());
        assert!(transcript.push(b"ignored\n", 1024).is_ok());
        assert!(!transcript.has_partial_record());
        assert!(transcript.push(input.as_bytes(), 1024).is_ok());
        assert_eq!(transcript.evidence().objective(), Some("hello 世界"));
        let mut transcript = ClaudeGoalTranscript::default();
        let combined = format!("{}\n{input}", "x".repeat(1025));
        assert!(transcript.push(combined.as_bytes(), 1024).is_ok());
        assert_eq!(transcript.evidence().objective(), Some("hello 世界"));
    }
}
