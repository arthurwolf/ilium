//! The server side of a conversion: the state machine behind the `start`, `cancel`, `resume` and `close`
//! messages of the client side (manual page "Server side", F-077).
//!
//! One conversion owns one pane. Its durable state is the JSON document `convert/<pane>` of the `session`
//! storage bucket (`{"pane", "target", "phase"}`), which a restart would still see, and a ten minute durable
//! watchdog that undoes a conversion nobody finished. Everything that cannot survive a restart (the job id, the
//! child process, the pending completions) lives in [`Runtime`] and is dropped on activation; the watchdog
//! cleans up the durable half after such a restart.
//!
//! Phases of the state: `frozen` (the pane is frozen and its agent is being stopped), `stopped` (the agent is
//! gone, the conversion runs) and `failed` (the conversion ended without a result and waits for the user's
//! answer: Resume original, if `can_resume`, or Close).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use ilium_plugin_sdk::host::{
    Bucket, ChildHandle, ConfigChange, HostClock, HostExtensions, HostFs, HostJobs, HostJournal,
    HostProcess, HostStorage, HostTimers, HostTree, ProcessSpec, SpawnSpec,
};
use ilium_plugin_sdk::primitives::{HostAgent, SessionRef};
use ilium_plugin_sdk::types::{
    ContainerKind, CreatePaneSpec, EpochMs, ErrorCode, Event, FreezePaneSpec, FsPath, Intent,
    MessageSpec, MessageTarget, NodeId, NodeView, Outcome, PluginError, PluginId, PluginSide,
    ReplacePaneCommandSpec, RequestId, RequestRef, Submission, TerminatePaneProcessSpec, ToastSpec,
    Tone, UnfreezePaneSpec,
};
use ilium_plugin_sdk::HostApi;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::paths::claude_project_slug;
use crate::transcript::{HashedIdentifiers, IdentifierSource};

/// The plugin id, also the target of the messages between the two sides.
pub const PLUGIN_ID: &str = "ilium.core.session-convert";
/// The safety-net timer of a conversion: ten minutes, in milliseconds.
pub const WATCHDOG_MILLIS: u64 = 10 * 60 * 1000;
/// Prefix of the durable timer name, followed by the pane id.
pub const WATCHDOG_PREFIX: &str = "convert-watchdog/";
/// Prefix of the storage key of a conversion, followed by the pane id.
const STATE_PREFIX: &str = "convert/";
/// The observe-only hook that runs after a successful conversion.
const AFTER_CONVERT_POINT: &str = "ilium.core.session-convert/after-convert";
/// The scope Claude Code transcripts are proposed into and read back from.
const CLAUDE_CONFIG_SCOPE: &str = "agent-config:claude";
/// The scope the Codex importer writes its rollouts into.
const CODEX_SESSIONS_SCOPE: &str = "agent-sessions:codex";
/// How long the agent has between the polite signal and the kill when its process is stopped.
const TERMINATE_GRACE_MILLIS: u32 = 3_000;
/// One `fs.read` call returns at most this many bytes (host limit 1 MiB).
const READ_CHUNK_BYTES: u32 = 1024 * 1024;
/// A converted transcript larger than this is not read back for the verification.
const MAX_VERIFY_BYTES: u64 = 256 * 1024 * 1024;
/// Most rollouts the lookup of an imported Codex session looks at.
const SCAN_MAX_ENTRIES: u32 = 5_000;

/// Steps of Claude Code to Codex, in the order of the dialog (page figure "The progress dialog").
const STEP_LOCATE: u32 = 1;
const STEP_INSPECT: u32 = 2;
const STEP_START_SERVER: u32 = 3;
const STEP_IMPORT: u32 = 4;
const STEP_VERIFY_ROLLOUT: u32 = 5;
const STEP_STOP_SERVER: u32 = 6;
/// Steps of Codex to Claude Code that are not shared by number with the list above.
const STEP_PARSE: u32 = 2;
const STEP_CONVERT: u32 = 3;
const STEP_WRITE: u32 = 4;
const STEP_VERIFY: u32 = 5;
const STEP_REGISTER: u32 = 6;

const INITIALIZE_ID: u64 = 1;
const IMPORT_ID: u64 = 2;

/// The durable state of one conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct State {
    pane: u64,
    target: String,
    phase: String,
    /// Set once the conversion failed: whether the original session may be resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    can_resume: Option<bool>,
    /// The agent of the original session, set once the conversion failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// The id of the original session, set once the conversion failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
}

/// What lives only as long as the plugin instance.
#[derive(Default)]
struct Run {
    source: String,
    session: Option<SessionRef>,
    cwd: Option<String>,
    job: Option<u64>,
    child: Option<(ChildHandle, u64)>,
    import_requested: bool,
    finishing: bool,
    written: Option<Written>,
}

/// A transcript proposed to the Claude Code folder, waiting for the host's answer.
#[derive(Clone)]
struct Written {
    path: String,
    session_id: String,
    kept: usize,
    dropped: usize,
}

#[derive(Default)]
struct Runtime {
    runs: HashMap<u64, Run>,
    /// Submission correlation of a stop request to the pane.
    terminates: HashMap<u64, u64>,
    /// Submission correlation of a freeze request to the pane.
    freezes: HashMap<u64, u64>,
    /// Job request id to the pane.
    jobs: HashMap<u64, u64>,
    /// Proposal request id to the pane.
    proposals: HashMap<u64, u64>,
    /// Scan request id to the pane and the thread the import named.
    scans: HashMap<u64, (u64, String)>,
    /// Child id to the pane.
    children: HashMap<u64, u64>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

/// Forgets every pending completion; the durable state stays for the watchdog.
pub fn reset() {
    RUNTIME.with(|cell| *cell.borrow_mut() = Runtime::default());
}

fn with_run<R>(pane: u64, change: impl FnOnce(&mut Run) -> R) -> Option<R> {
    RUNTIME.with(|cell| cell.borrow_mut().runs.get_mut(&pane).map(change))
}

/// A refused start: the code the service call answers with and the text the toast shows.
#[derive(Debug)]
pub struct Refusal {
    pub code: ErrorCode,
    pub message: String,
}

fn refusal(code: ErrorCode, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

fn state_key(pane: u64) -> String {
    format!("{STATE_PREFIX}{pane}")
}

fn load_state(api: &mut dyn HostApi, pane: u64) -> Option<State> {
    let bytes = HostStorage::get(api, Bucket::Session, state_key(pane))?;
    serde_json::from_slice(&bytes).ok()
}

fn save_state(api: &mut dyn HostApi, state: &State) -> Result<(), PluginError> {
    let bytes = serde_json::to_vec(state)
        .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))?;
    HostStorage::put(api, Bucket::Session, state_key(state.pane), bytes)
}

/// Removes the durable half of a conversion: the state and the watchdog.
fn drop_state(api: &mut dyn HostApi, pane: u64) {
    let _ = HostStorage::remove(api, Bucket::Session, state_key(pane));
    HostTimers::cancel_durable(api, format!("{WATCHDOG_PREFIX}{pane}"));
}

fn to_client(channel: &str, payload: &Value) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client: None,
        },
        channel: channel.to_string(),
        payload: payload.to_string().into_bytes(),
    }))
}

fn step(index: u32) -> Submission {
    to_client("step", &json!({ "index": index }))
}

fn progress(percent: f64, message: &str) -> Submission {
    to_client("progress", &json!({"percent": percent, "message": message}))
}

fn finished(pane: u64, ok: bool) -> Submission {
    to_client("finished", &json!({"pane": pane, "ok": ok}))
}

fn toast(message: &str) -> Submission {
    let title: String = format!("Cannot convert: {message}")
        .chars()
        .take(60)
        .collect();
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("session-convert-refused".to_string()),
        tone: Tone::Warning,
        title,
        body: None,
        actions: Vec::new(),
        duration_ms: None,
    }))
}

/// The resume command of an original session: Codex and Claude Code have different grammars.
fn resume_command(source: &str, session_id: &str) -> Vec<String> {
    if source == "codex" {
        vec!["codex".into(), "resume".into(), session_id.into()]
    } else {
        vec!["claude".into(), "--resume".into(), session_id.into()]
    }
}

fn agent_title(agent: &str) -> &'static str {
    if agent == "codex" {
        "codex (converted)"
    } else {
        "claude (converted)"
    }
}

/// True when the node or one of its ancestors is a Git worktree container.
fn in_worktree(api: &mut dyn HostApi, node: &NodeView) -> bool {
    let mut current = node.clone();
    for _ in 0..64 {
        if current.container == Some(ContainerKind::Worktree) {
            return true;
        }
        let Some(parent) = current.parent else {
            return false;
        };
        let Some(next) = HostTree::get(api, parent) else {
            return false;
        };
        current = next;
    }
    false
}

/// Checks a request to convert `pane` to `target` and, when it passes, freezes the pane and stops its agent.
///
/// `strict` also checks the session and the source transcript. The service entry point skips those two at call
/// time (the caller cannot wait for them); the flow then fails after the stop when there is no session.
pub fn begin(
    api: &mut dyn HostApi,
    pane: u64,
    target: &str,
    strict: bool,
    out: &mut Vec<Submission>,
) -> Result<(), Refusal> {
    let node = HostTree::get(api, NodeId(pane))
        .ok_or_else(|| refusal(ErrorCode::NotFound, "no such pane"))?;
    let source = node
        .agent
        .as_ref()
        .map(|agent| agent.signature.clone())
        .unwrap_or_default();
    let expected_target = match source.as_str() {
        "claude" => "codex",
        "codex" => "claude",
        _ => {
            return Err(refusal(
                ErrorCode::InvalidArgument,
                "only Claude Code and Codex sessions can be converted",
            ))
        }
    };
    if target != expected_target {
        return Err(refusal(
            ErrorCode::InvalidArgument,
            format!("a {source} session converts to {expected_target}"),
        ));
    }
    if in_worktree(api, &node) {
        return Err(refusal(ErrorCode::Conflict, "worktree pane"));
    }
    // The menu runs one conversion at a time; another plugin calling the service may start several, one per pane.
    let busy = if strict {
        !HostStorage::keys(api, Bucket::Session, STATE_PREFIX.to_string()).is_empty()
    } else {
        load_state(api, pane).is_some()
    };
    if busy {
        return Err(refusal(ErrorCode::Conflict, "another conversion runs"));
    }
    let session = HostAgent::session(api, NodeId(pane));
    if strict {
        let session = session
            .as_ref()
            .ok_or_else(|| refusal(ErrorCode::Conflict, "no verified session"))?;
        let source_path = FsPath {
            scope: session.scope.clone(),
            path: session.path.clone(),
        };
        if HostFs::metadata(api, source_path).is_err() {
            return Err(refusal(
                ErrorCode::NotFound,
                "the source transcript was not found",
            ));
        }
    }

    let state = State {
        pane,
        target: target.to_string(),
        phase: "frozen".to_string(),
        can_resume: None,
        source: None,
        session_id: None,
    };
    save_state(api, &state).map_err(|error| refusal(error.code, error.message))?;
    let _ = HostJournal::log_event(
        api,
        Some(NodeId(pane)),
        "convert-started".to_string(),
        json!({ "target": target }).to_string(),
    );
    let deadline = HostClock::now_unix_millis(api) + WATCHDOG_MILLIS;
    let _ = HostTimers::at(api, EpochMs(deadline), format!("{WATCHDOG_PREFIX}{pane}"));

    out.push(to_client(
        "begin",
        &json!({"pane": pane, "source": source, "target": target}),
    ));
    let freeze = Submission::keyed(
        Intent::FreezePane(FreezePaneSpec { pane: NodeId(pane) }),
        format!("freeze-{pane}"),
    );
    let terminate = Submission::new(Intent::TerminatePaneProcess(TerminatePaneProcessSpec {
        pane: NodeId(pane),
        grace_ms: TERMINATE_GRACE_MILLIS,
    }));
    RUNTIME.with(|cell| {
        let mut runtime = cell.borrow_mut();
        runtime.freezes.insert(freeze.correlation, pane);
        runtime.terminates.insert(terminate.correlation, pane);
        runtime.runs.insert(
            pane,
            Run {
                source,
                session,
                cwd: node.cwd.clone(),
                ..Run::default()
            },
        );
    });
    out.push(freeze);
    out.push(terminate);
    Ok(())
}

/// The `start` message of the client side: `{"pane": id, "target": "codex" | "claude"}`.
pub fn on_start(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Ok(request) = serde_json::from_slice::<Value>(payload) else {
        return;
    };
    let (Some(pane), Some(target)) = (
        request.get("pane").and_then(Value::as_u64),
        request.get("target").and_then(Value::as_str),
    ) else {
        return;
    };
    if let Err(refused) = begin(api, pane, target, true, out) {
        out.push(toast(&refused.message));
        out.push(finished(pane, false));
    }
}

fn pane_of_message(payload: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(payload)
        .ok()?
        .get("pane")?
        .as_u64()
}

/// The `cancel` message: stops the running job or app-server; the conversion then ends as a failure.
pub fn on_cancel(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    let Some(state) = load_state(api, pane) else {
        return;
    };
    if state.phase != "stopped" {
        return;
    }
    let (job, has_child) = with_run(pane, |run| (run.job, run.child.is_some())).unwrap_or_default();
    if let Some(job) = job {
        // The job ends with a cancelled completion, which fails the conversion.
        HostJobs::cancel(api, RequestId(job));
    } else if has_child {
        fail(api, pane, "The conversion was cancelled.", true, 0, 0, out);
    }
}

/// The `resume` message: puts the original session back into the frozen pane.
pub fn on_resume(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    let Some(state) = load_state(api, pane) else {
        return;
    };
    if state.phase != "failed" || state.can_resume != Some(true) {
        return;
    }
    let (source, session_id) = match (state.source.clone(), state.session_id.clone()) {
        (Some(source), Some(session_id)) => (source, session_id),
        _ => match HostAgent::session(api, NodeId(pane)) {
            Some(session) => (
                with_run(pane, |run| run.source.clone()).unwrap_or_default(),
                session.session_id,
            ),
            None => return,
        },
    };
    discard(api, pane);
    let _ = HostJournal::log_event(
        api,
        Some(NodeId(pane)),
        "convert-resumed".to_string(),
        json!({"session": session_id}).to_string(),
    );
    out.push(Submission::new(Intent::ReplacePaneCommand(
        ReplacePaneCommandSpec {
            pane: NodeId(pane),
            command: resume_command(&source, &session_id),
            cwd: None,
            keep_scrollback: true,
        },
    )));
    out.push(finished(pane, false));
}

/// The `close` message: ends the conversion and unfreezes the pane.
pub fn on_close(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    abandon(api, pane, out);
}

/// The watchdog of a pane fired: the conversion ran too long or was lost in a restart.
pub fn on_timer(api: &mut dyn HostApi, name: &str, out: &mut Vec<Submission>) {
    let Some(pane) = name
        .strip_prefix(WATCHDOG_PREFIX)
        .and_then(|text| text.parse::<u64>().ok())
    else {
        return;
    };
    abandon(api, pane, out);
}

/// Ends a conversion without a result: stops what runs, unfreezes the pane and tells the client.
fn abandon(api: &mut dyn HostApi, pane: u64, out: &mut Vec<Submission>) {
    if load_state(api, pane).is_none() {
        return;
    }
    let job = with_run(pane, |run| run.job).flatten();
    if let Some(job) = job {
        HostJobs::cancel(api, RequestId(job));
    }
    discard(api, pane);
    let _ = HostJournal::log_event(
        api,
        Some(NodeId(pane)),
        "convert-closed".to_string(),
        "{}".to_string(),
    );
    out.push(Submission::new(Intent::UnfreezePane(UnfreezePaneSpec {
        pane: NodeId(pane),
    })));
    out.push(finished(pane, false));
}

/// Forgets a conversion: its durable state, its watchdog, its child process and its pending completions.
fn discard(api: &mut dyn HostApi, pane: u64) {
    stop_child(api, pane);
    drop_state(api, pane);
    RUNTIME.with(|cell| {
        let mut runtime = cell.borrow_mut();
        runtime.runs.remove(&pane);
        runtime.terminates.retain(|_, owner| *owner != pane);
        runtime.freezes.retain(|_, owner| *owner != pane);
        runtime.jobs.retain(|_, owner| *owner != pane);
        runtime.proposals.retain(|_, owner| *owner != pane);
        runtime.scans.retain(|_, (owner, _)| *owner != pane);
        runtime.children.retain(|_, owner| *owner != pane);
    });
}

/// Asks the app-server of a pane to end, once.
fn stop_child(api: &mut dyn HostApi, pane: u64) {
    let child = with_run(pane, |run| run.child.take()).flatten();
    if let Some((handle, id)) = child {
        RUNTIME.with(|cell| cell.borrow_mut().children.remove(&id));
        let _ = HostProcess::child_signal(api, handle, "terminate".to_string());
    }
}

/// Ends the conversion with a failure the user sees: the pane stays frozen until Resume original or Close.
fn fail(
    api: &mut dyn HostApi,
    pane: u64,
    message: &str,
    can_resume: bool,
    carried: usize,
    left_out: usize,
    out: &mut Vec<Submission>,
) {
    let Some(mut state) = load_state(api, pane) else {
        return;
    };
    stop_child(api, pane);
    let (source, session_id) = with_run(pane, |run| {
        (
            run.source.clone(),
            run.session
                .as_ref()
                .map(|session| session.session_id.clone()),
        )
    })
    .unwrap_or_default();
    RUNTIME.with(|cell| {
        let mut runtime = cell.borrow_mut();
        runtime.jobs.retain(|_, owner| *owner != pane);
        runtime.proposals.retain(|_, owner| *owner != pane);
        runtime.scans.retain(|_, (owner, _)| *owner != pane);
        if let Some(run) = runtime.runs.get_mut(&pane) {
            run.job = None;
        }
    });
    state.phase = "failed".to_string();
    state.can_resume = Some(can_resume);
    state.source = (!source.is_empty()).then_some(source);
    state.session_id = session_id;
    let _ = save_state(api, &state);
    let _ = HostJournal::log_event(
        api,
        Some(NodeId(pane)),
        "convert-failed".to_string(),
        json!({"message": message}).to_string(),
    );
    out.push(to_client(
        "failed",
        &json!({
            "pane": pane,
            "message": message,
            "carried": carried,
            "left_out": left_out,
            "can_resume": can_resume,
        }),
    ));
}

/// Ends the conversion with a new pane that resumes the converted session; the frozen pane is closed.
fn succeed(
    api: &mut dyn HostApi,
    pane: u64,
    target: &str,
    command: Vec<String>,
    hook: Value,
    out: &mut Vec<Submission>,
) {
    discard(api, pane);
    let _ = HostJournal::log_event(
        api,
        Some(NodeId(pane)),
        "convert-finished".to_string(),
        hook.to_string(),
    );
    out.push(Submission::new(Intent::CreatePane(CreatePaneSpec {
        parent: None,
        title: agent_title(target).to_string(),
        content_kind: "ilium.terminal".to_string(),
        cwd: None,
        command: Some(command),
        initial_input: None,
    })));
    out.push(Submission::new(Intent::CloseNode(NodeId(pane))));
    // The hook only observes: its answer, or its absence, never changes the outcome.
    let _ = HostExtensions::run_hook(api, AFTER_CONVERT_POINT.to_string(), hook.to_string());
    out.push(finished(pane, true));
}

/// A request the plugin submitted or started was answered.
pub fn on_completed(
    api: &mut dyn HostApi,
    request: RequestRef,
    outcome: Outcome,
    out: &mut Vec<Submission>,
) {
    match request {
        RequestRef::Submission(correlation) => {
            let (stop, freeze) = RUNTIME.with(|cell| {
                let mut runtime = cell.borrow_mut();
                (
                    runtime.terminates.remove(&correlation),
                    runtime.freezes.remove(&correlation),
                )
            });
            if let Some(pane) = stop {
                on_stopped(api, pane, outcome, out);
            } else if let Some(pane) = freeze {
                if !outcome.is_ok() && !matches!(outcome, Outcome::Duplicate(_)) {
                    fail(
                        api,
                        pane,
                        &format!("The pane could not be frozen: {}", describe(&outcome)),
                        false,
                        0,
                        0,
                        out,
                    );
                }
            }
        }
        RequestRef::Call(id) => {
            let (job, proposal, scan) = RUNTIME.with(|cell| {
                let mut runtime = cell.borrow_mut();
                (
                    runtime.jobs.remove(&id.0),
                    runtime.proposals.remove(&id.0),
                    runtime.scans.remove(&id.0),
                )
            });
            if let Some(pane) = job {
                on_job_done(api, pane, id, outcome, out);
            } else if let Some(pane) = proposal {
                on_proposal_done(api, pane, id, outcome, out);
            } else if let Some((pane, thread)) = scan {
                on_scan_done(api, pane, &thread, id, out);
            }
        }
    }
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Refused(error) | Outcome::Failed(error) => error.message.clone(),
        Outcome::Cancelled => "cancelled".to_string(),
        Outcome::Duplicate(_) => "duplicate request".to_string(),
        _ => "failed".to_string(),
    }
}

/// The agent process was asked to stop: on success the conversion starts.
fn on_stopped(api: &mut dyn HostApi, pane: u64, outcome: Outcome, out: &mut Vec<Submission>) {
    let Some(mut state) = load_state(api, pane) else {
        return;
    };
    if state.phase != "frozen" {
        return;
    }
    if !outcome.is_ok() {
        fail(
            api,
            pane,
            &format!("The agent could not be stopped: {}", describe(&outcome)),
            false,
            0,
            0,
            out,
        );
        return;
    }
    state.phase = "stopped".to_string();
    let _ = save_state(api, &state);
    let session = with_run(pane, |run| run.session.clone())
        .flatten()
        .or_else(|| HostAgent::session(api, NodeId(pane)));
    let Some(session) = session else {
        fail(
            api,
            pane,
            "The pane has no verified session.",
            false,
            0,
            0,
            out,
        );
        return;
    };
    with_run(pane, |run| run.session = Some(session.clone()));
    out.push(step(STEP_LOCATE));
    if state.target == "claude" {
        start_job(api, pane, &session, out);
    } else {
        start_app_server(api, pane, out);
    }
}

// ---- Codex to Claude Code: the job, the proposal and the verification ----

fn start_job(api: &mut dyn HostApi, pane: u64, session: &SessionRef, out: &mut Vec<Submission>) {
    let input = json!({ "rollout": session.path }).to_string().into_bytes();
    match HostJobs::start(api, "codex-to-claude".to_string(), input) {
        Ok(job) => {
            RUNTIME.with(|cell| cell.borrow_mut().jobs.insert(job.0, pane));
            with_run(pane, |run| run.job = Some(job.0));
            out.push(step(STEP_PARSE));
        }
        Err(error) => fail(api, pane, &error.message, true, 0, 0, out),
    }
}

/// The progress of a running job, as the percent of its rollout lines converted.
pub fn on_job_progress(job: u64, percent: f32, out: &mut Vec<Submission>) {
    let known = RUNTIME.with(|cell| cell.borrow().jobs.contains_key(&job));
    if !known {
        return;
    }
    let percent = f64::from(percent).clamp(0.0, 100.0);
    out.push(step(STEP_CONVERT));
    out.push(progress(
        percent.floor(),
        &format!("converted {}% of the rollout", percent.floor()),
    ));
}

/// The job output: the converted transcript and its counts.
#[derive(Deserialize)]
struct JobOutput {
    claude_jsonl: String,
    kept: usize,
    #[serde(default)]
    dropped: usize,
    #[serde(default)]
    session_id: String,
}

fn on_job_done(
    api: &mut dyn HostApi,
    pane: u64,
    job: RequestId,
    outcome: Outcome,
    out: &mut Vec<Submission>,
) {
    if load_state(api, pane).is_none() {
        return;
    }
    with_run(pane, |run| run.job = None);
    if let Outcome::Failed(error) | Outcome::Refused(error) = &outcome {
        let message = if error.code == ErrorCode::Cancelled {
            "The conversion was cancelled.".to_string()
        } else {
            error.message.clone()
        };
        fail(api, pane, &message, true, 0, 0, out);
        return;
    }
    if !outcome.is_ok() {
        fail(api, pane, "The conversion was cancelled.", true, 0, 0, out);
        return;
    }
    let Some(bytes) = HostJobs::take_output(api, job) else {
        fail(
            api,
            pane,
            "The conversion returned no result.",
            true,
            0,
            0,
            out,
        );
        return;
    };
    let Ok(output) = serde_json::from_slice::<JobOutput>(&bytes) else {
        fail(
            api,
            pane,
            "The conversion result is unreadable.",
            true,
            0,
            0,
            out,
        );
        return;
    };
    out.push(step(STEP_WRITE));
    propose_transcript(api, pane, output, out);
}

/// Proposes the converted transcript as a new file of the Claude Code folder. The host writes it only when
/// no file of that name exists, so a clash fails the conversion and never replaces a transcript.
fn propose_transcript(
    api: &mut dyn HostApi,
    pane: u64,
    output: JobOutput,
    out: &mut Vec<Submission>,
) {
    let now = HostClock::now_unix_millis(api);
    let session_id = if !output.session_id.is_empty() {
        output.session_id.clone()
    } else if let Some(found) = first_session_id(&output.claude_jsonl) {
        found
    } else {
        HashedIdentifiers::new(format!("{pane}\u{0}{now}"))
            .next_identifier()
            .hyphenated()
    };
    let cwd = with_run(pane, |run| run.cwd.clone()).flatten();
    let slug = claude_project_slug(Path::new(cwd.as_deref().unwrap_or("/")));
    let path = format!("projects/{slug}/{session_id}.jsonl");
    let mut content = output.claude_jsonl.into_bytes();
    content.push(b'\n');
    let change = ConfigChange {
        path: path.clone(),
        content: Some(content),
        revision: None,
        summary: "Add the converted Codex session".to_string(),
    };
    match HostFs::propose_agent_config(
        api,
        "claude".to_string(),
        "Convert a Codex session to Claude Code".to_string(),
        vec![change],
    ) {
        Ok(request) => {
            RUNTIME.with(|cell| cell.borrow_mut().proposals.insert(request.0, pane));
            with_run(pane, |run| {
                run.written = Some(Written {
                    path,
                    session_id,
                    kept: output.kept,
                    dropped: output.dropped,
                });
            });
        }
        Err(error) => fail(
            api,
            pane,
            &format!("The transcript could not be written: {}", error.message),
            true,
            0,
            0,
            out,
        ),
    }
}

fn on_proposal_done(
    api: &mut dyn HostApi,
    pane: u64,
    request: RequestId,
    outcome: Outcome,
    out: &mut Vec<Submission>,
) {
    let Some(written) = with_run(pane, |run| run.written.take()).flatten() else {
        return;
    };
    if load_state(api, pane).is_none() {
        return;
    }
    if !outcome.is_ok() {
        fail(
            api,
            pane,
            &format!(
                "The transcript could not be written: {}",
                describe(&outcome)
            ),
            true,
            0,
            0,
            out,
        );
        return;
    }
    let applied = HostFs::take_proposal(api, request)
        .is_some_and(|result| result.rejected.is_empty() && !result.applied.is_empty());
    if !applied {
        fail(
            api,
            pane,
            "The Claude Code folder did not accept the transcript.",
            true,
            0,
            0,
            out,
        );
        return;
    }
    out.push(step(STEP_VERIFY));
    let lines = count_lines(api, &written.path);
    if lines != Some(written.kept) {
        fail(
            api,
            pane,
            "The Claude Code transcript did not verify.",
            true,
            written.kept,
            written.dropped,
            out,
        );
        return;
    }
    out.push(step(STEP_REGISTER));
    let command = vec![
        "claude".to_string(),
        "--resume".to_string(),
        written.session_id.clone(),
    ];
    let hook = json!({
        "from": "codex",
        "to": "claude",
        "messages": written.kept,
        "path": written.path,
    });
    succeed(api, pane, "claude", command, hook, out);
}

/// The `sessionId` of the first line of a transcript, when the job output does not carry it.
fn first_session_id(jsonl: &str) -> Option<String> {
    let first = jsonl.lines().next()?;
    let value: Value = serde_json::from_str(first).ok()?;
    value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Reads the written transcript back and counts its lines.
fn count_lines(api: &mut dyn HostApi, path: &str) -> Option<usize> {
    let file = FsPath {
        scope: CLAUDE_CONFIG_SCOPE.to_string(),
        path: path.to_string(),
    };
    let mut offset: u64 = 0;
    let mut lines = 0usize;
    let mut last: Option<u8> = None;
    loop {
        let chunk = HostFs::read(api, file.clone(), offset, READ_CHUNK_BYTES).ok()?;
        if chunk.is_empty() {
            break;
        }
        lines += chunk.iter().filter(|byte| **byte == b'\n').count();
        last = chunk.last().copied();
        offset += chunk.len() as u64;
        if offset > MAX_VERIFY_BYTES {
            return None;
        }
    }
    if last.is_some_and(|byte| byte != b'\n') {
        lines += 1;
    }
    Some(lines)
}

// ---- Claude Code to Codex: a private `codex app-server` ----

fn start_app_server(api: &mut dyn HostApi, pane: u64, out: &mut Vec<Submission>) {
    out.push(step(STEP_INSPECT));
    out.push(step(STEP_START_SERVER));
    let spec = SpawnSpec {
        process: ProcessSpec {
            program: "codex".to_string(),
            arguments: vec!["app-server".to_string()],
            cwd: None,
            stdin: None,
            environment: Vec::new(),
            timeout_millis: 0,
        },
        stdin_open: true,
        line_mode: true,
    };
    let handle = match HostProcess::spawn(api, spec) {
        Ok(handle) => handle,
        Err(error) => {
            fail(
                api,
                pane,
                &format!("codex app-server could not start: {}", error.message),
                true,
                0,
                0,
                out,
            );
            return;
        }
    };
    let child = HostProcess::child_id(api, handle).0;
    RUNTIME.with(|cell| cell.borrow_mut().children.insert(child, pane));
    with_run(pane, |run| run.child = Some((handle, child)));
    send_line(
        api,
        pane,
        &json!({
            "id": INITIALIZE_ID,
            "method": "initialize",
            "params": {
                "clientInfo": {"name": "ilium-session-convert", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true},
            },
        }),
        out,
    );
}

/// Writes one JSON line to the app-server; a broken pipe fails the conversion.
fn send_line(api: &mut dyn HostApi, pane: u64, message: &Value, out: &mut Vec<Submission>) -> bool {
    let Some((handle, _)) = with_run(pane, |run| run.child).flatten() else {
        return false;
    };
    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    match HostProcess::child_write_stdin(api, handle, line) {
        Ok(()) => true,
        Err(error) => {
            fail(
                api,
                pane,
                &format!("codex app-server stopped answering: {}", error.message),
                true,
                0,
                0,
                out,
            );
            false
        }
    }
}

/// Output of the app-server: JSON lines, answers and notifications.
pub fn on_child_output(api: &mut dyn HostApi, child: u64, bytes: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = RUNTIME.with(|cell| cell.borrow().children.get(&child).copied()) else {
        return;
    };
    for line in String::from_utf8_lossy(bytes).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            // A line that is not JSON is noise of the child, not an answer.
            continue;
        };
        if load_state(api, pane).is_none() || with_run(pane, |run| run.finishing).unwrap_or(true) {
            return;
        }
        on_app_server_message(api, pane, &message, out);
    }
}

fn on_app_server_message(
    api: &mut dyn HostApi,
    pane: u64,
    message: &Value,
    out: &mut Vec<Submission>,
) {
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the app-server refused the request");
        fail(
            api,
            pane,
            &format!("codex app-server: {text}"),
            true,
            0,
            0,
            out,
        );
        return;
    }
    match message.get("id").and_then(Value::as_u64) {
        Some(INITIALIZE_ID) => on_initialized(api, pane, message, out),
        Some(IMPORT_ID) => {
            out.push(progress(10.0, "Import accepted"));
        }
        _ => match message.get("method").and_then(Value::as_str) {
            Some("externalAgentConfig/import/progress") => {
                let (done, failed) = session_counts(message.get("params"));
                out.push(progress(
                    50.0,
                    &format!("imported {done} session(s), {failed} failed"),
                ));
            }
            Some("externalAgentConfig/import/completed") => {
                on_import_completed(api, pane, message, out)
            }
            _ => {}
        },
    }
}

fn on_initialized(api: &mut dyn HostApi, pane: u64, message: &Value, out: &mut Vec<Submission>) {
    if with_run(pane, |run| {
        std::mem::replace(&mut run.import_requested, true)
    })
    .unwrap_or(true)
    {
        return;
    }
    let codex_home = message
        .pointer("/result/codexHome")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let (session, cwd) =
        with_run(pane, |run| (run.session.clone(), run.cwd.clone())).unwrap_or_default();
    let Some(session) = session else {
        fail(
            api,
            pane,
            "The pane has no verified session.",
            true,
            0,
            0,
            out,
        );
        return;
    };
    // The session scope is the projects folder next to the Codex home, in the same user folder.
    let transcript = Path::new(&codex_home)
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(".claude")
        .join("projects")
        .join(&session.path);
    if !send_line(api, pane, &json!({"method": "initialized"}), out) {
        return;
    }
    out.push(step(STEP_IMPORT));
    send_line(
        api,
        pane,
        &json!({
            "id": IMPORT_ID,
            "method": "externalAgentConfig/import",
            "params": {
                "migrationItems": [{
                    "itemType": "SESSIONS",
                    "description": "Convert one Claude Code session for Codex",
                    "cwd": Value::Null,
                    "details": {"sessions": [{
                        "cwd": cwd.unwrap_or_default(),
                        "path": transcript.to_string_lossy(),
                        "title": Value::Null,
                    }]},
                }],
                "source": "ilium-session-convert",
            },
        }),
        out,
    );
}

/// `(successes, failures)` of the `SESSIONS` results of a notification's parameters.
fn session_counts(params: Option<&Value>) -> (usize, usize) {
    let Some(results) = params
        .and_then(|params| params.get("itemTypeResults"))
        .and_then(Value::as_array)
    else {
        return (0, 0);
    };
    results
        .iter()
        .filter(|result| result.get("itemType").and_then(Value::as_str) == Some("SESSIONS"))
        .fold((0, 0), |(successes, failures), result| {
            let count = |key: &str| {
                result
                    .get(key)
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            };
            (successes + count("successes"), failures + count("failures"))
        })
}

fn on_import_completed(
    api: &mut dyn HostApi,
    pane: u64,
    message: &Value,
    out: &mut Vec<Submission>,
) {
    let results: Vec<&Value> = message
        .pointer("/params/itemTypeResults")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter(|result| result.get("itemType").and_then(Value::as_str) == Some("SESSIONS"))
                .collect()
        })
        .unwrap_or_default();
    let failures: Vec<String> = results
        .iter()
        .flat_map(|result| {
            result
                .get("failures")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|failure| {
            let stage = failure
                .get("failureStage")
                .and_then(Value::as_str)
                .unwrap_or("import");
            let text = failure
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown failure");
            format!("{stage}: {text}")
        })
        .collect();
    if !failures.is_empty() {
        fail(
            api,
            pane,
            &format!("The Codex importer failed: {}", failures.join("; ")),
            true,
            0,
            failures.len(),
            out,
        );
        return;
    }
    let thread = results
        .iter()
        .flat_map(|result| {
            result
                .get("successes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|success| success.get("target").and_then(Value::as_str))
        .map(str::to_string);
    let Some(thread) = thread else {
        fail(
            api,
            pane,
            "Codex already holds this session; nothing was imported.",
            true,
            0,
            0,
            out,
        );
        return;
    };
    out.push(step(STEP_VERIFY_ROLLOUT));
    let scope_root = FsPath {
        scope: CODEX_SESSIONS_SCOPE.to_string(),
        path: String::new(),
    };
    match HostFs::scan(api, scope_root, "*.jsonl".to_string(), SCAN_MAX_ENTRIES) {
        Ok(request) => {
            RUNTIME.with(|cell| cell.borrow_mut().scans.insert(request.0, (pane, thread)));
        }
        // The lookup only confirms the import; without it the importer's answer stands.
        Err(_) => finish_import(api, pane, &thread, None, out),
    }
}

fn on_scan_done(
    api: &mut dyn HostApi,
    pane: u64,
    thread: &str,
    request: RequestId,
    out: &mut Vec<Submission>,
) {
    if load_state(api, pane).is_none() {
        return;
    }
    let found = HostFs::take_entries(api, request).and_then(|entries| {
        entries
            .into_iter()
            .find(|entry| entry.relative_path.contains(thread))
            .map(|entry| entry.relative_path)
    });
    finish_import(api, pane, thread, found, out);
}

fn finish_import(
    api: &mut dyn HostApi,
    pane: u64,
    thread: &str,
    rollout: Option<String>,
    out: &mut Vec<Submission>,
) {
    out.push(step(STEP_STOP_SERVER));
    with_run(pane, |run| run.finishing = true);
    stop_child(api, pane);
    let hook = json!({
        "from": "claude",
        "to": "codex",
        "messages": 0,
        "path": rollout.unwrap_or_default(),
    });
    succeed(
        api,
        pane,
        "codex",
        vec!["codex".into(), "resume".into(), thread.into()],
        hook,
        out,
    );
}

/// The app-server ended: before the import finished that fails the conversion.
pub fn on_child_exit(api: &mut dyn HostApi, child: u64, out: &mut Vec<Submission>) {
    let Some(pane) = RUNTIME.with(|cell| cell.borrow_mut().children.remove(&child)) else {
        return;
    };
    if with_run(pane, |run| run.finishing).unwrap_or(true) {
        return;
    }
    with_run(pane, |run| run.child = None);
    fail(
        api,
        pane,
        "codex app-server ended before the import finished.",
        true,
        0,
        0,
        out,
    );
}

/// The `ChildOutput`, `ChildExited`, request and message events of the server side go through here.
pub fn on_event(api: &mut dyn HostApi, event: Event, out: &mut Vec<Submission>) {
    match event {
        Event::Message(message) => {
            if message.from_side != PluginSide::Client {
                return;
            }
            match message.channel.as_str() {
                "start" => on_start(api, &message.payload, out),
                "cancel" => on_cancel(api, &message.payload, out),
                "resume" => on_resume(api, &message.payload, out),
                "close" => on_close(api, &message.payload, out),
                _ => {}
            }
        }
        Event::RequestCompleted(result) => on_completed(api, result.request, result.outcome, out),
        Event::JobProgress(job) => on_job_progress(job.job.0, job.percent, out),
        Event::DurableTimerFired(timer) => on_timer(api, &timer.name, out),
        Event::ChildOutput(output) => on_child_output(api, output.child.0, &output.bytes, out),
        Event::ChildExited(exit) => on_child_exit(api, exit.child.0, out),
        _ => {}
    }
}
