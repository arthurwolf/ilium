//! Submissions, outcomes, dedup keys (manual page requests-and-outcomes.md; RQO-*).

use std::time::Duration;

use ilium_plugin_api::generated::types::{ErrorCode, Event, NodeId, Outcome};
use proptest::prelude::*;

use crate::common::*;

fn writer() -> Setup {
    Setup::new(&["tree.write", "terminal.write", "panes.read"])
}

/// REQ: RQO-001
#[test]
fn rqo_001_ten_submissions_complete_once_each() {
    let env = writer().tree(flat_tree(10)).build();
    env.run((1..=10).map(|n| sub(n, rename(n + 1, "t"))).collect());
    let results = env.outcomes();
    assert_eq!(results.len(), 10);
    for n in 1..=10 {
        assert_eq!(count_for(&results, n), 1);
    }
}

/// REQ: RQO-001
#[test]
fn rqo_001_a_refused_batch_still_completes_every_submission_once() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, send_input(2, "x")), sub(3, rename(3, "b"))]);
    let results = env.outcomes();
    for n in 1..=3 {
        assert_eq!(count_for(&results, n), 1);
    }
}

/// REQ: RQO-001
#[test]
fn rqo_001_a_second_settle_adds_no_completion() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    env.settle();
    env.settle();
    assert_eq!(count_for(&env.outcomes(), 1), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]
    // REQ: RQO-001
    #[test]
    fn rqo_001_any_batch_of_1_to_40_completes_exactly_once_each(count in 1u64..40) {
        let env = writer().tree(flat_tree(2)).build();
        env.run((1..=count).map(|n| sub(n, rename(2, "t"))).collect());
        let results = env.outcomes();
        prop_assert_eq!(results.len() as u64, count);
    }
}

/// REQ: RQO-002
#[test]
fn rqo_002_a_returned_request_id_completes_once_with_the_matching_id() {
    let env = writer().tree(flat_tree(1)).build();
    let request = env.host.submit_directly(sub(1, rename(2, "a"))).expect("submit returns a request id");
    env.settle();
    let completed = env.probe.events_of("request-completed");
    assert_eq!(completed.len(), 1);
    match &completed[0] {
        Event::RequestCompleted(result) => assert_eq!(result.request, request),
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: RQO-002
#[test]
fn rqo_002_two_calls_give_two_distinct_ids_and_two_results() {
    let env = writer().tree(flat_tree(1)).build();
    let first = env.host.submit_directly(sub(1, rename(2, "a"))).expect("first");
    let second = env.host.submit_directly(sub(2, rename(2, "b"))).expect("second");
    env.settle();
    assert_ne!(first, second);
    assert_eq!(env.probe.count_of("request-completed"), 2);
}

/// REQ: RQO-002
#[test]
fn rqo_002_an_unknown_request_id_is_never_completed() {
    let env = writer().tree(flat_tree(1)).build();
    env.settle();
    assert_eq!(env.probe.count_of("request-completed"), 0);
}

/// REQ: RQO-003
#[test]
fn rqo_003_rename_after_close_in_one_batch_is_failed_not_found() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, close(2)), sub(2, rename(2, "x"))]);
    assert!(is_ok(&env.outcome(1)), "{:?}", env.outcome(1));
    assert!(is_failed(&env.outcome(2), ErrorCode::NotFound), "{:?}", env.outcome(2));
}

/// REQ: RQO-003
#[test]
fn rqo_003_rename_before_close_succeeds_for_both() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "x")), sub(2, close(2))]);
    assert!(is_ok(&env.outcome(1)));
    assert!(is_ok(&env.outcome(2)));
}

/// REQ: RQO-003
#[test]
fn rqo_003_applied_order_equals_submission_order() {
    let env = writer().tree(flat_tree(3)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, rename(3, "b")), sub(3, rename(4, "c"))]);
    let titles: Vec<String> = env
        .applied()
        .iter()
        .filter_map(|intent| match intent {
            ilium_plugin_api::generated::types::Intent::RenameNode(spec) => Some(spec.title.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, vec!["a", "b", "c"]);
}

/// REQ: RQO-004
#[test]
fn rqo_004_same_key_twice_gives_duplicate_naming_the_first_and_no_second_effect() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k:1", send_input(2, "a"))]);
    env.run(vec![keyed(2, "k:1", send_input(2, "a"))]);
    assert!(is_duplicate_of(&env.outcome(2), 1), "{:?}", env.outcome(2));
    let writes = env.applied().len();
    assert_eq!(writes, 1);
}

/// REQ: RQO-004
#[test]
fn rqo_004_at_599_999_ms_the_key_is_still_reserved() {
    let mut env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k:1", send_input(2, "a"))]);
    env.host.advance_clock(Duration::from_millis(599_999));
    env.run(vec![keyed(2, "k:1", send_input(2, "a"))]);
    assert!(is_duplicate_of(&env.outcome(2), 1));
}

/// REQ: RQO-004
#[test]
fn rqo_004_at_600_000_ms_the_key_is_free() {
    let mut env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k:1", send_input(2, "a"))]);
    env.host.advance_clock(Duration::from_millis(600_000));
    env.run(vec![keyed(2, "k:1", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(2)), "{:?}", env.outcome(2));
}

/// REQ: RQO-004
#[test]
fn rqo_004_at_600_001_ms_the_key_is_free() {
    let mut env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k:1", send_input(2, "a"))]);
    env.host.advance_clock(Duration::from_millis(600_001));
    env.run(vec![keyed(2, "k:1", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(2)));
}

/// REQ: RQO-005
#[test]
fn rqo_005_send_input_to_an_idle_agent_pane_gives_ok_epoch() {
    let nodes = vec![
        node(1, None, ilium_plugin_api::generated::types::NodeClass::Container, "root"),
        agent_pane(2, Some(1), ilium_plugin_api::generated::types::AgentActivity::Idle),
    ];
    let env = writer().tree(nodes).build();
    env.run(vec![keyed(1, "e:1", send_input(2, "a"))]);
    assert!(matches!(env.outcome(1), Some(Outcome::OkEpoch(_))), "{:?}", env.outcome(1));
}

/// REQ: RQO-005
#[test]
fn rqo_005_the_epoch_increases_with_each_prompt() {
    let nodes = vec![
        node(1, None, ilium_plugin_api::generated::types::NodeClass::Container, "root"),
        agent_pane(2, Some(1), ilium_plugin_api::generated::types::AgentActivity::Idle),
    ];
    let env = writer().tree(nodes).build();
    env.run(vec![keyed(1, "e:1", send_input(2, "a"))]);
    env.run(vec![keyed(2, "e:2", send_input(2, "b"))]);
    match (env.outcome(1), env.outcome(2)) {
        (Some(Outcome::OkEpoch(first)), Some(Outcome::OkEpoch(second))) => assert!(second > first),
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: RQO-005
#[test]
fn rqo_005_send_input_to_a_plain_terminal_is_plain_ok() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "e:1", send_input(2, "a"))]);
    assert!(matches!(env.outcome(1), Some(Outcome::Ok)), "{:?}", env.outcome(1));
}

/// REQ: RQO-006
#[test]
fn rqo_006_deactivating_the_plugin_cancels_a_slow_request() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.hold_next_request();
    env.run(vec![sub(1, rename(2, "a"))]);
    env.host.deactivate_plugin();
    env.settle();
    assert!(matches!(env.outcome(1), Some(Outcome::Cancelled)), "{:?}", env.outcome(1));
}

/// REQ: RQO-006
#[test]
fn rqo_006_host_shutdown_cancels_pending_requests() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.hold_next_request();
    env.run(vec![sub(1, rename(2, "a"))]);
    env.host.shutdown();
    assert!(matches!(env.outcome(1), Some(Outcome::Cancelled)));
}

/// REQ: RQO-006
#[test]
fn rqo_006_a_completed_request_is_not_cancelled_by_a_later_stop() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    env.host.deactivate_plugin();
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: RQO-007
#[test]
fn rqo_007_batch_of_three_refused_siblings_carry_refused_cancelled() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, send_input(2, "x")), sub(3, rename(3, "b"))]);
    for n in [1u64, 3] {
        assert!(is_refused(&env.outcome(n), ErrorCode::Cancelled));
        assert!(!matches!(env.outcome(n), Some(Outcome::Cancelled)));
    }
    assert!(is_refused(&env.outcome(2), ErrorCode::InvalidArgument));
}

/// REQ: RQO-007
#[test]
fn rqo_007_a_single_submission_batch_has_no_siblings() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![sub(1, send_input(2, "x"))]);
    assert_eq!(env.outcomes().len(), 1);
}

/// REQ: RQO-008
#[test]
fn rqo_008_take_after_31_s_is_not_found() {
    let env = writer().tree(flat_tree(1)).build();
    let request = env.host.submit_directly(sub(1, rename(2, "a"))).expect("id");
    env.settle();
    env.host.advance_clock(Duration::from_secs(31));
    assert_eq!(env.host.take_result(request), Err(ErrorCode::NotFound));
}

/// REQ: RQO-008
#[test]
fn rqo_008_second_take_is_not_found() {
    let env = writer().tree(flat_tree(1)).build();
    let request = env.host.submit_directly(sub(1, rename(2, "a"))).expect("id");
    env.settle();
    assert!(env.host.take_result(request).is_ok());
    assert_eq!(env.host.take_result(request), Err(ErrorCode::NotFound));
}

/// REQ: RQO-008
#[test]
fn rqo_008_take_at_29_999_ms_still_works_and_at_30_000_ms_is_gone() {
    for (millis, expect_found) in [(29_999u64, true), (30_000, false)] {
        let env = writer().tree(flat_tree(1)).build();
        let request = env.host.submit_directly(sub(1, rename(2, "a"))).expect("id");
        env.settle();
        env.host.advance_clock(Duration::from_millis(millis));
        assert_eq!(env.host.take_result(request).is_ok(), expect_found, "at {millis} ms");
    }
}

/// REQ: RQO-009
#[test]
fn rqo_009_both_defects_report_the_malformed_one_with_its_index() {
    let env = Setup::new(&["tree.write", "terminal.write"]).tree(flat_tree(2)).build();
    env.run(vec![sub(1, freeze(2)), sub(2, send_input(2, "x"))]);
    assert!(is_refused(&env.outcome(2), ErrorCode::InvalidArgument));
    assert!(message_of(&env.outcome(2)).contains("submission 1"), "{}", message_of(&env.outcome(2)));
    assert!(is_refused(&env.outcome(1), ErrorCode::Cancelled));
}

/// REQ: RQO-009
#[test]
fn rqo_009_a_lower_grant_offender_does_not_beat_a_later_malformed_one() {
    let env = Setup::new(&["terminal.write"]).tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, send_input(2, "x"))]);
    assert!(is_refused(&env.outcome(2), ErrorCode::InvalidArgument));
}

/// REQ: RQO-009
#[test]
fn rqo_009_the_lowest_index_malformed_is_reported() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, send_input(2, "x")), sub(2, send_input(2, "y"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    assert!(is_refused(&env.outcome(2), ErrorCode::Cancelled));
}

/// REQ: RQO-010
#[test]
fn rqo_010_257_submissions_trap_apply_nothing_and_complete_none() {
    let env = writer().tree(flat_tree(1)).build();
    env.run((1..=257).map(|n| sub(n, rename(2, "t"))).collect());
    assert!(env.outcomes().is_empty());
    assert!(env.applied().is_empty());
    assert_eq!(env.host.failures().len(), 1);
}

/// REQ: RQO-010
#[test]
fn rqo_010_256_submissions_are_accepted() {
    let env = writer().tree(flat_tree(1)).build();
    env.run((1..=256).map(|n| sub(n, rename(2, "t"))).collect());
    assert_eq!(env.outcomes().len(), 256);
    assert!(env.host.failures().is_empty());
}

/// REQ: RQO-010
#[test]
fn rqo_010_255_submissions_are_accepted() {
    let env = writer().tree(flat_tree(1)).build();
    env.run((1..=255).map(|n| sub(n, rename(2, "t"))).collect());
    assert_eq!(env.outcomes().len(), 255);
}

/// REQ: RQO-011
#[test]
fn rqo_011_a_refused_batch_applies_nothing_and_logs_the_zero_based_index() {
    let env = writer().tree(flat_tree(2)).build();
    env.run(vec![sub(1, rename(2, "a")), sub(2, rename(3, "b")), sub(3, send_input(2, "x"))]);
    assert!(env.applied().is_empty());
    assert!(!log_lines_containing(&env, "batch refused: submission 2").is_empty());
}

/// REQ: RQO-011
#[test]
fn rqo_011_no_dedup_key_of_a_refused_batch_is_reserved() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k:1", send_input(2, "a")), sub(2, send_input(2, "bad"))]);
    env.run(vec![keyed(3, "k:1", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(3)), "{:?}", env.outcome(3));
}

/// REQ: RQO-011
#[test]
fn rqo_011_offender_at_index_zero_is_logged_as_submission_0() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![sub(1, send_input(2, "x")), sub(2, rename(2, "a"))]);
    assert!(!log_lines_containing(&env, "batch refused: submission 0").is_empty());
}

/// REQ: RQO-020
#[test]
fn rqo_020_a_129_character_key_is_refused_invalid_argument() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, &"a".repeat(129), send_input(2, "x"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
}

/// REQ: RQO-020
#[test]
fn rqo_020_a_128_character_key_is_accepted() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, &"a".repeat(128), send_input(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: RQO-020
#[test]
fn rqo_020_a_one_character_key_is_accepted_and_an_empty_key_is_refused() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "a", send_input(2, "x"))]);
    assert!(is_ok(&env.outcome(1)));
    env.run(vec![keyed(2, "", send_input(2, "x"))]);
    assert!(is_refused(&env.outcome(2), ErrorCode::InvalidArgument));
}

/// REQ: RQO-020
#[test]
fn rqo_020_a_space_or_non_ascii_character_is_refused() {
    let env = writer().tree(flat_tree(1)).build();
    for (n, key) in [(1u64, "a b"), (2, "caf\u{e9}"), (3, "a*b")] {
        env.run(vec![keyed(n, key, send_input(2, "x"))]);
        assert!(is_refused(&env.outcome(n), ErrorCode::InvalidArgument), "key {key:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    // REQ: RQO-020
    #[test]
    fn rqo_020_keys_of_the_documented_grammar_are_accepted(key in "[A-Za-z0-9._:/-]{1,128}") {
        let env = writer().tree(flat_tree(1)).build();
        env.run(vec![keyed(1, &key, send_input(2, "x"))]);
        prop_assert!(is_ok(&env.outcome(1)));
    }

    // REQ: RQO-020
    #[test]
    fn rqo_020_a_key_with_one_foreign_character_is_refused(prefix in "[a-z]{0,10}", bad in "[ !#$%&*+,;<=>?@\\[\\]^`{|}~]") {
        let env = writer().tree(flat_tree(1)).build();
        env.run(vec![keyed(1, &format!("{prefix}{bad}"), send_input(2, "x"))]);
        prop_assert!(is_refused(&env.outcome(1), ErrorCode::InvalidArgument));
    }
}

/// REQ: RQO-021
#[test]
fn rqo_021_two_plugins_with_the_same_key_never_collide() {
    let env = writer().tree(flat_tree(1)).build();
    let other = env.host.add_second_plugin(&["terminal.write"]);
    env.run(vec![keyed(1, "shared", send_input(2, "a"))]);
    let outcome = other.submit_and_wait(keyed(1, "shared", send_input(2, "a")));
    assert!(is_ok(&Some(outcome)));
}

/// REQ: RQO-021
#[test]
fn rqo_021_the_key_is_reserved_for_10_minutes() {
    let mut env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k", send_input(2, "a"))]);
    env.host.advance_clock(Duration::from_secs(599));
    env.run(vec![keyed(2, "k", send_input(2, "a"))]);
    assert!(is_duplicate_of(&env.outcome(2), 1));
    env.host.advance_clock(Duration::from_secs(2));
    env.run(vec![keyed(3, "k", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(3)));
}

/// REQ: RQO-022
#[test]
fn rqo_022_failed_then_retry_with_the_same_key_is_duplicate() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.fail_next_write(ErrorCode::Io);
    env.run(vec![keyed(1, "k", send_input(2, "a"))]);
    assert!(is_failed(&env.outcome(1), ErrorCode::Io), "{:?}", env.outcome(1));
    env.run(vec![keyed(2, "k", send_input(2, "a"))]);
    assert!(is_duplicate_of(&env.outcome(2), 1));
}

/// REQ: RQO-022
#[test]
fn rqo_022_a_retry_with_a_new_key_is_applied() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.fail_next_write(ErrorCode::Io);
    env.run(vec![keyed(1, "k", send_input(2, "a"))]);
    env.run(vec![keyed(2, "k2", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(2)));
}

/// REQ: RQO-022
#[test]
fn rqo_022_the_old_key_is_free_after_600_000_ms() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.fail_next_write(ErrorCode::Io);
    env.run(vec![keyed(1, "k", send_input(2, "a"))]);
    env.host.advance_clock(Duration::from_millis(600_000));
    env.run(vec![keyed(2, "k", send_input(2, "a"))]);
    assert!(is_ok(&env.outcome(2)));
}

/// REQ: RQO-030
#[test]
fn rqo_030_after_a_restart_a_resubmit_within_10_minutes_is_duplicate() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    env.host.restart_server();
    env.host.advance_clock(Duration::from_secs(60));
    env.run(vec![keyed(2, "k", send_input_journaled(2, "a"))]);
    assert!(is_duplicate_of(&env.outcome(2), 1), "{:?}", env.outcome(2));
}

/// REQ: RQO-030
#[test]
fn rqo_030_after_a_restart_a_resubmit_after_10_minutes_is_accepted_and_input_is_not_replayed() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    env.host.restart_server();
    env.host.advance_clock(Duration::from_secs(601));
    let writes_before = env.kernel.step_log().iter().filter(|s| s.starts_with("pane-write")).count();
    assert_eq!(writes_before, 1, "no replay on restart");
    env.run(vec![keyed(2, "k", send_input_journaled(2, "a"))]);
    assert!(is_ok(&env.outcome(2)));
}

/// REQ: RQO-030
#[test]
fn rqo_030_the_journal_entry_precedes_the_terminal_write() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    let steps = env.kernel.step_log();
    let journal = steps.iter().position(|s| s == "journal:k").expect("journal");
    let write = steps.iter().position(|s| s.starts_with("pane-write")).expect("write");
    assert!(journal < write);
}

/// REQ: RQO-031
#[test]
fn rqo_031_a_restart_during_a_send_input_gives_exactly_one_input_uncertain() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.interrupt_after_journal(NodeId(2), "k");
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    env.host.restart_server();
    env.settle();
    assert_eq!(env.probe.count_of("input-uncertain"), 1);
}

/// REQ: RQO-031
#[test]
fn rqo_031_a_second_restart_does_not_repeat_input_uncertain_and_never_replays() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.interrupt_after_journal(NodeId(2), "k");
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    env.host.restart_server();
    env.settle();
    env.host.restart_server();
    env.settle();
    assert_eq!(env.probe.count_of("input-uncertain"), 1);
    let writes = env.kernel.step_log().iter().filter(|s| s.starts_with("pane-write")).count();
    assert_eq!(writes, 0);
}

/// REQ: RQO-031
#[test]
fn rqo_031_a_completed_send_input_gives_no_input_uncertain_after_a_restart() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![keyed(1, "k", send_input_journaled(2, "a"))]);
    env.host.restart_server();
    env.settle();
    assert_eq!(env.probe.count_of("input-uncertain"), 0);
}

/// REQ: RQO-040
#[test]
fn rqo_040_a_veto_becomes_refused_conflict_with_extension_vetoed_prefix() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.veto_next_hook("no way");
    env.run(vec![sub(1, rename(2, "a"))]);
    assert!(is_refused(&env.outcome(1), ErrorCode::Conflict), "{:?}", env.outcome(1));
    assert!(message_of(&env.outcome(1)).starts_with("extension.vetoed: "));
}

/// REQ: RQO-040
#[test]
fn rqo_040_no_veto_leaves_the_outcome_untouched() {
    let env = writer().tree(flat_tree(1)).build();
    env.run(vec![sub(1, rename(2, "a"))]);
    assert!(is_ok(&env.outcome(1)));
}

/// REQ: RQO-040
#[test]
fn rqo_040_a_vetoed_submission_applies_nothing() {
    let env = writer().tree(flat_tree(1)).build();
    env.kernel.veto_next_hook("no");
    env.run(vec![sub(1, rename(2, "a"))]);
    assert!(env.applied().is_empty());
}
