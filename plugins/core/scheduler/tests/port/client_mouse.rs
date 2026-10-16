//! Twin of ilium-client/src/mouse.rs.

use ilium_plugin_sdk::types::Delivery;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/mouse.rs::scheduled_input_checkbox_and_button_have_real_mouse_targets, scheduler-B02, scheduler-B03
#[test]
fn port_mouse_scheduled_input_checkbox_and_button_have_real_mouse_targets() {
    let (mut session, pane) = schedule_dialog("shell");
    let screen = draw(&mut session, SCHEDULE_VIEW, 100, 30);
    let (row, column) = screen.find("[x]").expect("Send Enter checkbox");
    assert!(client(&mut session).click(popup(SCHEDULE_VIEW), column, row));
    client(&mut session).settle();
    let after = draw(&mut session, SCHEDULE_VIEW, 100, 30);
    assert!(after.text().contains("[ ]"), "the checkbox click toggles Send Enter off");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "status");
    let (button_row, button_column) = after.find("Schedule input ]").or_else(|| after.find("[ Schedule input")).expect("schedule button");
    assert!(client(&mut session).click(popup(SCHEDULE_VIEW), button_column, button_row));
    pump(&mut session);
    assert_eq!(sent_after(&mut session, 30_000), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "status", Delivery::Typed));
}
