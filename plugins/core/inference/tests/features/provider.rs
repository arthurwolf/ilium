//! The provider side (F-084): `complete` accepts at once and a task answers with `provider-reply`
//! under the same call number; `models`; routing to the OpenAI-compatible and Ollama endpoints.

use ilium_plugin_core_inference::InferenceServer;
use ilium_plugin_sdk::exports::ProviderCall;
use ilium_plugin_sdk::host::HttpResponse;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::Event;
use ilium_plugin_sdk::InferenceProvider;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::{
    ollama_settings, openai_body, request, row_command, server_host, server_host_with,
};

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

fn ok_response(body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body,
    }
}

fn call(
    host: &mut FakeHost,
    number: u64,
    text: &str,
) -> Result<(), ilium_plugin_sdk::types::PluginError> {
    host.instance_mut::<InferenceServer>()
        .complete(ProviderCall {
            call: number,
            request: request("title", text),
        })
}

fn first_body(host: &mut FakeHost) -> Value {
    let calls = host.http().calls();
    serde_json::from_slice(calls[0].body.as_ref().expect("request body")).expect("JSON body")
}

/// REQ: F-084
#[test]
fn provider_084_complete_accepts_the_call_and_returns_ok_at_once() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 3, 1)),
    );
    assert!(call(&mut host, 7, "hello").is_ok());
}

/// REQ: F-084
#[test]
fn provider_084_no_reply_exists_before_the_task_has_run() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 3, 1)),
    );
    call(&mut host, 7, "hello").expect("accepted");
    assert!(host.inference().provider_replies().is_empty());
}

/// REQ: F-084
#[test]
fn provider_084_a_task_answers_with_the_same_call_number() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 3, 1)),
    );
    call(&mut host, 7, "hello").expect("accepted");
    host.settle();
    let replies = host.inference().provider_replies();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].0, 7);
}

/// REQ: F-084
#[test]
fn provider_084_the_reply_carries_text_model_and_token_counts_of_the_response() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 11, 2)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let (_, outcome) = host.inference().provider_replies().remove(0);
    assert_eq!(
        outcome.expect("reply ok"),
        ilium_plugin_sdk::host::Completion {
            text: "ok".to_string(),
            model: "gpt-5.6-terra".to_string(),
            input_tokens: 11,
            output_tokens: 2,
        }
    );
}

/// REQ: F-084
#[test]
fn provider_084_two_calls_are_answered_under_their_own_numbers() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("first", "gpt-5.6-terra", 1, 1)),
    );
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("second", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 10, "a").expect("accepted");
    call(&mut host, 11, "b").expect("accepted");
    host.settle();
    let mut replies: Vec<(u64, String)> = host
        .inference()
        .provider_replies()
        .into_iter()
        .map(|(number, outcome)| (number, outcome.expect("ok").text))
        .collect();
    replies.sort();
    assert_eq!(
        replies,
        [(10, "first".to_string()), (11, "second".to_string())]
    );
}

/// REQ: F-084
#[test]
fn provider_084_a_server_error_status_fails_the_call_with_an_err_reply() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        HttpResponse {
            status: 500,
            headers: vec![],
            body: b"{}".to_vec(),
        },
    );
    call(&mut host, 3, "hello").expect("accepted");
    host.settle();
    let replies = host.inference().provider_replies();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].1.is_err());
}

/// REQ: F-084
#[test]
fn provider_084_exactly_one_reply_is_made_per_call() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 4, "hello").expect("accepted");
    host.settle();
    host.settle();
    assert_eq!(host.inference().provider_replies().len(), 1);
}

/// REQ: F-084
#[test]
fn provider_084_openai_requests_post_to_the_chat_completions_endpoint() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let calls = host.http().calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].url, CHAT_URL);
}

/// REQ: F-084
#[test]
fn provider_084_openai_requests_send_the_key_as_a_bearer_header() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let calls = host.http().calls();
    assert!(calls[0]
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("authorization") && h.value == "Bearer sk-test-key"));
}

/// REQ: F-084
#[test]
fn provider_084_openai_body_names_the_selected_model_and_carries_the_messages() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let body = first_body(&mut host);
    assert_eq!(body["model"], "gpt-5.6-terra");
    assert_eq!(
        body["messages"],
        json!([{"role": "user", "content": "hello"}])
    );
}

/// REQ: F-084
#[test]
fn provider_084_the_output_limit_uses_max_completion_tokens_for_official_openai() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    let mut limited = request("title", "Reply with the word ok.");
    limited.max_output_tokens = Some(16);
    host.instance_mut::<InferenceServer>()
        .complete(ProviderCall {
            call: 1,
            request: limited,
        })
        .expect("accepted");
    host.settle();
    assert_eq!(first_body(&mut host)["max_completion_tokens"], 16);
}

/// REQ: F-084
#[test]
fn provider_084_each_request_has_a_45_second_timeout() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    assert_eq!(host.http().calls()[0].timeout_millis, 45_000);
}

/// REQ: F-084
#[test]
fn provider_084_a_custom_openai_compatible_url_is_used_as_the_base() {
    let settings = json!({
        "selected_provider": "open_ai",
        "openai": { "base_url": "http://localhost:8080/v1", "api_key": "k", "model": "local" }
    });
    let mut host = server_host_with(settings);
    host.http().expect(
        "POST",
        "http://localhost:8080/v1/chat/completions",
        ok_response(openai_body("ok", "local", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    assert_eq!(
        host.http().calls()[0].url,
        "http://localhost:8080/v1/chat/completions"
    );
}

/// REQ: F-084
#[test]
fn provider_084_a_blank_base_url_resolves_to_the_openai_default() {
    let settings = json!({
        "selected_provider": "open_ai",
        "openai": { "base_url": "", "api_key": "k", "model": "gpt-5.6-terra" }
    });
    let mut host = server_host_with(settings);
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    assert_eq!(host.http().calls()[0].url, CHAT_URL);
}

/// REQ: F-084
#[test]
fn provider_084_ollama_requests_post_to_api_chat_on_the_local_server() {
    let mut host = server_host_with(ollama_settings());
    host.http().expect(
        "POST",
        "http://127.0.0.1:11434/api/chat",
        ok_response(
            json!({"model": "llama3", "message": {"role": "assistant", "content": "ok"}})
                .to_string()
                .into_bytes(),
        ),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let calls = host.http().calls();
    assert_eq!(calls[0].url, "http://127.0.0.1:11434/api/chat");
    let (_, outcome) = host.inference().provider_replies().remove(0);
    assert_eq!(outcome.expect("ok").text, "ok");
}

/// REQ: F-084
#[test]
fn provider_084_ollama_needs_no_credential_header() {
    let mut host = server_host_with(ollama_settings());
    host.http().expect(
        "POST",
        "http://127.0.0.1:11434/api/chat",
        ok_response(
            json!({"model": "llama3", "message": {"role": "assistant", "content": "ok"}})
                .to_string()
                .into_bytes(),
        ),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let calls = host.http().calls();
    assert!(!calls[0]
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("authorization")));
}

/// REQ: F-084
#[test]
fn provider_084_ollama_body_names_the_model_and_is_not_streaming() {
    let mut host = server_host_with(ollama_settings());
    host.http().expect(
        "POST",
        "http://127.0.0.1:11434/api/chat",
        ok_response(
            json!({"message": {"role": "assistant", "content": "ok"}})
                .to_string()
                .into_bytes(),
        ),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    let body = first_body(&mut host);
    assert_eq!(body["model"], "llama3");
    assert_eq!(body["stream"], false);
}

/// REQ: F-084
#[test]
fn provider_084_activation_sends_nothing_when_no_provider_is_set() {
    let mut host = server_host_with(json!({}));
    host.settle();
    assert!(host.http().calls().is_empty());
}

/// REQ: F-084
#[test]
fn provider_084_activation_with_a_provider_still_sends_nothing_until_a_call_arrives() {
    let mut host = server_host();
    host.settle();
    assert!(host.http().calls().is_empty());
}

/// REQ: F-084
#[test]
fn provider_084_models_offers_the_saved_model_of_the_selected_provider() {
    let mut host = server_host();
    let models = host.instance_mut::<InferenceServer>().models();
    assert!(
        models.iter().any(|name| name == "gpt-5.6-terra"),
        "{models:?}"
    );
}

/// REQ: F-084
#[test]
fn provider_084_models_has_no_duplicates() {
    let mut host = server_host();
    let mut models = host.instance_mut::<InferenceServer>().models();
    let count = models.len();
    models.sort();
    models.dedup();
    assert_eq!(models.len(), count);
}

/// REQ: F-084
#[test]
fn provider_084_models_of_ollama_offer_its_saved_model() {
    let mut host = server_host_with(ollama_settings());
    let models = host.instance_mut::<InferenceServer>().models();
    assert!(models.iter().any(|name| name == "llama3"), "{models:?}");
}

/// REQ: F-084
#[test]
fn provider_084_load_models_reads_models_from_the_provider_and_sorts_the_ids() {
    let mut host = server_host();
    host.http().expect(
        "GET",
        "https://api.openai.com/v1/models",
        ok_response(
            json!({"data": [{"id": "gpt-b"}, {"id": "gpt-a"}]})
                .to_string()
                .into_bytes(),
        ),
    );
    host.deliver_one(row_command("ilium.core.inference/load-models"));
    host.settle();
    let models = host.instance_mut::<InferenceServer>().models();
    let a = models
        .iter()
        .position(|name| name == "gpt-a")
        .expect("gpt-a");
    let b = models
        .iter()
        .position(|name| name == "gpt-b")
        .expect("gpt-b");
    assert!(a < b);
}

/// REQ: F-084
#[test]
fn provider_084_load_models_keeps_the_saved_model_when_it_is_missing_from_the_list() {
    let mut host = server_host();
    host.http().expect(
        "GET",
        "https://api.openai.com/v1/models",
        ok_response(json!({"data": [{"id": "gpt-a"}]}).to_string().into_bytes()),
    );
    host.deliver_one(row_command("ilium.core.inference/load-models"));
    host.settle();
    let models = host.instance_mut::<InferenceServer>().models();
    assert!(
        models.iter().any(|name| name == "gpt-5.6-terra"),
        "{models:?}"
    );
}

/// REQ: F-084
#[test]
fn provider_084_a_failed_load_keeps_the_last_good_catalogue() {
    let mut host = server_host();
    host.http().expect(
        "GET",
        "https://api.openai.com/v1/models",
        ok_response(json!({"data": [{"id": "gpt-a"}]}).to_string().into_bytes()),
    );
    host.deliver_one(row_command("ilium.core.inference/load-models"));
    host.settle();
    host.http().expect(
        "GET",
        "https://api.openai.com/v1/models",
        HttpResponse {
            status: 401,
            headers: vec![],
            body: b"{}".to_vec(),
        },
    );
    host.deliver_one(row_command("ilium.core.inference/load-models"));
    host.settle();
    let models = host.instance_mut::<InferenceServer>().models();
    assert!(models.iter().any(|name| name == "gpt-a"), "{models:?}");
}

/// REQ: F-084
#[test]
fn provider_084_unrelated_events_make_no_submissions() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::PaneFocused(None));
    assert!(submissions.is_empty());
}

/// REQ: F-084
#[test]
fn provider_084_the_streaming_flag_is_not_set_for_plain_completions() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)),
    );
    call(&mut host, 1, "hello").expect("accepted");
    host.settle();
    assert!(host.http().streams().is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-084
    #[test]
    fn provider_084_reply_text_round_trips_the_response_text(text in "[ -~]{0,200}") {
        let mut host = server_host();
        host.http().expect("POST", CHAT_URL, ok_response(openai_body(&text, "gpt-5.6-terra", 1, 1)));
        call(&mut host, 5, "hello").expect("accepted");
        host.settle();
        let (number, outcome) = host.inference().provider_replies().remove(0);
        prop_assert_eq!(number, 5);
        prop_assert_eq!(outcome.expect("ok").text, text);
    }

    // REQ: F-084
    #[test]
    fn provider_084_every_call_number_is_returned_unchanged(number in any::<u64>()) {
        let mut host = server_host();
        host.http().expect("POST", CHAT_URL, ok_response(openai_body("ok", "gpt-5.6-terra", 1, 1)));
        call(&mut host, number, "hello").expect("accepted");
        host.settle();
        prop_assert_eq!(host.inference().provider_replies()[0].0, number);
    }
}
