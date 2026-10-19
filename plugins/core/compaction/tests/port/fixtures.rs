//! Synthetic transcript trees for the scan twins (copy of the line shapes of
//! ilium-client/src/compaction_scan/fixtures.rs; every prompt, path and token count is invented).
//! Files are written into the fake `agent-homes` scope of a host.

use ilium_plugin_sdk::testing::FakeHost;

#[derive(Clone, Copy)]
pub struct Process {
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
        Self::claude()
    }
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` of a Unix time in milliseconds (civil-from-days, no calendar crate).
fn iso(milliseconds: i64) -> String {
    let seconds = milliseconds.div_euclid(1000);
    let millis = milliseconds.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

fn jitter(index: usize, turn: usize, bound: u32) -> u32 {
    ((index as u64 * 7_919 + turn as u64 * 104_729 + 13) % (u64::from(bound) + 1)) as u32
}

fn claude_usage_line(id: &str, timestamp_ms: i64, read: u32, write: u32, output: u32) -> String {
    format!(
        r#"{{"type":"assistant","isSidechain":false,"timestamp":"{}","message":{{"id":"{id}","model":"claude-sonnet-5-5","role":"assistant","content":[{{"type":"text","text":"synthetic"}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":{read},"cache_creation_input_tokens":{write},"output_tokens":{output},"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":{write}}}}}}}}}"#,
        iso(timestamp_ms)
    )
}

/// One Claude session transcript; `tag` keeps request ids unique per file.
pub fn claude_session(process: &Process, index: usize, tag: &str) -> String {
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

/// One Codex rollout: the compaction is its own request followed by a `compacted` line.
pub fn codex_session(process: &Process, index: usize, tag: &str) -> String {
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

/// Writes `text` at the `agent-homes:` relative path.
pub fn put(host: &mut FakeHost, relative: &str, text: &str) {
    host.fs()
        .write(format!("agent-homes:{relative}"), text.as_bytes());
}

pub fn claude_main_path(project: &str, name: &str) -> String {
    format!(".claude/projects/{project}/{name}.jsonl")
}
pub fn claude_subagent_path(project: &str, session: &str, name: &str) -> String {
    format!(".claude/projects/{project}/{session}/subagents/{name}.jsonl")
}
pub fn claude_workflow_path(project: &str, session: &str, name: &str) -> String {
    format!(".claude/projects/{project}/{session}/workflows/{name}.jsonl")
}
pub fn codex_path(name: &str) -> String {
    format!(".codex/sessions/2026/09/30/rollout-{name}.jsonl")
}
pub fn codex_archived_path(name: &str) -> String {
    format!(".codex/archived_sessions/rollout-{name}.jsonl")
}

/// `sessions` main Claude sessions under one project.
pub fn write_claude_corpus(host: &mut FakeHost, sessions: usize) {
    let process = Process::claude();
    for index in 0..sessions {
        let text = claude_session(&process, index, "c");
        put(
            host,
            &claude_main_path("-synthetic-project", &format!("session-{index:02}")),
            &text,
        );
    }
}

/// `sessions` Codex rollouts.
pub fn write_codex_corpus(host: &mut FakeHost, sessions: usize) {
    let process = Process::codex();
    for index in 0..sessions {
        let text = codex_session(&process, index, "x");
        put(host, &codex_path(&format!("{index:02}")), &text);
    }
}
