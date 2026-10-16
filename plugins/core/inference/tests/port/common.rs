//! Shared helpers of the port twins.

use std::path::PathBuf;

use ilium_plugin_core_inference::{InferenceClient, InferenceServer};
use ilium_plugin_sdk::host::HttpResponse;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, PluginSide, SettingsChange,
    SettingsOrigin,
};
use serde_json::Value;

/// Command that refreshes the model list (the manual names no id; see API_GAPS.md).
pub const LOAD_MODELS: &str = "ilium.core.inference/load-models";
pub const OPENAI_MODELS_URL: &str = "https://api.openai.com/v1/models";
pub const OLLAMA_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";
pub const KILO_MODELS_URL: &str = "https://api.kilo.ai/api/gateway/models";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

fn builder(side: &str, settings: Value) -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side(side)
        .with_settings(settings)
        .build()
}

pub fn server_host(settings: Value) -> FakeHost {
    let mut host = builder("server", settings);
    host.activate::<InferenceServer>();
    host
}

pub fn client_host(settings: Value) -> FakeHost {
    let mut host = builder("client", settings);
    host.activate::<InferenceClient>();
    host
}

pub fn openai_settings(model: &str) -> Value {
    serde_json::json!({
        "selected_provider": "open_ai",
        "openai": { "base_url": "https://api.openai.com/v1", "api_key": "fixture-key", "model": model }
    })
}

pub fn ok(body: Value) -> HttpResponse {
    HttpResponse { status: 200, headers: vec![], body: body.to_string().into_bytes() }
}

pub fn status(code: u16) -> HttpResponse {
    HttpResponse { status: code, headers: vec![], body: b"{}".to_vec() }
}

pub fn row_command(command: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::RowAction,
        context: CommandContext::default(),
    })
}

/// The `settings-changed` event for the given keys, written by the user on the server side.
pub fn settings_changed(keys: &[&str]) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| key.to_string()).collect(),
        origin: SettingsOrigin::User,
        side: PluginSide::Server,
    })
}

/// Model names the plugin offers right now.
pub fn offered(host: &mut FakeHost) -> Vec<String> {
    use ilium_plugin_sdk::InferenceProvider;
    host.instance_mut::<InferenceServer>().models()
}
