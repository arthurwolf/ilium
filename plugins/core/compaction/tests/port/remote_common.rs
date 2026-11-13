//! Shared helpers of the remote-compaction twins: fixture bytes, a scripted fake summarizer, an in-memory stand-in
//! for the host's file handling (backup, stale check, write-back, pruning) and an independent reader that
//! validates rewritten transcripts without using the plugin's own verification.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ilium_plugin_core_compaction::remote::{
    backup_file_name, backups_to_prune, compact_session, transcript_snapshot, AgentKind,
    BackupEntry, CompactionError, CompactionEvent, CompactionOptions, CompactionOutcome,
    CompactionRequest, Summarizer, SummarizerError, SummaryRequest, SummaryResponse, Technique,
};
use serde_json::Value;

pub const CLAUDE_SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";
pub const CODEX_SESSION_ID: &str = "22222222-2222-4222-8222-222222222222";
pub const SECRET: &str = "sk-ABCDEFGHIJKLMNOPQRSTUV123456";
/// The wall clock the host hands the plugin in these tests (2026-10-05 12:00:00 UTC).
pub const NOW_MILLIS: i64 = 1_791_201_600_000;

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/remote-compaction")
        .join(name)
}

/// The transcript file name the agent would use, and its bytes.
pub fn fixture(agent: AgentKind) -> (String, Vec<u8>) {
    let (source, target) = match agent {
        AgentKind::Claude => ("claude_session.jsonl", format!("{CLAUDE_SESSION_ID}.jsonl")),
        AgentKind::Codex => (
            "codex_rollout.jsonl",
            format!("rollout-2026-10-05T10-00-00-{CODEX_SESSION_ID}.jsonl"),
        ),
    };
    let bytes = std::fs::read(fixture_path(source)).expect("read fixture");
    (target, bytes)
}

pub fn small_options(technique: Technique) -> CompactionOptions {
    CompactionOptions {
        technique,
        tail_tokens: 150,
        protected_recent_tool_tokens: 100,
        tool_result_chars: 300,
        summarizer_context_tokens: 100_000,
        summarizer_max_output_tokens: 500,
        ..CompactionOptions::default()
    }
}

pub fn request_for(
    agent: AgentKind,
    file_name: &str,
    bytes: &[u8],
    options: CompactionOptions,
) -> CompactionRequest {
    CompactionRequest {
        agent,
        transcript_path: PathBuf::from("/sessions").join(file_name),
        transcript: bytes.to_vec(),
        session_id: match agent {
            AgentKind::Claude => CLAUDE_SESSION_ID,
            AgentKind::Codex => CODEX_SESSION_ID,
        }
        .to_string(),
        project_cwd: PathBuf::from("/repo"),
        options,
        context_window_tokens: Some(200_000),
        now_millis: NOW_MILLIS,
        id_seed: 42,
    }
}

/// A well-formed reply for each technique.
pub fn good_reply(technique: Technique) -> String {
    match technique {
        Technique::ClaudeCode | Technique::BestOfAllWorlds => "<analysis>scratch</analysis>\n<summary>\n1. Primary Request and Intent:\n   FIRST SUMMARY MARKER add a verbose flag\n7. Pending Tasks:\n   - update the README\n8. Current Work:\n   reading README.md\n</summary>".to_string(),
        Technique::Opencode => "## Objective\n- FIRST SUMMARY MARKER add a verbose flag\n## Work State\n### Active\n- README\n## Next Move\n1. update README".to_string(),
        Technique::GeminiCli => "<scratchpad>x</scratchpad><state_snapshot><overall_goal>FIRST SUMMARY MARKER</overall_goal><task_state>README next</task_state></state_snapshot>".to_string(),
        Technique::Codex | Technique::Custom => "FIRST SUMMARY MARKER: the verbose flag is done and the README update is next.".to_string(),
    }
}

type Reply = Result<String, SummarizerError>;

/// Scripted summarizer. `on_call` runs inside every call (to simulate the agent appending to the transcript or the
/// user cancelling).
pub struct Fake<'a> {
    script: RefCell<VecDeque<Reply>>,
    default_reply: Reply,
    pub requests: RefCell<Vec<SummaryRequest>>,
    on_call: Option<Box<dyn Fn() + 'a>>,
}

impl<'a> Fake<'a> {
    pub fn always(reply: &str) -> Self {
        Self::scripted(Vec::new(), Ok(reply.to_string()))
    }

    pub fn failing() -> Self {
        Self::scripted(Vec::new(), Err(SummarizerError::Failed("offline".into())))
    }

    pub fn scripted(script: Vec<Reply>, default_reply: Reply) -> Self {
        Self {
            script: RefCell::new(script.into()),
            default_reply,
            requests: RefCell::new(Vec::new()),
            on_call: None,
        }
    }

    pub fn on_call(mut self, hook: impl Fn() + 'a) -> Self {
        self.on_call = Some(Box::new(hook));
        self
    }

    pub fn calls(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl Summarizer for Fake<'_> {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        self.requests.borrow_mut().push(request.clone());
        if let Some(hook) = &self.on_call {
            hook();
        }
        let reply = self
            .script
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| self.default_reply.clone());
        reply.map(|text| SummaryResponse {
            text,
            input_tokens: None,
            output_tokens: None,
        })
    }
}

pub struct Run {
    pub result: Result<CompactionOutcome, CompactionError>,
    pub events: Vec<CompactionEvent>,
}

pub fn run(request: &CompactionRequest, summarizer: &dyn Summarizer, cancel: &AtomicBool) -> Run {
    let mut events = Vec::new();
    let result = compact_session(request, summarizer, cancel, &mut |event| events.push(event));
    Run { result, events }
}

pub fn run_ok(
    request: &CompactionRequest,
    summarizer: &dyn Summarizer,
) -> (CompactionOutcome, Vec<CompactionEvent>) {
    let finished = run(request, summarizer, &AtomicBool::new(false));
    (
        finished.result.expect("compaction succeeds"),
        finished.events,
    )
}

/// Why the host refused to write a rewrite back.
#[derive(Debug, PartialEq, Eq)]
pub enum HostRefusal {
    /// The transcript no longer has the snapshot the rewrite was computed from.
    TranscriptChanged,
}

/// An in-memory stand-in for the host's `fs` calls around one transcript: backup, stale check, write-back and
/// pruning, driven only by the plugin's public helpers.
pub struct Disk {
    pub file_name: String,
    pub bytes: Vec<u8>,
    /// Backup files next to the transcript: name, modification time, content.
    pub backups: Vec<(BackupEntry, Vec<u8>)>,
}

impl Disk {
    pub fn new(file_name: &str, bytes: Vec<u8>) -> Self {
        Self {
            file_name: file_name.to_string(),
            bytes,
            backups: Vec::new(),
        }
    }

    pub fn fixture(agent: AgentKind) -> Self {
        let (name, bytes) = fixture(agent);
        Self::new(&name, bytes)
    }

    pub fn request(&self, agent: AgentKind, options: CompactionOptions) -> CompactionRequest {
        request_for(agent, &self.file_name, &self.bytes, options)
    }

    pub fn backup_names(&self) -> Vec<String> {
        self.backups
            .iter()
            .map(|(entry, _)| entry.name.clone())
            .collect()
    }

    /// Appends complete JSON lines the way a running agent does.
    pub fn append_lines(&mut self, lines: &[Value]) {
        if self.bytes.last().is_some_and(|byte| *byte != b'\n') {
            self.bytes.push(b'\n');
        }
        for line in lines {
            self.bytes.extend_from_slice(line.to_string().as_bytes());
            self.bytes.push(b'\n');
        }
    }

    /// What the host does with an outcome: refuse when the file changed since it was read, else back the original
    /// up, write the rewrite and prune old backups down to `keep_backups`.
    pub fn apply(
        &mut self,
        outcome: &CompactionOutcome,
        keep_backups: usize,
    ) -> Result<(), HostRefusal> {
        if transcript_snapshot(&self.bytes) != (outcome.source_len, outcome.source_last_line_hash) {
            return Err(HostRefusal::TranscriptChanged);
        }
        let name = backup_file_name(&self.file_name, NOW_MILLIS, &self.backup_names());
        let entry = BackupEntry {
            name,
            modified_millis: NOW_MILLIS,
        };
        self.backups.push((entry, std::mem::take(&mut self.bytes)));
        self.bytes = outcome.rewritten.clone();
        let entries: Vec<BackupEntry> = self
            .backups
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect();
        let pruned = backups_to_prune(&self.file_name, &entries, keep_backups);
        self.backups
            .retain(|(entry, _)| !pruned.contains(&entry.name));
        Ok(())
    }
}

pub fn parse_lines(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).expect("utf-8");
    text.lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| panic!("bad line {line:?}: {error}"))
        })
        .collect()
}

pub fn uuid_of(value: &Value) -> Option<&str> {
    value.get("uuid").and_then(Value::as_str)
}

/// Independent Claude validation: walks `parentUuid` from the newest record to the last boundary and returns the
/// chain (oldest first).
pub fn claude_active_chain(records: &[Value]) -> Vec<&Value> {
    let boundary_position = records
        .iter()
        .rposition(|record| record["subtype"] == "compact_boundary")
        .expect("a boundary exists");
    let by_uuid: std::collections::HashMap<&str, &Value> = records
        .iter()
        .filter_map(|record| uuid_of(record).map(|uuid| (uuid, record)))
        .collect();
    let mut chain = vec![records.last().expect("records")];
    while chain.last().map(|record| uuid_of(record)) != Some(uuid_of(&records[boundary_position])) {
        let parent = chain.last().expect("chain")["parentUuid"]
            .as_str()
            .expect("parentUuid is set before the boundary");
        chain.push(
            by_uuid
                .get(parent)
                .unwrap_or_else(|| panic!("missing parent {parent}")),
        );
        assert!(chain.len() <= records.len(), "cycle in the parent chain");
    }
    chain.reverse();
    chain
}

/// Every tool_use of the chain has exactly one tool_result and the reverse.
pub fn assert_tool_pairs_complete(chain: &[&Value]) {
    let mut used = Vec::new();
    let mut answered = Vec::new();
    for record in chain {
        for block in record["message"]["content"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            match block["type"].as_str() {
                Some("tool_use") => used.push(block["id"].as_str().expect("id").to_string()),
                Some("tool_result") => {
                    answered.push(block["tool_use_id"].as_str().expect("id").to_string())
                }
                _ => {}
            }
        }
    }
    used.sort();
    answered.sort();
    assert_eq!(used, answered, "every tool_use needs one tool_result");
}
