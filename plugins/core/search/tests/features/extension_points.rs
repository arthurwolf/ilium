//! Extension points (`sources`, `rank`), the `query` service and the contribution example.

use ilium_plugin_sdk::exports::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::types::{HookReply, PluginId};
use serde_json::{json, Value};

use crate::common::*;

fn points_page() -> String {
    page("reference-core-extension-points")
}

// ---- documentation rows -------------------------------------------------------------------

/// REQ: F-009
#[test]
fn doc_f_009_page_lists_sources_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "`sources`");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a source of results, such as issues or notes.");
    assert_eq!(row[4], "standard");
}

/// REQ: F-009
#[test]
fn doc_f_009_sources_payload_has_the_four_entry_fields_in_order() {
    let row = table_row(&page_text(), "`sources`");
    assert_eq!(row[3], "entry: id, title, kind, service (answers a query with ranked hits)");
}

/// REQ: F-009
#[test]
fn doc_f_009_page_lists_rank_as_a_standard_transform_hook() {
    let row = table_row(&page_text(), "`rank`");
    assert_eq!(row[1], "hook (transform)");
    assert_eq!(row[2], "Change the order of results before they are shown.");
    assert_eq!(row[4], "standard");
}

/// REQ: F-009
#[test]
fn doc_f_009_rank_payload_carries_query_and_hits_and_replies_with_hits() {
    let row = table_row(&page_text(), "`rank`");
    assert_eq!(row[3], "query, hits (id, kind, score); reply: hits");
}

/// REQ: F-009
#[test]
fn doc_f_009_the_plugin_owns_exactly_two_points() {
    let page = page_text();
    let rows = page.lines().filter(|line| line.starts_with("| `sources`") || line.starts_with("| `rank`")).count();
    assert_eq!(rows, 2);
    let row = table_row(&points_page(), "[Workspace search](/pages/core-plugin-search/)");
    assert_eq!(&row[1..], ["`ilium.core.search`", "2", "1", "0", "yes"]);
}

/// REQ: F-009
#[test]
fn doc_f_009_core_extension_points_page_agrees_on_both_points() {
    let page = points_page();
    assert!(page.contains(
        "| `ilium.core.search/sources` | collect | standard | Add a source of results, such as issues or notes. |"
    ));
    assert!(page.contains(
        "| `ilium.core.search/rank` | hook: transform | standard | Change the order of results before they are shown. |"
    ));
}

/// REQ: F-009
#[test]
fn doc_f_009_page_states_the_query_service_and_core_page_agrees() {
    assert!(page_text().contains("- `ilium.core.search/query`: query: search all sources"));
    assert!(points_page().contains("| `ilium.core.search/query` | query: search all sources |"));
}

/// REQ: F-009
#[test]
fn doc_f_009_the_plugin_is_replaceable_and_names_the_replaces_key() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("`\"replaces\": [\"ilium.core.search\"]`"));
}

/// REQ: F-009
#[test]
fn doc_f_009_page_names_the_provide_and_contribute_grant_shapes() {
    let page = page_text();
    assert!(page.contains("`extensions.provide:ilium.core.search/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.search/<point>`"));
    assert!(page.contains("`services.use:<name>`"));
}

/// REQ: F-009
#[test]
fn doc_f_009_the_issues_example_contributes_to_sources_with_priority_50() {
    let blocks = json_blocks(&page_text());
    let example: Value = serde_json::from_str(&blocks[2]).expect("example");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["id"], "issues");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"], json!({ "title": "Issues", "kind": "issue", "service": "example.issues/search" }));
}

// ---- behaviour: the query service ---------------------------------------------------------

fn call_query(payload: &str) -> ServiceReply {
    use ilium_plugin_sdk::ServiceProvider;
    let mut server = ilium_plugin_core_search::SearchServer::default();
    server.handle(ServiceCall {
        call: 1,
        service: QUERY_SERVICE.to_string(),
        method: "query".to_string(),
        payload_json: payload.to_string(),
        caller: PluginId("example.caller".to_string()),
    })
}

/// REQ: F-009
#[test]
fn search_009_query_service_answers_with_done_json_containing_hits() {
    match call_query(r#"{"query":"deploy"}"#) {
        ServiceReply::Done(text) => {
            let value: Value = serde_json::from_str(&text).expect("reply JSON");
            assert!(value["hits"].is_array(), "{value}");
        }
        ServiceReply::Deferred => {}
    }
}

/// REQ: F-009
#[test]
fn search_009_query_service_with_an_empty_query_answers_no_hits() {
    if let ServiceReply::Done(text) = call_query(r#"{"query":""}"#) {
        let value: Value = serde_json::from_str(&text).expect("reply JSON");
        assert_eq!(value["hits"], json!([]));
    }
}

// ---- behaviour: the rank hook and the sources point (gap helpers) --------------------------

/// REQ: F-009
#[test]
fn search_009_rank_hook_receives_query_and_hits_with_id_kind_score() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "the build failed with linker error");
    host.extensions().on_run_hook("ilium.core.search/rank", HookReply::Pass);
    ask(&mut host, 1, "linker");
    let calls = host.extensions().hook_calls("ilium.core.search/rank");
    assert_eq!(calls.len(), 1);
    let payload: Value = serde_json::from_str(&calls[0]).expect("payload JSON");
    assert_eq!(payload["query"], "linker");
    let first = &payload["hits"][0];
    assert!(first["id"].is_string() && first["kind"].is_string() && first["score"].is_number(), "{first}");
}

/// REQ: F-009
#[test]
fn search_009_rank_hook_replace_reorders_the_results_before_they_are_shown() {
    let mut host = activated_server();
    let first = add_terminal(&mut host, "first shell", "alpha needle").to_string();
    let second = add_terminal(&mut host, "second shell", "beta needle").to_string();
    let reversed = json!({ "hits": [
        { "id": second, "kind": "terminal", "score": 1.0 },
        { "id": first, "kind": "terminal", "score": 0.5 },
    ]});
    host.extensions().on_run_hook("ilium.core.search/rank", HookReply::Replace(reversed.to_string()));
    let hits = ask(&mut host, 1, "needle");
    let ids: Vec<&str> = hits.iter().map(|hit| hit["id"].as_str().expect("id")).collect();
    assert_eq!(ids, [second.as_str(), first.as_str()]);
}

/// REQ: F-009
#[test]
fn search_009_rank_hook_pass_keeps_the_original_order() {
    let mut host = activated_server();
    add_terminal(&mut host, "first shell", "alpha needle");
    add_terminal(&mut host, "second shell", "beta needle");
    let plain = ask(&mut host, 1, "needle");
    host.extensions().on_run_hook("ilium.core.search/rank", HookReply::Pass);
    let passed = ask(&mut host, 2, "needle");
    assert_eq!(plain, passed);
}

/// REQ: F-009
#[test]
fn search_009_rank_hook_refusal_does_not_hide_results() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "needle");
    host.extensions().on_run_hook("ilium.core.search/rank", HookReply::Refuse("no".to_string()));
    let hits = ask(&mut host, 1, "needle");
    assert_eq!(hits.len(), 1);
}

/// REQ: F-009
#[test]
fn search_009_sources_entry_service_is_asked_and_its_hits_join_the_results() {
    let mut host = activated_server();
    host.extensions().contribute_collect(
        "ilium.core.search/sources",
        "issues",
        50,
        json!({ "title": "Issues", "kind": "issue", "service": "example.issues/search" }),
    );
    host.extensions().set_service_reply(
        "example.issues/search",
        json!({ "hits": [{ "id": "issue-12", "kind": "issue", "title": "Deploy fails", "score": 0.9 }] }),
    );
    let hits = ask(&mut host, 1, "deploy");
    assert!(hits.iter().any(|hit| hit["id"] == "issue-12" && hit["kind"] == "issue"), "{hits:?}");
}

/// REQ: F-009
#[test]
fn search_009_sources_without_contributions_add_only_the_built_in_kinds() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "deploy now");
    let hits = ask(&mut host, 1, "deploy");
    assert!(hits.iter().all(|hit| ["agent", "terminal", "file", "meaning"].contains(&hit["kind"].as_str().unwrap_or(""))));
}

/// REQ: F-009
#[test]
fn search_009_a_failing_source_service_does_not_remove_the_other_results() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "deploy now");
    host.extensions().contribute_collect(
        "ilium.core.search/sources",
        "issues",
        50,
        json!({ "title": "Issues", "kind": "issue", "service": "example.issues/search" }),
    );
    host.extensions().fail_service("example.issues/search");
    let hits = ask(&mut host, 1, "deploy");
    assert!(hits.iter().any(|hit| hit["kind"] == "terminal"));
}
