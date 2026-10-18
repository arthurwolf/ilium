//! The extension points, service and topic the page declares: `schedule-sources` (collect),
//! `before-send-scheduled` (hook, veto), service `ilium.core.scheduler/queue`, topic `ilium.core.scheduler/fired`.

use crate::common::*;
use crate::feature_support::*;
use ilium_plugin_sdk::exports::{HookCall, HookReply};
use ilium_plugin_sdk::testing::FakeSession;

/// Rows of the Extension points table: `(point, kind, what, payload, risk)`.
fn point_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page
        .find("| Point | Kind |")
        .expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            [
                cells[0].clone(),
                cells[1].clone(),
                cells[2].clone(),
                cells[3].clone(),
                cells[4].clone(),
            ]
        })
        .collect()
}

/// REQ: F-055, F-057
#[test]
fn points_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["schedule-sources", "before-send-scheduled"]);
}

/// REQ: F-055
#[test]
fn points_schedule_sources_is_a_collect_point_with_entry_id_title_service() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`schedule-sources`")
        .expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Add a trigger source such as a calendar or a reset forecast."
    );
    assert_eq!(
        row[3],
        "entry: id, title, service (returns the next fire times)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-055, F-057
#[test]
fn points_before_send_scheduled_is_a_high_risk_veto_hook() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`before-send-scheduled`")
        .expect("row");
    assert_eq!(row[1], "hook (veto)");
    assert_eq!(
        row[2],
        "Refuse a scheduled or queued input at the moment it would be sent."
    );
    assert_eq!(row[3], "pane, text_length, schedule, source");
    assert_eq!(row[4], "high");
}

/// REQ: F-055, F-057
#[test]
fn points_the_reference_page_agrees_with_the_plugin_page() {
    let reference = manual_page("reference-core-extension-points.md");
    assert!(reference.contains("| `ilium.core.scheduler/schedule-sources` | collect | standard | Add a trigger source such as a calendar or a reset forecast. |"));
    assert!(reference.contains("| `ilium.core.scheduler/before-send-scheduled` | hook: veto | high | Refuse a scheduled or queued input at the moment it would be sent. |"));
    assert!(reference.contains(
        "| [Scheduler](/pages/core-plugin-scheduler/) | `ilium.core.scheduler` | 2 | 1 | 1 | yes |"
    ));
}

/// REQ: F-057
#[test]
fn points_the_service_is_queue_with_enqueue_list_cancel() {
    assert!(page_text()
        .contains("`ilium.core.scheduler/queue`: enqueue, list, cancel: manage the prompt queue"));
    assert!(manual_page("reference-core-extension-points.md").contains(
        "| `ilium.core.scheduler/queue` | enqueue, list, cancel: manage the prompt queue |"
    ));
}

/// REQ: F-055
#[test]
fn points_the_topic_fired_carries_schedule_and_pane() {
    assert!(page_text().contains("`ilium.core.scheduler/fired`: { schedule, pane }"));
    assert!(manual_page("reference-core-extension-points.md")
        .contains("| `ilium.core.scheduler/fired` | { schedule, pane } |"));
}

/// REQ: F-055, F-057
#[test]
fn points_the_core_plugin_is_replaceable_with_a_replaces_list() {
    assert!(page_text().contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.scheduler\"]`"
    ));
}

/// REQ: F-055, F-057
#[test]
fn points_the_example_package_vetoes_with_a_handler() {
    let example = json_block_after(
        &page_text(),
        "#### Example: Refuses scheduled prompts on days",
    );
    assert_eq!(example["exports"], serde_json::json!(["hook-handler"]));
    assert_eq!(
        example["capabilities"]["server"],
        serde_json::json!(["extensions.contribute:ilium.core.scheduler/before-send-scheduled"])
    );
    let contribution = &example["contributes"]["extensions"][0];
    assert_eq!(
        contribution["point"],
        "ilium.core.scheduler/before-send-scheduled"
    );
    assert_eq!(contribution["id"], "skip-on-holiday");
    assert_eq!(contribution["priority"], 50);
    assert_eq!(contribution["handler"], "skip_on_holiday");
}

/// REQ: F-055, F-057
#[test]
fn points_a_late_trapping_or_wrong_mode_handler_counts_as_pass() {
    assert!(page_text().contains("A handler that is late, traps or returns a reply the mode does not allow counts as `pass`."));
}

/// REQ: F-055, F-057
#[test]
fn points_the_stub_hook_reply_for_before_send_scheduled_is_pass() {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    let reply = session
        .run_hook(
            PLUGIN,
            HookCall {
                call: 1,
                point: "ilium.core.scheduler/before-send-scheduled".to_string(),
                payload_json:
                    r#"{"pane":1,"text_length":8,"schedule":"send:1","source":"scheduled"}"#
                        .to_string(),
            },
        )
        .expect("hook runs");
    assert_eq!(reply, HookReply::Pass);
}

/// REQ: F-057
#[test]
fn points_the_queue_service_answers_list_with_an_entries_array() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "list", "pane": pane.0}),
    )
    .expect("list answers");
    assert!(reply["entries"].is_array());
}

/// REQ: F-057
#[test]
fn points_the_queue_service_enqueue_adds_an_entry_and_list_shows_it() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let reply = enqueue(&mut session, pane, "hello", "once", 1).expect("enqueue answers");
    assert_eq!(reply["ok"], true);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "hello");
}

/// REQ: F-057
#[test]
fn points_the_queue_service_cancel_removes_one_entry() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "a", "once", 1).expect("a");
    enqueue(&mut session, pane, "b", "once", 1).expect("b");
    let entries = list_queue(&mut session, pane);
    let id = entries[0]["id"].clone();
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "cancel", "pane": pane.0, "id": id}),
    )
    .expect("cancel answers");
    assert_eq!(reply["ok"], true);
    let left = list_queue(&mut session, pane);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["text"], "b");
}

/// REQ: F-057
#[test]
fn points_the_queue_service_cancel_of_an_unknown_entry_is_not_found() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let error = call_queue(
        &mut session,
        serde_json::json!({"method": "cancel", "pane": pane.0, "id": "nope"}),
    )
    .expect_err("unknown id");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}

/// REQ: F-057
#[test]
fn points_the_queue_service_refuses_an_unknown_method() {
    let mut session = session();
    let error = call_queue(&mut session, serde_json::json!({"method": "explode"}))
        .expect_err("unknown method");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-057
#[test]
fn points_the_queue_service_refuses_an_unknown_mode() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let error = enqueue(&mut session, pane, "a", "sometimes", 1).expect_err("unknown mode");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

// Tests that need harness items the skeleton lacks (see API_GAPS.md). They stay compiled out
// (`cfg(any())` is always false) until the items are added and the gate is replaced.
#[cfg(any())]
mod needs_harness_items {
    use super::*;
    use serde_json::Value;

    /// REQ: F-055
    #[test]
    fn points_a_scheduled_send_publishes_the_fired_topic_with_schedule_and_pane() {
        let mut session = session();
        server(&mut session).set_clock(1_000_000_000);
        let pane = terminal_pane(&mut session, "claude-api");
        schedule(&mut session, pane, 0, 0, 5, "x", true).expect("schedule");
        server(&mut session).advance_clock(5_000);
        let topics = server(&mut session).published_topics();
        let (topic, payload) = topics.last().expect("one publication").clone();
        assert_eq!(topic, "ilium.core.scheduler/fired");
        let payload: Value = serde_json::from_str(&payload).expect("JSON payload");
        assert_eq!(payload["pane"], pane.0);
        assert_eq!(payload["schedule"], format!("send:{}", pane.0));
    }

    /// REQ: F-055, F-057
    #[test]
    fn points_a_veto_from_a_contributor_stops_the_send() {
        let mut session = session();
        server(&mut session).set_clock(1_000_000_000);
        let pane = terminal_pane(&mut session, "claude-api");
        session.contribute_hook("ilium.core.scheduler/before-send-scheduled", |_call| {
            HookReply::Refuse("holiday".into())
        });
        schedule(&mut session, pane, 0, 0, 5, "x", true).expect("schedule");
        server(&mut session).advance_clock(5_000);
        assert!(sends(&server(&mut session).submitted()).is_empty());
    }

    /// REQ: F-055
    #[test]
    fn points_a_veto_receives_pane_text_length_schedule_and_source() {
        let mut session = session();
        server(&mut session).set_clock(1_000_000_000);
        let pane = terminal_pane(&mut session, "claude-api");
        schedule(&mut session, pane, 0, 0, 5, "npm test", true).expect("schedule");
        server(&mut session).advance_clock(5_000);
        let calls = session.hook_calls("ilium.core.scheduler/before-send-scheduled");
        let payload: Value = serde_json::from_str(&calls[0].payload_json).expect("JSON");
        assert_eq!(payload["pane"], pane.0);
        assert_eq!(payload["text_length"], 8);
    }
}
