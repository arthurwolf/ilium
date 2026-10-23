//! Operating system accessibility values are passed to plugins unchanged (USR-952). The rule is
//! stated in accessibility-and-theming.md; the host behaviour needs the user rig and stays an
//! unimplemented! stub until the rig reads and reports the accessibility values.

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
    unimplemented!("ilium_plugin_host::testing::user_rig::UserRig::accessibility_values")
}
