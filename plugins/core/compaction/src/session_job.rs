//! The `compact-session` job: compacts one agent session transcript in place.
//!
//! The job reads the transcript through the host `fs` import (read-only scope `agent-sessions:<agent>`), condenses the
//! older conversation with the pure engine of [`crate::remote`], and hands the result back to the host as two
//! `agent-config.propose` changes: first a `<transcript>.pre-compaction-<stamp>.bak` backup of the original, then the
//! replacement of the transcript, which the host applies only while the file still has the revision this job read
//! (the agent is stopped before the job runs, so a changed file means somebody else wrote to it). The oldest backups
//! beyond `keep_backups` are removed afterwards. Any error leaves the original transcript as it was.
//!
//! Progress goes two ways: the percent through `jobs.report-progress`, and the step, the log lines and the token
//! figures through the session storage key [`progress_key`], which the server flow reads on every progress event and
//! forwards to the progress dialog.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use ilium_plugin_sdk::host::{
    Bucket, ChatMessage, CompletionRequest, ConfigChange, EntryKind, HostClock, HostFs,
    HostInference, HostJobs, HostSettings, HostStorage,
};
use ilium_plugin_sdk::types::{ErrorCode, FsPath, PluginError};
use ilium_plugin_sdk::{host, HostApi};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::remote::{
    backup_file_name, backups_to_prune, compact_session, AgentKind, BackupEntry, CompactionError,
    CompactionEvent, CompactionRequest, Summarizer, SummarizerError, SummaryRequest,
    SummaryResponse, TokenBreakdown,
};
use crate::remote_settings::RemoteCompactionSettings;

/// The job kind the server flow starts.
pub const JOB_KIND: &str = "compact-session";
/// The most log lines the progress document keeps.
const LOG_LINES: usize = 8;
/// The most directory entries one listing of a project folder asks for (the host limit).
const LIST_ENTRIES: u32 = 4_096;
/// Wording of a provider answer that says the prompt does not fit the model.
const SIZE_PHRASES: [&str; 11] = [
    "context length",
    "context window",
    "context_length",
    "maximum context",
    "too long",
    "too large",
    "prompt is too",
    "exceeds the",
    "token limit",
    "max_tokens",
    "413",
];
/// The longest summary text one model answer may have.
const MAX_SUMMARY_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// The storage key under which the job publishes its progress for `pane`.
pub fn progress_key(pane: u64) -> String {
    format!("compaction/progress/{pane}")
}

/// What the server flow hands to the job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionJobInput {
    pub pane: u64,
    /// `claude` or `codex`.
    pub agent: String,
    /// The read scope of the transcript, `agent-sessions:<agent>`.
    pub scope: String,
    /// The transcript path inside that scope.
    pub path: String,
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
}

/// What the job answers: the figures the dialog and the toast show.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SessionJobOutput {
    pub before_context: u64,
    pub after_context: u64,
    pub window: Option<u64>,
    pub summary_chars: usize,
    pub chunks: usize,
    pub used_fallback: bool,
    pub redactions: usize,
    /// The file name of the backup of the original transcript.
    pub backup: String,
    /// How many older backups were removed.
    pub pruned: usize,
}

/// The agent kind named by a detection signature.
pub fn agent_kind(signature: &str) -> Option<AgentKind> {
    match signature {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

/// The path the agent-config scope of `agent` uses for a transcript that the session scope names `path`.
pub fn config_path(agent: AgentKind, session_path: &str) -> String {
    match agent {
        AgentKind::Claude => format!("projects/{session_path}"),
        AgentKind::Codex => format!("sessions/{session_path}"),
    }
}

/// The settings the user chose, repaired when out of range; the defaults when none are stored.
pub fn read_settings(api: &mut dyn HostApi) -> RemoteCompactionSettings {
    let stored = HostSettings::get_json(api);
    serde_json::from_str::<RemoteCompactionSettings>(&stored)
        .unwrap_or_default()
        .sanitized()
}

fn failed(message: impl Into<String>) -> PluginError {
    PluginError::new(ErrorCode::Internal, message.into())
}

/// The progress document a run of the job keeps current.
#[derive(Debug, Default)]
struct Progress {
    step: usize,
    step_total: usize,
    title: String,
    percent: f64,
    log: Vec<String>,
    tokens: TokenBreakdown,
}

impl Progress {
    fn apply(&mut self, event: CompactionEvent) {
        match event {
            CompactionEvent::Step {
                index,
                total,
                title,
            } => {
                self.step = index;
                self.step_total = total;
                self.title = title;
            }
            CompactionEvent::Progress(fraction) => {
                self.percent = (f64::from(fraction) * 100.0).clamp(0.0, 100.0);
            }
            CompactionEvent::Log(line) => {
                self.log.push(line);
                if self.log.len() > LOG_LINES {
                    self.log.remove(0);
                }
            }
            CompactionEvent::Tokens(tokens) => self.tokens = tokens,
        }
    }

    fn document(&self) -> Value {
        json!({
            "step": self.step,
            "step_total": self.step_total,
            "title": self.title,
            "percent": self.percent,
            "log": self.log,
            "tokens": tokens_document(&self.tokens),
        })
    }
}

/// The token figures of a breakdown as a JSON object.
pub fn tokens_document(tokens: &TokenBreakdown) -> Value {
    json!({
        "before_context": tokens.before_context,
        "conversation_total": tokens.conversation_total,
        "masked_savings": tokens.masked_savings,
        "summarizer_input": tokens.summarizer_input,
        "summarizer_output": tokens.summarizer_output,
        "tail_kept": tokens.tail_kept,
        "after_context": tokens.after_context,
        "window": tokens.window,
    })
}

fn publish(pane: u64, progress: &Progress) {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let bytes = progress.document().to_string().into_bytes();
    // A progress document that cannot be stored only makes the dialog less detailed.
    let _ = HostStorage::put(api, Bucket::Session, progress_key(pane), bytes);
    HostJobs::report_progress(api, progress.percent as f32);
}

/// A provider answer about an oversized prompt is free text, so its wording decides whether the engine re-chunks.
fn classify_error(error: &PluginError) -> SummarizerError {
    if error.code == ErrorCode::Cancelled {
        return SummarizerError::Cancelled;
    }
    let lowered = error.message.to_lowercase();
    if SIZE_PHRASES.iter().any(|phrase| lowered.contains(phrase)) {
        SummarizerError::ContextTooLong(error.message.clone())
    } else {
        SummarizerError::Failed(error.message.clone())
    }
}

/// The model call of the engine, over the host `inference` import.
struct HostSummarizer<'cancel> {
    cancel: &'cancel AtomicBool,
}

impl HostSummarizer<'_> {
    fn is_cancelled(&self) -> bool {
        if self.cancel.load(Ordering::SeqCst) {
            return true;
        }
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let requested = HostJobs::is_cancelled(api);
        if requested {
            self.cancel.store(true, Ordering::SeqCst);
        }
        requested
    }
}

impl Summarizer for HostSummarizer<'_> {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        if self.is_cancelled() {
            return Err(SummarizerError::Cancelled);
        }
        let completion = CompletionRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: request.system.clone(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: request.user.clone(),
                },
            ],
            max_output_tokens: Some(u32::try_from(request.max_output_tokens).unwrap_or(u32::MAX)),
            json_schema: None,
            purpose: format!("compaction {}", request.label),
        };
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let id =
            HostInference::complete(api, completion).map_err(|error| classify_error(&error))?;
        let completion = HostInference::take_completion(api, id).ok_or_else(|| {
            SummarizerError::Failed("the model returned no completion".to_string())
        })?;
        if completion.text.len() > MAX_SUMMARY_TEXT_BYTES {
            return Err(SummarizerError::Failed(format!(
                "summary response exceeded the {MAX_SUMMARY_TEXT_BYTES}-byte limit"
            )));
        }
        Ok(SummaryResponse {
            text: completion.text,
            input_tokens: Some(u64::from(completion.input_tokens)),
            output_tokens: Some(u64::from(completion.output_tokens)),
        })
    }
}

/// A short stable seed for the record ids of the rewritten transcript.
fn id_seed(session_id: &str, now_millis: i64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in session_id.bytes().chain(now_millis.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn describe(error: &CompactionError) -> PluginError {
    if matches!(error, CompactionError::Cancelled) {
        PluginError::new(ErrorCode::Cancelled, "compaction cancelled")
    } else {
        failed(error.to_string())
    }
}

/// The folder part and the file name of a scope path.
fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// Reads the whole transcript and the revision the host will compare before it replaces it.
fn read_transcript(scope: &str, path: &str) -> Result<(Vec<u8>, String), PluginError> {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let request = HostFs::read_with_revision(
        api,
        FsPath {
            scope: scope.to_string(),
            path: path.to_string(),
        },
    )?;
    let revisioned = HostFs::take_revisioned(api, request)
        .ok_or_else(|| failed("the host returned no transcript"))?;
    Ok((revisioned.bytes, revisioned.revision))
}

/// The files of the transcript's folder with their modification times.
fn list_folder(scope: &str, folder: &str) -> Vec<BackupEntry> {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    HostFs::list_dir(
        api,
        FsPath {
            scope: scope.to_string(),
            path: folder.to_string(),
        },
        LIST_ENTRIES,
    )
    .unwrap_or_default()
    .into_iter()
    .filter(|entry| entry.metadata.kind == EntryKind::File)
    .map(|entry| BackupEntry {
        name: entry.name,
        modified_millis: entry
            .metadata
            .modified_unix_millis
            .map_or(0, |millis| i64::try_from(millis).unwrap_or(i64::MAX)),
    })
    .collect()
}

/// Proposes `changes` to the configuration folder of `agent` and fails when any of them was rejected.
fn propose(agent: AgentKind, title: &str, changes: Vec<ConfigChange>) -> Result<(), PluginError> {
    let name = match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
    };
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let request = HostFs::propose_agent_config(api, name.to_string(), title.to_string(), changes)?;
    let result = HostFs::take_proposal(api, request)
        .ok_or_else(|| failed("the host returned no answer to the proposal"))?;
    if result.rejected.is_empty() {
        Ok(())
    } else {
        Err(failed(format!(
            "the transcript folder refused the change: {}",
            result.history.trim()
        )))
    }
}

/// The settings applied to one run: the options of the agent, with the summary budget of the technique.
fn request_for(
    input: &SessionJobInput,
    agent: AgentKind,
    settings: &RemoteCompactionSettings,
    transcript: Vec<u8>,
    now_millis: i64,
) -> CompactionRequest {
    CompactionRequest {
        agent,
        transcript_path: PathBuf::from(format!("{}/{}", input.scope, input.path)),
        transcript,
        session_id: input.session_id.clone(),
        project_cwd: PathBuf::from(&input.cwd),
        options: settings.options_for(agent),
        context_window_tokens: None,
        now_millis,
        id_seed: id_seed(&input.session_id, now_millis),
    }
}

/// Runs the job: `input` is a JSON [`SessionJobInput`]; the answer is a JSON [`SessionJobOutput`].
pub fn run(input: &[u8]) -> Result<Vec<u8>, PluginError> {
    let input: SessionJobInput = serde_json::from_slice(input)
        .map_err(|error| PluginError::invalid_argument(format!("bad job input: {error}")))?;
    let agent = agent_kind(&input.agent).ok_or_else(|| {
        PluginError::invalid_argument("only Claude Code and Codex can be compacted")
    })?;
    let (settings, now_millis) = {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        (
            read_settings(api),
            i64::try_from(HostClock::now_unix_millis(api)).unwrap_or(i64::MAX),
        )
    };
    let (original, revision) = read_transcript(&input.scope, &input.path)?;
    let request = request_for(&input, agent, &settings, original.clone(), now_millis);

    let cancel = AtomicBool::new(false);
    let summarizer = HostSummarizer { cancel: &cancel };
    let mut progress = Progress::default();
    let outcome = compact_session(&request, &summarizer, &cancel, &mut |event| {
        progress.apply(event);
        publish(input.pane, &progress);
        let _ = summarizer.is_cancelled();
    })
    .map_err(|error| describe(&error))?;
    if cancel.load(Ordering::SeqCst) {
        return Err(PluginError::new(
            ErrorCode::Cancelled,
            "compaction cancelled",
        ));
    }

    let (folder, file_name) = split_path(&input.path);
    let existing = list_folder(&input.scope, folder);
    let names: Vec<String> = existing.iter().map(|entry| entry.name.clone()).collect();
    let backup = backup_file_name(file_name, now_millis, &names);
    let backup_path = config_path(agent, &format!("{folder}/{backup}"));
    let transcript_path = config_path(agent, &input.path);
    propose(
        agent,
        "Back up the transcript before compaction",
        vec![ConfigChange {
            path: backup_path,
            content: Some(original),
            revision: None,
            summary: "Keep the transcript as it was before compaction".to_string(),
        }],
    )?;
    propose(
        agent,
        "Replace the transcript with its compacted form",
        vec![ConfigChange {
            path: transcript_path,
            content: Some(outcome.rewritten),
            revision: Some(revision),
            summary: "Compact the conversation".to_string(),
        }],
    )?;
    let pruned = prune_backups(&input, agent, file_name, folder, settings.keep_backups);

    let output = SessionJobOutput {
        before_context: outcome.tokens.before_context,
        after_context: outcome.tokens.after_context,
        window: outcome.tokens.window,
        summary_chars: outcome.summary_chars,
        chunks: outcome.chunks,
        used_fallback: outcome.used_fallback,
        redactions: outcome.redactions,
        backup,
        pruned,
    };
    serde_json::to_vec(&output).map_err(|error| failed(error.to_string()))
}

/// Removes the backups beyond `keep`, oldest first; a backup that cannot be removed stays and costs nothing but disk.
fn prune_backups(
    input: &SessionJobInput,
    agent: AgentKind,
    file_name: &str,
    folder: &str,
    keep: usize,
) -> usize {
    let listing = list_folder(&input.scope, folder);
    let doomed = backups_to_prune(file_name, &listing, keep);
    if doomed.is_empty() {
        return 0;
    }
    let changes = doomed
        .iter()
        .map(|name| ConfigChange {
            path: config_path(agent, &format!("{folder}/{name}")),
            content: None,
            revision: None,
            summary: "Remove an old compaction backup".to_string(),
        })
        .collect();
    match propose(agent, "Remove old compaction backups", changes) {
        Ok(()) => doomed.len(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_path_of_a_transcript_follows_the_agents_folder_layout() {
        assert_eq!(
            config_path(AgentKind::Claude, "-proj/abc.jsonl"),
            "projects/-proj/abc.jsonl"
        );
        assert_eq!(
            config_path(AgentKind::Codex, "2026/11/14/rollout-1.jsonl"),
            "sessions/2026/11/14/rollout-1.jsonl"
        );
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::only_claude_and_codex_map_to_a_transcript_format
    #[test]
    fn only_claude_and_codex_map_to_a_transcript_format() {
        assert_eq!(agent_kind("claude"), Some(AgentKind::Claude));
        assert_eq!(agent_kind("codex"), Some(AgentKind::Codex));
        assert_eq!(agent_kind("antigravity"), None);
        assert_eq!(agent_kind(""), None);
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::payload_too_large_asks_for_a_smaller_chunk
    #[test]
    fn a_payload_too_large_answer_asks_for_a_smaller_chunk() {
        let error = PluginError::new(ErrorCode::Internal, "http 413: payload too large");
        assert!(matches!(
            classify_error(&error),
            SummarizerError::ContextTooLong(_)
        ));
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::other_failures_stay_plain_failures
    #[test]
    fn other_failures_stay_plain_failures() {
        let error = PluginError::new(ErrorCode::Internal, "connection reset by peer");
        assert!(matches!(classify_error(&error), SummarizerError::Failed(_)));
        let cancelled = PluginError::new(ErrorCode::Cancelled, "stop");
        assert!(matches!(
            classify_error(&cancelled),
            SummarizerError::Cancelled
        ));
    }

    #[test]
    fn the_record_id_seed_depends_on_the_session_and_the_clock() {
        assert_eq!(id_seed("a", 1), id_seed("a", 1));
        assert_ne!(id_seed("a", 1), id_seed("a", 2));
        assert_ne!(id_seed("a", 1), id_seed("b", 1));
    }

    #[test]
    fn the_progress_document_keeps_the_latest_log_lines_and_the_step() {
        let mut progress = Progress::default();
        progress.apply(CompactionEvent::Step {
            index: 2,
            total: 4,
            title: "Preparing".to_string(),
        });
        progress.apply(CompactionEvent::Progress(0.5));
        for index in 0..(LOG_LINES + 3) {
            progress.apply(CompactionEvent::Log(format!("line {index}")));
        }
        let document = progress.document();
        assert_eq!(document["step"], 2);
        assert_eq!(document["step_total"], 4);
        assert_eq!(document["percent"], 50.0);
        assert_eq!(document["log"].as_array().map(Vec::len), Some(LOG_LINES));
        assert_eq!(
            document["log"][LOG_LINES - 1],
            format!("line {}", LOG_LINES + 2)
        );
    }

    #[test]
    fn the_folder_and_the_file_name_of_a_path_are_split_at_the_last_slash() {
        assert_eq!(split_path("-proj/abc.jsonl"), ("-proj", "abc.jsonl"));
        assert_eq!(split_path("abc.jsonl"), ("", "abc.jsonl"));
        assert_eq!(
            split_path("2026/11/14/rollout.jsonl"),
            ("2026/11/14", "rollout.jsonl")
        );
    }

    #[test]
    fn the_job_input_round_trips_through_json() {
        let input = SessionJobInput {
            pane: 4,
            agent: "claude".to_string(),
            scope: "agent-sessions:claude".to_string(),
            path: "-proj/abc.jsonl".to_string(),
            session_id: "abc".to_string(),
            cwd: "/work".to_string(),
        };
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(
            serde_json::from_str::<SessionJobInput>(&text).unwrap(),
            input
        );
    }
}
