//! Shared helpers of the titles port twins (self-contained copy of the feature helpers).
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_titles::TitlesServer;
use ilium_plugin_sdk::host::inference::{CompletionRequest, Completion};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, CommandContext, CommandInvokedEvent, CommandOrigin, ErrorCode, Event,
    InputSource, InputSubmittedEvent, Intent, NodeClass, NodeId, Outcome, PluginError, RenameNodeSpec, RequestId,
    RequestRef, RequestResult, RestructureOp, RestructureSpec, SetInferredIconSpec, Submission,
};
use serde_json::json;

pub const PROMPT_EVENT: &str = "Agent receives a prompt";
pub const MAX_ATTEMPTS: usize = 3;

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn activated_server() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server");
    host.activate::<TitlesServer>();
    host
}

pub fn prompt_event(node: u64, text: &str) -> Event {
    Event::InputSubmitted(InputSubmittedEvent { node: NodeId(node), text: text.to_string(), source: InputSource::User, epoch: 1 })
}

pub fn command_event(name: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: name.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext::default(),
    })
}

pub fn finished(node: u64) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous: AgentActivity::Working,
        current: AgentActivity::Done,
        revision: 1,
    })
}

pub fn started_working(node: u64) -> Event {
    Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(node),
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 1,
    })
}

pub fn completion(text: &str) -> Completion {
    Completion { text: text.to_string(), model: "test-model".to_string(), input_tokens: 10, output_tokens: 4 }
}

pub fn plugin_error(code: ErrorCode) -> PluginError {
    PluginError { code, message: "test".to_string(), retry_after_ms: None }
}

pub fn title_for_prompt(host: &mut FakeHost, node: u64, prompt: &str, reply: &str) {
    host.deliver_one(prompt_event(node, prompt));
    host.settle();
    let requests = host.inference_requests();
    let (id, _) = requests.last().cloned().expect("an inference request was made");
    host.complete_inference(id, completion(reply));
    host.settle();
}

pub fn renames(submissions: &[Submission]) -> Vec<RenameNodeSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::RenameNode(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn icons(submissions: &[Submission]) -> Vec<SetInferredIconSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetInferredIcon(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn restructures(submissions: &[Submission]) -> Vec<RestructureSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Restructure(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn reverts(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::RevertRestructure(token) => Some(*token),
            _ => None,
        })
        .collect()
}

pub fn outcome_event(correlation: u64, outcome: Outcome) -> Event {
    Event::RequestCompleted(RequestResult { request: RequestRef::Submission(correlation), outcome })
}

pub fn pane(name: &str, parent: Option<u64>) -> TreeNode {
    TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    }
}

pub fn editor_pane(name: &str) -> TreeNode {
    TreeNode { content_kind: Some("ilium.editor".to_string()), ..pane(name, None) }
}

pub fn host_with_two_panes() -> FakeHost {
    let mut host = activated_server();
    host.tree().add(pane("shell-1", None));
    host.tree().add(pane("shell-2", None));
    host.settle();
    host
}

pub fn restructure_requests(host: &FakeHost) -> Vec<(RequestId, CompletionRequest)> {
    host.inference_requests().into_iter().filter(|(_, request)| request.purpose.contains("restructure")).collect()
}

fn plan(children: serde_json::Value) -> String {
    json!({"children": children, "animations": {"definitions": [], "project": "none"}}).to_string()
}

pub fn plan_reply() -> String {
    plan(json!([{"kind": "group", "title": "Billing API", "short_title": "Billing", "icon": null, "children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ]}]))
}

pub fn plan_without_pane_two() -> String {
    plan(json!([{"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null}]))
}

pub fn plan_with_duplicate_pane() -> String {
    plan(json!([
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 1, "title": "claude again", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ]))
}

pub fn plan_with_empty_title() -> String {
    plan(json!([
        {"kind": "pane", "id": 1, "title": "", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ]))
}

/// Starts a restructure with two panes and answers the first request with `reply`.
pub fn organised_host(reply: &str) -> FakeHost {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion(reply));
    host.settle();
    host
}

/// Answers every attempt (at most `MAX_ATTEMPTS`) with `reply`.
pub fn rejected_host(reply: &str) -> FakeHost {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    for _ in 0..MAX_ATTEMPTS {
        let Some((id, _)) = restructure_requests(&host).pop() else { break };
        host.complete_inference(id, completion(reply));
        host.settle();
    }
    host
}

pub fn restructure_correlation(host: &FakeHost) -> u64 {
    host.submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::Restructure(_)))
        .expect("restructure submitted")
        .correlation
}

/// Titles the restructure renames.
pub fn applied_titles(host: &FakeHost) -> Vec<String> {
    let mut titles = Vec::new();
    for spec in restructures(&host.submitted()) {
        for operation in &spec.operations {
            if let RestructureOp::Rename(rename) = operation {
                titles.push(rename.title.clone());
            }
        }
    }
    titles
}
