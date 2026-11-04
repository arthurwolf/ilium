//! Requirement-coverage tests for the production performance and sizing rules (PRF-027 to PRF-043).
//!
//! Each test names the requirement it exercises in a `/// REQ:` line. Where the statement has no
//! entry point in the repository, the test calls the closest existing API and asserts the
//! documented behaviour; those tests are expected to fail until the behaviour exists, and the
//! assertion message says what is missing.

use std::time::Instant;

use ilium_plugin_api::generated::host::tree::HostTree;
use ilium_plugin_api::generated::types::events::{DurableTimerEvent, Event};
use ilium_plugin_api::generated::types::ids::TimerId;
use ilium_plugin_api::generated::types::tree::NodeQuery;
use ilium_plugin_api::ids::{NodeId, PluginId};
use ilium_plugin_host::host_capacity::CapacityHost;
use ilium_plugin_host::imports::tree::TreeImport;
use ilium_plugin_host::scheduler::{CallKind, EventPush, Scheduler};

/// The plugin side used by the scheduler tests.
fn side() -> PluginId {
    PluginId("prf-plugin".to_owned())
}

/// An overdue durable-timer fire. Its due-time order is its index, and its lateness is its index.
fn overdue_fire(index: u64) -> Event {
    Event::DurableTimerFired(DurableTimerEvent {
        timer: TimerId(index),
        name: format!("timer-{index}"),
        late_by_millis: index,
    })
}

/// REQ: PRF-027
#[test]
fn overdue_durable_timers_are_delivered_once_in_due_order_in_batches_of_at_most_32() {
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 256);
    for index in 0..100u64 {
        assert!(
            matches!(scheduler.push_event(&side, overdue_fire(index)), EventPush::Queued),
            "PRF-027: overdue fire {index} must be queued without being dropped or merged"
        );
    }

    let mut delivered_timer_ids = Vec::new();
    loop {
        let batch = scheduler.take_events(&side, 32);
        if batch.is_empty() {
            break;
        }
        assert!(
            batch.len() <= 32,
            "PRF-027: a batch holds {} durable-timer-fired events, the limit is 32",
            batch.len()
        );
        for event in batch {
            match event {
                Event::DurableTimerFired(fired) => delivered_timer_ids.push(fired.timer.0),
                other => panic!("PRF-027: unexpected event in the overdue batch: {other:?}"),
            }
        }
    }
    assert_eq!(
        delivered_timer_ids,
        (0..100u64).collect::<Vec<u64>>(),
        "PRF-027: every one of the 100 overdue timers must arrive exactly once, in due-time order. \
         Note: no start-time trigger exists in the scheduler; this checks the batch drain only"
    );
}

/// REQ: PRF-028
#[test]
fn overdue_fires_beyond_a_full_64_entry_queue_are_held_not_refused_so_the_plugin_is_not_restarted()
{
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 64);

    let mut refused_fires = Vec::new();
    for index in 0..256u64 {
        if matches!(
            scheduler.push_event(&side, overdue_fire(index)),
            EventPush::Stuck | EventPush::DiscardedIncoming | EventPush::DroppedOldest
        ) {
            refused_fires.push(index);
        }
    }
    assert!(
        refused_fires.is_empty(),
        "PRF-028 not implemented: the scheduler refuses {} of 256 overdue fires once the 64-entry \
         queue is full (first refused: fire {:?}). The host must hold pending fires outside the \
         plugin event queue and admit them only while the queue has room",
        refused_fires.len(),
        refused_fires.first()
    );
    let delivered = scheduler.take_events(&side, usize::MAX).len();
    assert_eq!(
        delivered, 256,
        "PRF-028: all 256 overdue fires must arrive after a burst of 256 against a 64-entry queue"
    );
}

/// REQ: PRF-029
#[test]
fn overdue_fires_wait_for_an_empty_execution_bucket_and_are_never_refused_with_quota_exceeded() {
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 64);
    scheduler.set_bucket_balance_ms(&side, 0);

    for index in 0..10u64 {
        let result = scheduler.push_event(&side, overdue_fire(index));
        assert!(
            !matches!(
                result,
                EventPush::Stuck | EventPush::DiscardedIncoming | EventPush::DroppedOldest
            ),
            "PRF-029: overdue fire {index} must not be refused or dropped with an empty bucket"
        );
    }
    assert_eq!(
        scheduler.queue_len(&side),
        10,
        "PRF-029: all 10 overdue fires must wait in the pending list while the bucket is empty"
    );

    let early = scheduler.take_events(&side, 10);
    assert!(
        early.is_empty(),
        "PRF-029 not implemented: take_events hands out {} overdue fires while the \
         ilium-execution bucket balance is 0 ms; delivery must wait until the balance is above 0 ms",
        early.len()
    );

    scheduler.set_bucket_balance_ms(&side, 1);
    let delivered = scheduler.take_events(&side, 10);
    assert_eq!(
        delivered.len(),
        10,
        "PRF-029: after the balance rises above 0 ms all 10 overdue fires must be delivered"
    );
}

/// REQ: PRF-031
#[test]
fn more_than_32_pending_overdue_fires_after_a_start_write_one_summary_log_line() {
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 256);
    for index in 0..100u64 {
        let _ = scheduler.push_event(&side, overdue_fire(index));
    }

    let summaries: Vec<_> = scheduler
        .log_lines()
        .into_iter()
        .filter(|line| line.message.contains("100"))
        .collect();
    assert_eq!(
        summaries.len(),
        1,
        "PRF-031 not implemented: expected one host-log entry stating 100 pending fires for the \
         start, but the scheduler log holds {} lines in total and no start-time summary is written",
        scheduler.log_lines().len()
    );
    assert!(
        summaries[0].message.contains("99"),
        "PRF-031: the summary must state the largest late-by-millis delivered (99): {}",
        summaries[0].message
    );
}

/// REQ: PRF-037
#[test]
fn find_with_every_field_unset_returns_the_first_4096_ids_of_a_5000_node_tree() {
    let mut tree = TreeImport;
    let query = NodeQuery {
        content_kind: None,
        signature: None,
        under: None,
    };
    let ids = tree.find(query);
    assert_eq!(
        ids.len(),
        4096,
        "PRF-037 not implemented: TreeImport is a skeleton with no node store, so find returns {} \
         ids for a tree that should hold 5000 nodes; it must cap the result at the first 4096 ids \
         in tree order without an error",
        ids.len()
    );
    assert_eq!(
        ids.first(),
        Some(&NodeId(0)),
        "PRF-037: the first returned id must be the first node in tree order"
    );
}

/// REQ: PRF-038
#[test]
fn tree_changed_burst_of_10000_nodes_is_delivered_in_events_of_at_most_4096_ids_without_drops() {
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 64);
    for index in 0..10_000u64 {
        let _ = scheduler.push_event(&side, Event::TreeChanged(vec![NodeId(index)]));
    }

    let events = scheduler.take_events(&side, usize::MAX);
    let mut event_count = 0usize;
    let mut delivered_ids = Vec::new();
    for event in events {
        match event {
            Event::TreeChanged(nodes) => {
                event_count += 1;
                assert!(
                    nodes.len() <= 4096,
                    "PRF-038 not implemented: one tree-changed event carries {} ids; the host must \
                     split a larger burst into events of at most 4096 ids",
                    nodes.len()
                );
                delivered_ids.extend(nodes.into_iter().map(|node| node.0));
            }
            other => panic!("PRF-038: unexpected event in the tree-changed burst: {other:?}"),
        }
    }
    assert!(
        event_count >= 3,
        "PRF-038: 10,000 changed nodes need at least three tree-changed events, got {event_count}"
    );
    assert_eq!(
        delivered_ids,
        (0..10_000u64).collect::<Vec<u64>>(),
        "PRF-038: the events must cover each of the 10,000 ids exactly once"
    );
}

/// REQ: PRF-042
#[test]
fn job_start_is_refused_with_busy_when_the_16_entry_queue_is_full() {
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 16);
    let now = Instant::now();
    for _ in 0..17 {
        // The returned ticket carries no refusal, so the only observable refusal is an
        // abandoned call. A refused start is expected to appear there.
        let _ticket = scheduler.enqueue(&side, CallKind::JobRun, now, Box::new(|| {}));
    }
    let refused_starts = scheduler
        .abandoned()
        .iter()
        .filter(|call| call.plugin.0 == side.0)
        .count();
    assert!(
        refused_starts >= 1,
        "PRF-042 not implemented: Scheduler::enqueue(CallKind::JobRun) accepts a 17th start with \
         a full 16-entry queue and returns only a ticket; it must refuse it with `busy`. The \
         `invalid-argument` refusal of an oversized input is not reachable through this API"
    );
}

/// REQ: PRF-043
#[test]
fn host_capacity_refusal_of_an_activation_is_reported_as_host_capacity() {
    let mut host = CapacityHost::new(10, 1);
    assert!(
        host.activate("prf-first").is_ok(),
        "PRF-043: the first activation within the active ceiling must succeed"
    );
    match host.activate("prf-second") {
        Err(refusal) => assert_eq!(
            refusal.reason, "host-capacity",
            "PRF-043: a start at the machine-wide ceiling must be refused with `host-capacity`"
        ),
        Ok(()) => panic!(
            "PRF-043: a second activation at the active ceiling of 1 must be refused with \
             `host-capacity`"
        ),
    }
    // The refusal half is covered above. The release half (the same start succeeding after a job
    // is released) has no entry point: CapacityHost exposes no release or deactivate operation.
}

/// REQ: PRF-030
#[test]
fn prf_030_rearmed_repeating_schedule_is_due_from_now_not_from_the_fired_due_time() {
    // Closest existing entry point: Scheduler::push_event with a fired durable timer. It queues the
    // fire and arms nothing, so no re-armed due time exists to check.
    let scheduler = Scheduler::new(10);
    let side = side();
    scheduler.register_side(&side, 64);
    assert!(
        matches!(scheduler.push_event(&side, overdue_fire(0)), EventPush::Queued),
        "PRF-030: the fired durable timer must be queued"
    );
    panic!(
        "PRF-030 not implemented: no durable-timer re-arm exists in the host. Nothing computes the \
         next due time of a repeating schedule from the current time, so a 10-hour outage cannot be \
         checked to arm one timer due within one hour rather than ten"
    );
}

/// REQ: PRF-032
#[test]
fn prf_032_plugin_reads_a_host_power_value_that_changes_within_5_seconds_of_unplugging() {
    panic!(
        "PRF-032 not implemented: the host and plugin API expose no power-state value (no on-battery \
         or power-saving field in ilium-plugin-api or ilium-plugin-host), and no fake host can switch \
         one. A plugin therefore has no host value to read and no way to be shown the change"
    );
}

/// REQ: PRF-033
#[test]
fn prf_033_scene_frame_rate_is_capped_at_30_fps_on_battery_and_not_on_mains() {
    panic!(
        "PRF-033 not implemented: no host code caps a scene's frame rate by power state. The scene \
         frame_rate_cap is not clamped to 30 fps on battery, and a 60 fps scene cannot be run under a \
         fake battery state to show 30 fps on battery and 60 fps on mains"
    );
}

/// REQ: PRF-034
#[test]
fn prf_034_ordinary_timer_armed_at_50_ms_fires_no_more_often_than_250_ms_on_battery() {
    panic!(
        "PRF-034 not implemented: the host has no ordinary timer API and no power-state floor. A timer \
         armed at 50 ms is not held to a 250 ms minimum interval on battery, and at 50 ms on mains"
    );
}

/// REQ: PRF-035
#[test]
fn prf_035_handler_burning_25_ms_is_trapped_at_the_20_ms_deadline_with_or_without_a_throttled_core() {
    panic!(
        "PRF-035 not implemented: the host has no thermal or clock-throttle signal. Deadlines and the \
         execution bucket are not shown to stay unscaled under a throttled core, so a 25 ms CPU burn \
         cannot be checked to trap at the 20 ms deadline with a throttled core"
    );
}

/// REQ: PRF-036
#[test]
fn prf_036_plugin_with_one_250_ms_timer_and_no_events_wakes_4_or_fewer_times_per_second() {
    panic!(
        "PRF-036 not implemented: the host has no wake-up counter that separates timer expiries and \
         poll wake-ups with no event to handle. A 60 second run of a plugin with one 250 ms timer \
         cannot be checked for 4 or fewer wake-ups per second"
    );
}

/// REQ: PRF-039
#[test]
fn prf_039_tree_walk_inside_on_events_is_trapped_while_an_index_built_in_a_job_is_not() {
    panic!(
        "PRF-039 not implemented: TreeImport::children and TreeImport::find are stubs that return an \
         empty list, so a 5,000 node tree cannot be walked. The on-events walk cannot be trapped at the \
         20 ms deadline, and the job-built index comparison cannot run"
    );
}

/// REQ: PRF-040
#[test]
fn prf_040_benchmark_records_median_and_p99_for_children_and_tree_changed_without_failing() {
    panic!(
        "PRF-040 not implemented: no benchmark for a 10,000 node tree exists. Nothing records the median \
         and 99th percentile of one root children call or of one tree-changed delivery with 1,000 ids"
    );
}

/// REQ: PRF-041
#[test]
fn prf_041_change_to_concurrent_jobs_or_memory_mib_states_worst_case_mib_in_the_commit() {
    panic!(
        "PRF-041 not implemented: no review checklist or commit check requires the worst-case figure of \
         concurrent_jobs times memory_mib in MiB when either value changes. Nothing in the repository \
         enforces the statement"
    );
}
