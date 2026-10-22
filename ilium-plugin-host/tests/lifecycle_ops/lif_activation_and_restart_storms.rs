//! Activation latency and restart storms (LIF-315, LIF-316).
//!
//! The reference-machine targets of LIF-315 are checked on the machine that runs the test. The
//! CPU-use bound of LIF-316 (below one core after 10 s) needs a CPU meter the rig does not have,
//! so this file checks only the restart concurrency and the restart spacing.

use ilium_plugin_api::Side;

use crate::common::*;

/// Sixty-four distinct plugin ids, each installed at version 1.0.0.
fn sixty_four_ids() -> Vec<String> {
    (0..64)
        .map(|index| format!("com.example.load{index:02}"))
        .collect()
}

/// The `p`-th percentile (nearest rank) of a sorted slice of durations.
fn nearest_rank(sorted: &[std::time::Duration], percent: usize) -> std::time::Duration {
    let rank = (sorted.len() * percent).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

/// REQ: LIF-315
#[test]
fn lif_315_sixty_four_plugins_all_activate_on_each_start() {
    let rig = rig();
    let ids = sixty_four_ids();
    for id in &ids {
        install_ok(&rig, &pkg(id, "1.0.0"));
    }
    let server = rig.start_server("default");
    assert!(ids.iter().all(|id| server.active_version(id).is_some()));
}

/// REQ: LIF-315
#[test]
fn lif_315_activation_meets_p95_200_ms_and_p99_1_s_over_100_runs() {
    let rig = rig();
    let ids = sixty_four_ids();
    for id in &ids {
        install_ok(&rig, &pkg(id, "1.0.0"));
    }
    let mut durations = Vec::with_capacity(100);
    for _ in 0..100 {
        let started = std::time::Instant::now();
        let server = rig.start_server("default");
        durations.push(started.elapsed());
        server.shutdown();
    }
    durations.sort();
    assert!(nearest_rank(&durations, 95) <= ms(200));
    assert!(nearest_rank(&durations, 99) <= secs(1));
}

/// REQ: LIF-316
#[test]
fn lif_316_failing_plugins_start_at_most_four_restarts_at_once() {
    let rig = rig();
    let ids = sixty_four_ids();
    let failing = &ids[..32];
    for id in failing {
        install_ok(&rig, &pkg(id, "1.0.0"));
    }
    let server = rig.start_server("default");
    for id in failing {
        server.fail(id, Side::Server);
    }
    let immediate = failing
        .iter()
        .filter(|id| server.restart_delay_ms(id, Side::Server) == Some(0))
        .count();
    assert!(immediate <= 4, "restarts started at once: {immediate}");
}

/// REQ: LIF-316
#[test]
fn lif_316_restarts_beyond_the_first_four_are_spaced_by_backoff() {
    let rig = rig();
    let ids = sixty_four_ids();
    let failing = &ids[..32];
    for id in failing {
        install_ok(&rig, &pkg(id, "1.0.0"));
    }
    let server = rig.start_server("default");
    for id in failing {
        server.fail(id, Side::Server);
    }
    let spaced = failing
        .iter()
        .filter(|id| matches!(server.restart_delay_ms(id, Side::Server), Some(delay) if delay > 0))
        .count();
    assert!(spaced >= 28, "spaced restarts: {spaced}");
}
