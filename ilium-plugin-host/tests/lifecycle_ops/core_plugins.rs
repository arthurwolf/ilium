//! Core plugins page (LIF-276).

use crate::common::*;

const ESSENTIAL: [&str; 4] = [
    "ilium.core.tree",
    "ilium.core.panes",
    "ilium.core.commands",
    "ilium.core.settings",
];

/// REQ: LIF-276
#[test]
fn lif_276_the_plugins_screen_offers_no_disable_switch_for_the_four_essential_plugins() {
    let rig = rig();
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let screen = server.render_plugins_screen();
    for id in ESSENTIAL {
        let row = screen.row_for(id).unwrap_or_else(|| panic!("row for {id}"));
        assert!(!row.has_switch, "{id} has a switch");
        server.press_space_on(id);
        assert!(server.is_enabled(id), "{id} was disabled by Space");
    }
}

/// REQ: LIF-276
#[test]
fn lif_276_a_non_essential_plugin_has_a_switch() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("s1");
    assert!(
        server
            .render_plugins_screen()
            .row_for(NOTES)
            .expect("row")
            .has_switch
    );
}

/// REQ: LIF-276
#[test]
fn lif_276_the_cli_refuses_to_disable_an_essential_plugin() {
    let rig = rig();
    let _server = rig.start_server("s1");
    for id in ESSENTIAL {
        let output = rig.cli(&["disable", id]);
        assert_ne!(output.exit_code, 0, "{id}");
    }
}
