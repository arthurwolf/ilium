//! A neutral conversation model parsed from either agent's transcript.
//!
//! Only the *active* conversation is read: for Claude Code the `parentUuid`
//! chain from the newest record back to the last compaction boundary (plus the
//! boundary's preserved segment), for Codex the items after the last
//! `compacted` record applied as an anchor. Everything the model never sees in
//! that context (thinking, images, harness noise, sidechains) is dropped here.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ilium_agent_session::is_codex_injected_message;
use serde_json::{json, Map, Value};

use crate::error::CompactionError;
use crate::tokens::estimate_tokens;
use crate::transcript_io::{load_transcript, FileSnapshot, TailShape};
use crate::AgentKind;

/// Newest user messages a legacy Codex `compacted` record keeps, in estimated
/// tokens (`COMPACT_USER_MESSAGE_MAX_TOKENS` in Codex).
const CODEX_LEGACY_KEPT_USER_TOKENS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A genuine user prompt.
    User,
    Assistant,
    /// A user-shaped record that only carries tool results.
    ToolResults,
    /// The summary written by an earlier compaction, kept as an anchor.
    EarlierSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Part {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        text: String,
        is_error: bool,
    },
    /// A stand-in for something that cannot be sent, such as an image.
    Omitted(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Turn {
    pub(crate) role: Role,
    pub(crate) parts: Vec<Part>,
    /// Claude record uuids this turn was built from (empty for Codex).
    pub(crate) uuids: Vec<String>,
    pub(crate) timestamp: Option<String>,
    /// Claude assistant model, reused when the turn is re-emitted.
    pub(crate) model: Option<String>,
    pub(crate) message_id: Option<String>,
}

impl Turn {
    pub(crate) fn new(role: Role, parts: Vec<Part>) -> Self {
        Self {
            role,
            parts,
            uuids: Vec::new(),
            timestamp: None,
            model: None,
            message_id: None,
        }
    }

    pub(crate) fn text_parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
    }

    /// The turn's plain text, with parts joined by blank lines.
    pub(crate) fn text(&self) -> String {
        self.text_parts().collect::<Vec<_>>().join("\n\n")
    }

    pub(crate) fn estimated_tokens(&self) -> u64 {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(text) | Part::Omitted(text) => estimate_tokens(text),
                Part::ToolCall {
                    name, arguments, ..
                } => estimate_tokens(name) + estimate_tokens(&arguments.to_string()),
                Part::ToolResult { text, .. } => estimate_tokens(text),
            })
            .sum()
    }

    /// Tokens of the plain message text only, the part a Codex tail keeps.
    pub(crate) fn message_tokens(&self) -> u64 {
        self.text_parts().map(estimate_tokens).sum()
    }
}

/// What the Claude writer needs from the newest conversation records.
#[derive(Debug, Clone)]
pub(crate) struct ClaudeHead {
    pub(crate) leaf_uuid: String,
    pub(crate) session_id: Option<String>,
    /// `isSidechain,userType,entrypoint,cwd,version,gitBranch` of a recent record.
    pub(crate) template: Map<String, Value>,
    pub(crate) model: Option<String>,
}

/// What the Codex writer needs from the end of the rollout.
#[derive(Debug, Clone)]
pub(crate) struct CodexTail {
    pub(crate) last_ordinal: Option<u64>,
    pub(crate) has_ordinals: bool,
    /// The newest real `token_count` record, copied structurally.
    pub(crate) token_count_template: Option<Value>,
    pub(crate) context_window: Option<u64>,
}

pub(crate) struct ParsedTranscript {
    pub(crate) turns: Vec<Turn>,
    pub(crate) snapshot: FileSnapshot,
    pub(crate) shape: TailShape,
    pub(crate) warnings: Vec<String>,
    pub(crate) claude: Option<ClaudeHead>,
    pub(crate) codex: Option<CodexTail>,
}

pub(crate) fn parse_transcript(
    agent: AgentKind,
    path: &Path,
) -> Result<ParsedTranscript, CompactionError> {
    let loaded = load_transcript(path)?;
    let mut warnings = Vec::new();
    if loaded.shape.dropped_torn_fragment {
        warnings.push("the transcript ends in a torn line; it will be dropped".to_string());
    }
    let other_skipped = loaded
        .skipped_lines
        .saturating_sub(usize::from(loaded.shape.dropped_torn_fragment));
    if other_skipped > 0 {
        warnings.push(format!(
            "{other_skipped} unparseable or oversized transcript lines were ignored"
        ));
    }
    let mut parsed = ParsedTranscript {
        turns: Vec::new(),
        snapshot: loaded.snapshot,
        shape: loaded.shape,
        warnings,
        claude: None,
        codex: None,
    };
    match agent {
        AgentKind::Claude => {
            let (turns, head) = parse_claude(&loaded.values, path, &mut parsed.warnings)?;
            parsed.turns = turns;
            parsed.claude = Some(head);
        }
        AgentKind::Codex => {
            let (turns, tail) = parse_codex(&loaded.values, &mut parsed.warnings);
            parsed.turns = turns;
            parsed.codex = Some(tail);
        }
    }
    Ok(parsed)
}

fn image_placeholder(base64_chars: usize) -> String {
    let bytes = base64_chars / 4 * 3;
    ilium_prompts::render_value(
        "compaction/image-placeholder",
        &json!({ "bytes": bytes.to_string() }),
    )
    .trim()
    .to_string()
}

/// Flattens a tool result's `content` (a string, or text/image blocks).
fn flatten_result_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => {
            let mut pieces = Vec::new();
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            pieces.push(text.to_string());
                        }
                    }
                    Some("image") => {
                        let size = block
                            .pointer("/source/data")
                            .and_then(Value::as_str)
                            .map_or(0, str::len);
                        pieces.push(image_placeholder(size));
                    }
                    _ => {}
                }
            }
            pieces.join("\n")
        }
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

// ---------------------------------------------------------------- Claude

fn is_chained_type(kind: &str) -> bool {
    matches!(kind, "user" | "assistant" | "system" | "attachment")
}

fn string_field<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(Value::as_str)
}

fn is_compact_boundary(entry: &Value) -> bool {
    string_field(entry, "type") == Some("system")
        && string_field(entry, "subtype") == Some("compact_boundary")
}

/// Text Claude Code injects into user-shaped records that no human wrote.
fn is_claude_noise(text: &str) -> bool {
    let start = text.trim_start();
    is_codex_injected_message(text)
        || start.starts_with("<local-command-")
        || start.starts_with("<system-reminder>")
        || start.starts_with(
            "Caveat: The messages below were generated by the user while running local commands",
        )
}

fn parse_claude(
    values: &[Value],
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(Vec<Turn>, ClaudeHead), CompactionError> {
    let mut by_uuid: HashMap<&str, usize> = HashMap::new();
    let mut leaf: Option<usize> = None;
    for (index, entry) in values.iter().enumerate() {
        let (Some(kind), Some(uuid)) = (string_field(entry, "type"), string_field(entry, "uuid"))
        else {
            continue;
        };
        if !is_chained_type(kind) {
            continue;
        }
        by_uuid.insert(uuid, index);
        if entry.get("isSidechain").and_then(Value::as_bool) != Some(true) {
            leaf = Some(index);
        }
    }
    let Some(leaf) = leaf else {
        return Err(CompactionError::NoAnchorRecord {
            path: path.to_path_buf(),
            reason: "no user, assistant, system or attachment record carries a uuid".to_string(),
        });
    };

    // Newest to oldest along parentUuid, stopping at the last boundary.
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = Some(leaf);
    while let Some(index) = cursor {
        if !seen.insert(index) {
            warnings.push("the parentUuid chain contains a cycle; it was cut there".to_string());
            break;
        }
        chain.push(index);
        let entry = &values[index];
        if is_compact_boundary(entry) {
            break;
        }
        cursor = match string_field(entry, "parentUuid") {
            None => None,
            Some(parent) => match by_uuid.get(parent) {
                Some(found) => Some(*found),
                None => {
                    warnings.push(
                        "the parentUuid chain reaches a missing record (pruned history); \
                         the chain was cut there"
                            .to_string(),
                    );
                    None
                }
            },
        };
    }
    chain.reverse();
    splice_preserved_segment(values, &by_uuid, &mut chain, warnings);

    let turns = claude_turns(values, &chain);
    let head = claude_head(values, &chain, leaf);
    Ok((turns, head))
}

/// A boundary written by Claude Code itself keeps a recent tail physically
/// *before* the boundary; the loader relinks it after the summary. Mirror that
/// when every record of the segment is still present.
fn splice_preserved_segment(
    values: &[Value],
    by_uuid: &HashMap<&str, usize>,
    chain: &mut Vec<usize>,
    warnings: &mut Vec<String>,
) {
    let Some(&boundary_index) = chain.first() else {
        return;
    };
    let boundary = &values[boundary_index];
    if !is_compact_boundary(boundary) {
        return;
    }
    let Some(segment) = boundary.pointer("/compactMetadata/preservedSegment") else {
        return;
    };
    let (Some(head_uuid), Some(anchor_uuid), Some(tail_uuid)) = (
        string_field(segment, "headUuid"),
        string_field(segment, "anchorUuid"),
        string_field(segment, "tailUuid"),
    ) else {
        return;
    };
    let mut preserved = Vec::new();
    let mut cursor = by_uuid.get(tail_uuid).copied();
    let mut complete = false;
    let mut guard = HashSet::new();
    while let Some(index) = cursor {
        if !guard.insert(index) {
            break;
        }
        preserved.push(index);
        let uuid = string_field(&values[index], "uuid").unwrap_or_default();
        if uuid == head_uuid {
            complete = true;
            break;
        }
        cursor = string_field(&values[index], "parentUuid")
            .and_then(|parent| by_uuid.get(parent).copied());
    }
    let anchor_position = chain
        .iter()
        .position(|&index| string_field(&values[index], "uuid") == Some(anchor_uuid));
    match (complete, anchor_position) {
        (true, Some(position)) => {
            preserved.reverse();
            let insert_at = position + 1;
            for (offset, index) in preserved.into_iter().enumerate() {
                if !chain.contains(&index) {
                    chain.insert(insert_at + offset, index);
                }
            }
        }
        _ => warnings.push(
            "an earlier compaction's preserved tail could not be relinked; it is not part of \
             the summarized conversation"
                .to_string(),
        ),
    }
}

fn claude_head(values: &[Value], chain: &[usize], leaf: usize) -> ClaudeHead {
    const TEMPLATE_KEYS: [&str; 6] = [
        "isSidechain",
        "userType",
        "entrypoint",
        "cwd",
        "version",
        "gitBranch",
    ];
    let template_source = chain
        .iter()
        .rev()
        .map(|&index| &values[index])
        .find(|entry| {
            matches!(string_field(entry, "type"), Some("user" | "assistant"))
                && string_field(entry, "cwd").is_some()
                && string_field(entry, "sessionId").is_some()
        })
        .unwrap_or(&values[leaf]);
    let mut template = Map::new();
    for key in TEMPLATE_KEYS {
        if let Some(value) = template_source.get(key) {
            template.insert(key.to_string(), value.clone());
        }
    }
    let model = chain
        .iter()
        .rev()
        .filter_map(|&index| {
            values[index]
                .pointer("/message/model")
                .and_then(Value::as_str)
        })
        .find(|model| *model != "<synthetic>")
        .map(str::to_string);
    ClaudeHead {
        leaf_uuid: string_field(&values[leaf], "uuid")
            .unwrap_or_default()
            .to_string(),
        session_id: string_field(template_source, "sessionId").map(str::to_string),
        template,
        model,
    }
}

fn claude_turns(values: &[Value], chain: &[usize]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for &index in chain {
        let entry = &values[index];
        if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let uuid = string_field(entry, "uuid").map(str::to_string);
        let timestamp = string_field(entry, "timestamp").map(str::to_string);
        match string_field(entry, "type") {
            Some("user") => {
                for (role, parts) in claude_user_turns(entry) {
                    push_turn(
                        &mut turns,
                        role,
                        parts,
                        uuid.clone(),
                        timestamp.clone(),
                        None,
                        None,
                    );
                }
            }
            Some("assistant") => {
                let Some((parts, model, message_id)) = claude_assistant_parts(entry) else {
                    continue;
                };
                push_turn(
                    &mut turns,
                    Role::Assistant,
                    parts,
                    uuid,
                    timestamp,
                    model,
                    message_id,
                );
            }
            _ => {}
        }
    }
    turns
}

/// Appends a turn, merging consecutive tool-result records and consecutive
/// assistant records of one API message (Claude splits one block per record).
fn push_turn(
    turns: &mut Vec<Turn>,
    role: Role,
    parts: Vec<Part>,
    uuid: Option<String>,
    timestamp: Option<String>,
    model: Option<String>,
    message_id: Option<String>,
) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = turns.last_mut() {
        let mergeable = match role {
            Role::ToolResults => last.role == Role::ToolResults,
            Role::Assistant => {
                last.role == Role::Assistant
                    && message_id.is_some()
                    && last.message_id == message_id
            }
            _ => false,
        };
        if mergeable {
            last.parts.extend(parts);
            last.uuids.extend(uuid);
            return;
        }
    }
    let mut turn = Turn::new(role, parts);
    turn.uuids.extend(uuid);
    turn.timestamp = timestamp;
    turn.model = model;
    turn.message_id = message_id;
    turns.push(turn);
}

fn claude_user_turns(entry: &Value) -> Vec<(Role, Vec<Part>)> {
    let content = entry.pointer("/message/content");
    if entry.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
        let text = flatten_result_content(content);
        return vec![(
            Role::EarlierSummary,
            vec![Part::Text(strip_claude_summary_wrapper(&text))],
        )];
    }
    if entry.get("isMeta").and_then(Value::as_bool) == Some(true)
        || string_field(entry, "promptSource") == Some("system")
    {
        return Vec::new();
    }
    let human = entry
        .pointer("/origin/kind")
        .and_then(Value::as_str)
        .is_none_or(|kind| kind == "human");
    let mut turns = Vec::new();
    match content {
        Some(Value::String(text)) => {
            if human && !text.trim().is_empty() && !is_claude_noise(text) {
                turns.push((Role::User, vec![Part::Text(text.clone())]));
            }
        }
        Some(Value::Array(blocks)) => {
            let mut results = Vec::new();
            let mut prompt = Vec::new();
            let mut text_pieces: Vec<String> = Vec::new();
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("tool_result") => {
                        let call_id = string_field(block, "tool_use_id").unwrap_or_default();
                        results.push(Part::ToolResult {
                            call_id: call_id.to_string(),
                            text: flatten_result_content(block.get("content")),
                            is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
                        });
                    }
                    Some("text") => {
                        if let Some(text) = string_field(block, "text") {
                            if !text.trim().is_empty() && !is_claude_noise(text) {
                                text_pieces.push(text.to_string());
                            }
                        }
                    }
                    Some("image") => {
                        let size = block
                            .pointer("/source/data")
                            .and_then(Value::as_str)
                            .map_or(0, str::len);
                        prompt.push(Part::Omitted(image_placeholder(size)));
                    }
                    _ => {}
                }
            }
            if !results.is_empty() {
                turns.push((Role::ToolResults, results));
            }
            if human && !text_pieces.is_empty() {
                let mut parts = vec![Part::Text(text_pieces.join("\n"))];
                parts.extend(prompt);
                turns.push((Role::User, parts));
            }
        }
        _ => {}
    }
    turns
}

/// Claude's summary message wraps the summary in boilerplate; the anchor only
/// needs the summary itself.
fn strip_claude_summary_wrapper(text: &str) -> String {
    let body = if text.starts_with("This session is being continued") {
        text.split_once("Summary:\n")
            .map_or(text, |(_, summary)| summary)
    } else {
        text
    };
    let end = [
        "\n\nIf you need specific details from before compaction",
        "\n\nRecent messages are preserved verbatim.",
        "\n\nContinue the conversation from where it left off",
    ]
    .iter()
    .filter_map(|marker| body.find(marker))
    .min()
    .unwrap_or(body.len());
    body[..end].trim().to_string()
}

type AssistantParts = (Vec<Part>, Option<String>, Option<String>);

fn claude_assistant_parts(entry: &Value) -> Option<AssistantParts> {
    let message = entry.get("message")?;
    let model = string_field(message, "model");
    if model == Some("<synthetic>")
        || entry.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let mut parts = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) => {
            if !text.trim().is_empty() {
                parts.push(Part::Text(text.clone()));
            }
        }
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = string_field(block, "text") {
                            if !text.trim().is_empty() {
                                parts.push(Part::Text(text.to_string()));
                            }
                        }
                    }
                    Some("tool_use") => {
                        if let Some(id) = string_field(block, "id") {
                            parts.push(Part::ToolCall {
                                id: id.to_string(),
                                name: string_field(block, "name").unwrap_or("tool").to_string(),
                                arguments: block.get("input").cloned().unwrap_or_else(|| json!({})),
                            });
                        }
                    }
                    // thinking, redacted_thinking, server tool blocks: never part of the summary input.
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Some((
        parts,
        model.map(str::to_string),
        string_field(message, "id").map(str::to_string),
    ))
}

// ---------------------------------------------------------------- Codex

fn codex_summary_prefix() -> &'static str {
    ilium_prompts::compaction::CODEX_SUMMARY_PREFIX.trim()
}

/// Harness-injected user messages Codex records beside real prompts.
fn is_codex_harness_text(text: &str) -> bool {
    const PREFIXES: [&str; 11] = [
        "<user_instructions>",
        "<permissions instructions>",
        "<permissions_instructions>",
        "<turn_aborted>",
        "<collaboration_mode>",
        "<skills_instructions>",
        "<plugins_instructions>",
        "<apps_instructions>",
        "<model_switch>",
        "<subagent_notification>",
        "<codex_internal_context",
    ];
    let start = text.trim_start();
    is_codex_injected_message(text) || PREFIXES.iter().any(|prefix| start.starts_with(prefix))
}

fn codex_message_texts(payload: &Value) -> Vec<String> {
    match payload.get("content") {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("input_text" | "output_text" | "text")
                )
            })
            .filter_map(|block| string_field(block, "text").map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Plain text shape of a Codex `function_call_output.output`.
fn flatten_codex_output(output: &Value) -> String {
    match output {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Value::String(text) => Some(text.as_str()),
                other => other.get("text").and_then(Value::as_str),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => ["content", "output"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| output.to_string()),
        other => other.to_string(),
    }
}

struct CodexReplay {
    turns: Vec<Turn>,
    warnings_pending: Vec<String>,
}

impl CodexReplay {
    fn push_user(&mut self, text: String, timestamp: Option<String>) {
        let mut turn = Turn::new(Role::User, vec![Part::Text(text)]);
        turn.timestamp = timestamp;
        self.turns.push(turn);
    }

    /// Adds assistant parts, continuing the open assistant turn when the
    /// previous turn is one (tool calls follow the message that announced them).
    fn push_assistant(&mut self, part: Part, timestamp: Option<String>) {
        if let Some(last) = self.turns.last_mut() {
            if last.role == Role::Assistant {
                last.parts.push(part);
                return;
            }
        }
        let mut turn = Turn::new(Role::Assistant, vec![part]);
        turn.timestamp = timestamp;
        self.turns.push(turn);
    }

    fn push_result(&mut self, part: Part, timestamp: Option<String>) {
        if let Some(last) = self.turns.last_mut() {
            if last.role == Role::ToolResults {
                last.parts.push(part);
                return;
            }
        }
        let mut turn = Turn::new(Role::ToolResults, vec![part]);
        turn.timestamp = timestamp;
        self.turns.push(turn);
    }

    fn push_summary(&mut self, text: String) {
        self.turns
            .push(Turn::new(Role::EarlierSummary, vec![Part::Text(text)]));
    }

    fn apply_item(&mut self, payload: &Value, timestamp: Option<String>) {
        match string_field(payload, "type") {
            Some("message") => self.apply_message(payload, timestamp),
            Some("function_call") | Some("custom_tool_call") => {
                let (Some(id), Some(name)) = (
                    string_field(payload, "call_id"),
                    string_field(payload, "name"),
                ) else {
                    return;
                };
                let arguments = match (
                    string_field(payload, "arguments"),
                    string_field(payload, "input"),
                ) {
                    (Some(text), _) => serde_json::from_str(text)
                        .unwrap_or_else(|_| Value::String(text.to_string())),
                    (None, Some(text)) => Value::String(text.to_string()),
                    (None, None) => json!({}),
                };
                self.push_assistant(
                    Part::ToolCall {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments,
                    },
                    timestamp,
                );
            }
            Some("local_shell_call") => {
                let id = string_field(payload, "call_id").or_else(|| string_field(payload, "id"));
                let (Some(id), Some(command)) = (id, payload.pointer("/action/command")) else {
                    return;
                };
                self.push_assistant(
                    Part::ToolCall {
                        id: id.to_string(),
                        name: "shell".to_string(),
                        arguments: json!({ "command": command }),
                    },
                    timestamp,
                );
            }
            Some("function_call_output")
            | Some("custom_tool_call_output")
            | Some("local_shell_call_output") => {
                let Some(call_id) = string_field(payload, "call_id") else {
                    return;
                };
                let output = payload.get("output").unwrap_or(&Value::Null);
                let is_error = output.get("success").and_then(Value::as_bool) == Some(false);
                self.push_result(
                    Part::ToolResult {
                        call_id: call_id.to_string(),
                        text: flatten_codex_output(output),
                        is_error,
                    },
                    timestamp,
                );
            }
            Some("compaction") => {
                self.warnings_pending.push(
                    "an earlier compaction by Codex is encrypted and unreadable; only the \
                     messages it retained are summarized"
                        .to_string(),
                );
                self.push_summary(
                    ilium_prompts::compaction::OPAQUE_COMPACTION_NOTE
                        .trim()
                        .to_string(),
                );
            }
            // reasoning (encrypted), agent_message, ghost_snapshot, web_search_call, ...
            _ => {}
        }
    }

    fn apply_message(&mut self, payload: &Value, timestamp: Option<String>) {
        let role = string_field(payload, "role").unwrap_or_default();
        let texts = codex_message_texts(payload);
        match role {
            "user" => {
                let prefix = codex_summary_prefix();
                let mut kept = Vec::new();
                for text in texts {
                    if text.trim().is_empty() {
                        continue;
                    }
                    if let Some(summary) = text.trim_start().strip_prefix(prefix) {
                        self.push_summary(summary.trim().to_string());
                    } else if !is_codex_harness_text(&text) {
                        kept.push(text);
                    }
                }
                if !kept.is_empty() {
                    self.push_user(kept.join("\n"), timestamp);
                }
            }
            "assistant" => {
                let text = texts
                    .into_iter()
                    .filter(|text| !text.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    self.push_assistant(Part::Text(text), timestamp);
                }
            }
            // developer and system messages are harness instructions.
            _ => {}
        }
    }

    /// Applies a `compacted` record the way Codex's own resume does.
    fn apply_compacted(&mut self, payload: &Value) {
        if let Some(Value::Array(history)) = payload.get("replacement_history") {
            self.turns.clear();
            for item in history {
                self.apply_item(item, None);
            }
            return;
        }
        let message = string_field(payload, "message").unwrap_or_default();
        if message.trim().is_empty() {
            return;
        }
        // Legacy shape: the newest user messages (bounded) plus the summary.
        let mut budget = CODEX_LEGACY_KEPT_USER_TOKENS;
        let mut kept: Vec<Turn> = Vec::new();
        for turn in self
            .turns
            .iter()
            .rev()
            .filter(|turn| turn.role == Role::User)
        {
            let cost = turn.estimated_tokens();
            if cost > budget && !kept.is_empty() {
                break;
            }
            budget = budget.saturating_sub(cost);
            kept.push(turn.clone());
        }
        kept.reverse();
        self.turns = kept;
        let summary = message
            .trim_start()
            .strip_prefix(codex_summary_prefix())
            .unwrap_or(message);
        self.push_summary(summary.trim().to_string());
    }

    /// `thread_rolled_back`: Codex drops the newest `count` user turns and
    /// everything after the first of them.
    fn roll_back(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let mut remaining = count;
        let mut cut = None;
        for (index, turn) in self.turns.iter().enumerate().rev() {
            if turn.role == Role::User {
                remaining -= 1;
                cut = Some(index);
                if remaining == 0 {
                    break;
                }
            }
        }
        if let Some(index) = cut {
            self.turns.truncate(index);
        }
    }
}

fn parse_codex(values: &[Value], warnings: &mut Vec<String>) -> (Vec<Turn>, CodexTail) {
    let mut replay = CodexReplay {
        turns: Vec::new(),
        warnings_pending: Vec::new(),
    };
    let mut tail = CodexTail {
        last_ordinal: None,
        has_ordinals: false,
        token_count_template: None,
        context_window: None,
    };
    for entry in values {
        if let Some(ordinal) = entry.get("ordinal").and_then(Value::as_u64) {
            tail.has_ordinals = true;
            tail.last_ordinal = Some(ordinal);
        } else {
            tail.last_ordinal = None;
        }
        let timestamp = string_field(entry, "timestamp").map(str::to_string);
        let payload = entry.get("payload").unwrap_or(&Value::Null);
        match string_field(entry, "type") {
            Some("response_item") => replay.apply_item(payload, timestamp),
            Some("compacted") => replay.apply_compacted(payload),
            Some("event_msg") => match string_field(payload, "type") {
                Some("thread_rolled_back") => {
                    let count = payload
                        .get("num_turns")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    replay.roll_back(count);
                }
                Some("token_count") => {
                    if let Some(info) = payload.get("info").filter(|info| info.is_object()) {
                        tail.token_count_template = Some(entry.clone());
                        if let Some(window) =
                            info.get("model_context_window").and_then(Value::as_u64)
                        {
                            tail.context_window = Some(window);
                        }
                    }
                }
                Some("task_started") => {
                    if let Some(window) =
                        payload.get("model_context_window").and_then(Value::as_u64)
                    {
                        tail.context_window.get_or_insert(window);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    for warning in replay.warnings_pending {
        if seen.insert(warning.clone()) {
            warnings.push(warning);
        }
    }
    (replay.turns, tail)
}

#[cfg(test)]
mod tests;
