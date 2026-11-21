//! Bounded activity evidence from a caller-verified agent transcript.
//!
//! This parser does not establish transcript ownership. Callers must fence
//! the live process, session, file identity and generation before using it.

use std::collections::{HashSet, VecDeque};

use ilium_core::polling_detection::{ActivityEvent, ActivityKind};
use ilium_core::AgentClass;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use crate::polling_incremental::{IncrementalBatch, IncrementalReader, TranscriptCheckpoint};

const MAX_RECENT_IDS: usize = 4096;
const MESSAGE_CHARS: usize = 200;
const CALL_CHARS: usize = 300;
const RESULT_CHARS: usize = 120;
const ID_CHARS: usize = 256;

/// Deduplicates provider streaming records in a bounded, per-session cache.
#[derive(Clone, Default)]
pub struct ActivityExtractor {
    recent: HashSet<[u8; 32]>,
    order: VecDeque<[u8; 32]>,
}

impl ActivityExtractor {
    pub(crate) fn retained_heap_bytes(&self) -> usize {
        // Conservative bucket/control allowance for the supported standard
        // HashSet layout, including spare capacity rather than only live keys.
        let recent = if self.recent.capacity() == 0 {
            0
        } else {
            self.recent
                .capacity()
                .saturating_mul(2)
                .saturating_add(16)
                .saturating_mul(std::mem::size_of::<[u8; 32]>() + 1)
        };
        recent.saturating_add(
            self.order
                .capacity()
                .saturating_mul(std::mem::size_of::<[u8; 32]>()),
        )
    }

    pub(crate) fn checkpoint_ids(&self) -> Vec<[u8; 32]> {
        self.order.iter().copied().collect()
    }

    pub(crate) fn from_checkpoint_ids(digests: &[[u8; 32]]) -> Option<Self> {
        if digests.len() > MAX_RECENT_IDS {
            return None;
        }
        let recent: HashSet<_> = digests.iter().copied().collect();
        if recent.len() != digests.len() {
            return None;
        }
        Some(Self {
            recent,
            order: digests.iter().copied().collect(),
        })
    }

    pub fn observe(&mut self, provider: &AgentClass, entry: &Value) -> Vec<ActivityEvent> {
        if entry["isSidechain"].as_bool() == Some(true) {
            return Vec::new();
        }
        let timestamp = bounded_option(entry["timestamp"].as_str(), 64);
        match provider {
            AgentClass::Codex => self.codex(entry, timestamp),
            AgentClass::Claude => self.claude(entry, timestamp),
            _ => Vec::new(),
        }
    }

    fn first_seen(&mut self, key: Option<String>) -> bool {
        let Some(key) = key else { return true };
        let key: [u8; 32] = Sha256::digest(key.as_bytes()).into();
        if !self.recent.insert(key) {
            return false;
        }
        self.order.push_back(key);
        if self.order.len() > MAX_RECENT_IDS {
            if let Some(oldest) = self.order.pop_front() {
                self.recent.remove(&oldest);
            }
        }
        true
    }

    fn first_linked_result(&mut self, id: Option<&str>) -> bool {
        let Some(id) = id else { return false };
        let call_digest: [u8; 32] = Sha256::digest(format!("call:{id}").as_bytes()).into();
        self.recent.contains(&call_digest) && self.first_seen(Some(format!("result:{id}")))
    }

    fn codex(&mut self, entry: &Value, timestamp: Option<String>) -> Vec<ActivityEvent> {
        if entry["type"].as_str() != Some("response_item") {
            return Vec::new();
        }
        let payload = &entry["payload"];
        match payload["type"].as_str() {
            Some("message")
                if payload["role"].as_str() == Some("assistant")
                    && is_visible_codex_message(payload) =>
            {
                let id = bounded_option(payload["id"].as_str(), ID_CHARS);
                if !self.first_seen(id.as_ref().map(|id| format!("message:{id}"))) {
                    return Vec::new();
                }
                let parts = payload["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|part| {
                        (part["type"].as_str() == Some("output_text"))
                            .then(|| part["text"].as_str())
                            .flatten()
                    });
                let mut text = String::new();
                let mut count = 0;
                let mut truncated = false;
                'parts: for part in parts {
                    if !text.is_empty() && !part.is_empty() {
                        if count == MESSAGE_CHARS {
                            truncated = true;
                            break;
                        }
                        text.push(' ');
                        count += 1;
                    }
                    for character in part.chars() {
                        if count == MESSAGE_CHARS {
                            truncated = true;
                            break 'parts;
                        }
                        text.push(character);
                        count += 1;
                    }
                }
                if text.trim().is_empty() {
                    return Vec::new();
                }
                vec![ActivityEvent {
                    kind: ActivityKind::Message,
                    timestamp,
                    name: None,
                    call_id: None,
                    text: text.trim_end().to_owned(),
                    truncated,
                    is_error: None,
                }]
            }
            Some("function_call" | "custom_tool_call") => {
                let id = bounded_option(payload["call_id"].as_str(), ID_CHARS);
                if !self.first_seen(id.as_ref().map(|id| format!("call:{id}"))) {
                    return Vec::new();
                }
                let name = bounded_option(payload["name"].as_str(), 80);
                let raw = payload["arguments"]
                    .as_str()
                    .or_else(|| payload["input"].as_str())
                    .unwrap_or("");
                vec![event(
                    ActivityKind::ToolCall,
                    timestamp,
                    name,
                    id,
                    raw,
                    CALL_CHARS,
                    None,
                )]
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                let id = bounded_option(payload["call_id"].as_str(), ID_CHARS);
                if !self.first_linked_result(id.as_deref()) {
                    return Vec::new();
                }
                let raw = payload["output"].as_str().unwrap_or("");
                vec![event(
                    ActivityKind::ToolResult,
                    timestamp,
                    None,
                    id,
                    raw,
                    RESULT_CHARS,
                    None,
                )]
            }
            _ => Vec::new(),
        }
    }

    fn claude(&mut self, entry: &Value, timestamp: Option<String>) -> Vec<ActivityEvent> {
        let role = entry["type"].as_str();
        let message = &entry["message"];
        let Some(parts) = message["content"].as_array() else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let kind = part["type"].as_str();
            let message_id = bounded_option(message["id"].as_str(), ID_CHARS);
            match (role, kind) {
                (Some("assistant"), Some("text")) => {
                    let key = message_id
                        .as_ref()
                        .map(|id| format!("message:{id}:{index}"));
                    if !self.first_seen(key) {
                        continue;
                    }
                    let raw = part["text"].as_str().unwrap_or("");
                    if !raw.is_empty() {
                        result.push(event(
                            ActivityKind::Message,
                            timestamp.clone(),
                            None,
                            None,
                            raw,
                            MESSAGE_CHARS,
                            None,
                        ));
                    }
                }
                (Some("assistant"), Some("tool_use")) => {
                    let id = bounded_option(part["id"].as_str(), ID_CHARS);
                    if !self.first_seen(id.as_ref().map(|id| format!("call:{id}"))) {
                        continue;
                    }
                    let name = bounded_option(part["name"].as_str(), 80);
                    let input = &part["input"];
                    let safe_wait_fields = wait_fields(input);
                    let raw = input["command"]
                        .as_str()
                        .or_else(|| input.as_str())
                        .unwrap_or(&safe_wait_fields);
                    result.push(event(
                        ActivityKind::ToolCall,
                        timestamp.clone(),
                        name,
                        id,
                        raw,
                        CALL_CHARS,
                        None,
                    ));
                }
                (Some("user"), Some("tool_result")) => {
                    let id = bounded_option(part["tool_use_id"].as_str(), ID_CHARS);
                    if !self.first_linked_result(id.as_deref()) {
                        continue;
                    }
                    let extracted = structured_result(&part["content"]);
                    result.push(event(
                        ActivityKind::ToolResult,
                        timestamp.clone(),
                        None,
                        id,
                        &extracted,
                        RESULT_CHARS,
                        part["is_error"].as_bool(),
                    ));
                }
                _ => {}
            }
        }
        result
    }
}

pub(crate) fn is_visible_codex_message(payload: &Value) -> bool {
    let channel = payload["channel"]
        .as_str()
        .or_else(|| payload["phase"].as_str());
    matches!(channel, Some("commentary" | "final" | "final_answer"))
        && payload["phase"].as_str() != Some("analysis")
        && payload["channel"].as_str() != Some("analysis")
}

fn structured_result(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.chars().take(RESULT_CHARS + 1).collect();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["type"].as_str() == Some("text"))
        .filter_map(|part| part["text"].as_str())
        .flat_map(str::chars)
        .take(RESULT_CHARS + 1)
        .collect()
}

fn wait_fields(input: &Value) -> String {
    let mut fields = serde_json::Map::new();
    for key in [
        "cell_id",
        "session_id",
        "target",
        "agent_id",
        "monitor_id",
        "job_id",
        "reservation_id",
        "yield_time_ms",
        "timeout_ms",
    ] {
        let value = &input[key];
        if let Some(text) = value.as_str().filter(|text| {
            text.len() <= 80
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        }) {
            fields.insert(key.to_owned(), Value::String(text.to_owned()));
        } else if let Some(number) = value.as_u64() {
            fields.insert(key.to_owned(), Value::from(number));
        }
    }
    if fields.is_empty() {
        String::new()
    } else {
        Value::Object(fields).to_string()
    }
}

fn bounded_option(value: Option<&str>, limit: usize) -> Option<String> {
    value
        .filter(|value| value.chars().count() <= limit)
        .map(str::to_owned)
}

fn event(
    kind: ActivityKind,
    timestamp: Option<String>,
    name: Option<String>,
    call_id: Option<String>,
    raw: &str,
    limit: usize,
    is_error: Option<bool>,
) -> ActivityEvent {
    let mut chars = raw.chars();
    let text = chars.by_ref().take(limit).collect();
    let truncated = chars.next().is_some();
    ActivityEvent {
        kind,
        timestamp,
        name,
        call_id,
        text,
        truncated,
        is_error,
    }
}

/// Bounded chronological evidence window. Tool outcomes remain linked
/// observations but do not advance the scheduling interval.
pub struct ActivityWindow {
    interval: usize,
    capacity: usize,
    since_check: usize,
    last_check: Option<std::time::Duration>,
    events: VecDeque<ActivityEvent>,
}

impl ActivityWindow {
    pub fn new(interval: usize, capacity: usize) -> Self {
        Self {
            interval: interval.clamp(1, 100),
            capacity: capacity.clamp(5, 100),
            since_check: 0,
            last_check: None,
            events: VecDeque::new(),
        }
    }

    /// Returns true when enough new eligible events have accumulated. The
    /// caller owns rate limits and calls `mark_checked_at` after admission.
    pub fn push(&mut self, event: ActivityEvent) -> bool {
        if event.counts_for_interval() {
            self.since_check = self.since_check.saturating_add(1);
        }
        self.events.push_back(event);
        // The bounded result allowance prevents tool outputs growing the
        // queue without limit, while preserving roughly one per action.
        while self.events.len() > self.capacity.saturating_mul(2) {
            self.events.pop_front();
        }
        while self
            .events
            .iter()
            .filter(|item| item.counts_for_interval())
            .count()
            > self.capacity
        {
            self.events.pop_front();
        }
        self.since_check >= self.interval
    }

    /// Lost continuity invalidates evidence and interval progress, but cannot
    /// erase the spacing established by an already admitted paid check.
    pub(crate) fn discard_evidence(&mut self) {
        self.events.clear();
        self.since_check = 0;
    }

    pub fn eligible_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| event.counts_for_interval())
            .count()
    }

    pub fn events(&self) -> impl Iterator<Item = &ActivityEvent> {
        self.events.iter()
    }

    /// Uses caller-supplied monotonic time; an observation does not itself
    /// reset the interval, so deferred events coalesce into the newest window.
    pub fn ready_at(&self, now: std::time::Duration, minimum_spacing: std::time::Duration) -> bool {
        self.since_check >= self.interval
            && self.last_check.is_none_or(|last| {
                now.checked_sub(last)
                    .is_some_and(|elapsed| elapsed >= minimum_spacing)
            })
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.since_check
    }

    /// The window owner validates the captured count and continuity first.
    pub(crate) fn mark_checked_through(&mut self, count: usize, now: std::time::Duration) {
        self.since_check = self.since_check.saturating_sub(count);
        self.last_check = Some(now);
    }

    /// Call only when the owner has made a local check or admitted a window.
    pub fn mark_checked_at(&mut self, now: std::time::Duration) {
        self.since_check = 0;
        self.last_check = Some(now);
    }
}

/// Local signals select candidates for model review; they are not verdicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSignal {
    RepeatedCheck,
    WaitContinuation,
    RepeatedUpdate,
}

pub fn candidate_signal(events: &[ActivityEvent]) -> Option<CandidateSignal> {
    for (index, event) in events.iter().enumerate() {
        if event.text.is_empty() || (event.truncated && event.kind != ActivityKind::ToolCall) {
            continue;
        }
        let repetitions = events[index..]
            .iter()
            .filter(|other| {
                other.kind == event.kind
                    && other.name == event.name
                    && (!other.truncated || event.kind == ActivityKind::ToolCall)
                    && (other.text == event.text
                        || (event.kind == ActivityKind::ToolCall
                            && !event.truncated
                            && !other.truncated
                            && normalized_call_shape(other) == normalized_call_shape(event))
                        || (event.kind == ActivityKind::Message
                            && similar_update(&event.text, &other.text)))
            })
            .count();
        if repetitions < 3 {
            continue;
        }
        match event.kind {
            ActivityKind::Message => return Some(CandidateSignal::RepeatedUpdate),
            ActivityKind::ToolCall => {
                let name = event.name.as_deref().unwrap_or("").to_ascii_lowercase();
                let operation = name
                    .rsplit("__")
                    .next()
                    .unwrap_or(&name)
                    .rsplit('.')
                    .next()
                    .unwrap_or(&name);
                if operation == "wait" || operation == "write_stdin" || operation == "wait_agent" {
                    return Some(CandidateSignal::WaitContinuation);
                }
                let text = event.text.to_ascii_lowercase();
                if text.contains("status")
                    || text.contains("progress check")
                    || text.contains("poll")
                {
                    return Some(CandidateSignal::RepeatedCheck);
                }
                // Unknown repeated calls need model review rather than a
                // claim that the command really checks an unfinished job.
                return Some(CandidateSignal::RepeatedCheck);
            }
            ActivityKind::ToolResult => {}
        }
    }
    None
}

/// Group wait continuations without treating their requested timeout as the
/// operation's identity. The caller must separately retain the requested time.
pub fn normalized_call_shape(event: &ActivityEvent) -> String {
    let name = event.name.as_deref().unwrap_or("");
    let operation = name
        .rsplit("__")
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or(name);
    if matches!(operation, "write_stdin" | "wait_agent" | "wait") {
        if let Ok(Value::Object(mut arguments)) = serde_json::from_str::<Value>(&event.text) {
            let has_target = [
                "session_id",
                "cell_id",
                "target",
                "agent_id",
                "monitor_id",
                "job_id",
            ]
            .iter()
            .any(|key| {
                arguments.get(*key).is_some_and(|value| {
                    value.as_str().is_some_and(|text| !text.is_empty()) || value.is_number()
                })
            });
            arguments.remove("yield_time_ms");
            arguments.remove("timeout_ms");
            arguments.remove("max_output_tokens");
            if has_target {
                return Value::Object(arguments).to_string();
            }
        }
    }
    event.text.clone()
}

fn similar_update(left: &str, right: &str) -> bool {
    let tokenize = |text: &str| -> HashSet<String> {
        text.split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let left_words = tokenize(left);
    let right_words = tokenize(right);
    if left_words.len() < 4 || right_words.len() < 4 {
        return false;
    }
    // Distinct numbers may identify distinct jobs or meaningful progress.
    let numeric = |words: &HashSet<String>| -> HashSet<String> {
        words
            .iter()
            .filter(|word| word.chars().any(|character| character.is_ascii_digit()))
            .cloned()
            .collect()
    };
    if numeric(&left_words) != numeric(&right_words) {
        return false;
    }
    let intersection = left_words.intersection(&right_words).count();
    intersection.saturating_mul(4) >= left_words.union(&right_words).count().saturating_mul(3)
}

/// One complete JSONL record, or an explicitly missing oversized record.
#[derive(Debug, PartialEq, Eq)]
pub enum FramedRecord {
    Record(Vec<u8>),
    Oversized,
}

/// Incremental framing with bounded partial-record memory. A caller persists
/// offsets only after consuming a complete record and fences file identity.
#[derive(Clone)]
pub struct ActivityFramer {
    maximum_line_bytes: usize,
    pending: Vec<u8>,
    oversized: bool,
}

impl ActivityFramer {
    pub(crate) fn retained_heap_bytes(&self) -> usize {
        self.pending.capacity()
    }

    pub(crate) fn at_record_boundary(&self) -> bool {
        self.pending.is_empty() && !self.oversized
    }

    pub fn new(maximum_line_bytes: usize) -> Self {
        Self {
            maximum_line_bytes,
            pending: Vec::new(),
            oversized: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<FramedRecord> {
        let mut records = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if self.oversized {
                    records.push(FramedRecord::Oversized);
                    self.oversized = false;
                } else {
                    records.push(FramedRecord::Record(std::mem::take(&mut self.pending)));
                }
            } else if !self.oversized {
                if self.pending.len() == self.maximum_line_bytes {
                    self.pending.clear();
                    self.oversized = true;
                } else {
                    self.pending.push(byte);
                }
            }
        }
        records
    }
}
