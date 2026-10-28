//! Plugin screen failure placeholder of the page `user-plugin-health` (USR-338).

use std::time::Duration;

use crate::common::*;

/// REQ: USR-338
#[test]
fn usr_338_failed_view_dims_the_last_frame_then_shows_a_placeholder_and_leaves_other_windows_alone()
{
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let notes_window = server.open_plugin_window(NOTES);
    let other_window = server.open_plugin_window(OTHER);
    server.set_view_delay(NOTES, Duration::from_millis(250));

    rig.advance(Duration::from_secs(1));
    assert!(
        notes_window.is_dimmed_last_frame(),
        "the last good frame must be dimmed"
    );
    rig.advance(Duration::from_secs(3));
    assert!(
        notes_window.is_placeholder(),
        "a placeholder must replace the view"
    );
    let placeholder = notes_window.placeholder_text();
    assert!(
        placeholder.contains(NOTES),
        "the placeholder must name the plugin: {placeholder}"
    );
    assert!(
        placeholder.contains("view.timeout"),
        "the placeholder must name the code: {placeholder}"
    );
    let keys = notes_window.keys();
    assert!(keys.contains(&"Retry".to_string()) && keys.contains(&"Details".to_string()));
    assert!(
        !other_window.is_blank(),
        "another plugin's window must not be blanked"
    );
}

/// REQ: USR-338
#[test]
fn usr_338_placeholder_replaces_the_dimmed_frame_within_two_seconds_of_the_failure() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let notes_window = server.open_plugin_window(NOTES);
    server.set_view_delay(NOTES, Duration::from_millis(250));

    rig.advance(Duration::from_secs(1));
    assert!(
        notes_window.is_dimmed_last_frame(),
        "the last good frame must still be dimmed one second in"
    );
    rig.advance(Duration::from_secs(2));
    assert!(
        notes_window.is_placeholder(),
        "the dimmed frame must give way to the placeholder by 2 s"
    );
}
