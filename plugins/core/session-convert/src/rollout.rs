//! Parsing of a Codex rollout (JSON Lines) into conversation items. Moved from `ilium-session-convert`'s
//! `codex_rollout`: the classification of every line is unchanged, the file streaming is not (the host reads the
//! file and hands the text over, see [`parse_rollout_text`]).

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::lenient_json::{LenientObject, LenientSeq, LenientString};
use crate::MAX_TOOL_RESULT_CHARS;

/// The only top-level record kinds that can hold conversation.
const KEPT_RECORD_KINDS: [&str; 2] = ["session_meta", "response_item"];

/// User-role text blocks that are harness context, not something the person
/// typed. Matched against the block text after leading whitespace.
const HARNESS_BLOCK_PREFIXES: [&str; 6] = [
    "# AGENTS.md instructions",
    "<environment_context>",
    "<user_instructions>",
    "<permissions instructions>",
    "<permissions_instructions>",
    "<codex_internal_context",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RolloutMeta {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
}

/// How a tool call carried its arguments in the rollout.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolArguments {
    /// `function_call.arguments`: a JSON document in a string.
    JsonText(String),
    /// `custom_tool_call.input`: free-form text (a patch, a script).
    FreeText(String),
    /// Already structured (synthesised from `local_shell_call`).
    Structured(Value),
}

/// A tool output flattened to text and cut at [`crate::MAX_TOOL_RESULT_CHARS`]
/// while it was read, with the facts about the complete text the writer
/// needs to render the result exactly as if it had seen all of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundedToolOutput {
    /// The first [`crate::MAX_TOOL_RESULT_CHARS`] characters of the output.
    pub text: String,
    /// Character count of the complete flattened output.
    pub original_chars: usize,
    /// True when the complete output was longer than `text`.
    pub truncated: bool,
    /// False when the complete output is empty or whitespace only.
    pub has_visible_text: bool,
}

impl BoundedToolOutput {
    /// True when the complete output holds nothing but whitespace.
    pub fn is_blank(&self) -> bool {
        !self.has_visible_text
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceItem {
    UserText {
        text: String,
        timestamp: Option<String>,
    },
    AssistantText {
        text: String,
        timestamp: Option<String>,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: ToolArguments,
        timestamp: Option<String>,
    },
    ToolOutput {
        call_id: String,
        output: BoundedToolOutput,
        is_error: bool,
    },
}

/// What a scan learned about the lines it did not turn into items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RolloutCounts {
    /// Lines that held JSON (everything except blank/unreadable lines).
    pub json_lines: usize,
    /// Unparseable lines, including the oversized ones.
    pub malformed_lines: usize,
    /// Lines over the line cap, skipped unread (a subset of `malformed_lines`).
    pub oversized_lines: usize,
    /// Lines parsed but intentionally not converted, by coarse kind.
    pub dropped_by_kind: BTreeMap<String, usize>,
}

impl RolloutCounts {
    pub fn dropped_total(&self) -> usize {
        self.dropped_by_kind.values().sum::<usize>() + self.malformed_lines
    }

    fn drop_kind(&mut self, kind: impl Into<String>) {
        *self.dropped_by_kind.entry(kind.into()).or_insert(0) += 1;
    }
}

/// A whole rollout parsed from text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRollout {
    pub meta: RolloutMeta,
    pub items: Vec<SourceItem>,
    /// Lines that held JSON (everything except blank/unparseable lines).
    pub json_lines: usize,
    pub malformed_lines: usize,
    /// Lines parsed but intentionally not converted, by coarse kind.
    pub dropped_by_kind: BTreeMap<String, usize>,
}

impl ParsedRollout {
    pub fn dropped_total(&self) -> usize {
        self.dropped_by_kind.values().sum::<usize>() + self.malformed_lines
    }
}

/// Parses a whole rollout. Only the first `session_meta` owns the rollout's identity.
pub fn parse_rollout_text(text: &str) -> ParsedRollout {
    match parse_rollout_text_checked(text, &mut |_, _| Ok::<(), std::convert::Infallible>(())) {
        Ok(parsed) => parsed,
        Err(never) => match never {},
    }
}

/// Parses a whole rollout like [`parse_rollout_text`], and calls `before_line(index, total)` before each
/// physical line (blank lines included; `index` counts from 0, `total` is the number of lines). The first
/// error `before_line` returns stops the parse and is returned, which is how a job reports progress and
/// honours a cancel request between lines.
pub fn parse_rollout_text_checked<E>(
    text: &str,
    before_line: &mut dyn FnMut(usize, usize) -> Result<(), E>,
) -> Result<ParsedRollout, E> {
    let mut meta = RolloutMeta::default();
    let mut items = Vec::new();
    let mut counts = RolloutCounts::default();
    let total = text.lines().count();
    for (index, line) in text.lines().enumerate() {
        before_line(index, total)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match classify_line(trimmed) {
            LineOutcome::Malformed => counts.malformed_lines += 1,
            LineOutcome::Dropped(kind) => {
                counts.json_lines += 1;
                counts.drop_kind(kind);
            }
            LineOutcome::Meta(found) => {
                counts.json_lines += 1;
                if meta.session_id.is_some() {
                    counts.drop_kind("session_meta");
                } else {
                    meta = found;
                }
            }
            LineOutcome::Item(item) => {
                counts.json_lines += 1;
                items.push(item);
            }
        }
    }
    Ok(ParsedRollout {
        meta,
        items,
        json_lines: counts.json_lines,
        malformed_lines: counts.malformed_lines,
        dropped_by_kind: counts.dropped_by_kind,
    })
}

/// What one non-blank line means, independent of scan state.
#[derive(Debug, PartialEq)]
pub enum LineOutcome {
    Meta(RolloutMeta),
    Item(SourceItem),
    /// Valid JSON that carries no conversation, by coarse kind.
    Dropped(String),
    Malformed,
}

/// Classifies one trimmed line. Dropped kinds are recognised from the raw
/// text and only syntax-checked (a truncated final line of a live rollout
/// must still count as unreadable); kept kinds and undecidable layouts go
/// through the typed parse.
pub fn classify_line(trimmed: &str) -> LineOutcome {
    let sniffed = sniff_record_kind(trimmed);
    if let Some(kind) = sniffed.filter(|kind| !KEPT_RECORD_KINDS.contains(kind)) {
        return if is_syntactically_valid_json(trimmed) {
            LineOutcome::Dropped(kind.to_string())
        } else {
            LineOutcome::Malformed
        };
    }
    match serde_json::from_str::<RolloutRecord>(trimmed) {
        Ok(record) => absorb_record(record),
        // Valid JSON the typed record cannot hold: a non-object (previously
        // dropped under the empty kind) or, in theory, duplicate keys.
        Err(_) if is_syntactically_valid_json(trimmed) => {
            LineOutcome::Dropped(sniffed.unwrap_or("").to_string())
        }
        Err(_) => LineOutcome::Malformed,
    }
}

/// Validates JSON syntax without building anything.
fn is_syntactically_valid_json(text: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

/// Reads the value of a top-level `"type"` key straight from the text of a
/// JSON object, visiting only the keys and scalar values before it. Returns
/// `None` whenever the layout is not that simple (a nested object or array
/// before the key, an escaped value, no such key, not an object): the typed
/// parse then decides, so behaviour never depends on this shortcut.
fn sniff_record_kind(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut index = skip_json_whitespace(bytes, 0);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    loop {
        index = skip_json_whitespace(bytes, index);
        if bytes.get(index) != Some(&b'"') {
            return None;
        }
        let (key, after_key) = raw_json_string(bytes, index + 1)?;
        index = skip_json_whitespace(bytes, after_key);
        if bytes.get(index) != Some(&b':') {
            return None;
        }
        index = skip_json_whitespace(bytes, index + 1);
        if key == b"type" {
            if bytes.get(index) != Some(&b'"') {
                return None;
            }
            let (value, _) = raw_json_string(bytes, index + 1)?;
            if value.contains(&b'\\') {
                return None;
            }
            // Quotes and backslashes are ASCII, so the slice ends on a
            // character boundary of the UTF-8 line.
            let start = index + 1;
            return line.get(start..start + value.len());
        }
        match bytes.get(index) {
            Some(b'"') => {
                let (_, after_value) = raw_json_string(bytes, index + 1)?;
                index = after_value;
            }
            Some(b'{' | b'[') | None => return None,
            Some(_) => {
                while bytes
                    .get(index)
                    .is_some_and(|byte| *byte != b',' && *byte != b'}')
                {
                    index += 1;
                }
            }
        }
        index = skip_json_whitespace(bytes, index);
        if bytes.get(index) != Some(&b',') {
            return None;
        }
        index += 1;
    }
}

fn skip_json_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        index += 1;
    }
    index
}

/// The raw (still escaped) contents of a JSON string whose opening quote
/// precedes `start`, and the index after its closing quote.
fn raw_json_string(bytes: &[u8], start: usize) -> Option<(&[u8], usize)> {
    let mut index = start;
    loop {
        match bytes.get(index)? {
            b'"' => return Some((&bytes[start..index], index + 1)),
            b'\\' => index += 2,
            _ => index += 1,
        }
    }
}

/// One rollout line. Every field is lenient so a field of an unexpected
/// shape reads as absent, exactly as the former `Value` lookups did.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RolloutRecord {
    timestamp: LenientString,
    #[serde(rename = "type")]
    kind: LenientString,
    payload: LenientObject<RecordPayload>,
}

/// The union of the `session_meta` and `response_item` payload fields.
/// Unknown fields (images, `encrypted_content`, `summary`, ...) are skipped.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RecordPayload {
    // session_meta
    id: LenientString,
    cwd: LenientString,
    git: LenientObject<GitPayload>,
    // response_item
    #[serde(rename = "type")]
    kind: LenientString,
    role: LenientString,
    content: LenientSeq<LenientObject<ContentBlock>>,
    call_id: LenientString,
    name: LenientString,
    arguments: LenientString,
    input: LenientString,
    action: LenientObject<ShellAction>,
    /// Kept as a tree because outputs come in several shapes; this is the
    /// one large allocation per kept line, bounded by the line cap.
    output: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GitPayload {
    branch: LenientString,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: LenientString,
    text: LenientString,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ShellAction {
    command: Option<Value>,
    working_directory: Option<Value>,
}

fn absorb_record(record: RolloutRecord) -> LineOutcome {
    let payload = record.payload.0;
    match record.kind.as_deref().unwrap_or("") {
        "session_meta" => LineOutcome::Meta(RolloutMeta {
            session_id: payload.id.0,
            cwd: payload.cwd.0,
            git_branch: payload.git.0.branch.0.filter(|branch| !branch.is_empty()),
        }),
        "response_item" => absorb_response_item(payload, record.timestamp.0),
        // `event_msg`, `turn_context`, `compacted`, `world_state`,
        // `token_usage_record`, ... carry no conversation of their own.
        other => LineOutcome::Dropped(other.to_string()),
    }
}

fn absorb_response_item(payload: RecordPayload, timestamp: Option<String>) -> LineOutcome {
    match payload.kind.as_deref().unwrap_or("") {
        "message" => absorb_message(payload, timestamp),
        "function_call" | "custom_tool_call" => absorb_tool_call(payload, timestamp),
        "local_shell_call" => absorb_local_shell_call(payload, timestamp),
        "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
            absorb_tool_output(payload)
        }
        // reasoning (encrypted), agent_message, ghost_snapshot, web_search_call, ...
        other => LineOutcome::Dropped(format!("response_item:{other}")),
    }
}

fn absorb_message(payload: RecordPayload, timestamp: Option<String>) -> LineOutcome {
    let role = payload.role.as_deref().unwrap_or("");
    let is_user = role == "user";
    if !is_user && role != "assistant" {
        // developer/system messages are harness instructions.
        return LineOutcome::Dropped(format!("message:{role}"));
    }
    let mut kept = Vec::new();
    for block in payload.content.0 {
        let block = block.0;
        let is_text = matches!(
            block.kind.as_deref(),
            Some("input_text" | "output_text" | "text")
        );
        let Some(text) = block.text.0.filter(|_| is_text) else {
            // encrypted_content, input_image, ...
            continue;
        };
        if is_user && is_harness_block(&text) {
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }
        kept.push(text);
    }
    if kept.is_empty() {
        return LineOutcome::Dropped(
            if is_user {
                "message:user (harness context)"
            } else {
                "message:assistant (no text)"
            }
            .to_string(),
        );
    }
    let text = kept.join("\n\n");
    LineOutcome::Item(if is_user {
        SourceItem::UserText { text, timestamp }
    } else {
        SourceItem::AssistantText { text, timestamp }
    })
}

fn is_harness_block(text: &str) -> bool {
    let start = text.trim_start();
    HARNESS_BLOCK_PREFIXES
        .iter()
        .any(|prefix| start.starts_with(prefix))
}

fn absorb_tool_call(payload: RecordPayload, timestamp: Option<String>) -> LineOutcome {
    let (Some(call_id), Some(name)) = (payload.call_id.0, payload.name.0) else {
        return LineOutcome::Dropped("tool_call (missing call_id or name)".to_string());
    };
    let arguments = match (payload.arguments.0, payload.input.0) {
        (Some(json_text), _) => ToolArguments::JsonText(json_text),
        (None, Some(free_text)) => ToolArguments::FreeText(free_text),
        (None, None) => ToolArguments::JsonText(String::new()),
    };
    LineOutcome::Item(SourceItem::ToolCall {
        call_id,
        name,
        arguments,
        timestamp,
    })
}

/// `local_shell_call` (an older built-in shell tool) keeps its argv under
/// `action.command`; it is presented as an ordinary `shell` call.
fn absorb_local_shell_call(payload: RecordPayload, timestamp: Option<String>) -> LineOutcome {
    let call_id = payload.call_id.0.or(payload.id.0);
    let action = payload.action.0;
    let (Some(call_id), Some(command)) = (call_id, action.command) else {
        return LineOutcome::Dropped("local_shell_call (missing call_id or command)".to_string());
    };
    let mut arguments = serde_json::Map::new();
    arguments.insert("command".to_string(), command);
    if let Some(workdir) = action.working_directory {
        arguments.insert("workdir".to_string(), workdir);
    }
    LineOutcome::Item(SourceItem::ToolCall {
        call_id,
        name: "shell".to_string(),
        arguments: ToolArguments::Structured(Value::Object(arguments)),
        timestamp,
    })
}

fn absorb_tool_output(payload: RecordPayload) -> LineOutcome {
    let Some(call_id) = payload.call_id.0 else {
        return LineOutcome::Dropped("tool_output (missing call_id)".to_string());
    };
    let is_error = payload
        .output
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success);
    LineOutcome::Item(SourceItem::ToolOutput {
        call_id,
        output: flatten_output(&payload.output),
        is_error,
    })
}

/// Turns the many `output` shapes into plain text: a string, an array of
/// `{type, text}` blocks, or a `{content|output: string}` object. The text is
/// cut at [`crate::MAX_TOOL_RESULT_CHARS`] as it is assembled; the facts
/// about the complete text are recorded in the result.
pub fn flatten_output(output: &Value) -> BoundedToolOutput {
    let mut builder = BoundedTextBuilder::new(crate::MAX_TOOL_RESULT_CHARS);
    match output {
        Value::Null => {}
        Value::String(text) => builder.push_text(text),
        Value::Array(blocks) => {
            for block in blocks {
                let piece = match block {
                    Value::String(text) => Some(text.as_str()),
                    other => other.get("text").and_then(Value::as_str),
                };
                if let Some(piece) = piece {
                    builder.push_block(piece);
                }
            }
        }
        Value::Object(map) => {
            match ["content", "output"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_str))
            {
                Some(text) => builder.push_text(text),
                // `Display` streams the compact JSON through the builder,
                // so even this fallback never materialises the whole text.
                None => builder.push_display(output),
            }
        }
        other => builder.push_display(other),
    }
    builder.finish()
}

/// Accumulates text up to a character cap while tracking, over the complete
/// text, its length, whether it has visible characters and its last
/// character (needed for the block join rule).
struct BoundedTextBuilder {
    cap: usize,
    output: BoundedToolOutput,
    last_char: Option<char>,
}

impl BoundedTextBuilder {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            output: BoundedToolOutput::default(),
            last_char: None,
        }
    }

    /// Appends one block of an output array: blocks are separated by a line
    /// break unless the text so far already ends with one (an empty block
    /// still adds the separator, as the joined text always did).
    fn push_block(&mut self, piece: &str) {
        if self.output.original_chars > 0 && self.last_char != Some('\n') {
            self.push_text("\n");
        }
        self.push_text(piece);
    }

    /// Appends text that continues the current piece.
    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let room = self.cap.saturating_sub(self.output.original_chars);
        let mut kept_chars = 0usize;
        let mut kept_end = text.len();
        for (index, _) in text.char_indices() {
            if kept_chars == room {
                kept_end = index;
                break;
            }
            kept_chars += 1;
        }
        self.output.text.push_str(&text[..kept_end]);
        let rest = &text[kept_end..];
        self.output.truncated |= !rest.is_empty();
        self.output.original_chars += kept_chars + rest.chars().count();
        self.output.has_visible_text |= text.chars().any(|character| !character.is_whitespace());
        self.last_char = text.chars().next_back();
    }

    fn push_display(&mut self, value: &impl std::fmt::Display) {
        // `fmt::Write` for this builder cannot fail.
        let _ = write!(self, "{value}");
    }

    fn finish(self) -> BoundedToolOutput {
        self.output
    }
}

impl std::fmt::Write for BoundedTextBuilder {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.push_text(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_text(content: &str) -> ParsedRollout {
        parse_rollout_text(content)
    }

    fn parse_lines(lines: &[Value]) -> ParsedRollout {
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        parse_text(&text.join("\n"))
    }

    /// REQ: PORT:ilium-session-convert/src/codex_rollout.rs::harness_blocks_are_dropped_per_block_not_per_message
    #[test]
    fn harness_blocks_are_dropped_per_block_not_per_message() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "# AGENTS.md instructions for /x\n..."},
                {"type": "input_text", "text": "<environment_context>\n</environment_context>"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"},
                {"type": "input_text", "text": "real prompt"}
            ]}
        })]);
        assert_eq!(
            parsed.items,
            vec![SourceItem::UserText {
                text: "real prompt".to_string(),
                timestamp: None
            }]
        );
    }

    /// REQ: PORT:ilium-session-convert/src/codex_rollout.rs::message_with_only_harness_blocks_is_dropped
    #[test]
    fn message_with_only_harness_blocks_is_dropped() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "<codex_internal_context source=\"goal\">x"}
            ]}
        })]);
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.dropped_total(), 1);
    }

    /// REQ: PORT:ilium-session-convert/src/codex_rollout.rs::output_arrays_are_joined_with_line_breaks
    #[test]
    fn output_arrays_are_joined_with_line_breaks() {
        let output = json!([
            {"type": "input_text", "text": "Script completed\n"},
            {"type": "input_text", "text": "{\"exit_code\":0}"},
            {"type": "input_text", "text": ""},
            7,
            "plain"
        ]);
        let flattened = flatten_output(&output);
        // The empty block adds the separator; "plain" then follows a '\n'.
        assert_eq!(flattened.text, "Script completed\n{\"exit_code\":0}\nplain");
        assert!(!flattened.truncated && flattened.has_visible_text);
        assert_eq!(flattened.original_chars, flattened.text.chars().count());
    }

    /// REQ: PORT:ilium-session-convert/src/codex_rollout.rs::local_shell_call_becomes_a_shell_call
    #[test]
    fn local_shell_call_becomes_a_shell_call() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "local_shell_call", "call_id": "c1",
                "action": {"type": "exec", "command": ["ls", "-la"], "working_directory": "/w"}}
        })]);
        assert_eq!(
            parsed.items,
            vec![SourceItem::ToolCall {
                call_id: "c1".to_string(),
                name: "shell".to_string(),
                arguments: ToolArguments::Structured(
                    json!({"command": ["ls", "-la"], "workdir": "/w"})
                ),
                timestamp: None,
            }]
        );
    }
}
