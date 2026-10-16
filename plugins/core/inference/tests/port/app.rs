//! Twins of the tests of ilium-client/src/app.rs that belong to inference.

use ilium_plugin_sdk::testing::{FakeHost, FakeSession};
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, Outcome, PluginError, ViewTarget};
use serde_json::json;

use crate::common::*;

fn load_models(host: &mut FakeHost) {
    host.deliver_one(row_command(LOAD_MODELS));
    host.settle();
}

/// REQ: PORT:ilium-client/src/app.rs::openai_refresh_starts_discovery_without_changing_the_saved_model
#[test]
fn port_084_refresh_asks_the_provider_for_its_models_and_keeps_the_saved_model() {
    let mut host = server_host(openai_settings("saved-model"));
    host.http().expect("GET", OPENAI_MODELS_URL, ok(json!({"data": [{"id": "first-model"}]})));
    load_models(&mut host);
    let calls = host.http().calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[0].url, OPENAI_MODELS_URL);
    assert!(offered(&mut host).iter().any(|name| name == "saved-model"));
}

/// REQ: PORT:ilium-client/src/app.rs::openai_refresh_starts_discovery_without_changing_the_saved_model
#[test]
fn port_084_refresh_never_rewrites_the_saved_model_setting() {
    let mut host = server_host(openai_settings("saved-model"));
    host.http().expect("GET", OPENAI_MODELS_URL, ok(json!({"data": [{"id": "first-model"}]})));
    load_models(&mut host);
    let saved = host.settings().get("openai").expect("openai table");
    assert_eq!(saved["model"], "saved-model");
}

/// REQ: PORT:ilium-client/src/app.rs::openai_catalog_preserves_saved_selection_and_last_good_list_on_failure
#[test]
fn port_084_a_loaded_list_keeps_the_saved_selection_and_is_offered() {
    let mut host = server_host(openai_settings("saved-model"));
    host.http().expect(
        "GET",
        OPENAI_MODELS_URL,
        ok(json!({"data": [{"id": "first-model"}, {"id": "second-model"}]})),
    );
    load_models(&mut host);
    let models = offered(&mut host);
    for expected in ["first-model", "second-model", "saved-model"] {
        assert!(models.iter().any(|name| name == expected), "{expected} in {models:?}");
    }
    assert_eq!(host.settings().get("openai").expect("table")["model"], "saved-model");
}

/// REQ: PORT:ilium-client/src/app.rs::openai_catalog_preserves_saved_selection_and_last_good_list_on_failure
#[test]
fn port_084_a_401_refresh_keeps_the_last_good_list_and_the_saved_model() {
    let mut host = server_host(openai_settings("saved-model"));
    host.http().expect(
        "GET",
        OPENAI_MODELS_URL,
        ok(json!({"data": [{"id": "first-model"}, {"id": "second-model"}]})),
    );
    load_models(&mut host);
    host.http().expect("GET", OPENAI_MODELS_URL, status(401));
    load_models(&mut host);
    let models = offered(&mut host);
    assert!(models.iter().any(|name| name == "first-model"), "{models:?}");
    assert!(models.iter().any(|name| name == "second-model"), "{models:?}");
    assert_eq!(host.settings().get("openai").expect("table")["model"], "saved-model");
}

/// Starts a refresh, changes the settings while the answer is still pending, then lets it land.
fn refresh_with_change_in_flight(change: impl FnOnce(&mut FakeHost)) -> Vec<String> {
    let mut host = server_host(openai_settings("saved-model"));
    host.http().expect("GET", OPENAI_MODELS_URL, ok(json!({"data": [{"id": "stale-model"}]})));
    host.deliver_one(row_command(LOAD_MODELS));
    change(&mut host);
    host.settle();
    offered(&mut host)
}

/// REQ: PORT:ilium-client/src/app.rs::openai_discovery_rejects_credential_and_endpoint_aba_and_provider_switches
#[test]
fn port_084_a_credential_changed_and_restored_mid_flight_rejects_the_stale_list() {
    let models = refresh_with_change_in_flight(|host| {
        host.settings().set("openai", json!({"base_url": "https://api.openai.com/v1", "api_key": "changed", "model": "saved-model"})).expect("set");
        host.deliver_one(settings_changed(&["openai"]));
        host.settings().set("openai", json!({"base_url": "https://api.openai.com/v1", "api_key": "fixture-key", "model": "saved-model"})).expect("set");
        host.deliver_one(settings_changed(&["openai"]));
    });
    assert!(!models.iter().any(|name| name == "stale-model"), "{models:?}");
    assert!(models.iter().any(|name| name == "saved-model"), "{models:?}");
}

/// REQ: PORT:ilium-client/src/app.rs::openai_discovery_rejects_credential_and_endpoint_aba_and_provider_switches
#[test]
fn port_084_an_endpoint_changed_and_restored_mid_flight_rejects_the_stale_list() {
    let models = refresh_with_change_in_flight(|host| {
        host.settings().set("openai", json!({"base_url": "changed", "api_key": "fixture-key", "model": "saved-model"})).expect("set");
        host.deliver_one(settings_changed(&["openai"]));
        host.settings().set("openai", json!({"base_url": "https://api.openai.com/v1", "api_key": "fixture-key", "model": "saved-model"})).expect("set");
        host.deliver_one(settings_changed(&["openai"]));
    });
    assert!(!models.iter().any(|name| name == "stale-model"), "{models:?}");
}

/// REQ: PORT:ilium-client/src/app.rs::openai_discovery_rejects_credential_and_endpoint_aba_and_provider_switches
#[test]
fn port_084_switching_provider_and_back_mid_flight_rejects_the_stale_list() {
    let models = refresh_with_change_in_flight(|host| {
        host.settings().set("selected_provider", json!("ollama")).expect("set");
        host.deliver_one(settings_changed(&["selected_provider"]));
        host.settings().set("selected_provider", json!("open_ai")).expect("set");
        host.deliver_one(settings_changed(&["selected_provider"]));
    });
    assert!(!models.iter().any(|name| name == "stale-model"), "{models:?}");
}

/// REQ: PORT:ilium-client/src/app.rs::openai_mouse_model_selection_persists_without_opening_manual_prompt
#[test]
fn port_084_choosing_a_listed_model_in_the_tab_persists_the_model_key() {
    let mut host = client_host(openai_settings("saved-model"));
    host.send_action(
        ViewTarget::SettingsPage("inference".to_string()),
        "model",
        ActionKind::Changed,
        ActionValue::Text("first-model".to_string()),
    );
    assert_eq!(host.settings().get("model"), Some(json!("first-model")));
}

/// REQ: PORT:ilium-client/src/app.rs::openai_mouse_model_selection_persists_without_opening_manual_prompt
#[test]
fn port_084_choosing_a_model_opens_no_popup_and_leaves_the_api_key_alone() {
    let mut host = client_host(openai_settings("saved-model"));
    host.send_action(
        ViewTarget::SettingsPage("inference".to_string()),
        "model",
        ActionKind::Changed,
        ActionValue::Text("first-model".to_string()),
    );
    let writes = host.settings().writes();
    assert!(writes.iter().all(|write| write.0 == "model"), "only the model key is written");
    assert!(host.settings().get("api_key").is_none());
}

/// REQ: PORT:ilium-client/src/app.rs::ollama_discovery_selects_a_returned_model_and_keeps_the_spinner_active
#[test]
fn port_084_ollama_refresh_reads_api_tags_and_offers_the_returned_models_in_order() {
    let settings = json!({"selected_provider": "ollama", "ollama": {"base_url": "http://127.0.0.1:11434", "model": ""}});
    let mut host = server_host(settings);
    host.http().expect(
        "GET",
        OLLAMA_TAGS_URL,
        ok(json!({"models": [{"name": "qwen3.6:latest"}, {"name": "gemma4:12b"}]})),
    );
    load_models(&mut host);
    assert_eq!(host.http().calls()[0].url, OLLAMA_TAGS_URL);
    let models = offered(&mut host);
    let qwen = models.iter().position(|name| name == "qwen3.6:latest").expect("qwen");
    let gemma = models.iter().position(|name| name == "gemma4:12b").expect("gemma");
    assert!(qwen < gemma, "returned order is kept: {models:?}");
}

/// REQ: PORT:ilium-client/src/app.rs::ollama_discovery_selects_a_returned_model_and_keeps_the_spinner_active
#[test]
fn port_084_ollama_refresh_selects_the_first_returned_model_when_none_is_saved() {
    let settings = json!({"selected_provider": "ollama", "ollama": {"base_url": "http://127.0.0.1:11434", "model": ""}});
    let mut host = server_host(settings);
    host.http().expect(
        "GET",
        OLLAMA_TAGS_URL,
        ok(json!({"models": [{"name": "qwen3.6:latest"}, {"name": "gemma4:12b"}]})),
    );
    load_models(&mut host);
    assert_eq!(host.settings().get("ollama").expect("table")["model"], "qwen3.6:latest");
}

/// REQ: PORT:ilium-client/src/app.rs::ollama_discovery_keeps_existing_models_and_exposes_the_transport_error
#[test]
fn port_084_a_failed_ollama_refresh_keeps_the_models_already_listed() {
    let settings = json!({"selected_provider": "ollama", "ollama": {"base_url": "http://127.0.0.1:11434", "model": "qwen3.6:latest"}});
    let mut host = server_host(settings);
    host.http().expect("GET", OLLAMA_TAGS_URL, ok(json!({"models": [{"name": "qwen3.6:latest"}]})));
    load_models(&mut host);
    host.http().expect("GET", OLLAMA_TAGS_URL, status(503));
    load_models(&mut host);
    assert!(offered(&mut host).iter().any(|name| name == "qwen3.6:latest"));
    assert_eq!(host.settings().get("ollama").expect("table")["model"], "qwen3.6:latest");
}

/// REQ: PORT:ilium-client/src/app.rs::kilo_discovery_preserves_saved_selection_and_cycles_live_free_models
#[test]
fn port_084_kilo_refresh_keeps_the_saved_model_and_offers_live_models() {
    let settings = json!({"selected_provider": "kilo_gateway", "kilo_gateway": {"model": "saved/model:free"}});
    let mut host = server_host(settings);
    host.http().expect("GET", KILO_MODELS_URL, ok(json!({"data": [{"id": "stepfun/step-5-preview-free"}]})));
    load_models(&mut host);
    assert_eq!(host.http().calls()[0].url, KILO_MODELS_URL);
    let models = offered(&mut host);
    assert!(models.iter().any(|name| name == "stepfun/step-5-preview-free"), "{models:?}");
    assert!(models.iter().any(|name| name == "saved/model:free"), "{models:?}");
    assert_eq!(host.settings().get("kilo_gateway").expect("table")["model"], "saved/model:free");
}

/// REQ: PORT:ilium-client/src/app.rs::kilo_discovery_preserves_saved_selection_and_cycles_live_free_models
#[test]
fn port_084_kilo_models_always_include_the_stable_free_routes() {
    let settings = json!({"selected_provider": "kilo_gateway", "kilo_gateway": {"model": "saved/model:free"}});
    let mut host = server_host(settings);
    host.http().expect("GET", KILO_MODELS_URL, ok(json!({"data": [{"id": "stepfun/step-5-preview-free"}]})));
    load_models(&mut host);
    let models = offered(&mut host);
    assert!(models.iter().any(|name| name == "kilo-auto/free"), "{models:?}");
    assert!(models.iter().any(|name| name == "openrouter/free"), "{models:?}");
}

/// REQ: PORT:ilium-client/src/app.rs::inference_test_lifecycle_animates_and_keeps_failures_visible
#[test]
fn port_084_a_failed_provider_test_stays_visible_in_the_last_test_row() {
    let settings = json!({"selected_provider": "ollama", "ollama": {"base_url": "http://127.0.0.1:11434", "model": "llama3"}});
    let mut host = server_host(settings);
    host.deliver_one(row_command("ilium.core.inference/test-provider"));
    let pending = host.pending_requests();
    assert_eq!(pending.len(), 1, "the run starts one request");
    host.complete(
        pending[0].clone(),
        Outcome::Failed(PluginError { code: ErrorCode::Network, message: String::new(), retry_after_ms: None }),
    );
    host.settle();
    assert_eq!(host.settings().info("last_test"), Some("failed".to_string()));
}

/// REQ: PORT:ilium-client/src/app.rs::inference_test_lifecycle_animates_and_keeps_failures_visible
#[test]
fn port_084_the_last_test_row_keeps_failed_until_the_next_run_starts() {
    let mut host = server_host(openai_settings("saved-model"));
    host.deliver_one(row_command("ilium.core.inference/test-provider"));
    let first = host.pending_requests()[0].clone();
    host.complete(
        first,
        Outcome::Failed(PluginError { code: ErrorCode::Network, message: String::new(), retry_after_ms: None }),
    );
    host.settle();
    host.settle();
    assert_eq!(host.settings().info("last_test"), Some("failed".to_string()));
}

/// REQ: PORT:ilium-client/src/app.rs::settings_tabs_cycle_through_inference_and_every_existing_tab
#[test]
fn port_084_the_plugin_tabs_sit_between_reset_planning_and_titles_in_the_rail() {
    let mut session = FakeSession::new();
    session.install_package_dir(manifest_path().parent().expect("package dir")).expect("install");
    let order = session.settings_tab_order();
    let position = |id: &str| order.iter().position(|tab| tab == id).unwrap_or_else(|| panic!("{id} in {order:?}"));
    assert_eq!(position("inference"), position("reset-planning") + 1);
    assert_eq!(position("llm-instructions"), position("inference") + 1);
    assert_eq!(position("titles"), position("llm-instructions") + 1);
}
