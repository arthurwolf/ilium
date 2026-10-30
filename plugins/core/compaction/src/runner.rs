//! The `compact` job runner: splits the transcript into chunks, summarises each through the host inference call,
//! merges the parts and falls back to an offline summary when the model keeps failing (manual page, "Job runner").

use ilium_plugin_sdk::generated::host::{ChatMessage, CompletionRequest};
use ilium_plugin_sdk::host::{Bucket, HostInference, HostJobs, HostSettings, HostStorage};
use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{ErrorCode, NodeId, PluginError};
use ilium_plugin_sdk::{host, HostApi};
use ilium_remote_compaction::{
    clean_summary, render_merge_prompt, render_prompt, validate_summary, PromptInputs, Technique,
};
use serde_json::Value;

use crate::redact::redact;

/// Bytes of transcript text per model call.
pub const CHUNK_BYTES: usize = 32 * 1024;
/// Model calls per chunk before the offline fallback is used.
const ATTEMPTS: usize = 3;
/// Output budget of one model call.
const MAX_OUTPUT_TOKENS: u32 = 16_000;
/// Storage key a test host uses to say after how many chunks a cancel request counts.
const CANCEL_AFTER_CHUNKS_KEY: &str = "compaction.cancel-after-chunks";
/// Lines kept from each end of the transcript by the offline fallback.
const FALLBACK_LINES: usize = 12;

/// The summariser settings the runner reads from `remote_compaction.*`.
struct RunnerSettings {
    technique: Technique,
    custom_prompt: Option<String>,
    redact_secrets: bool,
}

fn setting_text(settings: &Value, name: &str) -> Option<String> {
    settings
        .get(format!("remote_compaction.{name}"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The technique row for the agent whose signature is `signature`.
fn technique_row(signature: Option<&str>) -> &'static str {
    match signature {
        Some("claude") => "claude_technique",
        Some("codex") => "codex_technique",
        _ => "other_technique",
    }
}

fn read_settings(api: &mut dyn HostApi, focused: Option<NodeId>) -> RunnerSettings {
    let json = HostSettings::get_json(api);
    let settings: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
    let signature = focused
        .and_then(|pane| HostAgent::view(api, pane))
        .map(|view| view.signature);
    let row = technique_row(signature.as_deref());
    let default = if row == "codex_technique" { "codex" } else { "claude_code" };
    let technique = setting_text(&settings, row)
        .and_then(|id| Technique::from_id(&id))
        .or_else(|| Technique::from_id(default))
        .unwrap_or(Technique::ClaudeCode);
    RunnerSettings {
        technique,
        custom_prompt: setting_text(&settings, "custom_prompt"),
        redact_secrets: settings
            .get("remote_compaction.redact_secrets")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    }
}

/// One model call; `None` when the host reports a failure or returns no completion.
fn ask(api: &mut dyn HostApi, system: String, user: String, purpose: &str) -> Option<String> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage { role: "system".to_string(), content: system },
            ChatMessage { role: "user".to_string(), content: user },
        ],
        max_output_tokens: Some(MAX_OUTPUT_TOKENS),
        json_schema: None,
        purpose: purpose.to_string(),
    };
    let id = HostInference::complete(api, request).ok()?;
    HostInference::take_completion(api, id).map(|completion| completion.text)
}

/// Asks until the cleaned reply passes the technique's validation, at most `ATTEMPTS` times.
fn ask_validated(
    api: &mut dyn HostApi,
    settings: &RunnerSettings,
    purpose: &str,
    render: &dyn Fn(Option<&str>) -> Result<(String, String), PluginError>,
) -> Option<String> {
    let mut problem: Option<String> = None;
    for _ in 0..ATTEMPTS {
        let (system, user) = render(problem.as_deref()).ok()?;
        let Some(raw) = ask(api, system, user, purpose) else {
            problem = Some("the previous request failed".to_string());
            continue;
        };
        let cleaned = clean_summary(settings.technique, &raw);
        match validate_summary(settings.technique, &cleaned) {
            Ok(()) => return Some(cleaned),
            Err(reason) => problem = Some(reason),
        }
    }
    None
}

/// Splits `text` into pieces of at most `limit` bytes on character boundaries.
fn split_chunks(text: &str, limit: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = limit.min(rest.len());
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = rest.split_at(end);
        chunks.push(head);
        rest = tail;
    }
    chunks
}

/// A summary built without a model: the first and last lines of the transcript.
fn offline_summary(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    let (head, tail): (&[&str], &[&str]) = if lines.len() <= 2 * FALLBACK_LINES {
        (&lines, &[])
    } else {
        (&lines[..FALLBACK_LINES], &lines[lines.len() - FALLBACK_LINES..])
    };
    let mut summary = String::from("Offline summary (the model could not be reached). Start of the conversation:\n");
    summary.push_str(&head.join("\n"));
    if !tail.is_empty() {
        summary.push_str("\n...\nEnd of the conversation:\n");
        summary.push_str(&tail.join("\n"));
    }
    summary
}

/// Chunk count after which a pending cancel request takes effect (a test host sets it; production hosts do not).
fn cancel_after_chunks(api: &mut dyn HostApi) -> usize {
    HostStorage::get(api, Bucket::Global, CANCEL_AFTER_CHUNKS_KEY.to_string())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

fn cancelled() -> PluginError {
    PluginError::new(ErrorCode::Cancelled, "compaction stopped")
}

/// Runs the `compact` job over `input` and returns the summary text.
pub fn run_compact(input: &[u8], focused: Option<NodeId>) -> Result<Vec<u8>, PluginError> {
    let mut handle = host();
    let api: &mut dyn HostApi = &mut *handle;
    let settings = read_settings(api, focused);
    let mut text = String::from_utf8_lossy(input).into_owned();
    if settings.redact_secrets {
        text = redact(&text);
    }
    let chunks = split_chunks(&text, CHUNK_BYTES);
    let total = chunks.len().max(1);
    let after = cancel_after_chunks(api);
    let custom = settings.custom_prompt.as_deref();
    let mut partials = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        if index >= after && HostJobs::is_cancelled(api) {
            return Err(cancelled());
        }
        let label = format!("part {} of {total}", index + 1);
        let part_label = (total > 1).then_some(label.as_str());
        let render = |problem: Option<&str>| {
            render_prompt(
                settings.technique,
                &PromptInputs {
                    conversation: chunk,
                    custom_prompt: custom,
                    part_label,
                    retry_problem: problem,
                    ..PromptInputs::default()
                },
            )
            .map(|prompt| (prompt.system, prompt.user))
            .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
        };
        let summary = ask_validated(api, &settings, "compaction summary", &render)
            .unwrap_or_else(|| offline_summary(chunk));
        partials.push(summary);
        HostJobs::report_progress(api, (100.0 * (index + 1) as f32) / total as f32);
    }
    let summary = merge_partials(api, &settings, partials);
    Ok(summary.into_bytes())
}

/// One summary from the parts: the part itself, or the merge call's answer, or the parts joined when the merge fails.
fn merge_partials(api: &mut dyn HostApi, settings: &RunnerSettings, partials: Vec<String>) -> String {
    if partials.len() <= 1 {
        return partials.into_iter().next().unwrap_or_else(|| offline_summary(""));
    }
    let custom = settings.custom_prompt.as_deref();
    let render = |problem: Option<&str>| {
        render_merge_prompt(settings.technique, &partials, custom, problem)
            .map(|prompt| (prompt.system, prompt.user))
            .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
    };
    ask_validated(api, settings, "compaction merge", &render).unwrap_or_else(|| partials.join("\n\n"))
}
