//! Tests of [HST-100]..[HST-125] (reference-host-ipc.md). Every number below is copied from the
//! manual page; the expected error codes are the ids of its "Errors" table.

use std::cell::Cell;

use ilium_plugin_host::ipc::{
    check_payload, encode_checked, DedupVerdict, DetachedQueue, ForwardedDedup, OutcomeStore,
    PayloadKind, PluginRecord, PushOutcome, QueueVerdict, ReceiverConnection, WriterQueues,
    Written, CLIENT_TOO_SLOW_BYTES, DEDUP_WINDOW_MS, DETACHED_QUEUE_LIMIT, ENVELOPE_MAX_BYTES,
    EXIT_CODE_PROTOCOL_MISMATCH, OUTCOME_GRACE_MS, OUTCOME_RECORD_LIMIT, PLUGIN_QUEUE_MAX_BYTES,
    PLUGIN_QUEUE_MAX_RECORDS, PTY_FRAMES_PER_PLUGIN_ENVELOPE,
};
use ilium_plugin_host::testing::ipc_harness::{ClientDelivery, IpcHarness, WireItem};
use proptest::prelude::*;

use crate::common::*;

const SLASH_ERR: &str = "ipc.payload-too-large";

fn page_text() -> String {
    let path = manual_pages_dir().join("reference-host-ipc.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
}

// ---------------------------------------------------------------- HST-100 / HST-101 (page)

/// REQ: HST-100
#[test]
fn hst_100_attached_session_uses_a_single_connection_for_plugin_traffic() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.put_contribution(PLUGIN, "status.a", 16);
    harness.pump(ROUNDS);
    assert_eq!(harness.connection_count(), 1);
    assert!(harness.is_open(conn));
}

/// REQ: HST-100
#[test]
fn doc_hst_100_page_names_the_four_variants_and_unchanged_framing() {
    let text = page_text();
    for name in [
        "ClientRequest::Plugin(PluginEnvelope)",
        "ServerEvent::Plugin(PluginEnvelope)",
        "ServerEvent::Contributions(ContributionDelta)",
        "ServerEvent::Rejected(Rejection)",
    ] {
        assert!(text.contains(name), "page lacks {name}");
    }
    assert!(text.contains("This page adds no new connection and no new framing."));
}

/// REQ: HST-101
#[test]
fn doc_hst_101_envelope_body_listing_has_seven_variants_and_no_hook_call() {
    let text = page_text();
    let start = text
        .find("pub enum EnvelopeBody {")
        .expect("EnvelopeBody listing");
    let block = &text[start..];
    let block = &block[..block.find("\n}").expect("end of listing")];
    let variants: Vec<&str> = block
        .lines()
        .skip(1)
        .filter_map(|line| line.trim().split('(').next())
        .filter(|name| !name.is_empty())
        .collect();
    assert_eq!(
        variants,
        [
            "Submit",
            "Outcome",
            "Message",
            "ClientRequest",
            "ClientReply",
            "Event",
            "Control"
        ]
    );
    assert!(variants.iter().all(|name| !name.contains("Hook")));
}

// ---------------------------------------------------------------- HST-102

/// REQ: HST-102
#[test]
fn hst_102_sequence_starting_at_one_and_counting_up_is_accepted() {
    let mut receiver = ReceiverConnection::new();
    for seq in 1..=5u64 {
        assert!(receiver.on_envelope(seq, 100).is_ok(), "seq {seq}");
    }
    assert!(receiver.is_open());
}

/// REQ: HST-102
#[test]
fn hst_102_gap_closes_connection_with_sequence_gap() {
    let mut receiver = ReceiverConnection::new();
    receiver.on_envelope(1, 10).expect("seq 1");
    receiver.on_envelope(2, 10).expect("seq 2");
    let fault = receiver.on_envelope(4, 10).expect_err("gap at 4");
    assert_eq!(fault.code(), "ipc.sequence-gap");
    assert!(!receiver.is_open());
    assert_eq!(receiver.closed_with().as_deref(), Some("ipc.sequence-gap"));
}

/// REQ: HST-102
#[test]
fn hst_102_first_envelope_with_seq_two_is_a_gap() {
    let mut receiver = ReceiverConnection::new();
    let fault = receiver
        .on_envelope(2, 10)
        .expect_err("first seq must be 1");
    assert_eq!(fault.code(), "ipc.sequence-gap");
}

/// REQ: HST-102
#[test]
fn hst_102_repeated_seq_is_a_gap() {
    let mut receiver = ReceiverConnection::new();
    receiver.on_envelope(1, 10).expect("seq 1");
    let fault = receiver.on_envelope(1, 10).expect_err("repeat of 1");
    assert_eq!(fault.code(), "ipc.sequence-gap");
}

/// REQ: HST-102
#[test]
fn hst_102_envelope_after_close_is_refused() {
    let mut receiver = ReceiverConnection::new();
    receiver.on_envelope(1, 10).expect("seq 1");
    receiver.on_envelope(3, 10).expect_err("gap");
    assert!(receiver.on_envelope(4, 10).is_err());
    assert!(!receiver.is_open());
}

/// REQ: HST-102
#[test]
fn hst_102_each_direction_counts_on_its_own() {
    let mut toward_server = ReceiverConnection::new();
    let mut toward_client = ReceiverConnection::new();
    for seq in 1..=3u64 {
        toward_server
            .on_envelope(seq, 10)
            .expect("client to server");
    }
    assert!(toward_client.on_envelope(1, 10).is_ok());
}

/// REQ: HST-102
#[test]
fn hst_102_server_closes_a_connection_whose_client_skips_a_seq() {
    let (mut harness, conn) = attached();
    harness.send_client_envelope(conn, 1);
    harness.send_client_envelope(conn, 3);
    harness.pump(ROUNDS);
    assert!(!harness.is_open(conn));
    assert_eq!(
        harness.close_code(conn).as_deref(),
        Some("ipc.sequence-gap")
    );
}

proptest! {
    // REQ: HST-102
    #[test]
    fn hst_102_any_run_from_one_is_accepted_and_any_skip_closes(
        length in 1u64..40,
        skip_at in 1u64..40,
    ) {
        let mut receiver = ReceiverConnection::new();
        let mut closed = false;
        for position in 1..=length {
            let seq = if position >= skip_at { position + 1 } else { position };
            let result = receiver.on_envelope(seq, 1);
            if position >= skip_at {
                let error = result.expect_err("skip");
                prop_assert_eq!(error.code(), "ipc.sequence-gap");
                closed = true;
                break;
            }
            prop_assert!(result.is_ok());
        }
        prop_assert_eq!(receiver.is_open(), !closed);
    }
}

// ---------------------------------------------------------------- HST-103 / HST-104 / HST-105

/// REQ: HST-103
#[test]
fn hst_103_matching_hello_is_attached_with_snapshot_and_no_rejection() {
    let (harness, conn) = attached();
    let wire = harness.wire(conn);
    assert!(matches!(wire.first(), Some(WireItem::Attached { .. })));
    assert!(wire.iter().any(is_snapshot));
    assert!(!wire
        .iter()
        .any(|item| matches!(item, WireItem::Rejected { .. })));
    assert!(harness.is_open(conn));
}

/// REQ: HST-103
#[test]
fn hst_103_plugin_protocol_mismatch_is_rejected_and_closed() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.plugin_protocol = 2;
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    assert_eq!(
        harness.wire(conn),
        vec![WireItem::Rejected {
            code: "ipc.protocol-mismatch".to_owned()
        }]
    );
    assert!(!harness.is_open(conn));
}

/// REQ: HST-103
#[test]
fn hst_103_contract_mismatch_is_rejected_and_closed() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.contract = "0.2.0".to_owned();
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    assert_eq!(
        harness.wire(conn),
        vec![WireItem::Rejected {
            code: "ipc.protocol-mismatch".to_owned()
        }]
    );
    assert!(!harness.is_open(conn));
}

/// REQ: HST-103
#[test]
fn hst_103_plain_attach_with_mismatch_is_rejected_like_attach_interactive() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.plugin_protocol = 0;
    let conn = harness.connect("client-1", false, hello);
    harness.pump(ROUNDS);
    assert_eq!(
        harness.wire(conn),
        vec![WireItem::Rejected {
            code: "ipc.protocol-mismatch".to_owned()
        }]
    );
}

/// REQ: HST-103
#[test]
fn hst_103_mismatch_sends_no_snapshot_and_no_attached() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.plugin_protocol = 9;
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    let wire = harness.wire(conn);
    assert!(!wire.iter().any(is_snapshot));
    assert!(!wire
        .iter()
        .any(|item| matches!(item, WireItem::Attached { .. })));
}

/// REQ: HST-104
#[test]
fn hst_104_exit_code_constant_is_76() {
    assert_eq!(EXIT_CODE_PROTOCOL_MISMATCH, 76);
}

/// REQ: HST-104
#[test]
fn hst_104_client_prints_cause_and_exits_76_without_drawing() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.plugin_protocol = 2;
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    let exit = harness.client_exit(conn).expect("client exits on mismatch");
    assert_eq!(exit.code, 76);
    assert_eq!(
        exit.message,
        "client and server speak different plugin protocols; start the matching client"
    );
    assert!(!exit.interface_drawn);
}

/// REQ: HST-104
#[test]
fn hst_104_matching_client_does_not_exit() {
    let (harness, conn) = attached();
    assert!(harness.client_exit(conn).is_none());
}

/// REQ: HST-105
#[test]
fn hst_105_version_skew_disables_only_the_client_half() {
    let mut harness = server_with_demo();
    harness.install_server_plugin(OTHER_PLUGIN, "1.0.0", [2; 32]);
    let mut hello = harness.matching_hello();
    for installed in &mut hello.installed {
        if installed.id == PLUGIN {
            installed.version = "0.9.0".to_owned();
        }
    }
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    let skewed = harness.client_half(conn, PLUGIN);
    assert!(!skewed.running);
    assert_eq!(skewed.disabled_code.as_deref(), Some("plugin.version-skew"));
    assert!(harness.server_half_running(PLUGIN));
    assert!(harness.client_half(conn, OTHER_PLUGIN).running);
    assert!(harness.server_half_running(OTHER_PLUGIN));
    assert!(harness.is_open(conn));
}

/// REQ: HST-105
#[test]
fn hst_105_content_hash_difference_is_a_version_skew() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello.installed[0].content_hash = [9; 32];
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    assert_eq!(
        harness.client_half(conn, PLUGIN).disabled_code.as_deref(),
        Some("plugin.version-skew")
    );
}

/// REQ: HST-105
#[test]
fn hst_105_plugin_missing_on_the_server_disables_the_client_half() {
    let mut harness = server_with_demo();
    let mut hello = harness.matching_hello();
    hello
        .installed
        .push(ilium_plugin_host::testing::ipc_harness::InstalledSpec {
            id: OTHER_PLUGIN.to_owned(),
            version: "1.0.0".to_owned(),
            content_hash: [5; 32],
        });
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    let half = harness.client_half(conn, OTHER_PLUGIN);
    assert!(!half.running);
    assert_eq!(half.disabled_code.as_deref(), Some("plugin.server-missing"));
    assert!(harness.client_half(conn, PLUGIN).running);
}

/// REQ: HST-105
#[test]
fn hst_105_plugin_missing_on_the_client_keeps_the_server_half_running() {
    let mut harness = server_with_demo();
    harness.install_server_plugin(OTHER_PLUGIN, "1.0.0", [2; 32]);
    let mut hello = harness.matching_hello();
    hello
        .installed
        .retain(|installed| installed.id != OTHER_PLUGIN);
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    assert!(harness.server_half_running(OTHER_PLUGIN));
    assert!(harness.is_open(conn));
    assert!(harness.client_exit(conn).is_none());
}

// ---------------------------------------------------------------- HST-107 / HST-108 / HST-109

/// REQ: HST-107
#[test]
fn hst_107_first_contribution_message_is_snapshot_with_sequence_one() {
    let (harness, conn) = attached();
    let first = harness
        .wire(conn)
        .into_iter()
        .find(|item| matches!(item, WireItem::Snapshot { .. } | WireItem::Increment { .. }))
        .expect("a contribution message");
    assert!(matches!(first, WireItem::Snapshot { sequence: 1, .. }));
}

/// REQ: HST-107
#[test]
fn hst_107_later_messages_are_increments_with_consecutive_sequences() {
    let (mut harness, conn) = attached();
    for index in 0..3 {
        harness.put_contribution(PLUGIN, &format!("status.{index}"), 8);
    }
    harness.pump(ROUNDS);
    let sequences: Vec<u64> = harness
        .wire(conn)
        .iter()
        .filter_map(|item| match item {
            WireItem::Increment { sequence, .. } => Some(*sequence),
            _ => None,
        })
        .collect();
    assert_eq!(sequences, [2, 3, 4]);
}

/// REQ: HST-107
#[test]
fn hst_107_no_increment_precedes_the_snapshot() {
    let mut harness = server_with_demo();
    harness.put_contribution(PLUGIN, "status.early", 8);
    let hello = harness.matching_hello();
    let conn = harness.connect("client-1", true, hello);
    harness.put_contribution(PLUGIN, "status.late", 8);
    harness.pump(ROUNDS);
    let wire = harness.wire(conn);
    let snapshot = position(&wire, is_snapshot).expect("snapshot");
    let first_increment =
        position(&wire, |item| matches!(item, WireItem::Increment { .. })).expect("increment");
    assert!(snapshot < first_increment);
}

/// REQ: HST-108
#[test]
fn hst_108_sequence_gap_makes_the_client_resync_and_the_server_send_a_snapshot() {
    let (mut harness, conn) = attached();
    harness.put_contribution(PLUGIN, "status.a", 8);
    harness.drop_next_increment(conn);
    harness.put_contribution(PLUGIN, "status.b", 8);
    harness.put_contribution(PLUGIN, "status.c", 8);
    harness.pump(ROUNDS);
    assert_eq!(harness.resync_requests(conn), 1);
    let snapshots = harness
        .wire(conn)
        .iter()
        .filter(|item| is_snapshot(item))
        .count();
    assert_eq!(snapshots, 2);
    let mut mirror = harness.client_mirror(conn);
    let mut server = harness.server_contributions();
    mirror.sort();
    server.sort();
    assert_eq!(mirror, server);
}

/// REQ: HST-108
#[test]
fn hst_108_without_a_gap_the_client_never_resyncs() {
    let (mut harness, conn) = attached();
    harness.put_contribution(PLUGIN, "status.a", 8);
    harness.put_contribution(PLUGIN, "status.b", 8);
    harness.pump(ROUNDS);
    assert_eq!(harness.resync_requests(conn), 0);
}

/// REQ: HST-109
#[test]
fn hst_109_delta_of_exactly_4_mib_is_accepted_and_one_byte_more_is_refused() {
    let cap = 4 * MIB;
    assert!(check_payload(PayloadKind::ContributionDelta, cap - 1).is_ok());
    assert!(check_payload(PayloadKind::ContributionDelta, cap).is_ok());
    let fault = check_payload(PayloadKind::ContributionDelta, cap + 1).expect_err("over cap");
    assert_eq!(fault.code(), SLASH_ERR);
}

/// REQ: HST-109
#[test]
fn hst_109_a_larger_change_is_split_into_increments_of_at_most_4_mib() {
    let (mut harness, conn) = attached();
    for index in 0..10 {
        harness.put_contribution(PLUGIN, &format!("big.{index}"), MIB);
    }
    harness.pump(ROUNDS);
    let increments: Vec<(u64, usize)> = harness
        .wire(conn)
        .iter()
        .filter_map(|item| match item {
            WireItem::Increment { sequence, bytes } => Some((*sequence, *bytes)),
            _ => None,
        })
        .collect();
    assert!(!increments.is_empty());
    assert!(increments.iter().all(|(_, bytes)| *bytes <= 4 * MIB));
    for pair in increments.windows(2) {
        assert_eq!(pair[1].0, pair[0].0 + 1);
    }
    assert!(increments.iter().map(|(_, bytes)| *bytes).sum::<usize>() >= 10 * MIB);
}

// ---------------------------------------------------------------- HST-110 .. HST-115

/// REQ: HST-110
#[test]
fn hst_110_tree_change_precedes_request_completed_on_the_same_connection() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    let wire = harness.wire(conn);
    let tree = position(&wire, is_tree_change).expect("tree change");
    let outcome = position(&wire, is_outcome).expect("outcome");
    assert!(tree < outcome);
}

/// REQ: HST-110
#[test]
fn hst_110_every_submission_orders_its_own_tree_change_first() {
    let (mut harness, conn) = attached();
    for index in 0..4 {
        harness.submit(conn, PLUGIN, &format!("k{index}"), "create-pane");
    }
    harness.pump(ROUNDS);
    let wire = harness.wire(conn);
    let mut tree_changes_seen = 0;
    for item in &wire {
        match item {
            WireItem::TreeChange { .. } => tree_changes_seen += 1,
            WireItem::Outcome { .. } => {
                assert!(tree_changes_seen >= 1, "outcome before any tree change")
            }
            _ => {}
        }
    }
    assert_eq!(tree_changes_seen, 4);
}

/// REQ: HST-111
#[test]
fn hst_111_mirror_is_updated_before_the_tree_changed_event_is_queued() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    let log = harness.deliveries(conn);
    let mirror = log
        .iter()
        .position(|entry| matches!(entry, ClientDelivery::MirrorUpdated { .. }))
        .expect("mirror update");
    let event = log
        .iter()
        .position(|entry| matches!(entry, ClientDelivery::TreeChangedEvent { .. }))
        .expect("tree-changed event");
    assert!(mirror < event);
}

/// REQ: HST-111
#[test]
fn hst_111_request_completed_is_delivered_after_the_mirror_shows_the_effect() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    let log = harness.deliveries(conn);
    let mirror = log
        .iter()
        .position(|entry| matches!(entry, ClientDelivery::MirrorUpdated { .. }))
        .expect("mirror update");
    let completed = log
        .iter()
        .position(|entry| matches!(entry, ClientDelivery::RequestCompleted { .. }))
        .expect("request completed");
    assert!(mirror < completed);
}

/// REQ: HST-112
#[test]
fn hst_112_client_forwards_the_submission_and_applies_nothing_itself() {
    let (mut harness, conn) = attached();
    let tree_before = harness.server_tree_len();
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    assert_eq!(harness.submits_sent(conn), 1);
    assert_eq!(harness.client_local_effects(conn), 0);
    assert_eq!(harness.server_tree_len(), tree_before);
}

/// REQ: HST-112
#[test]
fn hst_112_only_the_server_applies_after_it_resumes() {
    let (mut harness, conn) = attached();
    let tree_before = harness.server_tree_len();
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.pause_apply(false);
    harness.pump(ROUNDS);
    assert_eq!(harness.server_tree_len(), tree_before + 1);
    assert_eq!(harness.client_local_effects(conn), 0);
}

/// REQ: HST-113
#[test]
fn hst_113_same_key_twice_in_one_scope_is_a_duplicate() {
    let mut table = ForwardedDedup::new();
    assert_eq!(table.admit(PLUGIN, "c1", "k", 0), DedupVerdict::Fresh);
    assert_eq!(
        table.admit(PLUGIN, "c1", "k", 1_000),
        DedupVerdict::Duplicate
    );
}

/// REQ: HST-113
#[test]
fn hst_113_two_clients_with_the_same_key_are_two_submissions() {
    let mut table = ForwardedDedup::new();
    assert_eq!(table.admit(PLUGIN, "c1", "k", 0), DedupVerdict::Fresh);
    assert_eq!(table.admit(PLUGIN, "c2", "k", 1), DedupVerdict::Fresh);
}

/// REQ: HST-113
#[test]
fn hst_113_two_plugins_with_the_same_key_and_client_are_two_submissions() {
    let mut table = ForwardedDedup::new();
    assert_eq!(table.admit(PLUGIN, "c1", "k", 0), DedupVerdict::Fresh);
    assert_eq!(table.admit(OTHER_PLUGIN, "c1", "k", 1), DedupVerdict::Fresh);
}

/// REQ: HST-113
#[test]
fn hst_113_window_is_ten_minutes_and_a_key_is_free_at_exactly_600000_ms() {
    assert_eq!(DEDUP_WINDOW_MS, 600_000);
    let mut table = ForwardedDedup::new();
    assert_eq!(table.admit(PLUGIN, "c1", "k", 1_000), DedupVerdict::Fresh);
    assert_eq!(
        table.admit(PLUGIN, "c1", "k", 1_000 + 599_999),
        DedupVerdict::Duplicate
    );
    let mut other = ForwardedDedup::new();
    assert_eq!(other.admit(PLUGIN, "c1", "k", 1_000), DedupVerdict::Fresh);
    assert_eq!(
        other.admit(PLUGIN, "c1", "k", 1_000 + 600_000),
        DedupVerdict::Fresh
    );
}

/// REQ: HST-113
#[test]
fn hst_113_server_answers_a_resent_key_from_its_table_without_applying_twice() {
    let (mut harness, conn) = attached();
    let tree_before = harness.server_tree_len();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    assert_eq!(harness.server_tree_len(), tree_before + 1);
}

/// REQ: HST-113
#[test]
fn hst_113_same_key_from_two_clients_applies_twice_on_the_server() {
    let mut harness = server_with_demo();
    let hello_a = harness.matching_hello();
    let hello_b = harness.matching_hello();
    let conn_a = harness.connect("client-a", true, hello_a);
    let conn_b = harness.connect("client-b", true, hello_b);
    harness.pump(ROUNDS);
    let tree_before = harness.server_tree_len();
    harness.submit(conn_a, PLUGIN, "same", "create-pane");
    harness.submit(conn_b, PLUGIN, "same", "create-pane");
    harness.pump(ROUNDS);
    assert_eq!(harness.server_tree_len(), tree_before + 2);
}

/// REQ: HST-114
#[test]
fn hst_114_grant_revoked_between_sending_and_applying_completes_as_revoked() {
    let (mut harness, conn) = attached();
    let tree_before = harness.server_tree_len();
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.revoke(PLUGIN, "panes.create");
    harness.pause_apply(false);
    harness.pump(ROUNDS);
    let outcome = harness.wire(conn).into_iter().find_map(|item| match item {
        WireItem::Outcome { outcome, .. } => Some(outcome),
        _ => None,
    });
    assert_eq!(outcome.as_deref(), Some("revoked"));
    assert_eq!(harness.server_tree_len(), tree_before);
}

/// REQ: HST-114
#[test]
fn hst_114_grant_given_between_sending_and_applying_is_honoured() {
    let mut harness = IpcHarness::new();
    harness.install_server_plugin(PLUGIN, "1.0.0", [1; 32]);
    let hello = harness.matching_hello();
    let conn = harness.connect("client-1", true, hello);
    harness.pump(ROUNDS);
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.grant(PLUGIN, "panes.create");
    harness.pause_apply(false);
    harness.pump(ROUNDS);
    let outcome = harness.wire(conn).into_iter().find_map(|item| match item {
        WireItem::Outcome { outcome, .. } => Some(outcome),
        _ => None,
    });
    assert_eq!(outcome.as_deref(), Some("ok-node"));
}

/// REQ: HST-114
#[test]
fn hst_114_revoked_submission_sends_no_tree_change() {
    let (mut harness, conn) = attached();
    harness.revoke(PLUGIN, "panes.create");
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    assert!(!harness.wire(conn).iter().any(is_tree_change));
}

/// REQ: HST-115
#[test]
fn hst_115_each_submit_gets_exactly_one_outcome() {
    let (mut harness, conn) = attached();
    for index in 0..5 {
        harness.submit(conn, PLUGIN, &format!("k{index}"), "create-pane");
    }
    harness.pump(ROUNDS);
    let wire = harness.wire(conn);
    for index in 0..5 {
        let key = format!("k{index}");
        let count = wire
            .iter()
            .filter(|item| matches!(item, WireItem::Outcome { dedup_key, .. } if *dedup_key == key))
            .count();
        assert_eq!(count, 1, "outcomes for {key}");
    }
}

/// REQ: HST-115
#[test]
fn hst_115_a_refused_submit_still_gets_one_outcome() {
    let (mut harness, conn) = attached();
    harness.revoke(PLUGIN, "panes.create");
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    let count = harness
        .wire(conn)
        .iter()
        .filter(|item| is_outcome(item))
        .count();
    assert_eq!(count, 1);
}

/// REQ: HST-115
#[test]
fn hst_115_outcome_seq_values_increase_by_one() {
    let (mut harness, conn) = attached();
    for index in 0..3 {
        harness.submit(conn, PLUGIN, &format!("k{index}"), "create-pane");
    }
    harness.pump(ROUNDS);
    let seqs: Vec<u64> = harness
        .wire(conn)
        .iter()
        .filter_map(|item| match item {
            WireItem::Outcome { seq, .. } => Some(*seq),
            _ => None,
        })
        .collect();
    assert_eq!(seqs.len(), 3);
    assert!(seqs.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

// ---------------------------------------------------------------- HST-116 .. HST-119

/// REQ: HST-116
#[test]
fn hst_116_records_are_kept_for_thirty_seconds_after_the_connection_closes() {
    assert_eq!(OUTCOME_GRACE_MS, 30_000);
    let mut store = OutcomeStore::new();
    store.record("c1", 1, "ok-node", 0);
    store.on_disconnect("c1", 1_000);
    assert_eq!(store.replay("c1", 0, 1_000 + 29_999).len(), 1);
}

/// REQ: HST-116
#[test]
fn hst_116_records_are_gone_after_the_grace_period() {
    let mut store = OutcomeStore::new();
    store.record("c1", 1, "ok-node", 0);
    store.on_disconnect("c1", 1_000);
    assert!(store.replay("c1", 0, 1_000 + 30_001).is_empty());
}

/// REQ: HST-116
#[test]
fn hst_116_records_of_a_connected_client_do_not_expire() {
    let mut store = OutcomeStore::new();
    store.record("c1", 1, "ok-node", 0);
    assert_eq!(store.record_count("c1", 3_600_000), 1);
}

/// REQ: HST-116
#[test]
fn hst_116_at_most_1024_records_per_client_id() {
    assert_eq!(OUTCOME_RECORD_LIMIT, 1_024);
    let mut store = OutcomeStore::new();
    for seq in 1..=1_024u64 {
        store.record("c1", seq, "ok-node", 0);
    }
    assert_eq!(store.record_count("c1", 0), 1_024);
    store.record("c1", 1_025, "ok-node", 0);
    assert_eq!(store.record_count("c1", 0), 1_024);
    let newest = store.replay("c1", 1_024, 0);
    assert_eq!(newest.len(), 1);
    assert_eq!(newest[0].seq, 1_025);
}

/// REQ: HST-116
#[test]
fn hst_116_the_limit_is_per_client_id() {
    let mut store = OutcomeStore::new();
    for seq in 1..=1_025u64 {
        store.record("c1", seq, "ok-node", 0);
    }
    store.record("c2", 1, "ok-node", 0);
    assert_eq!(store.record_count("c2", 0), 1);
}

/// REQ: HST-117
#[test]
fn hst_117_replay_returns_only_outcomes_with_a_higher_seq() {
    let mut store = OutcomeStore::new();
    for seq in 1..=5u64 {
        store.record("c1", seq, "ok-node", 0);
    }
    store.on_disconnect("c1", 0);
    let seqs: Vec<u64> = store
        .replay("c1", 3, 5_000)
        .iter()
        .map(|record| record.seq)
        .collect();
    assert_eq!(seqs, [4, 5]);
}

/// REQ: HST-117
#[test]
fn hst_117_reattach_replays_outcomes_then_sends_a_snapshot() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.submit(conn, PLUGIN, "k2", "create-pane");
    harness.pump(ROUNDS);
    let first_seq = harness
        .wire(conn)
        .iter()
        .find_map(|item| match item {
            WireItem::Outcome { seq, .. } => Some(*seq),
            _ => None,
        })
        .expect("first outcome");
    harness.disconnect(conn);
    harness.advance(seconds(10));
    let hello = harness.matching_hello();
    let again = harness.reattach("client-1", first_seq, hello);
    harness.pump(ROUNDS);
    let wire = harness.wire(again);
    let outcomes: Vec<&WireItem> = wire.iter().filter(|item| is_outcome(item)).collect();
    assert_eq!(outcomes.len(), 1);
    assert!(matches!(outcomes[0], WireItem::Outcome { dedup_key, .. } if dedup_key == "k2"));
    let outcome_at = position(&wire, is_outcome).expect("outcome");
    let snapshot_at = position(&wire, is_snapshot).expect("snapshot");
    assert!(outcome_at < snapshot_at);
}

/// REQ: HST-117
#[test]
fn hst_117_reattach_after_the_grace_period_replays_nothing() {
    let (mut harness, conn) = attached();
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.disconnect(conn);
    harness.advance(seconds(31));
    let hello = harness.matching_hello();
    let again = harness.reattach("client-1", 0, hello);
    harness.pump(ROUNDS);
    let wire = harness.wire(again);
    assert!(!wire.iter().any(is_outcome));
    assert!(wire.iter().any(is_snapshot));
}

/// REQ: HST-118
#[test]
fn hst_118_unapplied_submission_completes_as_cancelled_after_the_grace_period() {
    let (mut harness, conn) = attached();
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.disconnect(conn);
    harness.advance(seconds(29));
    harness.pump(ROUNDS);
    assert_eq!(harness.final_outcome("client-1", PLUGIN, "k1"), None);
    harness.advance(seconds(2));
    harness.pump(ROUNDS);
    assert_eq!(
        harness.final_outcome("client-1", PLUGIN, "k1").as_deref(),
        Some("cancelled")
    );
}

/// REQ: HST-118
#[test]
fn hst_118_a_cancelled_submission_is_not_replayed_on_a_later_attach() {
    let (mut harness, conn) = attached();
    harness.pause_apply(true);
    harness.submit(conn, PLUGIN, "k1", "create-pane");
    harness.pump(ROUNDS);
    harness.disconnect(conn);
    harness.advance(seconds(31));
    harness.pump(ROUNDS);
    harness.pause_apply(false);
    let tree_before = harness.server_tree_len();
    let hello = harness.matching_hello();
    let again = harness.reattach("client-1", 0, hello);
    harness.pump(ROUNDS);
    assert!(!harness.wire(again).iter().any(is_outcome));
    assert_eq!(harness.server_tree_len(), tree_before);
}

/// REQ: HST-119
#[test]
fn hst_119_the_257th_detached_submission_is_refused_busy_at_once() {
    assert_eq!(DETACHED_QUEUE_LIMIT, 256);
    let mut queue = DetachedQueue::new();
    for index in 0..256 {
        assert_eq!(
            queue.push(&format!("k{index}")),
            QueueVerdict::Queued,
            "submission {index}"
        );
    }
    assert_eq!(queue.len(), 256);
    assert_eq!(queue.push("k256"), QueueVerdict::RefusedBusy);
    assert_eq!(queue.len(), 256);
}

/// REQ: HST-119
#[test]
fn hst_119_the_256th_detached_submission_is_still_queued() {
    let mut queue = DetachedQueue::new();
    for index in 0..255 {
        queue.push(&format!("k{index}"));
    }
    assert_eq!(queue.push("last"), QueueVerdict::Queued);
}

/// REQ: HST-119
#[test]
fn hst_119_submissions_still_queued_at_client_exit_complete_as_cancelled() {
    let mut queue = DetachedQueue::new();
    for index in 0..3 {
        queue.push(&format!("k{index}"));
    }
    assert_eq!(queue.on_client_exit(), ["k0", "k1", "k2"]);
    assert_eq!(queue.len(), 0);
}

/// REQ: HST-119
#[test]
fn hst_119_a_refused_busy_submission_is_not_cancelled_at_exit() {
    let mut queue = DetachedQueue::new();
    for index in 0..256 {
        queue.push(&format!("k{index}"));
    }
    queue.push("overflow");
    let cancelled = queue.on_client_exit();
    assert_eq!(cancelled.len(), 256);
    assert!(!cancelled.iter().any(|key| key == "overflow"));
}

// ---------------------------------------------------------------- HST-120 .. HST-123

fn plugin_record(id: u32, bytes: usize, key: Option<&str>) -> PluginRecord {
    PluginRecord {
        id,
        bytes,
        coalesce_key: key.map(str::to_owned),
    }
}

/// REQ: HST-120
#[test]
fn hst_120_pty_frames_are_written_before_plugin_envelopes() {
    let mut queues = WriterQueues::new();
    queues.push_plugin(plugin_record(100, 10, None));
    for id in 0..3 {
        queues.push_pty_frame(id);
    }
    assert_eq!(queues.next(), Some(Written::Pty(0)));
    assert_eq!(queues.next(), Some(Written::Pty(1)));
    assert_eq!(queues.next(), Some(Written::Pty(2)));
    assert_eq!(queues.next(), Some(Written::Plugin(100)));
    assert_eq!(queues.next(), None);
}

/// REQ: HST-120
#[test]
fn hst_120_one_plugin_envelope_for_every_eight_frames_when_both_queues_are_busy() {
    assert_eq!(PTY_FRAMES_PER_PLUGIN_ENVELOPE, 8);
    let mut queues = WriterQueues::new();
    for id in 0..200 {
        queues.push_pty_frame(id);
    }
    for id in 0..40 {
        queues.push_plugin(plugin_record(1_000 + id, 10, None));
    }
    let mut frames_since_plugin = 0u32;
    let mut plugin_count = 0;
    for _ in 0..90 {
        match queues.next().expect("both queues hold data") {
            Written::Pty(_) => {
                frames_since_plugin += 1;
                assert!(frames_since_plugin <= 8, "more than eight frames in a row");
            }
            Written::Plugin(_) => {
                frames_since_plugin = 0;
                plugin_count += 1;
            }
        }
    }
    assert_eq!(plugin_count, 10);
}

/// REQ: HST-120
#[test]
fn hst_120_the_first_write_is_a_pty_frame() {
    let mut queues = WriterQueues::new();
    queues.push_plugin(plugin_record(1, 10, None));
    queues.push_pty_frame(7);
    assert_eq!(queues.next(), Some(Written::Pty(7)));
}

/// REQ: HST-120
#[test]
fn hst_120_a_plugin_only_queue_drains_without_waiting_for_frames() {
    let mut queues = WriterQueues::new();
    for id in 0..3 {
        queues.push_plugin(plugin_record(id, 10, None));
    }
    assert_eq!(queues.next(), Some(Written::Plugin(0)));
    assert_eq!(queues.next(), Some(Written::Plugin(1)));
    assert_eq!(queues.next(), Some(Written::Plugin(2)));
}

/// REQ: HST-120
#[test]
fn hst_120_plugin_envelopes_keep_their_order_among_themselves() {
    let mut queues = WriterQueues::new();
    for id in 0..30 {
        queues.push_pty_frame(id);
    }
    for id in 500..504 {
        queues.push_plugin(plugin_record(id, 10, None));
    }
    let plugin_order: Vec<u32> = std::iter::from_fn(|| queues.next())
        .filter_map(|written| match written {
            Written::Plugin(id) => Some(id),
            Written::Pty(_) => None,
        })
        .collect();
    assert_eq!(plugin_order, [500, 501, 502, 503]);
}

/// REQ: HST-121
#[test]
fn hst_121_queue_bounds_are_4_mib_and_4096_records() {
    assert_eq!(PLUGIN_QUEUE_MAX_BYTES, 4 * MIB);
    assert_eq!(PLUGIN_QUEUE_MAX_RECORDS, 4_096);
}

/// REQ: HST-121
#[test]
fn hst_121_record_4096_fits_and_record_4097_flags_a_resync() {
    let mut queues = WriterQueues::new();
    for id in 0..4_096u32 {
        let outcome = queues.push_plugin(plugin_record(id, 1, None));
        assert_eq!(outcome, PushOutcome::Queued, "record {id}");
    }
    assert!(!queues.resync_flagged());
    assert_eq!(
        queues.push_plugin(plugin_record(4_096, 1, None)),
        PushOutcome::ResyncFlagged
    );
    assert!(queues.resync_flagged());
}

/// REQ: HST-121
#[test]
fn hst_121_exactly_4_mib_fits_and_one_byte_more_flags_a_resync() {
    let mut queues = WriterQueues::new();
    for id in 0..3u32 {
        assert_eq!(
            queues.push_plugin(plugin_record(id, MIB, None)),
            PushOutcome::Queued
        );
    }
    assert_eq!(
        queues.push_plugin(plugin_record(3, MIB, None)),
        PushOutcome::Queued
    );
    assert!(!queues.resync_flagged());
    assert_eq!(queues.plugin_queued_bytes(), 4 * MIB);
    assert_eq!(
        queues.push_plugin(plugin_record(4, 1, None)),
        PushOutcome::ResyncFlagged
    );
}

/// REQ: HST-121
#[test]
fn hst_121_coalescable_increments_of_one_id_are_merged_first_without_a_resync() {
    let mut queues = WriterQueues::new();
    for id in 0..6_000u32 {
        let outcome = queues.push_plugin(plugin_record(id, 100, Some("status.a")));
        assert!(matches!(outcome, PushOutcome::Queued | PushOutcome::Merged));
        assert!(queues.plugin_queued_records() <= PLUGIN_QUEUE_MAX_RECORDS);
    }
    assert!(!queues.resync_flagged());
    assert_eq!(queues.plugin_queued_records(), 1);
}

/// REQ: HST-121
#[test]
fn hst_121_a_full_queue_merges_a_known_id_instead_of_flagging_a_resync() {
    let mut queues = WriterQueues::new();
    for id in 0..4_096u32 {
        queues.push_plugin(plugin_record(id, 1, Some(&format!("id.{id}"))));
    }
    assert_eq!(
        queues.push_plugin(plugin_record(9_000, 1, Some("id.0"))),
        PushOutcome::Merged
    );
    assert!(!queues.resync_flagged());
    assert_eq!(
        queues.push_plugin(plugin_record(9_001, 1, Some("id.new"))),
        PushOutcome::ResyncFlagged
    );
}

/// REQ: HST-122
#[test]
fn hst_122_exactly_16_mib_queued_does_not_disconnect_and_one_record_more_does() {
    assert_eq!(CLIENT_TOO_SLOW_BYTES, 16 * MIB);
    let mut queues = WriterQueues::new();
    for id in 0..16u32 {
        let outcome = queues.push_plugin(plugin_record(id, MIB, None));
        assert!(
            !matches!(outcome, PushOutcome::Disconnect(_)),
            "record {id}"
        );
    }
    match queues.push_plugin(plugin_record(16, MIB, None)) {
        PushOutcome::Disconnect(fault) => assert_eq!(fault.code(), "ipc.client-too-slow"),
        other => panic!("expected a disconnect, got {other:?}"),
    }
}

/// REQ: HST-122
#[test]
fn hst_122_a_stalled_client_is_disconnected_with_client_too_slow_and_gets_a_snapshot_later() {
    let (mut harness, conn) = attached();
    harness.flood_plugin_data(conn, MIB, 20);
    harness.pump(ROUNDS);
    assert!(!harness.is_open(conn));
    assert_eq!(
        harness.close_code(conn).as_deref(),
        Some("ipc.client-too-slow")
    );
    let hello = harness.matching_hello();
    let again = harness.reattach("client-1", 0, hello);
    harness.pump(ROUNDS);
    assert!(harness.wire(again).iter().any(is_snapshot));
}

/// REQ: HST-122
#[test]
fn hst_122_a_slow_client_loses_no_pane_output() {
    let (mut harness, conn) = attached();
    harness.push_pty_frames(conn, 50);
    harness.flood_plugin_data(conn, MIB, 20);
    harness.pump(ROUNDS);
    assert_eq!(harness.pty_frames_dropped(conn), 0);
}

/// REQ: HST-123
#[test]
fn hst_123_a_pty_frame_waits_for_at_most_one_plugin_envelope() {
    let mut queues = WriterQueues::new();
    for id in 0..50u32 {
        queues.push_plugin(plugin_record(id, MIB, None));
    }
    queues.push_pty_frame(1);
    let first = queues.next().expect("data queued");
    let second = queues.next().expect("data queued");
    let frame_at = [first, second]
        .iter()
        .position(|written| *written == Written::Pty(1));
    assert!(
        frame_at.is_some(),
        "the frame was not written within one envelope time"
    );
}

/// REQ: HST-123
#[test]
fn hst_123_frames_arriving_during_plugin_load_are_never_overtaken_by_two_envelopes() {
    let mut queues = WriterQueues::new();
    for id in 0..30u32 {
        queues.push_plugin(plugin_record(id, 1_000, None));
    }
    let mut plugin_in_row = 0;
    for frame in 0..20u32 {
        queues.push_pty_frame(frame);
        loop {
            match queues.next().expect("data queued") {
                Written::Pty(_) => break,
                Written::Plugin(_) => {
                    plugin_in_row += 1;
                    assert!(plugin_in_row <= 1, "frame {frame} delayed by two envelopes");
                }
            }
        }
        plugin_in_row = 0;
    }
}

// ---------------------------------------------------------------- HST-124 / HST-125

const CAPS: [(PayloadKind, usize); 7] = [
    (PayloadKind::MessageBody, MIB),
    (PayloadKind::TopicPayload, 64 * 1024),
    (PayloadKind::UiTree, MIB),
    (PayloadKind::UiTreeT1, 4 * MIB),
    (PayloadKind::SurfaceWrite, 16 * MIB),
    (PayloadKind::Envelope, 17 * MIB),
    (PayloadKind::ContributionDelta, 4 * MIB),
];

/// REQ: HST-124
#[test]
fn hst_124_every_cap_accepts_n_and_refuses_n_plus_one_with_payload_too_large() {
    for (kind, cap) in CAPS {
        assert!(check_payload(kind, cap - 1).is_ok(), "{kind:?} at n-1");
        assert!(check_payload(kind, cap).is_ok(), "{kind:?} at n");
        let fault = check_payload(kind, cap + 1).expect_err("n+1 is refused");
        assert_eq!(fault.code(), SLASH_ERR, "{kind:?}");
    }
}

/// REQ: HST-124
#[test]
fn hst_124_an_oversized_item_is_refused_before_the_encoder_runs() {
    let calls = Cell::new(0u32);
    let result = encode_checked(PayloadKind::MessageBody, MIB + 1, || {
        calls.set(calls.get() + 1);
        vec![0; MIB + 1]
    });
    assert_eq!(result.expect_err("too large").code(), SLASH_ERR);
    assert_eq!(calls.get(), 0);
}

/// REQ: HST-124
#[test]
fn hst_124_an_item_within_the_cap_is_encoded_once() {
    let calls = Cell::new(0u32);
    let result = encode_checked(PayloadKind::TopicPayload, 64 * 1024, || {
        calls.set(calls.get() + 1);
        vec![1; 64 * 1024]
    });
    assert_eq!(result.expect("within cap").len(), 64 * 1024);
    assert_eq!(calls.get(), 1);
}

/// REQ: HST-124
#[test]
fn hst_124_the_senders_call_completes_as_failed_too_large() {
    let fault = check_payload(PayloadKind::SurfaceWrite, 16 * MIB + 1).expect_err("too large");
    assert_eq!(fault.completion_outcome(), Some("failed(too-large)"));
}

/// REQ: HST-124
#[test]
fn doc_hst_124_page_table_lists_the_caps_used_by_the_tests() {
    let text = page_text();
    for row in [
        "| Plugin message body | 1 MiB |",
        "| Topic payload | 64 KiB |",
        "| `ui-tree` of a view | 1 MiB, 4 MiB with the T1 ceiling |",
        "| Surface write | 16 MiB |",
        "| Whole envelope | about 17 MiB |",
        "| Contribution delta | 4 MiB |",
    ] {
        assert!(text.contains(row), "page lacks row {row}");
    }
}

proptest! {
    // REQ: HST-124
    #[test]
    fn hst_124_check_payload_agrees_with_the_cap_for_any_length(
        index in 0usize..7,
        delta in -2i64..=2,
    ) {
        let (kind, cap) = CAPS[index];
        let length = (cap as i64 + delta) as usize;
        prop_assert_eq!(check_payload(kind, length).is_ok(), length <= cap);
    }
}

/// REQ: HST-125
#[test]
fn hst_125_envelope_of_17_mib_is_accepted() {
    assert_eq!(ENVELOPE_MAX_BYTES, 17 * MIB);
    let mut receiver = ReceiverConnection::new();
    assert!(receiver.on_envelope(1, 17 * MIB).is_ok());
    assert!(receiver.is_open());
}

/// REQ: HST-125
#[test]
fn hst_125_envelope_above_17_mib_is_dropped_and_the_connection_closed() {
    let mut receiver = ReceiverConnection::new();
    receiver.on_envelope(1, 100).expect("seq 1");
    let fault = receiver
        .on_envelope(2, 17 * MIB + 1)
        .expect_err("oversized");
    assert_eq!(fault.code(), SLASH_ERR);
    assert!(!receiver.is_open());
    assert_eq!(receiver.closed_with().as_deref(), Some(SLASH_ERR));
}

/// REQ: HST-125
#[test]
fn hst_125_nothing_is_accepted_after_an_oversized_envelope() {
    let mut receiver = ReceiverConnection::new();
    receiver
        .on_envelope(1, 17 * MIB + 1)
        .expect_err("oversized first envelope");
    assert!(receiver.on_envelope(2, 10).is_err());
}
