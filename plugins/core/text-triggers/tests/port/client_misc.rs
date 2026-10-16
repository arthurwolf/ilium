//! Twin of the execution lease: with two clients only one answers, because the host dedups a send-input by key.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, Event, Outcome, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/trigger_execution_lease.rs::only_one_client_executes_and_another_takes_over_after_drop
#[test]
fn port_001_only_one_client_executes_and_another_takes_over_after_drop() {
    let (mut first, pane) = armed("go", "yes");
    let (mut second, second_pane) = armed("go", "yes");
    let one = first.deliver_one(hit(pane, 1));
    let again = first.deliver_one(hit(pane, 1));
    assert_eq!(texts(&one), ["yes"]);
    assert_eq!(first.outcomes().iter().filter(|o| matches!(o, Outcome::Duplicate(_))).count(), 1, "{again:?}");
    drop(first);
    let takeover = second.deliver_one(hit(second_pane, 1));
    assert_eq!(texts(&takeover), ["yes"]);
}

