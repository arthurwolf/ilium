//! reference-test-harness.md "Delivering events and reading submissions": DEV-430 to DEV-436 and DEV-447.

use ilium_plugin_sdk_core::events;
use ilium_plugin_sdk_core::types::{
    AgentActivity, CommandOrigin, ErrorCode, Event, NodeId, Outcome, RequestRef,
};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::FakeHost;
use proptest::prelude::*;

use crate::common::{
    assert_fails_with, client_batch, close_node, outcome_code, sound, sound_keyed, spy, spy_edit,
    spy_reset, spy_script, with_clean_env, Spy, DEFAULT_CLOCK_MS,
};

fn sound_host() -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new().grant("sound.play");
    host.activate::<Spy>();
    host
}

// ---- DEV-430 ---------------------------------------------------------------------------------

/// REQ: DEV-430
#[test]
fn dev_430_events_reach_the_plugin_in_one_batch_in_the_given_order() {
    with_clean_env(|| {
        let mut host = sound_host();
        let events = client_batch(5);
        host.deliver(events.clone());
        let log = spy();
        assert_eq!(log.batches.len(), 1);
        assert_eq!(log.batches[0], events);
    });
}

/// REQ: DEV-430
#[test]
fn dev_430_a_batch_of_256_events_is_accepted() {
    with_clean_env(|| {
        let mut host = sound_host();
        host.deliver(client_batch(256));
        assert_eq!(spy().batches[0].len(), 256);
    });
}

/// REQ: DEV-430
#[test]
fn dev_430_a_batch_of_255_events_is_accepted() {
    with_clean_env(|| {
        let mut host = sound_host();
        host.deliver(client_batch(255));
        assert_eq!(spy().batches[0].len(), 255);
    });
}

/// REQ: DEV-430
#[test]
fn dev_430_a_batch_of_257_events_fails_with_batch_too_large() {
    with_clean_env(|| {
        let mut host = sound_host();
        assert_fails_with(codes::BATCH_TOO_LARGE, || host.deliver(client_batch(257)));
        assert!(spy().batches.is_empty());
    });
}

/// REQ: DEV-430
#[test]
fn dev_430_deliver_one_runs_on_events_once_with_that_event() {
    with_clean_env(|| {
        let mut host = sound_host();
        let event = Event::ClientAttached(ilium_plugin_sdk_core::types::ClientId(9));
        host.deliver_one(event.clone());
        assert_eq!(spy().batches, vec![vec![event]]);
    });
}

/// REQ: DEV-430
#[test]
fn dev_430_deliver_before_activate_fails_with_not_active() {
    with_clean_env(|| {
        let mut host = FakeHost::new();
        assert_fails_with(codes::NOT_ACTIVE, || host.deliver(client_batch(1)));
    });
}

// ---- DEV-431 ---------------------------------------------------------------------------------

/// REQ: DEV-431
#[test]
fn dev_431_deliver_returns_the_submissions_of_the_plugin_without_applying_them() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1), sound(2)]]);
        let submissions = host.deliver(client_batch(1));
        assert_eq!(submissions, vec![sound(1), sound(2)]);
        assert!(host.outcomes().is_empty());
        assert_eq!(host.pending_requests().len(), 2);
    });
}

/// REQ: DEV-431
#[test]
fn dev_431_submitted_records_every_submission_in_order() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1)], vec![sound(2), sound(3)]]);
        host.deliver(client_batch(1));
        host.deliver(client_batch(1));
        assert_eq!(host.submitted(), vec![sound(1), sound(2), sound(3)]);
    });
}

/// REQ: DEV-431
#[test]
fn dev_431_a_batch_with_no_submission_returns_an_empty_list() {
    with_clean_env(|| {
        let mut host = sound_host();
        assert!(host.deliver(client_batch(2)).is_empty());
        assert!(host.submitted().is_empty());
    });
}

// ---- DEV-432 ---------------------------------------------------------------------------------

/// REQ: DEV-432
#[test]
fn dev_432_deliver_does_not_deliver_the_outcome_by_itself() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        assert_eq!(spy().batches.len(), 1);
        assert_eq!(host.pending_requests().len(), 1);
    });
}

/// REQ: DEV-432
#[test]
fn dev_432_settle_delivers_the_outcome_as_a_request_completed_event() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.settle();
        let log = spy();
        assert_eq!(log.batches.len(), 2);
        assert_eq!(log.batches[1].len(), 1);
        match &log.batches[1][0] {
            Event::RequestCompleted(result) => {
                assert!(matches!(result.request, RequestRef::Submission(_)));
                assert_eq!(result.outcome, Outcome::Ok);
            }
            other => panic!("expected request-completed, got {other:?}"),
        }
        assert!(host.pending_requests().is_empty());
    });
}

/// REQ: DEV-432
#[test]
fn dev_432_settle_repeats_until_the_queue_is_empty() {
    with_clean_env(|| {
        let mut host = sound_host();
        let chain: Vec<_> = (1..=20u64).map(|index| vec![sound(index)]).collect();
        spy_script(chain);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(spy().batches.len(), 21);
        assert_eq!(host.outcomes().len(), 20);
        assert!(host.pending_requests().is_empty());
    });
}

/// REQ: DEV-432
#[test]
fn dev_432_settle_fails_with_settle_limit_when_the_plugin_never_stops_submitting() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_edit(|state| state.always = Some(sound(1)));
        host.deliver(client_batch(1));
        assert_fails_with(codes::SETTLE_LIMIT, || host.settle());
    });
}

/// REQ: DEV-432
#[test]
fn dev_432_settle_with_nothing_queued_returns_at_once() {
    with_clean_env(|| {
        let mut host = sound_host();
        host.settle();
        assert!(spy().batches.is_empty());
    });
}

// ---- DEV-433 ---------------------------------------------------------------------------------

/// REQ: DEV-433
#[test]
fn dev_433_257_submissions_from_one_batch_trap_with_submission_batch() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![(1..=257u64).map(sound).collect()]);
        assert_fails_with("trap.submission-batch", || host.deliver(client_batch(1)));
    });
}

/// REQ: DEV-433
#[test]
fn dev_433_256_submissions_from_one_batch_are_accepted() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![(1..=256u64).map(sound).collect()]);
        let submissions = host.deliver(client_batch(1));
        assert_eq!(submissions.len(), 256);
    });
}

/// REQ: DEV-433
#[test]
fn dev_433_255_submissions_from_one_batch_are_accepted() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![(1..=255u64).map(sound).collect()]);
        assert_eq!(host.deliver(client_batch(1)).len(), 255);
    });
}

// ---- DEV-434 ---------------------------------------------------------------------------------

/// REQ: DEV-434
#[test]
fn dev_434_a_missing_grant_refuses_the_offender_and_cancels_the_others() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1), close_node(2, 5), sound(3)]]);
        host.deliver(client_batch(1));
        host.settle();
        let outcomes = host.outcomes();
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcome_code(&outcomes[1]), Some(ErrorCode::NotPermitted));
        for index in [0usize, 2] {
            match &outcomes[index] {
                Outcome::Refused(error) => {
                    assert_eq!(error.code, ErrorCode::Cancelled);
                    assert_eq!(error.message, "batch refused: submission 1");
                }
                other => panic!("submission {index}: expected refused(cancelled), got {other:?}"),
            }
        }
    });
}

/// REQ: DEV-434
#[test]
fn dev_434_a_batch_where_every_submission_is_allowed_is_not_refused() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1), sound(2), sound(3)]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Ok, Outcome::Ok]);
    });
}

/// REQ: DEV-434
#[test]
fn dev_434_the_lowest_index_offender_names_the_batch_refusal() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1), close_node(2, 5), close_node(3, 6)]]);
        host.deliver(client_batch(1));
        host.settle();
        match &host.outcomes()[0] {
            Outcome::Refused(error) => assert_eq!(error.message, "batch refused: submission 1"),
            other => panic!("expected refused(cancelled), got {other:?}"),
        }
    });
}

/// REQ: DEV-434
#[test]
fn dev_434_a_malformed_submission_is_reported_before_a_missing_grant_inside_one_submission() {
    with_clean_env(|| {
        let mut host = sound_host();
        let mut offender = close_node(1, 5);
        offender.dedup_key = Some("bad key!".to_string());
        spy_script(vec![vec![offender]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            Some(ErrorCode::InvalidArgument)
        );
    });
}

// ---- DEV-435 ---------------------------------------------------------------------------------

fn run_with_key(key: &str) -> Outcome {
    let mut host = sound_host();
    spy_script(vec![vec![sound_keyed(1, key)]]);
    host.deliver(client_batch(1));
    host.settle();
    host.outcomes().remove(0)
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_of_every_allowed_character_class_is_accepted() {
    with_clean_env(|| {
        assert_eq!(run_with_key("Az09._:/-"), Outcome::Ok);
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_of_one_character_is_accepted() {
    with_clean_env(|| {
        assert_eq!(run_with_key("k"), Outcome::Ok);
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_of_128_characters_is_accepted() {
    with_clean_env(|| {
        assert_eq!(run_with_key(&"k".repeat(128)), Outcome::Ok);
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_of_127_characters_is_accepted() {
    with_clean_env(|| {
        assert_eq!(run_with_key(&"k".repeat(127)), Outcome::Ok);
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_of_129_characters_is_refused_with_dedup_key_invalid() {
    with_clean_env(|| match run_with_key(&"k".repeat(129)) {
        Outcome::Refused(error) => {
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(
                error.message.starts_with("dedup-key-invalid:"),
                "{}",
                error.message
            );
        }
        other => panic!("expected refused(invalid-argument), got {other:?}"),
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_an_empty_key_is_refused_with_dedup_key_invalid() {
    with_clean_env(|| match run_with_key("") {
        Outcome::Refused(error) => {
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(
                error.message.starts_with("dedup-key-invalid:"),
                "{}",
                error.message
            );
        }
        other => panic!("expected refused(invalid-argument), got {other:?}"),
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_key_with_a_space_is_refused_with_dedup_key_invalid() {
    with_clean_env(|| match run_with_key("two words") {
        Outcome::Refused(error) => {
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(
                error.message.starts_with("dedup-key-invalid:"),
                "{}",
                error.message
            );
        }
        other => panic!("expected refused(invalid-argument), got {other:?}"),
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_a_second_submission_with_the_same_key_inside_ten_minutes_is_a_duplicate() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![
            vec![sound_keyed(1, "same")],
            vec![sound_keyed(2, "same")],
        ]);
        host.deliver(client_batch(1));
        host.settle();
        host.advance_clock(599_000);
        host.deliver(client_batch(1));
        host.settle();
        let outcomes = host.outcomes();
        assert_eq!(outcomes[0], Outcome::Ok);
        assert!(
            matches!(outcomes[1], Outcome::Duplicate(_)),
            "{:?}",
            outcomes[1]
        );
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_after_ten_minutes_the_same_key_is_accepted_again() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![
            vec![sound_keyed(1, "same")],
            vec![sound_keyed(2, "same")],
        ]);
        host.deliver(client_batch(1));
        host.settle();
        host.advance_clock(601_000);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Ok]);
    });
}

/// REQ: DEV-435
#[test]
fn dev_435_two_different_keys_are_not_duplicates() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(1, "one"), sound_keyed(2, "two")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Ok]);
    });
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    // REQ: DEV-435
    #[test]
    fn dev_435_property_every_key_of_the_grammar_is_accepted(key in "[A-Za-z0-9._:/-]{1,128}") {
        let outcome = with_clean_env(|| run_with_key(&key));
        prop_assert_eq!(outcome, Outcome::Ok);
    }

    // REQ: DEV-435
    #[test]
    fn dev_435_property_a_key_with_a_character_outside_the_grammar_is_refused(
        head in "[A-Za-z0-9._:/-]{0,20}",
        bad in "[ !@#$%^&*()+=,;<>?|~`'\"]",
        tail in "[A-Za-z0-9._:/-]{0,20}",
    ) {
        let key = format!("{head}{bad}{tail}");
        let outcome = with_clean_env(|| run_with_key(&key));
        match outcome {
            Outcome::Refused(error) => {
                prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
                prop_assert!(error.message.starts_with("dedup-key-invalid:"));
            }
            other => prop_assert!(false, "expected refused, got {:?}", other),
        }
    }
}

// ---- DEV-436 ---------------------------------------------------------------------------------

/// REQ: DEV-436
#[test]
fn dev_436_deliver_does_not_move_the_clock() {
    with_clean_env(|| {
        let mut host = sound_host();
        for _ in 0..5 {
            host.deliver(client_batch(3));
        }
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS);
    });
}

/// REQ: DEV-436
#[test]
fn dev_436_time_moves_only_through_advance_clock() {
    with_clean_env(|| {
        let mut host = sound_host();
        host.advance_clock(250);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.clock().now(), DEFAULT_CLOCK_MS + 250);
    });
}

// ---- DEV-447 ---------------------------------------------------------------------------------

/// REQ: DEV-447
#[test]
fn dev_447_activity_builds_agent_activity_changed_with_revision_one() {
    let event = events::activity(41, AgentActivity::Idle, AgentActivity::Working);
    match event {
        Event::AgentActivityChanged(payload) => {
            assert_eq!(payload.node, NodeId(41));
            assert_eq!(payload.previous, AgentActivity::Idle);
            assert_eq!(payload.current, AgentActivity::Working);
            assert_eq!(payload.revision, 1);
        }
        other => panic!("expected agent-activity-changed, got {other:?}"),
    }
}

/// REQ: DEV-447
#[test]
fn dev_447_a_host_raises_the_revision_by_one_for_each_event_of_the_same_node() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grant("agent.read");
        host.activate::<Spy>();
        host.deliver(vec![
            events::activity(5, AgentActivity::Idle, AgentActivity::Working),
            events::activity(5, AgentActivity::Working, AgentActivity::Idle),
            events::activity(6, AgentActivity::Idle, AgentActivity::Working),
        ]);
        let revisions: Vec<(u64, u64)> = spy().batches[0]
            .iter()
            .filter_map(|event| match event {
                Event::AgentActivityChanged(payload) => Some((payload.node.0, payload.revision)),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, vec![(5, 1), (5, 2), (6, 1)]);
    });
}

/// REQ: DEV-447
#[test]
fn dev_447_command_builds_command_invoked_with_palette_origin_and_empty_context() {
    let event = events::command("example.demo/show", "{\"a\":1}");
    match event {
        Event::CommandInvoked(payload) => {
            assert_eq!(payload.command, "example.demo/show");
            assert_eq!(payload.arguments_json, "{\"a\":1}");
            assert_eq!(payload.origin, CommandOrigin::Palette);
            assert_eq!(payload.context.pane, None);
            assert_eq!(payload.context.view, None);
            assert_eq!(payload.context.buffer, None);
            assert_eq!(payload.context.line, None);
            assert_eq!(payload.context.virtual_row, None);
            assert_eq!(payload.context.menu_item, None);
        }
        other => panic!("expected command-invoked, got {other:?}"),
    }
}

/// REQ: DEV-447
#[test]
fn dev_447_every_builder_output_round_trips_through_the_event_variant() {
    let built = vec![
        events::activity(1, AgentActivity::None, AgentActivity::Done),
        events::command("a.b/c", "{}"),
        events::tree_changed(vec![NodeId(1), NodeId(2)]),
        events::node_removed(NodeId(3)),
    ];
    for event in built {
        let text = serde_json::to_string(&event).expect("event serializes");
        let back: Event = serde_json::from_str(&text).expect("event parses back");
        assert_eq!(back, event);
    }
}
