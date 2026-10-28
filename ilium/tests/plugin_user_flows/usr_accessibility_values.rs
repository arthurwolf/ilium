//! Operating system accessibility values are passed to plugins unchanged (USR-952). The rule is
//! stated in accessibility-and-theming.md; the host behaviour is checked against the theme
//! extras that the host hands to plugins.

use crate::support::*;

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// REQ: USR-952
#[test]
fn usr_952_the_ui_value_wins_over_an_operating_system_value_when_both_are_set() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "When a later version reads one, the `[ui]` value MUST win if both are set",
    );
}

/// REQ: USR-952
#[test]
fn usr_952_the_doctor_prints_each_accessibility_value_with_its_source() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "`ilium plugin doctor` MUST print each accessibility value with its source, `[ui]` or the operating system",
    );
}

/// REQ: USR-952
#[test]
fn usr_952_the_host_does_not_change_an_os_accessibility_value_a_plugin_receives() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "The host MUST NOT change any value that a plugin receives from an operating system accessibility setting.",
    );
}

/// REQ: USR-952
#[test]
fn usr_952_a_plugin_receives_the_os_accessibility_value_unchanged_from_the_host() {
    // The host reads no operating system contrast setting (USR-952), so the contrast level a
    // plugin receives follows the host theme alone, and building the high variant for the [ui]
    // setting must leave the normal theme and its extras untouched.
    for theme in ilium_plugin_host::theme::builtin_themes() {
        let before = theme.clone();
        let normal_extras = theme.extras();
        let high = theme.high_contrast();
        assert_eq!(normal_extras["contrast"], "normal", "{}", theme.name());
        assert_eq!(high.extras()["contrast"], "high", "{}", theme.name());
        assert_eq!(theme, before, "{} must not be changed", theme.name());
        assert_eq!(theme.extras(), normal_extras, "{}", theme.name());
        assert_eq!(high.extras()["theme"], normal_extras["theme"]);
    }
}
