//! F-076: freeze panes during a restore and thaw them when all report `running`
//! (page sample "Freeze panes during a restore"). Trigger: `pane-created` with `is_restored`.

use ilium_plugin_sdk::types::{NodeId, PaneLifecycleState};

use crate::common::*;

fn three_restored() -> (ilium_plugin_sdk::testing::FakeHost, Vec<u64>) {
    let mut host = activated_server();
    let panes: Vec<u64> = (0..3)
        .map(|index| add_shell(&mut host, &format!("shell-{index}")))
        .collect();
    (host, panes)
}

/// REQ: F-076
#[test]
fn freeze_076_each_restored_pane_gets_a_freeze_pane_with_its_key() {
    let (mut host, panes) = three_restored();
    let submitted = restore_panes(&mut host, &panes);
    let frozen = freezes(&submitted);
    assert_eq!(frozen.len(), 3);
    for (index, pane) in panes.iter().enumerate() {
        assert_eq!(frozen[index].0.pane, NodeId(*pane));
        assert_eq!(frozen[index].1, Some(format!("freeze-{pane}")));
    }
}

/// REQ: F-076
#[test]
fn freeze_076_a_pane_created_live_is_not_frozen() {
    let (mut host, panes) = three_restored();
    let submitted = host.deliver(vec![pane_created(panes[0], false)]);
    assert!(freezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_only_restored_panes_of_a_mixed_batch_are_frozen() {
    let (mut host, panes) = three_restored();
    let submitted = host.deliver(vec![
        pane_created(panes[0], true),
        pane_created(panes[1], false),
        pane_created(panes[2], true),
    ]);
    let frozen: Vec<NodeId> = freezes(&submitted)
        .into_iter()
        .map(|(spec, _)| spec.pane)
        .collect();
    assert_eq!(frozen, [NodeId(panes[0]), NodeId(panes[2])]);
}

/// REQ: F-076
#[test]
fn freeze_076_nothing_is_thawed_before_any_pane_reports_running() {
    let (mut host, panes) = three_restored();
    let submitted = restore_panes(&mut host, &panes);
    assert!(unfreezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_one_running_pane_of_three_does_not_thaw() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let submitted = host.deliver(vec![lifecycle_event(panes[0], PaneLifecycleState::Running)]);
    assert!(unfreezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_two_running_panes_of_three_do_not_thaw() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let submitted = host.deliver(vec![
        lifecycle_event(panes[0], PaneLifecycleState::Running),
        lifecycle_event(panes[1], PaneLifecycleState::Running),
    ]);
    assert!(unfreezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_the_last_running_pane_thaws_every_restored_pane_with_keys() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    host.deliver(vec![lifecycle_event(panes[0], PaneLifecycleState::Running)]);
    host.deliver(vec![lifecycle_event(panes[1], PaneLifecycleState::Running)]);
    let submitted = host.deliver(vec![lifecycle_event(panes[2], PaneLifecycleState::Running)]);
    let thawed = unfreezes(&submitted);
    assert_eq!(thawed.len(), 3);
    for (index, pane) in panes.iter().enumerate() {
        assert_eq!(thawed[index].0.pane, NodeId(*pane));
        assert_eq!(thawed[index].1, Some(format!("thaw-{pane}")));
    }
}

/// REQ: F-076
#[test]
fn freeze_076_all_running_in_one_batch_thaws_once() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let events = panes
        .iter()
        .map(|pane| lifecycle_event(*pane, PaneLifecycleState::Running))
        .collect();
    let submitted = host.deliver(events);
    assert_eq!(unfreezes(&submitted).len(), 3);
}

/// REQ: F-076
#[test]
fn freeze_076_frozen_state_reports_do_not_count_as_running() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let mut events: Vec<_> = panes[..2]
        .iter()
        .map(|pane| lifecycle_event(*pane, PaneLifecycleState::Running))
        .collect();
    events.push(lifecycle_event(panes[2], PaneLifecycleState::Frozen));
    assert!(unfreezes(&host.deliver(events)).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_restarting_state_reports_do_not_count_as_running() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let mut events: Vec<_> = panes[..2]
        .iter()
        .map(|pane| lifecycle_event(*pane, PaneLifecycleState::Running))
        .collect();
    events.push(lifecycle_event(panes[2], PaneLifecycleState::Restarting));
    assert!(unfreezes(&host.deliver(events)).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_the_same_pane_running_twice_does_not_complete_the_wait() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    host.deliver(vec![lifecycle_event(panes[0], PaneLifecycleState::Running)]);
    let submitted = host.deliver(vec![lifecycle_event(panes[0], PaneLifecycleState::Running)]);
    assert!(unfreezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_running_events_after_the_thaw_submit_nothing_more() {
    let (mut host, panes) = three_restored();
    restore_panes(&mut host, &panes);
    let events = panes
        .iter()
        .map(|pane| lifecycle_event(*pane, PaneLifecycleState::Running))
        .collect();
    host.deliver(events);
    let submitted = host.deliver(vec![lifecycle_event(panes[0], PaneLifecycleState::Running)]);
    assert!(unfreezes(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn freeze_076_a_single_restored_pane_thaws_when_it_runs() {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "only");
    restore_panes(&mut host, &[pane]);
    let submitted = host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)]);
    let thawed = unfreezes(&submitted);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].1, Some(format!("thaw-{pane}")));
}

/// REQ: F-076
#[test]
fn freeze_076_running_reports_of_unrelated_panes_are_ignored() {
    let (mut host, panes) = three_restored();
    let stranger = add_shell(&mut host, "stranger");
    restore_panes(&mut host, &panes);
    let submitted = host.deliver(vec![lifecycle_event(stranger, PaneLifecycleState::Running)]);
    assert!(unfreezes(&submitted).is_empty());
}
