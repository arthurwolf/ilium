//! Shared helpers: hosts for both sides, page parsing, request builders and event builders.
//!
//! Every call into a fake that the skeleton testkit lacks (plugin instance access, inference,
//! streams, topics, extensions, info rows) is made through the names listed in API_GAPS.md.

use std::path::PathBuf;

use ilium_plugin_core_inference::{InferenceClient, InferenceServer};
use ilium_plugin_sdk::host::{ChatMessage, Completion, CompletionRequest};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, ErrorCode, Event, PluginError,
};
use serde_json::Value;

/// The plugin id of the page.
pub const PLUGIN_ID: &str = "ilium.core.inference";

/// The command id the Run test button invokes (page sample 4 matches `ends_with("/test-provider")`).
pub const TEST_COMMAND: &str = "ilium.core.inference/test-provider";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/inference).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A page of the manual by file name.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    manual_page("core-plugin-inference.md")
}

/// Path of a golden file of this directory.
pub fn golden_path(name: &str) -> String {
    workspace_root()
        .join("plugins/core/inference/tests/features/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// The manifest as JSON.
pub fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading line `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// The text of the section that starts at the heading line `heading` and ends at the next heading of the same or a higher level.
pub fn section(page: &str, heading: &str) -> String {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let start = page
        .find(&format!("{heading}\n"))
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start + heading.len()..];
    let mut end = rest.len();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if hashes > 0 && hashes <= level && line.chars().nth(hashes) == Some(' ') {
            end = offset;
            break;
        }
        offset += line.len();
    }
    rest[..end].to_string()
}

/// The rows of the first markdown table in `text`, header and separator removed, cells trimmed and backticks removed.
pub fn table_rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().replace('`', ""))
                .collect()
        })
        .collect()
}

/// A server-side host with the manifest grants, not yet activated.
pub fn server_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build()
}

/// A client-side host with the manifest grants, not yet activated.
pub fn client_host_unactivated() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build()
}

/// A server-side host with the plugin activated and the given plugin settings.
pub fn server_host_with(settings: Value) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .with_settings(settings)
        .build();
    host.activate::<InferenceServer>();
    host
}

/// Settings that select the OpenAI-compatible provider with a key and the default endpoint.
pub fn openai_settings() -> Value {
    serde_json::json!({
        "selected_provider": "open_ai",
        "openai": { "base_url": "https://api.openai.com/v1", "api_key": "sk-test-key", "model": "gpt-5.6-terra" }
    })
}

/// Settings that select the local Ollama provider with the default endpoint.
pub fn ollama_settings() -> Value {
    serde_json::json!({
        "selected_provider": "ollama",
        "ollama": { "base_url": "http://127.0.0.1:11434", "model": "llama3" }
    })
}

/// A server-side host with the plugin activated on the OpenAI-compatible provider.
pub fn server_host() -> FakeHost {
    server_host_with(openai_settings())
}

/// A client-side host with the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = client_host_unactivated();
    host.activate::<InferenceClient>();
    host
}

/// A one-message request with the given purpose.
pub fn request(purpose: &str, text: &str) -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: text.to_string(),
        }],
        max_output_tokens: None,
        json_schema: None,
        purpose: purpose.to_string(),
    }
}

/// A completion of the model `model`.
pub fn completion(text: &str, model: &str) -> Completion {
    Completion {
        text: text.to_string(),
        model: model.to_string(),
        input_tokens: 3,
        output_tokens: 1,
    }
}

/// A plugin error of the given code with an empty message.
pub fn error(code: ErrorCode) -> PluginError {
    PluginError {
        code,
        message: String::new(),
        retry_after_ms: None,
    }
}

/// A command invocation from a settings row.
pub fn row_command(command: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::RowAction,
        context: CommandContext::default(),
    })
}

/// An OpenAI-style streaming payload carrying `delta`.
pub fn sse_delta(delta: &str) -> Vec<u8> {
    serde_json::json!({"choices": [{"delta": {"content": delta}}]})
        .to_string()
        .into_bytes()
}

/// A non-streaming OpenAI-style response body carrying `text` for `model`.
pub fn openai_body(text: &str, model: &str, input_tokens: u32, output_tokens: u32) -> Vec<u8> {
    serde_json::json!({
        "model": model,
        "choices": [{"message": {"role": "assistant", "content": text}}],
        "usage": {"prompt_tokens": input_tokens, "completion_tokens": output_tokens}
    })
    .to_string()
    .into_bytes()
}
