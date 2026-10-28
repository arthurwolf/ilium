//! Crash-loop guard levels and the safe-mode footer (LIF-978..981).
//!
//! The manual text of each row is checked against its page. The behaviour itself needs a rig that
//! can plant crash marks and start a server at a chosen safe-mode level, and records its exit
//! status; `OpsRig` has no such call yet, so those tests name the missing item in `API_GAPS.md`.

use crate::common::*;
use ilium_plugin_host::lifecycle::{CrashLoopGuard, GuardDecision, SafeMode};

/// A guard with `count` start crashes of `level`, one every 60 seconds ending at `end`.
fn guard_with(count: u64, level: SafeMode, end: u64) -> CrashLoopGuard {
    let mut guard = CrashLoopGuard::default();
    for index in 0..count {
        guard.record_crash(end - 60 * (count - 1 - index), level);
    }
    guard
}

/// REQ: LIF-978
#[test]
fn lif_978_third_party_safe_mode_crash_storm_moves_the_next_start_to_all_optional_as_stated() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("Three crashes during start within 5 minutes while the start runs in `third-party` safe mode MUST cause the next start to use `all-optional` safe mode"),
        "the requirement must name the third-party to all-optional step"
    );
}

/// REQ: LIF-978
#[test]
fn lif_978_the_guard_levels_run_in_the_order_third_party_then_all_optional_then_no_plugins() {
    let page = manual_page("user-safe-mode-and-troubleshooting");
    assert!(
        page.contains("The levels run in this order: `third-party`, then `all-optional`, then `--no-plugins`"),
        "the guard levels must run in the stated order"
    );
}

/// REQ: LIF-978
#[test]
fn lif_978_three_crash_marks_in_third_party_mode_step_the_next_start_to_all_optional() {
    let now = 10_000;
    let two = guard_with(2, SafeMode::ThirdParty, now);
    assert_eq!(two.decide(now, SafeMode::ThirdParty), GuardDecision::Start(SafeMode::ThirdParty));
    let three = guard_with(3, SafeMode::ThirdParty, now);
    assert_eq!(three.decide(now, SafeMode::ThirdParty), GuardDecision::Start(SafeMode::AllOptional));
    // Three crashes spread over more than five minutes do not step the guard.
    let mut spread = CrashLoopGuard::default();
    for at in [now - 900, now - 500, now] {
        spread.record_crash(at, SafeMode::ThirdParty);
    }
    assert_eq!(spread.decide(now, SafeMode::ThirdParty), GuardDecision::Start(SafeMode::ThirdParty));
}

/// REQ: LIF-979
#[test]
fn lif_979_all_optional_crash_storm_moves_the_next_start_to_no_plugins_and_opens_recovery() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("Three crashes during start within 5 minutes while the start runs in `all-optional` safe mode MUST cause the next start to use `--no-plugins`"),
        "the requirement must name the all-optional to --no-plugins step"
    );
    assert!(
        page.contains("and the start MUST open the recovery page as its first screen."),
        "the start after that step must open the recovery page first"
    );
}

/// REQ: LIF-979
#[test]
fn lif_979_the_no_plugins_level_is_the_last_level_of_the_guard_and_names_the_kernel_only() {
    let page = manual_page("user-safe-mode-and-troubleshooting");
    assert!(
        page.contains("`--no-plugins` (no plugin host, kernel only)"),
        "--no-plugins is the last guard level and runs the kernel only"
    );
}

/// REQ: LIF-979
#[test]
fn lif_979_three_crashes_in_all_optional_mode_start_the_next_run_with_no_plugins() {
    let now = 10_000;
    let guard = guard_with(3, SafeMode::AllOptional, now);
    assert_eq!(guard.decide(now, SafeMode::AllOptional), GuardDecision::Start(SafeMode::NoPlugins));
    assert_eq!(
        CrashLoopGuard::footer(SafeMode::NoPlugins),
        Some("Safe mode: --no-plugins, kernel only."),
        "the recovery start shows its level"
    );
}

/// REQ: LIF-980
#[test]
fn lif_980_exhausted_guard_exits_non_zero_with_one_crash_loop_exhausted_line() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("Ilium MUST exit with a non-zero status and print one line with the code `crash-loop.exhausted`"),
        "the exhausted guard must exit non-zero and print one crash-loop.exhausted line"
    );
}

/// REQ: LIF-980
#[test]
fn lif_980_every_later_start_uses_no_plugins_until_the_marks_clear_under_lif_294() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("until the marks clear under LIF-294 every later start MUST use `--no-plugins`"),
        "later starts stay on --no-plugins until LIF-294 clears the marks"
    );
}

/// REQ: LIF-980
#[test]
fn lif_980_three_crashes_under_no_plugins_stop_automatic_restarts() {
    let now = 10_000;
    let mut guard = guard_with(3, SafeMode::NoPlugins, now);
    assert_eq!(guard.decide(now, SafeMode::NoPlugins), GuardDecision::Exhausted);
    // Every later start stays exhausted until the marks clear (LIF-294).
    assert_eq!(guard.decide(now + 60, SafeMode::NoPlugins), GuardDecision::Exhausted);
    guard.clear();
    assert_eq!(guard.decide(now + 60, SafeMode::NoPlugins), GuardDecision::Start(SafeMode::NoPlugins));
}

/// REQ: LIF-981
#[test]
fn lif_981_every_start_shows_its_safe_mode_level_on_the_footer_banner() {
    let page = manual_page("reference-requirements-lifecycle-ops");
    assert!(
        page.contains("Every start MUST show the safe-mode level it runs in on the footer banner (`third-party`, `all-optional` or `--no-plugins`)"),
        "every start must show its level on the footer banner"
    );
}

/// REQ: LIF-981
#[test]
fn lif_981_the_footer_text_for_third_party_mode_is_stated_on_the_safe_mode_page() {
    let page = manual_page("user-safe-mode-and-troubleshooting");
    assert!(
        page.contains("The footer shows \"Safe mode: third-party plugins are off.\""),
        "the safe-mode page must state the footer text it shows"
    );
}

/// REQ: LIF-981
#[test]
fn lif_981_a_level_change_does_not_clear_the_crash_marks() {
    let now = 10_000;
    let guard = guard_with(3, SafeMode::ThirdParty, now);
    assert_eq!(guard.decide(now, SafeMode::ThirdParty), GuardDecision::Start(SafeMode::AllOptional));
    // The level moved on, but the third-party marks are still there and still count.
    assert_eq!(guard.marks_in_window(now, SafeMode::ThirdParty), 3);
    assert_eq!(guard.decide(now + 30, SafeMode::ThirdParty), GuardDecision::Start(SafeMode::AllOptional));
    assert_eq!(
        CrashLoopGuard::footer(SafeMode::ThirdParty),
        Some("Safe mode: third-party plugins are off.")
    );
}
