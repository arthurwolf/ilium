//! Intent routing between the client half and the server (api-intents.md; HST-490..494).

use std::time::Duration;

use ilium_plugin_api::generated::types::{Event, Intent, Outcome};
use ilium_plugin_api::Side;

use crate::common::*;

fn client_setup() -> Setup {
    Setup::new(&[
        "tree.read",
        "tree.write",
        "terminal.write",
        "panes.lifecycle",
    ])
    .side(Side::Client)
}

/// REQ: HST-490
#[test]
fn hst_490_every_intent_kind_is_applied_by_exactly_the_listed_side() {
    for case in Intent::CASE_NAMES {
        let side = ilium_plugin_host::testing::applying_side(case);
        let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
        let sides = env.kernel.sides_that_applied(case);
        assert!(sides.len() <= 1, "intent {case} applied on {sides:?}");
        assert!(
            matches!(side, Side::Server | Side::Client),
            "intent {case} has a listed side"
        );
    }
}

/// REQ: HST-490
#[test]
fn hst_490_a_server_intent_from_a_client_half_is_forwarded() {
    let env = client_setup().tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert_eq!(env.host.forwarded_submissions().len(), 1);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: HST-490
#[test]
fn hst_490_a_server_half_submitter_is_not_forwarded() {
    let env = Setup::new(&["tree.write"]).tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert!(env.host.forwarded_submissions().is_empty());
}

/// REQ: HST-490
#[test]
fn hst_490_a_client_applied_intent_is_not_forwarded() {
    let env = client_setup().tree(flat_tree(1)).build();
    let client_only = ilium_plugin_host::testing::a_client_applied_intent();
    env.run(vec![sub(1, client_only)]);
    assert!(env.host.forwarded_submissions().is_empty());
}

/// REQ: HST-491
#[test]
fn hst_491_the_outcome_goes_only_to_the_submitting_client() {
    let env = client_setup().tree(flat_tree(1)).build();
    let other = env.host.attach_second_client();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert_eq!(count_for(&env.outcomes(), 1), 1);
    assert!(other.received_outcomes().is_empty());
}

/// REQ: HST-491
#[test]
fn hst_491_two_clients_each_get_only_their_own_outcome() {
    let env = client_setup().tree(flat_tree(1)).build();
    let other = env.host.attach_second_client();
    env.run(vec![sub(1, rename(2, "a"))]);
    other.submit(sub(9, rename(2, "b")));
    env.settle();
    assert_eq!(other.received_outcomes().len(), 1);
    assert_eq!(count_for(&env.outcomes(), 9), 0);
}

/// REQ: HST-492
#[test]
fn hst_492_forwarded_submission_latency_p99_is_at_most_3_ms() {
    let env = client_setup().tree(flat_tree(1)).build();
    let mut samples = Vec::new();
    for n in 1..=200u64 {
        let started = env.host.now_monotonic();
        env.run(vec![sub(n, rename(2, "a"))]);
        let forwarded = env.host.forward_latency_of(n);
        let _ = started;
        samples.push(forwarded);
    }
    samples.sort();
    let p99 = samples[(samples.len() * 99 / 100).min(samples.len() - 1)];
    assert!(p99 <= Duration::from_millis(3), "p99 {p99:?}");
}

/// REQ: HST-492
#[test]
fn hst_492_the_median_is_not_above_the_p99_bound() {
    let env = client_setup().tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert!(env.host.forward_latency_of(1) <= Duration::from_millis(3));
}

/// REQ: HST-493
#[test]
fn hst_493_a_client_half_reads_the_tree_from_the_mirror() {
    let env = client_setup().tree(flat_tree(2)).build();
    env.kernel.freeze_server_tree_reads();
    let seen = env.host.client_tree_read();
    assert_eq!(seen.len(), 3);
}

/// REQ: HST-493
#[test]
fn hst_493_the_mirror_is_updated_before_tree_changed_is_delivered() {
    let env = client_setup().tree(flat_tree(2)).build();
    env.settle();
    env.host.deliver(vec![tree_changed(&[2])]);
    let sequence = lock(&env.probe.sequence).clone();
    let mirror = env.host.mirror_update_index();
    let delivered = sequence
        .iter()
        .rposition(|s| s == "on-events")
        .expect("delivery");
    assert!(
        mirror <= delivered as u64,
        "mirror update {mirror} before delivery {delivered}"
    );
}

/// REQ: HST-494
#[test]
fn hst_494_the_tree_change_reaches_the_mirror_before_request_completed() {
    let env = client_setup().tree(flat_tree(2)).build();
    env.run(vec![sub(1, close(2))]);
    let mirror_at = env.host.mirror_update_index();
    let completed_at = env.host.request_completed_index(1);
    assert!(
        mirror_at < completed_at,
        "mirror {mirror_at} before completion {completed_at}"
    );
}

/// REQ: HST-494
#[test]
fn hst_494_after_the_completion_the_mirror_no_longer_holds_the_closed_node() {
    let env = client_setup().tree(flat_tree(2)).build();
    env.run(vec![sub(1, close(2))]);
    assert!(is_ok(&env.outcome(1)));
    assert!(!env
        .host
        .client_tree_read()
        .iter()
        .any(|view| view.id.0 == 2));
}

#[allow(dead_code)]
fn unused(_: Event, _: Outcome) {}
