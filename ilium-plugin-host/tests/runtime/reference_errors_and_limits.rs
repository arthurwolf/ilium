//! Page reference-errors-and-limits.md: restart rule, failure list, tier clamps, session caps,
//! rates, quota, pending requests, timers (HST-400..412, API-040..046, 060..062, 070..074).

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_api::lifecycle::{RestartDecision, RestartPolicy};
use ilium_plugin_api::limits::{ceiling, clamp, default_of, LimitKey, Limits};
use ilium_plugin_api::{Side, Tier};
use ilium_plugin_host::testing::HostHarness;

use crate::support::*;

fn after(ms: u64) -> RestartDecision {
    RestartDecision::RestartAfterMs(ms)
}

fn requested(key: &str, value: u64) -> Limits {
    let mut limits = Limits::default();
    limits.0.insert(key.to_owned(), value);
    limits
}

// ---------------------------------------------------------------- restart rule (pure policy)

/// REQ: HST-400, API-040
#[test]
fn hst_400_first_failure_restarts_after_1_s_second_after_5_s_third_disables() {
    let mut policy = RestartPolicy::default();
    assert_eq!(policy.on_failure(0), after(1_000));
    assert_eq!(policy.on_failure(120_000), after(5_000));
    assert_eq!(policy.on_failure(240_000), RestartDecision::Disable);
}

/// REQ: HST-400, API-040
#[test]
fn hst_400_no_30_s_step_exists_for_any_failure_count() {
    let mut policy = RestartPolicy::default();
    let decisions: Vec<RestartDecision> = [0, 120_000, 240_000, 360_000, 480_000]
        .into_iter()
        .map(|time| policy.on_failure(time))
        .collect();
    // The first three failures are pinned exactly; later ones inside the window stay disabled.
    assert_eq!(decisions[0], after(1_000));
    assert_eq!(decisions[1], after(5_000));
    assert_eq!(decisions[2], RestartDecision::Disable);
    assert!(!decisions.contains(&after(30_000)), "{decisions:?}");
}

/// REQ: HST-400, API-040
#[test]
fn hst_400_third_failure_at_exactly_10_minutes_after_the_first_does_not_disable() {
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    policy.on_failure(1_000);
    assert_eq!(policy.on_failure(600_000), after(5_000));
}

/// REQ: HST-400, API-040
#[test]
fn hst_400_third_failure_one_ms_before_the_window_end_disables() {
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    policy.on_failure(1_000);
    assert_eq!(policy.on_failure(599_999), RestartDecision::Disable);
}

/// REQ: HST-402, API-040
#[test]
fn hst_402_failure_older_than_10_minutes_no_longer_counts() {
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    assert_eq!(policy.failures_in_window(599_999), 1);
    assert_eq!(policy.failures_in_window(600_000), 0);
    assert_eq!(policy.failures_in_window(600_001), 0);
    assert_eq!(policy.on_failure(700_000), after(1_000));
}

/// REQ: HST-412, API-041
#[test]
fn hst_412_reset_returns_the_count_to_zero_and_the_next_failure_restarts_after_1_s() {
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    policy.on_failure(10);
    policy.reset();
    assert_eq!(policy.failures_in_window(20), 0);
    assert_eq!(policy.on_failure(20), after(1_000));
}

proptest::proptest! {
    // REQ: HST-400, HST-402
    #[test]
    fn hst_400_decision_depends_only_on_failures_inside_the_window(gaps in proptest::collection::vec(0u64..400_000, 1..12)) {
        let mut policy = RestartPolicy::default();
        let mut times: Vec<u64> = Vec::new();
        let mut now = 0u64;
        for gap in gaps {
            now += gap;
            times.push(now);
            let counted = times.iter().filter(|t| now - **t < 600_000).count();
            let expected = match counted { 1 => after(1_000), 2 => after(5_000), _ => RestartDecision::Disable };
            proptest::prop_assert_eq!(policy.on_failure(now), expected);
        }
    }
}

// ---------------------------------------------------------------- restart rule (host)

/// REQ: HST-400, API-040, API-015
#[test]
fn hst_400_failures_at_0_2_and_4_minutes_restart_1_s_5_s_then_disable() {
    let (host, states) = trapped_host_at(&[0, minutes(2), minutes(4)]);
    assert_eq!(
        states,
        vec![
            InstanceState::RestartScheduled,
            InstanceState::RestartScheduled,
            InstanceState::StoppedByFailure
        ]
    );
    assert_eq!(host.restart_delay_ms(), None);
}

/// REQ: API-015
#[test]
fn api_015_new_instance_appears_after_exactly_1_s_and_not_before() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    advance_ms(&mut host, 999);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    advance_ms(&mut host, 1);
    assert_eq!(host.state(), InstanceState::Active);
}

/// REQ: API-015
#[test]
fn api_015_a_plugin_cannot_catch_a_trap_the_instance_ends() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    assert_ne!(host.state(), InstanceState::Active);
    assert_eq!(host.failures().len(), 1);
}

/// REQ: API-015
#[test]
fn api_015_second_restart_waits_5_s() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    advance_ms(&mut host, 1_000);
    fail_once(&mut host);
    advance_ms(&mut host, 4_999);
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    advance_ms(&mut host, 1);
    assert_eq!(host.state(), InstanceState::Active);
}

/// REQ: HST-411
#[test]
fn hst_411_client_and_server_keep_independent_failure_counts() {
    let host = HostHarness::new()
        .both_sides()
        .with_native_guest(Box::new(TrapGuest("trap.panic")))
        .build()
        .expect("host");
    host.fail_side(Side::Server);
    host.fail_side(Side::Server);
    assert_eq!(host.failure_count_of(Side::Server), 2);
    assert_eq!(host.failure_count_of(Side::Client), 0);
    host.fail_side(Side::Client);
    assert_eq!(host.state_of(Side::Client), InstanceState::RestartScheduled);
}

/// REQ: HST-411
#[test]
fn hst_411_disabling_one_side_leaves_the_other_running() {
    let host = HostHarness::new()
        .both_sides()
        .with_native_guest(Box::new(TrapGuest("trap.panic")))
        .build()
        .expect("host");
    for _ in 0..3 {
        host.fail_side(Side::Server);
    }
    assert_eq!(host.state_of(Side::Server), InstanceState::StoppedByFailure);
    assert_eq!(host.state_of(Side::Client), InstanceState::Active);
}

/// REQ: HST-402, API-041
#[test]
fn hst_402_a_successful_run_does_not_clear_a_failure() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    advance_ms(&mut host, 1_000);
    host.set_guest(Box::new(QuietGuest));
    host.deliver(one_event());
    host.pump(8);
    assert_eq!(host.failure_count(), 1);
}

/// REQ: HST-402
#[test]
fn hst_402_failure_expires_after_exactly_10_minutes() {
    let mut host = host_with(TrapGuest("trap.panic"));
    fail_once(&mut host);
    advance_ms(&mut host, minutes(10) - 1);
    assert_eq!(host.failure_count(), 1);
    advance_ms(&mut host, 1);
    assert_eq!(host.failure_count(), 0);
}

/// REQ: HST-412, API-041
#[test]
fn hst_412_manual_enable_after_disable_resets_the_count() {
    let (mut host, states) = trapped_host_at(&[0, 2_000, 4_000]);
    assert_eq!(states[2], InstanceState::StoppedByFailure);
    host.enable();
    assert_eq!(host.failure_count(), 0);
    fail_once(&mut host);
    assert_eq!(host.restart_delay_ms(), Some(1_000));
}

/// REQ: API-041
#[test]
fn api_041_a_disabled_side_stays_disabled_without_a_manual_enable() {
    let (mut host, _) = trapped_host_at(&[0, 2_000, 4_000]);
    advance_ms(&mut host, minutes(30));
    assert_eq!(host.state(), InstanceState::StoppedByFailure);
}

/// REQ: HST-401, API-042
#[test]
fn hst_401_trap_deadline_and_activate_error_count_as_failures() {
    let mut host = host_with(TrapGuest("trap.deadline"));
    fail_once(&mut host);
    assert_eq!(host.failure_count(), 1);
    let activate = HostHarness::new()
        .activate_error("io")
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    activate.pump(8);
    assert_eq!(activate.failure_count(), 1);
}

/// REQ: HST-401, API-042
#[test]
fn hst_401_revoke_disable_upgrade_shutdown_and_failed_job_do_not_count() {
    let host = host_with(QuietGuest);
    for _ in 0..3 {
        host.revoke("tree.read");
        host.pump(8);
    }
    assert_eq!(host.failure_count(), 0);
    host.disable();
    host.enable();
    host.upgrade_in_place();
    host.shutdown();
    host.fail_job();
    assert_eq!(host.failure_count(), 0);
    assert_ne!(host.state(), InstanceState::StoppedByFailure);
}

/// REQ: HST-403
#[test]
fn hst_403_trap_in_deactivate_drops_the_instance_and_does_not_count() {
    let host = host_with(QuietGuest);
    host.set_deactivate_result(Err(trap_with("trap.deadline")));
    host.disable();
    assert_eq!(host.failure_count(), 0);
    assert!(host.restore_bytes().is_none());
}

/// REQ: HST-403
#[test]
fn hst_403_clean_deactivate_keeps_restore_bytes() {
    let host = host_with(QuietGuest);
    host.set_deactivate_result(Ok(vec![1, 2, 3]));
    host.disable();
    assert_eq!(host.restore_bytes(), Some(vec![1, 2, 3]));
}

/// REQ: API-044
#[test]
fn api_044_a_trapping_view_restarts_the_plugin_not_only_the_view() {
    let host = host_with(QuietGuest);
    host.set_render_result(Err(trap_with("trap.invalid-ui-tree")));
    host.render_view("main");
    assert_eq!(host.state(), InstanceState::RestartScheduled);
}

/// REQ: API-045
#[test]
fn api_045_a_scene_frame_trap_restarts_the_client_side_after_1_s() {
    let host = HostHarness::new()
        .both_sides()
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.fail_scene_frame();
    assert_eq!(host.state_of(Side::Client), InstanceState::RestartScheduled);
    assert_eq!(host.state_of(Side::Server), InstanceState::Active);
}

/// REQ: API-046, HST-407
#[test]
fn api_046_session_memory_budget_refuses_the_instance_and_the_side_stays_inactive() {
    let host = HostHarness::new()
        .session_memory_mib(32)
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.activate_second_plugin_with_memory(33);
    assert_eq!(host.second_plugin_state(), InstanceState::Inactive);
    assert_eq!(
        host.second_plugin_reason(),
        Some("host-capacity".to_owned())
    );
    assert_eq!(host.second_plugin_failure_count(), 0);
    assert!(log_text(&host).contains("host-capacity"));
}

/// REQ: HST-407
#[test]
fn hst_407_refused_activation_retries_when_capacity_returns() {
    let host = HostHarness::new()
        .session_memory_mib(32)
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.activate_second_plugin_with_memory(33);
    host.free_session_memory_mib(32);
    host.pump(8);
    assert_eq!(host.second_plugin_state(), InstanceState::Active);
}

/// REQ: HST-407
#[test]
fn hst_407_session_helpers_limit_gives_the_same_refusal() {
    let host = HostHarness::new()
        .session_helpers(1)
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.activate_second_plugin_with_helper();
    host.activate_second_plugin_with_helper();
    assert_eq!(
        host.second_plugin_reason(),
        Some("host-capacity".to_owned())
    );
}

/// REQ: HST-407, API-046
#[test]
fn hst_407_exactly_at_the_budget_is_accepted() {
    let host = HostHarness::new()
        .session_memory_mib(32)
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.activate_second_plugin_with_memory(32);
    assert_eq!(host.second_plugin_state(), InstanceState::Active);
}

// ---------------------------------------------------------------- limits

/// REQ: HST-404, API-043
#[test]
fn hst_404_http_streams_99_as_t2_clamps_to_4_and_does_not_fail() {
    let clamped = clamp(&requested("http_streams", 99), Tier::T2).expect("clamp never fails");
    assert_eq!(clamped[&LimitKey::HttpStreams], 4);
}

/// REQ: HST-404, API-043
#[test]
fn hst_404_a_value_at_the_ceiling_is_kept_and_one_over_is_cut() {
    let ceil = ceiling(LimitKey::HttpStreams, Tier::T2);
    assert_eq!(
        clamp(&requested("http_streams", ceil), Tier::T2).unwrap()[&LimitKey::HttpStreams],
        ceil
    );
    assert_eq!(
        clamp(&requested("http_streams", ceil + 1), Tier::T2).unwrap()[&LimitKey::HttpStreams],
        ceil
    );
    assert_eq!(
        clamp(&requested("http_streams", ceil - 1), Tier::T2).unwrap()[&LimitKey::HttpStreams],
        ceil - 1
    );
}

/// REQ: HST-404, API-043
#[test]
fn hst_404_install_logs_the_clamp_once_naming_the_key_and_the_value() {
    let dir = tempfile::tempdir().unwrap();
    let report = ilium_plugin_host::install::install_with_limits(
        dir.path(),
        Tier::T2,
        &requested("http_streams", 99),
    );
    assert!(report.installed);
    let lines: Vec<_> = report
        .log
        .iter()
        .filter(|l| l.contains("http_streams"))
        .collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains('4'));
}

proptest::proptest! {
    // REQ: HST-404
    #[test]
    fn hst_404_clamp_never_exceeds_the_ceiling(value in 0u64..10_000_000, tier_index in 0usize..3, key_index in 0usize..46) {
        let tier = [Tier::T1, Tier::T2, Tier::T3][tier_index];
        let key = LimitKey::ALL[key_index];
        let clamped = clamp(&requested(key.as_key(), value), tier).expect("clamp never fails");
        proptest::prop_assert!(clamped[&key] <= ceiling(key, tier));
        proptest::prop_assert_eq!(clamped[&key], value.min(ceiling(key, tier)));
    }
}

/// REQ: HST-405
#[test]
fn hst_405_defaults_do_not_depend_on_the_tier() {
    for key in LimitKey::ALL {
        let value = default_of(key);
        for tier in [Tier::T1, Tier::T2, Tier::T3] {
            let clamped = clamp(&Limits::default(), tier).expect("clamp");
            assert_eq!(
                clamped[&key],
                value
                    .min(ceiling(key, tier))
                    .max(value.min(ceiling(key, tier))),
                "{key:?}"
            );
            assert!(
                ceiling(key, tier) >= value,
                "ceiling of {key:?} for {tier:?} is below the default"
            );
        }
    }
}

/// REQ: HST-408
#[test]
fn hst_408_event_queue_default_is_1024_with_ceilings_4096_1024_1024() {
    assert_eq!(default_of(LimitKey::EventQueue), 1_024);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T1), 4_096);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T2), 1_024);
    assert_eq!(ceiling(LimitKey::EventQueue, Tier::T3), 1_024);
}

/// REQ: HST-408
#[test]
fn hst_408_request_of_5000_is_cut_to_the_tier_ceiling() {
    assert_eq!(
        clamp(&requested("event_queue", 5_000), Tier::T1).unwrap()[&LimitKey::EventQueue],
        4_096
    );
    assert_eq!(
        clamp(&requested("event_queue", 5_000), Tier::T2).unwrap()[&LimitKey::EventQueue],
        1_024
    );
    assert_eq!(
        clamp(&requested("event_queue", 4_097), Tier::T1).unwrap()[&LimitKey::EventQueue],
        4_096
    );
}

/// REQ: HST-409
#[test]
fn hst_409_detector_deadline_default_20_ms_ceilings_50_20_20() {
    assert_eq!(default_of(LimitKey::DetectorDeadlineMs), 20);
    assert_eq!(ceiling(LimitKey::DetectorDeadlineMs, Tier::T1), 50);
    assert_eq!(ceiling(LimitKey::DetectorDeadlineMs, Tier::T2), 20);
    assert_eq!(ceiling(LimitKey::DetectorDeadlineMs, Tier::T3), 20);
}

/// REQ: HST-409
#[test]
fn hst_409_requests_around_the_t1_ceiling() {
    for (asked, expected) in [(49, 49), (50, 50), (51, 50)] {
        assert_eq!(
            clamp(&requested("detector_deadline_ms", asked), Tier::T1).unwrap()
                [&LimitKey::DetectorDeadlineMs],
            expected
        );
    }
}

/// REQ: HST-406
#[test]
fn hst_406_user_session_value_lower_than_the_default_is_applied() {
    let settings =
        ilium_plugin_host::session::SessionLimits::from_user(&[("session_memory_mib", 512)]);
    assert_eq!(settings.session_memory_mib, 512);
}

/// REQ: HST-406
#[test]
fn hst_406_user_session_value_equal_or_higher_than_the_default_is_ignored() {
    let defaults = ilium_plugin_host::session::SessionLimits::default();
    assert_eq!(defaults.session_memory_mib, 1_024);
    let same =
        ilium_plugin_host::session::SessionLimits::from_user(&[("session_memory_mib", 1_024)]);
    let higher =
        ilium_plugin_host::session::SessionLimits::from_user(&[("session_memory_mib", 4_096)]);
    assert_eq!(same.session_memory_mib, 1_024);
    assert_eq!(higher.session_memory_mib, 1_024);
}

/// REQ: HST-406
#[test]
fn hst_406_all_five_session_keys_have_the_documented_defaults() {
    let d = ilium_plugin_host::session::SessionLimits::default();
    assert_eq!(
        (
            d.session_memory_mib,
            d.session_helpers,
            d.session_processes,
            d.session_http_streams,
            d.session_decorations
        ),
        (1_024, 16, 16, 64, 65_536)
    );
}

// ---------------------------------------------------------------- capability enforcement

/// REQ: HST-410, API-010, API-016
#[test]
fn hst_410_unrequested_capability_traps_and_the_log_names_interface_and_function() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let reply = host.call_import_json("ilium:host/fs", "write", "{}");
    assert_eq!(
        reply.trap_code(),
        Some("host.capability-not-requested".to_owned())
    );
    let text = log_text(&host);
    assert!(
        text.contains("test.plugin") && text.contains("ilium:host/fs") && text.contains("write")
    );
}

/// REQ: HST-410, API-016
#[test]
fn hst_410_declined_grant_returns_not_permitted_and_logs_one_line_with_three_fields() {
    let host = HostHarness::new()
        .grants(&[])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    host.decline("tree.write");
    let reply = host.call_import_json("ilium:host/tree", "set-name", "{}");
    assert_eq!(reply.error_code(), Some("not-permitted"));
    assert_eq!(log_count(&host, "not-permitted"), 1);
    assert!(log_text(&host).contains("ilium:host/tree"));
}

/// REQ: API-010
#[test]
fn api_010_import_whose_capability_was_never_requested_traps_and_restarts() {
    let host = HostHarness::new()
        .grants(&[])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let reply = host.call_import_json("ilium:host/fs", "read", "{}");
    assert_eq!(
        reply.trap_code().as_deref(),
        Some("trap.import-not-requested")
    );
    assert_eq!(host.state(), InstanceState::RestartScheduled);
    assert!(log_text(&host).contains("ilium:host/fs"));
}

// ---------------------------------------------------------------- traps (API-011..014, 081)

fn trap_code_of_fixture(name: &str) -> String {
    let mut host = host_with_fixture(name, Tier::T3);
    fail_once(&mut host);
    let traps = host.failures();
    assert_eq!(traps.len(), 1, "fixture {name} must trap once");
    code_of(&traps[0])
}

/// REQ: API-011, API-081
#[test]
fn api_011_tree_with_4097_nodes_traps_invalid_ui_tree() {
    assert_eq!(
        trap_code_of_fixture("tree-4097-nodes"),
        "trap.invalid-ui-tree"
    );
}

/// REQ: API-011
#[test]
fn api_011_tree_with_4096_nodes_is_accepted() {
    let host = host_with_fixture("tree-4096-nodes", Tier::T3);
    host.render_view("main");
    assert!(host.failures().is_empty());
}

/// REQ: API-011, API-081
#[test]
fn api_011_duplicate_key_and_bad_index_trap() {
    assert_eq!(
        trap_code_of_fixture("tree-duplicate-key"),
        "trap.invalid-ui-tree"
    );
    assert_eq!(
        trap_code_of_fixture("tree-bad-index"),
        "trap.invalid-ui-tree"
    );
    assert_eq!(
        trap_code_of_fixture("tree-layout-count-mismatch"),
        "trap.invalid-ui-tree"
    );
}

/// REQ: API-012, API-081
#[test]
fn api_012_deadline_in_on_events_traps_and_the_log_says_100_ms_limit_20_ms() {
    let mut host = host_with_fixture("sleep-100ms", Tier::T3);
    fail_once(&mut host);
    assert_eq!(code_of(&host.failures()[0]), "trap.deadline");
    assert!(log_text(&host).contains("on-events 100 ms (limit 20 ms)"));
}

/// REQ: API-013, API-081
#[test]
fn api_013_memory_and_table_growth_trap_with_their_own_codes() {
    assert_eq!(trap_code_of_fixture("memory-grow"), "trap.memory");
    assert_eq!(trap_code_of_fixture("table-grow"), "trap.table");
}

/// REQ: API-014, API-081
#[test]
fn api_014_panic_in_on_events_traps_with_trap_panic() {
    assert_eq!(trap_code_of_fixture("panic-on-events"), "trap.panic");
}

/// REQ: API-081
#[test]
fn api_081_batch_over_256_submissions_traps_submission_batch_and_256_does_not() {
    let mut ok = host_with(BatchGuest(256));
    fail_once(&mut ok);
    assert!(ok.failures().is_empty());
    let mut over = host_with(BatchGuest(257));
    fail_once(&mut over);
    assert_eq!(code_of(&over.failures()[0]), "trap.submission-batch");
}

/// REQ: API-081
#[test]
fn api_081_the_eight_codes_are_exactly_the_documented_set_and_each_is_logged() {
    let codes = [
        "trap.deadline",
        "trap.memory",
        "trap.table",
        "trap.submission-batch",
        "trap.invalid-ui-tree",
        "trap.import-not-requested",
        "trap.abi",
        "trap.panic",
    ];
    for code in codes {
        let mut host = host_with(TrapGuest(code));
        fail_once(&mut host);
        assert_eq!(code_of(&host.failures()[0]), code);
        assert!(log_text(&host).contains(code), "log must name {code}");
    }
}

/// REQ: API-081
#[test]
fn api_081_a_trap_with_an_unknown_code_is_recorded_as_trap_abi_never_invented() {
    let mut host = host_with(TrapGuest("trap.made-up"));
    fail_once(&mut host);
    assert_eq!(code_of(&host.failures()[0]), "trap.abi");
}

// ---------------------------------------------------------------- errors, truncation, caps

/// REQ: API-060
#[test]
fn api_060_65_kib_raw_input_is_refused_invalid_argument_and_sends_no_bytes() {
    let host = host_with(QuietGuest);
    let reply = host.call_import_json(
        "ilium:host/panes",
        "send-raw",
        &format!("{{\"pane\":1,\"bytes\":{}}}", 65 * 1024),
    );
    assert_eq!(reply.error_code(), Some("invalid-argument"));
    assert_eq!(host.kernel().raw_bytes_sent(), 0);
}

/// REQ: API-060
#[test]
fn api_060_64_kib_raw_input_is_accepted() {
    let host = host_with(QuietGuest);
    let reply = host.call_import_json(
        "ilium:host/panes",
        "send-raw",
        &format!("{{\"pane\":1,\"bytes\":{}}}", 64 * 1024),
    );
    assert_ne!(reply.error_code(), Some("invalid-argument"));
}

/// REQ: API-061
#[test]
fn api_061_state_dependent_cap_returns_quota_exceeded_and_precheckable_cap_invalid_argument() {
    let host = host_with(QuietGuest);
    let rows = host.call_import_json(
        "ilium:host/panes",
        "read-cells",
        "{\"pane\":1,\"rows\":1001}",
    );
    assert_eq!(rows.error_code(), Some("quota-exceeded"));
    let stages = host.call_import_json(
        "ilium:host/panes",
        "send-input",
        "{\"pane\":1,\"stages\":17}",
    );
    assert_eq!(stages.error_code(), Some("invalid-argument"));
}

/// REQ: API-061
#[test]
fn api_061_the_boundary_values_1000_rows_and_16_stages_pass_the_cap_check() {
    let host = host_with(QuietGuest);
    let rows = host.call_import_json(
        "ilium:host/panes",
        "read-cells",
        "{\"pane\":1,\"rows\":1000}",
    );
    assert_ne!(rows.error_code(), Some("quota-exceeded"));
    let stages = host.call_import_json(
        "ilium:host/panes",
        "send-input",
        "{\"pane\":1,\"stages\":16}",
    );
    assert_ne!(stages.error_code(), Some("invalid-argument"));
}

/// REQ: API-062
#[test]
fn api_062_timer_of_10_ms_is_invalid_argument_and_50_ms_is_accepted() {
    let host = host_with(QuietGuest);
    assert_eq!(
        host.call_import_json(
            "ilium:host/timers",
            "set",
            "{\"delay-ms\":10,\"repeat\":true}"
        )
        .error_code(),
        Some("invalid-argument")
    );
    assert_eq!(
        host.call_import_json(
            "ilium:host/timers",
            "set",
            "{\"delay-ms\":49,\"repeat\":true}"
        )
        .error_code(),
        Some("invalid-argument")
    );
    assert_eq!(
        host.call_import_json(
            "ilium:host/timers",
            "set",
            "{\"delay-ms\":50,\"repeat\":true}"
        )
        .error_code(),
        None
    );
}

/// REQ: API-062
#[test]
fn api_062_sync_fs_read_over_1_mib_is_invalid_argument_and_1_mib_is_accepted() {
    let host = HostHarness::new()
        .grants(&["fs.read:project"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    let over = host.call_import_json(
        "ilium:host/fs",
        "read",
        &format!(
            "{{\"path\":\"a\",\"offset\":0,\"max-bytes\":{}}}",
            1_048_577
        ),
    );
    let at = host.call_import_json(
        "ilium:host/fs",
        "read",
        &format!(
            "{{\"path\":\"a\",\"offset\":0,\"max-bytes\":{}}}",
            1_048_576
        ),
    );
    assert_eq!(over.error_code(), Some("invalid-argument"));
    assert_ne!(at.error_code(), Some("invalid-argument"));
}

// ---------------------------------------------------------------- error record (HST-400 family: API-001..005 live in the other file)

/// REQ: API-070
#[test]
fn api_070_two_plugins_open_32_streams_each_and_a_65th_is_quota_exceeded() {
    let host = HostHarness::new()
        .two_plugins()
        .grants(&["http:example.com"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    for _ in 0..32 {
        assert!(host.open_stream_as(0).error_code().is_none());
        assert!(host.open_stream_as(1).error_code().is_none());
    }
    assert_eq!(host.open_stream_as(0).error_code(), Some("quota-exceeded"));
}

/// REQ: API-070
#[test]
fn api_070_session_caps_are_the_documented_numbers() {
    let d = ilium_plugin_host::session::SessionLimits::default();
    assert_eq!(
        (
            d.session_http_streams,
            d.session_processes,
            d.session_decorations
        ),
        (64, 16, 65_536)
    );
    assert_eq!(
        (
            d.session_pending_requests,
            d.session_key_grabs,
            d.session_children
        ),
        (4_096, 32, 16)
    );
}

// ---------------------------------------------------------------- rates (API-071, API-003)

fn sound_host() -> ilium_plugin_host::testing::HarnessHandle {
    host_with(BatchGuest(1))
}

/// REQ: API-071
#[test]
fn api_071_play_sound_allows_4_per_second_and_refuses_the_fifth() {
    let host = sound_host();
    for _ in 0..4 {
        assert!(host
            .call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
            .error_code()
            .is_none());
    }
    let fifth = host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    assert_eq!(fifth.error_code(), Some("rate-limited"));
    assert!(fifth.retry_after_ms().unwrap() > 0);
}

/// REQ: API-071
#[test]
fn api_071_window_slides_the_fifth_call_passes_exactly_1000_ms_after_the_first() {
    let mut host = sound_host();
    for _ in 0..4 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    advance_ms(&mut host, 999);
    assert_eq!(
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
            .error_code(),
        Some("rate-limited")
    );
    advance_ms(&mut host, 1);
    assert!(host
        .call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
        .error_code()
        .is_none());
}

/// REQ: API-071
#[test]
fn api_071_retry_after_equals_the_time_until_the_oldest_counted_call_leaves() {
    let mut host = sound_host();
    for _ in 0..4 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    advance_ms(&mut host, 300);
    let refused = host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    assert_eq!(refused.retry_after_ms(), Some(700));
}

/// REQ: API-071
#[test]
fn api_071_refused_calls_do_not_enter_the_window() {
    let mut host = sound_host();
    for _ in 0..4 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    for _ in 0..10 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    advance_ms(&mut host, 1_000);
    for _ in 0..4 {
        assert!(host
            .call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
            .error_code()
            .is_none());
    }
}

/// REQ: API-071
#[test]
fn api_071_window_survives_a_restart_of_the_plugin_side() {
    let mut host = sound_host();
    for _ in 0..4 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    host.set_guest(Box::new(TrapGuest("trap.panic")));
    fail_once(&mut host);
    advance_ms(&mut host, 100);
    assert_eq!(
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
            .error_code(),
        Some("rate-limited")
    );
}

/// REQ: API-071
#[test]
fn api_071_topics_publish_allows_100_per_second_and_refuses_the_101st() {
    let host = HostHarness::new()
        .grants(&["topics.publish:demo"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    for index in 0..100 {
        assert!(
            host.call_import_json(
                "ilium:host/topics",
                "publish",
                "{\"topic\":\"demo\",\"payload\":\"x\"}"
            )
            .error_code()
            .is_none(),
            "call {index}"
        );
    }
    assert_eq!(
        host.call_import_json(
            "ilium:host/topics",
            "publish",
            "{\"topic\":\"demo\",\"payload\":\"x\"}"
        )
        .error_code(),
        Some("rate-limited")
    );
}

/// REQ: API-071
#[test]
fn api_071_journal_log_event_allows_20_per_second_and_refuses_the_21st() {
    let host = host_with(QuietGuest);
    for _ in 0..20 {
        assert!(host
            .call_import_json(
                "ilium:host/journal",
                "log-event",
                "{\"kind\":\"k\",\"detail\":\"d\"}"
            )
            .error_code()
            .is_none());
    }
    assert_eq!(
        host.call_import_json(
            "ilium:host/journal",
            "log-event",
            "{\"kind\":\"k\",\"detail\":\"d\"}"
        )
        .error_code(),
        Some("rate-limited")
    );
}

/// REQ: API-071
#[test]
fn api_071_geocode_allows_one_per_second() {
    let mut host = HostHarness::new()
        .grants(&["geocode"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    assert!(host
        .call_import_json("ilium:host/geocode", "search", "{\"query\":\"Oslo\"}")
        .error_code()
        .is_none());
    assert_eq!(
        host.call_import_json("ilium:host/geocode", "search", "{\"query\":\"Oslo\"}")
            .error_code(),
        Some("rate-limited")
    );
    advance_ms(&mut host, 1_000);
    assert!(host
        .call_import_json("ilium:host/geocode", "search", "{\"query\":\"Oslo\"}")
        .error_code()
        .is_none());
}

/// REQ: API-071
#[test]
fn api_071_rates_are_per_plugin_not_shared_between_plugins() {
    let host = HostHarness::new()
        .two_plugins()
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    for _ in 0..4 {
        host.call_import_as(0, "ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    }
    assert!(host
        .call_import_as(1, "ilium:host/sounds", "play", "{\"name\":\"bell\"}")
        .error_code()
        .is_none());
}

// ---------------------------------------------------------------- quota (API-072, API-003)

/// REQ: API-072
#[test]
fn api_072_bucket_holds_2000_ms_and_refills_500_ms_per_second() {
    let host = host_with(QuietGuest);
    assert_eq!(host.quota_balance_ms(), 2_000);
    let mut host = host;
    host.charge_cpu_ms(2_000);
    advance_ms(&mut host, 1_000);
    assert_eq!(host.quota_balance_ms(), 500);
}

/// REQ: API-072
#[test]
fn api_072_balance_never_refills_above_2000_ms() {
    let mut host = host_with(QuietGuest);
    advance_ms(&mut host, minutes(5));
    assert_eq!(host.quota_balance_ms(), 2_000);
}

/// REQ: API-072, API-003
#[test]
fn api_072_empty_bucket_refuses_with_quota_exceeded_and_the_documented_retry_after() {
    let host = host_with(QuietGuest);
    host.charge_cpu_ms(2_000);
    assert_eq!(
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
            .error_code(),
        Some("quota-exceeded")
    );
    let reply = host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
    assert_eq!(reply.retry_after_ms(), Some(100));
}

/// REQ: API-072
#[test]
fn api_072_negative_balance_retry_after_is_max_100_ceil10_of_double_the_deficit() {
    for (charge, expected) in [
        (2_000 + 1, 100),
        (2_000 + 50, 100),
        (2_000 + 51, 110),
        (2_000 + 500, 1_000),
    ] {
        let host = host_with(QuietGuest);
        host.charge_cpu_ms(charge);
        let reply = host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}");
        assert_eq!(reply.retry_after_ms(), Some(expected), "charge {charge}");
    }
}

/// REQ: API-072
#[test]
fn api_072_a_balance_of_exactly_one_ms_is_not_empty() {
    let host = host_with(QuietGuest);
    host.charge_cpu_ms(1_999);
    assert!(host
        .call_import_json("ilium:host/sounds", "play", "{\"name\":\"bell\"}")
        .error_code()
        .is_none());
}

// ---------------------------------------------------------------- pending requests and timers

/// REQ: API-073
#[test]
fn api_073_the_65th_pending_request_returns_busy() {
    let host = HostHarness::new()
        .grants(&["http:example.com"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    for index in 0..64 {
        assert!(
            host.start_slow_request().error_code().is_none(),
            "request {index}"
        );
    }
    assert_eq!(host.start_slow_request().error_code(), Some("busy"));
}

/// REQ: API-073
#[test]
fn api_073_a_completed_request_frees_its_slot_when_the_completion_is_enqueued() {
    let host = HostHarness::new()
        .grants(&["http:example.com"])
        .with_native_guest(Box::new(QuietGuest))
        .build()
        .expect("host");
    for _ in 0..64 {
        host.start_slow_request();
    }
    host.complete_oldest_request();
    assert!(host.start_slow_request().error_code().is_none());
}

/// REQ: API-073
#[test]
fn api_073_refused_submissions_and_sync_calls_do_not_count() {
    let host = host_with(QuietGuest);
    for _ in 0..70 {
        host.call_import_json("ilium:host/sounds", "play", "{\"name\":\"\"}");
    }
    assert_eq!(host.pending_requests(), 0);
}

/// REQ: API-073
#[test]
fn api_073_default_is_64_and_limit_key_is_pending_requests() {
    assert_eq!(default_of(LimitKey::PendingRequests), 64);
}

/// REQ: API-074
#[test]
fn api_074_timer_never_fires_inside_the_call_that_set_it() {
    let host = host_with(QuietGuest);
    host.call_import_json(
        "ilium:host/timers",
        "set",
        "{\"delay-ms\":50,\"repeat\":false}",
    );
    assert!(host.queued_events().is_empty());
}

/// REQ: API-074
#[test]
fn api_074_first_tick_is_queued_after_the_delay_counted_from_the_return_of_set() {
    let mut host = host_with(QuietGuest);
    host.call_import_json(
        "ilium:host/timers",
        "set",
        "{\"delay-ms\":50,\"repeat\":false}",
    );
    advance_ms(&mut host, 49);
    assert!(host.queued_timer_ticks().is_empty());
    advance_ms(&mut host, 1);
    assert_eq!(host.queued_timer_ticks().len(), 1);
}

/// REQ: API-074
#[test]
fn api_074_cancel_removes_a_queued_undelivered_tick() {
    let mut host = host_with(QuietGuest);
    let id = host
        .call_import_json(
            "ilium:host/timers",
            "set",
            "{\"delay-ms\":50,\"repeat\":false}",
        )
        .timer_id();
    advance_ms(&mut host, 40);
    host.queue_timer_tick(id);
    host.call_import_json("ilium:host/timers", "cancel", &format!("{{\"id\":{id}}}"));
    assert!(host.queued_timer_ticks().is_empty());
    advance_ms(&mut host, 100);
    assert!(host.queued_timer_ticks().is_empty());
}

/// REQ: API-074
#[test]
fn api_074_cancelling_an_unknown_id_is_a_silent_no_op() {
    let host = host_with(QuietGuest);
    let reply = host.call_import_json("ilium:host/timers", "cancel", "{\"id\":987654}");
    assert!(reply.error_code().is_none());
    assert!(reply.trap_code().is_none());
    assert!(host.failures().is_empty());
}

/// REQ: API-002
#[test]
fn api_002_error_code_enum_has_exactly_the_15_documented_cases() {
    let names = ErrorCode::CASE_NAMES;
    assert_eq!(names.len(), 15);
    assert_eq!(names[0], "not-permitted");
    assert_eq!(names[14], "internal");
}
