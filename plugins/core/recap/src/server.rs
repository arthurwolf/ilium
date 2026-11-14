//! The server side of a recap run: the `recap` command line and the `get` method of the
//! `ilium.core.recap/recap` service. A run selects panes, checks the consent gate, starts one
//! `recap-pane` job per pane and writes the collected records once every job has ended.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use ilium_plugin_sdk::host::{
    HostCli, HostClientRequests, HostJobs, HostKernelSettings, HostServices, HostTimers, HostTree,
};
use ilium_plugin_sdk::primitives::HostAgent;
use ilium_plugin_sdk::types::{
    ClientId, ErrorCode, Intent, MessageSpec, MessageTarget, NodeClass, NodeId, Outcome,
    PluginError, PluginId, PluginSide, RequestId, RequestRef, RequestResult, Submission, TimerId,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::custody::redact_secrets;
use crate::dot::{self, DotRecap};
use crate::model::{RecapLength, RecapPaneState};
use crate::pane_filter::{
    parse_arguments, AgentStateName, OptionSpec, PaneFacts, PaneFilterArgs, PaneKind,
    ParsedArguments, CWD_OPTION, FILTER_OPTIONS,
};
use crate::pipeline::lengths_for;
use crate::render::{pane_from_record, pane_warning, render_markdown_entries, summary_record};

/// Most panes recapped by one run unless `--max-panes` says otherwise.
pub const DEFAULT_MAX_PANES: usize = 32;
const MAX_PANES_LIMIT: usize = 64;
const JOB_KIND: &str = "recap-pane";
const TERMINAL_CONTENT_KIND: &str = "ilium.terminal";

/// The parsed options of `ilium recap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub length: String,
    pub markdown: bool,
    pub include_self: bool,
    pub max_panes: usize,
    pub timeout_seconds: u32,
    pub max_concurrency: usize,
}

const RECAP_OPTIONS: [OptionSpec; 6] = [
    OptionSpec {
        long: "length",
        short: None,
        takes_value: true,
    },
    OptionSpec {
        long: "format",
        short: None,
        takes_value: true,
    },
    OptionSpec {
        long: "timeout-seconds",
        short: None,
        takes_value: true,
    },
    OptionSpec {
        long: "max-concurrency",
        short: None,
        takes_value: true,
    },
    OptionSpec {
        long: "max-panes",
        short: None,
        takes_value: true,
    },
    OptionSpec {
        long: "include-self",
        short: None,
        takes_value: false,
    },
];

/// Seconds a whole run may take unless `--timeout-seconds` says otherwise.
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 60;
/// Panes recapped at the same time unless `--max-concurrency` says otherwise.
pub const DEFAULT_MAX_CONCURRENCY: usize = 4;

fn ranged(
    parsed: &ParsedArguments,
    option: &str,
    low: u64,
    high: u64,
) -> Result<Option<u64>, String> {
    let Some(text) = parsed.last(option) else {
        return Ok(None);
    };
    match text.parse::<u64>() {
        Ok(value) if (low..=high).contains(&value) => Ok(Some(value)),
        _ => Err(format!("recap: --{option} must be {low} to {high}")),
    }
}

/// Parses the recap options out of an already split command line.
pub fn parse_options(parsed: &ParsedArguments) -> Result<Options, String> {
    let length = parsed.last("length").unwrap_or("both").to_string();
    if lengths_for(&length).is_none() {
        return Err("recap: --length must be short, long or both".to_string());
    }
    let markdown = match parsed.last("format").unwrap_or("jsonl") {
        "jsonl" => false,
        "markdown" => true,
        _ => return Err("recap: --format must be jsonl or markdown".to_string()),
    };
    if let Some(word) = parsed.words.first() {
        return Err(format!("recap: unexpected argument '{word}'"));
    }
    Ok(Options {
        length,
        markdown,
        include_self: parsed.has("include-self"),
        max_panes: ranged(parsed, "max-panes", 1, MAX_PANES_LIMIT as u64)?
            .map_or(DEFAULT_MAX_PANES, |value| value as usize),
        timeout_seconds: ranged(parsed, "timeout-seconds", 1, 600)?
            .map_or(DEFAULT_TIMEOUT_SECONDS, |value| value as u32),
        max_concurrency: ranged(parsed, "max-concurrency", 1, 8)?
            .map_or(DEFAULT_MAX_CONCURRENCY, |value| value as usize),
    })
}

/// Splits `argv` along the recap options and the shared pane selectors.
pub fn parse_args(argv: &[String]) -> Result<(Options, ParsedArguments), String> {
    let mut specs: Vec<OptionSpec> = RECAP_OPTIONS.to_vec();
    specs.extend(FILTER_OPTIONS);
    specs.push(CWD_OPTION);
    let parsed = parse_arguments(argv, &specs).map_err(|message| format!("recap: {message}"))?;
    let options = parse_options(&parsed)?;
    Ok((options, parsed))
}

/// True when the kernel `inference` tab says automatic AI is on. Anything unreadable is closed.
fn automatic_ai_is_on(api: &mut dyn HostApi) -> bool {
    HostKernelSettings::read(api, "inference".to_string())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("automatic_ai").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Onboarding state read from the kernel `setup` tab: finished unless a wizard was started and
/// not completed. Used only when no client is attached to ask.
fn onboarding_finished_from_settings(api: &mut dyn HostApi) -> bool {
    HostKernelSettings::read(api, "setup".to_string())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|value| onboarding_finished(&value))
}

/// The onboarding rule: finished when no wizard was started or the started one completed.
pub fn onboarding_finished(setup: &Value) -> bool {
    let flag = |name: &str| setup.get(name).and_then(Value::as_bool).unwrap_or(false);
    !flag("started") || flag("completed")
}

/// Every pane of the tree, in tree order.
fn all_panes(api: &mut dyn HostApi) -> Vec<NodeId> {
    let mut found = Vec::new();
    let mut stack: Vec<NodeId> = HostTree::roots(api).into_iter().rev().collect();
    while let Some(node) = stack.pop() {
        let Some(view) = HostTree::get(api, node) else {
            continue;
        };
        if view.class == NodeClass::Pane {
            found.push(node);
        } else {
            stack.extend(HostTree::children(api, node).into_iter().rev());
        }
    }
    found
}

/// What the selectors see of one terminal pane, `None` for a pane that is no terminal (editors
/// and boards are never recapped).
fn terminal_facts(
    api: &mut dyn HostApi,
    pane: NodeId,
    caller: Option<NodeId>,
) -> Option<(PaneFacts, String)> {
    let view = HostTree::get(api, pane)?;
    if view.class != NodeClass::Pane || view.content_kind.as_deref() != Some(TERMINAL_CONTENT_KIND)
    {
        return None;
    }
    let state = view.agent.as_ref().and_then(|agent| {
        AgentStateName::from_name(match agent.activity.as_str() {
            "awaiting-approval" => "waiting-approval",
            other => other,
        })
    });
    let name = redact_secrets(&view.title).text;
    let cwd = view.cwd.as_ref().map(PathBuf::from);
    let facts = PaneFacts {
        session_name: String::new(),
        session_id: String::new(),
        pane_id: pane.0,
        name: name.clone(),
        short_name: None,
        project: cwd.clone(),
        cwd,
        kind: if view.agent.is_some() {
            PaneKind::Agent
        } else {
            PaneKind::Shell
        },
        agent: view.agent.as_ref().map(|agent| agent.signature.clone()),
        state,
        is_self: caller == Some(pane),
    };
    Some((facts, name))
}

/// The name of a terminal pane, `None` for a pane that does not exist or is no terminal.
fn terminal_name(api: &mut dyn HostApi, pane: NodeId) -> Option<String> {
    terminal_facts(api, pane, None).map(|(_, name)| name)
}

/// A record for a pane that was not recapped because of `state`.
fn blocked_record(pane: NodeId, name: &str, state: RecapPaneState, reason: &str) -> Value {
    json!({
        "type": "recap",
        "pane_id": pane.0.to_string(),
        "pane_name": name,
        "state": state.name(),
        "goals": [],
        "evidence_age_seconds": null,
        "evidence_stale": false,
        "window_truncated": false,
        "short": null,
        "long": null,
        "failure": reason,
    })
}

/// Who waits for the answer of a run.
enum Waiter {
    Cli {
        invocation: u64,
        options: Options,
    },
    Service {
        call: u64,
    },
    /// The in-pane recap line of one pane, with the agent session the run started under.
    Dot {
        pane: NodeId,
        session: Option<String>,
    },
}

struct Run {
    waiter: Waiter,
    lengths: Vec<RecapLength>,
    /// The unanswered `onboarding-state` client request and the panes it holds back.
    gate: Option<(u64, Vec<(NodeId, String)>)>,
    /// Panes that wait for a free job slot.
    queue: VecDeque<(NodeId, String)>,
    /// Job request id to pane.
    pending: HashMap<u64, (NodeId, String)>,
    /// The run-wide timeout timer.
    timer: Option<TimerId>,
    records: Vec<Value>,
    warnings: Vec<Value>,
    cancelled: bool,
}

impl Run {
    fn max_concurrency(&self) -> usize {
        match &self.waiter {
            Waiter::Cli { options, .. } => options.max_concurrency,
            Waiter::Service { .. } | Waiter::Dot { .. } => DEFAULT_MAX_CONCURRENCY,
        }
    }

    fn is_idle(&self) -> bool {
        self.gate.is_none() && self.queue.is_empty() && self.pending.is_empty()
    }
}

#[derive(Default)]
struct Runs {
    next: u64,
    runs: HashMap<u64, Run>,
}

thread_local! {
    static RUNS: RefCell<Runs> = RefCell::new(Runs::default());
    /// The client that answers `onboarding-state`: the most recently attached one.
    static CLIENT: RefCell<Option<ClientId>> = const { RefCell::new(None) };
}

/// A client attached: it becomes the one that answers `onboarding-state`, and it is sent the
/// recaps kept on panes that still hold their session.
pub fn on_client_attached(api: &mut dyn HostApi, client: ClientId) {
    CLIENT.with(|slot| *slot.borrow_mut() = Some(client));
    for pane in all_panes(api) {
        let Some(stored) = HostTree::metadata(api, pane).and_then(|b| DotRecap::from_payload(&b))
        else {
            continue;
        };
        if stored.has_text() && stored.is_current(session_of(api, pane).as_deref()) {
            queue_result(Some(client), &stored);
        }
    }
}

/// A client detached.
pub fn on_client_detached(client: ClientId) {
    CLIENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == Some(client) {
            *slot = None;
        }
    });
}

thread_local! {
    /// Messages to clients produced while an event was handled; the handler returns them.
    static OUTBOX: RefCell<Vec<Submission>> = const { RefCell::new(Vec::new()) };
}

/// Takes the messages queued since the last call.
pub fn take_outbox() -> Vec<Submission> {
    OUTBOX.with(|outbox| std::mem::take(&mut *outbox.borrow_mut()))
}

/// Queues one in-pane recap result for `client`, or for every client when `None`.
fn queue_result(client: Option<ClientId>, recap: &DotRecap) {
    let message = Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(crate::PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client,
        },
        channel: dot::RESULT_CHANNEL.to_string(),
        payload: recap.to_payload(),
    }));
    OUTBOX.with(|outbox| outbox.borrow_mut().push(message));
}

/// The agent session id a pane holds now; `None` for a shell or an unverified session.
fn session_of(api: &mut dyn HostApi, pane: NodeId) -> Option<String> {
    HostAgent::session(api, pane).map(|session| session.session_id)
}

/// Starts the in-pane recap of one pane. A pane that cannot be recapped is answered at once so
/// the client does not keep showing that it is working.
pub fn start_dot(api: &mut dyn HostApi, pane: u64) {
    let pane = NodeId(pane);
    let running = RUNS.with(|runs| {
        runs.borrow()
            .runs
            .values()
            .any(|run| matches!(&run.waiter, Waiter::Dot { pane: owner, .. } if *owner == pane))
    });
    if running {
        return;
    }
    let Some(name) = terminal_name(api, pane) else {
        queue_result(
            None,
            &DotRecap::failed(pane.0, "unavailable", "the pane is not a terminal"),
        );
        return;
    };
    let session = session_of(api, pane);
    let lengths = lengths_for("both").unwrap_or_default();
    let waiter = Waiter::Dot { pane, session };
    let timeout = Some(DEFAULT_TIMEOUT_SECONDS);
    if let Some(run) = begin(
        api,
        waiter,
        lengths,
        vec![(pane, name)],
        Vec::new(),
        timeout,
    ) {
        finish(api, run);
    }
}

fn emit(api: &mut dyn HostApi, invocation: u64, record: &Value) {
    // A closed output (the caller went away) is not an error the command can act on.
    let _ = HostCli::write_line(api, invocation, record.to_string());
}

fn refuse(api: &mut dyn HostApi, invocation: u64, message: &str) {
    let _ = HostCli::write_stderr(api, invocation, format!("{message}\n"));
    HostCli::finish(api, invocation, 2);
}

/// The selection of a command line could not be used: one `error` record, exit 2.
fn refuse_selection(api: &mut dyn HostApi, invocation: u64, message: &str) {
    emit(
        api,
        invocation,
        &json!({
            "type": "error",
            "command": "recap",
            "code": "invalid-selection",
            "message": message,
        }),
    );
    HostCli::finish(api, invocation, 2);
}

/// Starts the `recap` command line.
pub fn start_cli(api: &mut dyn HostApi, invocation: u64, argv: &[String], cwd: &str) {
    let (options, parsed) = match parse_args(argv) {
        Ok(parts) => parts,
        Err(message) => return refuse(api, invocation, &message),
    };
    let Some(lengths) = lengths_for(&options.length) else {
        return refuse(
            api,
            invocation,
            "recap: --length must be short, long or both",
        );
    };
    let filter = match PaneFilterArgs::from_parsed(&parsed).compile(Path::new(cwd), None) {
        Ok(filter) => filter,
        Err(message) => return refuse_selection(api, invocation, &message),
    };
    let caller = HostCli::caller_pane(api, invocation);
    let candidates: Vec<NodeId> = match HostCli::selected_panes(api, invocation) {
        Some(selection) => selection.panes,
        None => all_panes(api),
    };
    let mut chosen: Vec<(NodeId, String)> = Vec::new();
    for pane in candidates {
        if caller == Some(pane) && !options.include_self {
            continue;
        }
        let Some((facts, name)) = terminal_facts(api, pane, caller) else {
            continue;
        };
        if filter.accepts(&facts) {
            chosen.push((pane, name));
        }
    }
    chosen.sort_by_key(|(pane, _)| pane.0);
    chosen.dedup_by_key(|(pane, _)| pane.0);
    let mut warnings = Vec::new();
    if chosen.len() > options.max_panes {
        let skipped = chosen.len() - options.max_panes;
        chosen.truncate(options.max_panes);
        warnings.push(json!({
            "type": "warning",
            "message": "extra panes skipped",
            "skipped": skipped,
        }));
    }
    let timeout = options.timeout_seconds;
    let waiter = Waiter::Cli {
        invocation,
        options,
    };
    if let Some(run) = begin(api, waiter, lengths, chosen, warnings, Some(timeout)) {
        finish(api, run);
    }
}

/// Starts a run: checks the consent gate, then starts the jobs or denies the panes.
fn begin(
    api: &mut dyn HostApi,
    waiter: Waiter,
    lengths: Vec<RecapLength>,
    panes: Vec<(NodeId, String)>,
    warnings: Vec<Value>,
    timeout_seconds: Option<u32>,
) -> Option<Run> {
    let mut run = Run {
        waiter,
        lengths,
        gate: None,
        queue: VecDeque::new(),
        pending: HashMap::new(),
        timer: None,
        records: Vec::new(),
        warnings,
        cancelled: false,
    };
    if panes.is_empty() || !automatic_ai_is_on(api) {
        deny_all(&mut run, panes);
        return Some(run);
    }
    let client = CLIENT.with(|slot| *slot.borrow());
    match client {
        None => {
            if onboarding_finished_from_settings(api) {
                run.queue.extend(panes);
            } else {
                deny_all(&mut run, panes);
            }
        }
        Some(client) => {
            match HostClientRequests::request(
                api,
                client,
                "onboarding-state".to_string(),
                "{}".to_string(),
                GATE_TIMEOUT_MILLIS,
            ) {
                Ok(request) => run.gate = Some((request.0, panes)),
                Err(_) => deny_all(&mut run, panes),
            }
        }
    }
    if run.is_idle() {
        return Some(run);
    }
    if let Some(seconds) = timeout_seconds {
        run.timer = HostTimers::set(api, seconds.saturating_mul(1_000), None).ok();
    }
    fill_slots(api, &mut run);
    store_or_keep(api, run)
}

/// Time the client has to answer `onboarding-state`.
const GATE_TIMEOUT_MILLIS: u32 = 2_000;

fn deny_all(run: &mut Run, panes: Vec<(NodeId, String)>) {
    for (pane, name) in panes {
        run.records.push(blocked_record(
            pane,
            &name,
            RecapPaneState::Denied,
            "AI features are off or onboarding is not finished",
        ));
    }
}

/// Starts queued panes until `max_concurrency` jobs run.
fn fill_slots(api: &mut dyn HostApi, run: &mut Run) {
    let length_name = match (
        run.lengths.contains(&RecapLength::Short),
        run.lengths.contains(&RecapLength::Long),
    ) {
        (true, false) => "short",
        (false, true) => "long",
        _ => "both",
    };
    while run.pending.len() < run.max_concurrency() {
        let Some((pane, name)) = run.queue.pop_front() else {
            break;
        };
        let input = json!({"pane": pane.0, "length": length_name}).to_string();
        match HostJobs::start(api, JOB_KIND.to_string(), input.into_bytes()) {
            Ok(request) => {
                run.pending.insert(request.0, (pane, name));
            }
            Err(error) => run.records.push(blocked_record(
                pane,
                &name,
                RecapPaneState::Failed,
                &format!("could not start the recap job: {}", error.message),
            )),
        }
    }
}

/// Keeps a run that still waits for the gate or a job; hands back one that waits for nothing.
fn store_or_keep(api: &mut dyn HostApi, run: Run) -> Option<Run> {
    if run.is_idle() {
        if let Some(timer) = run.timer {
            HostTimers::cancel(api, timer);
        }
        return Some(run);
    }
    RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let id = runs.next;
        runs.next += 1;
        runs.runs.insert(id, run);
    });
    None
}

/// Takes the stored run for which `matches` holds.
fn take_run(matches: impl Fn(&Run) -> bool) -> Option<Run> {
    RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        let id = runs
            .runs
            .iter()
            .find(|(_, run)| matches(run))
            .map(|(id, _)| *id)?;
        runs.runs.remove(&id)
    })
}

/// The answer to `onboarding-state` (or its failure) arrived: starts the jobs or denies.
fn on_gate_answer(api: &mut dyn HostApi, request: RequestId, result: &RequestResult) -> bool {
    let Some(mut run) = take_run(|run| {
        run.gate
            .as_ref()
            .is_some_and(|(held, _)| *held == request.0)
    }) else {
        return false;
    };
    let panes = run.gate.take().map(|(_, panes)| panes).unwrap_or_default();
    let finished = matches!(result.outcome, Outcome::Ok)
        && HostClientRequests::take_answer(api, request)
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.get("finished").and_then(Value::as_bool))
            .unwrap_or(false);
    if finished {
        run.queue.extend(panes);
        fill_slots(api, &mut run);
    } else {
        deny_all(&mut run, panes);
    }
    if let Some(run) = store_or_keep(api, run) {
        finish(api, run);
    }
    true
}

/// A job ended: files its record, starts the next queued pane and finishes the run when it was
/// the last one.
pub fn on_completed(api: &mut dyn HostApi, result: &RequestResult) {
    let RequestRef::Call(request) = &result.request else {
        return;
    };
    if on_gate_answer(api, *request, result) {
        return;
    }
    let Some(mut run) = take_run(|run| run.pending.contains_key(&request.0)) else {
        return;
    };
    let Some((pane, name)) = run.pending.remove(&request.0) else {
        return;
    };
    let record = match &result.outcome {
        Outcome::Ok => HostJobs::take_output(api, *request)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or_else(|| {
                blocked_record(
                    pane,
                    &name,
                    RecapPaneState::Failed,
                    "the job returned no record",
                )
            }),
        Outcome::Cancelled => blocked_record(pane, &name, RecapPaneState::Failed, "cancelled"),
        Outcome::Failed(error) | Outcome::Refused(error) => {
            blocked_record(pane, &name, RecapPaneState::Failed, &error.message)
        }
        _ => blocked_record(
            pane,
            &name,
            RecapPaneState::Failed,
            "the job did not complete",
        ),
    };
    run.records.push(record);
    fill_slots(api, &mut run);
    if let Some(run) = store_or_keep(api, run) {
        finish(api, run);
    }
}

/// Stops every job of `run` and files a `failed` record with `reason` for each unfinished pane.
fn abandon(api: &mut dyn HostApi, run: &mut Run, reason: &str) {
    for (request, (pane, name)) in std::mem::take(&mut run.pending) {
        HostJobs::cancel(api, RequestId(request));
        run.records
            .push(blocked_record(pane, &name, RecapPaneState::Failed, reason));
    }
    for (pane, name) in std::mem::take(&mut run.queue) {
        run.records
            .push(blocked_record(pane, &name, RecapPaneState::Failed, reason));
    }
    if let Some((_, panes)) = run.gate.take() {
        for (pane, name) in panes {
            run.records
                .push(blocked_record(pane, &name, RecapPaneState::Failed, reason));
        }
    }
}

/// The command line was cancelled: stops its jobs and reports what finished.
pub fn on_cancelled(api: &mut dyn HostApi, invocation: u64) {
    let Some(mut run) = take_run(
        |run| matches!(&run.waiter, Waiter::Cli { invocation: owner, .. } if *owner == invocation),
    ) else {
        return;
    };
    abandon(api, &mut run, "cancelled");
    run.cancelled = true;
    if let Some(timer) = run.timer.take() {
        HostTimers::cancel(api, timer);
    }
    finish(api, run);
}

/// The run-wide timeout passed: panes still running are reported as failed.
pub fn on_timer(api: &mut dyn HostApi, timer: TimerId) {
    let Some(mut run) = take_run(|run| run.timer == Some(timer)) else {
        return;
    };
    run.timer = None;
    abandon(api, &mut run, "timed out");
    finish(api, run);
}

fn pane_number(record: &Value) -> u64 {
    record
        .get("pane_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse().ok())
        .unwrap_or(u64::MAX)
}

/// Writes the records of an ended run to its waiter.
fn finish(api: &mut dyn HostApi, mut run: Run) {
    run.records.sort_by_key(pane_number);
    match &run.waiter {
        Waiter::Cli {
            invocation,
            options,
        } => {
            let invocation = *invocation;
            if options.markdown {
                let entries: Vec<_> = run.records.iter().filter_map(pane_from_record).collect();
                let report = render_markdown_entries(&entries, run.cancelled, &run.lengths);
                let _ = HostCli::write_text(api, invocation, report);
                for warning in &run.warnings {
                    let message = warning["message"].as_str().unwrap_or("warning");
                    let _ = HostCli::write_stderr(api, invocation, format!("{message}\n"));
                }
            } else {
                for warning in &run.warnings {
                    emit(api, invocation, warning);
                }
                for record in &run.records {
                    emit(api, invocation, record);
                    if let Some((pane, _)) = pane_from_record(record) {
                        if let Some(warning) = pane_warning(&pane) {
                            emit(api, invocation, &warning);
                        }
                    }
                }
                let summary = summary_record(
                    run.records
                        .iter()
                        .map(|record| record["state"].as_str().unwrap_or("failed")),
                    run.cancelled,
                );
                emit(api, invocation, &summary);
            }
            HostCli::finish(api, invocation, 0);
        }
        Waiter::Service { call } => {
            let reply = json!({"recaps": run.records}).to_string();
            let _ = HostServices::reply(api, *call, Ok(reply));
        }
        Waiter::Dot { pane, session } => {
            let recap = match run.records.first() {
                Some(record) => DotRecap::from_record(pane.0, session.clone(), record),
                None => DotRecap::failed(pane.0, "failed", "the run produced no record"),
            };
            deliver_dot(api, *pane, recap);
        }
    }
}

/// Drops text whose session is no longer the pane's, keeps a summarized result on the pane and
/// sends the outcome to the clients.
fn deliver_dot(api: &mut dyn HostApi, pane: NodeId, recap: DotRecap) {
    let recap = if recap.is_current(session_of(api, pane).as_deref()) {
        recap
    } else {
        DotRecap::failed(
            pane.0,
            "stale",
            "the pane changed agent session while the recap was written",
        )
    };
    if recap.has_text() {
        // A pane that vanished meanwhile has nowhere to keep it; the clients still get the text.
        let _ = HostTree::set_metadata(api, pane, recap.to_payload());
    }
    queue_result(None, &recap);
}

/// What the `get` method of the service answers.
pub enum ServiceAnswer {
    Done(String),
    Deferred,
}

/// The `get` method: `{"panes": [<id>], "length": "short"|"long"|"both"}`.
pub fn service_get(
    api: &mut dyn HostApi,
    call: u64,
    payload: &str,
) -> Result<ServiceAnswer, PluginError> {
    let invalid = |message: &str| PluginError::new(ErrorCode::InvalidArgument, message);
    let value: Value =
        serde_json::from_str(payload).map_err(|_| invalid("the payload is not JSON"))?;
    let ids = value
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("panes is required"))?;
    let mut panes = Vec::new();
    for id in ids {
        let id = id.as_u64().ok_or_else(|| invalid("panes are integers"))?;
        panes.push(NodeId(id));
    }
    let length = value
        .get("length")
        .and_then(Value::as_str)
        .unwrap_or("both");
    let lengths =
        lengths_for(length).ok_or_else(|| invalid("length must be short, long or both"))?;
    let named: Vec<(NodeId, String)> = panes
        .into_iter()
        .filter_map(|pane| terminal_name(api, pane).map(|name| (pane, name)))
        .collect();
    if named.is_empty() {
        return Ok(ServiceAnswer::Done(json!({"recaps": []}).to_string()));
    }
    match begin(
        api,
        Waiter::Service { call },
        lengths,
        named,
        Vec::new(),
        None,
    ) {
        Some(mut run) => {
            run.records.sort_by_key(pane_number);
            Ok(ServiceAnswer::Done(
                json!({"recaps": run.records}).to_string(),
            ))
        }
        None => Ok(ServiceAnswer::Deferred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn options(words: &[&str]) -> Result<Options, String> {
        parse_args(&args(words)).map(|(options, _)| options)
    }

    #[test]
    fn defaults_cover_both_lengths_as_jsonl() {
        let options = options(&[]).expect("no options is valid");
        assert_eq!(options.length, "both");
        assert!(!options.markdown && !options.include_self);
        assert_eq!(options.max_panes, DEFAULT_MAX_PANES);
        assert_eq!(options.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        assert_eq!(options.max_concurrency, DEFAULT_MAX_CONCURRENCY);
    }

    #[test]
    fn options_accept_spaced_and_equals_forms() {
        let options = options(&[
            "--length=short",
            "--format",
            "markdown",
            "--max-panes",
            "3",
            "--timeout-seconds=5",
            "--max-concurrency",
            "2",
            "--include-self",
        ])
        .expect("valid");
        assert_eq!(options.length, "short");
        assert!(options.markdown && options.include_self);
        assert_eq!(options.max_panes, 3);
        assert_eq!(options.timeout_seconds, 5);
        assert_eq!(options.max_concurrency, 2);
    }

    #[test]
    fn ranges_are_inclusive_at_both_edges() {
        for (option, low, high) in [
            ("--timeout-seconds", 1, 600),
            ("--max-concurrency", 1, 8),
            ("--max-panes", 1, 64),
        ] {
            for value in [low, high] {
                assert!(
                    options(&[option, &value.to_string()]).is_ok(),
                    "{option} {value}"
                );
            }
            for value in [low - 1, high + 1] {
                assert!(
                    options(&[option, &value.to_string()]).is_err(),
                    "{option} {value}"
                );
            }
            assert!(options(&[option, "many"]).is_err());
        }
    }

    #[test]
    fn length_error_uses_the_documented_message() {
        assert_eq!(
            options(&["--length", "medium"]),
            Err("recap: --length must be short, long or both".to_string())
        );
    }

    #[test]
    fn bad_values_and_unknown_options_are_usage_errors() {
        for bad in [
            args(&["--length", "medium"]),
            args(&["--length"]),
            args(&["--format", "xml"]),
            args(&["--include-self=yes"]),
            args(&["--bogus"]),
            args(&["stray"]),
        ] {
            assert!(parse_args(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_blocked_record_has_the_record_shape_without_text() {
        let record = blocked_record(NodeId(7), "agent", RecapPaneState::Denied, "off");
        assert_eq!(record["type"], "recap");
        assert_eq!(record["pane_id"], "7");
        assert_eq!(record["state"], "denied");
        assert!(record["short"].is_null());
        assert_eq!(record["failure"], "off");
    }

    #[test]
    fn onboarding_is_finished_unless_a_started_wizard_is_incomplete() {
        assert!(onboarding_finished(&json!({})));
        assert!(onboarding_finished(
            &json!({"started": true, "completed": true})
        ));
        assert!(!onboarding_finished(
            &json!({"started": true, "completed": false})
        ));
    }

    #[test]
    fn records_sort_by_numeric_pane_id() {
        let mut records = vec![
            blocked_record(NodeId(10), "b", RecapPaneState::Idle, "x"),
            blocked_record(NodeId(9), "a", RecapPaneState::Idle, "x"),
        ];
        records.sort_by_key(pane_number);
        assert_eq!(records[0]["pane_id"], "9");
    }
}
