//! Which monitor `ilium progress wait` picks and what a settled wait names (original
//! ilium/src/progress_wait.rs).

use serde_json::json;

use crate::common::*;
use crate::support::*;

const A: &str = "/opt/a.sh";
const B: &str = "/opt/b.sh";

fn wait_next(run: &Run) -> String {
    record_of(&run.stdout, "progress_wait")["next"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// REQ: PORT:ilium/src/progress_wait.rs::a_settled_wait_names_the_exact_clear_command_for_its_own_monitor
#[test]
fn port_060_a_settled_wait_names_the_clear_command_of_its_own_monitor_only() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    let run = progress(&mut host, pane, &["wait", &id.to_string()]);
    assert!(
        wait_next(&run).ends_with(&format!("`ilium progress clear --monitor-id {id}`.")),
        "{}",
        wait_next(&run)
    );
    progress(&mut host, pane, &["clear", "--monitor-id", &id.to_string()]);
    let cleared = progress(&mut host, pane, &["wait", &id.to_string()]);
    assert!(
        !wait_next(&cleared).contains("progress clear"),
        "{}",
        wait_next(&cleared)
    );
    let none = progress(&mut host, pane, &["wait"]);
    assert!(
        !wait_next(&none).contains("progress clear"),
        "{}",
        wait_next(&none)
    );
}

/// REQ: PORT:ilium/src/progress_wait.rs::a_wait_without_an_id_picks_the_only_unsettled_monitor_or_refuses_to_guess
#[test]
fn port_060_a_wait_without_an_id_picks_the_only_unsettled_monitor_or_refuses_to_guess() {
    let (mut host, pane) = host_with_pane();
    let none = progress(&mut host, pane, &["wait"]);
    assert_eq!(none.exit, Some(5));
    let first = register(&mut host, pane, A, &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, B, &running("job-b", 0, "B"), &[]);
    let ambiguous = progress(&mut host, pane, &["wait"]);
    assert_eq!(ambiguous.exit, Some(2));
    let record = record_of(&ambiguous.stdout, "progress_rejected");
    assert_eq!(record["code"], "monitor-ambiguous");
    assert_eq!(record["monitor_ids"], json!([first, second]));
    assert!(
        record["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ilium progress wait"),
        "{record}"
    );
    expect_probe(&mut host, A, &done("job-a", "A done"));
    expect_probe(&mut host, B, &running("job-b", 5, "B"));
    tick(&mut host, 1);
    let open = start_in_pane(&mut host, pane, &["progress", "wait"]);
    expect_probe(&mut host, A, &done("job-a", "A done"));
    expect_probe(&mut host, B, &done("job-b", "B done"));
    tick(&mut host, 1);
    assert_eq!(
        record_of(&open.read().stdout, "progress_wait")["monitor_id"],
        second
    );
}

/// REQ: PORT:ilium/src/progress_wait.rs::a_wait_without_an_id_picks_the_only_unsettled_monitor_or_refuses_to_guess
#[test]
fn port_060_a_wait_without_an_id_is_refused_when_a_done_and_an_error_monitor_remain() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, A, &running("job-a", 0, "A"), &[]);
    register(&mut host, pane, B, &running("job-b", 0, "B"), &[]);
    expect_probe(&mut host, A, &done("job-a", "A done"));
    expect_probe(&mut host, B, &failed("job-b", 5, "B", "boom"));
    tick(&mut host, 1);
    let run = progress(&mut host, pane, &["wait"]);
    assert_eq!(code(&run), "monitor-ambiguous");
}

/// REQ: PORT:ilium/src/progress_wait.rs::a_settled_wait_names_the_exact_clear_command_for_its_own_monitor
#[test]
fn port_060_a_wait_that_ends_on_a_task_error_names_the_clear_command_of_that_monitor() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(&mut host, &failed(JOB, 40, "Encoding", "encoder stopped"));
    let run = progress(&mut host, pane, &["wait", &id.to_string()]);
    assert_eq!(run.exit, Some(3), "{}", run.stdout);
    assert!(
        wait_next(&run).ends_with(&format!("`ilium progress clear --monitor-id {id}`.")),
        "{}",
        wait_next(&run)
    );
}
