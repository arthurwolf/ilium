//! Twins of ilium-client/src/theme.rs: the colour scheme and the four colour overrides of the
//! `[theme]` table, as the settings plugin exposes them (F-092, src/docs/settings.md).

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/theme.rs::chrome_titles_are_inset_and_preserve_the_right_panel_corner, F-092
#[test]
fn port_092_the_settings_title_is_inset_by_two_cells_in_the_panel_border() {
    let mut host = client_host();
    let screen = host.render(tab("user-interface"), 120, 4);
    let first = screen.row(0);
    assert!(
        first.starts_with("\u{256d}"),
        "rounded corner first: {first:?}"
    );
    assert_eq!(first.chars().count(), 120);
}

/// REQ: PORT:ilium-client/src/theme.rs::chrome_titles_are_inset_and_preserve_the_right_panel_corner, F-092
#[test]
fn port_092_a_long_title_never_overwrites_the_closing_corner() {
    let mut host = client_host();
    let screen = host.render(tab("user-interface"), 40, 4);
    assert!(screen.row(0).ends_with("\u{256e}"), "{:?}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/theme.rs::color_scheme_defaults_to_dark, F-092
#[test]
fn port_092_the_colour_scheme_defaults_to_dark() {
    let mut host = client_host();
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("dark")));
}

/// REQ: PORT:ilium-client/src/theme.rs::color_scheme_defaults_to_dark, F-092
#[test]
fn port_092_the_colour_scheme_accepts_light_and_refuses_other_names() {
    let mut host = client_host();
    host.settings()
        .set("ui.color_scheme", json!("light"))
        .expect("accepted");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
    let error = host
        .settings()
        .set("ui.color_scheme", json!("sepia"))
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/theme.rs::colors_default_to_the_original_hardcoded_values_when_uninitialised, F-092
#[test]
fn port_092_the_four_colour_overrides_are_unset_by_default() {
    let mut host = client_host();
    for key in [
        "theme.accent_bg",
        "theme.accent_fg",
        "theme.border_focused",
        "theme.border_unfocused",
    ] {
        assert!(is_unset(value_of(&mut host, key)), "{key}");
    }
}

/// REQ: PORT:ilium-client/src/theme.rs::colors_default_to_the_original_hardcoded_values_when_uninitialised, F-092
#[test]
fn port_092_the_original_accent_colours_are_shown_when_nothing_is_overridden() {
    let mut host = client_host();
    let text = tab_text(&mut host, "user-interface", 120, 60).to_lowercase();
    assert!(text.contains("#9d7cd8"), "{text}");
    assert!(text.contains("#1a1b26"), "{text}");
}

/// REQ: PORT:ilium-client/src/theme.rs::default_theme_is_the_dark_preset, F-092
#[test]
fn port_092_the_default_theme_is_the_dark_preset_with_no_override() {
    let mut host = client_host();
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("dark")));
    assert!(is_unset(value_of(&mut host, "theme.accent_bg")));
}

/// REQ: PORT:ilium-client/src/theme.rs::for_scheme_selects_the_matching_preset, F-092
#[test]
fn port_092_each_scheme_name_selects_its_own_preset_and_both_are_accepted() {
    for scheme in ["dark", "light"] {
        let mut host = client_host();
        choose(&mut host, "user-interface", "ui.color_scheme", scheme);
        assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!(scheme)));
    }
}

/// REQ: PORT:ilium-client/src/theme.rs::light_preset_differs_from_dark_in_background_and_border_colors, F-092
#[test]
fn port_092_the_light_page_differs_from_the_dark_page() {
    let mut dark = client_host();
    let dark_screen = dark.render(tab("user-interface"), 100, 30);
    let mut light = client_host();
    choose(&mut light, "user-interface", "ui.color_scheme", "light");
    let light_screen = light.render(tab("user-interface"), 100, 30);
    assert_ne!(dark_screen, light_screen);
}

/// REQ: PORT:ilium-client/src/theme.rs::light_preset_differs_from_dark_in_background_and_border_colors, F-092
#[test]
fn port_092_the_foreground_accent_is_shared_by_both_schemes() {
    for scheme in ["dark", "light"] {
        let mut host = client_host();
        choose(&mut host, "user-interface", "ui.color_scheme", scheme);
        assert!(is_unset(value_of(&mut host, "theme.accent_fg")), "{scheme}");
    }
}

/// REQ: PORT:ilium-client/src/theme.rs::muted_accent_bg_darkens_for_dark_scheme_and_lightens_for_light_scheme, F-092
#[test]
fn port_092_an_accent_override_is_kept_and_the_scheme_still_applies() {
    let mut host = client_host();
    host.settings()
        .set("theme.accent_bg", json!("#9d7cd8"))
        .expect("accepted");
    for scheme in ["dark", "light"] {
        choose(&mut host, "user-interface", "ui.color_scheme", scheme);
        assert_eq!(
            value_of(&mut host, "theme.accent_bg"),
            Some(json!("#9d7cd8"))
        );
    }
}

/// REQ: PORT:ilium-client/src/theme.rs::muted_accent_bg_darkens_for_dark_scheme_and_lightens_for_light_scheme, F-092
#[test]
fn port_092_the_two_schemes_render_the_muted_accent_differently() {
    let mut dark = client_host();
    let mut light = client_host();
    choose(&mut light, "user-interface", "ui.color_scheme", "light");
    assert_ne!(
        dark.render(tab("user-interface"), 100, 30),
        light.render(tab("user-interface"), 100, 30)
    );
}
