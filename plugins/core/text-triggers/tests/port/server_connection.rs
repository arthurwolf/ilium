//! Twins of ilium-server ipc/connection.rs and encoded_storage_tests.rs: rule distribution to panes and clients, ordering, and stale payloads.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/ipc/connection.rs::text_trigger_attach_and_live_broadcast_use_current_authority
#[test]
fn port_001_text_trigger_attach_and_live_broadcast_use_current_authority() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["r"]);
    assert_eq!(filter_regexes(&host, pane), ["a"]);
    set_rules(&mut host, &["a", "b"], &["r", "s"]);
    assert_eq!(filter_regexes(&host, pane), ["a", "b"]);
    let mut client = client_host();
    client.kernel_settings().set_tab(
        "text_triggers",
        host.kernel_settings().tab("text_triggers").expect("table"),
    );
    client.settle();
    assert_eq!(client.settings().get("patterns"), Some(json!(["a", "b"])));
}

/// REQ: PORT:ilium-server/src/ipc/connection.rs::text_trigger_lag_repair_then_old_event_cannot_regress_settings
#[test]
fn port_002_text_trigger_lag_repair_then_old_event_cannot_regress_settings() {
    let (mut host, pane) = armed("go", "yes");
    set_rules(&mut host, &["go", "second"], &["yes", "no"]);
    set_rules(&mut host, &["go"], &["yes"]);
    let stale = host.deliver_one(matched(pane, &rule_id(1), "second", 1));
    assert!(send_inputs(&stale).is_empty());
    assert_eq!(host.settings().get("patterns"), Some(json!(["go"])));
}

/// REQ: PORT:ilium-server/src/ipc/connection.rs::text_trigger_reader_exit_drain_cannot_restore_a_stale_payload
#[test]
fn port_003_text_trigger_reader_exit_drain_cannot_restore_a_stale_payload() {
    let (mut host, pane) = armed("go", "yes");
    set_rules(&mut host, &[], &[]);
    assert!(filter_ids(&host, pane).is_empty());
    let late = host.deliver_one(hit(pane, 1));
    assert!(send_inputs(&late).is_empty());
    assert_eq!(host.settings().get("patterns"), Some(json!([])));
}

/// REQ: PORT:ilium-server/src/ipc/connection.rs::text_trigger_stalled_socket_releases_settings_and_orders_later_frames
#[test]
fn port_004_text_trigger_stalled_socket_releases_settings_and_orders_later_frames() {
    let (mut host, pane) = armed("go", "yes");
    host.settings()
        .set("replies", json!(["first"]))
        .expect("accepted");
    host.settings()
        .set("replies", json!(["second"]))
        .expect("accepted");
    host.settle();
    let submissions = host.deliver_one(hit(pane, 1));
    assert_eq!(texts(&submissions), ["second"]);
}

/// REQ: PORT:ilium-server/src/ipc/connection.rs::text_trigger_writer_refuses_snapshots_without_authority
#[test]
fn port_005_text_trigger_writer_refuses_snapshots_without_authority() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    host.revoke("terminal.output-filter");
    set_rules(&mut host, &["a"], &["r"]);
    assert!(host.output_filters().iter().all(|(node, _)| *node != pane));
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: PORT:ilium-server/src/ipc/encoded_storage_tests.rs::authoritative_text_trigger_snapshot_replaces_an_undersized_queue_lease
#[test]
fn port_006_authoritative_text_trigger_snapshot_replaces_an_undersized_queue_lease() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["a"], &["r"]);
    let (patterns, replies) = many_patterns(MAX_RULES);
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    let reply_refs: Vec<&str> = replies.iter().map(String::as_str).collect();
    set_rules(&mut host, &refs, &reply_refs);
    assert_eq!(filter_ids(&host, pane).len(), MAX_RULES);
}
