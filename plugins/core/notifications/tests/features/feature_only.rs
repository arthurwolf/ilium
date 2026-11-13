//! Helpers used only by the `features` test target (the `features_gaps` target mounts `common.rs`
//! by path and never needs these, so they live here to keep each target free of dead code).

use crate::common::*;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::*;
use serde_json::{json, Value};

/// The plugin id (manifest `id`).
pub const PLUGIN_ID: &str = "ilium.core.notifications";

/// Topic of the Progress plugin that reports the state of a monitored task
/// (reference-core-extension-points.md: `ilium.core.progress/changed`).
pub const PROGRESS_TOPIC: &str = "ilium.core.progress/changed";

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-notifications.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// Any page of the manual by file name.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// A user documentation page under src/docs.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// The manifest as JSON.
pub fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading text `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// The server grants of the manifest.
pub fn server_grants() -> Vec<String> {
    manifest()["capabilities"]["server"]
        .as_array()
        .expect("server grants")
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// The progress topic event of a monitored task.
///
/// Assumed payload (API_GAPS.md): `{ monitor, pane, job_id, status, percent }` with `status` one of
/// `running`, `done`, `error`, `lost`.
pub fn task_event(pane: NodeId, job_id: &str, status: &str) -> Event {
    let percent = if status == "running" { 50.0 } else { 100.0 };
    Event::TopicPublished(TopicPublishedEvent {
        topic: PROGRESS_TOPIC.to_string(),
        from: PluginId("ilium.core.progress".to_string()),
        payload_json: json!({
            "monitor": 1, "pane": pane.0, "job_id": job_id, "status": status, "percent": percent
        })
        .to_string(),
    })
}

/// Delivers one task outcome and returns the submissions.
pub fn task_outcome(
    host: &mut FakeHost,
    pane: NodeId,
    job_id: &str,
    status: &str,
) -> Vec<Submission> {
    let submissions = host.deliver_one(task_event(pane, job_id, status));
    host.settle();
    submissions
}

/// An `attention-changed` event.
pub fn attention_event(node: NodeId, marked: bool) -> Event {
    Event::AttentionChanged(AttentionEvent {
        node,
        marked,
        reason: if marked {
            Some("waiting for you".to_string())
        } else {
            None
        },
        by: Some(PluginId(PLUGIN_ID.to_string())),
    })
}

/// The desktop notices among the submissions.
pub fn notices(submissions: &[Submission]) -> Vec<NotifySpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Notify(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The sound ids among the submissions.
pub fn sounds(submissions: &[Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::PlaySound(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// The attention marks among the submissions.
pub fn marks(submissions: &[Submission]) -> Vec<MarkAttentionSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::MarkAttention(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The acknowledged nodes among the submissions.
pub fn acks(submissions: &[Submission]) -> Vec<NodeId> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::AckAttention(node) => Some(*node),
            _ => None,
        })
        .collect()
}

/// The dedup key rule of api-notifications.md: 1 to 128 characters from `A-Za-z0-9._:/-`.
pub fn is_valid_dedup_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().count() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
}

/// Rows of the first markdown table that starts with `header_prefix`: the cells of each body row.
pub fn table_rows(text: &str, header_prefix: &str) -> Vec<Vec<String>> {
    let start = text
        .find(header_prefix)
        .unwrap_or_else(|| panic!("table {header_prefix} missing"));
    text[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}
