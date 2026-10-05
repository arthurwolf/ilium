//! Claude Code transcript format (`~/.claude/projects/**/*.jsonl`).
//!
//! Rules (from the measured research, see `compaction/claude/extract.py`):
//!
//! * `assistant` lines are written once per content block of a message; all
//!   blocks share `message.id` and `message.usage`. Requests are deduplicated
//!   by `message.id`, and a later line with the same id replaces the earlier
//!   usage (streaming writes end with the final counts).
//! * When `usage.iterations` exists, the `message` iterations are the main
//!   requests (summed for cost; the first one gives the context size) and the
//!   other iterations (`advisor_message`) are excluded, so an advisor tool call
//!   never double counts the context.
//! * Context size = `input_tokens + cache_read_input_tokens +
//!   cache_creation_input_tokens`. The write tier comes from
//!   `usage.cache_creation.ephemeral_{5m,1h}_input_tokens`; untiered writes are
//!   assumed to be 1-hour writes.
//! * `system` lines with subtype `compact_boundary` carry
//!   `compactMetadata {trigger, preTokens, postTokens, durationMs}`. The logged
//!   `postTokens` is **not** the next prompt size; the first following
//!   assistant request is the measured post-compaction request.
//! * The `isCompactSummary` user line that follows holds the summary text; its
//!   length / 3.5 estimates the summary tokens.

use std::collections::HashMap;

use serde::de::{self, IgnoredAny, Visitor};
use serde::Deserialize;

use super::sink::{RawCompaction, RawTurn, TraceSink};
use super::tools::add_claude_tool;
use super::{FormatParser, LogFormat, ParseOptions};
use crate::agent::AgentKind;
use crate::trace::CompactionTrigger;
use crate::trace::ToolAccumulator;
use crate::util::{fnv1a_64, parse_iso8601_ms, saturate_u32};

pub(super) const FORMAT: LogFormat = LogFormat {
    agent: AgentKind::ClaudeCode,
    name: "claude-code-jsonl",
    relevance_needles: &[b"\"usage\"", b"compact_boundary", b"isCompactSummary"],
    new_parser: || Box::new(ClaudeParser::default()),
};

#[derive(Default)]
struct ClaudeParser {
    /// Message-id hash to turn index.
    turn_by_message: HashMap<u64, usize>,
    /// Summary characters seen before their `compact_boundary` line.
    orphan_summary_characters: Option<usize>,
    /// Hashes of the tool-use ids already counted (blocks can repeat).
    seen_tool_ids: std::collections::HashSet<u64>,
}

#[derive(Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: Option<bool>,
    #[serde(rename = "isCompactSummary", default)]
    is_compact_summary: Option<bool>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessage>,
    #[serde(rename = "compactMetadata", default)]
    compact_metadata: Option<CompactMetadata>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
    #[serde(default)]
    content: Option<ClaudeContent>,
}

#[derive(Deserialize, Default)]
struct UsageFields {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    #[serde(flatten)]
    totals: UsageFields,
    #[serde(default)]
    iterations: Option<Vec<ClaudeIteration>>,
}

#[derive(Deserialize)]
struct ClaudeIteration {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(flatten)]
    fields: UsageFields,
}

#[derive(Deserialize, Default)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: Option<u64>,
    #[serde(default)]
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct CompactMetadata {
    #[serde(default)]
    trigger: Option<String>,
    #[serde(rename = "preTokens", default)]
    pre_tokens: Option<u64>,
    #[serde(rename = "postTokens", default)]
    post_tokens: Option<u64>,
    #[serde(rename = "durationMs", default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    precomputed: Option<serde_json::Value>,
}

/// A message `content`: the character length when it is a string (the
/// compaction summary), the tool-use blocks when it is a block list.
struct ClaudeContent {
    text_characters: usize,
    tool_uses: Vec<ToolUse>,
}

/// One `tool_use` block, reduced to what the rework features need.
#[derive(Default)]
struct ToolUse {
    id: Option<String>,
    name: Option<String>,
    input: ToolInput,
}

/// The fields of a tool input that matter; everything else is dropped.
#[derive(Default)]
struct ToolInput {
    file_path: Option<String>,
    pattern: Option<String>,
    path: Option<String>,
    glob: Option<String>,
    command: Option<String>,
}

/// A string value, or `None` for any other JSON type.
struct MaybeString(Option<String>);

impl<'de> Deserialize<'de> for MaybeString {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StringVisitor;
        impl<'de> Visitor<'de> for StringVisitor {
            type Value = MaybeString;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(MaybeString(Some(value.to_string())))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(MaybeString(None))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(MaybeString(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(MaybeString(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(MaybeString(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(MaybeString(None))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(MaybeString(None))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MaybeString(None))
            }
        }
        deserializer.deserialize_any(StringVisitor)
    }
}

impl<'de> Deserialize<'de> for ToolInput {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InputVisitor;
        impl<'de> Visitor<'de> for InputVisitor {
            type Value = ToolInput;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a tool input")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut input = ToolInput::default();
                while let Some(key) = map.next_key::<String>()? {
                    let slot = match key.as_str() {
                        "file_path" | "notebook_path" => &mut input.file_path,
                        "pattern" => &mut input.pattern,
                        "path" => &mut input.path,
                        "glob" => &mut input.glob,
                        "command" => &mut input.command,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    let value = map.next_value::<MaybeString>()?.0;
                    if slot.is_none() {
                        *slot = value;
                    }
                }
                Ok(input)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(ToolInput::default())
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(ToolInput::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(ToolInput::default())
            }
        }
        deserializer.deserialize_any(InputVisitor)
    }
}

/// A content-list element: a tool use, or anything else (dropped).
struct Block(Option<ToolUse>);

impl<'de> Deserialize<'de> for Block {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BlockVisitor;
        impl<'de> Visitor<'de> for BlockVisitor {
            type Value = Block;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a content block")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind = None;
                let mut tool = ToolUse::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => kind = map.next_value::<MaybeString>()?.0,
                        "id" => tool.id = map.next_value::<MaybeString>()?.0,
                        "name" => tool.name = map.next_value::<MaybeString>()?.0,
                        "input" => tool.input = map.next_value::<ToolInput>()?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(Block((kind.as_deref() == Some("tool_use")).then_some(tool)))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(Block(None))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Block(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Block(None))
            }
        }
        deserializer.deserialize_any(BlockVisitor)
    }
}

impl<'de> Deserialize<'de> for ClaudeContent {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ContentVisitor;
        impl<'de> Visitor<'de> for ContentVisitor {
            type Value = ClaudeContent;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a message content value")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(ClaudeContent {
                    text_characters: value.chars().count(),
                    tool_uses: Vec::new(),
                })
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut tool_uses = Vec::new();
                while let Some(block) = seq.next_element::<Block>()? {
                    tool_uses.extend(block.0);
                }
                Ok(ClaudeContent {
                    text_characters: 0,
                    tool_uses,
                })
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(ClaudeContent {
                    text_characters: 0,
                    tool_uses: Vec::new(),
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(ClaudeContent {
                    text_characters: 0,
                    tool_uses: Vec::new(),
                })
            }
        }
        deserializer.deserialize_any(ContentVisitor)
    }
}

/// Token sums of the main requests of one assistant message.
struct MainUsage {
    context: u64,
    input: u64,
    cache_read: u64,
    write_5m: u64,
    write_1h: u64,
    output: u64,
}

impl MainUsage {
    fn from_fields(parts: &[&UsageFields]) -> Self {
        let value = |field: Option<u64>| field.unwrap_or(0);
        let mut sum = MainUsage {
            context: 0,
            input: 0,
            cache_read: 0,
            write_5m: 0,
            write_1h: 0,
            output: 0,
        };
        let mut written_total = 0_u64;
        for fields in parts {
            sum.input += value(fields.input_tokens);
            sum.cache_read += value(fields.cache_read_input_tokens);
            sum.output += value(fields.output_tokens);
            written_total += value(fields.cache_creation_input_tokens);
            if let Some(tiers) = &fields.cache_creation {
                sum.write_5m += value(tiers.ephemeral_5m_input_tokens);
                sum.write_1h += value(tiers.ephemeral_1h_input_tokens);
            }
        }
        // Untiered or partially tiered writes are assumed to be 1-hour writes.
        let tiered = sum.write_5m + sum.write_1h;
        sum.write_1h += written_total.saturating_sub(tiered);
        if let Some(first) = parts.first() {
            sum.context = value(first.input_tokens)
                + value(first.cache_read_input_tokens)
                + value(first.cache_creation_input_tokens);
        }
        sum
    }
}

impl ClaudeUsage {
    /// The main-request usage: `message` iterations when the log lists
    /// iterations (advisor iterations excluded), else the top-level usage.
    fn main_usage(&self) -> MainUsage {
        if let Some(iterations) = self.iterations.as_ref().filter(|list| !list.is_empty()) {
            let message_iterations: Vec<&UsageFields> = iterations
                .iter()
                .filter(|iteration| iteration.kind.as_deref() == Some("message"))
                .map(|iteration| &iteration.fields)
                .collect();
            if message_iterations.is_empty() {
                return MainUsage::from_fields(&[&iterations[0].fields]);
            }
            return MainUsage::from_fields(&message_iterations);
        }
        MainUsage::from_fields(&[&self.totals])
    }
}

impl FormatParser for ClaudeParser {
    fn parse_line(
        &mut self,
        line: &[u8],
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) -> Result<(), serde_json::Error> {
        let parsed: ClaudeLine = serde_json::from_slice(line)?;
        let timestamp_ms = parsed
            .timestamp
            .as_deref()
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);
        match parsed.kind.as_deref() {
            Some("assistant") => self.handle_assistant(parsed, timestamp_ms, sink, options),
            Some("system") if parsed.subtype.as_deref() == Some("compact_boundary") => {
                self.handle_boundary(parsed, timestamp_ms, sink, options);
            }
            Some("user") if parsed.is_compact_summary == Some(true) => {
                self.handle_summary(parsed, sink, options);
            }
            _ => {}
        }
        Ok(())
    }
}

impl ClaudeParser {
    fn handle_assistant(
        &mut self,
        parsed: ClaudeLine,
        timestamp_ms: i64,
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) {
        let Some(message) = parsed.message else {
            return;
        };
        let (Some(usage), Some(id)) = (message.usage.as_ref(), message.id.as_deref()) else {
            sink.counters_mut().requests_without_usage += 1;
            return;
        };
        let model = message.model.as_deref().unwrap_or("");
        if id.is_empty() || model == "<synthetic>" {
            return;
        }
        if !sink.is_subagent() {
            if parsed.is_sidechain == Some(true) {
                sink.counters_mut().sidechain_requests_skipped += 1;
                return;
            }
            let lowered = model.to_ascii_lowercase();
            if options
                .skip_models_in_main_sessions
                .iter()
                .any(|excluded| lowered.contains(excluded.as_str()))
            {
                sink.counters_mut().skipped_model_requests += 1;
                return;
            }
        }
        let main = usage.main_usage();
        if main.context == 0 && main.output == 0 {
            sink.counters_mut().requests_without_usage += 1;
            return;
        }
        let id_hash64 = fnv1a_64(id.as_bytes());
        let model_index = sink.model_index(model);
        let raw = RawTurn {
            timestamp_ms,
            context_tokens: saturate_u32(main.context),
            input_tokens: saturate_u32(main.input),
            cache_read_tokens: saturate_u32(main.cache_read),
            cache_write_5m_tokens: saturate_u32(main.write_5m),
            cache_write_1h_tokens: saturate_u32(main.write_1h),
            output_tokens: saturate_u32(main.output),
            model: model_index,
            id_hash: (id_hash64 >> 32) as u32 ^ id_hash64 as u32,
        };
        self.orphan_summary_characters = None;
        let tools = self.collect_tools(message.content.as_ref(), parsed.cwd.as_deref());
        if let Some(&existing) = self.turn_by_message.get(&id_hash64) {
            sink.counters_mut().duplicate_requests += 1;
            sink.replace_turn(existing, raw);
            sink.add_tools_to_turn(existing, &tools);
            return;
        }
        let index = sink.push_turn_with_tools(raw, &tools);
        self.turn_by_message.insert(id_hash64, index);
    }

    /// The rework features of the tool-use blocks of one line, skipping
    /// blocks already counted (the same block can be logged twice).
    fn collect_tools(
        &mut self,
        content: Option<&ClaudeContent>,
        cwd: Option<&str>,
    ) -> ToolAccumulator {
        let mut accumulator = ToolAccumulator::default();
        let Some(content) = content else {
            return accumulator;
        };
        for tool in &content.tool_uses {
            if let Some(id) = tool.id.as_deref().filter(|id| !id.is_empty()) {
                if !self.seen_tool_ids.insert(fnv1a_64(id.as_bytes())) {
                    continue;
                }
            }
            let input = &tool.input;
            add_claude_tool(
                &mut accumulator,
                tool.name.as_deref().unwrap_or(""),
                input.file_path.as_deref(),
                [
                    input.pattern.as_deref(),
                    input.path.as_deref(),
                    input.glob.as_deref(),
                ],
                input.command.as_deref(),
                cwd,
            );
        }
        accumulator
    }

    fn handle_boundary(
        &mut self,
        parsed: ClaudeLine,
        timestamp_ms: i64,
        sink: &mut TraceSink,
        options: &ParseOptions,
    ) {
        let metadata = parsed.compact_metadata;
        let trigger = match metadata.as_ref().and_then(|meta| meta.trigger.as_deref()) {
            Some("auto") => CompactionTrigger::Auto,
            Some("manual") => CompactionTrigger::Manual,
            _ => CompactionTrigger::Unknown,
        };
        let duration_ms = metadata
            .as_ref()
            .and_then(|meta| meta.duration_ms)
            .map(saturate_u32);
        let precomputed_flag = metadata
            .as_ref()
            .and_then(|meta| meta.precomputed.as_ref())
            .is_some_and(json_truthy);
        // The research treated a compaction that finished in under a second as
        // precomputed in the background.
        let finished_instantly = duration_ms.is_some_and(|millis| millis < 1_000);
        let raw = RawCompaction {
            timestamp_ms,
            trigger: Some(trigger),
            pre_tokens: metadata
                .as_ref()
                .and_then(|meta| meta.pre_tokens)
                .map_or(0, saturate_u32),
            logged_post_tokens: metadata
                .as_ref()
                .and_then(|meta| meta.post_tokens)
                .map_or(0, saturate_u32),
            summary_tokens: 0,
            summary_estimated: true,
            duration_ms: duration_ms.unwrap_or(0),
            precomputed: precomputed_flag || finished_instantly,
            replacement_history_tokens: 0,
        };
        sink.open_compaction(raw);
        if let Some(characters) = self.orphan_summary_characters.take() {
            let tokens = characters_to_tokens(characters, options.summary_characters_per_token);
            sink.attach_summary_to_open_compaction(tokens);
        }
    }

    fn handle_summary(&mut self, parsed: ClaudeLine, sink: &mut TraceSink, options: &ParseOptions) {
        let characters = parsed
            .message
            .and_then(|message| message.content)
            .map_or(0, |content| content.text_characters);
        let tokens = characters_to_tokens(characters, options.summary_characters_per_token);
        if !sink.attach_summary_to_open_compaction(tokens) {
            self.orphan_summary_characters = Some(characters);
        }
    }
}

fn characters_to_tokens(characters: usize, characters_per_token: f64) -> u32 {
    if characters_per_token <= 0.0 {
        return 0;
    }
    (characters as f64 / characters_per_token).round() as u32
}

fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
    }
}
