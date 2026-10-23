//! Host-drawn title and tier badge of the page `user-plugin-security-and-privacy` (USR-339).

use crate::common::*;

/// REQ: USR-339
#[test]
fn usr_339_t2_window_keeps_a_host_title_with_name_and_badge_that_the_plugin_cannot_cover() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").tier(2));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let window = server.open_plugin_window(NOTES);
    let title = window.title_line();
    assert!(
        title.contains(NOTES),
        "the title must name the plugin: {title}"
    );
    assert!(
        title.contains("T2"),
        "the tier badge must be shown: {title}"
    );

    window.plugin_draw_on_row(0, "HIJACK TITLE");
    assert_eq!(
        window.title_line(),
        title,
        "a plugin must not change or cover the title"
    );
    assert!(
        !window.row_text(0).contains("HIJACK"),
        "a plugin must not draw into the row holding the title"
    );
}
