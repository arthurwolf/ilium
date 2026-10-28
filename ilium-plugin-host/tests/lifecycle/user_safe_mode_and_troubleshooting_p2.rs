//! Part 2: LIF-185, LIF-186, LIF-294, LIF-187 (bisect step bound, crash-loop guard, bisect without a culprit).
use crate::support::*;

fn host_with(n: usize) -> LifeHost {
    let mut host = LifeHost::new();
    for i in 0..n {
        host.install(
            PluginSpec::new(&format!("com.test.p{i:02}"))
                .sides(&[Side::Server])
                .startup(),
        );
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

/// REQ: LIF-294
#[test]
fn lif_294_guard_clears_after_a_start_that_stays_up_five_minutes() {
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

/// REQ: LIF-187
#[test]
fn lif_187_a_fault_outside_every_plugin_ends_with_not_a_plugin() {
    let mut host = host_with(4);
    host.cli(&["plugin", "bisect", "start"]);
    let mut last = None;
    for _ in 0..40 {
        let out = host.cli(&["plugin", "bisect", "bad"]);
        let record = out
            .stdout_jsonl
            .iter()
            .find(|line| line["type"] == "bisect")
            .cloned();
        let finished = record
            .as_ref()
            .is_some_and(|line| line["reason"] == "not-a-plugin");
        last = record;
        if finished {
            break;
        }
    }
    let record = last.expect("a bisect record");
    assert_eq!(record["reason"], "not-a-plugin", "{record}");
    assert!(record["found"].is_null(), "{record}");
}

/// REQ: LIF-187
#[test]
fn lif_187_not_a_plugin_record_names_no_plugin_when_every_half_is_good() {
    let mut host = host_with(4);
    host.cli(&["plugin", "bisect", "start"]);
    let mut last = None;
    for _ in 0..40 {
        let out = host.cli(&["plugin", "bisect", "good"]);
        let record = out
            .stdout_jsonl
            .iter()
            .find(|line| line["type"] == "bisect")
            .cloned();
        let finished = record
            .as_ref()
            .is_some_and(|line| line["reason"] == "not-a-plugin");
        last = record;
        if finished {
            break;
        }
    }
    let record = last.expect("a bisect record");
    assert_eq!(record["reason"], "not-a-plugin", "{record}");
    assert!(record["found"].is_null(), "{record}");
    assert!(!record.to_string().contains("com.test.p"), "{record}");
}

/// REQ: LIF-294
#[test]
fn lif_294_a_stop_before_five_minutes_keeps_the_crash_marks() {
    let mut host = LifeHost::new();
    host.install(PluginSpec::new(A).sides(&[Side::Server]).startup());
    for _ in 0..3 {
        host.crash_during_start();
    }
    host.start_after_crashes();
    host.start(StartMode::Normal);
    host.advance_ms(299_000);
    host.shutdown();
    host.start_after_crashes();
    assert!(matches!(host.safe_mode(), SafeMode::ThirdParty));
}
