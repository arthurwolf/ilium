//! The server side of remote compaction: the state machine behind the Compact button, the automatic context
//! trigger and the progress dialog of the client side (manual page core-plugin-compaction.md, "Remote compaction").
//!
//! One run at a time owns one pane. Its sequence:
//!
//! 1. `begin` checks that remote compaction is on and that the pane has an agent with a transcript the plugin can read,
//!    tells the client to open the dialog and starts polling the transcript every half second.
//! 2. When the transcript is between turns (`transcript_is_at_pause_point`) the pane is frozen and its agent process is
//!    terminated. A busy agent gets one interrupt after `pause_timeout_seconds` and 45 more seconds before the run gives up.
//! 3. The terminate answer starts the `compact-session` job ([`crate::session_job`]), which rewrites the transcript.
//! 4. A finished job replaces the pane with `<agent> resume <same id>`; a failed one leaves the transcript as it was and
//!    offers to resume the original session in the frozen pane.
//!
//! The automatic trigger watches the focused pane: whenever its agent finishes a turn, the context fill of the transcript is
//! compared with `threshold_percent` ([`crate::monitor::Monitor`]). While remote compaction is off, or the pane has no
//! compactable session, the Compact button keeps sending the agent's own `/compact` through the toolbar plugin.
//!
//! Only the identity of a run is durable (the session storage key `compaction/run/<pane>` and a one hour watchdog), so a
//! restart cannot leave a pane frozen for good; everything else lives in [`Runtime`].

use std::cell::RefCell;

use ilium_plugin_sdk::host::{
    Bucket, HostClock, HostFs, HostJobs, HostPanes, HostStorage, HostTimers,
};
use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{
    EpochMs, ErrorCode, Event, FreezePaneSpec, FsPath, Intent, MessageSpec, MessageTarget, NodeId,
    Outcome, PluginId, PluginSide, ReplacePaneCommandSpec, RequestId, RequestRef, Signal,
    SignalPaneSpec, Submission, TerminatePaneProcessSpec, TimerId, ToastSpec, Tone,
    UnfreezePaneSpec,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::monitor::Monitor;
use crate::remote::{latest_context_usage, transcript_is_at_pause_point, AgentKind};
use crate::remote_settings::RemoteCompactionSettings;
use crate::session_job::{
    agent_kind, progress_key, read_settings, SessionJobInput, SessionJobOutput, JOB_KIND,
};

/// The plugin id, also the target of the messages between its two sides.
pub const PLUGIN_ID: &str = "ilium.core.compaction";
/// The plugin whose server side sends the agent's own `/compact`.
const TOOLBAR_PLUGIN: &str = "ilium.core.agent-toolbar";
/// How often a waiting run looks at the transcript.
const POLL_MILLIS: u32 = 500;
/// How long the agent has between the polite signal and the kill when its process is stopped.
const TERMINATE_GRACE_MILLIS: u32 = 3_000;
/// How long a busy agent has after the interrupt before the run gives up.
pub const PAUSE_AFTER_INTERRUPT_GRACE_MILLIS: u64 = 45_000;
/// The safety net of a run, in milliseconds: a run nobody finished is undone after an hour.
const WATCHDOG_MILLIS: u64 = 60 * 60 * 1000;
/// Prefix of the durable timer name, followed by the pane id.
const WATCHDOG_PREFIX: &str = "compaction-watchdog/";
/// Prefix of the session storage key of a run, followed by the pane id.
const STATE_PREFIX: &str = "compaction/run/";
/// How much of the end of a transcript the pause probe and the context probe look at.
const PROBE_TAIL_BYTES: u64 = 4 * 1024 * 1024;
/// One `fs.read` call returns at most this many bytes (host limit 1 MiB).
const READ_CHUNK_BYTES: u32 = 1024 * 1024;
/// The window assumed for a transcript that does not state one (the usual Claude Code window).
const FALLBACK_WINDOW_TOKENS: u64 = 200_000;
/// The global storage key that remembers a dismissed privacy banner.
const BANNER_KEY: &str = "compaction/privacy-banner-dismissed";
/// How many lines of the dialog log a message carries.
const REPLY_LOG_LINES: usize = 8;

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The agent is still in its turn; the pane is live.
    Waiting,
    /// The pane is frozen and its agent process was asked to end.
    Stopping,
    /// The job rewrites the transcript.
    Compacting,
    /// The run ended without a result and waits for the user's answer.
    Failed,
}

/// What lives only as long as the plugin instance.
struct Run {
    pane: u64,
    agent: AgentKind,
    signature: String,
    session_id: String,
    scope: String,
    path: String,
    phase: Phase,
    settings: RemoteCompactionSettings,
    started_millis: u64,
    interrupted: bool,
    frozen: bool,
    stopped: bool,
    timer: Option<TimerId>,
    job: Option<RequestId>,
    log: Vec<String>,
}

#[derive(Default)]
struct Runtime {
    run: Option<Run>,
    monitor: Monitor,
    /// Correlation of the terminate request of the run.
    terminate: Option<u64>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

/// Forgets the run and the monitor; the durable state stays for the watchdog.
pub fn reset() {
    RUNTIME.with(|cell| *cell.borrow_mut() = Runtime::default());
}

fn with_run<R>(change: impl FnOnce(&mut Run) -> R) -> Option<R> {
    RUNTIME.with(|cell| cell.borrow_mut().run.as_mut().map(change))
}

fn with_monitor<R>(change: impl FnOnce(&mut Monitor) -> R) -> R {
    RUNTIME.with(|cell| change(&mut cell.borrow_mut().monitor))
}

fn state_key(pane: u64) -> String {
    format!("{STATE_PREFIX}{pane}")
}

fn to_client(channel: &str, payload: &Value) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client: None,
        },
        channel: format!("remote-{channel}"),
        payload: payload.to_string().into_bytes(),
    }))
}

fn toast(id: &str, tone: Tone, title: &str) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some(id.to_string()),
        tone,
        title: title.chars().take(60).collect(),
        body: None,
        actions: Vec::new(),
        duration_ms: None,
    }))
}

/// The command that resumes a session: Claude Code and Codex have different grammars.
pub fn resume_command(agent: AgentKind, session_id: &str) -> Vec<String> {
    match agent {
        AgentKind::Claude => vec!["claude".into(), "--resume".into(), session_id.into()],
        AgentKind::Codex => vec!["codex".into(), "resume".into(), session_id.into()],
    }
}

fn agent_label(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
    }
}

/// What a poll of a waiting run decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseStep {
    /// The transcript is between turns: freeze and stop the agent.
    Reached,
    /// Look again at the next poll.
    Wait,
    /// The agent is still busy after the pause timeout: send one interrupt.
    Interrupt,
    /// The agent did not pause even after the interrupt.
    GiveUp,
}

/// The decision of one poll: `waited` milliseconds since the run began, `timeout` the user's pause timeout.
pub fn pause_step(at_pause_point: bool, waited: u64, timeout: u64, interrupted: bool) -> PauseStep {
    if at_pause_point {
        PauseStep::Reached
    } else if waited >= timeout.saturating_add(PAUSE_AFTER_INTERRUPT_GRACE_MILLIS) {
        PauseStep::GiveUp
    } else if !interrupted && waited >= timeout {
        PauseStep::Interrupt
    } else {
        PauseStep::Wait
    }
}

/// The context fill of a transcript in percent of its window, when the transcript states a usage.
pub fn context_fill_percent(agent: AgentKind, transcript: &[u8]) -> Option<f64> {
    let (tokens, window) = latest_context_usage(agent, transcript)?;
    let window = window
        .filter(|window| *window > 0)
        .unwrap_or(FALLBACK_WINDOW_TOKENS);
    Some(tokens as f64 * 100.0 / window as f64)
}

/// The last `max_bytes` of a file of a read scope.
fn read_tail(
    api: &mut dyn HostApi,
    scope: &str,
    path: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    let location = FsPath {
        scope: scope.to_string(),
        path: path.to_string(),
    };
    let length = HostFs::metadata(api, location.clone())
        .map_err(|error| error.message)?
        .length;
    let mut offset = length.saturating_sub(max_bytes);
    let mut bytes = Vec::new();
    while offset < length {
        let chunk = HostFs::read(api, location.clone(), offset, READ_CHUNK_BYTES)
            .map_err(|error| error.message)?;
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The answer to a run that the plugin cannot do remotely: the agent's own `/compact`, through the toolbar plugin.
fn native_compact(pane: u64, out: &mut Vec<Submission>) {
    out.push(Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(TOOLBAR_PLUGIN.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: "compact".to_string(),
        payload: pane.to_string().into_bytes(),
    })));
}

/// The session the plugin can compact for `pane`: the agent, its detection signature and its transcript location.
struct Plan {
    agent: AgentKind,
    signature: String,
    session_id: String,
    scope: String,
    path: String,
}

fn plan_for(api: &mut dyn HostApi, pane: u64) -> Option<Plan> {
    let view = HostAgent::view(api, NodeId(pane))?;
    let agent = agent_kind(&view.signature)?;
    let session = HostAgent::session(api, NodeId(pane))?;
    if session.path.is_empty() || session.scope != format!("agent-sessions:{}", view.signature) {
        return None;
    }
    let location = FsPath {
        scope: session.scope.clone(),
        path: session.path.clone(),
    };
    HostFs::metadata(api, location).ok()?;
    Some(Plan {
        agent,
        signature: view.signature,
        session_id: session.session_id,
        scope: session.scope,
        path: session.path,
    })
}

/// Starts a run for `pane`, or sends the agent's own `/compact` when remote compaction does not apply.
/// An automatic run never falls back and never complains.
pub fn begin(api: &mut dyn HostApi, pane: u64, automatic: bool, out: &mut Vec<Submission>) {
    let settings = read_settings(api);
    let plan = if settings.enabled {
        plan_for(api, pane)
    } else {
        None
    };
    let busy = RUNTIME.with(|cell| cell.borrow().run.is_some());
    let Some(plan) = plan else {
        if !automatic {
            native_compact(pane, out);
        }
        return;
    };
    if busy {
        if !automatic {
            out.push(toast(
                "compaction-busy",
                Tone::Warning,
                "Another compaction is still running",
            ));
        }
        return;
    }
    let now = HostClock::monotonic_millis(api);
    let timer = HostTimers::set(api, POLL_MILLIS, Some(POLL_MILLIS)).ok();
    let state = json!({ "pane": pane, "session_id": plan.session_id }).to_string();
    let _ = HostStorage::put(api, Bucket::Session, state_key(pane), state.into_bytes());
    let deadline = HostClock::now_unix_millis(api) + WATCHDOG_MILLIS;
    let _ = HostTimers::at(api, EpochMs(deadline), format!("{WATCHDOG_PREFIX}{pane}"));
    let technique = settings.technique_for(&plan.signature);
    with_monitor(|monitor| monitor.record_attempt(pane, now));
    out.push(to_client(
        "begin",
        &json!({
            "pane": pane,
            "agent": agent_label(plan.agent),
            "technique": technique.label(),
            "automatic": automatic,
            "banner": !banner_dismissed(api),
        }),
    ));
    let run = Run {
        pane,
        agent: plan.agent,
        signature: plan.signature,
        session_id: plan.session_id,
        scope: plan.scope,
        path: plan.path,
        phase: Phase::Waiting,
        settings,
        started_millis: now,
        interrupted: false,
        frozen: false,
        stopped: false,
        timer,
        job: None,
        log: Vec::new(),
    };
    RUNTIME.with(|cell| cell.borrow_mut().run = Some(run));
    log_line(
        out,
        format!("Waiting for {} to finish its turn", agent_label(plan.agent)),
    );
    poll(api, out);
}

/// Appends one line to the log of the run and tells the client.
fn log_line(out: &mut Vec<Submission>, line: String) {
    let lines = with_run(|run| {
        run.log.push(line);
        if run.log.len() > REPLY_LOG_LINES {
            run.log.remove(0);
        }
        run.log.clone()
    })
    .unwrap_or_default();
    out.push(to_client("log", &json!({ "log": lines })));
}

/// The `start` message of the client side: `{"pane": id}`; the Compact command arrives through it.
pub fn on_start(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    begin(api, pane, false, out);
}

fn pane_of_message(payload: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(payload)
        .ok()?
        .get("pane")?
        .as_u64()
}

/// One look at the transcript of a waiting run.
fn poll(api: &mut dyn HostApi, out: &mut Vec<Submission>) {
    let Some((pane, agent, scope, path, started, timeout, interrupted)) = with_run(|run| {
        (run.phase == Phase::Waiting).then(|| {
            (
                run.pane,
                run.agent,
                run.scope.clone(),
                run.path.clone(),
                run.started_millis,
                run.settings.pause_timeout_seconds * 1000,
                run.interrupted,
            )
        })
    })
    .flatten() else {
        return;
    };
    let tail = match read_tail(api, &scope, &path, PROBE_TAIL_BYTES) {
        Ok(tail) => tail,
        Err(message) => {
            fail(
                api,
                &format!("The transcript could not be read: {message}"),
                false,
                out,
            );
            return;
        }
    };
    let waited = HostClock::monotonic_millis(api).saturating_sub(started);
    match pause_step(
        transcript_is_at_pause_point(agent, &tail),
        waited,
        timeout,
        interrupted,
    ) {
        PauseStep::Wait => {}
        PauseStep::Interrupt => {
            with_run(|run| run.interrupted = true);
            log_line(
                out,
                "The agent is still busy after the pause timeout; sending an interrupt".to_string(),
            );
            out.push(Submission::new(Intent::SignalPane(SignalPaneSpec {
                pane: NodeId(pane),
                signal: Signal::Interrupt,
            })));
        }
        PauseStep::GiveUp => fail(
            api,
            "The agent did not reach a safe pause in time.",
            false,
            out,
        ),
        PauseStep::Reached => {
            stop_polling(api);
            with_run(|run| {
                run.phase = Phase::Stopping;
                run.frozen = true;
            });
            log_line(
                out,
                "Safe pause reached; stopping the agent process".to_string(),
            );
            out.push(to_client("phase", &json!({ "phase": "stopping" })));
            out.push(Submission::keyed(
                Intent::FreezePane(FreezePaneSpec { pane: NodeId(pane) }),
                format!("compaction-freeze-{pane}"),
            ));
            let terminate =
                Submission::new(Intent::TerminatePaneProcess(TerminatePaneProcessSpec {
                    pane: NodeId(pane),
                    grace_ms: TERMINATE_GRACE_MILLIS,
                }));
            RUNTIME.with(|cell| cell.borrow_mut().terminate = Some(terminate.correlation));
            out.push(terminate);
        }
    }
}

fn stop_polling(api: &mut dyn HostApi) {
    if let Some(Some(timer)) = with_run(|run| run.timer.take()) {
        HostTimers::cancel(api, timer);
    }
}

/// A volatile timer fired: the poll of a waiting run.
pub fn on_timer(api: &mut dyn HostApi, timer: TimerId, out: &mut Vec<Submission>) {
    if with_run(|run| run.timer == Some(timer)) == Some(true) {
        poll(api, out);
    }
}

/// Ends the run without a result the user sees: the transcript is untouched, the client shows the reason and, when the
/// agent was stopped, offers to resume the original session.
fn fail(api: &mut dyn HostApi, message: &str, stopped: bool, out: &mut Vec<Submission>) {
    stop_polling(api);
    let Some(pane) = with_run(|run| {
        run.phase = Phase::Failed;
        run.job = None;
        run.stopped = run.stopped || stopped;
        run.pane
    }) else {
        return;
    };
    let can_resume = with_run(|run| run.stopped).unwrap_or(false);
    with_monitor(|monitor| monitor.record_failure(pane));
    out.push(to_client(
        "failed",
        &json!({ "pane": pane, "message": message, "can_resume": can_resume }),
    ));
}

/// The pane left the run: the durable half is dropped.
fn forget(api: &mut dyn HostApi, pane: u64) {
    let _ = HostStorage::remove(api, Bucket::Session, state_key(pane));
    let _ = HostStorage::remove(api, Bucket::Session, progress_key(pane));
    HostTimers::cancel_durable(api, format!("{WATCHDOG_PREFIX}{pane}"));
    stop_polling(api);
    RUNTIME.with(|cell| {
        let mut runtime = cell.borrow_mut();
        runtime.run = None;
        runtime.terminate = None;
    });
}

/// The `cancel` message: a waiting run ends at once, a running job is asked to stop.
pub fn on_cancel(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    let Some((phase, job)) = with_run(|run| (run.phase, run.job))
        .filter(|_| with_run(|run| run.pane == pane).unwrap_or(false))
    else {
        return;
    };
    match phase {
        Phase::Waiting => {
            // Nothing was stopped yet, so cancelling just ends the wait.
            forget(api, pane);
            out.push(to_client("finished", &json!({ "pane": pane, "ok": false })));
            out.push(toast(
                "compaction-cancelled",
                Tone::Info,
                "Remote compaction cancelled",
            ));
        }
        Phase::Compacting => {
            if let Some(job) = job {
                log_line(out, "Cancelling".to_string());
                HostJobs::cancel(api, job);
            }
        }
        Phase::Stopping | Phase::Failed => {}
    }
}

/// The `resume` message: puts the original session back into the frozen pane.
pub fn on_resume(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    let Some((agent, session_id, ready)) = with_run(|run| {
        (
            run.agent,
            run.session_id.clone(),
            run.pane == pane && run.phase == Phase::Failed && run.stopped,
        )
    }) else {
        return;
    };
    if !ready {
        return;
    }
    forget(api, pane);
    out.push(Submission::new(Intent::ReplacePaneCommand(
        ReplacePaneCommandSpec {
            pane: NodeId(pane),
            command: resume_command(agent, &session_id),
            cwd: None,
            keep_scrollback: true,
        },
    )));
    out.push(to_client("finished", &json!({ "pane": pane, "ok": false })));
    out.push(toast(
        "compaction-resumed",
        Tone::Info,
        &format!("Resumed the original {} session", agent_label(agent)),
    ));
}

/// The `close` message: ends a failed run and unfreezes the pane.
pub fn on_close(api: &mut dyn HostApi, payload: &[u8], out: &mut Vec<Submission>) {
    let Some(pane) = pane_of_message(payload) else {
        return;
    };
    abandon(api, pane, out);
}

/// Ends a run without a result: stops the job, unfreezes the pane and tells the client.
fn abandon(api: &mut dyn HostApi, pane: u64, out: &mut Vec<Submission>) {
    let owned = with_run(|run| (run.pane == pane, run.job, run.frozen));
    let (job, frozen) = match owned {
        Some((true, job, frozen)) => (job, frozen),
        Some((false, _, _)) => return,
        // After a restart only the durable state is left.
        None => {
            if HostStorage::get(api, Bucket::Session, state_key(pane)).is_none() {
                return;
            }
            (None, true)
        }
    };
    if let Some(job) = job {
        HostJobs::cancel(api, job);
    }
    forget(api, pane);
    if frozen {
        out.push(Submission::new(Intent::UnfreezePane(UnfreezePaneSpec {
            pane: NodeId(pane),
        })));
    }
    out.push(to_client("finished", &json!({ "pane": pane, "ok": false })));
}

/// The durable watchdog of a run fired: it ran too long or was lost in a restart.
pub fn on_durable_timer(api: &mut dyn HostApi, name: &str, out: &mut Vec<Submission>) {
    let Some(pane) = name
        .strip_prefix(WATCHDOG_PREFIX)
        .and_then(|text| text.parse::<u64>().ok())
    else {
        return;
    };
    abandon(api, pane, out);
}

/// Whether the user closed the privacy banner of the dialog for good.
fn banner_dismissed(api: &mut dyn HostApi) -> bool {
    HostStorage::get(api, Bucket::Global, BANNER_KEY.to_string()).is_some()
}

/// The `dismiss-banner` message: the privacy banner never shows again.
pub fn on_dismiss_banner(api: &mut dyn HostApi) {
    let _ = HostStorage::put(api, Bucket::Global, BANNER_KEY.to_string(), b"1".to_vec());
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
            let is_terminate = RUNTIME.with(|cell| {
                let mut runtime = cell.borrow_mut();
                if runtime.terminate == Some(correlation) {
                    runtime.terminate = None;
                    true
                } else {
                    false
                }
            });
            if is_terminate {
                on_stopped(api, outcome, out);
            }
        }
        RequestRef::Call(job) => {
            if with_run(|run| run.job == Some(job)) == Some(true) {
                on_job_done(api, job, outcome, out);
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

/// The agent process was asked to stop: on success the job starts.
fn on_stopped(api: &mut dyn HostApi, outcome: Outcome, out: &mut Vec<Submission>) {
    let Some((pane, agent, input)) = with_run(|run| {
        (run.phase == Phase::Stopping).then(|| {
            (
                run.pane,
                run.agent,
                SessionJobInput {
                    pane: run.pane,
                    agent: run.signature.clone(),
                    scope: run.scope.clone(),
                    path: run.path.clone(),
                    session_id: run.session_id.clone(),
                    cwd: String::new(),
                },
            )
        })
    })
    .flatten() else {
        return;
    };
    if !outcome.is_ok() {
        fail(
            api,
            &format!("The agent could not be stopped: {}", describe(&outcome)),
            false,
            out,
        );
        return;
    }
    with_run(|run| run.stopped = true);
    log_line(
        out,
        format!("{} process stopped; screen frozen", agent_label(agent)),
    );
    let bytes = match serde_json::to_vec(&input) {
        Ok(bytes) => bytes,
        Err(error) => {
            fail(api, &error.to_string(), true, out);
            return;
        }
    };
    match HostJobs::start(api, JOB_KIND.to_string(), bytes) {
        Ok(job) => {
            with_run(|run| {
                run.job = Some(job);
                run.phase = Phase::Compacting;
            });
            out.push(to_client(
                "phase",
                &json!({ "phase": "compacting", "pane": pane }),
            ));
        }
        Err(error) => fail(api, &error.message, true, out),
    }
}

/// The progress of the job: the percent, and the step, the log and the token figures the job published.
pub fn on_job_progress(
    api: &mut dyn HostApi,
    job: RequestId,
    percent: f32,
    out: &mut Vec<Submission>,
) {
    let Some(pane) = with_run(|run| (run.job == Some(job)).then_some(run.pane)).flatten() else {
        return;
    };
    let mut document = HostStorage::get(api, Bucket::Session, progress_key(pane))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or_else(|| json!({ "percent": f64::from(percent) }));
    document["pane"] = json!(pane);
    out.push(to_client("progress", &document));
}

fn on_job_done(api: &mut dyn HostApi, job: RequestId, outcome: Outcome, out: &mut Vec<Submission>) {
    with_run(|run| run.job = None);
    match &outcome {
        Outcome::Failed(error) | Outcome::Refused(error) => {
            let message = if error.code == ErrorCode::Cancelled {
                "Remote compaction was cancelled.".to_string()
            } else {
                error.message.clone()
            };
            fail(api, &message, true, out);
            return;
        }
        Outcome::Cancelled => {
            fail(api, "Remote compaction was cancelled.", true, out);
            return;
        }
        _ if !outcome.is_ok() => {
            fail(api, "Remote compaction failed.", true, out);
            return;
        }
        _ => {}
    }
    let Some(bytes) = HostJobs::take_output(api, job) else {
        fail(api, "The compaction returned no result.", true, out);
        return;
    };
    let Ok(result) = serde_json::from_slice::<SessionJobOutput>(&bytes) else {
        fail(api, "The compaction result is unreadable.", true, out);
        return;
    };
    succeed(api, &result, out);
}

/// Ends the run with a pane that resumes the compacted session in place of the frozen one.
fn succeed(api: &mut dyn HostApi, result: &SessionJobOutput, out: &mut Vec<Submission>) {
    let Some((pane, agent, session_id)) =
        with_run(|run| (run.pane, run.agent, run.session_id.clone()))
    else {
        return;
    };
    forget(api, pane);
    with_monitor(|monitor| monitor.record_success(pane));
    out.push(Submission::new(Intent::ReplacePaneCommand(
        ReplacePaneCommandSpec {
            pane: NodeId(pane),
            command: resume_command(agent, &session_id),
            cwd: None,
            keep_scrollback: true,
        },
    )));
    out.push(to_client(
        "finished",
        &json!({
            "pane": pane,
            "ok": true,
            "used_fallback": result.used_fallback,
            "before": result.before_context,
            "after": result.after_context,
        }),
    ));
    let label = agent_label(agent);
    let title = if result.used_fallback {
        format!("Compacted with the offline fallback summary; {label} resumed")
    } else {
        format!("Conversation compacted remotely; {label} resumed")
    };
    out.push(toast("compaction-done", Tone::Success, &title));
}

/// An agent changed its activity: the automatic trigger looks at the focused pane when its agent finished a turn.
pub fn on_activity(
    api: &mut dyn HostApi,
    node: NodeId,
    finished_turn: bool,
    out: &mut Vec<Submission>,
) {
    if !finished_turn || RUNTIME.with(|cell| cell.borrow().run.is_some()) {
        return;
    }
    let settings = read_settings(api);
    if !settings.enabled || !settings.automatic || HostPanes::focused(api) != Some(node) {
        return;
    }
    let Some(plan) = plan_for(api, node.0) else {
        return;
    };
    let Ok(tail) = read_tail(api, &plan.scope, &plan.path, PROBE_TAIL_BYTES) else {
        return;
    };
    let Some(fill) = context_fill_percent(plan.agent, &tail) else {
        return;
    };
    let now = HostClock::monotonic_millis(api);
    let cooldown = settings.cooldown_minutes * 60_000;
    let trigger = with_monitor(|monitor| {
        monitor.should_trigger(
            node.0,
            fill,
            f64::from(settings.threshold_percent),
            cooldown,
            now,
        )
    });
    if trigger {
        begin(api, node.0, true, out);
    }
}

/// A pane or node is gone: its trigger state is forgotten, and a run on it ends.
pub fn on_pane_gone(api: &mut dyn HostApi, node: NodeId) {
    with_monitor(|monitor| monitor.forget(node.0));
    if with_run(|run| run.pane == node.0) == Some(true) {
        if let Some(Some(job)) = with_run(|run| run.job) {
            HostJobs::cancel(api, job);
        }
        forget(api, node.0);
    }
}

/// Routes one event of the server side. Events that other parts of the plugin also look at (request completions)
/// are only observed.
pub fn on_event(api: &mut dyn HostApi, event: &Event, out: &mut Vec<Submission>) {
    use ilium_plugin_sdk::types::AgentActivity;
    match event {
        Event::Message(message) if message.from_side == PluginSide::Client => {
            match message.channel.as_str() {
                "remote-start" => on_start(api, &message.payload, out),
                "remote-cancel" => on_cancel(api, &message.payload, out),
                "remote-resume" => on_resume(api, &message.payload, out),
                "remote-close" => on_close(api, &message.payload, out),
                "remote-dismiss-banner" => on_dismiss_banner(api),
                _ => {}
            }
        }
        Event::TimerFired(timer) => on_timer(api, *timer, out),
        Event::DurableTimerFired(fired) if fired.name.starts_with(WATCHDOG_PREFIX) => {
            on_durable_timer(api, &fired.name, out);
        }
        Event::RequestCompleted(result) => {
            on_completed(api, result.request.clone(), result.outcome.clone(), out);
        }
        Event::JobProgress(progress) => on_job_progress(api, progress.job, progress.percent, out),
        Event::AgentActivityChanged(change) => {
            let finished = matches!(change.current, AgentActivity::Idle | AgentActivity::Done)
                && change.previous != change.current;
            on_activity(api, change.node, finished, out);
        }
        Event::PaneExited(exited) => on_pane_gone(api, exited.node),
        Event::NodeRemoved(node) => on_pane_gone(api, *node),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: u64 = 120_000;

    /// REQ: PORT:ilium-client/src/remote_compaction_worker.rs::pause_wait_requests_one_interrupt_then_gives_up
    #[test]
    fn a_busy_agent_gets_one_interrupt_then_the_run_gives_up() {
        assert_eq!(pause_step(false, 0, TIMEOUT, false), PauseStep::Wait);
        assert_eq!(
            pause_step(false, TIMEOUT - 1, TIMEOUT, false),
            PauseStep::Wait
        );
        assert_eq!(
            pause_step(false, TIMEOUT, TIMEOUT, false),
            PauseStep::Interrupt
        );
        assert_eq!(
            pause_step(false, TIMEOUT + 1, TIMEOUT, true),
            PauseStep::Wait
        );
        assert_eq!(
            pause_step(
                false,
                TIMEOUT + PAUSE_AFTER_INTERRUPT_GRACE_MILLIS,
                TIMEOUT,
                true
            ),
            PauseStep::GiveUp
        );
    }

    #[test]
    fn a_pause_point_wins_over_every_timeout() {
        assert_eq!(pause_step(true, 0, TIMEOUT, false), PauseStep::Reached);
        assert_eq!(
            pause_step(true, TIMEOUT * 10, TIMEOUT, true),
            PauseStep::Reached
        );
    }

    /// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::success_resumes_the_same_session_and_closes_the_dialog
    #[test]
    fn each_agent_resumes_its_own_session_by_id() {
        assert_eq!(
            resume_command(AgentKind::Claude, "abc"),
            vec!["claude", "--resume", "abc"]
        );
        assert_eq!(
            resume_command(AgentKind::Codex, "abc"),
            vec!["codex", "resume", "abc"]
        );
    }

    #[test]
    fn the_context_fill_uses_the_transcript_window_or_the_default() {
        let claude = br#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","usage":{"input_tokens":100000,"output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"x"}]}}
"#;
        let fill = context_fill_percent(AgentKind::Claude, claude).expect("a usage");
        assert!((fill - 50.0).abs() < 1.0, "fill was {fill}");
        assert_eq!(context_fill_percent(AgentKind::Claude, b"not json\n"), None);
    }

    #[test]
    fn the_watchdog_names_carry_the_pane() {
        assert_eq!(state_key(7), "compaction/run/7");
        assert_eq!(format!("{WATCHDOG_PREFIX}{}", 7), "compaction-watchdog/7");
    }
}
