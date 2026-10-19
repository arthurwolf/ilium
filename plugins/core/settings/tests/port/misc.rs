//! Twins of the single settings-related tests of ilium-client lib.rs, render_cache.rs, ui.rs and
//! of ilium-server/tests/common/mod.rs (F-089, F-090, F-091, F-092).

use ilium_plugin_sdk::types::{ActionKind, ActionValue};
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/lib.rs::client_surface_key_distinguishes_settings_tabs_without_owned_text, F-090
#[test]
fn port_090_each_tab_renders_its_own_label_and_not_the_label_of_another_tab() {
    let mut host = client_host();
    let debug = tab_text(&mut host, "debug", 100, 30);
    let about = tab_text(&mut host, "about", 100, 30);
    assert!(debug.contains("Debug"), "{debug}");
    assert_ne!(debug, about);
}

/// REQ: PORT:ilium-client/src/lib.rs::client_surface_key_distinguishes_settings_tabs_without_owned_text, F-090
#[test]
fn port_090_the_normal_surface_has_no_settings_page_text() {
    let mut host = client_host();
    let overview = host.render(
        ilium_plugin_sdk::types::ViewTarget::Panel("overview".to_string()),
        80,
        24,
    );
    assert!(!overview.text().contains("Debug"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::removed_selection_successor_follows_the_configured_sidebar_order, F-090
#[test]
fn port_090_the_tree_order_choice_accepts_name_ascending_and_keeps_it() {
    let mut host = client_host();
    choose(
        &mut host,
        "user-interface",
        "ui.tree_order",
        "name_ascending",
    );
    assert_eq!(
        value_of(&mut host, "ui.tree_order"),
        Some(json!("name_ascending"))
    );
}

/// REQ: PORT:ilium-client/src/render_cache.rs::removed_selection_successor_follows_the_configured_sidebar_order, F-090
#[test]
fn port_090_the_tree_order_choice_refuses_an_unknown_order() {
    let mut host = client_host();
    let error = host
        .settings()
        .set("ui.tree_order", json!("sideways"))
        .expect_err("refused");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/ui.rs::split_view_renders_configured_directional_screen_transfer_controls, F-090
#[test]
fn port_090_the_icons_tab_saves_the_screen_transfer_icons() {
    let mut host = client_host();
    choose(
        &mut host,
        "icons",
        "ui.icons.screen_transfer_right",
        "\u{21e2}",
    );
    choose(
        &mut host,
        "icons",
        "ui.icons.screen_transfer_left",
        "\u{21e0}",
    );
    assert_eq!(
        value_of(&mut host, "ui.icons.screen_transfer_right"),
        Some(json!("\u{21e2}"))
    );
    assert_eq!(
        value_of(&mut host, "ui.icons.screen_transfer_left"),
        Some(json!("\u{21e0}"))
    );
}

/// REQ: PORT:ilium-client/src/ui.rs::split_view_renders_configured_directional_screen_transfer_controls, F-090
#[test]
fn port_090_the_icons_tab_shows_the_configured_transfer_icon() {
    let mut host = client_host();
    choose(
        &mut host,
        "icons",
        "ui.icons.screen_transfer_right",
        "\u{21e2}",
    );
    let text = tab_text(&mut host, "icons", 100, 40);
    assert!(text.contains('\u{21e2}'), "{text}");
}

/// REQ: PORT:ilium-server/tests/common/mod.rs::initial_state_consumes_pane_snapshot_and_settings_through_sync_complete, F-091
#[test]
fn port_091_the_agent_detection_polling_values_of_the_initial_state_are_readable() {
    let mut host = client_host();
    host.settings()
        .set("agent_detection.working_poll_seconds", json!(10))
        .expect("accepted");
    host.settings()
        .set("agent_detection.idle_poll_seconds", json!(45))
        .expect("accepted");
    assert_eq!(
        value_of(&mut host, "agent_detection.working_poll_seconds"),
        Some(json!(10))
    );
    assert_eq!(
        value_of(&mut host, "agent_detection.idle_poll_seconds"),
        Some(json!(45))
    );
}

/// REQ: PORT:ilium-server/tests/common/mod.rs::initial_state_consumes_pane_snapshot_and_settings_through_sync_complete, F-091
#[test]
fn port_091_a_later_settings_change_is_visible_after_the_first_read() {
    let mut host = client_host();
    let first = value_of(&mut host, "agent_detection.working_poll_seconds");
    host.settings()
        .set("agent_detection.working_poll_seconds", json!(10))
        .expect("accepted");
    host.send_action(
        tab("agent-monitoring"),
        "agent_detection.working_poll_seconds",
        ActionKind::Changed,
        ActionValue::Number(12.0),
    );
    host.advance_clock(600);
    host.settle();
    assert_ne!(
        value_of(&mut host, "agent_detection.working_poll_seconds"),
        first
    );
}
