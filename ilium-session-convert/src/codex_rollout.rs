//! Reads a Codex rollout (`rollout-*-<id>.jsonl`) one line at a time into the
//! handful of conversation items worth carrying to Claude Code, counting
//! everything else as dropped.
//!
//! Memory stays proportional to the longest single line, never to the file:
//! the raw bytes of a line are sniffed for their record kind first, so the
//! ~70% of a rollout that is dropped (`event_msg`, `compacted`,
//! `world_state`, token usage, ...) is never parsed into a tree; kept lines
//! are deserialized into small typed structs that skip images and encrypted
//! reasoning without allocating them; tool outputs are cut at
//! [`crate::MAX_TOOL_RESULT_CHARS`] while being flattened; and lines over
//! [`crate::MAX_LINE_BYTES`] are skipped unread.
//!
//! Two rollout generations are handled. Older files use
//! `function_call shell_command` and `custom_tool_call apply_patch`; current
//! ones use `custom_tool_call exec` (a JavaScript snippet) plus structured
//! `custom_tool_call_output` arrays. Both carry `response_item` payloads, which
//! are the single source of truth: `event_msg` records duplicate them (the
//! user message in particular) and are never converted.
//!
//! [`RolloutScanner`] streams the kept items with the byte offset of their
//! line; [`RolloutLineAccess`] re-reads one such line by offset, which lets a
//! writer pair a tool call with an output that sits anywhere later in the
//! file without holding the outputs in memory. One line yields at most one
//! item, so an offset identifies an item exactly.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::bounded_lines::{BoundedLine, BoundedLineReader};
use crate::error::ConvertError;
use crate::lenient_json::{LenientObject, LenientSeq, LenientString};

/// How many lines are read between progress and cancellation checks.
const PROGRESS_INTERVAL_LINES: usize = 500;

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
pub(crate) struct RolloutMeta {
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) git_branch: Option<String>,
}

/// How a tool call carried its arguments in the rollout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToolArguments {
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
pub(crate) struct BoundedToolOutput {
    /// The first [`crate::MAX_TOOL_RESULT_CHARS`] characters of the output.
    pub(crate) text: String,
    /// Character count of the complete flattened output.
    pub(crate) original_chars: usize,
    /// True when the complete output was longer than `text`.
    pub(crate) truncated: bool,
    /// False when the complete output is empty or whitespace only.
    pub(crate) has_visible_text: bool,
}

impl BoundedToolOutput {
    /// True when the complete output holds nothing but whitespace.
    pub(crate) fn is_blank(&self) -> bool {
        !self.has_visible_text
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SourceItem {
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
pub(crate) struct RolloutCounts {
    /// Lines that held JSON (everything except blank/unreadable lines).
    pub(crate) json_lines: usize,
    /// Unparseable lines, including the oversized ones.
    pub(crate) malformed_lines: usize,
    /// Lines over the line cap, skipped unread (a subset of `malformed_lines`).
    pub(crate) oversized_lines: usize,
    /// Lines parsed but intentionally not converted, by coarse kind.
    pub(crate) dropped_by_kind: BTreeMap<String, usize>,
}

impl RolloutCounts {
    pub(crate) fn dropped_total(&self) -> usize {
        self.dropped_by_kind.values().sum::<usize>() + self.malformed_lines
    }

    fn drop_kind(&mut self, kind: impl Into<String>) {
        *self.dropped_by_kind.entry(kind.into()).or_insert(0) += 1;
    }
}

/// Size and modification time of the source, captured when a scan opens it
/// so a second pass can tell whether the file changed in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceSignature {
    pub(crate) length: u64,
    pub(crate) modified: Option<SystemTime>,
}

impl SourceSignature {
    pub(crate) fn of(path: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// The caps a scan enforces; [`ScanLimits::default`] uses the crate limits.
/// Tests pass smaller ones so oversized lines need no 256 MiB fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScanLimits {
    pub(crate) source_bytes: u64,
    pub(crate) line_bytes: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            source_bytes: crate::MAX_SOURCE_TRANSCRIPT_BYTES,
            // 256 MiB fits every supported `usize`.
            line_bytes: crate::MAX_LINE_BYTES as usize,
        }
    }
}

/// One kept item and the byte offset of the line it came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScannedItem {
    pub(crate) offset: u64,
    pub(crate) item: SourceItem,
}

/// Streams the kept items of a rollout in file order. Session metadata and
/// the drop counters are available at any point and complete once
/// [`RolloutScanner::next_item`] has returned `None`.
pub(crate) struct RolloutScanner<'a> {
    path: PathBuf,
    lines: BoundedLineReader<BufReader<File>>,
    source: SourceSignature,
    limits: ScanLimits,
    line_count: usize,
    meta: RolloutMeta,
    counts: RolloutCounts,
    /// Receives the fraction of bytes read every few hundred lines and
    /// returns `false` to cancel.
    on_progress: &'a mut dyn FnMut(f32) -> bool,
}

impl<'a> RolloutScanner<'a> {
    pub(crate) fn open(
        path: &Path,
        on_progress: &'a mut dyn FnMut(f32) -> bool,
    ) -> Result<Self, ConvertError> {
        Self::open_with_limits(path, ScanLimits::default(), on_progress)
    }

    pub(crate) fn open_with_limits(
        path: &Path,
        limits: ScanLimits,
        on_progress: &'a mut dyn FnMut(f32) -> bool,
    ) -> Result<Self, ConvertError> {
        let unreadable = |error| ConvertError::SourceUnreadable {
            path: path.to_path_buf(),
            error,
        };
        let file = ilium_platform::secure_fs::open_regular_file(path).map_err(unreadable)?;
        let metadata = file.metadata().map_err(unreadable)?;
        let source = SourceSignature {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        };
        if source.length > limits.source_bytes {
            return Err(ConvertError::FileTooLarge {
                path: path.to_path_buf(),
                bytes: source.length,
                maximum: limits.source_bytes,
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            lines: BoundedLineReader::new(BufReader::new(file), limits.line_bytes),
            source,
            limits,
            line_count: 0,
            meta: RolloutMeta::default(),
            counts: RolloutCounts::default(),
            on_progress,
        })
    }

    pub(crate) fn meta(&self) -> &RolloutMeta {
        &self.meta
    }

    pub(crate) fn counts(&self) -> &RolloutCounts {
        &self.counts
    }

    pub(crate) fn source_signature(&self) -> SourceSignature {
        self.source
    }

    #[cfg(test)]
    pub(crate) fn bytes_read(&self) -> u64 {
        self.lines.offset()
    }

    /// Returns the next kept item, or `None` at the end of the file.
    pub(crate) fn next_item(&mut self) -> Result<Option<ScannedItem>, ConvertError> {
        loop {
            let classified =
                match self
                    .lines
                    .next_line()
                    .map_err(|error| ConvertError::SourceUnreadable {
                        path: self.path.clone(),
                        error,
                    })? {
                    None => return Ok(None),
                    Some(BoundedLine::Oversized { .. }) => ClassifiedLine::Oversized,
                    Some(BoundedLine::Line { offset, bytes }) => {
                        // Lossy decoding keeps invalid UTF-8 inside strings readable
                        // (as before); valid lines borrow the buffer without a copy.
                        let text = String::from_utf8_lossy(bytes);
                        let trimmed = text.trim();
                        if trimmed.is_empty() {
                            ClassifiedLine::Blank
                        } else {
                            ClassifiedLine::Json(offset, classify_line(trimmed))
                        }
                    }
                };
            self.line_count += 1;
            let bytes_read = self.lines.offset();
            if bytes_read > self.limits.source_bytes {
                // The file grew past the cap while being read.
                return Err(ConvertError::FileTooLarge {
                    path: self.path.clone(),
                    bytes: bytes_read,
                    maximum: self.limits.source_bytes,
                });
            }
            if self.line_count.is_multiple_of(PROGRESS_INTERVAL_LINES) {
                let fraction = if self.source.length == 0 {
                    0.0
                } else {
                    (bytes_read as f64 / self.source.length as f64) as f32
                };
                if !(self.on_progress)(fraction) {
                    return Err(ConvertError::Cancelled);
                }
            }
            match classified {
                ClassifiedLine::Blank => {}
                ClassifiedLine::Oversized => {
                    self.counts.malformed_lines += 1;
                    self.counts.oversized_lines += 1;
                }
                ClassifiedLine::Json(_, LineOutcome::Malformed) => {
                    self.counts.malformed_lines += 1;
                }
                ClassifiedLine::Json(_, LineOutcome::Dropped(kind)) => {
                    self.counts.json_lines += 1;
                    self.counts.drop_kind(kind);
                }
                ClassifiedLine::Json(_, LineOutcome::Meta(meta)) => {
                    self.counts.json_lines += 1;
                    // Only the first `session_meta` owns the rollout's identity.
                    if self.meta.session_id.is_some() {
                        self.counts.drop_kind("session_meta");
                    } else {
                        self.meta = meta;
                    }
                }
                ClassifiedLine::Json(offset, LineOutcome::Item(item)) => {
                    self.counts.json_lines += 1;
                    return Ok(Some(ScannedItem { offset, item }));
                }
            }
        }
    }
}

/// Re-reads single lines of a rollout by the offsets a [`RolloutScanner`]
/// reported. Keeps its own file handle so it never disturbs a running scan.
pub(crate) struct RolloutLineAccess {
    path: PathBuf,
    lines: BoundedLineReader<BufReader<File>>,
}

impl RolloutLineAccess {
    pub(crate) fn open(path: &Path) -> Result<Self, ConvertError> {
        Self::open_with_line_limit(path, ScanLimits::default().line_bytes)
    }

    pub(crate) fn open_with_line_limit(
        path: &Path,
        line_bytes: usize,
    ) -> Result<Self, ConvertError> {
        let file = ilium_platform::secure_fs::open_regular_file(path).map_err(|error| {
            ConvertError::SourceUnreadable {
                path: path.to_path_buf(),
                error,
            }
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            lines: BoundedLineReader::new(BufReader::new(file), line_bytes),
        })
    }

    /// The item of the line starting at `offset`, or `None` when that line
    /// holds no kept item (blank, dropped, malformed, oversized, or past the
    /// end of the file).
    pub(crate) fn item_at(&mut self, offset: u64) -> Result<Option<SourceItem>, ConvertError> {
        let unreadable = |error| ConvertError::SourceUnreadable {
            path: self.path.clone(),
            error,
        };
        self.lines.seek_to(offset).map_err(unreadable)?;
        let Some(BoundedLine::Line { bytes, .. }) = self.lines.next_line().map_err(unreadable)?
        else {
            return Ok(None);
        };
        let text = String::from_utf8_lossy(bytes);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(match classify_line(trimmed) {
            LineOutcome::Item(item) => Some(item),
            _ => None,
        })
    }
}

/// Everything a complete scan collects, for tests that want the items in
/// memory. Conversions stream with [`RolloutScanner`] instead.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedRollout {
    pub(crate) meta: RolloutMeta,
    pub(crate) items: Vec<SourceItem>,
    /// Lines that held JSON (everything except blank/unparseable lines).
    pub(crate) json_lines: usize,
    pub(crate) malformed_lines: usize,
    /// Lines parsed but intentionally not converted, by coarse kind.
    pub(crate) dropped_by_kind: BTreeMap<String, usize>,
}

#[cfg(test)]
impl ParsedRollout {
    pub(crate) fn dropped_total(&self) -> usize {
        self.dropped_by_kind.values().sum::<usize>() + self.malformed_lines
    }
}

/// Parses the whole rollout at `path`. `on_progress` receives the fraction
/// of bytes read every few hundred lines and returns `false` to cancel.
#[cfg(test)]
pub(crate) fn parse_rollout(
    path: &Path,
    on_progress: &mut dyn FnMut(f32) -> bool,
) -> Result<ParsedRollout, ConvertError> {
    parse_rollout_with_limits(path, ScanLimits::default(), on_progress)
}

#[cfg(test)]
pub(crate) fn parse_rollout_with_limits(
    path: &Path,
    limits: ScanLimits,
    on_progress: &mut dyn FnMut(f32) -> bool,
) -> Result<ParsedRollout, ConvertError> {
    let mut scanner = RolloutScanner::open_with_limits(path, limits, on_progress)?;
    let mut items = Vec::new();
    while let Some(scanned) = scanner.next_item()? {
        items.push(scanned.item);
    }
    let counts = scanner.counts().clone();
    Ok(ParsedRollout {
        meta: scanner.meta().clone(),
        items,
        json_lines: counts.json_lines,
        malformed_lines: counts.malformed_lines,
        dropped_by_kind: counts.dropped_by_kind,
    })
}

enum ClassifiedLine {
    Blank,
    Oversized,
    Json(u64, LineOutcome),
}

/// What one non-blank line means, independent of scan state.
#[derive(Debug, PartialEq)]
enum LineOutcome {
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
fn classify_line(trimmed: &str) -> LineOutcome {
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
pub(crate) fn flatten_output(output: &Value) -> BoundedToolOutput {
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

    const FIXTURE: &str = include_str!("../tests/fixtures/codex_rollout_sample.jsonl");

    fn rollout_file(content: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        std::fs::write(&path, content).unwrap();
        (directory, path)
    }

    fn parse_text(content: &str) -> ParsedRollout {
        let (_directory, path) = rollout_file(content);
        parse_rollout(&path, &mut |_| true).unwrap()
    }

    fn parse_lines(lines: &[Value]) -> ParsedRollout {
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        parse_text(&text.join("\n"))
    }

    fn output(call_id: &str, output: Value) -> String {
        json!({"type": "response_item", "payload": {
            "type": "function_call_output", "call_id": call_id, "output": output
        }})
        .to_string()
    }

    fn bounded(output: Value) -> BoundedToolOutput {
        flatten_output(&output)
    }

    #[test]
    fn fixture_counts_match_the_previous_whole_tree_parser() {
        let parsed = parse_text(FIXTURE);
        let expected: BTreeMap<String, usize> = [
            ("compacted", 1),
            ("event_msg", 4),
            ("message:developer", 1),
            ("message:user (harness context)", 1),
            ("response_item:ghost_snapshot", 1),
            ("response_item:reasoning", 1),
            ("response_item:web_search_call", 1),
            ("token_usage_record", 1),
            ("turn_context", 1),
            ("world_state", 1),
        ]
        .into_iter()
        .map(|(kind, count)| (kind.to_string(), count))
        .collect();
        assert_eq!(parsed.dropped_by_kind, expected);
        assert_eq!(parsed.malformed_lines, 1);
        assert_eq!(parsed.json_lines, 27);
        assert_eq!(parsed.dropped_total(), 14);
        assert_eq!(parsed.items.len(), 13);
        assert_eq!(
            parsed.meta.session_id.as_deref(),
            Some("01a0f000-1111-7222-8333-444444444444")
        );
        assert_eq!(parsed.meta.git_branch.as_deref(), Some("feature/x"));
        let user_prompts = parsed
            .items
            .iter()
            .filter(|item| matches!(item, SourceItem::UserText { .. }))
            .count();
        assert_eq!(user_prompts, 1);
    }

    #[test]
    fn sniffing_reads_only_a_top_level_type_key() {
        assert_eq!(
            sniff_record_kind(
                r#"{"timestamp":"2026-01-01T00:00:00.000Z","type":"event_msg","payload":{"type":"x"}}"#
            ),
            Some("event_msg")
        );
        assert_eq!(
            sniff_record_kind(
                r#"{ "n" : 12.5 , "ok":true, "none":null, "type" : "turn_context" }"#
            ),
            Some("turn_context")
        );
        assert_eq!(
            sniff_record_kind(r#"{"id":"quote \" and \\ inside","type":"compacted"}"#),
            Some("compacted")
        );
        // A nested value before the key: undecided, the typed parse decides.
        assert_eq!(
            sniff_record_kind(r#"{"payload":{"type":"message"},"type":"response_item"}"#),
            None
        );
        assert_eq!(sniff_record_kind(r#"{"a":[1],"type":"x"}"#), None);
        // Escaped, non-string, missing or not an object: undecided.
        assert_eq!(sniff_record_kind(r#"{"type":"event\u005fmsg"}"#), None);
        assert_eq!(sniff_record_kind(r#"{"type":5}"#), None);
        assert_eq!(sniff_record_kind(r#"{"other":"x"}"#), None);
        assert_eq!(sniff_record_kind(r#"{"type":"unterminated"#), None);
        assert_eq!(sniff_record_kind(r#"["type","x"]"#), None);
        assert_eq!(sniff_record_kind("{}"), None);
        assert_eq!(sniff_record_kind(r#"{"é":"ü","type":"ok"}"#), Some("ok"));
    }

    #[test]
    fn dropped_kinds_found_by_sniffing_agree_with_the_typed_parse() {
        let lines = [
            r#"{"timestamp":"t","type":"event_msg","payload":{"type":"task_started","big":"x"}}"#,
            r#"{"payload":{"type":"task_started"},"type":"event_msg"}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","encrypted_content":"gAAA"}}"#,
            r#"{"payload":{"type":"reasoning"},"type":"response_item"}"#,
            r#"{"type":5,"payload":{}}"#,
            r#"[1,2]"#,
            r#""just a string""#,
        ];
        let outcomes: Vec<LineOutcome> = lines.iter().map(|line| classify_line(line)).collect();
        assert_eq!(outcomes[0], LineOutcome::Dropped("event_msg".to_string()));
        assert_eq!(outcomes[1], LineOutcome::Dropped("event_msg".to_string()));
        assert_eq!(
            outcomes[2],
            LineOutcome::Dropped("response_item:reasoning".to_string())
        );
        assert_eq!(outcomes[2], outcomes[3]);
        assert_eq!(outcomes[4], LineOutcome::Dropped(String::new()));
        assert_eq!(outcomes[5], LineOutcome::Dropped(String::new()));
        assert_eq!(outcomes[6], LineOutcome::Dropped(String::new()));
    }

    #[test]
    fn a_truncated_dropped_record_is_malformed_not_dropped_by_kind() {
        let parsed = parse_text(concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_st"
        ));
        assert_eq!(parsed.dropped_by_kind.get("event_msg"), Some(&1));
        assert_eq!(parsed.malformed_lines, 1);
        assert_eq!(parsed.json_lines, 1);
    }

    #[test]
    fn an_oversized_line_is_skipped_and_counted_without_breaking_the_scan() {
        let user = json!({"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]}})
        .to_string();
        let huge = output("call_big", Value::String("x".repeat(4096)));
        let content = format!("{user}\n{huge}\n{user}\n");
        let (_directory, path) = rollout_file(&content);
        let limits = ScanLimits {
            source_bytes: crate::MAX_SOURCE_TRANSCRIPT_BYTES,
            line_bytes: 1024,
        };
        let parsed = parse_rollout_with_limits(&path, limits, &mut |_| true).unwrap();
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.malformed_lines, 1);
        assert_eq!(parsed.json_lines, 2);
        let mut keep_going = |_| true;
        let mut scanner = RolloutScanner::open_with_limits(&path, limits, &mut keep_going).unwrap();
        while scanner.next_item().unwrap().is_some() {}
        assert_eq!(scanner.counts().oversized_lines, 1);
        assert_eq!(scanner.counts().malformed_lines, 1);
        assert_eq!(scanner.bytes_read(), content.len() as u64);
    }

    #[test]
    fn offsets_re_read_exactly_the_item_of_that_line() {
        let (_directory, path) = rollout_file(FIXTURE);
        let mut keep_going = |_| true;
        let mut scanner = RolloutScanner::open(&path, &mut keep_going).unwrap();
        let mut access = RolloutLineAccess::open(&path).unwrap();
        let mut scanned = Vec::new();
        while let Some(item) = scanner.next_item().unwrap() {
            scanned.push(item);
        }
        assert_eq!(scanned.len(), 13);
        // Out of order on purpose: random access must not depend on sequence.
        for item in scanned.iter().rev() {
            assert_eq!(
                access.item_at(item.offset).unwrap(),
                Some(item.item.clone())
            );
        }
        assert!(scanned
            .windows(2)
            .all(|pair| pair[0].offset < pair[1].offset));
        // Offsets of lines that hold no item, and past the end, yield nothing.
        assert_eq!(access.item_at(0).unwrap(), None);
        assert_eq!(access.item_at(FIXTURE.len() as u64 + 10).unwrap(), None);
        assert_eq!(
            scanner.source_signature(),
            SourceSignature::of(&path).unwrap()
        );
    }

    #[test]
    fn progress_is_reported_every_interval_and_can_cancel() {
        let line = r#"{"type":"event_msg","payload":{}}"#;
        let content = std::iter::repeat_n(line, PROGRESS_INTERVAL_LINES * 2 + 1)
            .collect::<Vec<_>>()
            .join("\n");
        let (_directory, path) = rollout_file(&content);
        let mut fractions = Vec::new();
        let parsed = parse_rollout(&path, &mut |fraction| {
            fractions.push(fraction);
            true
        })
        .unwrap();
        assert_eq!(fractions.len(), 2);
        assert!(fractions[0] > 0.0 && fractions[0] < fractions[1] && fractions[1] <= 1.0);
        assert_eq!(
            parsed.dropped_by_kind["event_msg"],
            PROGRESS_INTERVAL_LINES * 2 + 1
        );
        let error = parse_rollout(&path, &mut |_| false).unwrap_err();
        assert!(matches!(error, ConvertError::Cancelled));
    }

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

    #[test]
    fn fields_of_unexpected_shapes_read_as_absent() {
        // Mirrors the former `Value::as_str` tolerance: a numeric call id
        // drops the call, a non-array content drops the message, a
        // non-object payload drops the record under its kind.
        let parsed = parse_lines(&[
            json!({"type": "response_item", "payload": {"type": "function_call", "call_id": 7, "name": "x"}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": "text"}}),
            json!({"type": "response_item", "payload": "nope"}),
            json!({"type": "session_meta", "payload": null}),
            json!({"timestamp": 12, "type": "response_item", "payload": {"type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "ok"}]}}),
        ]);
        assert_eq!(
            parsed.items,
            vec![SourceItem::AssistantText {
                text: "ok".to_string(),
                timestamp: None
            }]
        );
        assert_eq!(
            parsed
                .dropped_by_kind
                .get("tool_call (missing call_id or name)"),
            Some(&1)
        );
        assert_eq!(
            parsed.dropped_by_kind.get("message:user (harness context)"),
            Some(&1)
        );
        assert_eq!(parsed.dropped_by_kind.get("response_item:"), Some(&1));
        assert_eq!(parsed.json_lines, 5);
        assert_eq!(parsed.meta, RolloutMeta::default());
    }

    #[test]
    fn only_the_first_session_meta_owns_the_identity() {
        let parsed = parse_lines(&[
            json!({"type": "session_meta", "payload": {"id": "first", "cwd": "/a", "git": {"branch": ""}}}),
            json!({"type": "session_meta", "payload": {"id": "second", "cwd": "/b"}}),
        ]);
        assert_eq!(parsed.meta.session_id.as_deref(), Some("first"));
        assert_eq!(parsed.meta.git_branch, None);
        assert_eq!(parsed.dropped_by_kind.get("session_meta"), Some(&1));
    }

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

    #[test]
    fn output_objects_use_their_text_field_or_their_json() {
        assert_eq!(
            bounded(json!({"output": "done", "metadata": {"exit_code": 0}})).text,
            "done"
        );
        assert_eq!(bounded(json!({"content": "c", "output": "o"})).text, "c");
        assert_eq!(
            bounded(json!({"success": false})).text,
            "{\"success\":false}"
        );
        assert_eq!(bounded(json!(42)).text, "42");
        assert_eq!(bounded(Value::Null), BoundedToolOutput::default());
    }

    #[test]
    fn tool_outputs_are_cut_at_the_cap_while_parsing() {
        let cap = crate::MAX_TOOL_RESULT_CHARS;
        let below = bounded(Value::String("x".repeat(cap - 1)));
        assert!(!below.truncated && below.original_chars == cap - 1);
        let exact = bounded(Value::String("x".repeat(cap)));
        assert!(!exact.truncated && exact.text.chars().count() == cap);
        let above = bounded(Value::String("x".repeat(cap + 10)));
        assert!(above.truncated && above.has_visible_text);
        assert_eq!(above.text.chars().count(), cap);
        assert_eq!(above.original_chars, cap + 10);
        // The cut counts characters, so a multibyte character at the cap is
        // kept whole and the ones after it are counted, not copied.
        let mut text = "a".repeat(cap - 1);
        text.push_str("é🦀z");
        let multibyte = bounded(Value::String(text));
        assert!(multibyte.text.ends_with("aé"));
        assert_eq!(multibyte.text.chars().count(), cap);
        assert_eq!(multibyte.original_chars, cap + 2);
        assert!(multibyte.truncated);
    }

    #[test]
    fn blank_outputs_are_recognised_over_the_complete_text() {
        let spaces = bounded(Value::String(" ".repeat(crate::MAX_TOOL_RESULT_CHARS + 1)));
        assert!(spaces.is_blank() && spaces.truncated);
        let mut late = " ".repeat(crate::MAX_TOOL_RESULT_CHARS);
        late.push('!');
        let late = bounded(Value::String(late));
        assert!(!late.is_blank() && late.truncated);
        assert!(bounded(json!(["", "  \n"])).is_blank());
        assert!(bounded(Value::String(String::new())).is_blank());
    }

    #[test]
    fn capped_arrays_join_exactly_like_the_complete_text() {
        // The separator rule is decided on the complete text: the first
        // block fills the cap and ends without a newline, so the separator
        // and the second block are counted but not copied.
        let cap = crate::MAX_TOOL_RESULT_CHARS;
        let output = json!([{"text": "a".repeat(cap)}, {"text": "b"}]);
        let flattened = flatten_output(&output);
        assert_eq!(flattened.text.len(), cap);
        assert_eq!(flattened.original_chars, cap + 2);
        assert!(flattened.truncated);
    }

    #[test]
    fn tool_output_items_carry_the_error_flag_and_the_bounded_text() {
        let parsed = parse_text(&output("c1", json!({"success": false, "content": "boom"})));
        assert_eq!(
            parsed.items,
            vec![SourceItem::ToolOutput {
                call_id: "c1".to_string(),
                output: BoundedToolOutput {
                    text: "boom".to_string(),
                    original_chars: 4,
                    truncated: false,
                    has_visible_text: true,
                },
                is_error: true,
            }]
        );
    }

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

    #[test]
    fn oversized_rollout_is_rejected_from_metadata_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.jsonl");
        let limits = ScanLimits {
            source_bytes: 4096,
            line_bytes: 1024,
        };
        std::fs::File::create(&path)
            .unwrap()
            .set_len(limits.source_bytes + 1)
            .unwrap();
        let error = parse_rollout_with_limits(&path, limits, &mut |_| true).unwrap_err();
        assert!(matches!(
            error,
            ConvertError::FileTooLarge { bytes, maximum, .. } if bytes == 4097 && maximum == 4096
        ));
        // The default limit is the crate's source sanity bound.
        assert_eq!(
            ScanLimits::default().source_bytes,
            crate::MAX_SOURCE_TRANSCRIPT_BYTES
        );
    }
}
