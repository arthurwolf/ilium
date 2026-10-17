//! reference-host-architecture.md rows that need a running server host
//! (HST-007..014, 016, 017, 019, 020, 024..026, 029..032, 035, 039..042).

use std::time::Duration;

use ilium_plugin_api::{ErrorCode, ProcessKind, Side, Tier};
use ilium_plugin_host::testing::boot::{BootHarness, PluginSpec};
use proptest::prelude::*;

use crate::common::*;

fn two_sided(id: &str) -> PluginSpec {
    PluginSpec::new(id, Tier::T1)
        .core()
        .sides(&[Side::Server, Side::Client])
        .activation(&["startup"])
        .client_activation(&["startup"])
}

// HST-007
/// REQ: HST-007
#[test]
fn hst_007_server_side_is_hosted_by_the_session_server_only() {
    let run = started(vec![two_sided("ilium.core.tree")]);
    assert_eq!(
        run.host_process("ilium.core.tree", Side::Server),
        ProcessKind::Server
    );
}

/// REQ: HST-007
#[test]
fn hst_007_client_and_scene_sides_are_hosted_by_an_attached_client() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let client = run.attach_client().expect("client attaches");
    assert_eq!(
        run.host_process_of_client("ilium.core.tree", Side::Client, client),
        ProcessKind::Client
    );
}

/// REQ: HST-007
#[test]
fn hst_007_a_cli_run_hosts_the_server_component_in_the_cli_process() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"]),
        )
        .build();
    let cli = run.run_cli("ilium.core.recap", "recap");
    assert_eq!(cli.host_process, ProcessKind::Cli);
}

/// REQ: HST-007
#[test]
fn hst_007_a_side_is_never_hosted_twice_at_the_same_time() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    run.attach_client().expect("client");
    assert_eq!(run.hosts_of("ilium.core.tree", Side::Server), 1);
}

// HST-008
/// REQ: HST-008
#[test]
fn hst_008_a_t1_component_is_never_placed_in_a_helper() {
    let run = started(vec![startup("ilium.core.titles")]);
    assert_eq!(run.helper_count(), 0);
    assert_eq!(
        run.host_process("ilium.core.titles", Side::Server),
        ProcessKind::Server
    );
}

/// REQ: HST-008
#[test]
fn hst_008_a_t1_job_runs_in_the_same_process() {
    let mut run = started(vec![
        startup("ilium.core.search").limit("concurrent_jobs", 2)
    ]);
    run.start_jobs("ilium.core.search", 1);
    assert_eq!(run.helper_count(), 0);
}

// HST-009
/// REQ: HST-009
#[test]
fn hst_009_t2_and_t3_run_only_in_a_helper() {
    let run = started(vec![
        PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"]),
        PluginSpec::new("acme.t3", Tier::T3).activation(&["startup"]),
    ]);
    for plugin in ["acme.t2", "acme.t3"] {
        assert_eq!(
            run.host_process(plugin, Side::Server),
            ProcessKind::Helper,
            "{plugin}"
        );
    }
}

/// REQ: HST-009
#[test]
fn hst_009_the_parent_engine_never_instantiates_a_t2_or_t3_component() {
    let run = started(vec![
        PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"])
    ]);
    assert_eq!(run.parent_instantiations("acme.t2"), 0);
    assert_eq!(run.helper_instantiations("acme.t2"), 1);
}

/// REQ: HST-009
#[test]
fn hst_009_safe_plugin_mode_places_no_third_party_component_anywhere() {
    let run = started_in(
        ilium_plugin_host::lifecycle::StartMode::SafePlugins,
        vec![PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"])],
    );
    assert_eq!(
        run.parent_instantiations("acme.t2") + run.helper_instantiations("acme.t2"),
        0
    );
}

// HST-010
/// REQ: HST-010
#[test]
fn hst_010_a_client_exit_leaves_the_server_and_other_clients_running() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let first = run.attach_client().expect("first");
    let second = run.attach_client().expect("second");
    run.client_exit(first);
    assert!(run.server_alive());
    assert_eq!(
        run.client_side_state(second, "ilium.core.tree").state,
        "active"
    );
}

/// REQ: HST-010
#[test]
fn hst_010_a_helper_exit_leaves_the_server_running() {
    let mut run = started(vec![
        PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"]),
        startup("ilium.core.titles"),
    ]);
    run.kill_helper();
    assert!(run.server_alive());
    assert_eq!(
        run.side_state("ilium.core.titles", Side::Server).state,
        "active"
    );
}

/// REQ: HST-010
#[test]
fn hst_010_a_cli_process_exit_leaves_the_server_running() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.run_cli_exit();
    assert!(run.server_alive());
}

// HST-011
/// REQ: HST-011
#[test]
fn hst_011_a_session_has_exactly_one_server_host() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.attach_client().expect("client");
    assert_eq!(run.server_host_count(), 1);
}

/// REQ: HST-011
#[test]
fn hst_011_the_dispatcher_is_the_only_plugin_subscriber_of_the_journal() {
    let run = started(vec![
        startup("ilium.core.titles"),
        startup("ilium.core.cost"),
    ]);
    assert_eq!(run.journal_plugin_subscribers(), 1);
}

// HST-012
/// REQ: HST-012
#[test]
fn hst_012_failure_counts_survive_a_client_detach_and_attach() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    let client = run.attach_client().expect("client");
    run.trap_in("ilium.core.titles", Side::Server);
    let before = run
        .side_state("ilium.core.titles", Side::Server)
        .failures_in_window;
    run.client_exit(client);
    run.attach_client().expect("reattach");
    assert_eq!(
        run.side_state("ilium.core.titles", Side::Server)
            .failures_in_window,
        before
    );
    assert!(before >= 1);
}

/// REQ: HST-012
#[test]
fn hst_012_queued_events_and_quota_buckets_survive_a_detach() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    let client = run.attach_client().expect("client");
    run.deliver_tree_traffic("ilium.core.titles", 3);
    let queue = run.queue_depth("ilium.core.titles", Side::Server);
    let bucket = run.quota_bucket("ilium.core.titles", Side::Server);
    run.client_exit(client);
    assert_eq!(run.queue_depth("ilium.core.titles", Side::Server), queue);
    assert_eq!(run.quota_bucket("ilium.core.titles", Side::Server), bucket);
}

/// REQ: HST-012
#[test]
fn hst_012_a_restart_timer_keeps_running_across_a_detach() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    let client = run.attach_client().expect("client");
    run.trap_in("ilium.core.titles", Side::Server);
    run.client_exit(client);
    run.advance(Duration::from_secs(5));
    assert_eq!(
        run.side_state("ilium.core.titles", Side::Server).state,
        "active"
    );
}

// HST-013
/// REQ: HST-013
#[test]
fn hst_013_a_failure_in_one_client_host_does_not_touch_another() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let first = run.attach_client().expect("first");
    let second = run.attach_client().expect("second");
    run.trap_in_client(first, "ilium.core.tree");
    assert_eq!(
        run.client_side_state(second, "ilium.core.tree")
            .failures_in_window,
        0
    );
    assert_eq!(
        run.client_side_state(second, "ilium.core.tree").state,
        "active"
    );
    assert!(
        run.client_side_state(first, "ilium.core.tree")
            .failures_in_window
            >= 1
    );
}

/// REQ: HST-013
#[test]
fn hst_013_a_client_failure_does_not_change_the_server_side_count() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let client = run.attach_client().expect("client");
    run.trap_in_client(client, "ilium.core.tree");
    assert_eq!(
        run.side_state("ilium.core.tree", Side::Server)
            .failures_in_window,
        0
    );
}

// HST-014
/// REQ: HST-014
#[test]
fn hst_014_disabling_in_one_client_leaves_other_clients_enabled() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let first = run.attach_client().expect("first");
    let second = run.attach_client().expect("second");
    run.fail_until_disabled_in_client(first, "ilium.core.tree");
    assert_eq!(
        run.client_side_state(first, "ilium.core.tree").state,
        "stopped-by-failure"
    );
    assert_eq!(
        run.client_side_state(second, "ilium.core.tree").state,
        "active"
    );
}

/// REQ: HST-014
#[test]
fn hst_014_the_notice_is_shown_only_to_the_client_that_disabled_it() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let first = run.attach_client().expect("first");
    let second = run.attach_client().expect("second");
    run.fail_until_disabled_in_client(first, "ilium.core.tree");
    assert_eq!(run.notices_for_client(first).len(), 1);
    assert!(run.notices_for_client(second).is_empty());
}

// HST-016
/// REQ: HST-016
#[test]
fn hst_016_a_late_scene_frame_does_not_delay_the_client_frame() {
    let mut run = started(vec![PluginSpec::new("ilium.core.scenes", Tier::T1)
        .core()
        .sides(&[Side::Scene])]);
    let client = run.attach_client().expect("client");
    run.set_scene_frame_delay(Duration::from_millis(500));
    let frame = run.compose_client_frame(client);
    assert!(
        frame.elapsed < Duration::from_millis(100),
        "{:?}",
        frame.elapsed
    );
}

/// REQ: HST-016
#[test]
fn hst_016_the_compositor_reuses_the_previous_complete_frame() {
    let mut run = started(vec![PluginSpec::new("ilium.core.scenes", Tier::T1)
        .core()
        .sides(&[Side::Scene])]);
    let client = run.attach_client().expect("client");
    let first = run.compose_client_frame(client);
    run.set_scene_frame_delay(Duration::from_millis(500));
    let second = run.compose_client_frame(client);
    assert_eq!(second.scene_frame_id, first.scene_frame_id);
    assert!(second.reused_previous_scene_frame);
}

// HST-017
/// REQ: HST-017
#[test]
fn hst_017_a_job_has_its_own_store_and_memory() {
    let mut run = started(vec![
        startup("ilium.core.search").limit("concurrent_jobs", 2)
    ]);
    run.start_jobs("ilium.core.search", 1);
    let main = run.store_ids("ilium.core.search", Side::Server);
    let jobs = run.job_store_ids("ilium.core.search");
    assert_eq!(jobs.len(), 1);
    assert!(!main.contains(&jobs[0]));
    assert_ne!(
        run.job_memory_base("ilium.core.search", 0),
        run.main_memory_base("ilium.core.search")
    );
}

/// REQ: HST-017
#[test]
fn hst_017_a_job_runs_on_the_job_worker_bank() {
    let mut run = started(vec![
        startup("ilium.core.search").limit("concurrent_jobs", 2)
    ]);
    run.start_jobs("ilium.core.search", 1);
    assert_eq!(run.job_worker_bank("ilium.core.search", 0), "job");
}

// HST-019
/// REQ: HST-019
#[test]
fn hst_019_a_cli_host_activates_only_the_entry_plugin_and_its_required_services() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"])
                .requires(&["ilium.core.inference"]),
        )
        .plugin(startup("ilium.core.inference"))
        .plugin(startup("ilium.core.titles"))
        .build();
    let cli = run.run_cli("ilium.core.recap", "recap");
    let mut active = cli.activated.clone();
    active.sort();
    assert_eq!(
        active,
        vec![
            "ilium.core.inference".to_owned(),
            "ilium.core.recap".to_owned()
        ]
    );
}

/// REQ: HST-019
#[test]
fn hst_019_a_cli_host_activates_no_other_core_plugin_even_with_startup() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"]),
        )
        .plugin(startup("ilium.core.titles"))
        .build();
    let cli = run.run_cli("ilium.core.recap", "recap");
    assert!(!cli.activated.contains(&"ilium.core.titles".to_owned()));
}

// HST-020
/// REQ: HST-020
#[test]
fn hst_020_a_session_import_in_a_cli_host_returns_no_session() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"]),
        )
        .build();
    let error = run
        .cli_call_tree_import("ilium.core.recap")
        .expect_err("no session");
    // The manual's `unavailable` maps to the base code not-permitted (errors page).
    assert_eq!(error.code, ErrorCode::NotPermitted);
    assert_eq!(error.message, "no session");
}

/// REQ: HST-020
#[test]
fn hst_020_the_cli_host_starts_no_server_and_takes_no_session_lock() {
    let mut run = BootHarness::new(ProcessKind::Cli)
        .plugin(
            PluginSpec::new("ilium.core.recap", Tier::T1)
                .core()
                .activation(&["cli:recap"]),
        )
        .build();
    let _ = run.cli_call_tree_import("ilium.core.recap");
    assert_eq!(run.servers_started(), 0);
    assert!(!run.session_lock_taken());
    assert!(!run.snapshot_written());
}

// HST-024
/// REQ: HST-024
#[test]
fn hst_024_the_server_host_owns_the_retained_contribution_registry() {
    let mut run = started(vec![
        startup("ilium.core.notifications").contributes("ilium.core.titles/before-title")
    ]);
    assert_eq!(run.contribution_registry_owner(), ProcessKind::Server);
    assert_eq!(run.contributions().len(), 1);
}

/// REQ: HST-024
#[test]
fn hst_024_a_client_replica_is_built_from_contribution_delta_records() {
    let mut run = started(vec![
        startup("ilium.core.notifications").contributes("ilium.core.titles/before-title")
    ]);
    let client = run.attach_client().expect("client");
    assert_eq!(run.replica_source(client), "ContributionDelta");
    assert_eq!(run.replica_contributions(client).len(), 1);
}

// HST-025
/// REQ: HST-025
#[test]
fn hst_025_a_client_owned_point_is_not_visible_to_server_plugins() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    run.attach_client().expect("client");
    run.client_registers_point("ilium.core.tree/row-decorator");
    assert!(!run.server_sees_point("ilium.core.tree/row-decorator"));
}

/// REQ: HST-025
#[test]
fn hst_025_a_client_owned_service_is_not_visible_to_server_plugins() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    run.attach_client().expect("client");
    run.client_registers_service("ilium.core.tree/focus");
    assert!(!run.server_sees_service("ilium.core.tree/focus"));
}

/// REQ: HST-025
#[test]
fn hst_025_a_server_owned_point_is_visible_in_the_server_process() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.server_registers_point("ilium.core.titles/before-title");
    assert!(run.server_sees_point("ilium.core.titles/before-title"));
}

// HST-026
/// REQ: HST-026
#[test]
fn hst_026_a_restored_tree_of_30_nodes_gives_one_tree_changed_batch() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(30)
        .plugin(startup("ilium.core.titles").subscribes("tree-changed"))
        .build();
    run.start();
    assert_eq!(run.tree_changed_batches("ilium.core.titles"), 1);
    assert_eq!(run.tree_changed_nodes("ilium.core.titles"), 30);
}

/// REQ: HST-026
#[test]
fn hst_026_each_subscriber_gets_exactly_one_batch_and_no_restore_replay() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(30)
        .plugin(startup("ilium.core.titles").subscribes("tree-changed"))
        .plugin(startup("ilium.core.cost").subscribes("tree-changed"))
        .build();
    run.start();
    for plugin in ["ilium.core.titles", "ilium.core.cost"] {
        assert_eq!(run.tree_changed_batches(plugin), 1, "{plugin}");
        assert_eq!(run.restore_step_events(plugin), 0, "{plugin}");
    }
}

/// REQ: HST-026
#[test]
fn hst_026_a_plugin_that_does_not_subscribe_gets_no_batch() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(30)
        .plugin(startup("ilium.core.titles"))
        .build();
    run.start();
    assert_eq!(run.tree_changed_batches("ilium.core.titles"), 0);
}

// HST-029
/// REQ: HST-029
#[test]
fn hst_029_shutdown_markers_appear_in_the_documented_order() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.shutdown();
    let trace = run.trace();
    let order = [
        "services-stop",
        "refuse-submissions",
        "deactivate",
        "drain-session-work",
        "final-snapshot",
    ];
    let positions: Vec<usize> = order
        .iter()
        .map(|marker| {
            trace
                .iter()
                .position(|entry| entry == marker)
                .unwrap_or_else(|| panic!("missing {marker}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}"
    );
}

/// REQ: HST-029
#[test]
fn hst_029_submissions_are_refused_as_cancelled_before_the_first_deactivate() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.shutdown_hold_before_deactivate();
    let outcome = run.submit_from("ilium.core.titles");
    assert_eq!(outcome, "cancelled");
}

// HST-030
fn shutdown_run(count: usize, slow_ms: u64) -> ilium_plugin_host::testing::boot::BootRun {
    let mut harness = BootHarness::new(ProcessKind::Server);
    for index in 0..count {
        harness =
            harness.plugin(startup(&format!("ilium.core.p{index}")).deactivate_takes_ms(slow_ms));
    }
    let mut run = harness.build();
    run.start();
    run
}

/// REQ: HST-030
#[test]
fn hst_030_deactivation_runs_in_reverse_activation_order() {
    let mut run = shutdown_run(4, 0);
    let activated = run.activation_order();
    run.shutdown();
    let mut reverse = activated.clone();
    reverse.reverse();
    assert_eq!(run.deactivation_order(), reverse);
}

/// REQ: HST-030
#[test]
fn hst_030_total_plugin_time_up_to_five_seconds_deactivates_everyone() {
    let mut run = shutdown_run(5, 1000);
    let report = run.shutdown();
    assert_eq!(report.deactivated.len(), 5);
    assert!(report.forced.is_empty());
    assert!(run.shutdown_plugin_time() <= Duration::from_secs(5));
}

/// REQ: HST-030
#[test]
fn hst_030_over_five_seconds_drops_the_rest_without_a_call() {
    let mut run = shutdown_run(6, 1000);
    let report = run.shutdown();
    assert!(run.shutdown_plugin_time() <= Duration::from_secs(5));
    assert_eq!(report.deactivated.len(), 5);
    assert_eq!(report.forced.len(), 1);
    assert_eq!(run.deactivate_calls(&report.forced[0]), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    // REQ: HST-030
    #[test]
    fn hst_030_shutdown_plugin_time_never_exceeds_the_budget(count in 1usize..10, slow in 0u64..2500) {
        let mut run = shutdown_run(count, slow);
        run.shutdown();
        prop_assert!(run.shutdown_plugin_time() <= Duration::from_secs(5));
    }
}

// HST-031
/// REQ: HST-031
#[test]
fn hst_031_a_client_connecting_before_step_six_gets_server_starting() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup("ilium.core.titles"))
        .build();
    run.begin_start_paused_before_accept();
    let error = run.attach_client().expect_err("refused");
    assert_eq!(error.as_str(), "server.starting");
}

/// REQ: HST-031
#[test]
fn hst_031_the_refusal_is_retriable_and_the_client_retries_for_five_seconds() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup("ilium.core.titles"))
        .build();
    run.begin_start_paused_before_accept();
    let attempt = run.client_connect_with_retry(Duration::from_secs(5), Duration::from_secs(2));
    assert!(
        attempt.is_ok(),
        "the server opens at 2 s, inside the 5 s window"
    );
}

/// REQ: HST-031
#[test]
fn hst_031_a_client_gives_up_after_five_seconds() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .plugin(startup("ilium.core.titles"))
        .build();
    run.begin_start_paused_before_accept();
    let attempt = run.client_connect_with_retry(Duration::from_secs(5), Duration::from_secs(6));
    assert!(attempt.is_err());
}

// HST-032
/// REQ: HST-032
#[test]
fn hst_032_client_shutdown_is_bounded_at_two_seconds() {
    let mut run = started(vec![two_sided("ilium.core.tree").deactivate_takes_ms(1500)]);
    let client = run.attach_client().expect("client");
    let elapsed = run.client_shutdown(client);
    assert!(elapsed <= Duration::from_secs(2), "{elapsed:?}");
}

/// REQ: HST-032
#[test]
fn hst_032_a_slow_client_shutdown_is_cut_at_two_seconds() {
    let mut run = started(vec![two_sided("ilium.core.tree").deactivate_takes_ms(9000)]);
    let client = run.attach_client().expect("client");
    let elapsed = run.client_shutdown(client);
    assert!(elapsed <= Duration::from_secs(2), "{elapsed:?}");
}

/// REQ: HST-032
#[test]
fn hst_032_an_exiting_client_does_not_stop_outcomes_for_other_clients() {
    let mut run = started(vec![two_sided("ilium.core.tree").deactivate_takes_ms(9000)]);
    let leaving = run.attach_client().expect("leaving");
    let staying = run.attach_client().expect("staying");
    run.client_shutdown(leaving);
    assert!(run.deliver_outcome_to_client(staying));
}

// HST-035
/// REQ: HST-035
#[test]
fn hst_035_a_trap_ends_only_the_trapping_instance() {
    let mut run = started(vec![
        startup("ilium.core.titles"),
        startup("ilium.core.cost"),
    ]);
    run.trap_in("ilium.core.titles", Side::Server);
    assert_eq!(
        run.side_state("ilium.core.cost", Side::Server).state,
        "active"
    );
}

/// REQ: HST-035
#[test]
fn hst_035_pending_requests_of_the_trapped_instance_complete_as_cancelled() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.leave_requests_pending("ilium.core.titles", 3);
    run.trap_in("ilium.core.titles", Side::Server);
    assert_eq!(
        run.pending_outcomes("ilium.core.titles"),
        vec!["cancelled"; 3]
    );
}

/// REQ: HST-035
#[test]
fn hst_035_retained_contributions_are_kept_and_marked_stale() {
    let mut run = started(vec![
        startup("ilium.core.titles").contributes("ilium.core.titles/before-title")
    ]);
    run.trap_in("ilium.core.titles", Side::Server);
    let contributions = run.contributions();
    assert_eq!(contributions.len(), 1);
    assert!(contributions[0].stale);
}

// HST-039
/// REQ: HST-039
#[test]
fn hst_039_a_killed_helper_does_not_crash_the_parent() {
    let mut run = started(vec![
        PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"])
    ]);
    run.kill_helper();
    assert!(run.server_alive());
}

/// REQ: HST-039
#[test]
fn hst_039_a_hung_helper_does_not_hang_the_parent() {
    let mut run = started(vec![
        PluginSpec::new("acme.t2", Tier::T2).activation(&["startup"])
    ]);
    run.hang_helper();
    let elapsed = run.timed_pump(Duration::from_secs(10));
    assert!(elapsed < Duration::from_secs(10));
    assert!(run.server_alive());
}

// HST-040
/// REQ: HST-040
#[test]
fn hst_040_disabling_a_core_plugin_keeps_the_session_running() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.fail_until_disabled("ilium.core.titles", Side::Server);
    assert!(run.server_alive());
}

/// REQ: HST-040
#[test]
fn hst_040_the_disabled_state_persists_across_restarts() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    run.fail_until_disabled("ilium.core.titles", Side::Server);
    let mut again = run.restart_server();
    assert!(again.disabled_reason("ilium.core.titles").is_some());
}

/// REQ: HST-040
#[test]
fn hst_040_the_recovery_page_opens_once_in_the_affected_client() {
    let mut run = started(vec![two_sided("ilium.core.tree")]);
    let client = run.attach_client().expect("client");
    run.fail_until_disabled_in_client(client, "ilium.core.tree");
    assert_eq!(run.recovery_page_opens(client), 1);
    run.advance(Duration::from_secs(60));
    assert_eq!(run.recovery_page_opens(client), 1);
}

// HST-041
/// REQ: HST-041
#[test]
fn hst_041_the_recovery_page_works_with_every_plugin_disabled() {
    let mut harness = BootHarness::new(ProcessKind::Server);
    for name in CORE_PLUGINS {
        harness = harness.plugin(startup(&core_id(name)));
    }
    let mut run = harness.build();
    run.start();
    run.disable_all_plugins();
    assert!(run.command_works("kernel.open-recovery"));
    assert!(run.recovery_page_renders());
}

/// REQ: HST-041
#[test]
fn hst_041_a_plugin_cannot_remove_the_recovery_command() {
    let mut run = started(vec![startup("ilium.core.titles")]);
    let outcome =
        run.plugin_tries_to_unregister_command("ilium.core.titles", "kernel.open-recovery");
    assert!(outcome.is_err());
    assert!(run.command_works("kernel.open-recovery"));
}

// HST-042
/// REQ: HST-042
#[test]
fn hst_042_with_tree_disabled_rows_come_from_names_and_status_only() {
    let mut run = BootHarness::new(ProcessKind::Server)
        .restored_tree(5)
        .plugin(startup("ilium.core.tree"))
        .build();
    run.start();
    run.disable("ilium.core.tree");
    let rows = run.kernel_tree_rows();
    assert_eq!(rows.len(), 5);
    for row in rows {
        assert!(row.uses_only_name_and_status);
    }
}
