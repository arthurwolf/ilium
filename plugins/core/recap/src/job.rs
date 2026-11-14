//! The `recap-pane` job: everything that needs the host while one pane is recapped. The job reads
//! the evidence (a transcript of the pane's verified session through the `agent-sessions` file
//! scope, or the visible screen), asks the model once for both lengths and returns the finished
//! `recap` record. Input `{"pane": <id>, "length": "short"|"long"|"both"}`; input
//! `{"evidence": "<prompt>"}` is the bare model call and returns the completion text.

use std::cell::RefCell;

use ilium_plugin_sdk::host::{
    ChatMessage, CompletionRequest, EntryKind, HostFs, HostInference, HostJobs, HostTree,
};
use ilium_plugin_sdk::primitives::{HostAgent, HostTerminal, ScreenPart, SessionRef};
use ilium_plugin_sdk::types::{ErrorCode, FsPath, NodeId, NodeView, PluginError};
use ilium_plugin_sdk::HostApi;
use serde_json::Value;

use crate::custody::redact_secrets;
use crate::evidence::ByteSource;
use crate::fence::{PaneFence, RecapCache};
use crate::model::{PaneRecap, RecapPaneState};
use crate::pipeline::{
    empty_facts, lengths_for, recap_pane, PaneEnv, PaneInput, Source, SYSTEM_PROMPT,
};
use crate::render::pane_record;
use crate::transcript::AgentKind;

/// How long one model call may take before the pane fails. The job wall-clock limit is 20 s.
const MODEL_WAIT_MILLIS: u32 = 18_000;
/// Rows of the visible screen read for a shell pane.
const SCREEN_ROWS: u32 = 40;
/// Bytes of one transcript read.
const READ_CHUNK_BYTES: u32 = 1024 * 1024;
const PROJECT_SCOPE: &str = "project";

thread_local! {
    static CACHE: RefCell<RecapCache> = RefCell::new(RecapCache::default());
}

fn invalid(message: &str) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

/// Runs the job `kind`; `recap-pane` is the only kind.
pub fn run(api: &mut dyn HostApi, kind: &str, input: &[u8]) -> Result<Vec<u8>, PluginError> {
    if kind != "recap-pane" {
        return Err(PluginError::new(
            ErrorCode::Unsupported,
            format!("ilium.core.recap: unknown job kind {kind}"),
        ));
    }
    let value: Value =
        serde_json::from_slice(input).map_err(|_| invalid("the job input is not JSON"))?;
    if let Some(evidence) = value.get("evidence").and_then(Value::as_str) {
        HostJobs::report_progress(api, 10.0);
        if HostJobs::is_cancelled(api) {
            return Err(PluginError::new(ErrorCode::Cancelled, "run cancelled"));
        }
        let text = complete(api, evidence).map_err(|failure| match failure {
            ModelFailure::Cancelled => PluginError::new(ErrorCode::Cancelled, "run cancelled"),
            ModelFailure::Failed(message) => PluginError::new(ErrorCode::Internal, message),
        })?;
        HostJobs::report_progress(api, 100.0);
        return Ok(text.into_bytes());
    }
    let pane = value
        .get("pane")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("the job input needs a pane id"))?;
    let length = value
        .get("length")
        .and_then(Value::as_str)
        .unwrap_or("both");
    let lengths =
        lengths_for(length).ok_or_else(|| invalid("length must be short, long or both"))?;
    let recap = recap_for(api, NodeId(pane));
    let record = pane_record(&recap, &lengths);
    serde_json::to_vec(&record)
        .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
}

enum ModelFailure {
    Cancelled,
    Failed(String),
}

/// One model call for the prompt `user`.
fn complete(api: &mut dyn HostApi, user: &str) -> Result<String, ModelFailure> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: SYSTEM_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user.to_string(),
            },
        ],
        max_output_tokens: None,
        json_schema: Some(RESPONSE_SCHEMA.to_string()),
        purpose: "recap".to_string(),
    };
    let id = HostInference::complete(api, request).map_err(|error| {
        ModelFailure::Failed(format!("model provider failed: {}", error.message))
    })?;
    if let Err(error) = HostJobs::wait_request(api, id, MODEL_WAIT_MILLIS) {
        if HostJobs::is_cancelled(api) {
            return Err(ModelFailure::Cancelled);
        }
        return Err(ModelFailure::Failed(match error.code {
            ErrorCode::TimedOut => "model call timed out".to_string(),
            _ => format!("model provider failed: {}", error.message),
        }));
    }
    if HostJobs::is_cancelled(api) {
        return Err(ModelFailure::Cancelled);
    }
    HostInference::take_completion(api, id)
        .map(|completion| completion.text)
        .ok_or_else(|| ModelFailure::Failed("model provider failed: no completion".to_string()))
}

/// The JSON shape the model must answer with: both lengths in one object.
const RESPONSE_SCHEMA: &str = r#"{"type":"object","required":["short","long"],"properties":{"short":{"type":"object","required":["text"],"properties":{"text":{"type":"string"},"pointers":{"type":"array","items":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"note":{"type":"string"}}}}}},"long":{"type":"object","required":["text"],"properties":{"text":{"type":"string"},"pointers":{"type":"array","items":{"type":"object","required":["path"],"properties":{"path":{"type":"string"},"note":{"type":"string"}}}}}}}}"#;

/// A transcript read through the host file scope. The api sits in a `RefCell` because the
/// pipeline holds this source and the environment at the same time.
struct HostTranscript<'a, 'b> {
    api: &'a RefCell<&'b mut dyn HostApi>,
    file: FsPath,
    length: u64,
}

impl ByteSource for HostTranscript<'_, '_> {
    fn length(&self) -> Result<u64, String> {
        Ok(self.length)
    }

    fn read_at(&self, offset: u64, max: usize) -> Result<Vec<u8>, String> {
        let wanted = u32::try_from(max).unwrap_or(u32::MAX).min(READ_CHUNK_BYTES);
        let mut guard = self.api.borrow_mut();
        HostFs::read(&mut **guard, self.file.clone(), offset, wanted).map_err(|error| error.message)
    }
}

struct JobEnv<'a, 'b> {
    api: &'a RefCell<&'b mut dyn HostApi>,
    pane: NodeId,
    is_agent: bool,
}

impl PaneEnv for JobEnv<'_, '_> {
    fn complete(&mut self, prompt: &str) -> Result<String, String> {
        let mut guard = self.api.borrow_mut();
        complete(&mut **guard, prompt).map_err(|failure| match failure {
            ModelFailure::Cancelled => "cancelled".to_string(),
            ModelFailure::Failed(message) => message,
        })
    }

    fn cancelled(&mut self) -> bool {
        HostJobs::is_cancelled(&mut **self.api.borrow_mut())
    }

    fn is_file(&mut self, relative: &str) -> bool {
        let path = FsPath {
            scope: PROJECT_SCOPE.to_string(),
            path: relative.to_string(),
        };
        HostFs::metadata(&mut **self.api.borrow_mut(), path)
            .is_ok_and(|metadata| metadata.kind == EntryKind::File)
    }

    fn current_fence(&mut self) -> Option<PaneFence> {
        let mut guard = self.api.borrow_mut();
        let api: &mut dyn HostApi = &mut **guard;
        HostTree::get(api, self.pane)?;
        Some(PaneFence {
            pane_id: self.pane.0.to_string(),
            session_id: if self.is_agent {
                HostAgent::session(api, self.pane).map(|session| session.session_id)
            } else {
                None
            },
        })
    }

    fn progress(&mut self, percent: f32) {
        HostJobs::report_progress(&mut **self.api.borrow_mut(), percent);
    }
}

fn agent_kind(signature: &str) -> AgentKind {
    let name = signature.to_ascii_lowercase();
    if name.contains("claude") {
        AgentKind::Claude
    } else if name.contains("codex") {
        AgentKind::Codex
    } else {
        AgentKind::Other
    }
}

fn without_text(pane: NodeId, name: &str, state: RecapPaneState, reason: &str) -> PaneRecap {
    PaneRecap {
        pane_id: pane.0.to_string(),
        pane_name: name.to_string(),
        state,
        facts: empty_facts(),
        short: None,
        long: None,
        failure: Some(reason.to_string()),
    }
}

/// Gathers the host facts of `pane` and runs the pipeline.
fn recap_for(api: &mut dyn HostApi, pane: NodeId) -> PaneRecap {
    let Some(view) = HostTree::get(api, pane) else {
        return without_text(
            pane,
            "",
            RecapPaneState::Unavailable,
            "the pane no longer exists",
        );
    };
    let name = redact_secrets(&view.title).text;
    let cwd = view.cwd.clone().unwrap_or_default();
    let session = view
        .agent
        .as_ref()
        .and_then(|_| HostAgent::session(api, pane));
    let now = ilium_plugin_sdk::host::HostClock::now_unix_millis(api);
    let screen = if view.agent.is_none() {
        HostTerminal::read_screen(api, pane, ScreenPart::Visible, SCREEN_ROWS)
            .map(|screen| screen.rows.join("\n"))
    } else {
        None
    };
    let (file, length, modified) = match &session {
        Some(session) => open_transcript(api, session),
        None => (None, 0, None),
    };
    let cell: RefCell<&mut dyn HostApi> = RefCell::new(api);
    let transcript = file.map(|file| HostTranscript {
        api: &cell,
        file,
        length,
    });
    let source = source_for(&view, transcript.as_ref(), modified, cwd, screen);
    let input = PaneInput {
        pane_id: pane.0.to_string(),
        pane_name: name,
        source,
        fence: PaneFence {
            pane_id: pane.0.to_string(),
            session_id: session.as_ref().map(|session| session.session_id.clone()),
        },
    };
    let mut env = JobEnv {
        api: &cell,
        pane,
        is_agent: view.agent.is_some(),
    };
    CACHE.with(|cache| recap_pane(&input, now, &mut env, &mut cache.borrow_mut()))
}

fn source_for<'a, 'b>(
    view: &NodeView,
    transcript: Option<&'a HostTranscript<'a, 'b>>,
    modified_unix_millis: Option<u64>,
    cwd: String,
    screen_text: Option<String>,
) -> Source<'a> {
    match &view.agent {
        Some(agent) => Source::Agent {
            class: agent_kind(&agent.signature),
            transcript: transcript.map(|source| source as &dyn ByteSource),
            modified_unix_millis,
            project_dir: cwd,
            activity: agent.activity.as_str().to_string(),
        },
        None => Source::Shell { cwd, screen_text },
    }
}

/// The file, length and modification time of the session transcript, when it can be opened.
fn open_transcript(
    api: &mut dyn HostApi,
    session: &SessionRef,
) -> (Option<FsPath>, u64, Option<u64>) {
    let file = FsPath {
        scope: session.scope.clone(),
        path: session.path.clone(),
    };
    match HostFs::metadata(api, file.clone()) {
        Ok(metadata) if metadata.kind == EntryKind::File => {
            (Some(file), metadata.length, metadata.modified_unix_millis)
        }
        _ => (None, 0, None),
    }
}
