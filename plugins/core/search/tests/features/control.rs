//! The control plane drives the panel through the open command's arguments: a query, moving
//! the selection, opening a result and closing the panel (the voice agent and `ilium ctl`).

use ilium_plugin_core_search::client::{parse_control, with_state, Control};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, EditorPlacement, Event, Intent,
};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

fn hits_json() -> Value {
    json!([
        { "id": "7", "kind": "agent", "title": "claude: deploy script fix" },
        { "id": "8", "kind": "terminal", "title": "build shell" },
        { "id": "docs/deploy.md", "kind": "file", "title": "docs/deploy.md", "line": 3 },
    ])
}

/// The open command with control arguments, the way the kernel forwards a control request.
fn control_event(arguments: Value) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: OPEN_COMMAND.to_string(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Voice,
        context: CommandContext::default(),
    })
}

fn selected() -> usize {
    with_state(|state| state.selected)
}

/// REQ: F-009
#[test]
fn search_009_parse_control_reads_every_action_and_treats_no_action_as_a_plain_open() {
    assert_eq!(parse_control("").unwrap(), None);
    assert_eq!(parse_control("{}").unwrap(), None);
    assert_eq!(
        parse_control(r#"{"action":"query","query":"deploy","index":2}"#).unwrap(),
        Some(Control::Query {
            text: "deploy".to_string(),
            index: Some(2)
        })
    );
    assert_eq!(
        parse_control(r#"{"action":"select_next"}"#).unwrap(),
        Some(Control::SelectNext)
    );
    assert_eq!(
        parse_control(r#"{"action":"select_previous"}"#).unwrap(),
        Some(Control::SelectPrevious)
    );
    assert_eq!(
        parse_control(r#"{"action":"open_result"}"#).unwrap(),
        Some(Control::OpenResult { index: None })
    );
    assert_eq!(
        parse_control(r#"{"action":"close"}"#).unwrap(),
        Some(Control::Close)
    );
}

/// REQ: F-009
#[test]
fn search_009_parse_control_refuses_an_unknown_action_a_blank_query_and_broken_json() {
    assert!(parse_control(r#"{"action":"explode"}"#).is_err());
    assert!(parse_control(r#"{"action":"query"}"#).is_err());
    assert!(parse_control(r#"{"action":"query","query":"   "}"#).is_err());
    assert!(parse_control("not json").is_err());
}

/// REQ: F-009
#[test]
fn search_009_a_control_query_opens_the_panel_and_sends_one_query() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy" }),
    )]);
    assert_eq!(open_views(&submissions).len(), 1);
    let queries = query_messages(&submissions);
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0]["query"], json!("deploy"));
    assert_eq!(queries[0]["revision"], json!(1));
    assert_eq!(with_state(|state| state.draft.clone()), "deploy");
}

/// REQ: F-009
#[test]
fn search_009_a_control_query_on_an_open_panel_does_not_open_it_again() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![control_event(
        json!({ "action": "query", "query": "build" }),
    )]);
    assert!(open_views(&submissions).is_empty());
    assert_eq!(query_messages(&submissions).len(), 1);
}

/// REQ: F-009
#[test]
fn search_009_a_control_query_with_an_index_selects_that_result_when_the_answer_arrives() {
    let mut host = activated_client();
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy", "index": 1 }),
    )]);
    assert_eq!(selected(), 0, "the answer has not arrived yet");
    host.deliver(vec![results_event(1, hits_json())]);
    assert_eq!(selected(), 1);
}

/// REQ: F-009
#[test]
fn search_009_a_control_index_past_the_results_leaves_the_selection_alone() {
    let mut host = activated_client();
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy", "index": 9 }),
    )]);
    host.deliver(vec![results_event(1, hits_json())]);
    assert_eq!(selected(), 0);
}

/// REQ: F-009
#[test]
fn search_009_select_next_and_previous_move_the_selection_and_stop_at_both_ends() {
    let mut host = client_showing("deploy", hits_json());
    for expected in [1, 2, 2] {
        host.deliver(vec![control_event(json!({ "action": "select_next" }))]);
        assert_eq!(selected(), expected);
    }
    for expected in [1, 0, 0] {
        host.deliver(vec![control_event(json!({ "action": "select_previous" }))]);
        assert_eq!(selected(), expected);
    }
}

/// REQ: F-009
#[test]
fn search_009_select_next_waits_for_the_answer_of_a_query_in_flight() {
    let mut host = activated_client();
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy" }),
    )]);
    host.deliver(vec![control_event(json!({ "action": "select_next" }))]);
    assert_eq!(selected(), 0);
    host.deliver(vec![results_event(1, hits_json())]);
    assert_eq!(selected(), 1);
}

/// REQ: F-009
#[test]
fn search_009_a_newer_query_drops_the_requests_that_waited_for_the_older_one() {
    let mut host = activated_client();
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy", "index": 2 }),
    )]);
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "build" }),
    )]);
    host.deliver(vec![results_event(2, hits_json())]);
    assert_eq!(selected(), 0);
}

/// REQ: F-009
#[test]
fn search_009_open_result_without_an_index_opens_the_selected_result_and_closes_the_panel() {
    let mut host = client_showing("deploy", hits_json());
    host.deliver(vec![control_event(json!({ "action": "select_next" }))]);
    let submissions = host.deliver(vec![control_event(json!({ "action": "open_result" }))]);
    assert_eq!(close_views(&submissions).len(), 1);
    let focused: Vec<_> = submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(spec) => Some(spec.arguments_json.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(focused, vec![json!({ "node": 8 }).to_string()]);
    assert!(!with_state(|state| state.open));
}

/// REQ: F-009
#[test]
fn search_009_open_result_with_an_index_opens_that_file_in_the_editor() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![control_event(
        json!({ "action": "open_result", "index": 2 }),
    )]);
    let opened: Vec<_> = submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::EditorOpen(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].path.path, "docs/deploy.md");
    assert_eq!(opened[0].line, Some(3));
    assert_eq!(opened[0].placement, EditorPlacement::NewPane);
}

/// REQ: F-009
#[test]
fn search_009_open_result_with_an_index_past_the_results_does_nothing() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![control_event(
        json!({ "action": "open_result", "index": 3 }),
    )]);
    assert!(close_views(&submissions).is_empty());
    assert!(with_state(|state| state.open));
}

/// REQ: F-009
#[test]
fn search_009_open_result_waits_for_the_answer_of_a_query_in_flight() {
    let mut host = activated_client();
    host.deliver(vec![control_event(
        json!({ "action": "query", "query": "deploy" }),
    )]);
    let waiting = host.deliver(vec![control_event(json!({ "action": "open_result" }))]);
    assert!(close_views(&waiting).is_empty());
    let answered = host.deliver(vec![results_event(1, hits_json())]);
    assert_eq!(close_views(&answered).len(), 1);
}

/// REQ: F-009
#[test]
fn search_009_close_closes_an_open_panel_and_is_ignored_when_none_is_open() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![control_event(json!({ "action": "close" }))]);
    assert_eq!(close_views(&submissions).len(), 1);
    assert!(!with_state(|state| state.open));
    let again = host.deliver(vec![control_event(json!({ "action": "close" }))]);
    assert!(close_views(&again).is_empty());
}

/// REQ: F-009
#[test]
fn search_009_navigation_without_an_open_panel_does_nothing() {
    let mut host: FakeHost = activated_client();
    for action in ["select_next", "select_previous", "open_result"] {
        let submissions = host.deliver(vec![control_event(json!({ "action": action }))]);
        assert!(submissions.is_empty(), "{action}");
    }
}

/// REQ: F-009
#[test]
fn search_009_an_unknown_control_action_is_dropped() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![control_event(json!({ "action": "explode" }))]);
    assert!(submissions.is_empty());
}
