//! Topic `ilium.core.terminal/screen-changed`: `{ pane, first-row, last-row, revision, generation }`
//! when visible rows change, at most 10 per second per pane, a change inside the quiet window kept
//! and published when it ends (page "Extension points", Topics).

use ilium_plugin_sdk::types::{Event, NodeId, ScreenChangedEvent, TimerId};
use serde_json::Value;

use crate::common::server_host;

const TOPIC: &str = "ilium.core.terminal/screen-changed";

fn changed(pane: u64, first_row: u32, last_row: u32, revision: u64) -> Event {
    Event::ScreenChanged(ScreenChangedEvent {
        pane: NodeId(pane),
        first_row,
        last_row,
        revision,
    })
}

fn published(host: &ilium_plugin_sdk::testing::FakeHost) -> Vec<(String, Value)> {
    host.published_topics()
        .into_iter()
        .filter(|(topic, _)| topic == TOPIC)
        .map(|(topic, payload)| {
            (
                topic,
                serde_json::from_str(&payload).expect("payload is JSON"),
            )
        })
        .collect()
}

/// REQ: F-013
#[test]
fn feature_013_a_screen_change_publishes_the_topic_with_pane_and_rows() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 2, 4, 1)]);
    let topics = published(&host);
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].1["pane"], 7);
    assert_eq!(topics[0].1["first-row"], 2);
    assert_eq!(topics[0].1["last-row"], 4);
}

/// REQ: F-013
#[test]
fn feature_013_the_topic_payload_names_pane_rows_revision_and_generation() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 0, 0, 9)]);
    let payload = &published(&host)[0].1;
    let mut keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["first-row", "generation", "last-row", "pane", "revision"]
    );
    assert_eq!(payload["revision"], 9);
    assert_eq!(payload["generation"], 9);
}

/// REQ: F-013
#[test]
fn feature_013_at_most_ten_publications_in_one_second_of_fake_time() {
    let mut host = server_host();
    for step in 0..40u64 {
        host.deliver(vec![changed(7, 0, 1, step + 1)]);
        host.advance_clock(25);
    }
    assert!(
        published(&host).len() <= 10,
        "{} publications in 1000 ms",
        published(&host).len()
    );
}

/// REQ: F-013
#[test]
fn feature_013_a_quiet_gap_of_100_ms_allows_the_next_publication() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 0, 1, 1)]);
    host.advance_clock(100);
    host.deliver(vec![changed(7, 0, 1, 2)]);
    host.settle();
    assert_eq!(published(&host).len(), 2);
}

/// REQ: F-013
#[test]
fn feature_013_an_event_for_another_pane_is_published_with_that_pane() {
    let mut host = server_host();
    host.deliver(vec![changed(11, 3, 3, 5)]);
    assert_eq!(published(&host)[0].1["pane"], 11);
}

/// REQ: F-013
#[test]
fn feature_013_no_screen_change_publishes_nothing() {
    let mut host = server_host();
    host.advance_clock(5_000);
    host.settle();
    assert!(published(&host).is_empty());
}

/// REQ: F-013
#[test]
fn feature_013_a_change_inside_the_quiet_window_is_published_when_the_timer_fires() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 4, 5, 1)]);
    host.advance_clock(30);
    host.deliver(vec![changed(7, 1, 2, 2)]);
    assert_eq!(published(&host).len(), 1, "the second change is deferred");
    let timers = host.active_timers();
    assert_eq!(timers.len(), 1, "one flush timer is armed");
    host.advance_clock(100);
    host.deliver(vec![Event::TimerFired(TimerId(u64::from(timers[0].0)))]);
    let topics = published(&host);
    assert_eq!(topics.len(), 2);
    assert_eq!(topics[1].1["first-row"], 1);
    assert_eq!(topics[1].1["last-row"], 2);
    assert_eq!(topics[1].1["revision"], 2);
}

/// REQ: F-013
#[test]
fn feature_013_deferred_rows_are_merged_into_one_publication() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 4, 5, 1)]);
    host.advance_clock(20);
    host.deliver(vec![changed(7, 8, 9, 2)]);
    host.advance_clock(20);
    host.deliver(vec![changed(7, 0, 1, 3)]);
    assert_eq!(
        host.active_timers().len(),
        1,
        "one timer serves both deferrals"
    );
    host.advance_clock(100);
    host.deliver(vec![changed(7, 6, 6, 4)]);
    let topics = published(&host);
    assert_eq!(topics.len(), 2);
    assert_eq!(topics[1].1["first-row"], 0);
    assert_eq!(topics[1].1["last-row"], 9);
    assert_eq!(topics[1].1["revision"], 4);
}

/// REQ: F-013
#[test]
fn feature_013_panes_are_limited_independently() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 0, 0, 1), changed(8, 0, 0, 1)]);
    assert_eq!(published(&host).len(), 2);
}
