//! Values of the kernel tabs this plugin writes (User Interface, Debug): defaults, accepted
//! spellings and ranges from src/docs/settings.md ("User Interface", "Debug", `config.toml`
//! structure). An invalid value "makes that table fail validation with a message naming the key".

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ErrorCode;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::*;

fn default_of(key: &str) -> Option<Value> {
    client_host().settings().get(key)
}

fn assert_refused_naming(host: &mut FakeHost, key: &str, value: Value) {
    let error = host.settings().set(key, value.clone()).expect_err(&format!("{key}={value}"));
    assert_eq!(error.code, ErrorCode::InvalidArgument, "{key}={value}");
    assert!(error.message.contains(key), "message {:?} must name {key}", error.message);
}

/// Boundaries n-1, n, n+1 at both ends of an inclusive integer range.
fn assert_range(key: &str, low: i64, high: i64) {
    for ok in [low, low + 1, high - 1, high] {
        let mut host = client_host();
        host.settings().set(key, json!(ok)).unwrap_or_else(|error| panic!("{key}={ok}: {error:?}"));
        assert_eq!(host.settings().get(key), Some(json!(ok)), "{key}={ok}");
    }
    for bad in [low - 1, high + 1] {
        let mut host = client_host();
        assert_refused_naming(&mut host, key, json!(bad));
        assert_eq!(host.settings().get(key), default_of(key), "{key} unchanged after {bad}");
    }
}

/// REQ: F-092
#[test]
fn ui_092_theme_motion_density_and_glyph_defaults() {
    assert_eq!(default_of("ui.color_scheme"), Some(json!("dark")));
    assert_eq!(default_of("ui.motion_level"), Some(json!("reduced")));
    assert_eq!(default_of("ui.sidebar_density"), Some(json!("standard")));
    assert_eq!(default_of("ui.use_stable_glyphs"), Some(json!(false)));
}

/// REQ: F-092
#[test]
fn ui_092_color_scheme_accepts_dark_and_light_only() {
    for ok in ["dark", "light"] {
        let mut host = client_host();
        host.settings().set("ui.color_scheme", json!(ok)).expect("accepted");
        assert_eq!(host.settings().get("ui.color_scheme"), Some(json!(ok)));
    }
    for bad in ["Dark", "solarized", "", "auto"] {
        assert_refused_naming(&mut client_host(), "ui.color_scheme", json!(bad));
    }
}

/// REQ: F-092
#[test]
fn ui_092_motion_level_accepts_full_reduced_off_only() {
    for ok in ["full", "reduced", "off"] {
        let mut host = client_host();
        host.settings().set("ui.motion_level", json!(ok)).expect("accepted");
        assert_eq!(host.settings().get("ui.motion_level"), Some(json!(ok)));
    }
    for bad in ["Full", "none", "on", ""] {
        assert_refused_naming(&mut client_host(), "ui.motion_level", json!(bad));
    }
}

/// REQ: F-092
#[test]
fn ui_092_sidebar_density_accepts_compact_standard_comfortable_only() {
    for ok in ["compact", "standard", "comfortable"] {
        let mut host = client_host();
        host.settings().set("ui.sidebar_density", json!(ok)).expect("accepted");
        assert_eq!(host.settings().get("ui.sidebar_density"), Some(json!(ok)));
    }
    for bad in ["tight", "Standard", ""] {
        assert_refused_naming(&mut client_host(), "ui.sidebar_density", json!(bad));
    }
}

/// REQ: F-092
#[test]
fn ui_092_stable_glyphs_is_an_explicit_opt_in_boolean() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.use_stable_glyphs"), Some(json!(false)));
    host.settings().set("ui.use_stable_glyphs", json!(true)).expect("accepted");
    assert_eq!(host.settings().get("ui.use_stable_glyphs"), Some(json!(true)));
    assert_refused_naming(&mut host, "ui.use_stable_glyphs", json!("yes"));
}

/// REQ: F-090
#[test]
fn ui_090_left_panel_sizing_mode_defaults_to_width_dependent_and_accepts_three_spellings() {
    assert_eq!(default_of("ui.left_panel_sizing_mode"), Some(json!("terminal_width_dependent")));
    for ok in ["fixed", "focus_dependent", "terminal_width_dependent"] {
        let mut host = client_host();
        host.settings().set("ui.left_panel_sizing_mode", json!(ok)).expect("accepted");
    }
    for bad in ["width_dependent", "Fixed", "focus-dependent"] {
        assert_refused_naming(&mut client_host(), "ui.left_panel_sizing_mode", json!(bad));
    }
}

/// REQ: F-090
#[test]
fn ui_090_panel_width_defaults() {
    assert_eq!(default_of("ui.left_panel_fixed_width"), Some(json!(32)));
    assert_eq!(default_of("ui.left_panel_unfocused_width"), Some(json!(24)));
    assert_eq!(default_of("ui.left_panel_focused_width"), Some(json!(44)));
    assert_eq!(default_of("ui.left_panel_minimum_terminal_width"), Some(json!(120)));
}

/// REQ: F-090
#[test]
fn ui_090_fixed_panel_width_ranges_from_16_to_80() {
    assert_range("ui.left_panel_fixed_width", 16, 80);
}

/// REQ: F-090
#[test]
fn ui_090_unfocused_panel_width_ranges_from_16_to_80() {
    assert_range("ui.left_panel_unfocused_width", 16, 80);
}

/// REQ: F-090
#[test]
fn ui_090_focused_panel_width_ranges_from_16_to_80() {
    assert_range("ui.left_panel_focused_width", 16, 80);
}

/// REQ: F-090
#[test]
fn ui_090_minimum_terminal_width_ranges_from_40_to_500() {
    assert_range("ui.left_panel_minimum_terminal_width", 40, 500);
}

/// REQ: F-090
#[test]
fn ui_090_last_prompt_banner_lines_range_from_1_to_20_with_default_4() {
    assert_eq!(default_of("ui.last_prompt_max_lines"), Some(json!(4)));
    assert_range("ui.last_prompt_max_lines", 1, 20);
}

/// REQ: F-090
#[test]
fn ui_090_tree_order_defaults_to_manual_and_accepts_the_six_documented_spellings() {
    assert_eq!(default_of("ui.tree_order"), Some(json!("manual")));
    for ok in ["manual", "type", "age_ascending", "age_descending", "name_ascending", "name_descending"] {
        let mut host = client_host();
        host.settings().set("ui.tree_order", json!(ok)).expect("accepted");
        assert_eq!(host.settings().get("ui.tree_order"), Some(json!(ok)));
    }
    assert_refused_naming(&mut client_host(), "ui.tree_order", json!("newest"));
}

/// REQ: F-090
#[test]
fn ui_090_agent_identifier_mode_defaults_to_icon_and_accepts_four_spellings() {
    assert_eq!(default_of("ui.agent_identifier_mode"), Some(json!("icon")));
    for ok in ["full_name", "letter", "icon", "hidden"] {
        let mut host = client_host();
        host.settings().set("ui.agent_identifier_mode", json!(ok)).expect("accepted");
    }
    assert_refused_naming(&mut client_host(), "ui.agent_identifier_mode", json!("nothing"));
}

/// REQ: F-090
#[test]
fn ui_090_toggle_defaults_of_the_user_interface_tab() {
    let off = [
        "ui.show_inferred_title_icons",
        "ui.show_tree_row_management_controls",
        "ui.agent_debug_menu_enabled",
    ];
    let on = [
        "ui.show_context_menu_icons",
        "ui.agent_toolbar_enabled",
        "ui.show_toolbar_labels",
        "ui.last_prompt_enabled",
        "ui.terminal_text_selection_enabled",
        "ui.lock_closed_enabled",
        "ui.auto_remove_empty_groups",
    ];
    for key in off {
        assert_eq!(default_of(key), Some(json!(false)), "{key}");
    }
    for key in on {
        assert_eq!(default_of(key), Some(json!(true)), "{key}");
    }
}

/// REQ: F-090
#[test]
fn ui_090_every_toggle_refuses_a_non_boolean_naming_the_key() {
    for key in [
        "ui.show_inferred_title_icons",
        "ui.agent_toolbar_enabled",
        "ui.lock_closed_enabled",
        "ui.auto_remove_empty_groups",
    ] {
        assert_refused_naming(&mut client_host(), key, json!("on"));
        assert_refused_naming(&mut client_host(), key, json!(1));
    }
}

/// REQ: F-092
#[test]
fn theme_092_the_four_colour_overrides_accept_hex_and_refuse_other_text() {
    for key in ["theme.accent_bg", "theme.accent_fg", "theme.border_focused", "theme.border_unfocused"] {
        let mut host = client_host();
        host.settings().set(key, json!("#1f6feb")).expect("accepted");
        assert_eq!(host.settings().get(key), Some(json!("#1f6feb")));
        assert_refused_naming(&mut host, key, json!("blue"));
    }
}

/// REQ: F-086
#[test]
fn debug_086_file_logging_is_off_by_default() {
    assert_eq!(default_of("debug.file_logging_enabled"), Some(json!(false)));
}

/// REQ: F-086
#[test]
fn debug_086_file_logging_switches_on_and_off() {
    let mut host = client_host();
    host.settings().set("debug.file_logging_enabled", json!(true)).expect("accepted");
    assert_eq!(host.settings().get("debug.file_logging_enabled"), Some(json!(true)));
    host.settings().set("debug.file_logging_enabled", json!(false)).expect("accepted");
    assert_eq!(host.settings().get("debug.file_logging_enabled"), Some(json!(false)));
}

/// REQ: F-086
#[test]
fn debug_086_file_logging_refuses_a_non_boolean_invalid_argument() {
    for value in [json!("true"), json!(1)] {
        let mut host = client_host();
        assert_refused_naming(&mut host, "debug.file_logging_enabled", value);
    }
}

/// REQ: F-091
#[test]
fn config_091_a_refused_value_leaves_the_stored_value_untouched() {
    let mut host = client_host();
    host.settings().set("ui.motion_level", json!("off")).expect("accepted");
    assert_refused_naming(&mut host, "ui.motion_level", json!("bogus"));
    assert_eq!(host.settings().get("ui.motion_level"), Some(json!("off")));
}

/// REQ: F-091
#[test]
fn config_091_changes_to_one_table_do_not_touch_another() {
    let mut host = client_host();
    host.settings().set("debug.file_logging_enabled", json!(true)).expect("accepted");
    host.settings().set("ui.motion_level", json!("full")).expect("accepted");
    assert_eq!(host.settings().get("debug.file_logging_enabled"), Some(json!(true)));
    assert_eq!(host.settings().get("ui.sidebar_density"), Some(json!("standard")));
}

proptest! {
    // REQ: F-090
    #[test]
    fn ui_090_fixed_width_accepts_exactly_16_to_80(width in -10i64..600) {
        let mut host = client_host();
        let outcome = host.settings().set("ui.left_panel_fixed_width", json!(width));
        if (16..=80).contains(&width) {
            prop_assert!(outcome.is_ok());
        } else {
            prop_assert_eq!(outcome.expect_err("refused").code, ErrorCode::InvalidArgument);
        }
    }

    // REQ: F-090
    #[test]
    fn ui_090_minimum_terminal_width_accepts_exactly_40_to_500(width in 0i64..700) {
        let mut host = client_host();
        let outcome = host.settings().set("ui.left_panel_minimum_terminal_width", json!(width));
        if (40..=500).contains(&width) {
            prop_assert!(outcome.is_ok());
        } else {
            prop_assert_eq!(outcome.expect_err("refused").code, ErrorCode::InvalidArgument);
        }
    }
}
