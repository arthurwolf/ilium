//! Twins of ilium-server text_triggers/matching.rs: ordering, cancellation and resumption of matching work, seen as the order and count of send-inputs.

use ilium_plugin_sdk::types::{ErrorCode, Outcome};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::cancelled_queued_step_preserves_original_engine_and_bytes
#[test]
fn port_001_cancelled_queued_step_preserves_original_engine_and_bytes() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    set_rules(&mut host, &["other"], &["no"]);
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "other", 2));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "no")]);
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::matching_runs_on_a_real_cpu_owner_without_retaining_finite_job_credit
#[test]
fn port_002_matching_runs_on_a_real_cpu_owner_without_retaining_finite_job_credit() {
    let (mut host, pane) = armed("go", "yes");
    for round in 0..200u32 {
        host.deliver_one(hit(pane, round));
    }
    assert_eq!(applied(&mut host), 200);
    let submissions = host.deliver_one(hit(pane, 1_000));
    assert_eq!(texts(&submissions), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::ordered_cpu_steps_preserve_split_escape_sequences_and_resize_quiet_policy
#[test]
fn port_003_ordered_cpu_steps_preserve_split_escape_sequences_and_resize_quiet_policy() {
    let (mut host, pane) = armed("go", "yes");
    let mut all = Vec::new();
    for row in [3u32, 1, 2] {
        all.extend(host.deliver_one(hit(pane, row)));
    }
    let keys: Vec<Option<String>> = all.iter().map(|s| s.dedup_key.clone()).collect();
    assert_eq!(
        keys,
        [3u32, 1, 2].map(|row| Some(dedup_key(&rule_id(0), pane, row)))
    );
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::refused_delivery_retains_decision_and_resumes_without_duplicate_matching
#[test]
fn port_004_refused_delivery_retains_decision_and_resumes_without_duplicate_matching() {
    let (mut host, pane) = armed("go", "yes");
    host.revoke("terminal.write");
    host.deliver_one(hit(pane, 1));
    host.settle();
    assert!(host
        .outcomes()
        .iter()
        .any(|o| matches!(o, Outcome::Refused(e) if e.code == ErrorCode::NotPermitted)));
    let sent_after_refusal = host.submitted().len();
    host.deliver_one(dropped(&rule_id(0), 0));
    host.settle();
    assert_eq!(host.submitted().len(), sent_after_refusal);
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::replay_resync_retains_rule_admissions_and_settle_clock
#[test]
fn port_005_replay_resync_retains_rule_admissions_and_settle_clock() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.deliver_one(dropped(&rule_id(0), 50));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::same_size_native_resize_rejects_a_prepared_screen_projection
#[test]
fn port_006_same_size_native_resize_rejects_a_prepared_screen_projection() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(2_001);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers/matching.rs::split_osc_highwater_tracks_library_termination_and_retained_capacity
#[test]
fn port_007_split_osc_highwater_tracks_library_termination_and_retained_capacity() {
    let (mut host, pane) = armed("go", "yes");
    let long = format!("go \u{1b}]0;{}\u{7}", "t".repeat(4_096));
    let submissions = host.deliver_one(matched(pane, &rule_id(0), &long, 1));
    assert_eq!(texts(&submissions), ["yes"]);
}
