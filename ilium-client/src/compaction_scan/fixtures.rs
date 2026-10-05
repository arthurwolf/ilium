//! Synthetic transcript trees for the scan tests. Every prompt, path and
//! token count is invented; the line shapes copy real Claude Code and Codex
//! transcripts (the fixtures of `ilium-compaction-analysis` document them).

use std::path::{Path, PathBuf};

/// Parameters of a generated session: warm turns growing by `growth` plus a
/// deterministic jitter, compacting when the context reaches `trigger`.
#[derive(Clone, Copy)]
pub(crate) struct Process {
    pub turns: usize,
    pub trigger: u32,
    pub start_context: u32,
    pub growth: u32,
    pub growth_jitter: u32,
    pub post_cache_read: u32,
    pub post_fresh: u32,
    pub epoch_seconds: i64,
}

impl Process {
    pub const fn claude() -> Self {
        Self {
            turns: 160,
            trigger: 150_000,
            start_context: 20_000,
            growth: 2_000,
            growth_jitter: 1_000,
            post_cache_read: 15_000,
            post_fresh: 50_000,
            epoch_seconds: 1_780_000_000,
        }
    }
    pub const fn codex() -> Self {
        Self {
            turns: 160,
            trigger: 150_000,
            start_context: 20_000,
            growth: 2_000,
            growth_jitter: 1_000,
            post_cache_read: 15_000,
            post_fresh: 50_000,
            epoch_seconds: 1_780_000_000,
        }
    }
}

fn iso(milliseconds: i64) -> String {
    chrono::DateTime::from_timestamp_millis(milliseconds)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// Deterministic pseudo-jitter in `0..=bound`.
fn jitter(index: usize, turn: usize, bound: u32) -> u32 {
    let mixed = (index as u64 * 7_919 + turn as u64 * 104_729 + 13) % (u64::from(bound) + 1);
    mixed as u32
}

fn claude_usage_line(id: &str, timestamp_ms: i64, read: u32, write: u32, output: u32) -> String {
    format!(
        r#"{{"type":"assistant","isSidechain":false,"timestamp":"{}","message":{{"id":"{id}","model":"claude-sonnet-5-5","role":"assistant","content":[{{"type":"text","text":"synthetic"}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":{read},"cache_creation_input_tokens":{write},"output_tokens":{output},"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":{write}}}}}}}}}"#,
        iso(timestamp_ms)
    )
}

/// One Claude session transcript; `tag` keeps request ids unique per file so
/// the cross-file dedupe does not merge unrelated sessions.
pub(crate) fn claude_session(process: &Process, index: usize, tag: &str) -> String {
    let mut lines = Vec::new();
    let mut clock = (process.epoch_seconds + index as i64 * 100_000) * 1_000;
    let mut context = process.start_context;
    let mut since_compaction = 100;
    let mut after_compaction = false;
    for turn in 0..process.turns {
        clock += 30_000;
        let id = format!("msg_{tag}_{index}_{turn}");
        let (read, write) = if turn == 0 {
            (0, process.start_context)
        } else if after_compaction {
            (process.post_cache_read, process.post_fresh)
        } else {
            (
                context,
                process.growth + jitter(index, turn, process.growth_jitter),
            )
        };
        lines.push(claude_usage_line(&id, clock, read, write, 300));
        context = read + write + 2;
        after_compaction = false;
        since_compaction += 1;
        if context >= process.trigger && since_compaction >= 5 {
            lines.push(format!(
                r#"{{"type":"system","subtype":"compact_boundary","timestamp":"{}","compactMetadata":{{"trigger":"auto","preTokens":{context},"postTokens":15000,"durationMs":9000}}}}"#,
                iso(clock + 1_000)
            ));
            lines.push(format!(
                r#"{{"type":"user","isCompactSummary":true,"timestamp":"{}","message":{{"role":"user","content":"{}"}}}}"#,
                iso(clock + 1_100),
                "s".repeat(14_000)
            ));
            after_compaction = true;
            since_compaction = 0;
            clock += 20_000;
        }
    }
    lines.join("\n") + "\n"
}

fn codex_record(id: &str, timestamp_ms: i64, input: u32, cached: u32, output: u32) -> String {
    format!(
        r#"{{"timestamp":"{}","type":"token_usage_record","payload":{{"response_id":"{id}","usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{output}}}}}}}"#,
        iso(timestamp_ms)
    )
}

/// One Codex rollout: the compaction is its own request followed by a
/// `compacted` line and a rebuilt prefix.
pub(crate) fn codex_session(process: &Process, index: usize, tag: &str) -> String {
    let mut lines = vec![
        r#"{"timestamp":"2026-09-01T00:00:00.000Z","type":"session_meta","payload":{"id":"synthetic","source":"cli"}}"#.to_string(),
        r#"{"timestamp":"2026-09-01T00:00:01.000Z","type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#.to_string(),
    ];
    let mut clock = (process.epoch_seconds + index as i64 * 100_000) * 1_000;
    let mut context = process.start_context;
    let mut cached = 0;
    let mut since_compaction = 100;
    let mut serial = 0;
    for turn in 0..process.turns {
        clock += 20_000;
        serial += 1;
        let id = format!("resp_{tag}_{index}_{serial}");
        if turn > 0 {
            cached = context;
            context += process.growth + jitter(index, turn, process.growth_jitter);
        }
        lines.push(codex_record(&id, clock, context, cached, 150));
        since_compaction += 1;
        if context + 150 >= process.trigger && since_compaction >= 5 {
            clock += 15_000;
            serial += 1;
            let request_id = format!("resp_{tag}_{index}_{serial}");
            lines.push(codex_record(
                &request_id,
                clock,
                context + 200,
                context * 9 / 10,
                4_000,
            ));
            lines.push(format!(
                r#"{{"timestamp":"{}","type":"compacted","payload":{{"message":"","replacement_history":[],"compaction_response_id":"{request_id}"}}}}"#,
                iso(clock + 10)
            ));
            context = process.post_cache_read + process.post_fresh;
            cached = process.post_cache_read;
            since_compaction = 0;
            clock += 10_000;
            serial += 1;
            let post_id = format!("resp_{tag}_{index}_{serial}");
            lines.push(codex_record(&post_id, clock, context, cached, 150));
        }
    }
    lines.join("\n") + "\n"
}

/// Writes `text` to `path`, creating the parent directories.
pub(crate) fn write(path: &Path, text: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create fixture directory");
    }
    std::fs::write(path, text).expect("write fixture file");
    path.to_path_buf()
}

pub(crate) fn claude_main_path(home: &Path, project: &str, name: &str) -> PathBuf {
    home.join(".claude/projects")
        .join(project)
        .join(format!("{name}.jsonl"))
}

pub(crate) fn claude_subagent_path(
    home: &Path,
    project: &str,
    session: &str,
    name: &str,
) -> PathBuf {
    home.join(".claude/projects")
        .join(project)
        .join(session)
        .join("subagents")
        .join(format!("{name}.jsonl"))
}

pub(crate) fn claude_workflow_path(
    home: &Path,
    project: &str,
    session: &str,
    name: &str,
) -> PathBuf {
    home.join(".claude/projects")
        .join(project)
        .join(session)
        .join("workflows")
        .join(format!("{name}.jsonl"))
}

pub(crate) fn codex_path(home: &Path, name: &str) -> PathBuf {
    home.join(".codex/sessions/2026/09/30")
        .join(format!("rollout-{name}.jsonl"))
}

pub(crate) fn codex_archived_path(home: &Path, name: &str) -> PathBuf {
    home.join(".codex/archived_sessions")
        .join(format!("rollout-{name}.jsonl"))
}

/// A corpus of `sessions` main Claude sessions under one project.
pub(crate) fn write_claude_corpus(home: &Path, sessions: usize) {
    let process = Process::claude();
    for index in 0..sessions {
        write(
            &claude_main_path(home, "-synthetic-project", &format!("session-{index:02}")),
            &claude_session(&process, index, "c"),
        );
    }
}

/// A corpus of `sessions` Codex rollouts.
pub(crate) fn write_codex_corpus(home: &Path, sessions: usize) {
    let process = Process::codex();
    for index in 0..sessions {
        write(
            &codex_path(home, &format!("{index:02}")),
            &codex_session(&process, index, "x"),
        );
    }
}
