//! The running command lines of the plugin and the host requests each one waits for.
//!
//! A command line is a small state machine: it starts on `cli-invoked`, asks the host for
//! something (another session's panes, a service reply, the standard input) and continues when
//! the matching `request-completed` arrives. `Routes` remembers which command line owns which
//! request; the plugin instance is single-threaded, so the state lives in one thread-local cell.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::host::HostCli;
use ilium_plugin_sdk::types::{RequestRef, RequestResult, Submission};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::broadcast::BroadcastJob;
use crate::panes::PanesJob;

/// Which command line owns which request.
#[derive(Debug, Default)]
pub struct Routes {
    calls: HashMap<u64, u64>,
    submissions: HashMap<u64, u64>,
}

impl Routes {
    /// `invocation` waits for the host call `request`.
    pub fn call(&mut self, request: u64, invocation: u64) {
        self.calls.insert(request, invocation);
    }

    /// `invocation` waits for the outcome of the submission `correlation`.
    pub fn submission(&mut self, correlation: u64, invocation: u64) {
        self.submissions.insert(correlation, invocation);
    }

    fn owner(&mut self, request: &RequestRef) -> Option<u64> {
        match request {
            RequestRef::Call(id) => self.calls.remove(&id.0),
            RequestRef::Submission(correlation) => self.submissions.remove(correlation),
        }
    }

    fn forget(&mut self, invocation: u64) {
        self.calls.retain(|_, owner| *owner != invocation);
        self.submissions.retain(|_, owner| *owner != invocation);
    }
}

/// Whether a command line has more to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Pending,
    Done,
}

enum Job {
    Panes(Box<PanesJob>),
    Broadcast(Box<BroadcastJob>),
}

#[derive(Default)]
struct State {
    jobs: HashMap<u64, Job>,
    routes: Routes,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Starts the command line `name` (`panes` or `broadcast`) as invocation `invocation`.
pub fn start(
    api: &mut dyn HostApi,
    invocation: u64,
    name: &str,
    argv: &[String],
    cwd: &str,
    out: &mut Vec<Submission>,
) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let State { jobs, routes } = &mut *state;
        let job = match name {
            "panes" => PanesJob::start(api, invocation, argv, cwd, routes)
                .map(|job| Job::Panes(Box::new(job))),
            "broadcast" => BroadcastJob::start(api, invocation, argv, cwd, routes, out)
                .map(|job| Job::Broadcast(Box::new(job))),
            _ => {
                emit(
                    api,
                    invocation,
                    &error_record(name, "unknown-command", "this plugin has no such command"),
                );
                HostCli::finish(api, invocation, 2);
                None
            }
        };
        if let Some(job) = job {
            jobs.insert(invocation, job);
        }
    });
}

/// Continues the command line that waits for `result`; results of nobody are ignored.
pub fn on_completed(api: &mut dyn HostApi, result: &RequestResult, out: &mut Vec<Submission>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let State { jobs, routes } = &mut *state;
        let Some(invocation) = routes.owner(&result.request) else {
            return;
        };
        let Some(job) = jobs.remove(&invocation) else {
            return;
        };
        let next = match job {
            Job::Panes(mut job) => match job.on_completed(api, result, routes) {
                Step::Pending => Some(Job::Panes(job)),
                Step::Done => None,
            },
            Job::Broadcast(job) => (*job)
                .on_completed(api, result, routes, out)
                .map(|job| Job::Broadcast(Box::new(job))),
        };
        if let Some(job) = next {
            jobs.insert(invocation, job);
        }
    });
}

/// A cancelled command line stops waiting.
pub fn on_cancelled(invocation: u64) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.jobs.remove(&invocation);
        state.routes.forget(invocation);
    });
}

/// Writes one JSONL record.
pub fn emit(api: &mut dyn HostApi, invocation: u64, record: &Value) {
    // A closed output (the caller went away) is not an error the command can act on.
    let _ = HostCli::write_line(api, invocation, record.to_string());
}

/// The `error` record of a command that could not start.
pub fn error_record(command: &str, code: &str, message: &str) -> Value {
    json!({"type": "error", "command": command, "code": code, "message": message})
}

/// The `warning` record of a running session that could not be read.
pub fn unreachable_warning(command: &str, session: &str, message: &str) -> Value {
    json!({
        "type": "warning",
        "command": command,
        "code": "session-unreachable",
        "session": session,
        "message": message,
        "hint": "a server started before this command existed may not answer; its panes were not included",
    })
}
