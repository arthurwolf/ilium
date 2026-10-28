//! Twins of `ilium-client/src/lib.rs` offer handling: how the client side answers a `voice-say` request.

use serde_json::{json, Value};

use crate::common::{client_host, client_request, enabled_client};

fn answer(host: &mut ilium_plugin_sdk::testing::FakeHost, call: u64, payload: Value) -> String {
    host.deliver_one(client_request(call, "voice-say", payload));
    host.settle();
    let responses = host.client_requests().responses();
    match &responses.last().expect("an answer").result {
        Ok(line) => line.clone(),
        Err(error) => error.message.clone(),
    }
}

/// REQ: PORT:ilium-client/src/lib.rs::a_server_offer_becomes_a_pending_offer_for_the_voice_owner
#[test]
fn offer_052_a_server_offer_is_answered_with_one_response_for_its_call() {
    let mut host = enabled_client("semantic_vad");
    let _ = answer(
        &mut host,
        7,
        json!({ "sentences": ["hello"], "start": false }),
    );
    let responses = host.client_requests().responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].call, 7);
}

/// REQ: PORT:ilium-client/src/lib.rs::a_start_request_switches_voice_on_only_when_it_is_off_or_failed
#[test]
fn offer_052_start_switches_voice_on_only_when_it_is_off() {
    let mut off = client_host();
    let line = answer(
        &mut off,
        1,
        json!({ "sentences": ["hello"], "start": true }),
    );
    let record: Value = serde_json::from_str(&line).expect("record");
    assert_eq!(record["started_voice"], true);
    assert_eq!(
        off.kernel_settings().get("voice-control", "enabled"),
        Some(json!(true))
    );

    let mut running = enabled_client("semantic_vad");
    let line = answer(
        &mut running,
        2,
        json!({ "sentences": ["hello"], "start": true }),
    );
    let record: Value = serde_json::from_str(&line).expect("record");
    assert_eq!(record["started_voice"], false);
}

/// REQ: PORT:ilium-client/src/lib.rs::a_start_request_switches_voice_on_only_when_it_is_off_or_failed
#[test]
fn offer_052_without_start_a_switched_off_client_answers_voice_off() {
    let mut host = client_host();
    let line = answer(
        &mut host,
        1,
        json!({ "sentences": ["hello"], "start": false }),
    );
    assert!(line.contains("voice-off"), "{line}");
    assert_eq!(
        host.kernel_settings().get("voice-control", "enabled"),
        Some(json!(false))
    );
}

/// REQ: PORT:ilium-client/src/lib.rs::a_server_offer_becomes_a_pending_offer_for_the_voice_owner
#[test]
fn offer_052_an_offer_to_an_enabled_client_is_not_answered_voice_off() {
    let mut host = enabled_client("semantic_vad");
    let line = answer(
        &mut host,
        3,
        json!({ "sentences": ["hello"], "start": false }),
    );
    assert!(!line.contains("voice-off"), "{line}");
}
