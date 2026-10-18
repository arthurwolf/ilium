//! Shared helpers of the recap feature tests.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_recap::RecapServer;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{AgentActivity, AgentView, NodeClass};
use serde_json::Value;

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<RecapServer>();
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
    host.kernel_settings()
        .set_tab("inference", serde_json::json!({"automatic_ai": false}));
}

/// Opens the consent gate: automatic AI is on and the client answers `onboarding-state` finished.
pub fn open_gate(host: &mut FakeHost) {
    host.kernel_settings()
        .set_tab("inference", serde_json::json!({"automatic_ai": true}));
    host.set_onboarding_finished(true);
}
