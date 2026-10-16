//! Baseline scenarios of src/docs/how-it-works.md: F-079 ordered event journal and F-087
//! background worker banks (`ilium-execution`: bounded banks for finite jobs and services).

use crate::common::*;

/// REQ: F-079, COR-001
#[test]
fn f_079_journal_sequence_numbers_start_at_one_and_grow_by_one() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        type_text(kernel, pane, "a");
        type_text(kernel, pane, "b");
        let sequences = kernel.journal_sequences();
        assert!(!sequences.is_empty());
        let expected: Vec<u64> = (1..=sequences.len() as u64).collect();
        assert_eq!(sequences, expected);
    });
}

/// REQ: F-079, COR-001
#[test]
fn f_079_events_of_one_pane_keep_the_order_of_the_actions_that_caused_them() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["sh", "-c", "exit 0"]);
        kernel.settle();
        close(kernel, pane);
        let names = kernel.event_names_for(pane);
        let created = names.iter().position(|n| n == "pane-created").expect("pane-created");
        let exited = names.iter().position(|n| n == "pane-exited").expect("pane-exited");
        let removed = names.iter().position(|n| n == "node-removed").expect("node-removed");
        assert!(created < exited && exited < removed, "{names:?}");
    });
}

/// REQ: F-079, COR-001
#[test]
fn f_079_a_second_world_replays_the_same_event_names_for_the_same_actions() {
    let run = || {
        let kernel = world(ilium_plugin_host::lifecycle::StartMode::NoPlugins);
        let pane = terminal(&kernel, None, "t", &["cat"]);
        rename(&kernel, pane, "renamed");
        kernel.event_names_for(pane)
    };
    assert_eq!(run(), run());
}

/// REQ: F-087, COR-001
#[test]
fn f_087_a_finite_job_runs_to_completion_and_reports_its_result() {
    in_both_modes(|kernel| {
        let ticket = kernel.run_finite_job("bank-a", Box::new(|| 7 * 2));
        kernel.settle();
        assert_eq!(kernel.job_result(ticket), Some(14));
    });
}

/// REQ: F-087, COR-001
#[test]
fn f_087_jobs_beyond_the_bank_size_wait_and_never_exceed_the_worker_count() {
    in_both_modes(|kernel| {
        let workers = kernel.bank_worker_count("bank-a");
        let tickets: Vec<_> = (0..workers * 3).map(|n| kernel.run_finite_job("bank-a", Box::new(move || n as u64))).collect();
        kernel.settle();
        assert!(kernel.bank_peak_concurrency("bank-a") <= workers);
        assert!(tickets.iter().all(|t| kernel.job_result(*t).is_some()));
    });
}

/// REQ: F-087, COR-001
#[test]
fn f_087_a_panicking_job_does_not_stop_the_bank() {
    in_both_modes(|kernel| {
        let bad = kernel.run_failing_job("bank-a");
        let good = kernel.run_finite_job("bank-a", Box::new(|| 2 * 2));
        kernel.settle();
        assert_eq!(kernel.job_result(bad), None);
        assert_eq!(kernel.job_result(good), Some(4));
    });
}
