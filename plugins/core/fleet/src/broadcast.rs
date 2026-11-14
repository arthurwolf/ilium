//! `ilium broadcast`: sends one message to every agent, in every running session on this
//! machine, that passes the shared pane selection (see `pane_filter`).
//!
//! Delivery is the same as the client's "Send message to all": the text is inserted into each
//! agent's composer and submitted with Enter (a multi-line text goes in as one bracketed paste).
//! The command's own session is reached through `send-input` intents; every other session gets
//! one `broadcast` cross-session request with its recipients. With `--when-idle`, busy agents
//! get the message through their prompt queue instead, so it is sent when their current turn
//! finishes (a `broadcast` request with `queue: true` targets, to the own session as well).
//!
//! Recipients are live agent panes only; shells, editors, boards and agents that are not running
//! are never typed into. The pane running this command is skipped unless `--include-self` is
//! given. An extension on `ilium.core.fleet/before-broadcast` may rewrite the message or end the
//! broadcast; a sent broadcast is announced on `ilium.core.fleet/broadcast-sent`.
//!
//! Output is JSONL: a `progress` record, a `warning` per session that could not be read, one
//! `result` per recipient (`outcome` is `delivered`, `queued`, `failed`, `skipped` or, with
//! `--dry-run`, `would-send`/`would-queue`), then one `summary`. The exit status is 1 when
//! nothing was selected or any delivery or session failed, 2 for a request that cannot run.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ilium_plugin_sdk::host::{
    CrossRequest, HostCli, HostExtensions, HostFs, HostSessions, HostTopics, Level,
};
use ilium_plugin_sdk::log::log;
use ilium_plugin_sdk::types::{
    Delivery, FsPath, Intent, NodeId, Outcome, RequestId, RequestRef, RequestResult, SendInputSpec,
    Submission,
};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::jobs::{emit, error_record, unreachable_warning, Routes, Step};
use crate::pane_filter::{
    parse_arguments, AgentStateName, OptionSpec, PaneFacts, PaneFilter, PaneFilterArgs, PaneKind,
    CWD_OPTION, FILTER_OPTIONS,
};
use crate::scan::{live_sessions, Scan, SessionPanes, SessionRef};

const COMMAND: &str = "broadcast";
/// The token that reads the message from standard input instead.
const STDIN_TOKEN: &str = "-";
/// The most standard input read for a message.
const STDIN_LIMIT: u32 = 1024 * 1024;
/// The most bytes read from `--file`.
const FILE_LIMIT: u32 = 1024 * 1024;
/// The scope `--file` paths belong to.
const FILE_SCOPE: &str = "project";
const DEFAULT_TIMEOUT_SECONDS: u64 = 15;
/// The hook that may rewrite or end a broadcast.
pub const BEFORE_BROADCAST: &str = "ilium.core.fleet/before-broadcast";
/// The topic that announces a sent broadcast.
pub const BROADCAST_SENT: &str = "ilium.core.fleet/broadcast-sent";

/// The options of `ilium broadcast`.
pub fn option_specs() -> Vec<OptionSpec> {
    let mut specs = FILTER_OPTIONS.to_vec();
    specs.push(CWD_OPTION);
    specs.extend([
        OptionSpec {
            long: "file",
            short: None,
            takes_value: true,
        },
        OptionSpec {
            long: "when-idle",
            short: None,
            takes_value: false,
        },
        OptionSpec {
            long: "dry-run",
            short: None,
            takes_value: false,
        },
        OptionSpec {
            long: "include-self",
            short: None,
            takes_value: false,
        },
        OptionSpec {
            long: "timeout-seconds",
            short: None,
            takes_value: true,
        },
        // Internal exact project fence for resource notices; an ordinary broadcast keeps its
        // documented selector behaviour.
        OptionSpec {
            long: "exact-project",
            short: None,
            takes_value: true,
        },
    ]);
    specs
}

/// What happens, or happened, to one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fate {
    Delivered,
    Queued,
    WouldSend,
    WouldQueue,
    Skipped(String),
    Failed(String),
}

impl Fate {
    fn name(&self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Queued => "queued",
            Self::WouldSend => "would-send",
            Self::WouldQueue => "would-queue",
            Self::Skipped(_) => "skipped",
            Self::Failed(_) => "failed",
        }
    }

    fn reason(&self) -> Option<&str> {
        match self {
            Self::Skipped(reason) | Self::Failed(reason) => Some(reason),
            _ => None,
        }
    }
}

/// How one recipient receives the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Now,
    Queue,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    recipients: usize,
    delivered: usize,
    queued: usize,
    planned: usize,
    skipped: usize,
    failed: usize,
}

impl Tally {
    fn count(&mut self, fate: &Fate) {
        match fate {
            Fate::Delivered => self.delivered += 1,
            Fate::Queued => self.queued += 1,
            Fate::WouldSend | Fate::WouldQueue => self.planned += 1,
            Fate::Skipped(_) => self.skipped += 1,
            Fate::Failed(_) => self.failed += 1,
        }
        if !matches!(fate, Fate::Skipped(_)) {
            self.recipients += 1;
        }
    }
}

/// The flags of one `ilium broadcast`.
#[derive(Debug, Clone)]
struct Options {
    when_idle: bool,
    dry_run: bool,
    include_self: bool,
    timeout_seconds: u64,
    exact_project: Option<PathBuf>,
}

/// Where the message comes from.
#[derive(Debug, PartialEq, Eq)]
enum Intake {
    Text(String),
    File(String),
    Stdin,
}

/// One agent the message is meant for.
#[derive(Debug)]
struct Target {
    session: usize,
    facts: PaneFacts,
    route: Route,
    fate: Option<Fate>,
}

#[derive(Debug, PartialEq, Eq)]
enum Stage {
    /// Waiting for standard input.
    Stdin(u64),
    /// Reading the panes of the sessions.
    Scanning,
    /// Waiting for the `before-broadcast` extensions.
    Hook(u64),
    /// Waiting for the deliveries.
    Delivering,
}

/// One running `ilium broadcast`.
#[derive(Debug)]
pub struct BroadcastJob {
    invocation: u64,
    options: Options,
    filter: PaneFilter,
    caller: Option<NodeId>,
    /// The pane ids `--pane` named in the own session; `None` selects every pane.
    only_panes: Option<Vec<u64>>,
    message: String,
    stage: Stage,
    scan: Option<Scan>,
    sessions: Vec<SessionRef>,
    unreachable: usize,
    targets: Vec<Target>,
    /// Cross-session request id -> the targets it carries.
    carried: HashMap<u64, Vec<usize>>,
    /// Submission correlation -> its target.
    submitted: HashMap<u64, usize>,
}

impl BroadcastJob {
    /// Parses the command line and starts it; `None` when the command is already over.
    pub fn start(
        api: &mut dyn HostApi,
        invocation: u64,
        argv: &[String],
        cwd: &str,
        routes: &mut Routes,
        out: &mut Vec<Submission>,
    ) -> Option<Self> {
        let prepared = prepare(argv, cwd);
        let (options, filter, intake) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => return refuse(api, invocation, &message),
        };
        let caller = HostCli::caller_pane(api, invocation);
        let only_panes = HostCli::selected_panes(api, invocation)
            .filter(|selection| !selection.all)
            .map(|selection| selection.panes.iter().map(|pane| pane.0).collect());
        let mut job = Self {
            invocation,
            options,
            filter,
            caller,
            only_panes,
            message: String::new(),
            stage: Stage::Scanning,
            scan: None,
            sessions: Vec::new(),
            unreachable: 0,
            targets: Vec::new(),
            carried: HashMap::new(),
            submitted: HashMap::new(),
        };
        let raw = match intake {
            Intake::Text(text) => text,
            Intake::File(path) => match read_file(api, &path) {
                Ok(text) => text,
                Err(message) => return refuse(api, invocation, &message),
            },
            Intake::Stdin => match HostCli::read_stdin(api, invocation, STDIN_LIMIT) {
                Ok(request) => {
                    routes.call(request, invocation);
                    job.stage = Stage::Stdin(request);
                    return Some(job);
                }
                Err(error) => {
                    return refuse(
                        api,
                        invocation,
                        &format!(
                            "cannot read the message from standard input: {}",
                            error.message
                        ),
                    )
                }
            },
        };
        job.with_message(api, &raw, routes, out)
    }

    /// Continues with the message in hand. `Some(self)` while the command still waits.
    fn with_message(
        mut self,
        api: &mut dyn HostApi,
        raw: &str,
        routes: &mut Routes,
        out: &mut Vec<Submission>,
    ) -> Option<Self> {
        self.message = match normalize_message(raw) {
            Ok(message) => message,
            Err(message) => return refuse(api, self.invocation, &message),
        };
        self.stage = Stage::Scanning;
        let sessions = live_sessions(api);
        emit(
            api,
            self.invocation,
            &json!({
                "type": "progress",
                "command": COMMAND,
                "stage": if self.options.dry_run { "planning" } else { "sending" },
                "sessions": sessions.len(),
                "message_bytes": self.message.len(),
            }),
        );
        let ask_others = self.only_panes.is_none();
        let scan = Scan::start(api, sessions.clone(), self.caller, true, ask_others);
        for request in scan.pending_requests() {
            routes.call(request, self.invocation);
        }
        self.sessions = sessions;
        self.scan = Some(scan);
        match self.advance(api, routes, out) {
            Step::Pending => Some(self),
            Step::Done => None,
        }
    }

    /// Takes in one answer.
    pub fn on_completed(
        mut self,
        api: &mut dyn HostApi,
        result: &RequestResult,
        routes: &mut Routes,
        out: &mut Vec<Submission>,
    ) -> Option<Self> {
        match &result.request {
            RequestRef::Submission(correlation) => {
                if let Some(index) = self.submitted.remove(correlation) {
                    self.targets[index].fate = Some(fate_of_submission(&result.outcome));
                }
            }
            RequestRef::Call(RequestId(request)) => {
                let request = *request;
                if self.stage == Stage::Stdin(request) {
                    let bytes = HostCli::take_stdin(api, request).unwrap_or_default();
                    let raw = String::from_utf8_lossy(&bytes).into_owned();
                    return self.with_message(api, &raw, routes, out);
                }
                if self.stage == Stage::Hook(request) {
                    self.on_hook_answer(api, request, &result.outcome);
                    self.stage = Stage::Delivering;
                    self.deliver(api, routes, out);
                } else if let Some(indices) = self.carried.remove(&request) {
                    self.on_cross_reply(api, request, &indices, &result.outcome);
                } else if let Some(scan) = self.scan.as_mut() {
                    scan.on_completed(api, request, &result.outcome);
                }
            }
        }
        match self.advance(api, routes, out) {
            Step::Pending => Some(self),
            Step::Done => None,
        }
    }

    /// Moves to the next stage when the current one is complete.
    fn advance(
        &mut self,
        api: &mut dyn HostApi,
        routes: &mut Routes,
        out: &mut Vec<Submission>,
    ) -> Step {
        if self.stage == Stage::Scanning && self.scan.as_ref().is_some_and(Scan::is_complete) {
            self.plan(api, routes, out);
        }
        if self.stage == Stage::Delivering && self.carried.is_empty() && self.submitted.is_empty() {
            self.finish(api);
            return Step::Done;
        }
        Step::Pending
    }

    /// Picks the recipients out of the pane lists, then asks the extensions and delivers.
    fn plan(&mut self, api: &mut dyn HostApi, routes: &mut Routes, out: &mut Vec<Submission>) {
        let Some(scan) = self.scan.take() else {
            return;
        };
        for (index, (session, panes)) in scan.sessions.iter().zip(scan.panes).enumerate() {
            match panes {
                Some(SessionPanes::Panes(panes)) => {
                    for facts in panes {
                        if self.is_recipient(session, &facts) {
                            self.targets.push(self.target_for(index, facts));
                        }
                    }
                }
                Some(SessionPanes::Unreachable(message)) => {
                    self.unreachable += 1;
                    emit(
                        api,
                        self.invocation,
                        &unreachable_warning(COMMAND, &session.name, &message),
                    );
                }
                None => {}
            }
        }
        let deliverable = self
            .targets
            .iter()
            .filter(|target| target.fate.is_none())
            .count();
        self.stage = Stage::Delivering;
        if self.options.dry_run || deliverable == 0 {
            return;
        }
        let payload = json!({
            "message": self.message,
            "targets": self
                .targets
                .iter()
                .filter(|target| target.fate.is_none())
                .map(|target| json!({"session": target.facts.session_name, "pane_id": target.facts.pane_id}))
                .collect::<Vec<_>>(),
            "source": "cli",
        });
        match HostExtensions::run_hook(api, BEFORE_BROADCAST.to_owned(), payload.to_string()) {
            Ok(RequestId(request)) => {
                routes.call(request, self.invocation);
                self.stage = Stage::Hook(request);
            }
            // No extension point to ask (not granted, or no host): send the message as it is.
            Err(_) => self.deliver(api, routes, out),
        }
    }

    fn is_recipient(&self, session: &SessionRef, facts: &PaneFacts) -> bool {
        facts.kind == PaneKind::Agent
            && self.filter.accepts(facts)
            && self
                .options
                .exact_project
                .as_deref()
                .is_none_or(|project| facts.project.as_deref() == Some(project))
            && (!session.is_self
                || self
                    .only_panes
                    .as_ref()
                    .is_none_or(|panes| panes.contains(&facts.pane_id)))
    }

    fn target_for(&self, session: usize, facts: PaneFacts) -> Target {
        let route = choose_route(facts.state, self.options.when_idle);
        let fate = if facts.is_self && !self.options.include_self {
            Some(Fate::Skipped(
                "the pane running this command (use --include-self)".to_owned(),
            ))
        } else if self.options.dry_run {
            Some(match route {
                Route::Now => Fate::WouldSend,
                Route::Queue => Fate::WouldQueue,
            })
        } else {
            None
        };
        Target {
            session,
            facts,
            route,
            fate,
        }
    }

    /// Applies the answer of the `before-broadcast` extensions.
    fn on_hook_answer(&mut self, api: &mut dyn HostApi, request: u64, outcome: &Outcome) {
        match outcome {
            Outcome::Refused(error) => {
                self.skip_all(&format!(
                    "a before-broadcast extension vetoed the message: {}",
                    error.message
                ));
            }
            Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => {
                let reply = HostExtensions::take_reply(api, RequestId(request));
                let rewritten = reply
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| {
                        value
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    });
                if let Some(message) = rewritten {
                    if message.trim().is_empty() {
                        self.skip_all("a before-broadcast extension ended the broadcast");
                    } else {
                        self.message = message;
                    }
                }
            }
            Outcome::Failed(error) => log(
                Level::Warn,
                &format!("a before-broadcast extension failed: {}", error.message),
            ),
            Outcome::Duplicate(_) | Outcome::Cancelled => {}
        }
    }

    fn skip_all(&mut self, reason: &str) {
        for target in self
            .targets
            .iter_mut()
            .filter(|target| target.fate.is_none())
        {
            target.fate = Some(Fate::Skipped(reason.to_owned()));
        }
    }

    /// Sends the message to every target that still waits for it.
    fn deliver(&mut self, api: &mut dyn HostApi, routes: &mut Routes, out: &mut Vec<Submission>) {
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for index in 0..self.targets.len() {
            let target = &self.targets[index];
            if target.fate.is_some() {
                continue;
            }
            let own = self.sessions[target.session].is_self;
            if own && target.route == Route::Now {
                let submission = self.input_for(target.facts.pane_id);
                routes.submission(submission.correlation, self.invocation);
                self.submitted.insert(submission.correlation, index);
                out.push(submission);
            } else {
                groups.entry(target.session).or_default().push(index);
            }
        }
        for (session, indices) in groups {
            let socket = self.sessions[session].socket_id.clone();
            let arguments = json!({
                "text": self.message,
                "targets": indices
                    .iter()
                    .map(|index| {
                        let target = &self.targets[*index];
                        json!({
                            "pane_id": target.facts.pane_id,
                            "queue": target.route == Route::Queue,
                        })
                    })
                    .collect::<Vec<_>>(),
                "timeout_seconds": self.options.timeout_seconds,
            });
            let sent = HostSessions::send(
                api,
                CrossRequest {
                    session: socket,
                    command: COMMAND.to_owned(),
                    arguments_json: arguments.to_string(),
                },
            );
            match sent {
                Ok(RequestId(request)) => {
                    routes.call(request, self.invocation);
                    self.carried.insert(request, indices);
                }
                Err(error) => {
                    for index in indices {
                        self.targets[index].fate = Some(Fate::Failed(error.message.clone()));
                    }
                }
            }
        }
    }

    /// The `send-input` that types the message into `pane` and submits it.
    fn input_for(&self, pane: u64) -> Submission {
        let delivery = if self.message.contains('\n') {
            Delivery::Pasted
        } else {
            Delivery::Submitted
        };
        Submission::keyed(
            Intent::SendInput(SendInputSpec {
                pane: NodeId(pane),
                text: self.message.clone(),
                delivery,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: true,
            }),
            format!("broadcast-{pane}-{}", self.message.len()),
        )
    }

    /// Applies the answer of one session to the targets it carried.
    fn on_cross_reply(
        &mut self,
        api: &mut dyn HostApi,
        request: u64,
        indices: &[usize],
        outcome: &Outcome,
    ) {
        let reply = HostSessions::take_reply(api, RequestId(request));
        let panes: Vec<u64> = indices
            .iter()
            .map(|index| self.targets[*index].facts.pane_id)
            .collect();
        let fates = match outcome {
            Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => {
                fates_of_reply(reply.as_deref(), &panes)
            }
            Outcome::Failed(error) | Outcome::Refused(error) => {
                vec![Fate::Failed(error.message.clone()); panes.len()]
            }
            Outcome::Duplicate(_) | Outcome::Cancelled => {
                vec![Fate::Failed("the request was not carried out".to_owned()); panes.len()]
            }
        };
        for (index, fate) in indices.iter().zip(fates) {
            self.targets[*index].fate = Some(fate);
        }
    }

    /// Prints the results and the summary.
    fn finish(&mut self, api: &mut dyn HostApi) {
        let mut tally = Tally::default();
        for target in &self.targets {
            let fate = target
                .fate
                .clone()
                .unwrap_or_else(|| Fate::Failed("no answer".to_owned()));
            tally.count(&fate);
            let mut record = target.facts.to_json();
            record["type"] = json!("result");
            record["command"] = json!(COMMAND);
            record["outcome"] = json!(fate.name());
            if let Some(reason) = fate.reason() {
                record["reason"] = json!(reason);
            }
            emit(api, self.invocation, &record);
        }
        if tally.delivered + tally.queued > 0 {
            let payload = json!({
                "targets": tally.delivered + tally.queued,
                "length": self.message.len(),
            });
            if let Err(error) =
                HostTopics::publish(api, BROADCAST_SENT.to_owned(), payload.to_string())
            {
                log(
                    Level::Warn,
                    &format!(
                        "the broadcast-sent topic was not published: {}",
                        error.message
                    ),
                );
            }
        }
        let ok = tally.recipients > 0 && tally.failed == 0 && self.unreachable == 0;
        emit(
            api,
            self.invocation,
            &json!({
                "type": "summary",
                "command": COMMAND,
                "ok": ok,
                "dry_run": self.options.dry_run,
                "sessions": self.sessions.len(),
                "unreachable_sessions": self.unreachable,
                "recipients": tally.recipients,
                "delivered": tally.delivered,
                "queued": tally.queued,
                "planned": tally.planned,
                "skipped": tally.skipped,
                "failed": tally.failed,
            }),
        );
        HostCli::finish(api, self.invocation, u8::from(!ok));
    }
}

/// Reports a request that cannot run and ends the command.
fn refuse(api: &mut dyn HostApi, invocation: u64, message: &str) -> Option<BroadcastJob> {
    emit(
        api,
        invocation,
        &error_record(COMMAND, "invalid-request", message),
    );
    HostCli::finish(api, invocation, 2);
    None
}

type Prepared = (Options, PaneFilter, Intake);

/// Parses the options, the selection and the message source.
fn prepare(argv: &[String], cwd: &str) -> Result<Prepared, String> {
    let parsed = parse_arguments(argv, &option_specs())?;
    let timeout_seconds = match parsed.last("timeout-seconds") {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(text) => text
            .parse::<u64>()
            .ok()
            .filter(|seconds| (1..=600).contains(seconds))
            .ok_or_else(|| format!("invalid value {text:?} for --timeout-seconds (1 to 600)"))?,
    };
    let options = Options {
        when_idle: parsed.has("when-idle"),
        dry_run: parsed.has("dry-run"),
        include_self: parsed.has("include-self"),
        timeout_seconds,
        exact_project: parsed.last("exact-project").map(PathBuf::from),
    };
    let base = parsed.last("cwd").unwrap_or(cwd);
    let filter = PaneFilterArgs::from_parsed(&parsed).compile(Path::new(base), None)?;
    let intake = match (parsed.last("file"), parsed.words.as_slice()) {
        (Some(_), [_, ..]) => return Err("the message words conflict with --file".to_owned()),
        (Some(path), []) => Intake::File(path.to_owned()),
        (None, []) => return Err("a message or --file is required".to_owned()),
        (None, [word]) if word == STDIN_TOKEN => Intake::Stdin,
        (None, words) => Intake::Text(words.join(" ")),
    };
    Ok((options, filter, intake))
}

/// The text of `--file`, read from the project.
fn read_file(api: &mut dyn HostApi, path: &str) -> Result<String, String> {
    let bytes = HostFs::read(
        api,
        FsPath {
            scope: FILE_SCOPE.to_owned(),
            path: path.to_owned(),
        },
        0,
        FILE_LIMIT,
    )
    .map_err(|error| format!("cannot read message file {path:?}: {}", error.message))?;
    String::from_utf8(bytes).map_err(|_| format!("message file {path:?} is not UTF-8 text"))
}

/// The message with Windows line endings and trailing newlines removed. An empty one is refused.
fn normalize_message(raw: &str) -> Result<String, String> {
    let message = raw.replace("\r\n", "\n");
    let message = message.trim_end_matches('\n');
    if message.trim().is_empty() {
        return Err("the message is empty".to_owned());
    }
    Ok(message.to_owned())
}

/// `--when-idle` queues the message for a busy agent; every other agent gets it now.
fn choose_route(state: Option<AgentStateName>, when_idle: bool) -> Route {
    let is_idle = state.is_some_and(AgentStateName::is_idle);
    if when_idle && !is_idle {
        Route::Queue
    } else {
        Route::Now
    }
}

fn fate_of_submission(outcome: &Outcome) -> Fate {
    match outcome {
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => Fate::Delivered,
        Outcome::Duplicate(_) => {
            Fate::Skipped("an identical message was sent to this pane a moment ago".to_owned())
        }
        Outcome::Refused(error) | Outcome::Failed(error) => Fate::Failed(error.message.clone()),
        Outcome::Cancelled => Fate::Failed("the delivery was cancelled".to_owned()),
    }
}

/// The fate of each of `panes` from the reply of a `broadcast` cross-session request.
fn fates_of_reply(reply: Option<&str>, panes: &[u64]) -> Vec<Fate> {
    let failed = |reason: &str| vec![Fate::Failed(reason.to_owned()); panes.len()];
    let Some(text) = reply else {
        return failed("the session sent no answer");
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return failed("the session answered nonsense");
    };
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return failed(message);
    }
    let Some(entries) = value.as_array() else {
        return failed("the session answered something else than a list");
    };
    panes
        .iter()
        .map(|pane| {
            let Some(entry) = entries
                .iter()
                .find(|entry| entry.get("pane_id").and_then(Value::as_u64) == Some(*pane))
            else {
                return Fate::Failed("the session did not report this pane".to_owned());
            };
            let reason = entry
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("no reason given")
                .to_owned();
            match entry.get("outcome").and_then(Value::as_str) {
                Some("delivered") => Fate::Delivered,
                Some("queued") => Fate::Queued,
                Some("skipped") => Fate::Skipped(reason),
                _ => Fate::Failed(reason),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn message_words_join_and_line_endings_normalise() {
        let (_, _, intake) = prepare(&words(&["please", "re-read", "the rules"]), "/").unwrap();
        assert_eq!(intake, Intake::Text("please re-read the rules".to_owned()));
        let (_, _, intake) = prepare(&words(&["-"]), "/").unwrap();
        assert_eq!(intake, Intake::Stdin);
        assert_eq!(
            normalize_message("line one\r\nline two\r\n").unwrap(),
            "line one\nline two"
        );
        assert!(normalize_message("  \n").is_err());
    }

    #[test]
    fn a_message_or_file_is_required_and_selection_flags_parse() {
        assert!(prepare(&[], "/").is_err());
        assert!(prepare(&words(&["--file", "notice.md", "hello"]), "/").is_err());
        let (options, _, intake) = prepare(
            &words(&[
                "--project",
                "ilium,/w/lumen",
                "--regex",
                "review|audit",
                "--invert",
                "--agent",
                "codex",
                "--state",
                "idle,working",
                "--when-idle",
                "hello",
            ]),
            "/",
        )
        .unwrap();
        assert!(options.when_idle && !options.dry_run && !options.include_self);
        assert_eq!(intake, Intake::Text("hello".to_owned()));
        let (_, _, intake) = prepare(&words(&["--file", "notice.md"]), "/").unwrap();
        assert_eq!(intake, Intake::File("notice.md".to_owned()));
    }

    #[test]
    fn a_timeout_outside_the_range_is_refused() {
        assert!(prepare(&words(&["--timeout-seconds", "0", "hi"]), "/").is_err());
        assert!(prepare(&words(&["--timeout-seconds", "601", "hi"]), "/").is_err());
        let (options, _, _) = prepare(&words(&["--timeout-seconds", "30", "hi"]), "/").unwrap();
        assert_eq!(options.timeout_seconds, 30);
    }

    #[test]
    fn when_idle_queues_only_for_busy_agents() {
        assert_eq!(
            choose_route(Some(AgentStateName::Working), false),
            Route::Now
        );
        assert_eq!(
            choose_route(Some(AgentStateName::Working), true),
            Route::Queue
        );
        assert_eq!(choose_route(Some(AgentStateName::Done), true), Route::Now);
        assert_eq!(choose_route(Some(AgentStateName::Idle), true), Route::Now);
    }

    #[test]
    fn urgent_resource_selection_never_reaches_a_neighboring_project() {
        let job_options = Options {
            when_idle: true,
            dry_run: false,
            include_self: false,
            timeout_seconds: 15,
            exact_project: Some(PathBuf::from("/workspace/project")),
        };
        let facts = |project: &str| PaneFacts {
            session_name: "default".to_owned(),
            session_id: "default".to_owned(),
            pane_id: 1,
            name: "agent".to_owned(),
            short_name: None,
            project: Some(PathBuf::from(project)),
            cwd: None,
            kind: PaneKind::Agent,
            agent: Some("Codex".to_owned()),
            state: Some(AgentStateName::Idle),
            is_self: false,
        };
        let fits = |facts: &PaneFacts| {
            job_options
                .exact_project
                .as_deref()
                .is_none_or(|project| facts.project.as_deref() == Some(project))
        };
        assert!(fits(&facts("/workspace/project")));
        assert!(!fits(&facts("/workspace/project/nested")));
        assert!(!fits(&facts("/workspace")));
    }

    #[test]
    fn tally_counts_skips_apart_from_recipients() {
        let mut tally = Tally::default();
        for fate in [
            Fate::Delivered,
            Fate::Queued,
            Fate::Failed("x".to_owned()),
            Fate::Skipped("self".to_owned()),
        ] {
            tally.count(&fate);
        }
        assert_eq!(tally.recipients, 3);
        assert_eq!(
            (tally.delivered, tally.queued, tally.failed, tally.skipped),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn a_session_reply_maps_to_a_fate_per_pane() {
        let reply = r#"[{"pane_id":1,"outcome":"delivered"},{"pane_id":2,"outcome":"queued"},
            {"pane_id":3,"outcome":"failed","reason":"gone"}]"#;
        assert_eq!(
            fates_of_reply(Some(reply), &[1, 2, 3, 4]),
            vec![
                Fate::Delivered,
                Fate::Queued,
                Fate::Failed("gone".to_owned()),
                Fate::Failed("the session did not report this pane".to_owned()),
            ]
        );
        assert_eq!(
            fates_of_reply(Some(r#"{"error":"no route"}"#), &[1]),
            vec![Fate::Failed("no route".to_owned())]
        );
        assert_eq!(fates_of_reply(None, &[1]).len(), 1);
    }
}
