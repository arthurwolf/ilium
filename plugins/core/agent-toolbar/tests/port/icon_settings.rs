//! Twins of icon_settings.rs: the configurable icon catalogue and stable-glyph preference.
//! Request/reply shape: see API_GAPS_ports.md.
use serde_json::Value;
use crate::common::{ask, Place};

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(&reply[name.as_str()], value, "field {name} of reply {reply}");
    }
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::catalogue_contains_thousands_of_named_icons_in_detailed_categories
#[test]
fn port_icon_settings_catalogue_contains_thousands_of_named_icons_in_detailed_categories() {
    check(
        Place::Client,
        "icons.catalogue",
        r####"{}"####,
        r####"{"icons_at_least": 10680, "categories_at_least": 50, "labels": ["Animals · Mammals", "Computer", "Mathematics", "Nerd Font · Material Design"], "official_utf8_at_least": 3500, "nerd_font_at_least": 8000, "official_before_nerd": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::every_goal_phase_has_a_named_configurable_default_icon
#[test]
fn port_icon_settings_every_goal_phase_has_a_named_configurable_default_icon() {
    check(
        Place::Client,
        "icons.defaults",
        r####"{}"####,
        r####"{"goal_active": "🎯", "goal_paused": "⏸️", "goal_blocked": "🚧", "goal_usage_limited": "⌛", "goal_reached": "🏁"}"####,
    );
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::frozen_agent_marker_is_a_separately_configurable_snowflake
#[test]
fn port_icon_settings_frozen_agent_marker_is_a_separately_configurable_snowflake() {
    check(
        Place::Client,
        "icons.defaults",
        r####"{"key": "frozen_agent"}"####,
        r####"{"default": "❄", "settable": "❅", "lock": "🔒"}"####,
    );
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::full_browse_document_preserves_family_order
#[test]
fn port_icon_settings_full_browse_document_preserves_family_order() {
    check(
        Place::Client,
        "icons.browse",
        r####"{}"####,
        r####"{"official_before_nerd": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::semantic_hits_group_without_reintroducing_lexical_filtering
#[test]
fn port_icon_settings_semantic_hits_group_without_reintroducing_lexical_filtering() {
    check(
        Place::Client,
        "icons.semantic",
        r####"{"hit": "🚀"}"####,
        r####"{"entries": 1}"####,
    );
}

/// REQ: PORT:ilium-client/src/icon_settings.rs::stable_glyph_preference_maps_status_defaults_and_preserves_custom_icons
#[test]
fn port_icon_settings_stable_glyph_preference_maps_status_defaults_and_preserves_custom_icons() {
    check(
        Place::Client,
        "icons.stable",
        r####"{"custom": "😴"}"####,
        r####"{"WaitingApproval": "?", "Done": "!", "WaitingBackground": "◷", "BackgroundTaskStillRunning": "~", "Parked": "z", "GoalActive": "@", "GoalPaused": "||", "GoalBlocked": "#", "GoalUsageLimited": "%", "GoalReached": "=", "TaskDone": "ok", "TaskError": "x", "MonitorFailed": "!", "ScheduledInput": "t", "RowRename": "✎", "RowMoveUp": "↑", "RowMoveDown": "↓", "RowClose": "×", "RowRetitle": "↻", "RowProjectRestructure": "⟳", "AskForUpdate": "?", "normal_mode": "default_glyph", "custom_preserved": "😴"}"####,
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::icon_picker_v_switches_between_dense_and_single_column_modes
#[test]
fn port_icon_settings_icon_picker_v_switches_between_dense_and_single_column_modes() {
    // Assumed op `icons.picker-keys`: replays chords on an open picker and answers its column mode and openness.
    check(
        Place::Client,
        "icons.picker-keys",
        r####"{"chords": ["v"]}"####,
        r####"{"columns": "single", "open": true}"####,
    );
    check(
        Place::Client,
        "icons.picker-keys",
        r####"{"chords": ["v", "v"]}"####,
        r####"{"columns": "dense", "open": true}"####,
    );
}
