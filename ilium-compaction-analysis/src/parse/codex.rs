//! Codex CLI rollout format (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`).
//!
//! Rules (from the measured research, see `compaction/codex/extract.py`):
//!
//! * `token_usage_record` lines are deduplicated by `response_id`. The usage is
//!   `{input_tokens (includes the cached part), cached_input_tokens,
//!   cache_write_input_tokens (always 0 today), output_tokens (includes the
//!   reasoning tokens)}`; the context size of a request is `input_tokens`.
//! * A top-level `compacted` line marks a compaction. Its summarisation request
//!   is the `token_usage_record` whose `response_id` equals
//!   `compaction_response_id` (or `latest_token_usage_record.response_id`), one
//!   of the last few records. Lines without such an id (older formats, or
//!   history replayed by a resumed thread) cannot be matched and are counted
//!   in `compactions_unmatched`.
//! * The size the trigger fired at is the **previous work response**'s
//!   `input + output`; the measured post-compaction request is the next work
//!   response (its input size and cached split).
//! * The model comes from `turn_context.model` or
//!   `thread_settings_applied.thread_settings.model` and applies to the
//!   following requests. The context window comes from
//!   `event_msg token_count ... "model_context_window"` (read with a byte scan,
//!   no JSON parse), `turn_context` or `session_meta` when present.
//! * Files are keyed by file, never by `thread_id`: a resumed thread repeats
//!   history in a new file.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::value::RawValue;

use super::sink::{RawCompaction, RawTurn, TraceSink};
use super::tools::{add_command, add_edit, extract_exec_commands};
use super::{FormatParser, LogFormat, ParseOptions};
use crate::agent::AgentKind;
use crate::trace::CompactionTrigger;
use crate::trace::ToolAccumulator;
use crate::util::{find_bytes, fnv1a_64, parse_iso8601_ms, saturate_u32};

pub(super) const FORMAT: LogFormat = LogFormat {
    agent: AgentKind::Codex,
    name: "codex-rollout-jsonl",
    relevance_needles: &[
        b"token_usage_record",
        b"\"compacted\"",
        b"turn_context",
        b"session_meta",
        b"thread_settings_applied",
        b"model_context_window",
        b"\"function_call\"",
        b"\"custom_tool_call\"",
    ],
    new_parser: || Box::new(CodexParser::default()),
};

/// Tool-call lines above this size are not analysed (the research skipped
/// `exec` code above 400 kB the same way).
const MAX_TOOL_LINE_BYTES: usize = 1024 * 1024;

/// Function-call tools that run one shell command in `cmd` / `command`.
const SHELL_TOOLS: [&str; 5] = [
    "exec_command",
    "shell",
    "local_shell",
    "shell_command",
    "container.exec",
];

/// How many recent requests are searched for a compaction's summarisation call.
const REQUEST_LOOKBACK: usize = 4;

#[derive(Default)]
struct CodexParser {
    /// Response-id hash to turn index.
    turn_by_response: HashMap<u64, usize>,
    /// Model currently in force.
    current_model: Option<String>,
    /// Working directory in force (never stored; resolves relative paths).
    current_cwd: Option<String>,
    /// Tool calls seen since the last usage record: a response's tool calls
    /// precede its `token_usage_record`.
    pending_tools: ToolAccumulator,
}

#[derive(Deserialize)]
struct CodexLine<'a> {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(borrow, default)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct UsageRecord {
    #[serde(default)]
    response_id: Option<String>,
    #[serde(default)]
    usage: Option<CodexUsage>,
}

#[derive(Deserialize)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cached_input_tokens: Option<u64>,
    #[serde(default)]
    cache_write_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ContextPayload {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
struct MetaPayload {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    source: Option<serde_json::Value>,
    #[serde(default)]
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
struct ToolCallPayload {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    input: Option<String>,
}

/// The `cmd` / `command` argument of a shell tool: a string or an argv list.
#[derive(Deserialize)]
#[serde(untagged)]
enum CommandArgument {
    Text(String),
    Words(Vec<String>),
}

#[derive(Deserialize, Default)]
struct ShellArguments {
    #[serde(default)]
    cmd: Option<CommandArgument>,
    #[serde(default)]
    command: Option<CommandArgument>,
}

#[derive(Deserialize)]
struct EventPayload {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    thread_settings: Option<ThreadSettings>,
}

#[derive(Deserialize)]
struct ThreadSettings {
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct CompactedPayload<'a> {
    #[serde(default)]
    compaction_response_id: Option<String>,
    #[serde(default)]
    latest_token_usage_record: Option<LatestRecord>,
    #[serde(borrow, default)]
    replacement_history: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct LatestRecord {
    #[serde(default)]
    response_id: Option<String>,
}

impl FormatParser for CodexParser {
    fn parse_line(
        &mut self,
        line: &[u8],
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) -> Result<(), serde_json::Error> {
        if let Some(window) = scan_context_window(line) {
            sink.set_context_window(window);
        }
        let envelope: CodexLine<'_> = serde_json::from_slice(line)?;
        let Some(payload) = envelope.payload else {
            return Ok(());
        };
        let timestamp_ms = envelope
            .timestamp
            .as_deref()
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);
        match envelope.kind.as_deref() {
            Some("token_usage_record") => self.handle_usage(payload, timestamp_ms, sink)?,
            Some("compacted") => self.handle_compacted(payload, timestamp_ms, sink, options)?,
            Some("turn_context") => {
                let context: ContextPayload = serde_json::from_str(payload.get())?;
                if let Some(model) = context.model.filter(|name| !name.is_empty()) {
                    self.current_model = Some(model);
                }
                if let Some(window) = context.model_context_window {
                    sink.set_context_window(saturate_u32(window));
                }
                if let Some(cwd) = context.cwd.filter(|cwd| !cwd.is_empty()) {
                    self.current_cwd = Some(cwd);
                }
            }
            Some("session_meta") => {
                let meta: MetaPayload = serde_json::from_str(payload.get())?;
                let is_subagent = meta
                    .source
                    .as_ref()
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|source| source.contains_key("subagent"));
                if is_subagent {
                    sink.mark_subagent();
                }
                if let Some(window) = meta.model_context_window {
                    sink.set_context_window(saturate_u32(window));
                }
                if let Some(cwd) = meta.cwd.filter(|cwd| !cwd.is_empty()) {
                    self.current_cwd = Some(cwd);
                }
            }
            Some("response_item") => self.handle_tool_call(line, payload, sink)?,
            // Only `thread_settings_applied` events carry the model; the needle
            // check avoids parsing every `token_count` event.
            Some("event_msg") if find_bytes(line, b"thread_settings_applied").is_some() => {
                let event: EventPayload = serde_json::from_str(payload.get())?;
                if event.kind.as_deref() == Some("thread_settings_applied") {
                    if let Some(model) = event
                        .thread_settings
                        .and_then(|settings| settings.model)
                        .filter(|name| !name.is_empty())
                    {
                        self.current_model = Some(model);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl CodexParser {
    fn handle_usage(
        &mut self,
        payload: &RawValue,
        timestamp_ms: i64,
        sink: &mut TraceSink,
    ) -> Result<(), serde_json::Error> {
        let record: UsageRecord = serde_json::from_str(payload.get())?;
        let Some(usage) = record.usage else {
            sink.counters_mut().requests_without_usage += 1;
            return Ok(());
        };
        let response_hash = record
            .response_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(|id| fnv1a_64(id.as_bytes()));
        if let Some(hash) = response_hash {
            if self.turn_by_response.contains_key(&hash) {
                sink.counters_mut().duplicate_requests += 1;
                return Ok(());
            }
        }
        let input_total = usage.input_tokens.unwrap_or(0);
        let cached = usage.cached_input_tokens.unwrap_or(0).min(input_total);
        let written = usage
            .cache_write_input_tokens
            .unwrap_or(0)
            .min(input_total - cached);
        let uncached = input_total - cached - written;
        let model = self.current_model.clone().unwrap_or_default();
        let model_index = sink.model_index(&model);
        let hash64 = response_hash.unwrap_or(0);
        let tools = std::mem::take(&mut self.pending_tools);
        let index = sink.push_turn_with_tools(
            RawTurn {
                timestamp_ms,
                context_tokens: saturate_u32(input_total),
                input_tokens: saturate_u32(uncached),
                cache_read_tokens: saturate_u32(cached),
                // Codex bills no cache-write premium; the (always zero today)
                // field is kept in the short tier so no token is lost.
                cache_write_5m_tokens: saturate_u32(written),
                cache_write_1h_tokens: 0,
                output_tokens: saturate_u32(usage.output_tokens.unwrap_or(0)),
                model: model_index,
                id_hash: (hash64 >> 32) as u32 ^ hash64 as u32,
            },
            &tools,
        );
        if let Some(hash) = response_hash {
            self.turn_by_response.insert(hash, index);
        }
        Ok(())
    }

    /// Records the features of a `function_call` / `custom_tool_call` line for
    /// the next usage record. Other `response_item` lines are ignored.
    fn handle_tool_call(
        &mut self,
        line: &[u8],
        payload: &RawValue,
        sink: &mut TraceSink,
    ) -> Result<(), serde_json::Error> {
        let is_call = find_bytes(line, b"\"type\":\"function_call\"").is_some()
            || find_bytes(line, b"\"type\":\"custom_tool_call\"").is_some();
        if !is_call {
            return Ok(());
        }
        if line.len() > MAX_TOOL_LINE_BYTES {
            sink.counters_mut().tool_calls_dropped += 1;
            return Ok(());
        }
        let call: ToolCallPayload = serde_json::from_str(payload.get())?;
        if !matches!(
            call.kind.as_deref(),
            Some("function_call" | "custom_tool_call")
        ) {
            return Ok(());
        }
        let name = call.name.as_deref().unwrap_or("");
        let cwd = self.current_cwd.as_deref();
        let pending = &mut self.pending_tools;
        pending.tool_calls = pending.tool_calls.saturating_add(1);
        if name == "apply_patch" {
            add_edit(pending);
        }
        if name == "exec" {
            let code = call.input.as_deref().unwrap_or("");
            if code.contains("apply_patch") {
                add_edit(pending);
            }
            for command in extract_exec_commands(code) {
                add_command(pending, &command, cwd, true);
            }
        } else if SHELL_TOOLS.contains(&name) {
            let arguments: ShellArguments = call
                .arguments
                .as_deref()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_default();
            let command = match arguments.cmd.or(arguments.command) {
                Some(CommandArgument::Text(text)) => Some(text),
                Some(CommandArgument::Words(words)) => Some(words.join(" ")),
                None => None,
            };
            if let Some(command) = command {
                add_command(pending, &command, cwd, true);
            }
        }
        Ok(())
    }

    fn handle_compacted(
        &mut self,
        payload: &RawValue,
        timestamp_ms: i64,
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) -> Result<(), serde_json::Error> {
        let compacted: CompactedPayload<'_> = serde_json::from_str(payload.get())?;
        let request_id = compacted
            .compaction_response_id
            .filter(|id| !id.is_empty())
            .or_else(|| {
                compacted
                    .latest_token_usage_record
                    .and_then(|record| record.response_id)
                    .filter(|id| !id.is_empty())
            });
        let request_index = request_id.and_then(|id| {
            let hash = fnv1a_64(id.as_bytes());
            self.turn_by_response
                .get(&hash)
                .copied()
                .filter(|&index| index + REQUEST_LOOKBACK >= sink.turn_count())
        });
        let history_characters = compacted
            .replacement_history
            .map_or(0, |raw| raw.get().len());
        let raw = RawCompaction {
            timestamp_ms,
            // Codex does not record whether a compaction was automatic.
            trigger: Some(CompactionTrigger::Unknown),
            replacement_history_tokens: if options.history_characters_per_token > 0.0 {
                (history_characters as f64 / options.history_characters_per_token).round() as u32
            } else {
                0
            },
            ..RawCompaction::default()
        };
        let matched =
            request_index.is_some_and(|index| sink.open_compaction_from_request(index, raw));
        if !matched {
            sink.counters_mut().compactions_unmatched += 1;
        }
        Ok(())
    }
}

/// Reads `"model_context_window":N` out of a line without parsing JSON.
fn scan_context_window(line: &[u8]) -> Option<u32> {
    const KEY: &[u8] = b"\"model_context_window\":";
    let start = find_bytes(line, KEY)? + KEY.len();
    let digits: Vec<u8> = line[start..]
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take_while(u8::is_ascii_digit)
        .collect();
    let text = std::str::from_utf8(&digits).ok()?;
    let value: u64 = text.parse().ok()?;
    (value > 0).then(|| saturate_u32(value))
}
