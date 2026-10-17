//! Small helpers shared by the port twins (on top of the feature tests' common module).

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, NodeId};

use crate::common::*;

/// The rows of the popup `view` rendered at 80x24.
pub fn rows(session: &mut FakeSession, view: &str) -> Vec<String> {
    draw(session, view, 80, 24)
        .text()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The form row (dotted leader) that carries `label`.
pub fn form_row(session: &mut FakeSession, view: &str, label: &str) -> String {
    rows(session, view)
        .into_iter()
        .find(|row| row.contains(label) && row.contains('…'))
        .unwrap_or_default()
}

/// A session with a terminal pane named `name` and the schedule dialog open.
pub fn schedule_dialog(name: &str) -> (FakeSession, NodeId) {
    let mut session = session();
    let pane = terminal_pane(&mut session, name);
    invoke_menu(&mut session, SCHEDULE_COMMAND, pane);
    (session, pane)
}

/// A session with a terminal pane named `name` and the queue dialog open.
pub fn queue_dialog(name: &str) -> (FakeSession, NodeId) {
    let mut session = session();
    let pane = terminal_pane(&mut session, name);
    invoke_menu(&mut session, QUEUE_COMMAND, pane);
    (session, pane)
}

pub fn commit_schedule(session: &mut FakeSession) {
    press(session, SCHEDULE_VIEW, "ctrl+enter");
    pump(session);
}

pub fn commit_queue(session: &mut FakeSession) {
    press(session, QUEUE_VIEW, "ctrl+enter");
    pump(session);
}

pub fn select_delivery(session: &mut FakeSession, index: u32) {
    let host = client(session);
    host.send_action(
        popup(QUEUE_VIEW),
        "delivery",
        ActionKind::Selected,
        ActionValue::Index(index),
    );
    host.settle();
}

/// Number of send-input intents the server has submitted so far.
pub fn send_count(session: &mut FakeSession) -> usize {
    sends(&server(session).submitted()).len()
}

/// Advances the server clock by `ms` and returns the number of sends so far.
pub fn sent_after(session: &mut FakeSession, ms: u64) -> usize {
    server(session).advance_clock(ms);
    send_count(session)
}

/// Whether the schedule dialog is still shown.
pub fn schedule_dialog_open(session: &mut FakeSession) -> bool {
    draw(session, SCHEDULE_VIEW, 80, 24)
        .text()
        .contains("Schedule input for")
}

pub fn queue_dialog_open(session: &mut FakeSession) -> bool {
    draw(session, QUEUE_VIEW, 80, 24)
        .text()
        .contains("Queue prompt after agent finishes")
}
