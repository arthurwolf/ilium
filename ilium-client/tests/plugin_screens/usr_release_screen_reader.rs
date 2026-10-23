//! Release gate for the screen reader (USR-950, USR-951). The rules are in accessibility-and-theming.md.
//! The screen-reader run itself is a tester's release step on each platform; these tests check
//! the procedure and the release-note rule as written, and the reading behaviour needs the screens
//! rig with a screen-reader mode, which does not exist yet (see API_GAPS.md).

fn manual_page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn states(page: &str, fact: &str) {
    assert!(page.contains(fact), "the page must state: {fact}");
}

/// REQ: USR-950
#[test]
fn usr_950_the_release_notes_name_the_reader_and_terminal_for_each_platform() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "The release notes MUST name, for each shipped platform, the screen reader and the terminal emulator",
    );
}

/// REQ: USR-950
#[test]
fn usr_950_a_platform_without_a_named_reader_is_not_called_screen_reader_supported() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "A platform with no named reader MUST NOT be described as screen-reader supported in any user interface or document.",
    );
}

/// REQ: USR-951
#[test]
fn usr_951_the_consent_and_settings_check_runs_at_48_columns_with_the_reader_flag() {
    let page = manual_page("accessibility-and-theming");
    states(&page, "`[ui] screen_reader = true`");
    states(&page, "at 48 columns");
}

/// REQ: USR-951
#[test]
fn usr_951_the_five_reader_steps_are_all_required_to_pass() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "(1) each focusable element is read on its own line, in focus order",
    );
    states(
        &page,
        "(2) each element is read with its role word and state (button, checked, unchecked, selected, N of M)",
    );
    states(
        &page,
        "(3) no box-drawing character or spinner frame is read",
    );
    states(
        &page,
        "(4) after each Tab the cursor sits on the first cell of the focused element",
    );
    states(
        &page,
        "(5) the reader announces the focus change without the tester moving the cursor",
    );
}

/// REQ: USR-951
#[test]
fn usr_951_a_failed_step_blocks_the_release_and_the_record_names_the_view() {
    let page = manual_page("accessibility-and-theming");
    states(
        &page,
        "A failed step blocks the release and the release record names the view and the step.",
    );
}

/// REQ: USR-951
#[test]
fn usr_951_each_focusable_element_is_read_on_its_own_line_in_focus_order() {
    unimplemented!("ilium_client::plugin_host::screens_rig::ScreensRig::set_screen_reader")
}

/// REQ: USR-951
#[test]
fn usr_951_no_box_drawing_or_spinner_frame_is_spoken_on_the_consent_dialog() {
    unimplemented!("ilium_client::plugin_host::screens_rig::ScreensRig::speech_lines")
}
