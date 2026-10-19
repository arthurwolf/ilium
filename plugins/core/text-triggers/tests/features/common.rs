//! Shared helpers: hosts for both sides, event builders, settings seeding and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_text_triggers::{TextTriggersClient, TextTriggersServer};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentView, Delivery, DroppedCount, Event, Intent, NodeClass, NodeId,
    OutputMatchedEvent, SendInputSpec, Submission,
};
use serde_json::json;

/// The filter rate the page sample uses (`rate_per_second: 2`).
pub const RATE_PER_SECOND: u16 = 2;
/// The list rows of the page sample hold at most this many items (`max_items: 32`).
pub const MAX_RULES: usize = 32;

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<TextTriggersServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<TextTriggersClient>();
    host
}

/// Adds a terminal pane that runs an agent to the fake tree.
pub fn add_agent_pane(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: "claude".to_string(),
            activity: AgentActivity::Working,
            objective: None,
            goal: None,
        }),
    })
}

/// Writes the pattern and reply lists (page sample keys `patterns` and `replies`).
pub fn set_rules(host: &mut FakeHost, patterns: &[&str], replies: &[&str]) {
    host.settings()
        .set("patterns", json!(patterns))
        .expect("patterns accepted");
    host.settings()
        .set("replies", json!(replies))
        .expect("replies accepted");
    host.settle();
}

/// The id of the output filter of rule `index` (the toggle key of the page sample is `rule-{index}`).
pub fn rule_id(index: usize) -> String {
    format!("rule-{index}")
}

/// An `output-matched` event as the host filter raises it.
pub fn matched(node: u64, filter_id: &str, line: &str, row: u32) -> Event {
    Event::OutputMatched(OutputMatchedEvent {
        node: NodeId(node),
        filter_id: filter_id.to_string(),
        line: line.to_string(),
        row,
    })
}

/// An `events-dropped` event with one `output-matched` record.
pub fn dropped(filter_id: &str, count: u32) -> Event {
    Event::EventsDropped(vec![DroppedCount {
        event: "output-matched".to_string(),
        source: Some(filter_id.to_string()),
        count,
    }])
}

/// The `send-input` specs among the submissions, in order.
pub fn send_inputs(submissions: &[Submission]) -> Vec<SendInputSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SendInput(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The exact `send-input` spec the page sample builds for a reply.
pub fn expected_spec(node: u64, reply: &str) -> SendInputSpec {
    SendInputSpec {
        pane: NodeId(node),
        text: reply.to_string(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
        stages: Vec::new(),
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: false,
    }
}

/// The dedup key the page sample builds: `trigger-{filter_id}-{node}-{row}`.
pub fn dedup_key(filter_id: &str, node: u64, row: u32) -> String {
    format!("trigger-{filter_id}-{node}-{row}")
}
