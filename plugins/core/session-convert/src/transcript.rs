//! Turns converted Codex conversation items into the lines of a Claude Code transcript.
//!
//! Moved from `ilium-session-convert/src/claude_writer/{mod,transcript_passes}.rs`. The kernel read the rollout
//! twice and streamed every line into a temporary file; the plugin holds the parsed items (the host reads the
//! rollout and hands the text over), so the two passes become one function, [`assemble`], and the lines are
//! returned instead of written. The pairing rule is the kernel's: a result is emitted right after its call; the
//! first output recorded for a call id answers the first call with that id, wherever either sits in the rollout;
//! later outputs for the same id, and outputs whose id has no call, are dropped.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::claude_writer::{
    bounded_result_text, claude_tool_use, normalize_timestamp, title_from_prompt,
};
use crate::prompts::{MISSING_RESULT, SYNTHETIC_FIRST_PROMPT};
use crate::rollout::SourceItem;

/// Claude Code version stamped on written lines (the one verified to resume).
pub const CLAUDE_CODE_VERSION: &str = "2.1.285";
/// Any valid model string works for replay; this is the one used when verifying.
const PLACEHOLDER_MODEL: &str = "claude-haiku-4-5-20251001";

/// A 128-bit identifier in the shape of a version 4 UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identifier([u8; 16]);

impl Identifier {
    /// The identifier with the bytes of a UUID, setting the version and variant bits.
    pub fn from_bytes(mut bytes: [u8; 16]) -> Self {
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self(bytes)
    }

    /// `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`.
    pub fn hyphenated(&self) -> String {
        let simple = self.simple();
        format!(
            "{}-{}-{}-{}-{}",
            &simple[0..8],
            &simple[8..12],
            &simple[12..16],
            &simple[16..20],
            &simple[20..32]
        )
    }

    /// Thirty-two lower-case hex digits, no separators.
    pub fn simple(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Where the generated identifiers (line uuids, message ids, tool-use ids) come from. Production derives them
/// from a seed; tests use a fixed sequence so two conversions can be compared byte for byte.
pub trait IdentifierSource {
    fn next_identifier(&mut self) -> Identifier;
}

/// The production [`IdentifierSource`]: SHA-256 of a seed and a counter. The host owns the entropy (the seed is
/// built from its clocks and the rollout path); these identifiers only have to be unique within one transcript
/// and unlikely to clash with another session, they are not secrets.
#[derive(Debug, Clone)]
pub struct HashedIdentifiers {
    seed: String,
    counter: u64,
}

impl HashedIdentifiers {
    pub fn new(seed: impl Into<String>) -> Self {
        Self {
            seed: seed.into(),
            counter: 0,
        }
    }
}

impl IdentifierSource for HashedIdentifiers {
    fn next_identifier(&mut self) -> Identifier {
        let mut hasher = Sha256::new();
        hasher.update(self.seed.as_bytes());
        hasher.update(self.counter.to_le_bytes());
        self.counter += 1;
        let digest = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Identifier::from_bytes(bytes)
    }
}

/// What the written transcript is for.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptTarget<'a> {
    pub session_id: &'a str,
    pub cwd: &'a str,
    pub git_branch: Option<&'a str>,
    /// Used for any line whose source item has no usable timestamp.
    pub fallback_timestamp: DateTime<Utc>,
}

/// Whether the transcript ends with the `ai-title` line Claude Code labels its resume picker with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleLine {
    Append,
    Omit,
}

/// Counters the assembly reports once it has finished.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AssemblyStats {
    /// Source items that became part of the transcript.
    pub converted_items: usize,
    pub tool_calls: usize,
    pub synthesized_results: usize,
    pub truncated_results: usize,
    pub synthesized_first_prompt: bool,
    /// Tool outputs that answered no call, or answered one a second time.
    pub unpaired_outputs: usize,
}

/// The finished transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    /// One JSON object per entry, without the trailing newline.
    pub lines: Vec<String>,
    pub stats: AssemblyStats,
}

/// Emits Claude Code lines in order, maintaining the `parentUuid` chain and the "last known timestamp" that
/// lines without a source timestamp inherit.
struct ClaudeLineEmitter<'a> {
    target: TranscriptTarget<'a>,
    previous_uuid: Option<String>,
    last_timestamp: String,
    identifiers: &'a mut dyn IdentifierSource,
    lines: Vec<String>,
}

impl<'a> ClaudeLineEmitter<'a> {
    fn new(
        target: TranscriptTarget<'a>,
        initial_timestamp: String,
        identifiers: &'a mut dyn IdentifierSource,
    ) -> Self {
        Self {
            target,
            previous_uuid: None,
            last_timestamp: initial_timestamp,
            identifiers,
            lines: Vec::new(),
        }
    }

    fn next_identifier(&mut self) -> Identifier {
        self.identifiers.next_identifier()
    }

    fn push_message(&mut self, role: &str, timestamp: Option<&str>, message: Value) {
        let uuid = self.next_identifier().hyphenated();
        if let Some(timestamp) = timestamp {
            self.last_timestamp = timestamp.to_string();
        }
        let mut line = Map::new();
        line.insert("type".into(), json!(role));
        line.insert("uuid".into(), json!(uuid));
        line.insert("parentUuid".into(), json!(self.previous_uuid));
        line.insert("sessionId".into(), json!(self.target.session_id));
        line.insert("timestamp".into(), json!(self.last_timestamp));
        line.insert("cwd".into(), json!(self.target.cwd));
        line.insert("version".into(), json!(CLAUDE_CODE_VERSION));
        line.insert("isSidechain".into(), json!(false));
        line.insert("userType".into(), json!("external"));
        line.insert("entrypoint".into(), json!("cli"));
        if let Some(branch) = self.target.git_branch {
            line.insert("gitBranch".into(), json!(branch));
        }
        line.insert("message".into(), message);
        self.previous_uuid = Some(uuid);
        self.lines.push(Value::Object(line).to_string());
    }

    fn push_user_text(&mut self, timestamp: Option<&str>, text: &str) {
        self.push_message("user", timestamp, json!({"role": "user", "content": text}));
    }

    fn push_assistant_block(&mut self, timestamp: Option<&str>, block: Value, stop_reason: &str) {
        let message_id = format!("msg_{}", self.next_identifier().simple());
        self.push_message(
            "assistant",
            timestamp,
            json!({
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "model": PLACEHOLDER_MODEL,
                "content": [block],
                "stop_reason": stop_reason,
                "stop_sequence": null,
            }),
        );
    }

    fn push_tool_result(
        &mut self,
        timestamp: Option<&str>,
        tool_use_id: &str,
        text: &str,
        is_error: bool,
    ) {
        self.push_message(
            "user",
            timestamp,
            json!({"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": text,
                "is_error": is_error,
            }]}),
        );
    }

    /// `ai-title` is how Claude Code labels a session in its resume picker; it sits outside the uuid chain,
    /// exactly as in real transcripts.
    fn push_title(&mut self, title: &str) {
        self.lines.push(
            json!({"type": "ai-title", "aiTitle": title, "sessionId": self.target.session_id})
                .to_string(),
        );
    }

    fn next_tool_use_id(&mut self) -> String {
        format!("toolu_{}", self.next_identifier().simple())
    }
}

/// What a look over the whole item list says before any line is written (the kernel's pass 1).
#[derive(Debug, Default)]
struct ConversationFacts {
    /// Position of the first output recorded for each call id.
    first_output: HashMap<String, usize>,
    call_ids: HashSet<String>,
    outputs_seen: usize,
    /// First normalisable source timestamp, in Claude's format.
    first_timestamp: Option<String>,
    /// Session title from the first user prompt.
    title: Option<String>,
    /// `None` until the first item that is not a tool output was seen.
    first_is_user: Option<bool>,
    /// Items other than tool outputs (each becomes at least one line).
    conversation_items: usize,
}

impl ConversationFacts {
    fn observe_all(items: &[SourceItem]) -> Self {
        let mut facts = Self::default();
        let mut first_prompt_seen = false;
        for (position, item) in items.iter().enumerate() {
            let (call_id, timestamp) = match item {
                SourceItem::ToolOutput { call_id, .. } => {
                    facts.outputs_seen += 1;
                    facts
                        .first_output
                        .entry(call_id.clone())
                        .or_insert(position);
                    continue;
                }
                SourceItem::UserText { text, timestamp } => {
                    if !first_prompt_seen {
                        first_prompt_seen = true;
                        facts.title = title_from_prompt(text);
                    }
                    (None, timestamp)
                }
                SourceItem::AssistantText { timestamp, .. } => (None, timestamp),
                SourceItem::ToolCall {
                    call_id, timestamp, ..
                } => (Some(call_id), timestamp),
            };
            facts.conversation_items += 1;
            if facts.first_is_user.is_none() {
                facts.first_is_user = Some(matches!(item, SourceItem::UserText { .. }));
            }
            if facts.first_timestamp.is_none() {
                facts.first_timestamp = normalize_timestamp(timestamp.as_deref());
            }
            if let Some(call_id) = call_id {
                facts.call_ids.insert(call_id.clone());
            }
        }
        facts
    }

    /// True when the transcript must open with a placeholder user prompt: Claude Code requires a user message
    /// first and the source does not start with one.
    fn needs_synthetic_first_prompt(&self) -> bool {
        match self.first_is_user {
            Some(first_is_user) => !first_is_user,
            None => self.outputs_seen > 0,
        }
    }

    /// Outputs that answer no call (orphans) or answer one a second time.
    fn unpaired_outputs(&self) -> usize {
        let paired = self
            .call_ids
            .iter()
            .filter(|call_id| self.first_output.contains_key(*call_id))
            .count();
        self.outputs_seen - paired
    }
}

/// How many source items would become lines, before anything is written. Zero means the rollout holds no
/// conversation (only tool outputs, or nothing).
pub fn conversation_item_count(items: &[SourceItem]) -> usize {
    ConversationFacts::observe_all(items).conversation_items
}

/// Builds the Claude Code lines for `items`.
pub fn assemble(
    items: &[SourceItem],
    target: &TranscriptTarget<'_>,
    identifiers: &mut dyn IdentifierSource,
    title: TitleLine,
) -> Assembled {
    let mut facts = ConversationFacts::observe_all(items);
    let initial_timestamp = facts.first_timestamp.clone().unwrap_or_else(|| {
        target
            .fallback_timestamp
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    });
    let mut emitter = ClaudeLineEmitter::new(*target, initial_timestamp, identifiers);
    let mut stats = AssemblyStats {
        unpaired_outputs: facts.unpaired_outputs(),
        ..AssemblyStats::default()
    };
    if facts.needs_synthetic_first_prompt() {
        emitter.push_user_text(None, SYNTHETIC_FIRST_PROMPT);
        stats.synthesized_first_prompt = true;
    }
    // Call ids whose first call already took the recorded output.
    let mut answered: HashSet<String> = HashSet::new();
    for item in items {
        match item {
            SourceItem::UserText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                emitter.push_user_text(timestamp.as_deref(), text);
                stats.converted_items += 1;
            }
            SourceItem::AssistantText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                emitter.push_assistant_block(
                    timestamp.as_deref(),
                    json!({"type": "text", "text": text}),
                    "end_turn",
                );
                stats.converted_items += 1;
            }
            SourceItem::ToolCall {
                call_id,
                name,
                arguments,
                timestamp,
            } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                let (tool_name, tool_input) = claude_tool_use(name, arguments);
                let tool_use_id = emitter.next_tool_use_id();
                emitter.push_assistant_block(
                    timestamp.as_deref(),
                    json!({
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": tool_name,
                        "input": tool_input,
                    }),
                    "tool_use",
                );
                stats.tool_calls += 1;
                stats.converted_items += 1;
                // Only the first call with a given id receives the output.
                let recorded = if answered.insert(call_id.clone()) {
                    facts.first_output.get(call_id).and_then(|position| {
                        match items.get(*position) {
                            Some(SourceItem::ToolOutput {
                                output, is_error, ..
                            }) => Some((output, *is_error)),
                            _ => None,
                        }
                    })
                } else {
                    None
                };
                match recorded {
                    Some((output, is_error)) => {
                        let (text, truncated) = bounded_result_text(output);
                        stats.truncated_results += usize::from(truncated);
                        emitter.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            &text,
                            is_error,
                        );
                        stats.converted_items += 1;
                    }
                    None => {
                        emitter.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            MISSING_RESULT,
                            false,
                        );
                        stats.synthesized_results += 1;
                    }
                }
            }
            // Consumed when their call was emitted; orphans were counted before.
            SourceItem::ToolOutput { .. } => {}
        }
    }
    if title == TitleLine::Append {
        if let Some(title) = facts.title.take() {
            emitter.push_title(&title);
        }
    }
    Assembled {
        lines: emitter.lines,
        stats,
    }
}

/// Identifiers drawn from a fixed counter, so a test can compare two conversions of the same source.
#[cfg(test)]
pub struct SequentialIdentifiers(pub u64);

#[cfg(test)]
impl IdentifierSource for SequentialIdentifiers {
    fn next_identifier(&mut self) -> Identifier {
        self.0 += 1;
        let mut bytes = [0u8; 16];
        bytes[8..].copy_from_slice(&self.0.to_be_bytes());
        Identifier::from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rollout::{parse_rollout_text, ToolArguments};

    const SESSION_ID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn fixed_target() -> TranscriptTarget<'static> {
        TranscriptTarget {
            session_id: SESSION_ID,
            cwd: "/work",
            git_branch: Some("feature/x"),
            fallback_timestamp: DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
                .expect("literal timestamp")
                .with_timezone(&Utc),
        }
    }

    fn text_output(text: &str) -> crate::rollout::BoundedToolOutput {
        crate::rollout::BoundedToolOutput {
            text: text.to_string(),
            original_chars: text.chars().count(),
            truncated: false,
            has_visible_text: !text.trim().is_empty(),
        }
    }

    fn call(id: &str) -> SourceItem {
        SourceItem::ToolCall {
            call_id: id.to_string(),
            name: "shell".to_string(),
            arguments: ToolArguments::JsonText("{\"command\":[\"ls\"]}".to_string()),
            timestamp: None,
        }
    }

    fn output(id: &str, text: &str) -> SourceItem {
        SourceItem::ToolOutput {
            call_id: id.to_string(),
            output: text_output(text),
            is_error: false,
        }
    }

    fn user(text: &str, timestamp: Option<&str>) -> SourceItem {
        SourceItem::UserText {
            text: text.to_string(),
            timestamp: timestamp.map(str::to_string),
        }
    }

    fn assemble_lines(items: &[SourceItem], title: TitleLine) -> Assembled {
        assemble(items, &fixed_target(), &mut SequentialIdentifiers(0), title)
    }

    fn parsed(lines: &[String]) -> Vec<Value> {
        lines
            .iter()
            .map(|line| serde_json::from_str(line).expect("each line is JSON"))
            .collect()
    }

    #[test]
    fn identifiers_have_the_shape_of_a_version_four_uuid() {
        let mut source = HashedIdentifiers::new("seed");
        let first = source.next_identifier();
        let second = source.next_identifier();
        assert_ne!(first, second);
        let text = first.hyphenated();
        assert_eq!(text.len(), 36);
        assert_eq!(text.as_bytes()[14], b'4');
        assert!(matches!(text.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert_eq!(first.simple().len(), 32);
        let mut again = HashedIdentifiers::new("seed");
        assert_eq!(again.next_identifier(), first);
        let mut other = HashedIdentifiers::new("other seed");
        assert_ne!(other.next_identifier(), first);
    }

    #[test]
    fn a_call_is_followed_by_its_result_and_the_chain_is_unbroken() {
        let items = [
            user("hello there", Some("2030-01-02T03:04:05.678Z")),
            call("c1"),
            output("c1", "a.txt\n"),
        ];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["parentUuid"], Value::Null);
        assert_eq!(lines[1]["parentUuid"], lines[0]["uuid"]);
        assert_eq!(lines[2]["parentUuid"], lines[1]["uuid"]);
        let tool_use_id = lines[1]["message"]["content"][0]["id"]
            .as_str()
            .expect("tool_use id");
        assert_eq!(lines[1]["message"]["content"][0]["name"], "Bash");
        assert_eq!(
            lines[2]["message"]["content"][0]["tool_use_id"].as_str(),
            Some(tool_use_id)
        );
        assert_eq!(lines[2]["message"]["content"][0]["content"], "a.txt\n");
        assert_eq!(lines[0]["timestamp"], "2030-01-02T03:04:05.678Z");
        assert_eq!(lines[0]["gitBranch"], "feature/x");
        assert_eq!(lines[0]["sessionId"], SESSION_ID);
        assert_eq!(assembled.stats.tool_calls, 1);
        assert_eq!(assembled.stats.converted_items, 3);
        assert_eq!(assembled.stats.unpaired_outputs, 0);
    }

    #[test]
    fn a_call_without_an_output_gets_the_missing_result_text() {
        let items = [user("go", None), call("c1")];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        assert_eq!(
            lines[2]["message"]["content"][0]["content"],
            Value::String(MISSING_RESULT.to_string())
        );
        assert_eq!(assembled.stats.synthesized_results, 1);
        assert_eq!(assembled.stats.converted_items, 2);
    }

    #[test]
    fn an_output_before_its_call_still_answers_the_first_call_only() {
        let items = [
            user("go", None),
            output("c1", "first"),
            call("c1"),
            call("c1"),
            output("c1", "second"),
        ];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        // user, call, result, call, result.
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[2]["message"]["content"][0]["content"], "first");
        assert_eq!(
            lines[4]["message"]["content"][0]["content"],
            Value::String(MISSING_RESULT.to_string())
        );
        // The second output answers a call that already has one.
        assert_eq!(assembled.stats.unpaired_outputs, 1);
    }

    #[test]
    fn an_orphan_output_is_dropped_and_counted() {
        let items = [user("go", None), output("nobody", "lost")];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        assert_eq!(assembled.lines.len(), 1);
        assert_eq!(assembled.stats.unpaired_outputs, 1);
    }

    #[test]
    fn a_conversation_that_opens_with_the_assistant_gets_a_placeholder_prompt() {
        let items = [SourceItem::AssistantText {
            text: "hi".to_string(),
            timestamp: Some("2030-01-02T03:04:05Z".to_string()),
        }];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["type"], "user");
        assert_eq!(
            lines[0]["message"]["content"],
            Value::String(SYNTHETIC_FIRST_PROMPT.to_string())
        );
        assert_eq!(lines[1]["parentUuid"], lines[0]["uuid"]);
        // The placeholder never post-dates the first real line.
        assert_eq!(lines[0]["timestamp"], "2030-01-02T03:04:05.000Z");
        assert!(assembled.stats.synthesized_first_prompt);
    }

    #[test]
    fn a_line_without_a_timestamp_inherits_the_last_known_one() {
        let items = [
            user("one", Some("2030-01-02T03:04:05.000Z")),
            SourceItem::AssistantText {
                text: "two".to_string(),
                timestamp: None,
            },
        ];
        let assembled = assemble_lines(&items, TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        assert_eq!(lines[1]["timestamp"], lines[0]["timestamp"]);
    }

    #[test]
    fn no_timestamp_anywhere_uses_the_fallback() {
        let assembled = assemble_lines(&[user("one", None)], TitleLine::Omit);
        let lines = parsed(&assembled.lines);
        assert_eq!(lines[0]["timestamp"], "2030-01-01T00:00:00.000Z");
    }

    #[test]
    fn the_title_line_sits_outside_the_chain_and_is_optional() {
        let items = [user("first line\nsecond line", None)];
        let with = assemble_lines(&items, TitleLine::Append);
        let lines = parsed(&with.lines);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["type"], "ai-title");
        assert_eq!(lines[1]["aiTitle"], "first line");
        assert_eq!(lines[1]["sessionId"], SESSION_ID);
        let without = assemble_lines(&items, TitleLine::Omit);
        assert_eq!(without.lines.len(), 1);
    }

    #[test]
    fn two_conversions_with_the_same_identifiers_are_identical() {
        let items = [user("go", None), call("c1"), output("c1", "ok")];
        let first = assemble_lines(&items, TitleLine::Append);
        let second = assemble_lines(&items, TitleLine::Append);
        assert_eq!(first, second);
    }

    #[test]
    fn a_rollout_text_converts_end_to_end() {
        let text = [
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello there"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"ls\"]}","call_id":"c1"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"{\"output\":\"a.txt\\n\"}"}}"#,
        ]
        .join("\n");
        let rollout = parse_rollout_text(&text);
        assert_eq!(conversation_item_count(&rollout.items), 2);
        let assembled = assemble_lines(&rollout.items, TitleLine::Omit);
        assert_eq!(assembled.lines.len(), 3);
        assert!(assembled.lines[2].contains("a.txt"));
    }
}
