/// REQ: PDT-017
#[test]
fn pdt_017_scene_manifest_requires_description() {
    // Observable: Install a scene plugin whose manifest lacks `description`: the exit code is non-zero and the output contains `manifest.description-missing`.
    panic!(
        "PDT-017 not implemented: A scene or glyph-plane pane MUST declare a `description` of 1 to 200 characters in its manifest, and install MUST refuse a manifest without one with `manifest.description-missing`."
    );
}

/// REQ: PDT-018
#[test]
fn pdt_018_screen_reader_prints_description_and_state_line() {
    // Observable: Enable screen-reader mode and open a scene pane in the harness: the linear output contains the manifest description followed by the plugin's state line and no cell grid.
    panic!(
        "PDT-018 not implemented: In screen-reader mode the host MUST print the description and the current scene state line instead of cells."
    );
}

/// REQ: PDT-019
#[test]
fn pdt_019_reduced_motion_limits_scene_frame_rate() {
    // Observable: Enable reduced motion and run a scene for 10 s in the harness: the frame counter is at most 10.
    panic!(
        "PDT-019 not implemented: With reduced motion on, a scene MUST be limited to at most 1 frame per second or one static frame."
    );
}

/// REQ: PDT-020
#[test]
fn pdt_020_scene_controls_keyboard_reachable_and_lint_rejects_mouse_only() {
    // Observable: Declare a scene control with a pointer binding and no key binding: `ilium plugin lint` prints `ui.mouse-only-control` naming the control.
    panic!(
        "PDT-020 not implemented: Every scene control MUST be reachable with the keyboard only, and `ilium plugin lint` MUST reject a mouse-only control with `ui.mouse-only-control`."
    );
}

/// REQ: PDT-021
#[test]
fn pdt_021_scene_state_line_conveys_required_information() {
    // Observable: Change the scene to an alert state in the harness: the state line returned by `scene.state` changes in the same frame as the cells.
    panic!(
        "PDT-021 not implemented: A scene MUST NOT convey required information in cells alone; every required state MUST appear in the state line."
    );
}
