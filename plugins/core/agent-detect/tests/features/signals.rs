//! F-028 pane status signals: `signal-changed` is mirrored into the activity slot, one text per pane and signal
//! (page "Status signals and goals" and "Signals from the agent").

use ilium_plugin_sdk::types::{Event, NodeId, SignalChangedEvent};

use crate::common::{decorations, plain, server_host};

fn signal(pane: u64, name: &str, value: &str) -> Event {
    Event::SignalChanged(SignalChangedEvent {
        pane: NodeId(pane),
        signal: name.to_string(),
        value: value.to_string(),
    })
}

/// REQ: F-028
#[test]
fn feature_028_a_signal_is_shown_as_name_colon_value_in_the_activity_slot() {
    let mut host = server_host();
    let submissions = host.deliver_one(signal(4, "branch", "main"));
    let specs = decorations(&submissions, "activity");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].node, NodeId(4));
    assert_eq!(plain(&specs[0].content), "branch: main");
}

/// REQ: F-028
#[test]
fn feature_028_a_signal_submission_carries_no_dedup_key() {
    let mut host = server_host();
    let submissions = host.deliver_one(signal(4, "branch", "main"));
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].dedup_key, None);
}

/// REQ: F-028
#[test]
fn feature_028_the_latest_value_of_a_signal_wins() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        signal(4, "branch", "main"),
        signal(4, "branch", "feature"),
    ]);
    let specs = decorations(&submissions, "activity");
    assert_eq!(
        plain(&specs.last().expect("a decoration").content),
        "branch: feature"
    );
}

/// REQ: F-028
#[test]
fn feature_028_signals_of_two_panes_update_their_own_rows() {
    let mut host = server_host();
    let submissions = host.deliver(vec![signal(1, "branch", "a"), signal(2, "branch", "b")]);
    let specs = decorations(&submissions, "activity");
    let rows: Vec<(NodeId, String)> = specs
        .iter()
        .map(|spec| (spec.node, plain(&spec.content)))
        .collect();
    assert_eq!(
        rows,
        [
            (NodeId(1), "branch: a".to_string()),
            (NodeId(2), "branch: b".to_string())
        ]
    );
}

/// REQ: F-028
#[test]
fn feature_028_an_empty_signal_value_is_still_shown() {
    let mut host = server_host();
    let submissions = host.deliver_one(signal(4, "branch", ""));
    let specs = decorations(&submissions, "activity");
    assert_eq!(plain(&specs[0].content), "branch: ");
}

/// REQ: F-028
#[test]
fn feature_028_a_signal_with_non_ascii_text_keeps_it_unchanged() {
    let mut host = server_host();
    let submissions = host.deliver_one(signal(4, "modèle", "naïve ⠋"));
    let specs = decorations(&submissions, "activity");
    assert_eq!(plain(&specs[0].content), "modèle: naïve ⠋");
}

/// REQ: F-028
#[test]
fn feature_028_a_signal_does_not_touch_the_goal_slot() {
    let mut host = server_host();
    let submissions = host.deliver_one(signal(4, "branch", "main"));
    assert!(decorations(&submissions, "goal").is_empty());
}
