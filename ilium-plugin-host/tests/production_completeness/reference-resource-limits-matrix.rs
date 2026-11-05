/// REQ: PDZ-016
#[test]
fn pdz_016_every_limit_has_one_row_with_default_key_cap_enforcer_and_error_code() {
    // Observable: Parse the matrix table on the published page: each of memory, cpu, time, stack, processes, fds and disk appears once, and every row has a non-empty value in all six columns.
    panic!(
        "PDZ-016 not implemented: Every limit MUST have exactly one row giving its default, manifest key, hard host cap, enforcement mechanism and the exact error code (`trap.memory`, `trap.deadline`, `trap.stack`, `quota-exceeded`, `helper.unstable` or another named code)."
    );
}

/// REQ: PDZ-017
#[test]
fn pdz_017_wasm_stack_exhaustion_traps_as_one_failure() {
    // Observable: Call an export that recurses without bound: the result is a trap with code `trap.stack` and `ilium plugin health <id> --json` shows `failures_in_window: 1`.
    panic!(
        "PDZ-017 not implemented: Exhausting the Wasm stack MUST trap with the code `trap.stack` and MUST count as one failure in the three-failure window."
    );
}

/// REQ: PDZ-018
#[test]
fn pdz_018_pids_max_returns_limit_processes_without_killing_helper() {
    // Observable: Spawn child processes until the limit is reached: the failing spawn returns `limit.processes`, the helper pid is unchanged and the plugin keeps answering calls.
    panic!(
        "PDZ-018 not implemented: Reaching `pids.max` MUST return the error `limit.processes` to the plugin on the spawning call and MUST NOT kill the helper."
    );
}

/// REQ: PDZ-019
#[test]
fn pdz_019_limit_breach_kill_is_logged_with_limit_name_and_observed_value() {
    // Observable: Allocate past the memory limit: the log line for the kill contains `limit: memory` and an `observed` byte count at or above the configured limit.
    panic!(
        "PDZ-019 not implemented: A limit breach that kills the helper MUST be logged with the limit name and the observed value at the time of the breach."
    );
}

/// REQ: PDZ-020
#[test]
fn pdz_020_each_row_failure_window_column_matches_host_behaviour() {
    // Observable: For the `fds` row, which states that the breach does not count: open file descriptors past the limit and check that `failures_in_window` stays 0.
    panic!(
        "PDZ-020 not implemented: Each row MUST state whether a breach counts toward the three-failure window, and the host behaviour MUST match that column."
    );
}

/// REQ: PDZ-021
#[test]
fn pdz_021_matrix_generated_from_host_constants_with_drift_test() {
    // Observable: Change the default memory limit constant in the host source without regenerating the page and run the manual tests: the drift test fails naming the `memory` row.
    panic!(
        "PDZ-021 not implemented: The matrix MUST be generated from the same source as the host constants, and a test MUST fail when the published table and the constants differ."
    );
}
