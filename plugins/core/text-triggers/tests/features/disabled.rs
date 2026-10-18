//! Disabling the plugin (COR-022): no output filtering, the stored values stay and the tab shows them read-only
//! with the page's notice; nothing is deleted (COR-033, COR-011).

use ilium_plugin_sdk::types::{DeactivationReason, ViewTarget};
use serde_json::json;

use crate::common::*;
use crate::manual::{manual_page, page_text};

const NOTICE: &str = "Text triggers is disabled. Values are kept.";

/// REQ: COR-022
#[test]
fn disabled_022_the_kernel_boot_table_says_no_output_filtering() {
    let page = manual_page("reference-kernel-and-boot.md");
    let row = page
        .lines()
        .find(|line| line.starts_with("| Text triggers | No output filtering"))
        .expect("row");
    assert_eq!(
        row,
        "| Text triggers | No output filtering | Output reaches the screen unchanged |"
    );
}

/// REQ: COR-022
#[test]
fn disabled_022_the_page_states_the_notice_and_that_values_are_kept() {
    let page = page_text();
    assert!(page.contains("the tab stays in the rail with its stored values shown read-only"));
    assert!(page.contains(&format!("the notice \"{NOTICE}\"")));
}

/// REQ: COR-022
#[test]
fn disabled_022_a_matched_line_after_disable_sends_nothing() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    host.deactivate(DeactivationReason::Disabled);
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 3));
    host.settle();
    assert!(send_inputs(&submissions).is_empty(), "{submissions:?}");
    assert!(host
        .submitted()
        .iter()
        .all(|s| send_inputs(std::slice::from_ref(s)).is_empty()));
}

/// REQ: COR-022, COR-033
#[test]
fn disabled_022_stored_rules_survive_deactivation() {
    let mut host = server_host();
    set_rules(&mut host, &["a", "b"], &["1", "2"]);
    host.deactivate(DeactivationReason::Disabled);
    assert_eq!(host.settings().get("patterns"), Some(json!(["a", "b"])));
    assert_eq!(host.settings().get("replies"), Some(json!(["1", "2"])));
}

/// REQ: COR-022
#[test]
fn disabled_022_the_tab_shows_the_notice_and_the_stored_pattern_read_only() {
    let mut host = client_host();
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    host.deactivate(DeactivationReason::Disabled);
    let text = host
        .render(
            ViewTarget::SettingsPage("text-triggers".to_string()),
            80,
            12,
        )
        .text();
    assert!(text.contains(NOTICE), "{text}");
    assert!(text.contains("\\(y/n\\)\\s*$"), "{text}");
}

/// REQ: COR-022
#[test]
fn disabled_022_a_disabled_tab_refuses_edits_and_keeps_the_values() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["1"]);
    host.deactivate(DeactivationReason::Disabled);
    host.send_action(
        ViewTarget::SettingsPage("text-triggers".to_string()),
        "rule-0",
        ilium_plugin_sdk::types::ActionKind::Toggled,
        ilium_plugin_sdk::types::ActionValue::Flag(false),
    );
    host.settle();
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: COR-022
#[test]
fn disabled_022_reactivation_filters_again_from_the_stored_values() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["\\(y/n\\)\\s*$"], &["y"]);
    host.deactivate(DeactivationReason::Disabled);
    host.activate::<ilium_plugin_core_text_triggers::TextTriggersServer>();
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 4));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}
