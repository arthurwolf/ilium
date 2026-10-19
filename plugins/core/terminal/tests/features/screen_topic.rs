//! Topic `ilium.core.terminal/screen-changed`: `{ pane, rows }` when visible rows change, at most
//! 10 per second (page "Extension points", Topics).

use ilium_plugin_sdk::types::{Event, NodeId, ScreenChangedEvent};
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
    assert!(topics[0].1.get("rows").is_some());
}

/// REQ: F-013
#[test]
fn feature_013_the_topic_payload_has_exactly_pane_and_rows() {
    let mut host = server_host();
    host.deliver(vec![changed(7, 0, 0, 1)]);
    let payload = &published(&host)[0].1;
    let mut keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["pane", "rows"]);
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
