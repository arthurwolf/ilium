//! AI triggers (F-044): which events start titling, from the Triggers tab.

use ilium_plugin_sdk::types::{AgentActivity, AgentActivityEvent, Event, NodeId};
use serde_json::json;

use crate::common::*;

const PROMPT_EVENT: &str = "Agent receives a prompt";
const FINISH_EVENT: &str = "Agent finishes work";

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

/// REQ: F-044
#[test]
fn f_044_with_default_triggers_a_prompt_starts_titling() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: F-044
#[test]
fn f_044_clearing_the_prompt_trigger_stops_titling_on_a_prompt() {
    let mut host = activated_server();
    host.settings().set(PROMPT_EVENT, json!([])).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(host.inference_requests().is_empty());
}

/// REQ: F-044
#[test]
fn f_044_setting_the_prompt_trigger_back_restores_titling() {
    let mut host = activated_server();
    host.settings().set(PROMPT_EVENT, json!([])).expect("setting accepted");
    host.settings().set(PROMPT_EVENT, json!(["Retitle element"])).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: F-044
#[test]
fn f_044_the_change_applies_to_the_next_event_without_a_restart() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "first"));
    host.settle();
    host.settings().set(PROMPT_EVENT, json!([])).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "second"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
    assert!(host.restarts().is_empty());
}

/// REQ: F-044
#[test]
fn f_044_agent_starts_working_retitles_by_default() {
    let mut host = activated_server();
    host.deliver_one(started_working(3));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: F-044
#[test]
fn f_044_agent_needs_approval_does_nothing_by_default() {
    let mut host = activated_server();
    host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(3),
        previous: AgentActivity::Working,
        current: AgentActivity::AwaitingApproval,
        revision: 1,
    }));
    host.settle();
    assert!(host.inference_requests().is_empty());
}

/// REQ: F-044
#[test]
fn f_044_agent_finishes_work_retitles_and_restructures_the_project() {
    let mut host = activated_server();
    host.deliver_one(finished(3));
    host.settle();
    let purposes: Vec<String> = host.inference_requests().into_iter().map(|(_, request)| request.purpose).collect();
    assert!(purposes.iter().any(|purpose| purpose == "pane-title"), "{purposes:?}");
    assert!(purposes.iter().any(|purpose| purpose.contains("restructure")), "{purposes:?}");
}

/// REQ: F-044
#[test]
fn f_044_agent_finishes_work_with_the_trigger_cleared_makes_no_request() {
    let mut host = activated_server();
    host.settings().set(FINISH_EVENT, json!([])).expect("setting accepted");
    host.settle();
    host.deliver_one(finished(3));
    host.settle();
    assert!(host.inference_requests().is_empty());
}

/// REQ: F-044
#[test]
fn f_044_finishing_work_never_starts_both_restructure_scopes() {
    let mut host = activated_server();
    host.deliver_one(finished(3));
    host.settle();
    let restructure_requests = host
        .inference_requests()
        .into_iter()
        .filter(|(_, request)| request.purpose.contains("restructure"))
        .count();
    assert_eq!(restructure_requests, 1);
}

/// REQ: F-044
#[test]
fn f_044_a_trigger_set_to_retitle_only_does_not_restructure() {
    let mut host = activated_server();
    host.settings().set(FINISH_EVENT, json!(["Retitle element"])).expect("setting accepted");
    host.settle();
    host.deliver_one(finished(3));
    host.settle();
    assert!(host.inference_requests().iter().all(|(_, request)| !request.purpose.contains("restructure")));
}

/// REQ: F-044
#[test]
fn f_044_an_unknown_action_name_is_rejected_by_the_setting() {
    let mut host = activated_server();
    let result = host.settings().set(PROMPT_EVENT, json!(["Launch rockets"]));
    assert!(result.is_err());
}

/// REQ: F-044
#[test]
fn f_044_trigger_values_persist_under_the_triggers_table_keys() {
    let mut host = activated_server();
    host.settings().set(PROMPT_EVENT, json!([])).expect("setting accepted");
    assert_eq!(host.settings().get(PROMPT_EVENT), Some(json!([])));
}

/// REQ: F-044
#[test]
fn f_044_page_says_toggles_decide_which_events_start_titling() {
    assert!(page_text().contains("**AI triggers.** Toggles decide which events start titling (the Triggers tab of Settings)."));
}

/// REQ: F-044
#[test]
fn f_044_startup_is_global_and_agent_events_act_on_the_pane_and_its_project() {
    let doc = user_doc("settings.md");
    assert!(doc.contains("Startup is global; the other agent events act on the originating pane and its project."));
}

/// REQ: F-044
#[test]
fn f_044_agent_session_ready_prompt_working_waiting_finish_and_checkpoint_are_the_default_refresh_events() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains(
        "By default titles are refreshed when an agent session becomes ready, when a prompt is received, when the agent starts working, when it waits on background work, when it finishes, and at every second submitted plain-shell command in a terminal."
    ));
}

/// REQ: F-044
#[test]
fn f_044_triggers_tab_maps_eight_events_to_three_actions() {
    let doc = user_doc("settings.md");
    assert!(doc.contains("Maps eight events to AI actions (Retitle element, Restructure project, Restructure all projects)."));
}
