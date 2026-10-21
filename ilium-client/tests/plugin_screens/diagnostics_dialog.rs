//! Diagnostics "Build support bundle" dialog: USR-226 of reference-requirements-user-admin.md
//! (core-plugin-diagnostics.md, "Support bundle"). Each test calls an unimplemented! rig stub
//! until the dialog lands.

use crate::support::*;

// ---- USR-226 ----

/// REQ: USR-226
#[test]
fn usr_226_the_dialog_lists_the_files_and_their_size_before_writing() {
    let mut rig = ScreensRig::new(80, 30);
    rig.install_plugin("example.notes", "T2");
    rig.open_support_bundle_dialog("example.notes");
    let screen = rig.screen_text();
    for file in ["crashes.txt", "log.txt", "stats.txt", "summary.txt"] {
        assert!(screen.contains(file), "file {file} not listed:\n{screen}");
    }
    assert!(screen.contains("Size"), "size not shown:\n{screen}");
}

/// REQ: USR-226
#[test]
fn usr_226_the_default_button_is_cancel() {
    let mut rig = ScreensRig::new(80, 30);
    rig.install_plugin("example.notes", "T2");
    rig.open_support_bundle_dialog("example.notes");
    assert_eq!(rig.default_button_label().as_deref(), Some("Cancel"));
}
