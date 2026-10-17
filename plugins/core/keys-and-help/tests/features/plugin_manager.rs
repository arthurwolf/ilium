//! Sample 5: the `show-plugins` command reads every installed plugin with `plugins.list`, the
//! Plugins panel shows a switch for each, and a switch calls `plugins.set_enabled`.

use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::{CommandContext, CommandInvokedEvent, CommandOrigin};

use crate::common::*;

fn plugins_panel() -> ViewTarget {
    ViewTarget::Panel("plugins".to_string())
}

fn manager_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    host.plugins().add_simple("ilium.core.fleet", "Fleet", true);
    host.plugins()
        .add_simple("com.example.notes", "Notes", false);
    host
}

fn show_plugins(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    origin: CommandOrigin,
) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: format!("{PLUGIN_ID}/show-plugins"),
        arguments_json: "{}".to_string(),
        origin,
        context: CommandContext::default(),
    }))
}

/// REQ: F-010
#[test]
fn manager_010_show_plugins_opens_the_plugins_panel() {
    let mut host = manager_host();
    let submissions = show_plugins(&mut host, CommandOrigin::Palette);
    assert!(submissions.iter().any(|submission| matches!(
        &submission.intent,
        Intent::OpenView(spec) if spec.view == plugins_panel()
    )));
}

/// REQ: F-010
#[test]
fn manager_010_the_panel_lists_every_installed_plugin_by_name() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    let text = host.render(plugins_panel(), 80, 12).text();
    assert!(text.contains("Fleet"), "{text}");
    assert!(text.contains("Notes"), "{text}");
}

/// REQ: F-010
#[test]
fn manager_010_each_row_shows_a_switch_in_its_current_state() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    let screen = host.render(plugins_panel(), 80, 12);
    let (fleet_row, _) = screen.find("Fleet").expect("Fleet");
    let (notes_row, _) = screen.find("Notes").expect("Notes");
    assert!(
        screen.row(fleet_row).contains("on"),
        "{}",
        screen.row(fleet_row)
    );
    assert!(
        screen.row(notes_row).contains("off"),
        "{}",
        screen.row(notes_row)
    );
}

/// REQ: F-010
#[test]
fn manager_010_toggling_a_switch_off_calls_set_enabled_false() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.send_action(
        plugins_panel(),
        "ilium.core.fleet",
        ActionKind::Toggled,
        ActionValue::Flag(false),
    );
    host.settle();
    assert_eq!(
        host.plugins().set_enabled_calls(),
        vec![("ilium.core.fleet".to_string(), false)]
    );
}

/// REQ: F-010
#[test]
fn manager_010_toggling_a_switch_on_calls_set_enabled_true() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.send_action(
        plugins_panel(),
        "com.example.notes",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    host.settle();
    assert_eq!(
        host.plugins().set_enabled_calls(),
        vec![("com.example.notes".to_string(), true)]
    );
}

/// REQ: F-010
#[test]
fn manager_010_the_row_shows_the_new_state_after_the_toggle() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.send_action(
        plugins_panel(),
        "com.example.notes",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    host.settle();
    let screen = host.render(plugins_panel(), 80, 12);
    let (row, _) = screen.find("Notes").expect("Notes");
    assert!(screen.row(row).contains("on"));
}

/// REQ: F-010
#[test]
fn manager_010_a_refused_toggle_keeps_the_old_state_and_shows_the_error_message() {
    let mut host = manager_host();
    host.plugins().fail_next_set_enabled(PluginError {
        code: ErrorCode::NotPermitted,
        message: "disabled-by-policy".to_string(),
        retry_after_ms: None,
    });
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.send_action(
        plugins_panel(),
        "com.example.notes",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    host.settle();
    let screen = host.render(plugins_panel(), 80, 12);
    let (row, _) = screen.find("Notes").expect("Notes");
    assert!(screen.row(row).contains("off"));
    assert!(screen.text().contains("disabled-by-policy"));
}

/// REQ: F-010
#[test]
fn manager_010_a_toggle_without_the_plugins_manage_grant_is_not_permitted() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.revoke("plugins.manage");
    host.send_action(
        plugins_panel(),
        "ilium.core.fleet",
        ActionKind::Toggled,
        ActionValue::Flag(false),
    );
    host.settle();
    assert!(host.plugins().set_enabled_calls().is_empty());
    assert!(host
        .failures()
        .iter()
        .any(|failure| failure.code == "not-permitted"));
}

/// REQ: F-010
#[test]
fn manager_010_an_empty_plugin_list_renders_the_panel_without_rows() {
    let mut host = seeded_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    let text = host.render(plugins_panel(), 80, 8).text();
    assert!(text.contains("Plugins"));
    assert!(!text.contains("Fleet"));
}

/// REQ: F-010
#[test]
fn manager_010_the_list_is_read_again_each_time_the_command_runs() {
    let mut host = manager_host();
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    host.plugins().add_simple("com.example.late", "Late", true);
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    assert!(host.render(plugins_panel(), 80, 12).text().contains("Late"));
}

/// REQ: F-010
#[test]
fn manager_010_the_command_from_the_toolbar_origin_behaves_like_the_palette() {
    let mut host = manager_host();
    let submissions = show_plugins(&mut host, CommandOrigin::Toolbar);
    assert!(submissions
        .iter()
        .any(|submission| matches!(&submission.intent, Intent::OpenView(_))));
}

/// REQ: F-010
#[test]
fn manager_010_a_command_of_another_plugin_does_not_reload_the_list() {
    let mut host = manager_host();
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: "ilium.core.fleet/open".to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext::default(),
    }));
    host.settle();
    assert!(!host
        .render(plugins_panel(), 80, 12)
        .text()
        .contains("Fleet"));
}

/// REQ: F-010
#[test]
fn manager_010_a_failed_state_is_shown_for_a_plugin_that_failed() {
    let mut host = seeded_host();
    host.plugins()
        .add_simple("com.example.broken", "Broken", false);
    host.plugins().mark_failed("com.example.broken");
    show_plugins(&mut host, CommandOrigin::Palette);
    host.settle();
    let screen = host.render(plugins_panel(), 80, 12);
    let (row, _) = screen.find("Broken").expect("Broken");
    assert!(screen.row(row).contains("failed"));
}
