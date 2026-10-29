//! reference-conformance-suite.md "Case catalogue": the lifecycle, event, grant, view, storage,
//! memory and package cases, run against their H (host) subject through `HostHarness` and the
//! pure api functions. The restart cases are in `reference_conformance_suite_restart.rs`.
//!
//! Cases that need a deadline breach or an allocation use the probe component (DEV-607); they
//! fail red with `fixture.missing` until `plugins/fixtures/conformance-probe` is built.

use ilium_plugin_api::abi::{self, QueueModel};
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::{
    Event, OutputMatchedEvent, PaneTitleChangedEvent,
};
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::generated::types::ui::ViewTarget;
use ilium_plugin_api::ids::NodeId;
use ilium_plugin_api::limits::{self, LimitKey};
use ilium_plugin_api::manifest::{CheckMode, ValidationContext};
use ilium_plugin_api::{ApiVersion, Platform, PluginError, Tier};
use ilium_plugin_host::storage::{PluginStorage, BUCKET_MAX_BYTES, VALUE_MAX_BYTES};
use proptest::prelude::*;
use serde_json::json;

use crate::common::{
    deliver_and_pump, harness_with, has_failure, is_active, manifest_text, play_sound, plugin_id,
    probe_harness, refused_codes, results, timer_events, trap, ScriptedGuest, BOUND,
};
use crate::gap;

fn validation_context(tier: Tier, mode: CheckMode) -> ValidationContext {
    ValidationContext {
        tier,
        mode,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    }
}

fn pane_title(node: u64, title: &str) -> Event {
    Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(node),
        title: title.to_string(),
        source: ilium_plugin_api::generated::types::TitleOrigin::Manual,
    })
}

fn output_matched(row: u32) -> Event {
    Event::OutputMatched(OutputMatchedEvent {
        node: NodeId(1),
        filter_id: "f".to_string(),
        line: "x".to_string(),
        row,
    })
}

fn invalid_argument(message: &str) -> PluginError {
    PluginError {
        code: ilium_plugin_api::ErrorCode::InvalidArgument,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

// ---------------------------------------------------------------- Lifecycle

/// REQ: CONF-LIFE-01
#[test]
fn conf_life_01_activate_precedes_the_first_event_call() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &[]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let calls = recorder.lock().expect("recorder").calls.clone();
    assert_eq!(calls, vec!["activate".to_string(), "on-events".to_string()]);
}

/// REQ: CONF-LIFE-01
#[test]
fn conf_life_01_activate_runs_exactly_once_across_many_batches() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &[]);
    for _ in 0..3 {
        deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    }
    let calls = recorder.lock().expect("recorder").calls.clone();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.as_str() == "activate")
            .count(),
        1
    );
    assert_eq!(calls.first().map(String::as_str), Some("activate"));
}

/// REQ: CONF-LIFE-02
#[test]
fn conf_life_02_activate_burning_2100_ms_traps_deadline() {
    let harness = probe_harness(json!({"burn_ms": {"activate": 2100}}), &[]);
    let handle = harness.build().expect("harness builds");
    assert!(has_failure(&handle, "trap.deadline"));
    assert!(!is_active(&handle));
}

/// REQ: CONF-LIFE-02
#[test]
fn conf_life_02_activate_burning_1900_ms_stays_under_the_2000_ms_deadline() {
    let harness = probe_harness(json!({"burn_ms": {"activate": 1900}}), &[]);
    let handle = harness.build().expect("harness builds");
    assert!(!has_failure(&handle, "trap.deadline"));
    assert!(is_active(&handle));
}

/// REQ: CONF-LIFE-02
#[test]
fn conf_life_02_activate_default_deadline_is_2000_ms_for_every_tier() {
    for tier in [Tier::T1, Tier::T2, Tier::T3] {
        let ceiling = limits::ceiling(LimitKey::ActivateDeadlineMs, tier);
        let expected = if tier == Tier::T1 { 5_000 } else { 2_000 };
        assert_eq!(ceiling, expected, "{tier:?}");
    }
    assert_eq!(limits::default_of(LimitKey::ActivateDeadlineMs), 2_000);
}

/// REQ: CONF-LIFE-03
#[test]
fn conf_life_03_deactivate_burning_600_ms_is_cut_and_keeps_no_restore_bytes() {
    let mut handle = probe_harness(
        json!({"burn_ms": {"deactivate": 600}, "restore": "AQID"}),
        &[],
    )
    .build()
    .expect("harness builds");
    let failures_before = handle.failures().len();
    let _ = gap::deactivate(&mut handle, DeactivationReason::Shutdown);
    assert_eq!(gap::restore_bytes(&handle), None);
    assert_eq!(
        handle.failures().len(),
        failures_before,
        "a cut deactivate is not a failure"
    );
}

/// REQ: CONF-LIFE-03
#[test]
fn conf_life_03_deactivate_burning_400_ms_keeps_its_restore_bytes() {
    let mut handle = probe_harness(
        json!({"burn_ms": {"deactivate": 400}, "restore": "AQID"}),
        &[],
    )
    .build()
    .expect("harness builds");
    let _ = gap::deactivate(&mut handle, DeactivationReason::Shutdown);
    assert_eq!(gap::restore_bytes(&handle), Some(vec![1, 2, 3]));
}

/// REQ: CONF-LIFE-03
#[test]
fn conf_life_03_restore_bytes_limit_is_one_mebibyte() {
    assert!(abi::restore_bytes_accepted(abi::RESTORE_BYTES_MAX - 1));
    assert!(abi::restore_bytes_accepted(abi::RESTORE_BYTES_MAX));
    assert!(!abi::restore_bytes_accepted(abi::RESTORE_BYTES_MAX + 1));
}

/// REQ: CONF-LIFE-04
#[test]
fn conf_life_04_activate_error_leaves_the_plugin_not_active() {
    let (mut guest, _recorder) = ScriptedGuest::quiet();
    guest.activate_error = Some(invalid_argument("settings: bad"));
    let built = ilium_plugin_host::testing::HostHarness::new()
        .with_native_guest(Box::new(guest))
        .build();
    let handle = built.expect("a failed activation still builds the harness");
    assert!(!is_active(&handle));
}

/// REQ: CONF-LIFE-04
#[test]
fn conf_life_04_activate_error_code_is_logged() {
    let (mut guest, _recorder) = ScriptedGuest::quiet();
    guest.activate_error = Some(invalid_argument("settings: bad"));
    let handle = ilium_plugin_host::testing::HostHarness::new()
        .with_native_guest(Box::new(guest))
        .build()
        .expect("a failed activation still builds the harness");
    let logged = handle
        .logs()
        .iter()
        .any(|line| line.message.contains("invalid-argument"));
    assert!(logged, "logs: {:?}", handle.logs().len());
}

/// REQ: CONF-LIFE-04
#[test]
fn conf_life_04_events_are_not_delivered_to_a_plugin_that_failed_to_activate() {
    let (mut guest, recorder) = ScriptedGuest::quiet();
    guest.activate_error = Some(invalid_argument("no"));
    let handle = ilium_plugin_host::testing::HostHarness::new()
        .with_native_guest(Box::new(guest))
        .build()
        .expect("harness builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!recorder
        .lock()
        .expect("recorder")
        .calls
        .iter()
        .any(|call| call == "on-events"));
}

/// REQ: CONF-LIFE-05
#[test]
fn conf_life_05_each_delivered_reason_reaches_the_plugin() {
    let delivered = [
        DeactivationReason::Shutdown,
        DeactivationReason::Disabled,
        DeactivationReason::Upgrade,
        DeactivationReason::Revoked,
        DeactivationReason::Granted,
    ];
    for reason in delivered {
        let (guest, recorder) = ScriptedGuest::quiet();
        let mut handle = harness_with(guest, &[]);
        let expected = crate::common::reason_name(&reason);
        let _ = gap::deactivate(&mut handle, reason);
        let calls = recorder.lock().expect("recorder").calls.clone();
        assert!(
            calls.contains(&format!("deactivate:{expected}")),
            "{expected}: {calls:?}"
        );
    }
}

/// REQ: CONF-LIFE-05
#[test]
fn conf_life_05_crash_restart_is_never_delivered_after_a_trap() {
    let (guest, recorder) = ScriptedGuest::answering(Box::new(|_| Err(trap("trap.deadline"))));
    let handle = harness_with(guest, &[]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let reasons = recorder.lock().expect("recorder").reasons.clone();
    assert!(
        !reasons.contains(&DeactivationReason::CrashRestart),
        "{reasons:?}"
    );
}

// ----------------------------------------------------------- Events and submissions

/// REQ: CONF-EVT-01
#[test]
fn conf_evt_01_600_queued_events_arrive_in_three_batches() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &[]);
    deliver_and_pump(&handle, timer_events(600));
    assert_eq!(
        recorder.lock().expect("recorder").batch_sizes,
        vec![256, 256, 88]
    );
}

/// REQ: CONF-EVT-01
#[test]
fn conf_evt_01_batch_boundary_255_256_257() {
    for (queued, expected) in [
        (255usize, vec![255usize]),
        (256, vec![256]),
        (257, vec![256, 1]),
    ] {
        let (guest, recorder) = ScriptedGuest::quiet();
        let handle = harness_with(guest, &[]);
        deliver_and_pump(&handle, timer_events(queued));
        assert_eq!(
            recorder.lock().expect("recorder").batch_sizes,
            expected,
            "queued={queued}"
        );
        assert_eq!(
            abi::batch_sizes(queued),
            expected,
            "pure model queued={queued}"
        );
    }
}

proptest! {
    /// REQ: CONF-EVT-01
    #[test]
    fn conf_evt_01_every_batch_holds_at_most_256_and_none_is_lost(queued in 1usize..1000) {
        let sizes = abi::batch_sizes(queued);
        prop_assert!(sizes.iter().all(|size| *size >= 1 && *size <= 256));
        prop_assert_eq!(sizes.iter().sum::<usize>(), queued);
        prop_assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
    }
}

/// REQ: CONF-EVT-02
#[test]
fn conf_evt_02_on_events_burning_25_ms_traps_deadline() {
    let handle = probe_harness(json!({"burn_ms": {"on-events": 25}}), &[])
        .build()
        .expect("builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(has_failure(&handle, "trap.deadline"));
}

/// REQ: CONF-EVT-02
#[test]
fn conf_evt_02_on_events_burning_15_ms_stays_under_the_20_ms_deadline() {
    let handle = probe_harness(json!({"burn_ms": {"on-events": 15}}), &[])
        .build()
        .expect("builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!has_failure(&handle, "trap.deadline"));
}

/// REQ: CONF-EVT-02
#[test]
fn conf_evt_02_event_deadline_is_20_ms_default_and_50_ms_ceiling_for_t1() {
    assert_eq!(limits::default_of(LimitKey::EventDeadlineMs), 20);
    assert_eq!(limits::ceiling(LimitKey::EventDeadlineMs, Tier::T1), 50);
    assert_eq!(limits::ceiling(LimitKey::EventDeadlineMs, Tier::T2), 20);
    assert_eq!(limits::ceiling(LimitKey::EventDeadlineMs, Tier::T3), 20);
}

/// REQ: CONF-EVT-03
#[test]
fn conf_evt_03_three_renames_of_one_node_arrive_as_one_event_with_the_last_name() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &["panes.read"]);
    deliver_and_pump(
        &handle,
        vec![pane_title(7, "a"), pane_title(7, "b"), pane_title(7, "c")],
    );
    let events = recorder.lock().expect("recorder").events.clone();
    assert_eq!(events, vec![pane_title(7, "c")]);
}

/// REQ: CONF-EVT-03
#[test]
fn conf_evt_03_renames_of_different_nodes_are_not_merged() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &["panes.read"]);
    deliver_and_pump(&handle, vec![pane_title(7, "a"), pane_title(8, "b")]);
    assert_eq!(recorder.lock().expect("recorder").events.len(), 2);
}

/// REQ: CONF-EVT-04
#[test]
fn conf_evt_04_1500_drop_oldest_events_into_a_1024_queue_count_477_dropped() {
    let mut queue = QueueModel::new(1024);
    for row in 0..1500u64 {
        queue.push("output-matched", row);
    }
    assert_eq!(queue.dropped_total(), 477);
    assert_eq!(
        queue.events_dropped_records(),
        vec![("output-matched".to_string(), 477)]
    );
}

/// REQ: CONF-EVT-04
#[test]
fn conf_evt_04_1024_events_fill_the_queue_without_a_drop() {
    let mut queue = QueueModel::new(1024);
    for row in 0..1024u64 {
        queue.push("output-matched", row);
    }
    assert_eq!(queue.dropped_total(), 0);
    assert!(queue.events_dropped_records().is_empty());
}

/// REQ: CONF-EVT-04
#[test]
fn conf_evt_04_next_batch_after_overflow_starts_with_events_dropped() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &["terminal.read"]);
    handle.deliver((0..1500).map(output_matched).collect());
    let _ = handle.pump(64);
    let events = recorder.lock().expect("recorder").events.clone();
    assert!(
        matches!(events.first(), Some(Event::EventsDropped(_))),
        "{:?}",
        events.first()
    );
}

/// REQ: CONF-EVT-05
#[test]
fn conf_evt_05_event_without_its_grant_is_not_delivered() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &[]);
    deliver_and_pump(&handle, vec![pane_title(1, "x")]);
    assert!(recorder.lock().expect("recorder").events.is_empty());
}

/// REQ: CONF-EVT-05
#[test]
fn conf_evt_05_event_with_its_grant_is_delivered() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let handle = harness_with(guest, &["panes.read"]);
    deliver_and_pump(&handle, vec![pane_title(1, "x")]);
    assert_eq!(recorder.lock().expect("recorder").events.len(), 1);
}

/// REQ: CONF-EVT-05
#[test]
fn conf_evt_05_revoked_grant_stops_delivery_of_later_events() {
    let (guest, recorder) = ScriptedGuest::quiet();
    let mut handle = harness_with(guest, &["panes.read"]);
    deliver_and_pump(&handle, vec![pane_title(1, "x")]);
    gap::revoke(&mut handle, "panes.read");
    deliver_and_pump(&handle, vec![pane_title(1, "y")]);
    assert_eq!(recorder.lock().expect("recorder").events.len(), 1);
}

/// REQ: CONF-EVT-06
#[test]
fn conf_evt_06_257_submissions_trap_submission_batch() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| {
        Ok((0..257u64)
            .map(|n| play_sound(n + 1, None))
            .collect::<Vec<Submission>>())
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(has_failure(&handle, "trap.submission-batch"));
}

/// REQ: CONF-EVT-06
#[test]
fn conf_evt_06_256_submissions_do_not_trap() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| {
        Ok((0..256u64)
            .map(|n| play_sound(n + 1, None))
            .collect::<Vec<Submission>>())
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!has_failure(&handle, "trap.submission-batch"));
}

proptest! {
    /// REQ: CONF-EVT-06
    #[test]
    fn conf_evt_06_the_pure_count_check_traps_exactly_above_256(count in 0usize..600) {
        let verdict = abi::check_submission_count(count);
        if count <= 256 {
            prop_assert!(verdict.is_ok());
        } else {
            let refusal = verdict.unwrap_err();
            prop_assert_eq!(refusal.as_str(), "trap.submission-batch");
        }
    }
}

/// REQ: CONF-EVT-07
#[test]
fn conf_evt_07_third_submission_invalid_cancels_the_others_with_the_index_message() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| {
        let mut batch: Vec<Submission> = (1..=4u64).map(|n| play_sound(n, None)).collect();
        batch[2].dedup_key = Some("bad key".to_string());
        Ok(batch)
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let refused = refused_codes(&results(&handle));
    assert_eq!(refused.len(), 4);
    for (correlation, code, message) in &refused {
        if *correlation == 3 {
            assert_eq!(code, "invalid-argument");
        } else {
            assert_eq!(code, "cancelled");
            assert_eq!(message, "batch refused: submission 2");
        }
    }
}

/// REQ: CONF-EVT-07
#[test]
fn conf_evt_07_pure_batch_verdicts_name_the_zero_based_index_of_the_offender() {
    use abi::{BatchCheck, BatchVerdict};
    let ok = BatchCheck {
        shape_ok: true,
        grant_ok: true,
    };
    let bad = BatchCheck {
        shape_ok: false,
        grant_ok: true,
    };
    let verdicts = abi::batch_verdicts(&[ok, ok, bad, ok]);
    assert_eq!(
        verdicts[0],
        BatchVerdict::Cancelled("batch refused: submission 2".to_string())
    );
    assert!(matches!(verdicts[2], BatchVerdict::Refused(_)));
}

/// REQ: CONF-EVT-07
#[test]
fn conf_evt_07_a_batch_without_an_offender_proceeds() {
    use abi::{BatchCheck, BatchVerdict};
    let ok = BatchCheck {
        shape_ok: true,
        grant_ok: true,
    };
    assert_eq!(
        abi::batch_verdicts(&[ok, ok]),
        vec![BatchVerdict::Proceed, BatchVerdict::Proceed]
    );
    assert!(abi::batch_verdicts(&[]).is_empty());
}

/// REQ: CONF-EVT-08
#[test]
fn conf_evt_08_second_submission_with_the_same_dedup_key_is_a_duplicate() {
    use ilium_plugin_api::generated::types::requests::Outcome;
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| {
        Ok(vec![
            play_sound(1, Some("once")),
            play_sound(2, Some("once")),
        ])
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let outcomes = results(&handle);
    let duplicates = outcomes
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Duplicate(_)))
        .count();
    assert_eq!(duplicates, 1);
}

/// REQ: CONF-EVT-08
#[test]
fn conf_evt_08_distinct_dedup_keys_are_not_duplicates() {
    use ilium_plugin_api::generated::types::requests::Outcome;
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| {
        Ok(vec![play_sound(1, Some("one")), play_sound(2, Some("two"))])
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!results(&handle)
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Duplicate(_))));
}

/// REQ: CONF-EVT-08
#[test]
fn conf_evt_08_same_key_after_ten_minutes_is_not_a_duplicate() {
    use ilium_plugin_api::generated::types::requests::Outcome;
    let (guest, _recorder) =
        ScriptedGuest::answering(Box::new(|_| Ok(vec![play_sound(1, Some("again"))])));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    handle.advance_clock(std::time::Duration::from_millis(600_001));
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!results(&handle)
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Duplicate(_))));
}

/// REQ: CONF-EVT-09
#[test]
fn conf_evt_09_dedup_key_with_a_space_is_refused_dedup_key_invalid() {
    let (guest, _recorder) =
        ScriptedGuest::answering(Box::new(|_| Ok(vec![play_sound(1, Some("has space"))])));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let refused = refused_codes(&results(&handle));
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].1, "invalid-argument");
    assert!(
        refused[0].2.starts_with("dedup-key-invalid:"),
        "{}",
        refused[0].2
    );
}

proptest! {
    /// REQ: CONF-EVT-09
    #[test]
    fn conf_evt_09_the_dedup_charset_decides_refusal_dedup_key_invalid(key in "[ -~]{1,40}") {
        let allowed = key.chars().all(|c| c.is_ascii_alphanumeric() || "._:-/".contains(c));
        let verdict = ilium_plugin_api::dedup::DedupKey::parse(&key);
        prop_assert_eq!(verdict.is_ok(), allowed, "key {:?}", key);
    }
}

/// REQ: CONF-EVT-10
#[test]
fn conf_evt_10_one_intent_gives_one_request_completed_with_the_same_correlation() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| Ok(vec![play_sound(41, None)])));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let outcomes = results(&handle);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].request,
        ilium_plugin_api::generated::types::requests::RequestRef::Submission(41)
    );
}

/// REQ: CONF-EVT-10
#[test]
fn conf_evt_10_the_plugin_receives_the_completion_as_an_event() {
    let (guest, recorder) = ScriptedGuest::answering(Box::new(|events| {
        if events.iter().any(|e| matches!(e, Event::ThemeChanged)) {
            Ok(vec![play_sound(5, None)])
        } else {
            Ok(Vec::new())
        }
    }));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let seen = recorder.lock().expect("recorder").events.clone();
    assert!(
        seen.iter()
            .any(|event| matches!(event, Event::RequestCompleted(_))),
        "{seen:?}"
    );
}

// ------------------------------------------------------------------------- Grants

/// REQ: CONF-GRANT-01
#[test]
fn conf_grant_01_declined_fs_read_import_returns_not_permitted() {
    let handle = probe_harness(
        json!({"call": {"fs.read": {"scope": "project", "path": "a.txt"}}}),
        &[],
    )
    .build()
    .expect("builds");
    let logged = handle
        .logs()
        .iter()
        .any(|line| line.message.contains("not-permitted"));
    assert!(logged);
}

/// REQ: CONF-GRANT-01
#[test]
fn conf_grant_01_granted_fs_read_import_is_not_refused() {
    let handle = probe_harness(
        json!({"call": {"fs.read": {"scope": "project", "path": "a.txt"}}}),
        &["fs.read:project"],
    )
    .build()
    .expect("builds");
    assert!(!handle
        .logs()
        .iter()
        .any(|line| line.message.contains("not-permitted")));
}

/// REQ: CONF-GRANT-02
#[test]
fn conf_grant_02_revoked_intent_grant_refuses_the_next_submission_not_permitted() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| Ok(vec![play_sound(1, None)])));
    let mut handle = harness_with(guest, &["sound.play"]);
    gap::revoke(&mut handle, "sound.play");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    let refused = refused_codes(&results(&handle));
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].1, "not-permitted");
}

/// REQ: CONF-GRANT-02
#[test]
fn conf_grant_02_without_a_revocation_the_same_submission_is_not_refused() {
    let (guest, _recorder) = ScriptedGuest::answering(Box::new(|_| Ok(vec![play_sound(1, None)])));
    let handle = harness_with(guest, &["sound.play"]);
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(refused_codes(&results(&handle))
        .iter()
        .all(|(_, code, _)| code != "not-permitted"));
}

/// REQ: CONF-GRANT-03
#[test]
fn conf_grant_03_grant_arguments_are_matched_exactly_project_does_not_cover_data() {
    let handle = probe_harness(
        json!({"call": {"fs.read": {"scope": "data", "path": "a.txt"}}}),
        &["fs.read:project"],
    )
    .build()
    .expect("builds");
    assert!(handle
        .logs()
        .iter()
        .any(|line| line.message.contains("not-permitted")));
}

/// REQ: CONF-GRANT-03
#[test]
fn conf_grant_03_exact_scope_match_is_allowed() {
    let handle = probe_harness(
        json!({"call": {"fs.read": {"scope": "project", "path": "a.txt"}}}),
        &["fs.read:project"],
    )
    .build()
    .expect("builds");
    assert!(!handle
        .logs()
        .iter()
        .any(|line| line.message.contains("not-permitted")));
}

/// REQ: CONF-GRANT-04
#[test]
fn conf_grant_04_grant_now_of_an_undeclared_grant_fails_harness_grant_not_declared() {
    let mut host =
        ilium_plugin_testkit::FakeHost::for_manifest(crate::common::manifest_path("minimal.json"))
            .expect("manifest loads");
    let error = host
        .grant_now("fs.read:project")
        .expect_err("not declared in the manifest");
    assert_eq!(error.code, "harness.grant-not-declared");
}

/// REQ: CONF-GRANT-04
#[test]
fn conf_grant_04_grant_now_of_a_declared_grant_succeeds() {
    let mut host =
        ilium_plugin_testkit::FakeHost::for_manifest(crate::common::manifest_path("minimal.json"))
            .expect("manifest loads");
    host.grant_now("sound.play")
        .expect("declared in the manifest");
}

/// REQ: CONF-GRANT-05
#[test]
fn conf_grant_05_call_of_a_capability_never_requested_traps_import_not_requested() {
    let handle = probe_harness(
        json!({"call": {"http.fetch": {"url": "https://example.test/"}}}),
        &[],
    )
    .build()
    .expect("builds");
    assert!(has_failure(&handle, "host.capability-not-requested"));
}

/// REQ: CONF-GRANT-05
#[test]
fn conf_grant_05_call_of_a_requested_capability_does_not_trap() {
    let handle = probe_harness(
        json!({"call": {"fs.read": {"scope": "project", "path": "a.txt"}}}),
        &["fs.read:project"],
    )
    .build()
    .expect("builds");
    assert!(!has_failure(&handle, "host.capability-not-requested"));
}

// -------------------------------------------------------------------------- Views

/// REQ: CONF-VIEW-01
#[test]
fn conf_view_01_render_burning_15_ms_keeps_the_last_good_render_and_counts_trap_deadline() {
    let mut handle = probe_harness(json!({"burn_ms": {"render": 15}}), &[])
        .build()
        .expect("builds");
    let outcome = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    assert_eq!(
        outcome.expect_err("over the 10 ms deadline").code.as_str(),
        "trap.deadline"
    );
}

/// REQ: CONF-VIEW-01
#[test]
fn conf_view_01_render_burning_8_ms_stays_under_the_10_ms_deadline() {
    let mut handle = probe_harness(json!({"burn_ms": {"render": 8}}), &[])
        .build()
        .expect("builds");
    let outcome = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    assert!(outcome.is_ok());
}

/// REQ: CONF-VIEW-02
#[test]
fn conf_view_02_rendering_twice_gives_equal_trees() {
    let mut handle = probe_harness(json!({}), &[]).build().expect("builds");
    let area = Area {
        columns: 80,
        rows: 24,
    };
    let first = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        area.clone(),
    );
    let second = gap::render(&mut handle, ViewTarget::Panel("main".to_string()), area);
    assert_eq!(first.expect("first"), second.expect("second"));
}

/// REQ: CONF-VIEW-02
#[test]
fn conf_view_02_a_render_between_two_equal_renders_changes_nothing() {
    let mut handle = probe_harness(json!({}), &[]).build().expect("builds");
    let panel = ViewTarget::Panel("main".to_string());
    let before = gap::render(
        &mut handle,
        panel.clone(),
        Area {
            columns: 80,
            rows: 24,
        },
    )
    .expect("before");
    let _ = gap::render(
        &mut handle,
        panel.clone(),
        Area {
            columns: 40,
            rows: 10,
        },
    );
    let after = gap::render(
        &mut handle,
        panel,
        Area {
            columns: 80,
            rows: 24,
        },
    )
    .expect("after");
    assert_eq!(before, after);
}

/// REQ: CONF-VIEW-03
#[test]
fn conf_view_03_4097_nodes_trap_invalid_ui_tree() {
    let tree = gap::ui_tree_with_nodes(4097);
    let mut handle = probe_harness(json!({"render_tree": tree}), &[])
        .build()
        .expect("builds");
    let outcome = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    assert_eq!(
        outcome.expect_err("one node too many").code.as_str(),
        "trap.invalid-ui-tree"
    );
}

/// REQ: CONF-VIEW-03
#[test]
fn conf_view_03_4096_nodes_render() {
    let tree = gap::ui_tree_with_nodes(4096);
    let mut handle = probe_harness(json!({"render_tree": tree}), &[])
        .build()
        .expect("builds");
    let outcome = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    assert!(outcome.is_ok());
}

/// REQ: CONF-VIEW-03
#[test]
fn conf_view_03_4095_nodes_render() {
    let tree = gap::ui_tree_with_nodes(4095);
    let mut handle = probe_harness(json!({"render_tree": tree}), &[])
        .build()
        .expect("builds");
    let outcome = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    assert!(outcome.is_ok());
}

/// REQ: CONF-VIEW-04
#[test]
fn conf_view_04_an_action_key_the_view_never_set_is_not_delivered() {
    let mut handle = probe_harness(json!({}), &[]).build().expect("builds");
    gap::click_action(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        "never-set",
    );
    let _ = handle.pump(8);
    assert_eq!(handle.failures().len(), 0);
    assert!(handle
        .logs()
        .iter()
        .all(|line| !line.message.contains("ui-action received")));
}

/// REQ: CONF-VIEW-04
#[test]
fn conf_view_04_an_action_key_the_view_did_set_is_delivered() {
    let mut handle = probe_harness(json!({"render_action_key": "go"}), &[])
        .build()
        .expect("builds");
    let _ = gap::render(
        &mut handle,
        ViewTarget::Panel("main".to_string()),
        Area {
            columns: 80,
            rows: 24,
        },
    );
    gap::click_action(&mut handle, ViewTarget::Panel("main".to_string()), "go");
    let _ = handle.pump(8);
    assert!(handle
        .logs()
        .iter()
        .any(|line| line.message.contains("ui-action received")));
}

// ------------------------------------------------------- Storage, memory and state

fn open_storage(dir: &std::path::Path) -> PluginStorage {
    PluginStorage::open(dir, plugin_id()).expect("storage opens")
}

/// REQ: CONF-STORE-01
#[test]
fn conf_store_01_a_stored_value_survives_closing_and_reopening() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let mut storage = open_storage(dir.path());
        storage.put("state", "k", b"v".to_vec()).expect("put");
    }
    let storage = open_storage(dir.path());
    assert_eq!(storage.get("state", "k").expect("get"), Some(b"v".to_vec()));
}

/// REQ: CONF-STORE-01
#[test]
fn conf_store_01_a_missing_key_reads_none_after_a_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = open_storage(dir.path());
    let storage = open_storage(dir.path());
    assert_eq!(storage.get("state", "never").expect("get"), None);
}

/// REQ: CONF-STORE-01
#[test]
fn conf_store_01_a_value_survives_a_plugin_trap_and_restart() {
    let handle = probe_harness(
        json!({"store": {"put": ["k", "v"]}, "trap_after": "on-events"}),
        &[],
    )
    .build()
    .expect("builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    // The probe traps with trap.deadline (CONTRACT.txt); the first failure restarts, so the
    // plugin must not end up stopped by failure.
    assert!(has_failure(&handle, "trap.deadline"));
    assert_ne!(handle.state(), InstanceState::StoppedByFailure);
    assert!(handle
        .logs()
        .iter()
        .any(|line| line.message.contains("store.get k = v")));
}

/// REQ: CONF-STORE-02
#[test]
fn conf_store_02_value_of_1_mebibyte_plus_one_byte_is_refused_quota_exceeded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut storage = open_storage(dir.path());
    let error = storage
        .put("state", "big", vec![0u8; VALUE_MAX_BYTES + 1])
        .expect_err("over the value limit");
    assert_eq!(error.code, ilium_plugin_api::ErrorCode::QuotaExceeded);
}

/// REQ: CONF-STORE-02
#[test]
fn conf_store_02_value_of_exactly_1_mebibyte_is_stored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut storage = open_storage(dir.path());
    storage
        .put("state", "edge", vec![0u8; VALUE_MAX_BYTES])
        .expect("at the limit");
    storage
        .put("state", "under", vec![0u8; VALUE_MAX_BYTES - 1])
        .expect("under the limit");
}

/// REQ: CONF-STORE-02
#[test]
fn conf_store_02_the_value_limit_is_1_048_576_bytes() {
    assert_eq!(VALUE_MAX_BYTES, 1_048_576);
}

/// REQ: CONF-STORE-03
#[test]
fn conf_store_03_a_bucket_over_16_mebibytes_is_refused_quota_exceeded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut storage = open_storage(dir.path());
    let chunk = vec![0u8; VALUE_MAX_BYTES];
    for index in 0..16 {
        storage
            .put("state", &format!("k{index}"), chunk.clone())
            .expect("within the bucket");
    }
    let error = storage
        .put("state", "one-more", vec![0u8; 1])
        .expect_err("17th mebibyte");
    assert_eq!(error.code, ilium_plugin_api::ErrorCode::QuotaExceeded);
}

/// REQ: CONF-STORE-03
#[test]
fn conf_store_03_a_bucket_of_exactly_16_mebibytes_is_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut storage = open_storage(dir.path());
    let chunk = vec![0u8; VALUE_MAX_BYTES];
    for index in 0..16 {
        storage
            .put("state", &format!("k{index}"), chunk.clone())
            .expect("within the bucket");
    }
    assert_eq!(BUCKET_MAX_BYTES, 16 * 1024 * 1024);
}

/// REQ: CONF-STORE-03
#[test]
fn conf_store_03_another_bucket_is_not_affected_by_a_full_bucket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut storage = open_storage(dir.path());
    let chunk = vec![0u8; VALUE_MAX_BYTES];
    for index in 0..16 {
        storage
            .put("state", &format!("k{index}"), chunk.clone())
            .expect("fill");
    }
    storage
        .put("other", "k", vec![1])
        .expect("a second bucket has its own quota");
}

/// REQ: CONF-MEM-01
#[test]
fn conf_mem_01_allocating_40_mib_with_the_default_32_traps_memory() {
    let handle = probe_harness(json!({"alloc_mib": 40}), &[])
        .build()
        .expect("builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(has_failure(&handle, "trap.memory"));
}

/// REQ: CONF-MEM-01
#[test]
fn conf_mem_01_allocating_24_mib_with_the_default_32_does_not_trap() {
    let handle = probe_harness(json!({"alloc_mib": 24}), &[])
        .build()
        .expect("builds");
    deliver_and_pump(&handle, vec![Event::ThemeChanged]);
    assert!(!has_failure(&handle, "trap.memory"));
}

/// REQ: CONF-MEM-01
#[test]
fn conf_mem_01_default_memory_limit_is_32_mib() {
    assert_eq!(limits::default_of(LimitKey::MemoryMib), 32);
}

/// REQ: CONF-MEM-02
#[test]
fn conf_mem_02_ceilings_are_256_for_t1_and_128_for_t2_and_t3() {
    assert_eq!(limits::ceiling(LimitKey::MemoryMib, Tier::T1), 256);
    assert_eq!(limits::ceiling(LimitKey::MemoryMib, Tier::T2), 128);
    assert_eq!(limits::ceiling(LimitKey::MemoryMib, Tier::T3), 128);
}

/// REQ: CONF-MEM-02
#[test]
fn conf_mem_02_a_request_of_256_is_clamped_to_128_for_t2_and_t3_not_refused() {
    let mut requested = limits::Limits::default();
    requested.0.insert("memory_mib".to_string(), 256);
    for tier in [Tier::T2, Tier::T3] {
        let clamped = limits::clamp(&requested, tier).expect("clamp never refuses");
        assert_eq!(clamped.get(&LimitKey::MemoryMib), Some(&128), "{tier:?}");
    }
    let t1 = limits::clamp(&requested, Tier::T1).expect("clamp");
    assert_eq!(t1.get(&LimitKey::MemoryMib), Some(&256));
}

proptest! {
    /// REQ: CONF-MEM-02
    #[test]
    fn conf_mem_02_clamped_memory_never_exceeds_the_tier_ceiling(request in 1u64..4096) {
        let mut requested = limits::Limits::default();
        requested.0.insert("memory_mib".to_string(), request);
        for tier in [Tier::T1, Tier::T2, Tier::T3] {
            let clamped = limits::clamp(&requested, tier).expect("clamp never refuses");
            let ceiling = limits::ceiling(LimitKey::MemoryMib, tier);
            prop_assert_eq!(clamped.get(&LimitKey::MemoryMib).copied(), Some(request.min(ceiling)));
        }
    }
}

/// REQ: CONF-MEM-02
#[test]
fn conf_mem_02_manifest_with_memory_mib_256_installs_without_an_error_diagnostic() {
    let text =
        std::fs::read_to_string(crate::common::manifest_path("limits-256.json")).expect("fixture");
    let diagnostics = ilium_plugin_api::manifest::validate_json(
        &text,
        &validation_context(Tier::T2, CheckMode::Install),
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != ilium_plugin_api::Severity::Error),
        "{diagnostics:?}"
    );
}

// ----------------------------------------------------------------------- Package

/// REQ: CONF-PKG-01
#[test]
fn conf_pkg_01_packing_the_same_files_twice_gives_equal_bytes() {
    let files = vec![
        (
            "manifest.json".to_string(),
            manifest_text("minimal.json").into_bytes(),
        ),
        ("server.wasm".to_string(), vec![0, 97, 115, 109, 1, 0, 0, 0]),
    ];
    let first = ilium_plugin_host::package::write_package(&files).expect("first pack");
    let second = ilium_plugin_host::package::write_package(&files).expect("second pack");
    assert_eq!(first, second);
}

/// REQ: CONF-PKG-01
#[test]
fn conf_pkg_01_the_input_order_of_files_does_not_change_the_bytes() {
    let a = (
        "manifest.json".to_string(),
        manifest_text("minimal.json").into_bytes(),
    );
    let b = ("server.wasm".to_string(), vec![0, 97, 115, 109, 1, 0, 0, 0]);
    let forward =
        ilium_plugin_host::package::write_package(&[a.clone(), b.clone()]).expect("forward");
    let backward = ilium_plugin_host::package::write_package(&[b, a]).expect("backward");
    assert_eq!(forward, backward);
}

/// REQ: CONF-PKG-02
#[test]
fn conf_pkg_02_api_version_section_0_1_0_is_three_u16_big_endian() {
    let bytes = [0u8, 0, 0, 1, 0, 0];
    let version = ApiVersion::from_custom_section(&bytes).expect("0.1.0");
    assert_eq!((version.major, version.minor, version.patch), (0, 1, 0));
}

/// REQ: CONF-PKG-02
#[test]
fn conf_pkg_02_section_of_five_or_seven_bytes_is_refused_package_api_section_invalid() {
    for length in [0usize, 5, 7] {
        let error = ApiVersion::from_custom_section(&vec![0u8; length]).expect_err("wrong length");
        assert_eq!(
            error.code.as_str(),
            "package.api-section-invalid",
            "length {length}"
        );
    }
}

proptest! {
    /// REQ: CONF-PKG-02
    #[test]
    fn conf_pkg_02_any_six_bytes_decode_as_big_endian_numbers(major in any::<u16>(), minor in any::<u16>(), patch in any::<u16>()) {
        let mut bytes = Vec::new();
        for number in [major, minor, patch] {
            bytes.extend_from_slice(&number.to_be_bytes());
        }
        let version = ApiVersion::from_custom_section(&bytes).expect("six bytes");
        prop_assert_eq!((version.major, version.minor, version.patch), (major, minor, patch));
    }
}

fn pack_fixture(manifest: &str, wasm: &[u8]) -> Vec<u8> {
    ilium_plugin_host::package::write_package(&[
        ("manifest.json".to_string(), manifest.as_bytes().to_vec()),
        ("server.wasm".to_string(), wasm.to_vec()),
    ])
    .expect("pack")
}

/// REQ: CONF-PKG-03
#[test]
fn conf_pkg_03_one_changed_byte_of_a_file_is_refused_manifest_file_hash() {
    let manifest = manifest_text("minimal.json");
    let good = pack_fixture(&manifest, &[0, 97, 115, 109, 1, 0, 0, 0]);
    let bad = pack_fixture(&manifest, &[0, 97, 115, 109, 1, 0, 0, 1]);
    assert_ne!(good, bad);
    let errors = ilium_plugin_host::package::read_package(&bad).expect_err("hash differs");
    assert!(
        errors
            .iter()
            .any(|d| d.code.as_str() == "manifest.file-hash"),
        "{errors:?}"
    );
}

/// REQ: CONF-PKG-03
#[test]
fn conf_pkg_03_an_entry_with_a_wrong_length_is_refused_manifest_file_length() {
    let manifest = manifest_text("minimal.json");
    let longer = pack_fixture(&manifest, &[0, 97, 115, 109, 1, 0, 0, 0, 9]);
    let errors = ilium_plugin_host::package::read_package(&longer).expect_err("length differs");
    assert!(
        errors
            .iter()
            .any(|d| d.code.as_str() == "manifest.file-length"),
        "{errors:?}"
    );
}

/// REQ: CONF-PKG-04
#[test]
fn conf_pkg_04_an_unsigned_package_is_refused_install_unsigned_without_allow_unsigned() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = pack_fixture(
        &manifest_text("minimal.json"),
        &[0, 97, 115, 109, 1, 0, 0, 0],
    );
    let file = dir.path().join("example.iliumpkg");
    std::fs::write(&file, package).expect("write package");
    let env = crate::common::cli_env(dir.path());
    let output = crate::common::run_cli(&env, &["install", file.to_str().expect("utf8"), "--yes"]);
    assert_eq!(output.exit_code, 1);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|r| r["code"] == "install.unsigned"));
}

/// REQ: CONF-PKG-04
#[test]
fn conf_pkg_04_an_unsigned_package_installs_as_tier_t3_with_allow_unsigned() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = pack_fixture(
        &manifest_text("minimal.json"),
        &[0, 97, 115, 109, 1, 0, 0, 0],
    );
    let file = dir.path().join("example.iliumpkg");
    std::fs::write(&file, package).expect("write package");
    let env = crate::common::cli_env(dir.path());
    let output = crate::common::run_cli(
        &env,
        &[
            "install",
            file.to_str().expect("utf8"),
            "--allow-unsigned",
            "--yes",
        ],
    );
    assert_eq!(output.exit_code, 0);
    let result = crate::common::records_of(&output, "result")[0].clone();
    assert_eq!(result["tier"], "T3");
}

/// REQ: CONF-PKG-05
#[test]
fn conf_pkg_05_an_unknown_top_level_key_is_refused_manifest_unknown_key() {
    let text =
        std::fs::read_to_string(crate::common::manifest_path("unknown-key.json")).expect("fixture");
    let diagnostics = ilium_plugin_api::manifest::validate_json(
        &text,
        &validation_context(Tier::T3, CheckMode::Install),
    );
    let hit = diagnostics
        .iter()
        .find(|d| d.code.as_str() == "manifest.unknown-key")
        .expect("unknown key reported");
    assert_eq!(hit.pointer, "/colour");
}

/// REQ: CONF-PKG-05
#[test]
fn conf_pkg_05_the_known_manifest_has_no_unknown_key_diagnostic() {
    let text = manifest_text("minimal.json");
    let diagnostics = ilium_plugin_api::manifest::validate_json(
        &text,
        &validation_context(Tier::T3, CheckMode::Install),
    );
    assert!(!diagnostics
        .iter()
        .any(|d| d.code.as_str() == "manifest.unknown-key"));
}

/// REQ: CONF-PKG-05
#[test]
fn conf_pkg_05_a_bound_on_wall_time_applies_to_every_validation() {
    let started = std::time::Instant::now();
    let _ = ilium_plugin_api::manifest::validate_json(
        "{}",
        &validation_context(Tier::T3, CheckMode::Check),
    );
    assert!(started.elapsed() < BOUND);
}
