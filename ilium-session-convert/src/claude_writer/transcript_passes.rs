//! The two passes that turn a stream of Codex conversation items into Claude
//! Code lines without ever holding the items, or the lines, in memory.
//!
//! Pass 1 ([`ConversationFacts::observe`] over every item) keeps only small
//! facts: which tool-call ids exist, where (byte offset) the first output
//! for each sits, the first timestamp, the title, and whether the
//! conversation opens with a user prompt. Pass 2 ([`TranscriptAssembler`])
//! walks the items again and emits each one's lines at once; when a tool
//! call is answered, the output is fetched from its recorded offset through
//! an [`OutputSource`], so the only per-call state is an id and a `u64`.
//!
//! The pairing rule is unchanged from the former in-memory converter: a
//! result is emitted right after its call; the first output recorded for a
//! call id answers the first call with that id, wherever either sits in the
//! file; later outputs for the same id, and outputs whose id has no call,
//! are dropped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::json;

use super::{
    bounded_result_text, claude_tool_use, normalize_timestamp, title_from_prompt,
    ClaudeLineEmitter, IdentifierSource, LineSink, MISSING_RESULT_TEXT, SYNTHETIC_FIRST_PROMPT,
};
use crate::codex_rollout::{BoundedToolOutput, RolloutLineAccess, SourceItem};
use crate::error::ConvertError;

/// Generous allowance for the JSON envelope every emitted line adds around
/// its source text, used to estimate the output size before writing.
const OUTPUT_LINE_OVERHEAD_BYTES: u64 = 512;

/// What the written transcript is for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TranscriptTarget<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) git_branch: Option<&'a str>,
    /// Used for any line whose source item has no usable timestamp.
    pub(crate) fallback_timestamp: DateTime<Utc>,
}

/// What pass 1 knows about one tool-call id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallPairing {
    /// A call was seen, no output (yet).
    CallOnly,
    /// An output was seen at `offset`, no call (yet).
    OutputOnly { offset: u64 },
    /// Both; `offset` is the first output's line.
    Paired { offset: u64 },
    /// Pass 2 already emitted the first call with this id.
    Answered,
}

/// How pass 2 should answer a tool call it has just emitted.
#[derive(Debug, PartialEq, Eq)]
enum CallClaim {
    /// The first call with this id; `output_offset` is where its output is.
    FirstCall { output_offset: Option<u64> },
    /// A repeated call id: the output already went to the first one.
    RepeatCall,
    /// Pass 1 never saw this id, so the source is not the file pass 1 read.
    Unknown,
}

/// The small facts pass 1 collects over the whole item stream.
#[derive(Debug, Default)]
pub(crate) struct ConversationFacts {
    pairings: HashMap<String, CallPairing>,
    outputs_seen: usize,
    paired_calls: usize,
    /// First normalisable source timestamp, in Claude's format.
    pub(crate) first_timestamp: Option<String>,
    /// Session title from the first user prompt.
    pub(crate) title: Option<String>,
    first_prompt_seen: bool,
    /// `None` until the first item that is not a tool output was seen.
    first_is_user: Option<bool>,
    /// Items other than tool outputs (each becomes at least one line).
    pub(crate) conversation_items: usize,
}

impl ConversationFacts {
    /// Records one item; `offset` identifies its line for a later re-read.
    pub(crate) fn observe(&mut self, offset: u64, item: &SourceItem) {
        let (call_id, timestamp) = match item {
            SourceItem::ToolOutput { call_id, .. } => {
                self.outputs_seen += 1;
                match self.pairings.get_mut(call_id) {
                    None => {
                        self.pairings
                            .insert(call_id.clone(), CallPairing::OutputOnly { offset });
                    }
                    // A later output for an id that already has one is dropped.
                    Some(pairing) => {
                        if *pairing == CallPairing::CallOnly {
                            *pairing = CallPairing::Paired { offset };
                            self.paired_calls += 1;
                        }
                    }
                }
                return;
            }
            SourceItem::UserText { text, timestamp } => {
                if !self.first_prompt_seen {
                    self.first_prompt_seen = true;
                    self.title = title_from_prompt(text);
                }
                (None, timestamp)
            }
            SourceItem::AssistantText { timestamp, .. } => (None, timestamp),
            SourceItem::ToolCall {
                call_id, timestamp, ..
            } => (Some(call_id), timestamp),
        };
        self.conversation_items += 1;
        if self.first_is_user.is_none() {
            self.first_is_user = Some(matches!(item, SourceItem::UserText { .. }));
        }
        if self.first_timestamp.is_none() {
            self.first_timestamp = normalize_timestamp(timestamp.as_deref());
        }
        if let Some(call_id) = call_id {
            match self.pairings.get_mut(call_id) {
                None => {
                    self.pairings.insert(call_id.clone(), CallPairing::CallOnly);
                }
                // A repeated call id leaves the pairing with the first call.
                Some(pairing) => {
                    if let CallPairing::OutputOnly { offset } = *pairing {
                        *pairing = CallPairing::Paired { offset };
                        self.paired_calls += 1;
                    }
                }
            }
        }
    }

    /// True when the transcript must open with a placeholder user prompt:
    /// Claude Code requires a user message first and the source does not
    /// start with one.
    pub(crate) fn needs_synthetic_first_prompt(&self) -> bool {
        match self.first_is_user {
            Some(first_is_user) => !first_is_user,
            None => self.outputs_seen > 0,
        }
    }

    /// Outputs that answer no call (orphans) or answer one a second time.
    pub(crate) fn unpaired_outputs(&self) -> usize {
        self.outputs_seen - self.paired_calls
    }

    /// Upper estimate of the written transcript's size: every kept byte of
    /// the source at most once, plus an envelope per emitted line (a call
    /// and its result are two lines; the placeholder prompt and the title
    /// two more).
    pub(crate) fn estimated_output_bytes(&self, source_bytes: u64) -> u64 {
        let lines = (self.conversation_items as u64)
            .saturating_mul(2)
            .saturating_add(2);
        source_bytes.saturating_add(lines.saturating_mul(OUTPUT_LINE_OVERHEAD_BYTES))
    }

    fn claim_call(&mut self, call_id: &str) -> CallClaim {
        let Some(pairing) = self.pairings.get_mut(call_id) else {
            return CallClaim::Unknown;
        };
        let claim = match *pairing {
            CallPairing::CallOnly => CallClaim::FirstCall {
                output_offset: None,
            },
            CallPairing::Paired { offset } => CallClaim::FirstCall {
                output_offset: Some(offset),
            },
            CallPairing::Answered => CallClaim::RepeatCall,
            // Pass 1 saw an output but no call for this id, yet pass 2 has a
            // call: the file is not the one pass 1 read.
            CallPairing::OutputOnly { .. } => CallClaim::Unknown,
        };
        *pairing = CallPairing::Answered;
        claim
    }
}

/// Where pass 2 fetches a tool output pass 1 located by offset.
pub(crate) trait OutputSource {
    /// The output recorded for `call_id` at `offset`. An implementation
    /// returns an error when that position no longer holds such an output,
    /// which means the source changed since pass 1.
    fn tool_output_for(
        &mut self,
        call_id: &str,
        offset: u64,
    ) -> Result<(BoundedToolOutput, bool), ConvertError>;

    /// The file the outputs come from, for error messages.
    fn source_path(&self) -> PathBuf;
}

/// [`OutputSource`] over the rollout file itself.
pub(crate) struct RolloutOutputSource {
    path: PathBuf,
    access: RolloutLineAccess,
}

impl RolloutOutputSource {
    pub(crate) fn open(path: &Path) -> Result<Self, ConvertError> {
        Ok(Self {
            path: path.to_path_buf(),
            access: RolloutLineAccess::open(path)?,
        })
    }
}

impl OutputSource for RolloutOutputSource {
    fn tool_output_for(
        &mut self,
        call_id: &str,
        offset: u64,
    ) -> Result<(BoundedToolOutput, bool), ConvertError> {
        match self.access.item_at(offset)? {
            Some(SourceItem::ToolOutput {
                call_id: found,
                output,
                is_error,
            }) if found == call_id => Ok((output, is_error)),
            _ => Err(ConvertError::SourceChanged {
                path: self.path.clone(),
            }),
        }
    }

    fn source_path(&self) -> PathBuf {
        self.path.clone()
    }
}

/// Counters pass 2 reports once it has finished.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AssemblyStats {
    /// Source items that became part of the transcript.
    pub(crate) converted_items: usize,
    pub(crate) tool_calls: usize,
    pub(crate) synthesized_results: usize,
    pub(crate) truncated_results: usize,
    pub(crate) synthesized_first_prompt: bool,
}

/// Pass 2: emits the lines for each item as it arrives.
pub(crate) struct TranscriptAssembler<'a> {
    emitter: ClaudeLineEmitter<'a>,
    facts: ConversationFacts,
    stats: AssemblyStats,
}

impl<'a> TranscriptAssembler<'a> {
    /// Starts the transcript, emitting the placeholder first prompt when the
    /// facts say the source does not open with a user message.
    pub(crate) fn begin(
        facts: ConversationFacts,
        target: &TranscriptTarget<'a>,
        identifiers: &'a mut dyn IdentifierSource,
        sink: &'a mut dyn LineSink,
    ) -> Result<Self, ConvertError> {
        // Seeded from the first source timestamp so a placeholder first line
        // never post-dates the lines after it.
        let initial_timestamp = facts.first_timestamp.clone().unwrap_or_else(|| {
            target
                .fallback_timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        });
        let mut assembler = Self {
            emitter: ClaudeLineEmitter::new(target, initial_timestamp, identifiers, sink),
            facts,
            stats: AssemblyStats::default(),
        };
        if assembler.facts.needs_synthetic_first_prompt() {
            assembler
                .emitter
                .push_user_text(None, SYNTHETIC_FIRST_PROMPT)?;
            assembler.stats.synthesized_first_prompt = true;
        }
        Ok(assembler)
    }

    /// Emits the lines for one item. Tool outputs are consumed when their
    /// call is emitted, so they produce nothing here.
    pub(crate) fn emit(
        &mut self,
        item: SourceItem,
        outputs: &mut dyn OutputSource,
    ) -> Result<(), ConvertError> {
        match item {
            SourceItem::UserText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                self.emitter.push_user_text(timestamp.as_deref(), &text)?;
                self.stats.converted_items += 1;
            }
            SourceItem::AssistantText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                self.emitter.push_assistant_blocks(
                    timestamp.as_deref(),
                    json!({"type": "text", "text": text}),
                    "end_turn",
                )?;
                self.stats.converted_items += 1;
            }
            SourceItem::ToolCall {
                call_id,
                name,
                arguments,
                timestamp,
            } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                let (tool_name, tool_input) = claude_tool_use(&name, &arguments);
                let tool_use_id = self.emitter.next_tool_use_id();
                self.emitter.push_assistant_blocks(
                    timestamp.as_deref(),
                    json!({
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": tool_name,
                        "input": tool_input,
                    }),
                    "tool_use",
                )?;
                self.stats.tool_calls += 1;
                self.stats.converted_items += 1;

                // Only the first call with a given id receives the output.
                let output = match self.facts.claim_call(&call_id) {
                    CallClaim::FirstCall {
                        output_offset: Some(offset),
                    } => Some(outputs.tool_output_for(&call_id, offset)?),
                    CallClaim::FirstCall {
                        output_offset: None,
                    }
                    | CallClaim::RepeatCall => None,
                    CallClaim::Unknown => {
                        return Err(ConvertError::SourceChanged {
                            path: outputs.source_path(),
                        })
                    }
                };
                match output {
                    Some((output, is_error)) => {
                        let (text, truncated) = bounded_result_text(&output);
                        self.stats.truncated_results += usize::from(truncated);
                        self.emitter.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            &text,
                            is_error,
                        )?;
                        self.stats.converted_items += 1;
                    }
                    None => {
                        self.emitter.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            MISSING_RESULT_TEXT,
                            false,
                        )?;
                        self.stats.synthesized_results += 1;
                    }
                }
            }
            // Consumed when their call was emitted; orphans were counted in pass 1.
            SourceItem::ToolOutput { .. } => {}
        }
        Ok(())
    }

    /// Appends the `ai-title` line and returns the counters.
    pub(crate) fn finish(mut self) -> Result<AssemblyStats, ConvertError> {
        if let Some(title) = self.facts.title.take() {
            self.emitter.push_title(&title)?;
        }
        Ok(self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_writer::test_support::SequentialIdentifiers;
    use crate::codex_rollout::parse_rollout;
    use serde_json::Value;

    const FIXTURE: &str = include_str!("../../tests/fixtures/codex_rollout_sample.jsonl");
    const SESSION_ID: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn fixed_target() -> TranscriptTarget<'static> {
        TranscriptTarget {
            session_id: SESSION_ID,
            cwd: "/work",
            git_branch: Some("feature/x"),
            fallback_timestamp: DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    fn parse_text(text: &str) -> Vec<SourceItem> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        std::fs::write(&path, text).unwrap();
        parse_rollout(&path, &mut |_| true).unwrap().items
    }

    /// [`OutputSource`] over a slice, offsets being indices.
    struct SliceOutputs<'a>(&'a [SourceItem]);

    impl OutputSource for SliceOutputs<'_> {
        fn tool_output_for(
            &mut self,
            call_id: &str,
            offset: u64,
        ) -> Result<(BoundedToolOutput, bool), ConvertError> {
            match self.0.get(offset as usize) {
                Some(SourceItem::ToolOutput {
                    call_id: found,
                    output,
                    is_error,
                }) if found == call_id => Ok((output.clone(), *is_error)),
                _ => Err(ConvertError::SourceChanged {
                    path: self.source_path(),
                }),
            }
        }

        fn source_path(&self) -> PathBuf {
            PathBuf::from("<slice>")
        }
    }

    struct StreamedResult {
        lines: Vec<String>,
        stats: AssemblyStats,
        unpaired_outputs: usize,
        conversation_items: usize,
    }

    /// Runs both passes over `items`, exactly as the converter does over a
    /// file, with a fixed identifier sequence.
    fn stream(items: &[SourceItem]) -> StreamedResult {
        let mut facts = ConversationFacts::default();
        for (index, item) in items.iter().enumerate() {
            facts.observe(index as u64, item);
        }
        let unpaired_outputs = facts.unpaired_outputs();
        let conversation_items = facts.conversation_items;
        let target = fixed_target();
        let mut identifiers = SequentialIdentifiers::default();
        let mut lines: Vec<String> = Vec::new();
        let mut assembler =
            TranscriptAssembler::begin(facts, &target, &mut identifiers, &mut lines).unwrap();
        let mut outputs = SliceOutputs(items);
        for item in items {
            assembler.emit(item.clone(), &mut outputs).unwrap();
        }
        let stats = assembler.finish().unwrap();
        StreamedResult {
            lines,
            stats,
            unpaired_outputs,
            conversation_items,
        }
    }

    /// Lines of the converted fixture, for the tests that extend it.
    const FIXTURE_LINES: usize = 14;

    fn line_at(streamed: &StreamedResult, index: usize) -> Value {
        serde_json::from_str(&streamed.lines[index]).unwrap()
    }

    fn result_content(streamed: &StreamedResult, index: usize) -> Value {
        line_at(streamed, index)["message"]["content"][0]["content"].clone()
    }

    fn fixture_without(needle: &str) -> String {
        FIXTURE
            .lines()
            .filter(|line| !line.contains(needle))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn output_line(call_id: &str, text: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-09-30T10:00:09.000Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"{call_id}","output":"{text}"}}}}"#
        )
    }

    fn call_line(call_id: &str, command: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-09-30T10:00:09.500Z","type":"response_item","payload":{{"type":"function_call","name":"shell_command","arguments":"{{\"command\":\"{command}\"}}","call_id":"{call_id}"}}}}"#
        )
    }

    #[test]
    fn the_fixture_streams_with_the_expected_counters() {
        // Covers out-of-order parallel outputs (call_2 before call_1), an
        // orphan output, a call without output (call_4), an array output.
        let streamed = stream(&parse_text(FIXTURE));
        assert_eq!(streamed.lines.len(), FIXTURE_LINES);
        assert_eq!(
            streamed.stats,
            AssemblyStats {
                converted_items: 12,
                tool_calls: 5,
                synthesized_results: 1,
                truncated_results: 0,
                synthesized_first_prompt: false,
            }
        );
        assert_eq!(streamed.unpaired_outputs, 1, "the orphan");
        assert_eq!(streamed.conversation_items, 8);
    }

    #[test]
    fn duplicate_outputs_answer_only_the_first_time() {
        let text = format!("{FIXTURE}\n{}\n", output_line("call_1", "second answer"));
        let streamed = stream(&parse_text(&text));
        assert_eq!(
            streamed.lines.len(),
            FIXTURE_LINES,
            "a duplicate adds nothing"
        );
        assert_eq!(streamed.unpaired_outputs, 2, "orphan plus the duplicate");
        let joined = streamed.lines.join("\n");
        assert!(joined.contains("a.txt") && !joined.contains("second answer"));
    }

    #[test]
    fn an_output_recorded_before_its_call_still_answers_it() {
        let text = format!(
            "{FIXTURE}\n{}\n{}\n",
            output_line("call_early", "came first"),
            call_line("call_early", "late")
        );
        let streamed = stream(&parse_text(&text));
        assert_eq!(
            streamed.lines.len(),
            FIXTURE_LINES + 2,
            "a call and its result"
        );
        assert_eq!(streamed.unpaired_outputs, 1, "only the fixture's orphan");
        assert_eq!(streamed.stats.synthesized_results, 1, "only call_4");
        assert_eq!(result_content(&streamed, FIXTURE_LINES), "came first");
    }

    #[test]
    fn a_repeated_call_id_gets_a_placeholder_result() {
        let text = format!("{FIXTURE}\n{}\n", call_line("call_1", "ls again"));
        let streamed = stream(&parse_text(&text));
        assert_eq!(streamed.lines.len(), FIXTURE_LINES + 2);
        assert_eq!(
            streamed.stats.synthesized_results, 2,
            "call_4 and the repeat"
        );
        assert_eq!(
            streamed.unpaired_outputs, 1,
            "the first call keeps the output"
        );
        assert_eq!(
            result_content(&streamed, FIXTURE_LINES),
            MISSING_RESULT_TEXT
        );
    }

    #[test]
    fn an_assistant_first_conversation_gets_a_placeholder_prompt_and_a_later_title() {
        let without_prompt = fixture_without("List the files and remember");
        let streamed = stream(&parse_text(&without_prompt));
        assert!(streamed.stats.synthesized_first_prompt);
        // The prompt and its title are gone, the placeholder takes one line.
        assert_eq!(streamed.lines.len(), FIXTURE_LINES - 1);
        assert_eq!(
            line_at(&streamed, 0)["message"]["content"],
            SYNTHETIC_FIRST_PROMPT
        );
        assert_ne!(line_at(&streamed, FIXTURE_LINES - 2)["type"], "ai-title");

        let later_prompt = format!(
            "{without_prompt}\n{}\n",
            r#"{"timestamp":"2026-09-30T10:00:08.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"a later prompt"}]}}"#
        );
        let streamed = stream(&parse_text(&later_prompt));
        assert!(streamed.stats.synthesized_first_prompt);
        assert_eq!(streamed.lines.len(), FIXTURE_LINES + 1);
        let title: Value = serde_json::from_str(streamed.lines.last().unwrap()).unwrap();
        assert_eq!(title["aiTitle"], "a later prompt");
        // The placeholder carries the first real timestamp of the source.
        assert_eq!(
            line_at(&streamed, 0)["timestamp"],
            "2026-09-30T10:00:02.500Z"
        );
    }

    #[test]
    fn only_orphan_outputs_or_nothing_convert_nothing() {
        let orphan_only: String = FIXTURE
            .lines()
            .filter(|line| line.contains("session_meta") || line.contains("call_orphan"))
            .collect::<Vec<_>>()
            .join("\n");
        let streamed = stream(&parse_text(&orphan_only));
        assert_eq!(streamed.conversation_items, 0);
        assert_eq!(streamed.unpaired_outputs, 1);
        // The converter refuses such a source before writing; the passes
        // alone would produce only the placeholder prompt.
        assert_eq!(streamed.lines.len(), 1);
        assert!(streamed.stats.synthesized_first_prompt);
        let empty = stream(&[]);
        assert!(empty.lines.is_empty());
        assert_eq!(empty.stats, AssemblyStats::default());
    }

    #[test]
    fn truncated_and_blank_outputs_keep_their_markers() {
        let long = "x".repeat(crate::MAX_TOOL_RESULT_CHARS + 7);
        let text = format!(
            "{}\n{}\n{}\n{}\n",
            call_line("call_long", "yes"),
            output_line("call_long", &long),
            call_line("call_blank", "true"),
            output_line("call_blank", "  \\n ")
        );
        let streamed = stream(&parse_text(&text));
        // Placeholder prompt, then each call with its result.
        assert_eq!(streamed.lines.len(), 5);
        assert_eq!(streamed.stats.truncated_results, 1);
        assert_eq!(streamed.stats.tool_calls, 2);
        assert!(streamed.lines[2].contains("7 more characters"));
        assert_eq!(
            result_content(&streamed, 4),
            ilium_prompts::conversion::EMPTY_RESULT
        );
    }

    #[test]
    fn an_unparseable_first_timestamp_is_skipped_for_the_seed() {
        let text = format!(
            "{}\n{}\n",
            r#"{"timestamp":"not a date","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"first"}]}}"#,
            r#"{"timestamp":"2026-09-30T11:00:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"then"}]}}"#
        );
        let streamed = stream(&parse_text(&text));
        assert_eq!(streamed.lines.len(), 4, "placeholder, both messages, title");
        assert_eq!(
            line_at(&streamed, 0)["timestamp"],
            "2026-09-30T11:00:00.000Z"
        );
        assert_eq!(
            line_at(&streamed, 1)["timestamp"],
            "2026-09-30T11:00:00.000Z"
        );
    }

    #[test]
    fn a_call_pass_one_never_saw_is_reported_as_a_changed_source() {
        let items = parse_text(FIXTURE);
        let mut facts = ConversationFacts::default();
        for (index, item) in items.iter().enumerate() {
            facts.observe(index as u64, item);
        }
        let target = fixed_target();
        let mut identifiers = SequentialIdentifiers::default();
        let mut lines: Vec<String> = Vec::new();
        let mut assembler =
            TranscriptAssembler::begin(facts, &target, &mut identifiers, &mut lines).unwrap();
        let foreign = parse_text(&call_line("call_never_scanned", "x"));
        let error = assembler
            .emit(foreign[0].clone(), &mut SliceOutputs(&items))
            .unwrap_err();
        assert!(
            matches!(error, ConvertError::SourceChanged { .. }),
            "{error}"
        );
    }

    #[test]
    fn an_output_that_moved_is_reported_as_a_changed_source() {
        let items = parse_text(FIXTURE);
        let mut facts = ConversationFacts::default();
        for (index, item) in items.iter().enumerate() {
            facts.observe(index as u64, item);
        }
        let target = fixed_target();
        let mut identifiers = SequentialIdentifiers::default();
        let mut lines: Vec<String> = Vec::new();
        let mut assembler =
            TranscriptAssembler::begin(facts, &target, &mut identifiers, &mut lines).unwrap();
        // Offsets now point at the wrong items.
        let mut shifted = items.clone();
        shifted.remove(0);
        let call = items
            .iter()
            .find(
                |item| matches!(item, SourceItem::ToolCall { call_id, .. } if call_id == "call_1"),
            )
            .unwrap();
        let error = assembler
            .emit(call.clone(), &mut SliceOutputs(&shifted))
            .unwrap_err();
        assert!(
            matches!(error, ConvertError::SourceChanged { .. }),
            "{error}"
        );
    }

    #[test]
    fn the_output_estimate_grows_with_items_and_never_overflows() {
        let mut facts = ConversationFacts::default();
        assert_eq!(facts.estimated_output_bytes(1_000), 1_000 + 2 * 512);
        facts.observe(
            0,
            &SourceItem::UserText {
                text: "hi".into(),
                timestamp: None,
            },
        );
        assert_eq!(facts.estimated_output_bytes(1_000), 1_000 + 4 * 512);
        assert_eq!(facts.estimated_output_bytes(u64::MAX), u64::MAX);
    }

    #[test]
    fn the_fixture_output_matches_the_golden_transcript() {
        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex_rollout_sample.claude.jsonl");
        let mut actual = stream(&parse_text(FIXTURE)).lines.join("\n");
        actual.push('\n');
        let golden = std::fs::read_to_string(&golden_path).unwrap_or_default();
        assert!(
            actual == golden,
            "the streamed fixture conversion differs from {}; actual output:\n{actual}",
            golden_path.display()
        );
    }
}
