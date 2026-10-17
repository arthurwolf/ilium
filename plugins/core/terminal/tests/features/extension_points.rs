//! Extension points, service and topic of the plugin (page "Extension points"), their risk marks
//! (SEC-293), the chain rule for typed input (EXT-055) and the harness stub row.

use ilium_plugin_sdk::testing::FakeSession;
use serde_json::Value;

use crate::common::{fenced_block_after, page, page_text, workspace_root};

fn table_row(text: &str, prefix: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("row {prefix:?} missing"));
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// REQ: EXT-052
#[test]
fn ext_052_the_page_lists_three_points_with_their_kinds() {
    let text = page_text();
    assert_eq!(
        table_row(&text, "| `before-send-input` |")[1],
        "hook (veto)"
    );
    assert_eq!(table_row(&text, "| `open-link` |")[1], "hook (first)");
    assert_eq!(table_row(&text, "| `link-actions` |")[1], "collect");
}

/// REQ: EXT-052
#[test]
fn ext_052_the_page_kinds_agree_with_the_core_points_table() {
    let core = page("reference-core-extension-points.md");
    assert_eq!(
        table_row(&core, "| `ilium.core.terminal/before-send-input` |")[1],
        "hook: veto"
    );
    assert_eq!(
        table_row(&core, "| `ilium.core.terminal/open-link` |")[1],
        "hook: first"
    );
    assert_eq!(
        table_row(&core, "| `ilium.core.terminal/link-actions` |")[1],
        "collect"
    );
}

/// REQ: EXT-052
#[test]
fn ext_052_the_page_payloads_name_the_documented_fields() {
    let text = page_text();
    let before = table_row(&text, "| `before-send-input` |")[3].clone();
    for field in [
        "pane",
        "length",
        "source (user, paste, plugin:<id>)",
        "preview (first 120 characters)",
    ] {
        assert!(
            before.contains(field),
            "before-send-input payload lacks {field}"
        );
    }
    let open = table_row(&text, "| `open-link` |")[3].clone();
    for field in [
        "pane",
        "link_kind",
        "text",
        "action (open-external, run-command, ignore)",
        "optional command",
    ] {
        assert!(open.contains(field), "open-link payload lacks {field}");
    }
    let actions = table_row(&text, "| `link-actions` |")[3].clone();
    for field in ["id", "link_kind", "title", "command"] {
        assert!(
            actions.contains(field),
            "link-actions payload lacks {field}"
        );
    }
}

/// REQ: SEC-293
#[test]
fn sec_293_before_send_input_is_high_on_the_page_and_in_the_security_table() {
    let security = page("reference-extension-security.md");
    assert_eq!(
        table_row(&page_text(), "| `before-send-input` |")[4],
        "high"
    );
    assert_eq!(
        table_row(&security, "| `ilium.core.terminal/before-send-input` |")[1],
        "high"
    );
}

/// REQ: SEC-293
#[test]
fn sec_293_open_link_risk_on_the_page_equals_the_security_table() {
    let security = page("reference-extension-security.md");
    let on_page = table_row(&page_text(), "| `open-link` |")[4].clone();
    assert_eq!(
        on_page,
        table_row(&security, "| `ilium.core.terminal/open-link` |")[1]
    );
}

/// REQ: SEC-293
#[test]
fn sec_293_the_page_risk_marks_agree_with_the_core_points_table() {
    let core = page("reference-core-extension-points.md");
    let text = page_text();
    for (short, long) in [
        ("before-send-input", "ilium.core.terminal/before-send-input"),
        ("open-link", "ilium.core.terminal/open-link"),
        ("link-actions", "ilium.core.terminal/link-actions"),
    ] {
        assert_eq!(
            table_row(&text, &format!("| `{short}` |"))[4],
            table_row(&core, &format!("| `{long}` |"))[2],
            "{short}"
        );
    }
}

/// REQ: EXT-050
#[test]
fn ext_050_the_link_actions_example_entry_has_link_kind_title_and_command() {
    let example: Value = serde_json::from_str(&fenced_block_after(
        &page_text(),
        "#### Example: Adds `Open ticket`",
        "json",
    ))
    .expect("example is JSON");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.terminal/link-actions");
    assert_eq!(entry["id"], "open-ticket");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["link_kind"], "ticket");
    assert_eq!(entry["entry"]["title"], "Open ticket");
    assert_eq!(entry["entry"]["command"], "open-ticket");
}

/// REQ: EXT-050
#[test]
fn ext_050_the_example_contributor_holds_the_contribute_grant_for_the_point() {
    let example: Value = serde_json::from_str(&fenced_block_after(
        &page_text(),
        "#### Example: Adds `Open ticket`",
        "json",
    ))
    .expect("example is JSON");
    assert_eq!(
        example["capabilities"]["server"],
        serde_json::json!(["extensions.contribute:ilium.core.terminal/link-actions"])
    );
}

/// REQ: EXT-055
#[test]
fn ext_055_the_requirement_row_exempts_typed_keystrokes_from_the_chain() {
    let requirements = page("reference-requirements-extension-points.md");
    let row = requirements
        .lines()
        .find(|line| line.starts_with("| [EXT-055]"))
        .expect("EXT-055 row");
    assert!(row.contains("MUST NOT run for keystrokes typed into a pane"));
    assert!(row.contains("once per write that Ilium makes on someone's behalf (paste, dialog, broadcast, voice, plugin write-input)"));
}

/// REQ: EXT-052
#[test]
fn ext_052_the_service_and_the_topic_are_listed_with_the_page_text() {
    let text = page_text();
    assert!(text.contains("`ilium.core.terminal/transfer`: send-screen: copy the visible screen of one pane to another pane"));
    assert!(text.contains("`ilium.core.terminal/screen-changed`: { pane, rows } when visible rows change, at most 10 per second"));
    assert!(text.contains("**Replaceable:** yes."));
}

/// REQ: EXT-052
#[test]
fn ext_052_the_core_points_table_counts_three_points_one_service_one_topic() {
    let core = page("reference-core-extension-points.md");
    let row = table_row(&core, "| [Terminal](/pages/core-plugin-terminal/)");
    assert_eq!(&row[2..], ["3", "1", "1", "yes"]);
}

/// REQ: DEV-489
#[test]
fn dev_489_the_harness_stub_row_for_terminal_replies_empty_collect_and_pass_hooks() {
    let harness = page("reference-test-harness.md");
    let row = table_row(&harness, "| `ilium.core.terminal` |");
    assert_eq!(
        row[1],
        "`before-send-input` (veto), `open-link` (first), `link-actions` (collect)"
    );
    assert_eq!(row[3], "collect: empty list; hooks: `pass`");
}

/// REQ: EXT-052
#[test]
fn ext_052_link_actions_with_no_contributor_collects_an_empty_list() {
    let mut session = FakeSession::new();
    session
        .install(workspace_root().join("plugins/core/terminal"))
        .start();
    let replies = session
        .call_extension(
            "ilium.core.terminal/link-actions",
            "{\"link_kind\":\"ticket\"}",
        )
        .expect("collect");
    assert!(replies.is_empty());
}

/// REQ: EXT-055
#[test]
fn ext_055_before_send_input_with_no_handler_lets_a_paste_through() {
    let mut session = FakeSession::new();
    session
        .install(workspace_root().join("plugins/core/terminal"))
        .start();
    let payload = "{\"pane\":3,\"length\":5,\"source\":\"paste\",\"preview\":\"hello\"}";
    let replies = session
        .call_extension("ilium.core.terminal/before-send-input", payload)
        .expect("veto chain");
    assert!(replies.is_empty());
}
