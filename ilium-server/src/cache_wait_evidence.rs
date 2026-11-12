//! Derive narrow wait evidence from an already verified agent transcript.
//!
//! A caller must associate the transcript with the current live process and
//! discard this state when that identity changes. This parser does not read
//! files, keep prompts or command bodies, or establish provider-cache identity.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ilium_core::AgentClass;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitKind {
    Monitor,
    Command,
    Subagent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WaitEvidence {
    pub monitor: bool,
    pub command: bool,
    pub subagent: bool,
}

/// Outstanding calls, keyed only by provider-issued tool call identifiers.
#[derive(Default)]
pub(crate) struct WaitTracker {
    calls: HashMap<String, WaitKind>,
    invalid: bool,
}

impl WaitTracker {
    pub fn observe(&mut self, provider: &AgentClass, entry: &Value) {
        match provider {
            AgentClass::Codex => self.observe_codex(entry),
            AgentClass::Claude => self.observe_claude(entry),
            _ => self.reset(),
        }
    }

    pub fn evidence(&self) -> WaitEvidence {
        if self.invalid {
            return WaitEvidence::default();
        }
        WaitEvidence {
            monitor: self.calls.values().any(|kind| *kind == WaitKind::Monitor),
            command: self.calls.values().any(|kind| *kind == WaitKind::Command),
            subagent: self.calls.values().any(|kind| *kind == WaitKind::Subagent),
        }
    }

    fn begin(&mut self, id: Option<&str>, kind: Option<WaitKind>) {
        if let (Some(id), Some(kind)) = (id.filter(|id| !id.is_empty()), kind) {
            // A malformed or unexpectedly large transcript cannot grow this
            // state indefinitely or leave an apparently valid partial set.
            if id.len() > 256 || (self.calls.len() >= 4096 && !self.calls.contains_key(id)) {
                self.calls.clear();
                self.invalid = true;
                return;
            }
            if self.invalid {
                return;
            }
            self.calls.insert(id.to_owned(), kind);
        }
    }

    fn reset(&mut self) {
        self.calls.clear();
        self.invalid = false;
    }

    fn end(&mut self, id: Option<&str>) {
        if let Some(id) = id {
            self.calls.remove(id);
        }
    }

    fn observe_codex(&mut self, entry: &Value) {
        let payload = &entry["payload"];
        match (entry["type"].as_str(), payload["type"].as_str()) {
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let kind = match payload["name"].as_str() {
                    Some("ilium_wait") => Some(WaitKind::Monitor),
                    Some("exec_command" | "write_stdin") => Some(WaitKind::Command),
                    Some("wait_agent") => Some(WaitKind::Subagent),
                    _ => None,
                };
                self.begin(payload["call_id"].as_str(), kind);
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                self.end(payload["call_id"].as_str());
            }
            (
                Some("event_msg"),
                Some("task_started" | "task_complete" | "turn_complete" | "turn_aborted"),
            ) => {
                self.reset();
            }
            (Some("response_item"), Some("message"))
                if payload["role"].as_str() == Some("user") =>
            {
                self.reset()
            }
            _ => {}
        }
    }

    fn observe_claude(&mut self, entry: &Value) {
        let content_value = &entry["message"]["content"];
        if entry["type"].as_str() == Some("user") && content_value.is_string() {
            self.reset();
            return;
        }
        let Some(content) = content_value.as_array() else {
            return;
        };
        match entry["type"].as_str() {
            Some("assistant") => {
                for item in content {
                    if item["type"].as_str() != Some("tool_use") {
                        continue;
                    }
                    let kind = match item["name"].as_str() {
                        Some("Bash") => Some(WaitKind::Command),
                        Some("Agent" | "Task") => Some(WaitKind::Subagent),
                        _ => None,
                    };
                    self.begin(item["id"].as_str(), kind);
                }
            }
            Some("user") => {
                // A new human message interrupts an outstanding turn. Tool
                // results are also user messages, but close only their ID.
                if content
                    .iter()
                    .any(|item| item["type"].as_str() == Some("text"))
                {
                    self.reset();
                }
                for item in content {
                    if item["type"].as_str() == Some("tool_result") {
                        self.end(item["tool_use_id"].as_str());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Incremental reader for one process-verified transcript. The caller must
/// bind this to the invocation generation, recheck the process before using
/// evidence, and run file I/O on its admitted blocking-I/O lane.
pub(crate) struct WaitTranscriptCursor {
    path: PathBuf,
    session_id: String,
    provider: AgentClass,
    generation: u64,
    file_identity: Option<(u64, u64)>,
    offset: u64,
    pending: Vec<u8>,
    tracker: WaitTracker,
    failed: bool,
    caught_up: bool,
}

impl WaitTranscriptCursor {
    pub fn new(path: PathBuf, session_id: String, provider: AgentClass, generation: u64) -> Self {
        Self {
            path,
            session_id,
            provider,
            generation,
            file_identity: None,
            offset: 0,
            pending: Vec::new(),
            tracker: WaitTracker::default(),
            failed: false,
            caught_up: false,
        }
    }

    pub fn caught_up(&self) -> bool {
        self.caught_up
    }

    /// Read no more than 256 KiB per call. None means incomplete or unsafe
    /// evidence, not that the agent is idle. Never retain or log parsed text.
    pub fn read_available(
        &mut self,
        generation: u64,
        verified_path: &Path,
    ) -> io::Result<Option<WaitEvidence>> {
        if self.failed || generation != self.generation || verified_path != self.path.as_path() {
            self.failed = true;
            self.pending.clear();
            self.tracker.reset();
            return Ok(None);
        }
        let result = self.read_inner();
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
            self.tracker.reset();
        }
        result
    }

    fn read_inner(&mut self) -> io::Result<Option<WaitEvidence>> {
        const CHUNK_BYTES: u64 = 256 * 1024;
        const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
        let mut file = ilium_platform::secure_fs::open_regular_file(&self.path)?;
        let metadata = file.metadata()?;
        let identity = transcript_file_identity(&metadata)
            .ok_or_else(|| io::Error::other("transcript identity unavailable"))?;
        if self
            .file_identity
            .is_some_and(|previous| previous != identity)
            || metadata.len() < self.offset
        {
            return Err(io::Error::other("transcript replaced or truncated"));
        }
        self.file_identity = Some(identity);
        let initial_len = metadata.len();
        file.seek(SeekFrom::Start(self.offset))?;
        let mut chunk = Vec::new();
        file.by_ref().take(CHUNK_BYTES).read_to_end(&mut chunk)?;
        self.offset = self.offset.saturating_add(chunk.len() as u64);
        for byte in chunk {
            if byte == b'\n' {
                let entry: Value = serde_json::from_slice(&self.pending)
                    .map_err(|_| io::Error::other("invalid transcript record"))?;
                self.pending.clear();
                if self.provider == AgentClass::Claude {
                    if entry
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id != self.session_id)
                    {
                        return Err(io::Error::other("transcript session changed"));
                    }
                    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                }
                self.tracker.observe(&self.provider, &entry);
            } else {
                if self.pending.len() >= MAX_RECORD_BYTES {
                    return Err(io::Error::other("transcript record exceeds limit"));
                }
                self.pending.push(byte);
            }
        }
        // Do not publish a partial scan, a partial record, or an observation
        // made while a writer was appending. Reopen to detect path replacement.
        let current = ilium_platform::secure_fs::open_regular_file(&self.path)?.metadata()?;
        if transcript_file_identity(&current) != Some(identity) {
            return Err(io::Error::other("transcript replaced during read"));
        }
        self.caught_up = self.offset >= current.len();
        if self.offset == initial_len && current.len() == initial_len && self.pending.is_empty() {
            Ok(Some(self.tracker.evidence()))
        } else {
            Ok(None)
        }
    }
}

#[cfg(unix)]
fn transcript_file_identity(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn transcript_file_identity(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    #[test]
    fn transcript_cursor_withholds_partial_records_and_fences_generation() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        std::fs::write(&path, b"{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"ilium_wait\",\"call_id\":\"a\"}}\n{\"type\":").unwrap();
        let mut cursor =
            WaitTranscriptCursor::new(path.clone(), "session".into(), AgentClass::Codex, 7);
        assert_eq!(cursor.read_available(7, &path).unwrap(), None);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"\"event_msg\",\"payload\":{\"type\":\"other\"}}\n")
            .unwrap();
        assert!(cursor.read_available(7, &path).unwrap().unwrap().monitor);
        assert_eq!(cursor.read_available(8, &path).unwrap(), None);
        assert_eq!(cursor.read_available(7, &path).unwrap(), None);
    }

    #[test]
    fn codex_tracks_parallel_explicit_calls_until_their_own_results() {
        let mut tracker = WaitTracker::default();
        for (id, name) in [("a", "ilium_wait"), ("b", "wait_agent"), ("c", "exec")] {
            tracker.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","call_id":id,"name":name}}));
        }
        assert_eq!(
            tracker.evidence(),
            WaitEvidence {
                monitor: true,
                command: false,
                subagent: true
            }
        );
        tracker.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"a"}}));
        assert_eq!(
            tracker.evidence(),
            WaitEvidence {
                monitor: false,
                command: false,
                subagent: true
            }
        );
        tracker.observe(
            &AgentClass::Codex,
            &json!({"type":"event_msg","payload":{"type":"turn_aborted"}}),
        );
        assert_eq!(tracker.evidence(), WaitEvidence::default());
    }

    #[test]
    fn claude_pairs_results_and_ignores_duplicate_api_request_blobs() {
        let mut tracker = WaitTracker::default();
        let call = json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Bash"},{"type":"tool_use","id":"b","name":"Agent"}]}});
        tracker.observe(&AgentClass::Claude, &call);
        tracker.observe(&AgentClass::Claude, &json!({"type":"api-request-blob","message":{"content":[{"type":"tool_result","tool_use_id":"a"}]}}));
        assert_eq!(
            tracker.evidence(),
            WaitEvidence {
                monitor: false,
                command: true,
                subagent: true
            }
        );
        tracker.observe(&AgentClass::Claude, &json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a"}]}}));
        assert_eq!(
            tracker.evidence(),
            WaitEvidence {
                monitor: false,
                command: false,
                subagent: true
            }
        );
        tracker.observe(
            &AgentClass::Claude,
            &json!({"type":"user","message":{"content":[{"type":"text","text":"stop"}]}}),
        );
        assert_eq!(tracker.evidence(), WaitEvidence::default());
    }

    #[test]
    fn interruption_and_new_turn_clear_stale_calls() {
        let mut tracker = WaitTracker::default();
        let call = json!({"type":"response_item","payload":{"type":"function_call","call_id":"a","name":"ilium_wait"}});
        tracker.observe(&AgentClass::Codex, &call);
        tracker.observe(
            &AgentClass::Codex,
            &json!({"type":"response_item","payload":{"type":"message","role":"user"}}),
        );
        assert_eq!(tracker.evidence(), WaitEvidence::default());
        tracker.observe(&AgentClass::Codex, &call);
        tracker.observe(
            &AgentClass::Codex,
            &json!({"type":"event_msg","payload":{"type":"task_started"}}),
        );
        assert_eq!(tracker.evidence(), WaitEvidence::default());
    }

    #[test]
    fn oversized_identifier_suppresses_evidence_until_a_new_turn() {
        let mut tracker = WaitTracker::default();
        tracker.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","call_id":"x".repeat(257),"name":"ilium_wait"}}));
        tracker.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","call_id":"valid","name":"ilium_wait"}}));
        assert_eq!(tracker.evidence(), WaitEvidence::default());
        tracker.observe(
            &AgentClass::Codex,
            &json!({"type":"event_msg","payload":{"type":"task_started"}}),
        );
        tracker.observe(&AgentClass::Codex, &json!({"type":"response_item","payload":{"type":"function_call","call_id":"valid","name":"ilium_wait"}}));
        assert!(tracker.evidence().monitor);
    }
}
