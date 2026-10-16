//! `ilium:host/inference` and the `inference-provider` export (api-inference.md). The positive
//! round trip uses one plugin as both caller and provider, as the manual's sequence diagram does.

use std::time::Duration;

use ilium_plugin_api::generated::exports::inference_provider::ProviderCall;
use ilium_plugin_api::generated::host::inference::{ChatMessage, Completion, CompletionRequest, HostInference};
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::{ErrorCode, PluginError};

use crate::common::*;

const GRANTS: &[&str] = &["inference.complete", "inference.provide"];

fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage { role: "user".to_owned(), content: "hi".to_owned() }],
        max_output_tokens: None,
        json_schema: None,
        purpose: "test".to_owned(),
    }
}

fn completion(text: &str) -> Completion {
    Completion { text: text.to_owned(), model: "m".to_owned(), input_tokens: 1, output_tokens: 1 }
}

fn seeded() -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.select_inference_provider("test.plugin");
    rig
}

/// Calls complete and returns the provider call id the plugin's own export received.
fn start(rig: &mut Rig) -> (RequestId, u64) {
    let id = rig.host.inference().complete(request()).expect("accepted");
    rig.host.pump(8);
    let call = rig.host.provider_calls().last().expect("the provider export was called").call;
    (id, call)
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_round_trip_through_the_provider() {
    let mut rig = seeded();
    let (id, call) = start(&mut rig);
    rig.host.inference().provider_reply(call, Ok(completion("hello"))).expect("reply");
    rig.host.pump(8);
    assert_eq!(rig.host.inference().take_completion(id).map(|c| c.text), Some("hello".to_owned()));
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_no_messages_invalid_argument() {
    let mut rig = seeded();
    let mut empty = request();
    empty.messages.clear();
    assert_eq!(code_of(rig.host.inference().complete(empty)), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_no_provider_selected_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(code_of(rig.host.inference().complete(request())), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_schema_size_boundaries_invalid_argument() {
    let mut rig = seeded();
    let schema = |len: usize| format!("\"{}\"", "a".repeat(len - 2));
    let mut at_limit = request();
    at_limit.json_schema = Some(schema(64 * KIB));
    assert!(rig.host.inference().complete(at_limit).is_ok());
    let mut over = request();
    over.json_schema = Some(schema(64 * KIB + 1));
    assert_eq!(code_of(rig.host.inference().complete(over)), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_schema_not_json_invalid_argument() {
    let mut rig = seeded();
    let mut bad = request();
    bad.json_schema = Some("{not json".to_owned());
    assert_eq!(code_of(rig.host.inference().complete(bad)), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_messages_over_four_mib_invalid_argument() {
    let mut rig = seeded();
    let mut big = request();
    big.messages[0].content = text_of(4 * MIB + 1);
    assert_eq!(code_of(rig.host.inference().complete(big)), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_sixty_fifth_pending_request_busy() {
    let mut rig = seeded();
    for _ in 0..64 {
        rig.host.inference().complete(request()).expect("within the pending limit");
    }
    assert_eq!(code_of(rig.host.inference().complete(request())), ErrorCode::Busy);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_unanswered_after_120_seconds_timed_out() {
    let mut rig = seeded();
    let (id, _) = start(&mut rig);
    rig.host.advance_clock(Duration::from_secs(120));
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), Some(ErrorCode::TimedOut));
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_just_before_120_seconds_still_pending() {
    let mut rig = seeded();
    let (id, _) = start(&mut rig);
    rig.host.advance_clock(Duration::from_millis(119_999));
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), None);
}

/// REQ: WIT:ilium:host/inference.complete
#[test]
fn wit_inference_complete_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["inference.complete"]);
    assert_eq!(code_of(rig.host.inference().complete(request())), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/inference.take-completion
#[test]
fn wit_inference_take_completion_returns_once() {
    let mut rig = seeded();
    let (id, call) = start(&mut rig);
    rig.host.inference().provider_reply(call, Ok(completion("once"))).expect("reply");
    rig.host.pump(8);
    assert!(rig.host.inference().take_completion(id).is_some());
    assert!(rig.host.inference().take_completion(id).is_none());
}

/// REQ: WIT:ilium:host/inference.take-completion
#[test]
fn wit_inference_take_completion_before_the_reply_is_none() {
    let mut rig = seeded();
    let (id, _) = start(&mut rig);
    assert!(rig.host.inference().take_completion(id).is_none());
}

/// REQ: WIT:ilium:host/inference.take-completion
#[test]
fn wit_inference_take_completion_unknown_request_is_none() {
    let mut rig = seeded();
    assert!(rig.host.inference().take_completion(RequestId(77)).is_none());
}

/// REQ: WIT:ilium:host/inference.models
#[test]
fn wit_inference_models_round_trip_lists_the_provider_models() {
    let mut rig = seeded();
    rig.host.set_provider_models(vec!["alpha".to_owned(), "beta".to_owned()]);
    let id = rig.host.inference().models().expect("accepted");
    rig.host.pump(8);
    assert_eq!(rig.host.inference().take_models(id), Some(vec!["alpha".to_owned(), "beta".to_owned()]));
}

/// REQ: WIT:ilium:host/inference.models
#[test]
fn wit_inference_models_no_provider_selected_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(code_of(rig.host.inference().models()), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/inference.models
#[test]
fn wit_inference_models_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["inference.complete"]);
    assert_eq!(code_of(rig.host.inference().models()), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/inference.take-models
#[test]
fn wit_inference_take_models_returns_once() {
    let mut rig = seeded();
    rig.host.set_provider_models(vec!["alpha".to_owned()]);
    let id = rig.host.inference().models().expect("accepted");
    rig.host.pump(8);
    assert!(rig.host.inference().take_models(id).is_some());
    assert!(rig.host.inference().take_models(id).is_none());
}

/// REQ: WIT:ilium:host/inference.take-models
#[test]
fn wit_inference_take_models_unknown_request_is_none() {
    let mut rig = seeded();
    assert!(rig.host.inference().take_models(RequestId(78)).is_none());
}

/// REQ: WIT:ilium:host/inference.provider-reply
#[test]
fn wit_inference_provider_reply_error_outcome_fails_the_request() {
    let mut rig = seeded();
    let (id, call) = start(&mut rig);
    let failure = PluginError { code: ErrorCode::Network, message: "down".to_owned(), retry_after_ms: None };
    rig.host.inference().provider_reply(call, Err(failure)).expect("accepted");
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), Some(ErrorCode::Network));
}

/// REQ: WIT:ilium:host/inference.provider-reply
#[test]
fn wit_inference_provider_reply_unknown_call_not_found() {
    let mut rig = seeded();
    assert_eq!(code_of(rig.host.inference().provider_reply(9_999, Ok(completion("x")))), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/inference.provider-reply
#[test]
fn wit_inference_provider_reply_second_reply_not_found() {
    let mut rig = seeded();
    let (_, call) = start(&mut rig);
    rig.host.inference().provider_reply(call, Ok(completion("a"))).expect("first");
    assert_eq!(code_of(rig.host.inference().provider_reply(call, Ok(completion("b")))), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/inference.provider-reply
#[test]
fn wit_inference_provider_reply_after_the_deadline_not_found() {
    let mut rig = seeded();
    let (_, call) = start(&mut rig);
    rig.host.advance_clock(Duration::from_secs(121));
    rig.host.pump(8);
    assert_eq!(code_of(rig.host.inference().provider_reply(call, Ok(completion("late")))), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/inference.provider-reply
#[test]
fn wit_inference_provider_reply_text_size_boundaries_invalid_argument() {
    let mut rig = seeded();
    let (_, call) = start(&mut rig);
    assert_eq!(code_of(rig.host.inference().provider_reply(call, Ok(completion(&text_of(4 * MIB + 1))))), ErrorCode::InvalidArgument);
    assert!(rig.host.inference().provider_reply(call, Ok(completion(&text_of(4 * MIB)))).is_ok());
}

/// REQ: WIT:ilium:exports/inference-provider.complete
#[test]
fn wit_inference_provider_complete_export_receives_the_call_and_request() {
    let mut rig = seeded();
    let (_, call) = start(&mut rig);
    let received = rig.host.provider_calls();
    assert_eq!(received.last().map(|c| c.call), Some(call));
    assert_eq!(received.last().map(|c| c.request.purpose.clone()), Some("test".to_owned()));
}

/// REQ: WIT:ilium:exports/inference-provider.complete
#[test]
fn wit_inference_provider_complete_export_can_be_called_directly() {
    let mut rig = seeded();
    let call = ProviderCall { call: 5, request: request() };
    assert!(rig.host.call_provider_complete(call).is_ok());
}

/// REQ: WIT:ilium:exports/inference-provider.complete
#[test]
fn wit_inference_provider_complete_export_not_provided_not_found() {
    let mut rig = rig(&["inference.complete"]);
    let call = ProviderCall { call: 5, request: request() };
    assert_eq!(code_of(rig.host.call_provider_complete(call)), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:exports/inference-provider.models
#[test]
fn wit_inference_provider_models_export_returns_the_models() {
    let mut rig = seeded();
    rig.host.set_provider_models(vec!["alpha".to_owned()]);
    assert_eq!(rig.host.call_provider_models(), Ok(vec!["alpha".to_owned()]));
}

/// REQ: WIT:ilium:exports/inference-provider.models
#[test]
fn wit_inference_provider_models_export_empty_list_is_valid() {
    let mut rig = seeded();
    rig.host.set_provider_models(Vec::new());
    assert_eq!(rig.host.call_provider_models(), Ok(Vec::new()));
}
