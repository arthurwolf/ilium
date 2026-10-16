//! `ilium wait` is the same command as `ilium progress wait` (F-108, F-060): the plugin declares
//! the alias `wait` and the kernel keeps no `wait` command of its own.

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";

/// REQ: F-108
#[test]
fn f_108_wait_without_a_monitor_prints_the_same_record_as_progress_wait() {
    let (mut host, pane) = host_with_pane();
    let alias = run_in_pane(&mut host, pane, &["wait"]);
    let full = progress(&mut host, pane, &["wait"]);
    assert_eq!(alias.exit, Some(5));
    assert_eq!(alias.exit, full.exit);
    assert_eq!(alias.stdout, full.stdout);
}

/// REQ: F-108
#[test]
fn f_108_wait_with_an_id_waits_for_that_monitor_and_exits_zero_on_done() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("export-1", 0, "Exporting"), &["--interval-seconds", "5"]);
    expect_probe(&mut host, PROBE, &done("export-1", "Finished"));
    tick(&mut host, 5);
    let run = run_in_pane(&mut host, pane, &["wait", &id.to_string()]);
    assert_eq!(run.exit, Some(0));
    let record = record_of(&run.stdout, "progress_wait");
    assert_eq!(record["outcome"], "done");
    assert_eq!(record["monitor_id"], id);
}

/// REQ: F-108
#[test]
fn f_108_wait_with_a_timeout_exits_six_when_it_elapses() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running("export-1", 0, "Exporting"), &["--interval-seconds", "5"]);
    host.process().allow_unexpected();
    let open = start_in_pane(&mut host, pane, &["wait", "--timeout-seconds", "2"]);
    tick(&mut host, 2);
    let run = open.read();
    assert_eq!(run.exit, Some(6));
    assert_eq!(record_of(&run.stdout, "progress_wait")["outcome"], "still-running");
}

/// REQ: F-108
#[test]
fn f_108_wait_with_several_monitors_and_no_id_is_refused_with_monitor_ambiguous() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let run = run_in_pane(&mut host, pane, &["wait"]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(record_of(&run.stdout, "progress_rejected")["code"], "monitor-ambiguous");
}

/// REQ: F-108, COR-040
#[test]
fn f_108_the_alias_is_declared_by_the_plugin_and_not_by_a_second_entry() {
    let manifest = manifest_value();
    assert_eq!(manifest["contributes"]["cli"]["aliases"], serde_json::json!(["wait"]));
    assert_eq!(manifest["contributes"]["cli"]["name"], "progress");
}

/// REQ: F-108
#[test]
fn f_108_the_cli_compatibility_page_names_wait_as_an_alias_owned_by_progress() {
    let text = page("reference-cli-compatibility");
    assert!(text.contains("| `ilium wait` | progress | [progress](/pages/core-plugin-progress/) | yes | JSONL | Alias of `ilium progress wait`. |"));
}

/// REQ: F-108
#[test]
fn f_108_wait_outside_a_pane_fails_like_progress_wait() {
    let (mut host, _pane) = host_with_pane();
    set_pane_env(&mut host, None);
    let alias = {
        let invocation = host.cli().invoke(&["wait"], b"");
        (invocation.stdout(), invocation.exit_code())
    };
    let full = {
        let invocation = host.cli().invoke(&["progress", "wait"], b"");
        (invocation.stdout(), invocation.exit_code())
    };
    assert_eq!(alias, full);
    assert_ne!(alias.1, Some(0));
    assert_eq!(record_of(&alias.0, "progress_request_failed")["code"], "pane-identity-unavailable");
}
