//! Pane events and restored panes (api-panes.md): PAN-008, 010, 011, 012.

use ilium_plugin_api::generated::types::{
    Delivery, ErrorCode, Intent, NodeId, Outcome, SendInputSpec, Signal, SignalPaneSpec,
};

use crate::common::*;

const PANES: &[&str] = &["panes.read", "terminal.write", "panes.signal", "panes.lifecycle"];

fn send_input(pane: NodeId) -> Intent {
    Intent::SendInput(SendInputSpec { pane, text: "x".into(), delivery: Delivery::Raw, prompt_epoch: None, stages: Vec::new() })
}

fn signal(pane: NodeId) -> Intent {
    Intent::SignalPane(SignalPaneSpec { pane, signal: Signal::Interrupt })
}

/// REQ: PAN-008
#[test]
fn pan_008_closing_a_running_pane_orders_pane_exited_then_node_removed_and_nothing_after() {
    let kernel = world(PANES);
    submit(&kernel, Intent::CloseNode(RUNNING_PANE));
    let names = kernel.event_names_for(RUNNING_PANE);
    let exited = names.iter().position(|n| n == "pane-exited").expect("pane-exited");
    let removed = names.iter().position(|n| n == "node-removed").expect("node-removed");
    assert!(exited < removed, "{names:?}");
    assert_eq!(removed, names.len() - 1, "nothing may follow node-removed: {names:?}");
}

/// REQ: PAN-008
#[test]
fn pan_008_pane_created_is_the_first_event_of_a_node() {
    let kernel = world(PANES);
    let created = kernel.create_pane_for_test();
    assert_eq!(kernel.event_names_for(created).first().map(String::as_str), Some("pane-created"));
}

/// REQ: PAN-008
#[test]
fn pan_008_no_tree_changed_for_the_node_after_node_removed() {
    let kernel = world(PANES);
    submit(&kernel, Intent::CloseNode(RUNNING_PANE));
    let names = kernel.event_names_for(RUNNING_PANE);
    let removed = names.iter().position(|n| n == "node-removed").expect("node-removed");
    assert!(names[removed..].iter().all(|n| n != "tree-changed"), "{names:?}");
}

/// REQ: PAN-010
#[test]
fn pan_010_restored_pane_exists_with_saved_title_and_size_before_its_process_starts() {
    let kernel = world(PANES);
    let info = kernel.pane_info(RESTORED_PANE).expect("restored pane is in the tree");
    assert!(info.is_restored);
    assert_eq!(info.title, "restored title");
    assert_eq!((info.columns, info.rows), (100, 30));
}

/// REQ: PAN-010
#[test]
fn pan_010_a_restored_pane_never_reports_a_zero_size() {
    let kernel = world(PANES);
    kernel.restore_pane_without_saved_size(RESTORED_PANE);
    let info = kernel.pane_info(RESTORED_PANE).expect("pane");
    assert!(info.columns > 0 && info.rows > 0, "{}x{}", info.columns, info.rows);
}

/// REQ: PAN-011
#[test]
fn pan_011_send_input_to_a_restored_pane_refused_pane_not_ready() {
    let kernel = world(PANES);
    assert_eq!(refusal(&submit(&kernel, send_input(RESTORED_PANE))), Some(ErrorCode::PaneNotReady));
}

/// REQ: PAN-011
#[test]
fn pan_011_signal_to_a_restored_pane_refused_pane_not_ready() {
    let kernel = world(PANES);
    assert_eq!(refusal(&submit(&kernel, signal(RESTORED_PANE))), Some(ErrorCode::PaneNotReady));
}

/// REQ: PAN-011
#[test]
fn pan_011_signal_to_an_exited_pane_refused_not_found_no_running_process() {
    let kernel = world(PANES);
    let outcome = submit(&kernel, signal(EXITED_PANE));
    assert_eq!(refusal(&outcome), Some(ErrorCode::NotFound));
    assert!(message(&outcome).contains("no running process"), "{}", message(&outcome));
}

/// REQ: PAN-011
#[test]
fn pan_011_send_input_to_a_running_pane_accepted() {
    let kernel = world(PANES);
    assert!(matches!(submit(&kernel, send_input(RUNNING_PANE)), Outcome::Ok));
}

/// REQ: PAN-012
#[test]
fn pan_012_is_restored_is_false_once_the_process_started() {
    let kernel = world(PANES);
    kernel.start_restored_process(RESTORED_PANE);
    assert!(!kernel.pane_info(RESTORED_PANE).expect("pane").is_restored);
}

/// REQ: PAN-012
#[test]
fn pan_012_is_restored_stays_true_before_the_process_starts() {
    let kernel = world(PANES);
    assert!(kernel.pane_info(RESTORED_PANE).expect("pane").is_restored);
}
