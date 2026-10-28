//! reference-sandbox.md: minimum kernel, the doctor check and the openat2 degradation rule
//! (SEC-442 .. SEC-444). Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-442
#[test]
fn sec_442_the_sandbox_page_states_the_minimum_kernel_and_the_per_start_prerequisite_checks() {
    assert_page_has(
        "reference-sandbox.md",
        &[
            "Ilium MUST document a minimum Linux kernel for T2 and T3 plugins",
            "each prerequisite of [SEC-055] separately",
            "unprivileged user namespaces, cgroup v2 delegation, seccomp filter support and `openat2` availability",
        ],
    );
}

/// REQ: SEC-443
#[test]
fn sec_443_the_sandbox_page_states_the_doctor_check_line_and_exit_rule() {
    assert_page_has(
        "reference-sandbox.md",
        &[
            "`ilium plugin doctor --sandbox` MUST print one JSONL `check` line per prerequisite",
            "It MUST exit 0 when all pass and 1 otherwise",
            "it MUST NOT change any system setting",
        ],
    );
}

/// REQ: SEC-444
#[test]
fn sec_444_the_sandbox_page_states_that_a_missing_openat2_is_degraded_not_unavailable() {
    assert_page_has(
        "reference-sandbox.md",
        &[
            "A kernel without `openat2` MUST NOT make `sandbox.unavailable` by itself",
            "`openat2` as `ok: false, degraded: true`",
            "A missing user namespace, cgroup v2 or seccomp prerequisite MUST give `sandbox.unavailable`",
        ],
    );
}

/// REQ: SEC-442
#[test]
fn sec_442_each_sandbox_prerequisite_is_checked_separately_at_start() {
    assert_page_has(
        "reference-sandbox.md",
        &["MUST check at server start, once per start, each prerequisite of [SEC-055] separately"],
    );
}

/// REQ: SEC-443
#[test]
fn sec_443_doctor_prints_one_check_line_per_prerequisite() {
    assert_page_has(
        "reference-sandbox.md",
        &["MUST print one JSONL `check` line per prerequisite with `name`, `ok`"],
    );
}

/// REQ: SEC-444
#[test]
fn sec_444_missing_openat2_is_reported_as_degraded_not_unavailable() {
    assert_page_has(
        "reference-sandbox.md",
        &["`doctor` reports `openat2` as `ok: false, degraded: true`"],
    );
}
