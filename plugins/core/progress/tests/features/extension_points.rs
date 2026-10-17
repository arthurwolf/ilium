//! Extension points, the service and the topic (F-060). The manifest declares none of them (see
//! ../API_GAPS.md), so the behaviour is tested through gap helpers and the page text.

use serde_json::json;

use crate::common::*;

/// REQ: F-060
#[test]
fn doc_f_060_the_page_lists_the_two_extension_points_with_kind_and_entry_fields() {
    let text = page_text();
    assert!(text.contains("| `footer-segments` | collect | Add a segment to the progress footer. | entry: id, priority, max_width | standard |"));
    assert!(text.contains("`probe-kinds`"));
    assert!(text.contains("entry: id, title, command_template, interval_seconds"));
}

/// REQ: F-060
#[test]
fn doc_f_060_the_page_names_the_report_service_and_the_changed_topic() {
    let text = page_text();
    assert!(text.contains("`ilium.core.progress/report`: start, update, finish"));
    assert!(text.contains("`ilium.core.progress/changed`: { monitor, status, percent }"));
}

/// REQ: F-060
#[test]
fn doc_f_060_the_plugin_is_replaceable_with_a_replaces_entry() {
    let text = page_text();
    assert!(text.contains("**Replaceable:** yes."));
    assert!(text.contains("\"replaces\": [\"ilium.core.progress\"]"));
}

/// REQ: F-060
#[test]
fn doc_f_060_the_example_probe_kind_is_valid_json_with_the_documented_entry() {
    let example = json_block_after(&page_text(), "Example: Adds a probe template for a CI run");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.progress/probe-kinds");
    assert_eq!(entry["id"], "ci-run");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["interval_seconds"], 30);
    assert_eq!(
        entry["entry"]["command_template"],
        "/usr/local/bin/ci-probe {run}"
    );
}

/// REQ: F-060
#[test]
fn f_060_a_contributed_probe_kind_is_accepted_without_failures() {
    let mut host = activated_server();
    contribute(
        &mut host,
        "ilium.core.progress/probe-kinds",
        "ci-run",
        50,
        json!({
        "title": "CI run", "command_template": "/usr/local/bin/ci-probe {run}", "interval_seconds": 30}),
    );
    assert!(host.failures().is_empty());
    let entries = host.extension_entries("ilium.core.progress/probe-kinds");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "ci-run");
    assert_eq!(entries[0].priority, 50);
    assert_eq!(entries[0].entry["interval_seconds"], 30);
}

/// REQ: F-060
#[test]
fn f_060_a_contributed_footer_segment_is_accepted_on_the_client_without_failures() {
    let mut host = activated_client();
    contribute(
        &mut host,
        "ilium.core.progress/footer-segments",
        "eta",
        10,
        json!({"max_width": 12}),
    );
    assert!(host.failures().is_empty());
    // The page lists the entry fields id, priority, max_width; the accepted entry reads back unchanged.
    let entries = host.extension_entries("ilium.core.progress/footer-segments");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "eta");
    assert_eq!(entries[0].priority, 10);
    assert_eq!(entries[0].entry["max_width"], 12);
}

/// REQ: F-060
#[test]
fn f_060_the_report_service_start_registers_a_monitor_and_finish_settles_it() {
    let (mut host, pane) = host_with_pane();
    let started = call_report(
        &mut host,
        &json!({"op": "start", "pane": pane, "job_id": "svc-job", "message": "Starting"})
            .to_string(),
    );
    assert!(started.is_ok());
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
    let finished = call_report(
        &mut host,
        &json!({"op": "finish", "pane": pane, "job_id": "svc-job", "message": "Done"}).to_string(),
    );
    assert!(finished.is_ok());
    assert_eq!(status(&mut host, pane)["running"], 0);
}

/// REQ: F-060
#[test]
fn f_060_a_changed_monitor_publishes_the_changed_topic_with_monitor_status_and_percent() {
    let (mut host, pane) = host_with_pane();
    register(
        &mut host,
        pane,
        "/opt/probe.sh",
        &running("job", 10, "W"),
        &[],
    );
    expect_probe(&mut host, "/opt/probe.sh", &running("job", 55, "W"));
    tick(&mut host, 1);
    let topics = published_topics(&host);
    let (_, payload) = topics
        .iter()
        .rev()
        .find(|(topic, _)| topic == "ilium.core.progress/changed")
        .expect("topic published");
    let value: serde_json::Value = serde_json::from_str(payload).expect("json");
    assert!(value.get("monitor").is_some());
    assert_eq!(value["status"], "running");
    assert_eq!(value["percent"].as_f64(), Some(55.0));
}
