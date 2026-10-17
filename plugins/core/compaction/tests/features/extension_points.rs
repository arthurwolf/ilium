//! Extension points, service and topic of the plugin (page "Extension points"; api-extension-points.md).

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{CommandOrigin, ErrorCode};
use serde_json::{json, Value};

use crate::common::*;

const BEFORE_COMPACT: &str = "ilium.core.compaction/before-compact";
const TOPIC_FINISHED: &str = "ilium.core.compaction/finished";

fn start(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) {
    host.deliver_one(command_on(
        COMPACT_COMMAND,
        CommandOrigin::Palette,
        Some(pane),
    ));
}

/// REQ: F-080
#[test]
fn doc_extension_table_lists_techniques_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "| `techniques` |");
    assert_eq!(row[2], "collect");
    assert_eq!(row[3], "Add a compaction technique.");
    assert_eq!(
        row[4],
        "entry: id, title, service (summarises a transcript)"
    );
    assert_eq!(row[5], "standard");
}

/// REQ: F-080
#[test]
fn doc_extension_table_lists_before_compact_as_a_standard_veto_hook() {
    let row = table_row(&page_text(), "| `before-compact` |");
    assert_eq!(row[2], "hook (veto)");
    assert_eq!(row[4], "pane, technique, tokens");
    assert_eq!(row[5], "standard");
}

/// REQ: F-080
#[test]
fn doc_service_and_topic_are_named_on_the_page() {
    let page = page_text();
    assert!(page.contains("`ilium.core.compaction/compact`: start: start a compaction of a pane"));
    assert!(
        page.contains("`ilium.core.compaction/finished`: { pane, tokens_before, tokens_after }")
    );
}

/// REQ: F-080
#[test]
fn doc_the_plugin_is_replaceable_by_a_package_that_replaces_its_id() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("\"replaces\": [\"ilium.core.compaction\"]"));
}

/// REQ: F-080
#[test]
fn extension_080_the_manifest_lists_no_topic_subscription_because_it_only_publishes() {
    assert!(manifest_value()["contributes"].get("topics").is_none());
}

/// REQ: F-080
#[test]
fn extension_080_before_compact_is_asked_once_before_the_job_with_pane_technique_and_tokens() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(BEFORE_COMPACT, HookReply::Pass);
    start(&mut host, pane);
    let calls = host.all_hook_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].point, BEFORE_COMPACT);
    let payload: Value = serde_json::from_str(&calls[0].payload_json).expect("payload is JSON");
    let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["pane", "technique", "tokens"]);
    assert_eq!(payload["pane"], json!(pane));
    assert_eq!(payload["technique"], "claude_code");
}

/// REQ: F-080
#[test]
fn extension_080_a_refusing_contributor_stops_the_compaction() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(
        BEFORE_COMPACT,
        HookReply::Refuse("a test run is active".to_string()),
    );
    start(&mut host, pane);
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-080
#[test]
fn extension_080_a_refusal_leaves_the_agent_running_and_the_pane_unfrozen() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(
        BEFORE_COMPACT,
        HookReply::Refuse("a test run is active".to_string()),
    );
    start(&mut host, pane);
    let frozen = intents(&host.submitted())
        .iter()
        .any(|intent| matches!(intent, ilium_plugin_sdk::types::Intent::FreezePane(_)));
    assert!(!frozen);
}

/// REQ: F-080
#[test]
fn extension_080_a_pass_reply_lets_the_compaction_start() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(BEFORE_COMPACT, HookReply::Pass);
    start(&mut host, pane);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn extension_080_a_reply_the_mode_does_not_allow_counts_as_pass() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(BEFORE_COMPACT, HookReply::Replace("{}".to_string()));
    start(&mut host, pane);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn extension_080_a_late_or_trapping_handler_counts_as_pass() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_trap(BEFORE_COMPACT);
    start(&mut host, pane);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn extension_080_the_first_refusal_wins_and_later_handlers_do_not_run() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.set_hook_reply(BEFORE_COMPACT, HookReply::Refuse("first".to_string()));
    host.set_hook_reply(BEFORE_COMPACT, HookReply::Pass);
    start(&mut host, pane);
    assert!(host.jobs().list().is_empty());
    assert_eq!(host.all_hook_calls().len(), 1);
}

/// REQ: F-080
#[test]
fn extension_080_a_finished_compaction_publishes_the_finished_topic_with_pane_and_token_counts() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Ok(
            b"1. Primary Request and Intent: x\n7. Pending Tasks: none\n8. Current Work: y"
                .to_vec(),
        ),
    );
    let topics = host.published_topics();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].0, TOPIC_FINISHED);
    let payload: Value = serde_json::from_str(&topics[0].1).expect("payload is JSON");
    let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["pane", "tokens_after", "tokens_before"]);
    assert_eq!(payload["pane"], json!(pane));
    assert!(payload["tokens_after"].as_u64() < payload["tokens_before"].as_u64());
}

/// REQ: F-080
#[test]
fn extension_080_a_failed_compaction_publishes_no_finished_topic() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    let error = ilium_plugin_sdk::types::PluginError {
        code: ErrorCode::Internal,
        message: "failed".to_string(),
        retry_after_ms: None,
    };
    host.jobs().finish(request, Err(error));
    assert!(host.published_topics().is_empty());
}

/// REQ: F-080
#[test]
fn extension_080_the_compact_service_starts_a_compaction_of_the_named_pane() {
    let mut session = FakeSession::new();
    session.install_core("compaction").start();
    let pane = add_agent_pane(
        session.server(PLUGIN_ID),
        "claude",
        "claude",
        ilium_plugin_sdk::types::AgentActivity::Idle,
    );
    session
        .server(PLUGIN_ID)
        .agents()
        .set_transcript(pane, transcript_of(500));
    session
        .call_service(
            "ilium.core.agent-toolbar",
            "ilium.core.compaction/compact",
            "^0.1",
            &json!({ "pane": pane }).to_string(),
        )
        .expect("service answers");
    assert_eq!(session.server(PLUGIN_ID).jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn extension_080_the_compact_service_refuses_an_unknown_pane_with_not_found() {
    let mut session = FakeSession::new();
    session.install_core("compaction").start();
    let error = session
        .call_service(
            "ilium.core.agent-toolbar",
            "ilium.core.compaction/compact",
            "^0.1",
            &json!({ "pane": 4_242 }).to_string(),
        )
        .expect_err("no such pane");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-080
#[test]
fn extension_080_the_compact_service_refuses_a_request_without_a_pane_with_invalid_argument() {
    let mut session = FakeSession::new();
    session.install_core("compaction").start();
    let error = session
        .call_service(
            "ilium.core.agent-toolbar",
            "ilium.core.compaction/compact",
            "^0.1",
            "{}",
        )
        .expect_err("pane missing");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-080
#[test]
fn extension_080_a_contributed_technique_appears_in_the_technique_rows_of_the_settings_tab() {
    let mut host = client_host();
    host.set_extension_entries(
        "ilium.core.compaction/techniques",
        vec![json!({ "id": "acme.terse", "title": "Terse", "service": "acme.terse/summarise" })],
    );
    let screen = host.render(
        ilium_plugin_sdk::types::ViewTarget::SettingsPage("remote-compaction".to_string()),
        120,
        40,
    );
    assert!(screen.text().contains("Terse"));
}
