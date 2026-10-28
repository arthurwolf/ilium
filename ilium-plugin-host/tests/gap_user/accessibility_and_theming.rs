//! Accessibility and theming of host-drawn plugin widgets, page `accessibility-and-theming`
//! (USR-300 .. USR-307).

use std::collections::BTreeSet;
use std::time::Duration;

use crate::common::assert_manual_says;
use ilium_plugin_host::a11y::{render_view, A11yScreen, A11ySettings, CellStyle, Key};
use ilium_plugin_host::theme::{builtin_themes, load_theme, Role};
use ilium_plugin_host::view::validate_view;

const LOW_CONTRAST_THEME: &str =
    "[theme]\nname = \"washed-out\"\n\n[colors]\nmuted = \"#202020\"\n";

const STATES_VIEW: &str = r#"{"widgets":[
  {"id":"err","kind":"label","role":"danger","text":"Failed"},
  {"id":"ok","kind":"label","role":"success","text":"Done"},
  {"id":"sel","kind":"list_row","selected":true,"label":"Pick"}]}"#;

const THREE_BUTTONS_VIEW: &str = r#"{"widgets":[
  {"id":"a","kind":"button","label":"Alpha","row":0,"col":0},
  {"id":"b","kind":"button","label":"Beta","row":0,"col":1},
  {"id":"c","kind":"button","label":"Gamma","row":1,"col":0}]}"#;

/// The attributes that can carry a focus or state cue without colour.
fn attribute_cue(style: &CellStyle) -> (bool, bool, bool) {
    (style.bold, style.reverse, style.underline)
}

/// REQ: USR-300
#[test]
fn usr_300_colour_only_states_keep_a_text_cue_under_no_color_and_16_colours() {
    let settings_list = [
        A11ySettings {
            no_color: true,
            ..Default::default()
        },
        A11ySettings {
            theme: "16-colour".to_string(),
            ..Default::default()
        },
    ];
    for settings in &settings_list {
        let frame = render_view(STATES_VIEW, settings);
        let failed_row = frame.row_containing("Failed").expect("error row");
        let failed_style = frame.style_of("Failed");
        let has_cue = failed_row.contains("error")
            || failed_row.contains('!')
            || failed_style.bold
            || failed_style.underline
            || failed_style.reverse;
        assert!(
            has_cue,
            "the error state has no non-colour cue: {failed_row}"
        );
        let pick_row = frame.row_containing("Pick").expect("selected row");
        let pick_style = frame.style_of("Pick");
        assert!(
            pick_row.trim_start().starts_with('>') || pick_style.reverse || pick_style.bold,
            "the selected state has no non-colour cue: {pick_row}"
        );
    }
}

/// REQ: USR-301
#[test]
fn usr_301_every_builtin_theme_meets_4_5_contrast_and_a_low_contrast_theme_is_refused() {
    for theme in builtin_themes() {
        for role in Role::ALL {
            let ratio = theme.contrast_ratio(role);
            assert!(
                ratio >= 4.5,
                "theme {} role {role:?} has contrast {ratio:.2}",
                theme.name()
            );
        }
    }
    let message = match load_theme(LOW_CONTRAST_THEME) {
        Ok(_) => panic!("a low-contrast theme was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("theme.low-contrast"), "{message}");
    assert!(
        message.contains("muted"),
        "the refusal must name the role: {message}"
    );
}

/// REQ: USR-302
#[test]
fn usr_302_high_contrast_raises_every_role_to_7_to_1_and_passes_extras_to_plugins() {
    for theme in builtin_themes() {
        let high = theme.high_contrast();
        for role in Role::ALL {
            let ratio = high.contrast_ratio(role);
            assert!(
                ratio >= 7.0,
                "high contrast of {} role {role:?} is {ratio:.2}",
                theme.name()
            );
        }
        assert_eq!(
            high.role_color(Role::Muted),
            high.foreground(),
            "muted text must become the foreground colour"
        );
        assert_eq!(
            high.extras()["contrast"],
            "high",
            "plugins read the setting from extras"
        );
    }
    let settings = A11ySettings {
        contrast: "high".to_string(),
        ..Default::default()
    };
    let plugin_view =
        r#"{"widgets":[{"id":"x","kind":"label","rgb":[10,200,30],"text":"Custom"}]}"#;
    let frame = render_view(plugin_view, &settings);
    assert!(
        frame.row_containing("Custom").is_some(),
        "a plugin ignoring the setting must still render"
    );
}

/// REQ: USR-303
#[test]
fn usr_303_every_action_is_keyboard_reachable_tab_never_traps_and_esc_leaves_a_modal() {
    let mut screen = A11yScreen::open(THREE_BUTTONS_VIEW, &A11ySettings::default());
    assert_eq!(
        screen.focus_order(),
        vec!["a", "b", "c"],
        "focus must follow reading order"
    );
    let mut reached = BTreeSet::new();
    for _ in 0..12 {
        reached.insert(screen.focused_id().to_string());
        screen.press(Key::Tab);
    }
    assert_eq!(reached.len(), 3, "Tab must reach every action: {reached:?}");

    let modal_view = r#"{"modal":true,"widgets":[{"id":"ok","kind":"button","label":"OK"}]}"#;
    let mut dialog = A11yScreen::open(modal_view, &A11ySettings::default());
    dialog.press(Key::Esc);
    assert!(dialog.is_closed(), "Esc must leave a modal plugin dialog");
}

/// REQ: USR-304
#[test]
fn usr_304_focus_indicator_differs_by_an_attribute_and_exactly_one_widget_carries_it() {
    let settings = A11ySettings {
        no_color: true,
        ..Default::default()
    };
    let screen = A11yScreen::open(THREE_BUTTONS_VIEW, &settings);
    let frame = screen.frame();
    let focused = attribute_cue(&frame.style_of("Alpha"));
    let unfocused = attribute_cue(&frame.style_of("Beta"));
    assert_ne!(
        focused, unfocused,
        "focus must show in an attribute besides colour"
    );
    assert_eq!(
        screen.focus_indicator_count(),
        1,
        "exactly one widget per window carries the focus indicator"
    );
}

/// REQ: USR-305
#[test]
fn usr_305_screen_reader_mode_is_linear_with_role_words_and_cursor_on_the_focused_element() {
    let settings = A11ySettings {
        screen_reader: true,
        ..Default::default()
    };
    let screen = A11yScreen::open(THREE_BUTTONS_VIEW, &settings);
    let lines = screen.frame().rows();
    assert_eq!(lines.len(), 3, "one logical element per line: {lines:?}");
    assert!(
        lines[0].contains("button") && lines[0].contains("1 of 3"),
        "role words and position missing: {}",
        lines[0]
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.chars().any(|c| ('\u{2500}'..='\u{257F}').contains(&c))),
        "linear mode must not draw box-drawing characters: {lines:?}"
    );
    assert_eq!(
        screen.cursor_position(),
        screen.focused_cell(),
        "the cursor must sit on the focused element's first cell"
    );
}

/// REQ: USR-306
#[test]
fn usr_306_focusable_widget_without_label_is_refused_and_icon_button_label_is_read() {
    let unlabelled = r#"{"widgets":[{"id":"naked","kind":"button","label":""}]}"#;
    let message = match validate_view(unlabelled) {
        Ok(()) => panic!("an empty-label button was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("ui.label-missing"), "{message}");
    assert!(
        message.contains("naked"),
        "the message must name the widget id: {message}"
    );

    let icon_only =
        r#"{"widgets":[{"id":"save","kind":"button","icon":"save","label":"Save file"}]}"#;
    assert!(
        validate_view(icon_only).is_ok(),
        "an icon button with a label is valid"
    );
    let settings = A11ySettings {
        screen_reader: true,
        ..Default::default()
    };
    let screen = A11yScreen::open(icon_only, &settings);
    assert!(
        screen
            .frame()
            .rows()
            .iter()
            .any(|row| row.contains("Save file")),
        "screen-reader mode must read the icon button's label"
    );
}

/// REQ: USR-307
#[test]
fn usr_307_reduced_motion_caps_redraws_stops_decor_and_warns_once_about_redraw_only_timers() {
    let settings = A11ySettings {
        reduced_motion: true,
        ..Default::default()
    };
    let animated = r#"{"widgets":[{"id":"spin","kind":"spinner"}],"timer_ms":100}"#;
    let mut screen = A11yScreen::open(animated, &settings);
    screen.advance(Duration::from_secs(1));
    let redraws = screen.redraws_in_last_second();
    assert!(
        redraws <= 1,
        "at most 1 full-view redraw per second, got {redraws}"
    );
    assert!(
        !screen.host_decorations_running(),
        "host decorative animations must stop"
    );

    let redraw_only = r#"{"widgets":[{"id":"t","kind":"label","text":"tick"}],"timer_ms":200}"#;
    let mut ticking = A11yScreen::open(redraw_only, &settings);
    ticking.advance(Duration::from_secs(4));
    let warnings = ticking
        .log_codes()
        .iter()
        .filter(|code| code.as_str() == "ui.motion-ignored")
        .count();
    assert_eq!(
        warnings, 1,
        "ui.motion-ignored must be logged once per session"
    );
}

/// REQ: USR-300
#[test]
fn usr_300_focused_button_under_no_color_has_a_text_marker_or_attribute() {
    assert_manual_says("accessibility-and-theming", "survives `NO_COLOR`");
    let settings = A11ySettings {
        no_color: true,
        ..Default::default()
    };
    let screen = A11yScreen::open(THREE_BUTTONS_VIEW, &settings);
    let frame = screen.frame();
    let focused_row = frame.row_containing("Alpha").expect("focused button row");
    let focused_style = frame.style_of("Alpha");
    assert!(
        focused_row.contains('>') || focused_style.reverse || focused_style.bold,
        "the focused button keeps a cue without colour: {focused_row}"
    );
}

/// REQ: USR-301
#[test]
fn usr_301_low_contrast_refusal_names_the_theme_as_well_as_the_role() {
    assert_manual_says(
        "accessibility-and-theming",
        "a theme that fails MUST be refused by the theme loader with `theme.low-contrast` naming the role",
    );
    let message = match load_theme(LOW_CONTRAST_THEME) {
        Ok(_) => panic!("a low-contrast theme was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("washed-out"),
        "the refusal also names the refused theme: {message}"
    );
}

/// REQ: USR-302
#[test]
fn usr_302_high_contrast_never_lowers_any_role_below_the_normal_theme() {
    assert_manual_says(
        "accessibility-and-theming",
        "raises every role to at least 7:1",
    );
    for theme in builtin_themes() {
        let high = theme.high_contrast();
        for role in Role::ALL {
            assert!(
                high.contrast_ratio(role) >= theme.contrast_ratio(role),
                "high contrast of {} must not lower role {role:?}",
                theme.name()
            );
        }
    }
}

/// REQ: USR-303
#[test]
fn usr_303_tab_from_the_last_widget_wraps_to_the_first() {
    assert_manual_says("accessibility-and-theming", "Tab MUST never trap focus");
    let mut screen = A11yScreen::open(THREE_BUTTONS_VIEW, &A11ySettings::default());
    for _ in 0..3 {
        screen.press(Key::Tab);
    }
    assert_eq!(
        screen.focused_id(),
        "a",
        "after the last widget, Tab returns to the first"
    );
}

/// REQ: USR-304
#[test]
fn usr_304_focus_indicator_moves_with_focus_and_stays_on_exactly_one_widget() {
    assert_manual_says(
        "accessibility-and-theming",
        "exactly one widget per window MUST carry it",
    );
    let settings = A11ySettings {
        no_color: true,
        ..Default::default()
    };
    let mut screen = A11yScreen::open(THREE_BUTTONS_VIEW, &settings);
    screen.press(Key::Tab);
    assert_eq!(screen.focused_id(), "b");
    assert_eq!(
        screen.focus_indicator_count(),
        1,
        "the indicator moves with focus and stays on one widget"
    );
    let frame = screen.frame();
    assert_ne!(
        attribute_cue(&frame.style_of("Beta")),
        attribute_cue(&frame.style_of("Alpha")),
        "the newly focused widget shows the attribute cue"
    );
}

/// REQ: USR-305
#[test]
fn usr_305_second_linear_line_carries_its_position_as_2_of_3() {
    assert_manual_says(
        "accessibility-and-theming",
        "role words (button, checked, unchecked, selected, N of M)",
    );
    let settings = A11ySettings {
        screen_reader: true,
        ..Default::default()
    };
    let screen = A11yScreen::open(THREE_BUTTONS_VIEW, &settings);
    let lines = screen.frame().rows();
    assert!(
        lines[1].contains("2 of 3"),
        "the second element announces its position: {}",
        lines[1]
    );
}

/// REQ: USR-306
#[test]
fn usr_306_empty_label_on_a_non_focusable_label_is_not_refused() {
    assert_manual_says(
        "accessibility-and-theming",
        "A widget kind that can be focused (button, toggle, input, list row) MUST carry a non-empty text label",
    );
    let decorative = r#"{"widgets":[{"id":"t","kind":"label","text":""}]}"#;
    assert!(
        validate_view(decorative).is_ok(),
        "only focusable widgets need a label"
    );
}

/// REQ: USR-307
#[test]
fn usr_307_timer_of_one_second_or_more_that_only_redraws_is_not_warned() {
    assert_manual_says(
        "accessibility-and-theming",
        "a plugin timer below 1000 ms that triggers only redraws",
    );
    let settings = A11ySettings {
        reduced_motion: true,
        ..Default::default()
    };
    let slow = r#"{"widgets":[{"id":"t","kind":"label","text":"tick"}],"timer_ms":1500}"#;
    let mut ticking = A11yScreen::open(slow, &settings);
    ticking.advance(Duration::from_secs(4));
    let warnings = ticking
        .log_codes()
        .iter()
        .filter(|code| code.as_str() == "ui.motion-ignored")
        .count();
    assert_eq!(warnings, 0, "a 1500 ms timer is outside the warned range");
}
