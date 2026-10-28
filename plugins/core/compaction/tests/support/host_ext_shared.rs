//! Compaction-lane hooks shared by the port and feature test targets.
//!
//! Mounted by both targets, so only methods that both targets call live here. Inputs the test sets go to the
//! plugin's own storage (`compaction.*` keys, the mailbox the `&self` readers use too); observations come from
//! what the plugin itself publishes or submits: the topic `ilium.core.compaction/scan-started` (one payload per
//! scan), `close-view` intents, and the writes it makes to the fake `agent-homes` file scope.

use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, Event, Intent, UiAction, ViewTarget, ViewVisibilityEvent};
use serde_json::Value;

/// Topic the plugin publishes once per scan it starts.
const SCAN_STARTED_TOPIC: &str = "ilium.core.compaction/scan-started";
/// Storage key holding the recommendation the optimiser panel shows (`{"tokens":..,"alternative":..}`).
pub const RECOMMENDATION_KEY: &str = "compaction.recommendation";

pub trait CompactionSharedHostExt {
    fn scans_started(&self) -> Vec<Value>;
    fn recommended_tokens(&self) -> u64;
    fn select_agent_target(&mut self, target: &str);
    fn agent_file_writes(&mut self) -> Vec<String>;
    fn open_view(&mut self, target: ViewTarget);
    fn view_closed(&self, target: ViewTarget) -> bool;
}

impl CompactionSharedHostExt for FakeHost {
    /// One payload per `scan-started` topic publication, in order.
    fn scans_started(&self) -> Vec<Value> {
        self.published_topics()
            .into_iter()
            .filter(|(topic, _)| topic == SCAN_STARTED_TOPIC)
            .map(|(_, payload)| serde_json::from_str(&payload).unwrap_or(Value::String(payload)))
            .collect()
    }

    /// The recommended token threshold the test seeded with `set_recommendation`.
    fn recommended_tokens(&self) -> u64 {
        let bytes = self
            .storage()
            .get(Bucket::Global, RECOMMENDATION_KEY)
            .expect("set_recommendation seeds the recommendation before it is read");
        let value: Value = serde_json::from_slice(&bytes).expect("the recommendation is JSON");
        value["tokens"].as_u64().expect("the recommendation holds tokens")
    }

    /// Picks the agent target (`claude` or `codex`) on the optimiser panel.
    fn select_agent_target(&mut self, target: &str) {
        self.deliver_one(Event::UiAction(UiAction {
            view: ViewTarget::Panel("optimizer".to_string()),
            key: "agent-target".to_string(),
            action_id: target.to_string(),
            kind: ActionKind::Activated,
            value: None,
        }));
        self.settle();
    }

    /// Paths the plugin wrote in the fake `agent-homes` scope, in order.
    fn agent_file_writes(&mut self) -> Vec<String> {
        self.fs()
            .write_log()
            .into_iter()
            .map(|call| call.path)
            .filter(|path| path.starts_with("agent-homes:"))
            .collect()
    }

    /// The user opens the view `target`.
    fn open_view(&mut self, target: ViewTarget) {
        self.deliver_one(Event::ViewVisibility(ViewVisibilityEvent {
            view: target,
            visible: true,
        }));
        self.settle();
    }

    /// Whether the plugin asked the host to close `target`.
    fn view_closed(&self, target: ViewTarget) -> bool {
        self.submitted()
            .iter()
            .any(|submission| matches!(&submission.intent, Intent::CloseView(closed) if *closed == target))
    }
}
