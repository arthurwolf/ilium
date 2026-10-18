//! Events the plugin handles (page samples 1 and 3): `settings-changed` naming `density`,
//! `theme-changed` and the `ui-action` of the `accent` colour row. The handler returns no
//! submissions: the host writes values, the plugin only redraws.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Event, NodeId, PluginSide, SettingsChange, SettingsOrigin, UiAction,
    ViewTarget,
};
use serde_json::json;

use crate::common::*;

fn overview() -> ViewTarget {
    ViewTarget::Panel("overview".to_string())
}

fn settings_changed(keys: &[&str], origin: SettingsOrigin) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| key.to_string()).collect(),
        origin,
        side: PluginSide::Client,
    })
}

fn accent_action(view: ViewTarget, key: &str, kind: ActionKind, hex: &str) -> Event {
    Event::UiAction(UiAction {
        view,
        key: key.to_string(),
        action_id: key.to_string(),
        kind,
        value: Some(ActionValue::Text(hex.to_string())),
    })
}

/// REQ: F-090
#[test]
fn events_090_settings_changed_for_density_returns_no_submissions() {
    let mut host = client_host();
    let submissions = host.deliver_one(settings_changed(&["density"], SettingsOrigin::User));
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-090
#[test]
fn events_090_a_density_change_shows_in_the_overview_panel() {
    let mut host = client_host();
    host.settings()
        .set("density", json!("compact"))
        .expect("accepted");
    host.deliver_one(settings_changed(&["density"], SettingsOrigin::User));
    host.settle();
    let screen = host.render(overview(), 120, 24);
    assert!(screen.text().contains("compact"), "{}", screen.text());
}

/// REQ: F-090
#[test]
fn events_090_the_overview_panel_follows_a_second_density_change() {
    let mut host = client_host();
    host.settings()
        .set("density", json!("compact"))
        .expect("accepted");
    host.deliver_one(settings_changed(&["density"], SettingsOrigin::User));
    host.settle();
    host.settings()
        .set("density", json!("comfortable"))
        .expect("accepted");
    host.deliver_one(settings_changed(&["density"], SettingsOrigin::Plugin));
    host.settle();
    let text = host.render(overview(), 120, 24).text();
    assert!(text.contains("comfortable"), "{text}");
    assert!(!text.contains("compact"), "{text}");
}

/// REQ: F-090
#[test]
fn events_090_settings_changed_for_other_keys_returns_no_submissions() {
    let mut host = client_host();
    let submissions = host.deliver_one(settings_changed(
        &["accent", "unrelated"],
        SettingsOrigin::File,
    ));
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-090
#[test]
fn events_090_settings_changed_with_every_origin_is_handled() {
    for origin in [
        SettingsOrigin::User,
        SettingsOrigin::Plugin,
        SettingsOrigin::Reset,
        SettingsOrigin::OtherPlugin,
        SettingsOrigin::File,
    ] {
        let mut host = client_host();
        let submissions = host.deliver_one(settings_changed(&["density"], origin));
        host.settle();
        assert!(submissions.is_empty());
        assert!(host.failures().is_empty());
    }
}

/// REQ: F-090
#[test]
fn events_090_settings_changed_with_no_keys_is_ignored() {
    let mut host = client_host();
    let submissions = host.deliver_one(settings_changed(&[], SettingsOrigin::User));
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-092
#[test]
fn events_092_theme_changed_redraws_without_submissions() {
    let mut host = client_host();
    let submissions = host.deliver_one(Event::ThemeChanged);
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-092
#[test]
fn events_092_the_overview_panel_still_renders_after_theme_changed() {
    let mut host = client_host();
    host.deliver_one(Event::ThemeChanged);
    host.settle();
    let screen = host.render(overview(), 80, 20);
    assert_eq!((screen.columns, screen.rows), (80, 20));
}

/// REQ: F-092
#[test]
fn events_092_a_changed_accent_action_updates_the_colour_picker_value() {
    let mut host = client_host();
    host.deliver_one(accent_action(
        overview(),
        "accent",
        ActionKind::Changed,
        "#ff8800",
    ));
    host.settle();
    let text = host.render(overview(), 120, 24).text();
    assert!(text.contains("#ff8800"), "{text}");
}

/// REQ: F-092
#[test]
fn events_092_an_action_for_another_key_leaves_the_accent_alone() {
    let mut host = client_host();
    host.deliver_one(accent_action(
        overview(),
        "other",
        ActionKind::Changed,
        "#ff8800",
    ));
    host.settle();
    let text = host.render(overview(), 120, 24).text();
    assert!(!text.contains("#ff8800"), "{text}");
}

/// REQ: F-092
#[test]
fn events_092_an_accent_action_of_another_kind_leaves_the_accent_alone() {
    let mut host = client_host();
    host.deliver_one(accent_action(
        overview(),
        "accent",
        ActionKind::HoverEnter,
        "#ff8800",
    ));
    host.settle();
    let text = host.render(overview(), 120, 24).text();
    assert!(!text.contains("#ff8800"), "{text}");
}

/// REQ: F-092
#[test]
fn events_092_an_accent_action_without_a_text_value_is_ignored() {
    let mut host = client_host();
    let submissions = host.deliver_one(Event::UiAction(UiAction {
        view: overview(),
        key: "accent".to_string(),
        action_id: "accent".to_string(),
        kind: ActionKind::Changed,
        value: Some(ActionValue::Number(3.0)),
    }));
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-090
#[test]
fn events_090_unrelated_events_are_ignored_without_failure() {
    let mut host = client_host();
    let submissions = host.deliver(vec![
        Event::PaneFocused(Some(NodeId(1))),
        Event::PaneFocused(None),
        Event::KeymapChanged,
        Event::DisplayModeChanged(None),
    ]);
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

/// REQ: F-090
#[test]
fn events_090_a_mixed_batch_is_handled_in_order() {
    let mut host = client_host();
    host.settings()
        .set("density", json!("compact"))
        .expect("accepted");
    let submissions = host.deliver(vec![
        settings_changed(&["density"], SettingsOrigin::User),
        Event::ThemeChanged,
        accent_action(overview(), "accent", ActionKind::Changed, "#102030"),
    ]);
    host.settle();
    assert!(submissions.is_empty());
    let text = host.render(overview(), 120, 24).text();
    assert!(
        text.contains("compact") && text.contains("#102030"),
        "{text}"
    );
}

/// REQ: F-090
#[test]
fn events_090_a_fuzzed_event_stream_never_fails_the_plugin() {
    let mut host = client_host();
    host.fuzz_run(7, 25).expect("no failure");
    assert!(!host.is_disabled());
}
