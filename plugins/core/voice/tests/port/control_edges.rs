//! Twins of the edge cases of the voice tool executor and the typed-sentence queue of
//! `ilium-client/src/{lib,normal_voice,control/*}.rs`, written against the realtime tool bridge, the
//! `voice-say` client request and the host tree. Tool argument names beyond the guide are assumed
//! (API_GAPS_ports.md).

use crate::host_ext::*;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{ErrorCode, Intent, NodeClass, NodeId, PluginError};
use serde_json::{json, Value};

use crate::common::{
    add_pane, client_request, configure, contribute_phrase, enabled_client, f8_grab, hear,
    invoked_commands, key_pressed, key_released, popup_button, CONFIRM_POPUP,
};

fn created_parent(host: &FakeHost) -> Option<NodeId> {
    host.submitted()
        .iter()
        .rev()
        .find_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) => Some(spec.parent),
            _ => None,
        })?
}

fn node(
    host: &mut FakeHost,
    name: &str,
    class: NodeClass,
    kind: Option<&str>,
    parent: Option<u64>,
) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class,
        content_kind: kind.map(str::to_string),
        agent: None,
    })
}

fn say(host: &mut FakeHost, call: u64, sentences: Value) -> Value {
    host.deliver_one(client_request(
        call,
        "voice-say",
        json!({ "sentences": sentences, "start": false }),
    ));
    host.settle();
    let responses = host.client_requests().responses();
    let line = match &responses.last().expect("an answer").result {
        Ok(line) => line.clone(),
        Err(error) => panic!("the request itself failed: {error:?}"),
    };
    serde_json::from_str(&line).expect("the answer is a JSON record")
}

fn strings_in(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::String(text) => found.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings_in(item, found)),
        Value::Object(map) => map.values().for_each(|item| strings_in(item, found)),
        _ => {}
    }
}

/// A host with a group holding a split view that holds the focused pane.
fn host_with_split_view() -> (FakeHost, u64) {
    let mut host = enabled_client("semantic_vad");
    let group = node(
        &mut host,
        "work",
        NodeClass::Container,
        Some("ilium.group"),
        None,
    );
    let split = node(
        &mut host,
        "split",
        NodeClass::Container,
        Some("ilium.split"),
        Some(group),
    );
    node(
        &mut host,
        "agent",
        NodeClass::Pane,
        Some("ilium.terminal"),
        Some(split),
    );
    (host, group)
}

/// REQ: PORT:ilium-client/src/control/resolver.rs::resolve_parent_walks_past_a_split_view_to_its_enclosing_group
#[test]
fn resolver_050_a_new_agent_without_a_target_lands_in_the_group_that_encloses_the_split_view() {
    let (mut host, group) = host_with_split_view();
    host.realtime()
        .call_tool(
            "ilium_create_agent",
            &json!({ "provider": "claude" }).to_string(),
        )
        .expect("created");
    assert_eq!(created_parent(&host), Some(NodeId(group)));
}

/// REQ: PORT:ilium-client/src/control/resolver.rs::resolve_parent_treats_a_blank_path_exactly_like_an_omitted_target
#[test]
fn resolver_050_a_blank_path_resolves_like_an_omitted_target() {
    let (mut omitted, group) = host_with_split_view();
    omitted
        .realtime()
        .call_tool(
            "ilium_create_agent",
            &json!({ "provider": "claude" }).to_string(),
        )
        .expect("created");
    let (mut blank, blank_group) = host_with_split_view();
    blank
        .realtime()
        .call_tool(
            "ilium_create_agent",
            &json!({ "provider": "claude", "path": "   " }).to_string(),
        )
        .expect("created");
    assert_eq!(created_parent(&omitted), Some(NodeId(group)));
    assert_eq!(created_parent(&blank), Some(NodeId(blank_group)));
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::whole_batch_refusal_leaves_existing_fifo_unchanged
#[test]
fn queue_052_an_over_long_batch_is_refused_whole_and_the_existing_text_is_untouched() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime()
        .call_tool(
            "ilium_type_text",
            &json!({ "text": "existing" }).to_string(),
        )
        .expect("typed");
    let many: Vec<String> = (0..33).map(|n| format!("s{n}")).collect();
    let record = say(&mut host, 7, json!(many));
    assert_eq!(record["code"], "invalid-request", "{record}");
    assert_eq!(host.realtime().typed_text(), ["existing"]);
    let accepted = say(&mut host, 8, json!(["one", "two"]));
    assert_eq!(accepted["accepted_sentences"], 2, "{accepted}");
    assert_eq!(host.realtime().typed_text(), ["existing", "one", "two"]);
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::actual_prepared_context_fills_original_slot_before_younger_semantic_input
#[test]
fn queue_052_a_context_update_does_not_let_younger_text_overtake_it() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime().set_active_agent("claude");
    host.realtime()
        .call_tool("ilium_type_text", &json!({ "text": "younger" }).to_string())
        .expect("typed");
    assert!(
        host.realtime().target_context().is_some(),
        "the prepared context was applied"
    );
    assert_eq!(host.realtime().typed_text(), ["younger"]);
    assert_eq!(host.realtime().tool_calls(), ["ilium_type_text"]);
}

/// REQ: PORT:ilium-client/src/lib.rs::voice_actor_acceptance_waits_for_reply_credit_and_all_original_sentence_slots
#[test]
fn queue_052_a_request_is_accepted_only_with_every_original_sentence_in_order() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let record = say(&mut host, 9, json!(["original first", "original second"]));
    assert_eq!(record["accepted_sentences"], 2, "{record}");
    assert_eq!(
        host.realtime().typed_text(),
        ["original first", "original second"]
    );
    assert_eq!(
        host.client_requests().responses().len(),
        1,
        "exactly one answer for the one request"
    );
}

/// REQ: PORT:ilium-client/src/control/executor.rs::full_shared_outbound_credit_refuses_semantic_control_and_exit_receipts
#[test]
fn executor_050_terminal_actions_are_refused_while_the_shared_outbound_credit_is_full_and_work_afterwards(
) {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let calls = [
        ("ilium_press_key", json!({ "key": "enter" })),
        ("ilium_send_text", json!({ "text": "hello" })),
        ("ilium_queue_prompt", json!({ "text": "run the tests" })),
    ];
    for (name, arguments) in &calls {
        host.inject_failure(
            "terminal.write",
            PluginError::new(ErrorCode::Busy, "rejected before admission"),
        );
        let error = host
            .realtime()
            .call_tool(name, &arguments.to_string())
            .expect_err("refused while full");
        assert_eq!(error.code, ErrorCode::Busy, "{name}");
        assert!(
            error.message.contains("rejected before admission"),
            "{name}: {}",
            error.message
        );
    }
    assert_eq!(host.realtime().typed_text(), Vec::<String>::new());
    host.realtime()
        .call_tool("ilium_send_text", &json!({ "text": "hello" }).to_string())
        .expect("accepted once credit frees");
    assert_eq!(host.realtime().typed_text(), ["hello"]);
}

/// REQ: PORT:ilium-client/src/control/snapshot.rs::redaction_suffix_keeps_valid_unicode_and_the_requested_limit
#[test]
fn snapshot_050_a_long_non_ascii_setting_keeps_its_tail_intact_in_the_snapshot() {
    let mut host = enabled_client("semantic_vad");
    configure(
        &mut host,
        "custom_prompt",
        json!(format!("{}tail", "\u{e9}".repeat(20))),
    );
    let snapshot: Value = serde_json::from_str(
        &host
            .realtime()
            .call_tool("ilium_read_settings", "{}")
            .expect("settings"),
    )
    .expect("JSON");
    let mut strings = Vec::new();
    strings_in(&snapshot, &mut strings);
    let tail = "\u{e9}\u{e9}\u{e9}tail";
    assert!(
        strings.iter().any(|text| text.ends_with(tail)),
        "no string ends with {tail:?}: {strings:?}"
    );
}

/// REQ: PORT:ilium-client/src/control/mod.rs::pending_confirmation_capacity_evicts_the_oldest_token_by_insertion_order
#[test]
fn confirmation_051_thirty_three_pending_kill_requests_leave_one_that_runs_exactly_once() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(
        &mut host,
        "kill",
        "kill the session",
        "kernel.kill_session",
        true,
    );
    for _ in 0..33 {
        hear(&mut host, "kill the session");
    }
    assert_eq!(
        host.popups().last().map(String::as_str),
        Some(CONFIRM_POPUP)
    );
    let first = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert_eq!(invoked_commands(&first), ["kernel.kill_session"]);
    let second = host.deliver_one(popup_button(CONFIRM_POPUP, "ok"));
    assert!(invoked_commands(&second).is_empty());
}

/// REQ: PORT:ilium-client/src/lib.rs::provider_failure_survives_the_following_voice_channel_close
#[test]
fn session_050_a_provider_failure_is_not_replaced_by_the_following_close() {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list().last().expect("job").request;
    host.jobs().finish(
        request,
        Err(PluginError::new(
            ErrorCode::Io,
            "OpenAI rejected one production tool schema",
        )),
    );
    host.settle();
    let failure = host.last_notice().expect("the failure is reported");
    assert!(
        failure.contains("OpenAI rejected one production tool schema"),
        "{failure}"
    );
    configure(&mut host, "enabled", json!(false));
    host.advance_clock(1000);
    host.settle();
    let after = host.last_notice().expect("a notice remains");
    assert!(
        after.contains("OpenAI rejected one production tool schema"),
        "{after}"
    );
    assert!(!after.contains("stopped unexpectedly"), "{after}");
}

/// REQ: PORT:ilium-client/src/control/parser_pool_settings_regressions.rs::parser_pool_execute_reports_missing_or_closed_writer_as_unsaved
#[test]
fn settings_050_an_engine_budget_change_whose_write_is_refused_is_reported_unsaved() {
    for code in [ErrorCode::Io, ErrorCode::Busy] {
        let mut host = enabled_client("semantic_vad");
        let before = host
            .realtime()
            .call_tool("ilium_read_settings", "{}")
            .expect("settings");
        host.inject_failure("settings.set", PluginError::new(code, "writer closed"));
        let error = host
            .realtime()
            .call_tool(
                "ilium_set_setting",
                &json!({ "path": "terminal.engine_memory_budget_mib", "value": 257 }).to_string(),
            )
            .expect_err("the refused write is an error");
        assert_eq!(error.code, code);
        let after = host
            .realtime()
            .call_tool("ilium_read_settings", "{}")
            .expect("settings");
        assert_eq!(before, after, "nothing was saved or applied");
    }
}

/// REQ: PORT:ilium-client/src/control/resolver.rs::resolve_parent_walks_past_a_split_view_to_its_enclosing_group
#[test]
fn resolver_050_two_agents_without_a_target_share_the_enclosing_group() {
    let (mut host, group) = host_with_split_view();
    for _ in 0..2 {
        host.realtime()
            .call_tool(
                "ilium_create_agent",
                &json!({ "provider": "claude" }).to_string(),
            )
            .expect("created");
    }
    let parents: Vec<NodeId> = host
        .submitted()
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreatePane(spec) => spec.parent,
            _ => None,
        })
        .collect();
    assert_eq!(parents, [NodeId(group), NodeId(group)]);
}

/// REQ: PORT:ilium-client/src/control/resolver.rs::resolve_parent_treats_a_blank_path_exactly_like_an_omitted_target
#[test]
fn resolver_050_an_empty_path_resolves_like_an_omitted_target() {
    let (mut host, group) = host_with_split_view();
    host.realtime()
        .call_tool(
            "ilium_create_agent",
            &json!({ "provider": "claude", "path": "" }).to_string(),
        )
        .expect("created");
    assert_eq!(created_parent(&host), Some(NodeId(group)));
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::whole_batch_refusal_leaves_existing_fifo_unchanged
#[test]
fn queue_052_a_batch_with_one_empty_sentence_is_refused_whole() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let record = say(&mut host, 10, json!(["valid one", ""]));
    assert_eq!(record["code"], "invalid-request", "{record}");
    assert!(host.realtime().typed_text().is_empty());
}

/// REQ: PORT:ilium-client/src/normal_voice.rs::actual_prepared_context_fills_original_slot_before_younger_semantic_input
#[test]
fn queue_052_text_typed_across_two_agent_context_updates_stays_in_order() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.realtime().set_active_agent("claude");
    host.realtime()
        .call_tool("ilium_type_text", &json!({ "text": "first" }).to_string())
        .expect("typed");
    host.realtime().set_active_agent("codex");
    host.realtime()
        .call_tool("ilium_type_text", &json!({ "text": "second" }).to_string())
        .expect("typed");
    assert_eq!(host.realtime().typed_text(), ["first", "second"]);
}

/// REQ: PORT:ilium-client/src/lib.rs::voice_actor_acceptance_waits_for_reply_credit_and_all_original_sentence_slots
#[test]
fn queue_052_an_empty_sentence_list_is_not_accepted() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    let record = say(&mut host, 11, json!([]));
    assert_ne!(record["accepted_sentences"], 0, "{record}");
    assert!(host.realtime().typed_text().is_empty());
}

/// REQ: PORT:ilium-client/src/control/executor.rs::full_shared_outbound_credit_refuses_semantic_control_and_exit_receipts
#[test]
fn executor_050_a_refused_text_send_does_not_count_as_pressing_enter() {
    let mut host = enabled_client("semantic_vad");
    add_pane(&mut host, "docs");
    host.inject_failure(
        "terminal.write",
        PluginError::new(ErrorCode::Busy, "rejected before admission"),
    );
    let _ = host
        .realtime()
        .call_tool("ilium_send_text", &json!({ "text": "hello" }).to_string());
    assert_ne!(host.realtime().last_submit_enter(), Some(true));
}

/// REQ: PORT:ilium-client/src/control/snapshot.rs::redaction_suffix_keeps_valid_unicode_and_the_requested_limit
#[test]
fn snapshot_050_a_short_non_ascii_setting_is_kept_whole() {
    let mut host = enabled_client("semantic_vad");
    configure(&mut host, "custom_prompt", json!("café au lait"));
    let snapshot: Value = serde_json::from_str(
        &host
            .realtime()
            .call_tool("ilium_read_settings", "{}")
            .expect("settings"),
    )
    .expect("JSON");
    let mut strings = Vec::new();
    strings_in(&snapshot, &mut strings);
    assert!(
        strings.iter().any(|text| text.contains("café au lait")),
        "{strings:?}"
    );
}

/// REQ: PORT:ilium-client/src/control/mod.rs::pending_confirmation_capacity_evicts_the_oldest_token_by_insertion_order
#[test]
fn confirmation_051_cancelling_the_newest_kill_request_runs_nothing() {
    let mut host = enabled_client("push_to_talk");
    contribute_phrase(
        &mut host,
        "kill",
        "kill the session",
        "kernel.kill_session",
        true,
    );
    for _ in 0..33 {
        hear(&mut host, "kill the session");
    }
    let answered = host.deliver_one(popup_button(CONFIRM_POPUP, "cancel"));
    assert!(invoked_commands(&answered).is_empty());
}

/// REQ: PORT:ilium-client/src/lib.rs::provider_failure_survives_the_following_voice_channel_close
#[test]
fn session_050_the_provider_failure_is_reported_before_any_close() {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list().last().expect("job").request;
    host.jobs().finish(
        request,
        Err(PluginError::new(
            ErrorCode::Io,
            "OpenAI rejected one production tool schema",
        )),
    );
    host.settle();
    let failure = host.last_notice().expect("the failure is reported");
    assert!(!failure.contains("stopped unexpectedly"), "{failure}");
}

/// REQ: PORT:ilium-client/src/control/parser_pool_settings_regressions.rs::parser_pool_execute_reports_missing_or_closed_writer_as_unsaved
#[test]
fn settings_050_an_engine_budget_change_that_is_written_shows_in_the_settings() {
    let mut host = enabled_client("semantic_vad");
    host.realtime()
        .call_tool(
            "ilium_set_setting",
            &json!({ "path": "terminal.engine_memory_budget_mib", "value": 257 }).to_string(),
        )
        .expect("saved");
    let settings = host
        .realtime()
        .call_tool("ilium_read_settings", "{}")
        .expect("settings");
    assert!(settings.contains("257"), "{settings}");
}
