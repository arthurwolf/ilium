//! Shared scenario helpers of the port twins. Setting keys beyond the page's `patterns` and `replies` are the
//! ones assumed in API_GAPS.md: `names`, `enabled`, `delays` (seconds, default 60, 0..=86400), `targets`
//! (`agents`, `terminals`, `both`), `ids` (authored stable ids).

use ilium_plugin_core_text_triggers::TextTriggersServer;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{Event, NodeClass, Outcome, Submission};
use serde_json::{Value, json};

use crate::common::*;

pub const REPLY: &str = "reply";
/// The host dedup window of the manual (10 minutes) in milliseconds.
pub const DEDUP_WINDOW_MS: u64 = 600_000;
/// The default delay between detection and send, in seconds (the original DEFAULT_TEXT_TRIGGER_DELAY_SECONDS).
pub const DEFAULT_DELAY_SECONDS: u64 = 60;
/// The longest accepted delay (24 hours, the original MAX_TEXT_TRIGGER_DELAY_SECONDS).
pub const MAX_DELAY_SECONDS: u64 = 86_400;

/// Server host with one agent pane and one rule (`pattern` -> `reply`) whose delay is 0.
pub fn armed(pattern: &str, reply: &str) -> (FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &[pattern], &[reply]);
    host.settings().set("delays", json!([0])).expect("delays accepted");
    host.settle();
    (host, pane)
}

/// Adds a plain terminal pane (no agent) to the fake tree.
pub fn add_plain_pane(host: &mut FakeHost) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "shell".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// Rule 0 matched on `row` of `pane`.
pub fn hit(pane: u64, row: u32) -> Event {
    matched(pane, &rule_id(0), "line", row)
}

/// Texts of the send-input submissions, in order.
pub fn texts(submissions: &[Submission]) -> Vec<String> {
    send_inputs(submissions).into_iter().map(|spec| spec.text).collect()
}

/// Number of send-inputs the kernel applied (not deduplicated, not refused) after settling.
pub fn applied(host: &mut FakeHost) -> usize {
    host.settle();
    host.outcomes().iter().filter(|o| !matches!(o, Outcome::Duplicate(_) | Outcome::Refused(_) | Outcome::Failed(_) | Outcome::Cancelled)).count()
}

/// Number of send-inputs the kernel dropped as duplicates after settling.
pub fn duplicates(host: &mut FakeHost) -> usize {
    host.settle();
    host.outcomes().iter().filter(|o| matches!(o, Outcome::Duplicate(_))).count()
}

/// Delivers one event per entry of `rows` for rule 0 of `pane` and returns (applied, duplicates).
pub fn deliver_rows(host: &mut FakeHost, pane: u64, rows: &[u32]) -> (usize, usize) {
    for row in rows {
        host.deliver_one(hit(pane, *row));
    }
    (applied(host), duplicates(host))
}

/// Filter ids installed on `pane`, sorted.
pub fn filter_ids(host: &FakeHost, pane: u64) -> Vec<String> {
    let mut ids: Vec<String> = host.output_filters().into_iter().filter(|(node, _)| *node == pane).map(|(_, f)| f.id).collect();
    ids.sort();
    ids
}

/// Regexes installed on `pane`, in filter id order.
pub fn filter_regexes(host: &FakeHost, pane: u64) -> Vec<String> {
    let mut all: Vec<(String, String)> =
        host.output_filters().into_iter().filter(|(node, _)| *node == pane).map(|(_, f)| (f.id, f.regex)).collect();
    all.sort();
    all.into_iter().map(|(_, regex)| regex).collect()
}

/// A server host built from a seeded `[text_triggers]` table before the plugin activates.
pub fn host_with_table(table: Value) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.seed_config_table("text_triggers", table);
    host.activate::<TextTriggersServer>();
    host
}

/// `count` simple patterns and replies.
pub fn many_patterns(count: usize) -> (Vec<String>, Vec<String>) {
    ((0..count).map(|i| format!("p{i}")).collect(), (0..count).map(|i| format!("r{i}")).collect())
}

/// The error code of a refused settings write.
pub fn settings_error(host: &mut FakeHost, key: &str, value: Value) -> ilium_plugin_sdk::types::ErrorCode {
    host.settings().set(key, value).expect_err("the write is refused").code
}

/// The Text Triggers tab (assumed view target, see API_GAPS.md).
pub fn tab() -> ilium_plugin_sdk::types::ViewTarget {
    ilium_plugin_sdk::types::ViewTarget::SettingsPage("text-triggers".to_string())
}

/// The trigger events tab (assumed view target, see API_GAPS.md).
pub fn triggers_tab() -> ilium_plugin_sdk::types::ViewTarget {
    ilium_plugin_sdk::types::ViewTarget::SettingsPage("trigger-events".to_string())
}

pub fn render_tab(host: &mut FakeHost, columns: u16) -> String {
    host.render(tab(), columns, 30).text()
}

pub fn render_triggers(host: &mut FakeHost, columns: u16) -> String {
    host.render(triggers_tab(), columns, 60).text()
}

/// Calls the `test-rule` service (assumed request and reply shapes, see API_GAPS.md).
pub fn preview_result(
    host: &mut FakeHost,
    pattern: &str,
    reply: &str,
    sample: &str,
) -> Result<Value, ilium_plugin_sdk::types::PluginError> {
    let request = json!({ "pattern": pattern, "reply": reply, "sample": sample }).to_string();
    host.call_service("ilium.core.text-triggers/test-rule", &request)
        .map(|text| serde_json::from_str(&text).expect("the service answers JSON"))
}

pub fn preview(host: &mut FakeHost, pattern: &str, reply: &str, sample: &str) -> Value {
    preview_result(host, pattern, reply, sample).expect("the service answers")
}
