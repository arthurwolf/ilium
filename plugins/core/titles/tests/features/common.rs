//! Shared helpers of the titles feature tests.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_titles::TitlesServer;
use ilium_plugin_sdk::host::inference::Completion;
use ilium_plugin_sdk::testing::{manual, FakeHost};
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, ErrorCode, Event, InputSource,
    InputSubmittedEvent, Intent, NodeId, Outcome, PluginError, RenameNodeSpec, RequestId,
    RequestRef, RequestResult, RestructureSpec, SetInferredIconSpec, Submission, TitleSource,
};

pub const PLUGIN_ID: &str = "ilium.core.titles";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> serde_json::Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn page_text() -> String {
    std::fs::read_to_string(manual::pages_dir().join("core-plugin-titles.md"))
        .expect("page is readable")
}

pub fn pages_text(name: &str) -> String {
    std::fs::read_to_string(manual::pages_dir().join(name)).expect("page is readable")
}

/// The user documentation of the existing implementation (src/docs), the oracle for kept behaviour.
pub fn user_doc(name: &str) -> String {
    let path = crate_dir().join("../../../src/docs").join(name);
    std::fs::read_to_string(path).expect("user doc is readable")
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else {
            break;
        };
        blocks.push(after[..end].to_string());
        rest = &after[end + 4..];
    }
    blocks
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else {
            break;
        };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the first table row of a page that starts with `prefix`.
pub fn table_row(page: &str, prefix: &str) -> Vec<String> {
    let row = page
        .lines()
        .find(|line| line.starts_with(prefix))
        .expect("table row exists");
    row.split('|').map(|cell| cell.trim().to_string()).collect()
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<TitlesServer>();
    host
}

pub fn prompt_event(node: u64, text: &str) -> Event {
    Event::InputSubmitted(InputSubmittedEvent {
        node: NodeId(node),
        text: text.to_string(),
        source: InputSource::User,
        epoch: 1,
    })
}

pub fn command_event(name: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: name.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext::default(),
    })
}

pub fn completion(text: &str) -> Completion {
    Completion {
        text: text.to_string(),
        model: "test-model".to_string(),
        input_tokens: 10,
        output_tokens: 4,
    }
}

pub fn plugin_error(code: ErrorCode) -> PluginError {
    PluginError {
        code,
        message: "test".to_string(),
        retry_after_ms: None,
    }
}

/// Submits a prompt for `node`, answers the one inference request with `reply` and settles.
pub fn title_for_prompt(host: &mut FakeHost, node: u64, prompt: &str, reply: &str) {
    host.deliver_one(prompt_event(node, prompt));
    host.settle();
    let requests = host.inference_requests();
    let (id, _) = requests
        .last()
        .cloned()
        .expect("an inference request was made");
    host.complete_inference(id, completion(reply));
    host.settle();
}

pub fn renames(submissions: &[Submission]) -> Vec<RenameNodeSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::RenameNode(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn icons(submissions: &[Submission]) -> Vec<SetInferredIconSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetInferredIcon(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn restructures(submissions: &[Submission]) -> Vec<RestructureSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Restructure(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

pub fn reverts(submissions: &[Submission]) -> Vec<u64> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::RevertRestructure(token) => Some(*token),
            _ => None,
        })
        .collect()
}

pub fn outcome_event(correlation: u64, outcome: Outcome) -> Event {
    Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(correlation),
        outcome,
    })
}

pub fn call_event(id: RequestId, outcome: Outcome) -> Event {
    Event::RequestCompleted(RequestResult {
        request: RequestRef::Call(id),
        outcome,
    })
}

pub fn inferred() -> TitleSource {
    TitleSource::Inferred
}
