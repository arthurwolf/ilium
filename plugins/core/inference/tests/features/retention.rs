//! Requirements COR-950, COR-951 and COR-952 of the Inference plugin (manual page
//! core-plugin-inference.md, "What is kept and for how long"): what the plugin writes to its logs,
//! the retention statement the Settings tab shows for the selected provider, and the one-time
//! retention confirmation before the first request to a remote provider.
//!
//! The rules are behaviour rules, so every test calls the plugin or the host. A check that needs a
//! testkit item the harness does not offer yet (the audit log, the privacy log, the retention
//! confirmation) is written as `unimplemented!` naming that item, and listed in API_GAPS.md.

use ilium_plugin_core_inference::{InferenceClient, InferenceServer};
use ilium_plugin_sdk::exports::ProviderCall;
use ilium_plugin_sdk::host::HttpResponse;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{PluginError, ViewTarget};
use ilium_plugin_sdk::InferenceProvider;
use serde_json::{json, Value};

use crate::common::{
    manifest_path, ollama_settings, openai_body, openai_settings, request, server_host,
    server_host_with,
};

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const OLLAMA_URL: &str = "http://127.0.0.1:11434/api/chat";
const PROMPT_MARK: &str = "PROMPT-MARK-5150 summarise the dock log";
const REPLY_MARK: &str = "REPLY-MARK-2718 the dock log is quiet";
const OPENAI_RETENTION: &str = "Kept 30 days, not used for training";
const OLLAMA_RETENTION: &str = "Stays on this machine";

fn complete_with(host: &mut FakeHost, number: u64, text: &str) -> Result<(), PluginError> {
    host.instance_mut::<InferenceServer>()
        .complete(ProviderCall {
            call: number,
            request: request("title", text),
        })
}

fn log_holds(host: &FakeHost, text: &str) -> bool {
    host.logs().iter().any(|line| line.contains(text))
}

/// A client-side host with the plugin settings given, activated and ready to render its tab.
fn client_host_with(settings: Value) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .with_settings(settings)
        .build();
    host.activate::<InferenceClient>();
    host
}

fn inference_tab_text(host: &mut FakeHost) -> String {
    host.render(ViewTarget::SettingsPage("inference".to_string()), 100, 30)
        .text()
}

/// The settings with the OpenAI-compatible provider selected, carrying `retention` when given.
fn openai_settings_with_retention(retention: Option<&str>) -> Value {
    let mut entry = openai_settings()["openai"].clone();
    if let Some(text) = retention {
        entry["retention"] = json!(text);
    }
    json!({ "selected_provider": "open_ai", "openai": entry })
}

/// The settings with Ollama selected, its entry carrying a retention statement.
fn ollama_settings_with_retention() -> Value {
    let mut entry = ollama_settings()["ollama"].clone();
    entry["retention"] = json!(OLLAMA_RETENTION);
    json!({ "selected_provider": "ollama", "openai": openai_settings()["openai"].clone(), "ollama": entry })
}

fn chat_ok(text: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: openai_body(text, "gpt-5.6-terra", 11, 2),
    }
}

fn ollama_ok(text: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: json!({"model": "llama3", "message": {"role": "assistant", "content": text}})
            .to_string()
            .into_bytes(),
    }
}

/// REQ: COR-950
#[test]
fn cor_950_the_plugin_log_never_holds_the_prompt_text_of_a_completion() {
    let mut host = server_host();
    host.http().expect("POST", CHAT_URL, chat_ok("ok"));
    complete_with(&mut host, 1, PROMPT_MARK).expect("accepted");
    host.settle();
    assert!(
        !log_holds(&host, PROMPT_MARK),
        "no plugin log line may carry the prompt text"
    );
}

/// REQ: COR-950
#[test]
fn cor_950_the_plugin_log_never_holds_the_reply_text_of_a_completion() {
    let mut host = server_host();
    host.http().expect("POST", CHAT_URL, chat_ok(REPLY_MARK));
    complete_with(&mut host, 2, "hello").expect("accepted");
    host.settle();
    assert!(
        !log_holds(&host, REPLY_MARK),
        "no plugin log line may carry the reply text"
    );
}

/// REQ: COR-950
#[test]
fn cor_950_a_failed_provider_call_logs_neither_the_prompt_nor_the_error_body() {
    let mut host = server_host();
    host.http().expect(
        "POST",
        CHAT_URL,
        HttpResponse {
            status: 503,
            headers: vec![],
            body: format!("{{\"error\": \"overloaded\", \"echo\": \"{PROMPT_MARK}\"}}")
                .into_bytes(),
        },
    );
    complete_with(&mut host, 3, PROMPT_MARK).expect("accepted");
    host.settle();
    assert!(
        !log_holds(&host, PROMPT_MARK),
        "a failed call must not log the prompt, even when the error body echoes it"
    );
}

/// REQ: COR-950
#[test]
fn cor_950_the_audit_log_holds_no_prompt_or_reply_text() {
    unimplemented!("ilium-plugin-testkit::FakeHost::audit_records")
}

/// REQ: COR-950
#[test]
fn cor_950_the_privacy_log_holds_metadata_but_no_prompt_or_reply_text() {
    unimplemented!("ilium-plugin-testkit::FakeHost::privacy_log")
}

/// REQ: COR-951
#[test]
fn cor_951_settings_show_the_retention_statement_of_the_selected_provider() {
    let mut host = client_host_with(openai_settings_with_retention(Some(OPENAI_RETENTION)));
    let screen = inference_tab_text(&mut host);
    assert!(
        screen.contains(OPENAI_RETENTION),
        "the Inference tab must show the retention statement of the selected entry"
    );
}

/// REQ: COR-951
#[test]
fn cor_951_settings_show_retention_unknown_when_the_entry_has_no_statement() {
    let mut host = client_host_with(openai_settings_with_retention(None));
    let screen = inference_tab_text(&mut host);
    assert!(
        screen.contains("retention unknown"),
        "an entry without a retention statement must show the text \"retention unknown\""
    );
}

/// REQ: COR-951
#[test]
fn cor_951_settings_show_only_the_selected_providers_statement() {
    let mut host = client_host_with(ollama_settings_with_retention());
    let screen = inference_tab_text(&mut host);
    assert!(
        screen.contains(OLLAMA_RETENTION),
        "the selected Ollama entry's statement must be shown"
    );
    assert!(
        !screen.contains(OPENAI_RETENTION),
        "a statement of an unselected provider must not be shown"
    );
}

/// REQ: COR-951
#[test]
fn cor_951_settings_never_imply_that_ilium_enforces_the_statement() {
    let mut host = client_host_with(openai_settings_with_retention(Some(OPENAI_RETENTION)));
    let screen = inference_tab_text(&mut host);
    assert!(
        screen.contains(OPENAI_RETENTION),
        "the statement must be shown before its wording is checked"
    );
    assert!(
        !screen.contains("enforced by Ilium") && !screen.contains("Ilium enforces"),
        "the Settings text must not say that Ilium enforces the provider's statement"
    );
}

/// REQ: COR-952
#[test]
fn cor_952_a_remote_request_is_not_sent_before_the_user_confirms_the_retention_statement() {
    let mut host = server_host_with(openai_settings_with_retention(Some(OPENAI_RETENTION)));
    host.http().expect("POST", CHAT_URL, chat_ok("ok"));
    let _ = complete_with(&mut host, 4, "hello");
    host.settle();
    assert!(
        host.http().calls().is_empty(),
        "no request may reach the remote provider before the user confirms its retention statement"
    );
}

/// REQ: COR-952
#[test]
fn cor_952_a_remote_provider_without_a_statement_also_waits_for_confirmation() {
    let mut host = server_host_with(openai_settings_with_retention(None));
    host.http().expect("POST", CHAT_URL, chat_ok("ok"));
    let _ = complete_with(&mut host, 5, "hello");
    host.settle();
    assert!(
        host.http().calls().is_empty(),
        "a remote provider with the retention text \"retention unknown\" still needs the confirmation"
    );
}

/// REQ: COR-952
#[test]
fn cor_952_a_local_provider_sends_without_any_retention_confirmation() {
    let mut host = server_host_with(ollama_settings_with_retention());
    host.http().expect("POST", OLLAMA_URL, ollama_ok("ok"));
    complete_with(&mut host, 6, "hello").expect("accepted");
    host.settle();
    let calls = host.http().calls();
    assert_eq!(
        calls.first().map(|call| call.url.as_str()),
        Some(OLLAMA_URL),
        "the local provider must send its request with no confirmation step"
    );
}

/// REQ: COR-952
#[test]
fn cor_952_the_request_goes_out_once_the_user_has_confirmed_the_statement() {
    unimplemented!("ilium-plugin-testkit::FakeSession::confirm_retention")
}

/// REQ: COR-952
#[test]
fn cor_952_the_confirmation_is_asked_once_per_provider_not_once_per_request() {
    unimplemented!("ilium-plugin-testkit::FakeSession::retention_confirmations")
}
