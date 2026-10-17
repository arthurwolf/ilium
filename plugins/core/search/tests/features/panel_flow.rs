//! The client side: the open command, the query field, the result list and closing the panel.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, Event, Intent};
use serde_json::json;

use crate::common::*;

fn hits_json() -> serde_json::Value {
    json!([
        { "id": "7", "kind": "agent", "title": "claude: deploy script fix" },
        { "id": "8", "kind": "terminal", "title": "build shell" },
        { "id": "docs/deploy.md", "kind": "file", "title": "docs/deploy.md" },
    ])
}

/// REQ: F-009
#[test]
fn search_009_open_command_submits_one_open_view_for_the_search_panel_with_focus() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![open_command_event()]);
    let opened = open_views(&submissions);
    assert_eq!(opened.len(), 1);
    match &opened[0].intent {
        Intent::OpenView(spec) => {
            assert_eq!(spec.view, panel());
            assert!(spec.focus);
        }
        other => panic!("{other:?}"),
    }
}

/// REQ: F-009
#[test]
fn search_009_open_command_submission_has_correlation_1_and_no_dedup_key() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![open_command_event()]);
    let opened = open_views(&submissions);
    assert_eq!(opened[0].correlation, 1);
    assert_eq!(opened[0].dedup_key, None);
}

/// REQ: F-009
#[test]
fn search_009_other_commands_do_not_open_the_panel() {
    let mut host = activated_client();
    let mut event = open_command_event();
    if let Event::CommandInvoked(invoked) = &mut event {
        invoked.command = "ilium.core.search/other".to_string();
    }
    let submissions = host.deliver(vec![event]);
    assert!(open_views(&submissions).is_empty());
}

/// REQ: F-009
#[test]
fn search_009_open_command_twice_submits_two_open_views() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![open_command_event(), open_command_event()]);
    assert_eq!(open_views(&submissions).len(), 2);
}

/// REQ: F-009
#[test]
fn search_009_activating_a_hit_in_the_list_closes_the_panel_with_correlation_2() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![action(
        "hits",
        ActionKind::Activated,
        ActionValue::Index(0),
    )]);
    let closed = close_views(&submissions);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].correlation, 2);
}

/// REQ: F-009
#[test]
fn search_009_activating_a_result_row_by_id_closes_the_panel() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![action(
        "results",
        ActionKind::Activated,
        ActionValue::Text("8".to_string()),
    )]);
    assert_eq!(close_views(&submissions).len(), 1);
}

/// REQ: F-009
#[test]
fn search_009_selecting_a_result_row_previews_without_closing_the_panel() {
    let mut host = client_showing("deploy", hits_json());
    let submissions = host.deliver(vec![action(
        "results",
        ActionKind::Selected,
        ActionValue::Text("8".to_string()),
    )]);
    assert!(close_views(&submissions).is_empty());
}

/// REQ: F-009
#[test]
fn search_009_typing_in_the_query_field_runs_no_query() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let submissions = host.deliver(vec![
        text_changed("dep"),
        text_changed("depl"),
        text_changed("deploy"),
    ]);
    assert!(query_messages(&submissions).is_empty());
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::search_state_waits_for_a_full_second_of_quiet_before_claiming_work
#[test]
fn port_search_ui_state_waits_for_quiet_before_a_query_and_sends_one_after_filter_changed() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let typing = host.deliver(vec![text_changed("needle")]);
    assert!(query_messages(&typing).is_empty());
    let settled = host.deliver(vec![filter_changed("needle")]);
    let messages = query_messages(&settled);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["query"], "needle");
    assert_eq!(messages[0]["revision"], 1);
}

/// REQ: F-009, PORT:ilium-client/src/app.rs::workspace_search_debounce_wakes_at_its_exact_deadline
#[test]
fn port_app_debounce_is_150_ms_by_the_kernel_and_the_plugin_sets_no_timer() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![text_changed("needle")]);
    host.advance_clock(149);
    assert!(
        query_messages(&host.submitted()).is_empty(),
        "the plugin never queries on its own timer"
    );
    assert!(page_text().contains("debounced to 150 ms by the kernel"));
    let after = host.deliver(vec![filter_changed("needle")]);
    assert_eq!(query_messages(&after).len(), 1);
}

/// REQ: F-009
#[test]
fn search_009_filter_changed_with_the_same_text_runs_the_query_once() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let first = host.deliver(vec![filter_changed("deploy")]);
    let second = host.deliver(vec![filter_changed("deploy")]);
    assert_eq!(query_messages(&first).len(), 1);
    assert!(query_messages(&second).is_empty());
}

/// REQ: F-009
#[test]
fn search_009_filter_changed_with_different_text_runs_a_new_query_with_the_next_revision() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![filter_changed("deploy")]);
    let second = host.deliver(vec![filter_changed("deploy s")]);
    let messages = query_messages(&second);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["revision"], 2);
    assert_eq!(messages[0]["query"], "deploy s");
}

/// REQ: F-009
#[test]
fn search_009_filter_changed_back_to_an_earlier_text_runs_the_query_again() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![filter_changed("a")]);
    host.deliver(vec![filter_changed("ab")]);
    let third = host.deliver(vec![filter_changed("a")]);
    assert_eq!(query_messages(&third).len(), 1);
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::stale_worker_completion_cannot_replace_a_newer_query
#[test]
fn port_search_ui_stale_answer_cannot_replace_a_newer_query() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![filter_changed("first")]);
    host.deliver(vec![filter_changed("second")]);
    host.deliver(vec![results_event(
        1,
        json!([{ "id": "1", "kind": "terminal", "title": "stale shell" }]),
    )]);
    host.settle();
    let screen = host.render(panel(), 80, 20).text();
    assert!(!screen.contains("stale shell"), "{screen}");
    host.deliver(vec![results_event(
        2,
        json!([{ "id": "2", "kind": "terminal", "title": "fresh shell" }]),
    )]);
    host.settle();
    let screen = host.render(panel(), 80, 20).text();
    assert!(screen.contains("fresh shell"), "{screen}");
}

/// REQ: F-009, PORT:ilium-client/src/app.rs::workspace_search_waits_then_applies_a_worker_result_for_the_current_query
#[test]
fn port_app_waits_then_applies_the_answer_for_the_current_query() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    let before = host.render(panel(), 80, 20).text();
    assert!(!before.contains("build shell"));
    host.deliver(vec![filter_changed("workspace_debounce_needle")]);
    host.deliver(vec![results_event(
        1,
        json!([{ "id": "8", "kind": "terminal", "title": "build shell" }]),
    )]);
    host.settle();
    let after = host.render(panel(), 80, 20).text();
    assert!(after.contains("build shell"), "{after}");
    assert_eq!(after.matches("build shell").count(), 1);
}

/// REQ: F-009
#[test]
fn search_009_an_answer_for_a_revision_never_asked_is_ignored() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![results_event(
        99,
        json!([{ "id": "1", "kind": "terminal", "title": "ghost shell" }]),
    )]);
    host.settle();
    assert!(!host.render(panel(), 80, 20).text().contains("ghost shell"));
}

/// REQ: F-009
#[test]
fn search_009_a_new_query_with_no_hits_clears_the_earlier_rows() {
    let mut host = client_showing("deploy", hits_json());
    host.deliver(vec![filter_changed("zzz")]);
    host.deliver(vec![results_event(2, json!([]))]);
    host.settle();
    let screen = host.render(panel(), 80, 20).text();
    assert!(!screen.contains("build shell"), "{screen}");
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::cancelled_terminal_preparation_does_not_publish_partial_matches
#[test]
fn port_search_ui_cancelled_scan_job_publishes_no_partial_matches() {
    use ilium_plugin_sdk::types::{ErrorCode, PluginError};
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "x");
    let _ = ask(&mut host, 1, "needle");
    for record in host.jobs().list() {
        host.jobs().finish(
            record.request,
            Err(PluginError {
                code: ErrorCode::Cancelled,
                message: "cancelled".to_string(),
                retry_after_ms: None,
            }),
        );
    }
    host.settle();
    let hits: Vec<_> = result_messages(&host.submitted())
        .into_iter()
        .flat_map(|(_, payload)| payload["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    assert!(hits.is_empty(), "{hits:?}");
}

/// REQ: F-009, PORT:ilium-client/src/app.rs::workspace_search_maintenance_preserves_an_active_settings_view
#[test]
fn port_app_an_answer_arriving_while_the_panel_is_closed_opens_no_view() {
    let mut host = activated_client();
    host.deliver(vec![open_command_event()]);
    host.deliver(vec![filter_changed("needle")]);
    host.deliver(vec![action("hits", ActionKind::Closed, ActionValue::None)]);
    let submissions = host.deliver(vec![results_event(1, json!([]))]);
    assert!(open_views(&submissions).is_empty());
    assert!(close_views(&submissions).is_empty());
}

/// REQ: F-009, PORT:ilium-client/src/app.rs::every_tree_context_menu_exposes_workspace_search
#[test]
fn port_app_the_open_command_is_titled_for_the_tree_context_menu_and_palette() {
    let manifest = manifest_value();
    let command = &manifest["contributes"]["commands"][0];
    assert_eq!(command["title"], "Search the workspace");
    assert_eq!(command["category"], "Search");
    let user_doc = std::fs::read_to_string(workspace_root().join("src/docs/panes-and-layout.md"))
        .expect("user doc");
    assert!(user_doc.contains("right-click and choose **Search workspace**"));
}
