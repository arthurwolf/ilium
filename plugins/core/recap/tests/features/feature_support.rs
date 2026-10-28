//! Helpers used only by the feature tests: manifest and manual-page readers, the client side
//! host, the editor pane and CLI runner, and the host hooks that only the feature tests call.
//! Mounted by the features target alone, so the port target never compiles them.

use ilium_plugin_core_recap::RecapClient;
use ilium_plugin_sdk::testing::{manual, FakeHost, TreeNode};
use ilium_plugin_sdk::types::{NodeClass, RequestRef};
use serde_json::Value;

use crate::common::manifest_path;

pub const PLUGIN_ID: &str = "ilium.core.recap";

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn pages_text(file: &str) -> String {
    std::fs::read_to_string(manual::pages_dir().join(file)).expect("page is readable")
}

pub fn page_text() -> String {
    pages_text("core-plugin-recap.md")
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

/// The cells of the first table row of `page` that starts with `prefix`.
pub fn table_row(page: &str, prefix: &str) -> Vec<String> {
    let row = page
        .lines()
        .find(|line| line.starts_with(prefix))
        .expect("table row");
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<RecapClient>();
    host
}

/// An editor pane node (never recapped).
pub fn editor_pane(name: &str) -> TreeNode {
    TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.editor".to_string()),
        agent: None,
    }
}

/// Runs `ilium <args>` (args start with the command name) and returns (stdout, stderr, exit code).
pub fn run_cli(host: &mut FakeHost, args: &[&str]) -> (String, String, Option<u8>) {
    let invocation = host.cli().invoke(args, b"");
    (
        invocation.stdout(),
        invocation.stderr(),
        invocation.exit_code(),
    )
}

/// One `read_screen` the plugin made (API_GAPS.md: F-112 reads the visible part, 40 rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRequest {
    /// Pane whose screen was read.
    pub pane: u64,
    /// Rows requested.
    pub rows: u16,
    /// Whether only the visible part was requested.
    pub visible_only: bool,
}

/// Host hooks only the feature tests use. Implemented for `FakeHost`.
pub trait RecapFeatureHostExt {
    /// Answers the next client request of `kind` with `reply_json`; `None` when none is pending.
    fn answer_client_request(&mut self, kind: &str, reply_json: &str) -> Option<String>;
    /// Leaves every client request unanswered so timeouts apply.
    fn leave_client_requests_unanswered(&mut self);
    /// Cancels the job currently running.
    fn cancel_current_job(&mut self);
    /// Scripts the text of the next inference answer.
    fn set_inference_answer(&mut self, answer: String);
    /// Wait budgets (ms) of the inference calls made.
    fn inference_wait_ms(&mut self) -> Vec<u64>;
    /// Progress values the job reported.
    fn reported_progress(&mut self) -> Vec<f64>;
    /// Screen read requests, in order.
    fn screen_requests(&mut self) -> Vec<ScreenRequest>;
}

impl RecapFeatureHostExt for FakeHost {
    fn answer_client_request(&mut self, kind: &str, reply_json: &str) -> Option<String> {
        let answered: Vec<_> = self
            .client_requests()
            .responses()
            .into_iter()
            .map(|response| response.call)
            .collect();
        let pending = self.client_requests().sent().into_iter().find(|sent| {
            let call = match &sent.request {
                RequestRef::Submission(correlation) => *correlation,
                RequestRef::Call(id) => id.0,
            };
            sent.kind == kind && !answered.contains(&call)
        })?;
        self.client_requests()
            .answer(pending.request, Ok(reply_json.to_string()));
        self.settle();
        Some(reply_json.to_string())
    }

    /// The fake client never answers on its own; this makes the test fail if an answer exists already.
    fn leave_client_requests_unanswered(&mut self) {
        let answers = self.client_requests().responses().len();
        assert_eq!(answers, 0, "a client request was answered although the test left them open");
    }

    fn cancel_current_job(&mut self) {
        self.jobs().request_cancel();
    }

    fn set_inference_answer(&mut self, answer: String) {
        self.inference().reply_text(&answer);
    }

    /// The request record carries no wait budget; the plugin reports the budgets it passed.
    fn inference_wait_ms(&mut self) -> Vec<u64> {
        crate::host_ext::harness_call(self, "reads", serde_json::json!({}))["inference_wait_ms"]
            .as_array()
            .map(|waits| waits.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default()
    }

    fn reported_progress(&mut self) -> Vec<f64> {
        self.jobs()
            .reported_progress()
            .into_iter()
            .map(f64::from)
            .collect()
    }

    fn screen_requests(&mut self) -> Vec<ScreenRequest> {
        crate::host_ext::harness_call(self, "reads", serde_json::json!({}))["screen_requests"]
            .as_array()
            .map(|requests| {
                requests
                    .iter()
                    .map(|request| ScreenRequest {
                        pane: request["pane"].as_u64().unwrap_or_default(),
                        rows: u16::try_from(request["rows"].as_u64().unwrap_or_default())
                            .unwrap_or(u16::MAX),
                        visible_only: request["visible_only"].as_bool().unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
