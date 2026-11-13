//! The extension points, service and topic the page declares: `before-notify` (hook, veto),
//! `channels` (collect), service `ilium.core.notifications/notify`, topic
//! `ilium.core.notifications/raised`. The kernel invokes hooks and collects contributions, so the
//! point tests read the page tables; the service is called through the session harness.

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::Urgency;

use crate::feature_only::{
    json_block_after, manual_page, notices, page_text, table_rows, PLUGIN_ID,
};

fn point_rows() -> Vec<Vec<String>> {
    table_rows(&page_text(), "| Point | Kind |")
}

fn reference_page() -> String {
    manual_page("reference-core-extension-points.md")
}

/// REQ: F-048
#[test]
fn points_048_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["before-notify", "channels"]);
}

/// REQ: F-048
#[test]
fn points_048_before_notify_is_a_veto_hook_with_the_documented_payload() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`before-notify`")
        .expect("row");
    assert_eq!(row[1], "hook (veto)");
    assert_eq!(
        row[2],
        "Mute or refuse a notification, for example at night."
    );
    assert_eq!(row[3], "event, pane, title, urgency, channels");
    assert_eq!(row[4], "standard");
}

/// REQ: F-048
#[test]
fn points_048_channels_is_a_collect_point_with_entry_id_title_service() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`channels`").expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a delivery channel such as a chat webhook.");
    assert_eq!(
        row[3],
        "entry: id, title, service (a service the plugin provides that delivers one notice)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-048
#[test]
fn points_048_every_point_is_standard_risk() {
    assert!(point_rows().iter().all(|row| row[4] == "standard"));
}

/// REQ: F-048
#[test]
fn points_048_the_reference_page_agrees_with_the_plugin_page() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.notifications/before-notify` | hook: veto | standard | Mute or refuse a notification, for example at night. |"));
    assert!(reference.contains("| `ilium.core.notifications/channels` | collect | standard | Add a delivery channel such as a chat webhook. |"));
    assert!(reference.contains("| [Notifications](/pages/core-plugin-notifications/) | `ilium.core.notifications` | 2 | 1 | 1 | yes |"));
}

/// REQ: F-048
#[test]
fn points_048_the_plugin_is_replaceable_and_a_replacement_provides_the_whole_surface() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("\"replaces\": [\"ilium.core.notifications\"]"));
}

/// REQ: F-048
#[test]
fn points_048_the_veto_example_contribution_is_a_valid_manifest_fragment() {
    let page = page_text();
    let fragment = json_block_after(
        &page,
        "#### Example: Refuses every notice with urgency `low`",
    );
    assert_eq!(fragment["api"], "ilium:plugin@0.1.0");
    assert_eq!(fragment["exports"], serde_json::json!(["hook-handler"]));
    assert_eq!(
        fragment["capabilities"]["server"],
        serde_json::json!(["extensions.contribute:ilium.core.notifications/before-notify"])
    );
    let contribution = &fragment["contributes"]["extensions"][0];
    assert_eq!(
        contribution["point"],
        "ilium.core.notifications/before-notify"
    );
    assert_eq!(contribution["priority"], 50);
}

/// REQ: F-048
#[test]
fn points_048_a_late_or_failing_handler_counts_as_pass() {
    assert!(page_text().contains("A handler that is late, traps or returns a reply the mode does not allow counts as `pass`."));
}

/// REQ: F-048
#[test]
fn points_048_the_reference_page_lists_the_service_and_the_topic() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.notifications/notify` | send: raise a notice with a title, a body and an urgency |"));
    assert!(reference.contains("| `ilium.core.notifications/raised` | { event, pane, urgency } |"));
}

/// REQ: F-048
#[test]
fn points_048_the_page_names_the_grants_for_providing_and_contributing() {
    let page = page_text();
    assert!(page.contains("grant `extensions.provide:ilium.core.notifications/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.notifications/<point>`"));
    assert!(page.contains("(grant `services.use:<name>`)"));
}

fn started_session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("notifications").start();
    session
}

/// REQ: F-048
#[test]
fn service_048_notify_raises_a_notice_with_the_given_title_body_and_urgency() {
    let mut session = started_session();
    let request = r#"{"title":"Tasks finished","body":"3 tasks are done.","urgency":"normal"}"#;
    let reply = session
        .call_service(
            PLUGIN_ID,
            "ilium.core.notifications/notify",
            "^0.1",
            request,
        )
        .expect("service answers");
    assert!(matches!(reply, ServiceReply::Done(_)));
    session.pump().expect("pump");
    let found = notices(&session.server(PLUGIN_ID).submitted());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Tasks finished");
    assert_eq!(found[0].body.as_deref(), Some("3 tasks are done."));
    assert!(matches!(found[0].urgency, Urgency::Normal));
}

/// REQ: F-048
#[test]
fn service_048_notify_maps_each_urgency_word() {
    for (word, expected) in [
        ("low", Urgency::Low),
        ("normal", Urgency::Normal),
        ("high", Urgency::High),
    ] {
        let mut session = started_session();
        let request = format!(r#"{{"title":"t","body":"b","urgency":"{word}"}}"#);
        session
            .call_service(
                PLUGIN_ID,
                "ilium.core.notifications/notify",
                "^0.1",
                &request,
            )
            .expect("service answers");
        session.pump().expect("pump");
        let found = notices(&session.server(PLUGIN_ID).submitted());
        assert_eq!(found.len(), 1, "{word}");
        assert!(
            std::mem::discriminant(&found[0].urgency) == std::mem::discriminant(&expected),
            "{word}"
        );
    }
}

/// REQ: F-048
#[test]
fn service_048_notify_with_an_unknown_urgency_is_refused_with_invalid_argument() {
    let mut session = started_session();
    let request = r#"{"title":"t","body":"b","urgency":"shouting"}"#;
    let error = session
        .call_service(
            PLUGIN_ID,
            "ilium.core.notifications/notify",
            "^0.1",
            request,
        )
        .expect_err("refused");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    session.pump().expect("pump");
    assert!(notices(&session.server(PLUGIN_ID).submitted()).is_empty());
}

/// REQ: F-048
#[test]
fn service_048_notify_without_a_title_is_refused_with_invalid_argument() {
    let mut session = started_session();
    let error = session
        .call_service(
            PLUGIN_ID,
            "ilium.core.notifications/notify",
            "^0.1",
            r#"{"body":"b","urgency":"low"}"#,
        )
        .expect_err("refused");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-048
#[test]
fn service_048_notify_with_malformed_json_is_refused_with_invalid_argument() {
    let mut session = started_session();
    let error = session
        .call_service(
            PLUGIN_ID,
            "ilium.core.notifications/notify",
            "^0.1",
            "{not json",
        )
        .expect_err("refused");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-048
#[test]
fn service_048_an_unknown_service_name_is_not_found() {
    let mut session = started_session();
    let error = session
        .call_service(PLUGIN_ID, "ilium.core.notifications/nothing", "^0.1", "{}")
        .expect_err("not found");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}
