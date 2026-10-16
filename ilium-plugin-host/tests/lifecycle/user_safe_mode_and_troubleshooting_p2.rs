//! Part 2: LIF-185, LIF-186 (bisect step bound, crash-loop guard).
use crate::support::*;

fn host_with(n: usize) -> LifeHost {
    let mut host = LifeHost::new();
    for i in 0..n {
        host.install(PluginSpec::new(&format!("com.test.p{i:02}")).sides(&[Side::Server]).startup());
    }
    host.start(StartMode::Normal);
    host
}

/// Runs bisect with the culprit being `culprit`; returns (steps, found id).
fn run_bisect(host: &mut LifeHost, culprit: usize) -> (u32, Option<String>) {
    let culprit_id = format!("com.test.p{culprit:02}");
    host.cli(&["plugin", "bisect", "start"]);
    let mut steps = 0;
    for _ in 0..40 {
        let culprit_running = host.running_plugins().contains(&culprit_id);
        let verb = if culprit_running { "bad" } else { "good" };
        let out = host.cli(&["plugin", "bisect", verb]);
        steps += 1;
        for line in out.stdout_jsonl.iter().filter(|l| l["type"] == "bisect") {
            if let Some(found) = line["found"].as_str() {
                return (steps, Some(found.to_owned()));
            }
        }
    }
    (steps, None)
}

/// REQ: LIF-185
#[test]
fn lif_185_sixteen_plugins_finish_in_at_most_four_steps() {
    let mut host = host_with(16);
    let (steps, found) = run_bisect(&mut host, 11);
    assert!(steps <= 4, "steps {steps}");
    assert_eq!(found.as_deref(), Some("com.test.p11"));
}

/// REQ: LIF-185
#[test]
fn lif_185_every_culprit_position_of_sixteen_is_found_within_four_steps() {
    for culprit in 0..16 {
        let mut host = host_with(16);
        let (steps, found) = run_bisect(&mut host, culprit);
        assert!(steps <= 4, "culprit {culprit}: steps {steps}");
        assert_eq!(found, Some(format!("com.test.p{culprit:02}")));
    }
}

/// REQ: LIF-185
#[test]
fn lif_185_seventeen_plugins_need_at_most_five_steps() {
    for culprit in [0usize, 8, 16] {
        let mut host = host_with(17);
        let (steps, found) = run_bisect(&mut host, culprit);
        assert!(steps <= 5, "culprit {culprit}: steps {steps}");
        assert_eq!(found, Some(format!("com.test.p{culprit:02}")));
    }
}

/// REQ: LIF-185
#[test]
fn lif_185_two_plugins_need_one_step() {
    let mut host = host_with(2);
    let (steps, found) = run_bisect(&mut host, 1);
    assert_eq!(steps, 1);
    assert_eq!(found.as_deref(), Some("com.test.p01"));
}

/// REQ: LIF-186
#[test]
fn lif_186_three_start_crashes_within_five_minutes_select_third_party_safe_mode() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    for _ in 0..3 {
        host.crash_during_start();
        host.advance_ms(30_000);
    }
    host.start_after_crashes();
    assert!(matches!(host.safe_mode(), SafeMode::ThirdParty));
}

/// REQ: LIF-186
#[test]
fn lif_186_two_start_crashes_do_not_select_safe_mode() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    for _ in 0..2 {
        host.crash_during_start();
        host.advance_ms(30_000);
    }
    host.start_after_crashes();
    assert!(matches!(host.safe_mode(), SafeMode::Off));
}

/// REQ: LIF-186
#[test]
fn lif_186_three_crashes_spread_over_more_than_five_minutes_do_not_select_safe_mode() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    for _ in 0..3 {
        host.crash_during_start();
        host.advance_ms(200_000);
    }
    host.start_after_crashes();
    assert!(matches!(host.safe_mode(), SafeMode::Off));
}

/// REQ: LIF-186
#[test]
fn lif_186_guard_clears_after_a_start_that_stays_up_five_minutes() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    for _ in 0..3 {
        host.crash_during_start();
    }
    host.start_after_crashes();
    host.start(StartMode::Normal);
    host.advance_ms(300_000);
    host.shutdown();
    host.start_after_crashes();
    assert!(matches!(host.safe_mode(), SafeMode::Off));
}
