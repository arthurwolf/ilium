//! Shared helpers of the recap feature tests.
#![allow(dead_code)]

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_recap::{RecapClient, RecapServer};
use ilium_plugin_sdk::testing::{manual, FakeHost, TreeNode};
use ilium_plugin_sdk::types::{AgentActivity, AgentView, NodeClass};
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.recap";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

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
        let Some(end) = after.find("\n```") else { break };
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
        let Some(end) = after.find("</pre>") else { break };
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
        let text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the first table row of `page` that starts with `prefix`.
pub fn table_row(page: &str, prefix: &str) -> Vec<String> {
    let row = page.lines().find(|line| line.starts_with(prefix)).expect("table row");
    row.trim().trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect()
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server")
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client")
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<RecapServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<RecapClient>();
    host
}

/// An agent pane node (`signature` such as "claude").
pub fn agent_pane(name: &str, signature: &str) -> TreeNode {
    TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity: AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    }
}

/// A plain shell pane node.
pub fn shell_pane(name: &str) -> TreeNode {
    TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    }
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
    (invocation.stdout(), invocation.stderr(), invocation.exit_code())
}

/// Every non-empty stdout line parsed as one JSON object.
pub fn jsonl(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every stdout line is one JSON object"))
        .collect()
}

pub fn of_type<'a>(all: &'a [Value], kind: &str) -> Vec<&'a Value> {
    all.iter().filter(|record| record["type"] == kind).collect()
}

pub fn summary(all: &[Value]) -> Value {
    let found = of_type(all, "summary");
    assert_eq!(found.len(), 1, "one summary record expected in {all:?}");
    found[0].clone()
}

/// Closes the consent gate: automatic AI is off in the kernel `inference` tab.
pub fn close_gate(host: &mut FakeHost) {
    host.kernel_settings().set_tab("inference", serde_json::json!({"automatic_ai": false}));
}

/// Opens the consent gate: automatic AI is on and the client answers `onboarding-state` finished.
pub fn open_gate(host: &mut FakeHost) {
    host.kernel_settings().set_tab("inference", serde_json::json!({"automatic_ai": true}));
    host.set_onboarding_finished(true);
}
