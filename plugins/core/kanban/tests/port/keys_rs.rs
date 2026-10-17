//! Twin of the board dialog key test of ilium-client/src/keys.rs. The Create Board dialog is the plugin's
//! popup `open-board`: plain `p` is typed into the Markdown file field and `ctrl+p` starts the file picker
//! (the Browse button's `fs.pick`); cancelling the picker returns to the popup with the typed text kept.

use ilium_plugin_sdk::types::{Outcome, ViewTarget};

use crate::common::{client_host_with, rows_of};

/// REQ: F-007, PORT:ilium-client/src/keys.rs::board_dialog_types_plain_p_and_reserves_control_p_for_browse
#[test]
fn port_keys_board_dialog_types_plain_p_and_reserves_control_p_for_browse() {
    let popup = ViewTarget::Popup("open-board".to_string());
    let mut host = client_host_with(None);
    host.render(popup.clone(), 48, 8);
    host.key(popup.clone(), "p");
    host.settle();
    assert_eq!(
        host.pending_requests().len(),
        0,
        "a plain p starts no picker"
    );
    let typed = rows_of(&host.render(popup.clone(), 48, 8));
    assert!(
        typed
            .iter()
            .any(|row| row.contains("Markdown file") && row.contains('p')),
        "{typed:?}"
    );

    host.key(popup.clone(), "ctrl+p");
    host.settle();
    assert_eq!(
        host.pending_requests().len(),
        1,
        "ctrl+p starts the one fs.pick request"
    );
    let request = host
        .pending_requests()
        .into_iter()
        .next()
        .expect("pick request");
    host.complete(request, Outcome::Cancelled);
    host.settle();
    let back = rows_of(&host.render(popup, 48, 8));
    assert!(
        back.iter().any(|row| row.contains("Open board from file")),
        "{back:?}"
    );
    assert!(
        back.iter()
            .any(|row| row.contains("Markdown file") && row.contains('p')),
        "{back:?}"
    );
}
