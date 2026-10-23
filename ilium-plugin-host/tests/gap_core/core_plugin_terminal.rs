//! Core-plugin gap tests for `core-plugin-terminal`: the kernel owns PTYs and vt100 state, so
//! disabling or crashing the terminal plugin keeps every PTY running at its size and input still
//! reaches the focused pane.

use ilium_plugin_api::Side;
use ilium_plugin_host::testing::ops::OpsRig;

const TERMINAL: &str = "ilium.core.terminal";

/// REQ: COR-218
#[test]
fn cor_218_disabling_the_terminal_plugin_keeps_each_pty_running_at_its_size_and_input_arrives() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let pane = server.open_pane("main");
    let size_before = server.pty_size(pane);
    server.disable(TERMINAL);
    assert!(server.pty_running(pane), "the PTY must keep running");
    assert_eq!(
        server.pty_size(pane),
        size_before,
        "the PTY must keep its size"
    );
    server.type_into_focused_pane("q");
    assert!(
        server.pane_received_input(pane).ends_with('q'),
        "typed input must still arrive"
    );
}

/// REQ: COR-218
#[test]
fn cor_218_crashing_the_terminal_plugin_does_not_kill_resize_or_detach_a_pty() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let pane = server.open_pane("main");
    let size_before = server.pty_size(pane);
    server.fail(TERMINAL, Side::Server);
    assert!(server.pty_running(pane), "the PTY must survive the crash");
    assert_eq!(
        server.pty_size(pane),
        size_before,
        "the PTY must keep its size"
    );
    assert!(!server.pty_detached(pane), "the PTY must stay attached");
}
