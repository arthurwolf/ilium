//! Extension points `toolbar-actions` and `banner-segments`, the `last-prompt` service and the Replaceable rule.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::AgentActivity;
use serde_json::{json, Value};

use crate::common::*;

const POINT_ACTIONS: &str = "ilium.core.agent-toolbar/toolbar-actions";
const SERVICE: &str = "ilium.core.agent-toolbar/last-prompt";

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

fn row(point: &str) -> [String; 5] {
    point_rows()
        .into_iter()
        .find(|row| row[0] == format!("`{point}`"))
        .unwrap_or_else(|| panic!("row {point}"))
}

/// REQ: COR-011
#[test]
fn ext_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["toolbar-actions", "banner-segments"]);
}

/// REQ: COR-011
#[test]
fn ext_toolbar_actions_is_a_collect_point_with_the_documented_entry() {
    let row = row("toolbar-actions");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[3],
        "entry: id, signature, label, icon, command, requires_action"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: COR-011
#[test]
fn ext_banner_segments_is_a_collect_point_with_priority_and_max_width() {
    let row = row("banner-segments");
    assert_eq!(row[1], "collect");
    assert!(
        row[3].starts_with("entry: id, priority, max_width"),
        "{}",
        row[3]
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-035
#[test]
fn ext_toolbar_actions_adds_a_button_to_panes_that_run_a_signature() {
    assert!(row("toolbar-actions")[2]
        .starts_with("Add a button to the toolbar of panes that run a signature."));
}

/// REQ: F-033
#[test]
fn ext_banner_segments_adds_a_segment_to_the_last_prompt_banner() {
    assert!(row("banner-segments")[2].starts_with("Add a short segment to the last-prompt banner."));
}

/// REQ: F-035
#[test]
fn ext_the_example_package_contributes_a_review_button_for_claude() {
    let page = page_text();
    let start = page
        .find("#### Example: Adds a Review button to Claude panes")
        .expect("example heading");
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json") + 8;
    let close = rest[open..].find("```").expect("end");
    let manifest: Value = serde_json::from_str(&rest[open..open + close]).expect("JSON");
    let entry = &manifest["contributes"]["extensions"][0];
    assert_eq!(entry["point"], POINT_ACTIONS);
    assert_eq!(
        entry["entry"],
        json!({"signature": "claude", "label": "Review", "icon": "eye", "command": "start-review"})
    );
    assert_eq!(
        manifest["capabilities"]["server"],
        json!(["extensions.contribute:ilium.core.agent-toolbar/toolbar-actions"])
    );
}

/// REQ: F-035
#[test]
fn ext_a_contributed_button_shows_on_matching_panes_only() {
    let mut host = client_host();
    let claude = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let codex = add_agent_pane(&mut host, "codex", "codex", AgentActivity::Idle);
    host.extensions().contribute(
        POINT_ACTIONS,
        "run-review",
        50,
        r#"{"signature":"claude","label":"Review","icon":"eye","command":"start-review"}"#,
    );
    let view = |pane: u64| {
        ilium_plugin_sdk::types::ViewTarget::Pane(ilium_plugin_sdk::types::NodeId(pane))
    };
    assert!(host.render(view(claude), 240, 6).text().contains("Review"));
    assert!(!host.render(view(codex), 240, 6).text().contains("Review"));
}

/// REQ: F-035
#[test]
fn ext_clicking_a_contributed_button_invokes_its_command() {
    let mut host = client_host();
    let claude = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.extensions().contribute(
        POINT_ACTIONS,
        "run-review",
        50,
        r#"{"signature":"claude","label":"Review","icon":"eye","command":"start-review"}"#,
    );
    let view = ilium_plugin_sdk::types::ViewTarget::Pane(ilium_plugin_sdk::types::NodeId(claude));
    let (row, column) = host
        .render(view.clone(), 240, 6)
        .find("Review")
        .expect("button");
    assert!(host.click(view, column, row));
    let invoked = host.submitted().into_iter().any(|submission| {
        matches!(&submission.intent, ilium_plugin_sdk::types::Intent::InvokeCommand(spec) if spec.command == "start-review")
    });
    assert!(invoked);
}

/// REQ: F-035
#[test]
fn ext_a_contributed_button_follows_the_built_in_buttons() {
    let mut host = client_host();
    let claude = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.extensions().contribute(
        POINT_ACTIONS,
        "run-review",
        50,
        r#"{"signature":"claude","label":"Review","icon":"eye","command":"start-review"}"#,
    );
    let view = ilium_plugin_sdk::types::ViewTarget::Pane(ilium_plugin_sdk::types::NodeId(claude));
    let text = host.render(view, 240, 6).text();
    assert!(
        text.find("Compact").unwrap() < text.find("Review").unwrap(),
        "{text}"
    );
}

fn session_with_plugin() -> FakeSession {
    let mut session = FakeSession::new();
    session.install(env!("CARGO_MANIFEST_DIR")).start();
    session
}

/// REQ: F-033
#[test]
fn svc_last_prompt_returns_the_last_prompt_text_of_the_pane() {
    let mut session = session_with_plugin();
    let pane = add_agent_pane(
        session.client(PLUGIN_ID),
        "claude",
        "claude",
        AgentActivity::Working,
    );
    session
        .client(PLUGIN_ID)
        .deliver_one(user_line(pane, "fix the build"));
    session.pump().expect("settles");
    let reply = session
        .call_service(
            "test.caller",
            SERVICE,
            "^0.1",
            &json!({"pane": pane}).to_string(),
        )
        .expect("service answers");
    assert_eq!(
        reply,
        ilium_plugin_sdk::exports::ServiceReply::Done(json!("fix the build").to_string())
    );
}

/// REQ: F-033
#[test]
fn svc_last_prompt_of_a_pane_without_a_prompt_is_null() {
    let mut session = session_with_plugin();
    let pane = add_agent_pane(
        session.client(PLUGIN_ID),
        "claude",
        "claude",
        AgentActivity::Working,
    );
    let reply = session
        .call_service(
            "test.caller",
            SERVICE,
            "^0.1",
            &json!({"pane": pane}).to_string(),
        )
        .expect("service answers");
    assert_eq!(
        reply,
        ilium_plugin_sdk::exports::ServiceReply::Done("null".to_string())
    );
}

/// REQ: F-033
#[test]
fn svc_last_prompt_of_an_unknown_pane_is_not_found() {
    let mut session = session_with_plugin();
    let error = session
        .call_service("test.caller", SERVICE, "^0.1", r#"{"pane":99999}"#)
        .expect_err("unknown pane");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}

/// REQ: F-033
#[test]
fn svc_last_prompt_with_a_malformed_request_is_invalid_argument() {
    let mut session = session_with_plugin();
    let error = session
        .call_service("test.caller", SERVICE, "^0.1", "not json")
        .expect_err("bad request");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: COR-011
#[test]
fn ext_the_page_states_the_plugin_is_replaceable() {
    let page = page_text();
    assert!(page.contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.agent-toolbar\"]`"
    ));
}

/// REQ: COR-011
#[test]
fn ext_the_page_states_the_service_name_and_its_purpose() {
    assert!(page_text().contains(
        "`ilium.core.agent-toolbar/last-prompt`: get: last prompt text of an agent pane"
    ));
}
