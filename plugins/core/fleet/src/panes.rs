//! `ilium panes`: lists the panes of every running session on this machine that pass the shared
//! pane selection (see `pane_filter`). It shows exactly which panes another selection-based
//! command, such as `ilium broadcast`, would act on.
//!
//! Output is JSONL: one `pane` record per selected pane, a `warning` per session that could not
//! be read, then one `summary`. The cost of the session's panes (`ilium.core.cost/usage`) is
//! added to each record of the command's own session when the service answers.
//!
//! Exit status: 0, 1 when no session is running, 2 for a selection that cannot be parsed.

use std::collections::HashMap;
use std::path::Path;

use ilium_plugin_sdk::host::Level;
use ilium_plugin_sdk::host::{HostCli, HostServices};
use ilium_plugin_sdk::log::log;
use ilium_plugin_sdk::types::{Outcome, RequestId, RequestRef, RequestResult};
use ilium_plugin_sdk::HostApi;
use serde_json::{json, Value};

use crate::jobs::{emit, error_record, unreachable_warning, Routes, Step};
use crate::pane_filter::{
    parse_arguments, OptionSpec, PaneFilter, PaneFilterArgs, CWD_OPTION, FILTER_OPTIONS,
};
use crate::scan::{live_sessions, Scan, SessionPanes};

const COMMAND: &str = "panes";
/// The service that prices a session's panes, and its method.
const COST_SERVICE: &str = "ilium.core.cost/usage";
const COST_METHOD: &str = "usage";

/// The options of `ilium panes`.
pub fn option_specs() -> Vec<OptionSpec> {
    let mut specs = FILTER_OPTIONS.to_vec();
    specs.push(CWD_OPTION);
    specs
}

/// One running `ilium panes`.
#[derive(Debug)]
pub struct PanesJob {
    invocation: u64,
    filter: PaneFilter,
    scan: Scan,
    /// The cost request, until it answered.
    cost_request: Option<u64>,
    /// Cost per pane id of the command's own session.
    costs: HashMap<u64, f64>,
}

impl PanesJob {
    /// Parses the command line and asks every session; `None` when the command is already over.
    pub fn start(
        api: &mut dyn HostApi,
        invocation: u64,
        argv: &[String],
        cwd: &str,
        routes: &mut Routes,
    ) -> Option<Self> {
        let filter = match compile(argv, cwd) {
            Ok(filter) => filter,
            Err(message) => {
                emit(
                    api,
                    invocation,
                    &error_record(COMMAND, "invalid-selection", &message),
                );
                HostCli::finish(api, invocation, 2);
                return None;
            }
        };
        let caller = HostCli::caller_pane(api, invocation);
        let sessions = live_sessions(api);
        let scan = Scan::start(api, sessions, caller, false, true);
        for request in scan.pending_requests() {
            routes.call(request, invocation);
        }
        let mut job = Self {
            invocation,
            filter,
            scan,
            cost_request: None,
            costs: HashMap::new(),
        };
        if !job.scan.sessions.is_empty() {
            match HostServices::call(
                api,
                COST_SERVICE.to_owned(),
                COST_METHOD.to_owned(),
                json!({"window": "session"}).to_string(),
            ) {
                Ok(RequestId(request)) => {
                    job.cost_request = Some(request);
                    routes.call(request, invocation);
                }
                Err(error) => log(
                    Level::Warn,
                    &format!("cost service failed: {}", error.message),
                ),
            }
        }
        job.finish_if_ready(api).then_some(job)
    }

    /// Takes in one answer.
    pub fn on_completed(
        &mut self,
        api: &mut dyn HostApi,
        result: &RequestResult,
        _routes: &mut Routes,
    ) -> Step {
        if let RequestRef::Call(RequestId(request)) = &result.request {
            if self.cost_request == Some(*request) {
                self.cost_request = None;
                self.take_costs(api, *request, &result.outcome);
            } else {
                self.scan.on_completed(api, *request, &result.outcome);
            }
        }
        if self.finish_if_ready(api) {
            Step::Pending
        } else {
            Step::Done
        }
    }

    fn take_costs(&mut self, api: &mut dyn HostApi, request: u64, outcome: &Outcome) {
        match outcome {
            Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_) => {
                let reply = HostServices::take_reply(api, RequestId(request));
                self.costs = reply
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .map(|value| cost_table(&value))
                    .unwrap_or_default();
            }
            Outcome::Failed(error) | Outcome::Refused(error) => log(
                Level::Warn,
                &format!("cost service failed: {}", error.message),
            ),
            Outcome::Duplicate(_) | Outcome::Cancelled => {}
        }
    }

    /// Prints the listing once every answer is in. Returns true while the job still waits.
    fn finish_if_ready(&mut self, api: &mut dyn HostApi) -> bool {
        if !self.scan.is_complete() || self.cost_request.is_some() {
            return true;
        }
        let mut selected = 0_usize;
        let mut unreachable = 0_usize;
        for (session, panes) in self.scan.sessions.iter().zip(&self.scan.panes) {
            match panes {
                Some(SessionPanes::Panes(panes)) => {
                    for pane in panes.iter().filter(|pane| self.filter.accepts(pane)) {
                        selected += 1;
                        let mut record = pane.to_json();
                        record["type"] = json!("pane");
                        if session.is_self {
                            if let Some(cost) = self.costs.get(&pane.pane_id) {
                                record["cost"] = json!(cost);
                            }
                        }
                        emit(api, self.invocation, &record);
                    }
                }
                Some(SessionPanes::Unreachable(message)) => {
                    unreachable += 1;
                    emit(
                        api,
                        self.invocation,
                        &unreachable_warning(COMMAND, &session.name, message),
                    );
                }
                None => {}
            }
        }
        let sessions = self.scan.sessions.len();
        emit(
            api,
            self.invocation,
            &json!({
                "type": "summary",
                "command": COMMAND,
                "ok": sessions > 0 && unreachable == 0,
                "sessions": sessions,
                "unreachable_sessions": unreachable,
                "panes": selected,
            }),
        );
        HostCli::finish(api, self.invocation, u8::from(sessions == 0));
        false
    }
}

/// Parses the selection of `ilium panes`. `Err` holds the message of an invalid selection.
fn compile(argv: &[String], cwd: &str) -> Result<PaneFilter, String> {
    let parsed = parse_arguments(argv, &option_specs())?;
    if let Some(word) = parsed.words.first() {
        return Err(format!("unexpected argument '{word}'"));
    }
    let base = parsed.last("cwd").unwrap_or(cwd);
    PaneFilterArgs::from_parsed(&parsed).compile(Path::new(base), None)
}

/// The `panes` table of a cost reply: `{"panes": {"<pane id>": <cost>}}`.
fn cost_table(reply: &Value) -> HashMap<u64, f64> {
    reply
        .get("panes")
        .and_then(Value::as_object)
        .map(|panes| {
            panes
                .iter()
                .filter_map(|(id, cost)| Some((id.parse::<u64>().ok()?, cost.as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn a_cost_reply_becomes_a_table_by_pane_id() {
        let table = cost_table(&json!({"panes": {"3": 1.42, "x": 2.0, "5": "free"}}));
        assert_eq!(table.len(), 1);
        assert_eq!(table[&3], 1.42);
        assert!(cost_table(&json!({})).is_empty());
    }

    #[test]
    fn a_positional_word_is_refused() {
        assert!(compile(&words(&["hello"]), "/").is_err());
    }

    #[test]
    fn the_selection_options_compile() {
        assert!(compile(&words(&["--agent", "claude", "--state", "idle"]), "/").is_ok());
        assert!(compile(&words(&["--state", "sleeping"]), "/").is_err());
    }
}
