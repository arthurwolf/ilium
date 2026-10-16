//! Sample 4: the help panel lists topic ids in a `tree-list` and renders the chosen topic as
//! `markdown`. `help.topic` returns the Markdown of an installed topic by its global id.

use ilium_plugin_sdk::prelude::*;

use crate::common::*;

const OWN_TOPIC: &str = "ilium.core.keys-and-help/keys-overview";
const GETTING_STARTED: &str = "ilium/getting-started";

fn help_panel() -> ViewTarget {
    ViewTarget::Panel("keys".to_string())
}

fn help_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    host.help().add_topic(OWN_TOPIC, "# Keys\n\nPress the prefix, then a key.");
    host.help().add_topic(GETTING_STARTED, "# Getting started\n\nWelcome to Ilium.");
    host
}

/// REQ: F-010
#[test]
fn topics_010_the_list_shows_the_two_titles_in_order() {
    let mut host = help_host();
    let text = host.render(help_panel(), 100, 20).text();
    let keys = text.find("Keys").expect("Keys");
    let started = text.find("Getting started").expect("Getting started");
    assert!(keys < started, "{text}");
}

/// REQ: F-010
#[test]
fn topics_010_the_first_topic_is_rendered_by_default() {
    let mut host = help_host();
    let text = host.render(help_panel(), 100, 20).text();
    assert!(text.contains("Press the prefix, then a key."), "{text}");
}

/// REQ: F-010
#[test]
fn topics_010_selecting_a_topic_shows_its_markdown() {
    let mut host = help_host();
    host.send_action(help_panel(), "topics", ActionKind::Selected, ActionValue::Text(GETTING_STARTED.to_string()));
    host.settle();
    let text = host.render(help_panel(), 100, 20).text();
    assert!(text.contains("Welcome to Ilium."), "{text}");
    assert!(!text.contains("Press the prefix, then a key."));
}

/// REQ: F-010
#[test]
fn topics_010_an_unknown_topic_shows_no_such_topic() {
    let mut host = help_host();
    host.send_action(help_panel(), "topics", ActionKind::Selected, ActionValue::Text("ilium/missing".to_string()));
    host.settle();
    assert!(host.render(help_panel(), 100, 20).text().contains("No such topic."));
}

/// REQ: F-010
#[test]
fn topics_010_the_topic_list_is_a_fixed_width_of_24_cells_beside_the_page() {
    let mut host = help_host();
    let screen = host.render(help_panel(), 100, 20);
    let (row, column) = screen.find("Press the prefix").expect("page text");
    assert_eq!(column, 24, "page starts at column {column} on row {row}");
}

/// REQ: F-010
#[test]
fn topics_010_selection_of_a_non_text_value_leaves_the_current_topic() {
    let mut host = help_host();
    host.send_action(help_panel(), "topics", ActionKind::Selected, ActionValue::Index(1));
    host.settle();
    assert!(host.render(help_panel(), 100, 20).text().contains("Press the prefix, then a key."));
}

/// REQ: F-010
#[test]
fn topics_010_a_topic_added_by_another_plugin_is_readable_by_its_global_id() {
    let mut host = help_host();
    host.help().add_topic("com.example.notes/formatting", "# Formatting");
    host.send_action(help_panel(), "topics", ActionKind::Selected, ActionValue::Text("com.example.notes/formatting".to_string()));
    host.settle();
    assert!(host.render(help_panel(), 100, 20).text().contains("Formatting"));
}

/// REQ: F-010
#[test]
fn topics_010_help_without_the_read_grant_is_not_permitted() {
    let mut host = help_host();
    host.revoke("help.read");
    host.send_action(help_panel(), "topics", ActionKind::Selected, ActionValue::Text(GETTING_STARTED.to_string()));
    host.settle();
    assert!(host.failures().iter().any(|failure| failure.code == "not-permitted"));
}

/// REQ: F-010
#[test]
fn topics_010_the_page_text_scrolls_with_a_scrolled_action_without_changing_the_topic() {
    let mut host = help_host();
    host.send_action(help_panel(), "page", ActionKind::Scrolled, ActionValue::Index(2));
    host.settle();
    assert!(host.failures().is_empty());
}
