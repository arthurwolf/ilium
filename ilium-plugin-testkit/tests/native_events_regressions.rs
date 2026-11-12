//! Native event/request boundary regressions, beyond the original DEV-430..436 cases.
//! These tests exercise the actual owned NativeEntry callbacks and public FakeHost methods.

#[path = "sdk_fakehost/common.rs"]
pub mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::host::InstanceState;
use ilium_plugin_sdk_core::types::{
    Activation, DeactivationReason, ErrorCode, Event, Intent, Outcome, PluginError, RequestRef,
    Submission,
};
use ilium_plugin_sdk_core::NativeEntry;
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::FakeHost;

use common::{
    assert_fails_with, client_batch, close_node, outcome_code, sound, sound_keyed, spy, spy_reset,
    spy_script, with_clean_env, Spy,
};

fn sound_host() -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new().grant("sound.play");
    host.activate::<Spy>();
    host
}

#[derive(Debug, Clone, PartialEq)]
enum Visit {
    Activate(u64),
    Events(u64, Vec<Event>),
    Deactivate(u64, DeactivationReason),
    Drop(u64),
}

thread_local! {
    static VISITS: RefCell<Vec<Visit>> = const { RefCell::new(Vec::new()) };
    static REPLIES: RefCell<VecDeque<Vec<Submission>>> = const { RefCell::new(VecDeque::new()) };
    static PANIC_EVENTS: RefCell<bool> = const { RefCell::new(false) };
    static PANIC_DEACTIVATE: RefCell<bool> = const { RefCell::new(false) };
}

struct NonSendProbe {
    instance: u64,
    _thread_owned: Rc<()>,
}

impl Default for NonSendProbe {
    fn default() -> Self {
        Self {
            instance: 0,
            _thread_owned: Rc::new(()),
        }
    }
}

impl Drop for NonSendProbe {
    fn drop(&mut self) {
        VISITS.with(|visits| visits.borrow_mut().push(Visit::Drop(self.instance)));
    }
}

impl NativeEntry for NonSendProbe {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        self.instance = activation.instance;
        VISITS.with(|visits| visits.borrow_mut().push(Visit::Activate(self.instance)));
        Ok(())
    }

    fn native_deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        VISITS.with(|visits| {
            visits
                .borrow_mut()
                .push(Visit::Deactivate(self.instance, reason));
        });
        PANIC_DEACTIVATE.with(|panic| assert!(!*panic.borrow(), "deactivate callback panicked"));
        Some(vec![99])
    }

    fn native_on_events(&mut self, events: Vec<Event>) -> Vec<Submission> {
        VISITS.with(|visits| {
            visits
                .borrow_mut()
                .push(Visit::Events(self.instance, events))
        });
        PANIC_EVENTS.with(|panic| assert!(!*panic.borrow(), "event callback panicked"));
        REPLIES.with(|replies| replies.borrow_mut().pop_front().unwrap_or_default())
    }
}

fn probe_host(grants: &[&str], replies: Vec<Vec<Submission>>) -> FakeHost {
    VISITS.with(|visits| visits.borrow_mut().clear());
    REPLIES.with(|script| *script.borrow_mut() = replies.into());
    PANIC_EVENTS.with(|panic| *panic.borrow_mut() = false);
    PANIC_DEACTIVATE.with(|panic| *panic.borrow_mut() = false);
    let mut host = FakeHost::new().grants(grants);
    host.activate::<NonSendProbe>();
    host
}

fn visits() -> Vec<Visit> {
    VISITS.with(|visits| visits.borrow().clone())
}

fn completed(events: &[Event]) -> Vec<(RequestRef, Outcome)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::RequestCompleted(result) => {
                Some((result.request.clone(), result.outcome.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn native_events_empty_batch_still_calls_the_handler_once_without_moving_time() {
    with_clean_env(|| {
        let mut host = sound_host();
        let before = host.clock().now();
        assert!(host.deliver(Vec::new()).is_empty());
        assert_eq!(spy().batches, vec![Vec::<Event>::new()]);
        assert_eq!(host.clock().now(), before);
    });
}

#[test]
fn native_events_oversized_input_is_rejected_before_touching_the_owned_instance() {
    with_clean_env(|| {
        let mut host = sound_host();
        assert_fails_with(codes::BATCH_TOO_LARGE, || host.deliver(client_batch(257)));
        assert_eq!(host.lifecycle_state(), InstanceState::Active);
        host.deliver(client_batch(1));
        assert_eq!(spy().batches.len(), 1);
    });
}

#[test]
fn native_events_oversized_reply_traps_drops_the_instance_and_admits_nothing() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![(1..=257).map(sound).collect()]);
        assert_fails_with("trap.submission-batch", || host.deliver(client_batch(1)));
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert!(host.submitted().is_empty());
        assert!(host.pending_requests().is_empty());
        assert!(host.outcomes().is_empty());
        assert_eq!(host.failures().len(), 1);
        assert_eq!(host.failures()[0].code, "trap.submission-batch");
        assert!(visits().contains(&Visit::Drop(1)));
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Deactivate(..))));
    });
}

#[test]
fn native_events_callback_panic_drops_non_send_instance_and_cancels_old_pending_work() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound_keyed(1, "pending")]]);
        host.deliver(client_batch(1));
        PANIC_EVENTS.with(|panic| *panic.borrow_mut() = true);
        assert_fails_with("trap.panic", || host.deliver(client_batch(1)));
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert!(host.pending_requests().is_empty());
        assert_eq!(host.outcomes(), vec![Outcome::Cancelled]);
        assert_eq!(host.failures()[0].code, "trap.panic");
        assert!(visits().contains(&Visit::Drop(1)));
        spy_reset();
        assert_eq!(host.activate::<Spy>().instance, 2);
        host.settle();
        assert!(spy().batches.is_empty());
    });
}

#[test]
fn native_events_later_malformed_shape_wins_over_an_earlier_missing_grant() {
    with_clean_env(|| {
        let mut host = sound_host();
        let mut malformed = sound(2);
        malformed.dedup_key = Some("bad key".into());
        spy_script(vec![vec![close_node(1, 7), malformed, sound(3)]]);
        host.deliver(client_batch(1));
        host.settle();
        let outcomes = host.outcomes();
        assert_eq!(outcome_code(&outcomes[1]), Some(ErrorCode::InvalidArgument));
        for index in [0, 2] {
            let Outcome::Refused(error) = &outcomes[index] else {
                panic!("expected refusal")
            };
            assert_eq!(error.code, ErrorCode::Cancelled);
            assert_eq!(error.message, "batch refused: submission 1");
        }
    });
}

#[test]
fn native_events_refused_batch_reserves_none_of_its_keys() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(1, "retry"), close_node(2, 7)]]);
        host.deliver(client_batch(1));
        host.settle();
        spy_script(vec![vec![sound_keyed(3, "retry")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes().last(), Some(&Outcome::Ok));
    });
}

#[test]
fn native_events_same_key_in_one_batch_points_to_the_first_request() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(11, "same"), sound_keyed(12, "same")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            host.outcomes(),
            vec![Outcome::Ok, Outcome::Duplicate(RequestRef::Submission(11))]
        );
    });
}

#[test]
fn native_events_duplicate_does_not_refresh_the_exact_ten_minute_reservation() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(1, "window")]]);
        host.deliver(client_batch(1));
        host.settle();
        host.advance_clock(599_999);
        spy_script(vec![vec![sound_keyed(2, "window")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            host.outcomes().last(),
            Some(&Outcome::Duplicate(RequestRef::Submission(1)))
        );
        host.advance_clock(1);
        spy_script(vec![vec![sound_keyed(3, "window")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes().last(), Some(&Outcome::Ok));
    });
}

#[test]
fn native_events_accepted_failed_and_cancelled_work_keeps_its_dedup_key() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![
            sound_keyed(1, "failed"),
            sound_keyed(2, "cancelled"),
        ]]);
        host.deliver(client_batch(1));
        host.complete(
            RequestRef::Submission(1),
            Outcome::Failed(PluginError::new(ErrorCode::Io, "fake failure")),
        );
        host.complete(RequestRef::Submission(2), Outcome::Cancelled);
        host.settle();
        spy_script(vec![vec![
            sound_keyed(3, "failed"),
            sound_keyed(4, "cancelled"),
        ]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            &host.outcomes()[2..],
            &[
                Outcome::Duplicate(RequestRef::Submission(1)),
                Outcome::Duplicate(RequestRef::Submission(2)),
            ]
        );
    });
}

#[test]
fn native_events_reused_correlations_preserve_separate_pending_occurrences() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(7), sound(7)]]);
        host.deliver(client_batch(1));
        assert_eq!(
            host.pending_requests(),
            vec![RequestRef::Submission(7), RequestRef::Submission(7)]
        );
        host.complete(RequestRef::Submission(7), Outcome::Cancelled);
        assert_eq!(host.pending_requests(), vec![RequestRef::Submission(7)]);
        host.complete(RequestRef::Submission(7), Outcome::Ok);
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Cancelled, Outcome::Ok]);
        assert_eq!(
            completed(&spy().batches[1]),
            vec![
                (RequestRef::Submission(7), Outcome::Cancelled),
                (RequestRef::Submission(7), Outcome::Ok),
            ]
        );
    });
}

#[test]
fn native_events_history_uses_submission_order_while_events_keep_completion_order() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1), sound(2)]]);
        host.deliver(client_batch(1));
        host.complete(RequestRef::Submission(2), Outcome::Cancelled);
        assert_eq!(host.outcomes(), vec![Outcome::Cancelled]);
        host.complete(RequestRef::Submission(1), Outcome::Ok);
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Cancelled]);
        host.settle();
        assert_eq!(
            completed(&spy().batches[1]),
            vec![
                (RequestRef::Submission(2), Outcome::Cancelled),
                (RequestRef::Submission(1), Outcome::Ok),
            ]
        );
    });
}

#[test]
fn native_events_manual_completion_is_recorded_once_and_waits_for_settle_to_deliver() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(5)]]);
        host.deliver(client_batch(1));
        host.complete(RequestRef::Submission(5), Outcome::Ok);
        assert!(host.pending_requests().is_empty());
        assert_eq!(host.outcomes(), vec![Outcome::Ok]);
        assert_eq!(spy().batches.len(), 1);
        assert_fails_with(codes::REQUEST_NOT_PENDING, || {
            host.complete(RequestRef::Submission(5), Outcome::Ok)
        });
        assert_fails_with(codes::REQUEST_NOT_PENDING, || {
            host.complete(RequestRef::Submission(999), Outcome::Ok)
        });
        host.settle();
        host.settle();
        assert_eq!(spy().batches.len(), 2);
        assert_eq!(host.outcomes(), vec![Outcome::Ok]);
    });
}

#[test]
fn native_events_completion_backlog_is_split_into_ordered_batches_of_at_most_256() {
    with_clean_env(|| {
        let mut host = sound_host();
        for group in 0..12u64 {
            let submissions: Vec<_> = (1..=25).map(|offset| sound(group * 25 + offset)).collect();
            spy_script(vec![submissions]);
            host.deliver(client_batch(1));
            for offset in 1..=25 {
                host.complete(RequestRef::Submission(group * 25 + offset), Outcome::Ok);
            }
        }
        host.settle();
        let log = spy();
        assert_eq!(log.batches.len(), 14);
        assert_eq!(log.batches[12].len(), 256);
        assert_eq!(log.batches[13].len(), 44);
        let refs: Vec<_> = log.batches[12..]
            .iter()
            .flat_map(|events| completed(events))
            .map(|(request, _)| request)
            .collect();
        assert_eq!(
            refs,
            (1..=300).map(RequestRef::Submission).collect::<Vec<_>>()
        );
    });
}

#[test]
fn native_events_exactly_one_hundred_settle_rounds_can_finish() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script((1..=100).map(|id| vec![sound(id)]).collect());
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes().len(), 100);
        assert_eq!(spy().batches.len(), 101);
        assert!(host.pending_requests().is_empty());
    });
}

#[test]
fn native_events_stopping_retires_queued_events_and_cancels_pending_without_leaking_to_new_instance(
) {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1), sound(2)]]);
        host.deliver(client_batch(1));
        host.complete(RequestRef::Submission(1), Outcome::Ok);
        host.deactivate(DeactivationReason::Shutdown);
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Cancelled]);
        host.activate::<NonSendProbe>();
        host.settle();
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Events(2, _))));
    });
}

#[test]
fn native_events_explicit_restart_retains_dedup_but_never_restores_deactivate_bytes() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound_keyed(1, "persistent")]]);
        host.deliver(client_batch(1));
        host.restart_with(vec![1, 2]);
        assert_eq!(host.restored(), Some(vec![1, 2]));
        REPLIES.with(|replies| {
            replies
                .borrow_mut()
                .push_back(vec![sound_keyed(2, "persistent")])
        });
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            host.outcomes(),
            vec![
                Outcome::Cancelled,
                Outcome::Duplicate(RequestRef::Submission(1))
            ]
        );
        host.grant_now("tree.read").expect("grant");
        assert_eq!(host.restored(), None);
    });
}

#[test]
fn native_events_revocation_completes_only_affected_pending_work_before_old_deactivation() {
    with_clean_env(|| {
        let mut host = probe_host(
            &["sound.play", "tree.write"],
            vec![vec![sound(1), close_node(2, 7)]],
        );
        host.deliver(client_batch(1));
        host.grants_changed(&["tree.write"]);
        let log = visits();
        let revoked_position = log
            .iter()
            .position(|visit| match visit {
                Visit::Events(1, events) => completed(events).iter().any(|(request, outcome)| {
                    *request == RequestRef::Submission(1)
                        && outcome_code(outcome) == Some(ErrorCode::Revoked)
                }),
                _ => false,
            })
            .expect("old instance receives revoked completion");
        let deactivate_position = log
            .iter()
            .position(|visit| *visit == Visit::Deactivate(1, DeactivationReason::Revoked))
            .expect("revoked deactivation");
        assert!(revoked_position < deactivate_position);
        assert_eq!(host.outcomes().len(), 1);
        assert_eq!(outcome_code(&host.outcomes()[0]), Some(ErrorCode::Revoked));
        assert_eq!(host.pending_requests(), vec![RequestRef::Submission(2)]);
        host.complete(RequestRef::Submission(2), Outcome::Ok);
        host.settle();
        assert_eq!(host.outcomes().last(), Some(&Outcome::Ok));
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Events(2, _))));
        assert_eq!(host.restored(), None);
    });
}

#[test]
fn native_events_single_revoke_uses_the_same_pending_request_revocation_boundary() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.revoke("sound.play");
        assert_eq!(outcome_code(&host.outcomes()[0]), Some(ErrorCode::Revoked));
        assert!(visits().iter().any(
            |visit| matches!(visit, Visit::Events(1, events) if completed(events).len() == 1)
        ));
        assert!(visits().contains(&Visit::Deactivate(1, DeactivationReason::Revoked)));
    });
}

#[test]
fn native_events_grant_addition_keeps_old_pending_without_delivering_it_to_the_new_instance() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.grant_now("tree.read").expect("grant");
        assert!(host.outcomes().is_empty());
        assert_eq!(host.pending_requests(), vec![RequestRef::Submission(1)]);
        assert_eq!(
            visits()
                .iter()
                .filter(|visit| matches!(visit, Visit::Events(..)))
                .count(),
            1
        );
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok]);
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Events(2, _))));
    });
}

#[test]
fn native_events_revocation_callback_submissions_are_recorded_but_cannot_escape_retirement() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1)], vec![sound(2)]]);
        host.deliver(client_batch(1));
        host.revoke("sound.play");
        assert_eq!(host.submitted(), vec![sound(1), sound(2)]);
        assert_eq!(outcome_code(&host.outcomes()[0]), Some(ErrorCode::Revoked));
        assert_eq!(
            outcome_code(&host.outcomes()[1]),
            Some(ErrorCode::NotPermitted)
        );
        assert!(host.pending_requests().is_empty());
        host.settle();
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Events(2, _))));
    });
}

#[test]
fn native_events_invalid_grant_replacement_preserves_pending_work_and_the_owned_instance() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        assert_fails_with(codes::GRANT_UNKNOWN, || {
            host.grants_changed(&["not.a.real.grant"])
        });
        assert_eq!(host.snapshot().instance, 1);
        assert_eq!(host.pending_requests(), vec![RequestRef::Submission(1)]);
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok]);
    });
}

#[test]
fn native_events_unsupported_effect_never_fabricates_success_but_manual_completion_is_available() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new().grant("tree.write");
        host.activate::<Spy>();
        spy_script(vec![vec![close_node(1, 7)]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            Some(ErrorCode::Unsupported)
        );
        assert!(matches!(host.outcomes()[0], Outcome::Failed(_)));
        spy_script(vec![vec![close_node(2, 7)]]);
        host.deliver(client_batch(1));
        host.complete(RequestRef::Submission(2), Outcome::Ok);
        host.settle();
        assert_eq!(host.outcomes()[1], Outcome::Ok);
    });
}

#[test]
fn native_events_configured_auto_outcome_does_not_bypass_shape_or_grant_refusal() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        host.auto_outcome(Outcome::Ok);
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            Some(ErrorCode::NotPermitted)
        );
        assert!(matches!(host.outcomes()[0], Outcome::Refused(_)));
    });
}

#[test]
fn native_events_mandatory_dedup_key_is_checked_before_the_missing_grant() {
    with_clean_env(|| {
        spy_reset();
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        spy_script(vec![vec![Submission {
            intent: Intent::AgentAction(Default::default()),
            correlation: 9,
            dedup_key: None,
        }]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(
            outcome_code(&host.outcomes()[0]),
            Some(ErrorCode::InvalidArgument)
        );
        let outcomes = host.outcomes();
        let Outcome::Refused(error) = &outcomes[0] else {
            panic!("expected refusal")
        };
        assert!(error.message.starts_with("dedup-key-invalid:"));
    });
}

#[test]
fn native_events_successful_sound_fake_records_one_effect_for_duplicate_submissions() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(1, "sound"), sound_keyed(2, "sound")]]);
        host.deliver(client_batch(1));
        assert!(host.played_sounds().is_empty());
        host.settle();
        assert_eq!(host.played_sounds(), vec!["ding".to_owned()]);
        assert_eq!(
            host.outcomes(),
            vec![Outcome::Ok, Outcome::Duplicate(RequestRef::Submission(1))]
        );
    });
}

#[test]
fn native_events_an_explicit_refusal_releases_its_reservation_for_a_retry() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound_keyed(1, "refused")]]);
        host.deliver(client_batch(1));
        host.complete(
            RequestRef::Submission(1),
            Outcome::Refused(PluginError::new(ErrorCode::Conflict, "test refusal")),
        );
        host.settle();
        spy_script(vec![vec![sound_keyed(2, "refused")]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes().last(), Some(&Outcome::Ok));
        assert_eq!(host.played_sounds(), vec!["ding".to_owned()]);
    });
}

#[test]
fn native_events_configured_auto_outcome_applies_only_to_later_submissions() {
    with_clean_env(|| {
        let mut host = sound_host();
        spy_script(vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.auto_outcome(Outcome::Cancelled);
        spy_script(vec![vec![sound(2)]]);
        host.deliver(client_batch(1));
        host.settle();
        assert_eq!(host.outcomes(), vec![Outcome::Ok, Outcome::Cancelled]);
        assert_eq!(host.played_sounds(), vec!["ding".to_owned()]);
    });
}

#[test]
fn native_events_later_revocation_also_rechecks_pending_work_from_an_older_instance() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        host.grant_now("tree.read").expect("grant");
        assert_eq!(host.pending_requests(), vec![RequestRef::Submission(1)]);
        host.revoke("sound.play");
        assert!(host.pending_requests().is_empty());
        assert_eq!(outcome_code(&host.outcomes()[0]), Some(ErrorCode::Revoked));
        host.settle();
        assert!(!visits()
            .iter()
            .any(|visit| matches!(visit, Visit::Events(2, _) | Visit::Events(3, _))));
    });
}

#[test]
fn native_events_panicking_grant_deactivation_retires_the_preserved_pending_work() {
    with_clean_env(|| {
        let mut host = probe_host(&["sound.play"], vec![vec![sound(1)]]);
        host.deliver(client_batch(1));
        PANIC_DEACTIVATE.with(|panic| *panic.borrow_mut() = true);
        assert_fails_with("deactivate callback panicked", || {
            host.grant_now("tree.read")
        });
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
        assert!(host.pending_requests().is_empty());
        assert_eq!(host.outcomes(), vec![Outcome::Cancelled]);
        assert!(visits().contains(&Visit::Drop(1)));
    });
}
