//! The page and the reference pages it feeds: tables, counts, examples and figures are checked by
//! parsing them (rule: a row that is only checkable by reading a page is parsed in a `doc_` test).

use serde_json::Value;

use crate::feature_only::{golden_path, page_text};
use crate::manual_pages::page;

/// The figure blocks (`<pre class="term">`) of the page with the markup removed.
fn figures(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let end = after.find("</pre>").expect("figure end");
        let mut text = String::new();
        let mut in_tag = false;
        for character in after[..end].chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        found.push(
            text.replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">"),
        );
        rest = &after[end..];
    }
    found
}

/// Table rows (cells trimmed) of the first table after `heading`.
fn table_after(page: &str, heading: &str) -> Vec<Vec<String>> {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("{heading} missing"));
    page[start..]
        .lines()
        .skip(1)
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// REQ: F-109
#[test]
fn doc_109_the_panes_figure_header_is_session_pane_agent_state_cost() {
    let figures = figures(&page_text());
    let lines: Vec<&str> = figures[0].lines().collect();
    assert_eq!(lines[0], "$ ilium panes");
    assert_eq!(lines[1], "SESSION  PANE  AGENT   STATE      COST");
}

/// REQ: F-109
#[test]
fn doc_109_the_panes_figure_lists_two_sessions_with_cost_only_on_agents() {
    let figures = figures(&page_text());
    let lines: Vec<&str> = figures[0].lines().collect();
    assert_eq!(lines.len(), 6);
    assert!(lines[2].starts_with("default  3     claude  working    $1.42"));
    assert!(lines[5].starts_with("review   4     shell   -          -"));
}

/// REQ: F-036, F-113
#[test]
fn doc_113_the_dialog_golden_equals_the_page_figure() {
    let golden =
        std::fs::read_to_string(golden_path("ask_for_update_48.txt")).expect("golden exists");
    let figures = figures(&page_text());
    assert_eq!(golden.trim_end_matches('\n'), figures[1]);
}

/// REQ: F-113
#[test]
fn doc_113_the_dialog_figure_has_message_field_send_and_cancel() {
    let figure = &figures(&page_text())[1];
    assert!(figure.contains("╭ Ask for update"));
    assert!(figure.contains("Message"));
    assert!(figure.contains("[ Send ]"));
    assert!(figure.contains("[ Cancel ]"));
}

/// REQ: F-113
#[test]
fn doc_113_the_dialog_figure_leaders_are_ellipsis_with_one_blank_on_each_side() {
    let figure = &figures(&page_text())[1];
    let to_row = figure.lines().nth(1).expect("To row");
    assert!(to_row.contains("To…"), "{to_row}");
    assert!(to_row.contains("… All agents (3)"), "{to_row}");
}

/// REQ: F-110
#[test]
fn doc_110_extension_point_table_has_selectors_and_before_broadcast() {
    let rows = table_after(&page_text(), "### Extension points");
    let names: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(names, ["`selectors`", "`before-broadcast`"]);
    assert_eq!(rows[0][1], "collect");
    assert_eq!(rows[0][4], "standard");
    assert_eq!(rows[1][1], "hook (transform)");
    assert_eq!(rows[1][4], "high");
}

/// REQ: F-110
#[test]
fn doc_110_reference_extension_table_agrees_with_the_page() {
    let reference = page("reference-core-extension-points");
    assert!(reference.contains("| `ilium.core.fleet/selectors` | collect | standard |"));
    assert!(reference.contains("| `ilium.core.fleet/before-broadcast` | hook: transform | high |"));
}

/// REQ: F-110, F-109
#[test]
fn doc_110_reference_counts_for_fleet_are_two_points_two_services_one_topic_replaceable() {
    let reference = page("reference-core-extension-points");
    assert!(reference
        .contains("| [Fleet](/pages/core-plugin-fleet/) | `ilium.core.fleet` | 2 | 2 | 1 | yes |"));
}

/// REQ: F-110, F-109
#[test]
fn doc_110_services_and_topic_are_the_two_services_and_the_broadcast_sent_topic() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.fleet/broadcast`: send: send a message to a selection"));
    assert!(page.contains("- `ilium.core.fleet/panes`: list: list panes of every session"));
    assert!(page.contains("- `ilium.core.fleet/broadcast-sent`: { targets, length }"));
}

/// REQ: F-109
#[test]
fn doc_109_the_page_states_that_the_cost_usage_service_is_called() {
    assert!(page_text().contains("Calls `ilium.core.cost/usage` for the cost column."));
}

/// REQ: F-110
#[test]
fn doc_110_the_page_states_the_plugin_is_replaceable() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("\"replaces\": [\"ilium.core.fleet\"]"));
}

/// REQ: F-110
#[test]
fn doc_110_the_selectors_example_is_valid_and_targets_the_selectors_point() {
    let page = page_text();
    let start = page
        .find("#### Example: Lets `ilium broadcast --select reviewers`")
        .expect("example heading");
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("block") + 8;
    let close = rest[open..].find("```").expect("end");
    let example: Value = serde_json::from_str(&rest[open..open + close]).expect("JSON");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.fleet/selectors");
    assert_eq!(entry["id"], "reviewers");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["title"], "Reviewers");
    assert_eq!(entry["entry"]["rule"]["tag"], "review");
    assert_eq!(
        example["capabilities"]["server"][0],
        "extensions.contribute:ilium.core.fleet/selectors"
    );
}

/// REQ: COR-022
#[test]
fn doc_022_disabled_notice_text_is_exact() {
    assert!(page_text().contains("\"Fleet is disabled. Values are kept.\""));
}

/// REQ: COR-022, COR-040
#[test]
fn doc_022_the_disabled_effect_row_lists_the_commands_the_manifest_declares() {
    // The kernel-and-boot row for Fleet names what fails when Fleet is off; every CLI command
    // the manifest contributes must be among the failing ones (a plugin-owned command cannot
    // stay alive when its owner is off).
    let boot = page("reference-kernel-and-boot");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Fleet | `ilium panes`"))
        .expect("effect row");
    for command in ["ilium panes", "ilium broadcast", "ilium new-pane"] {
        let failing = row.split("| ").nth(2).unwrap_or("");
        assert!(
            failing.contains(command),
            "{command} not listed as failing in: {row}"
        );
    }
}

/// REQ: COR-010
#[test]
fn doc_010_the_kernel_answers_404_for_the_route_while_the_plugin_is_off() {
    let boot = page("reference-kernel-and-boot");
    assert!(boot.contains("The Fleet plugin registers the route `POST /create_agent` with the grant `http-api.route:/create_agent`."));
    assert!(boot.contains("If Fleet is off, the kernel answers `404`"));
}

/// REQ: COR-011
#[test]
fn doc_011_the_api_tab_keeps_the_config_table_and_has_no_readers() {
    assert!(page_text().contains("| `api` | API | `[api]` | none |"));
    assert!(page_text().contains(
        "The values stay in the same `config.toml` tables as before, so no user setting moves."
    ));
}

/// REQ: F-058
#[test]
fn doc_058_the_page_says_the_kernel_owns_the_listener_port_and_switch() {
    assert!(page_text().contains(
        "The kernel owns the HTTP listener, its port and its on/off setting (Settings, API tab)."
    ));
}

/// REQ: F-036
#[test]
fn doc_036_the_ask_for_update_sample_channel_is_ask_for_update() {
    let page = page_text();
    assert!(page.contains("channel: \"ask-for-update\".into()"));
    assert!(page.contains("(\"update-send\", ActionKind::Activated, _) if !text.trim().is_empty()"));
}

/// REQ: F-036
#[test]
fn doc_036_the_notifications_handoff_sample_uses_the_agent_question_channel() {
    let page = page_text();
    assert!(page.contains("channel: \"agent-question\".into()"));
    assert!(page.contains("plugin: \"ilium.core.notifications\".into()"));
}

/// REQ: F-058
#[test]
fn doc_058_the_automation_doc_lists_the_four_request_fields() {
    let automation = std::fs::read_to_string(
        crate::manual_pages::workspace_root().join("src/docs/automation.md"),
    )
    .expect("doc");
    for field in ["`agent_type`", "`project`", "`prompt`", "`workspace`"] {
        assert!(automation.contains(field), "{field}");
    }
    assert!(automation.contains("The default is `127.0.0.1:8872`."));
}
