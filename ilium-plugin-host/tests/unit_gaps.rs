//! Unit gaps of `ilium-plugin-host`: near-miss tracking, limit rules, the installer and the
//! `ilium plugin` command surface, checked against the documented rules
//! (reference-host-budgets.md, reference-errors-and-limits.md, reference-plugin-cli.md,
//! reference-install-and-update.md).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ilium_plugin_api::limits::Limits;
use ilium_plugin_api::manifest::CheckMode;
use ilium_plugin_api::{ErrorCode, PluginError, Tier};
use ilium_plugin_host::calls::CallClass;
use ilium_plugin_host::cli::{self, CliEnv};
use ilium_plugin_host::install::{
    install_with_limits, Consent, ConsentDecision, ConsentRequest, InstallStep, Installer,
};
use ilium_plugin_host::limits_rules::{
    client_request_timeout_ms, hook_handler_breach_verdict, reply_deadline_ms, retry_after_ms,
    soft_breach_verdict, ProcessQuota, ReplyKind, ReplyWaiter, SoftVerdict,
};
use ilium_plugin_host::registry::PackageStore;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::telemetry::NearMissTracker;

const FIVE_MINUTES_MS: u64 = 5 * 60 * 1_000;

/// The number of child processes one session may hold (reference-errors-and-limits.md,
/// `session_processes` and HST-497).
const CHILD_LIMIT: usize = 16;

// ---------------------------------------------------------------------------------------------
// Test doubles and helpers.
// ---------------------------------------------------------------------------------------------

/// A consent double that counts how often the grant review was asked and always refuses.
struct CountingConsent {
    asked: AtomicUsize,
}

impl CountingConsent {
    fn new() -> CountingConsent {
        CountingConsent {
            asked: AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

impl Consent for CountingConsent {
    fn decide(&self, _request: &ConsentRequest) -> ConsentDecision {
        self.asked.fetch_add(1, Ordering::SeqCst);
        ConsentDecision::Deny
    }
}

/// A registry fetcher that can never reach the network.
struct UnreachableFetcher;

impl Fetcher for UnreachableFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// An installer over a package store rooted at `root`.
fn installer_over(root: &Path) -> Installer {
    Installer::new(PackageStore {
        root: root.to_path_buf(),
    })
}

/// A CLI environment whose directories all live under `root`; the project directory exists
/// and starts empty.
fn cli_env(root: &Path) -> CliEnv {
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("the project directory can be created");
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        project,
        SystemTime::UNIX_EPOCH,
        true,
        Arc::new(UnreachableFetcher),
    )
}

/// The error code of a refused quota step; panics when the step was accepted.
fn refusal_code(outcome: Result<(), PluginError>) -> ErrorCode {
    match outcome {
        Ok(()) => panic!("expected a refusal, but the step was accepted"),
        Err(error) => error.code,
    }
}

/// Fills a quota to the session child limit, one spawn at a time.
fn fill_to_child_limit(quota: &mut ProcessQuota) {
    for _ in 0..CHILD_LIMIT {
        quota
            .spawn()
            .expect("spawns up to the child limit are accepted");
    }
}

// ---------------------------------------------------------------------------------------------
// NearMissTracker (HST-313): near misses at 80 percent of the deadline, degraded at five.
// ---------------------------------------------------------------------------------------------

#[test]
fn near_miss_tracker_starts_empty_and_not_degraded() {
    let tracker = NearMissTracker::new();
    assert_eq!(
        tracker.count_in_window(0),
        0,
        "an empty tracker has no near misses"
    );
    assert!(!tracker.is_degraded(0), "an empty tracker is not degraded");
}

#[test]
fn four_near_misses_inside_the_window_do_not_degrade_the_plugin() {
    let mut tracker = NearMissTracker::new();
    for index in 0..4u64 {
        assert!(tracker.record_call(index * 1_000, 100, 90));
    }
    assert!(
        !tracker.is_degraded(4_000),
        "four near misses are below the threshold of five (HST-313)"
    );
}

#[test]
fn five_near_misses_inside_the_window_degrade_the_plugin() {
    let mut tracker = NearMissTracker::new();
    for index in 0..5u64 {
        assert!(tracker.record_call(index * 1_000, 100, 90));
    }
    assert!(
        tracker.is_degraded(4_000),
        "exactly the threshold of five near misses degrades the plugin (HST-313)"
    );
}

#[test]
fn near_misses_older_than_the_window_do_not_count_toward_degradation() {
    let mut tracker = NearMissTracker::new();
    for index in 0..5u64 {
        tracker.record_call(index * 1_000, 100, 90);
    }
    assert!(
        !tracker.is_degraded(4_000 + FIVE_MINUTES_MS + 1),
        "near misses older than five minutes expire"
    );
}

#[test]
fn only_the_near_misses_still_inside_the_window_count_toward_degradation() {
    // Five near misses at 0..4 s; at 300.5 s the first one has left the window, so four remain.
    let mut tracker = NearMissTracker::new();
    for index in 0..5u64 {
        tracker.record_call(index * 1_000, 100, 90);
    }
    assert_eq!(tracker.count_in_window(300_500), 4);
    assert!(!tracker.is_degraded(300_500));
}

#[test]
fn call_just_below_eighty_percent_of_its_deadline_is_not_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, 1_000, 799));
    assert_eq!(tracker.count_in_window(0), 0);
}

#[test]
fn call_at_exactly_eighty_percent_of_its_deadline_is_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 1_000, 800));
    assert_eq!(tracker.count_in_window(0), 1);
}

#[test]
fn call_that_missed_its_deadline_is_not_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, 100, 101));
    assert_eq!(tracker.count_in_window(0), 0);
}

#[test]
fn call_that_ends_exactly_at_its_deadline_is_in_time_and_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 100, 100));
    assert_eq!(tracker.count_in_window(0), 1);
}

#[test]
fn zero_deadline_and_zero_duration_count_as_in_time_and_at_least_eighty_percent() {
    // Literal reading of HST-313: 0 ms ends in time (0 <= 0) and is at least 80 % of 0 ms.
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 0, 0));
}

#[test]
fn repeated_calls_at_the_same_instant_are_all_counted() {
    let mut tracker = NearMissTracker::new();
    for _ in 0..3 {
        assert!(tracker.record_call(1_000, 100, 95));
    }
    assert_eq!(tracker.count_in_window(1_000), 3);
}

#[test]
fn count_in_window_ignores_entries_recorded_after_now() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(5_000, 100, 100));
    assert_eq!(
        tracker.count_in_window(4_000),
        0,
        "a near miss in the future is not counted"
    );
}

#[test]
fn count_in_window_drops_entries_older_than_five_minutes() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 100, 100));
    assert_eq!(tracker.count_in_window(1), 1);
    assert_eq!(tracker.count_in_window(FIVE_MINUTES_MS + 1), 0);
}

#[test]
fn near_miss_window_arithmetic_handles_zero_and_maximum_timestamps() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 100, 100));
    assert_eq!(
        tracker.count_in_window(0),
        1,
        "now_ms = 0 counts the entry at 0"
    );
    assert_eq!(
        tracker.count_in_window(u64::MAX),
        0,
        "now_ms = u64::MAX must not overflow and must drop the old entry"
    );
}

#[test]
fn near_miss_check_at_maximum_values_does_not_overflow() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(u64::MAX, u64::MAX, u64::MAX));
    assert_eq!(tracker.count_in_window(u64::MAX), 1);
    assert!(!tracker.is_degraded(u64::MAX));
}

#[test]
fn half_of_a_huge_deadline_is_not_a_near_miss_without_overflow() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, u64::MAX, u64::MAX / 2));
    assert_eq!(tracker.count_in_window(0), 0);
}

// ---------------------------------------------------------------------------------------------
// ProcessQuota (API-070, HST-497): live children are counted and refused at the limit.
// ---------------------------------------------------------------------------------------------

#[test]
fn new_process_quota_has_no_live_children() {
    assert_eq!(ProcessQuota::new().live(), 0);
}

#[test]
fn spawn_below_the_limit_increments_the_live_count() {
    let mut quota = ProcessQuota::new();
    assert!(quota.spawn().is_ok());
    assert_eq!(quota.live(), 1);
}

#[test]
fn spawn_at_the_limit_is_refused_with_quota_exceeded_not_a_panic() {
    let mut quota = ProcessQuota::new();
    fill_to_child_limit(&mut quota);
    assert_eq!(refusal_code(quota.spawn()), ErrorCode::QuotaExceeded);
    assert_eq!(
        quota.live(),
        CHILD_LIMIT,
        "a refused spawn does not change the count"
    );
}

#[test]
fn run_at_the_limit_is_refused_with_busy_as_process_run_requires() {
    // HST-497: a full session makes `process.run` return `busy`.
    let mut quota = ProcessQuota::new();
    fill_to_child_limit(&mut quota);
    assert_eq!(refusal_code(quota.run()), ErrorCode::Busy);
    assert_eq!(quota.live(), CHILD_LIMIT);
}

#[test]
fn short_runs_complete_without_staying_counted() {
    let mut quota = ProcessQuota::new();
    for _ in 0..5 {
        assert!(quota.run().is_ok());
    }
    assert_eq!(quota.live(), 0);
}

#[test]
fn exit_one_decrements_the_live_count_and_never_goes_below_zero() {
    let mut quota = ProcessQuota::new();
    quota.exit_one();
    assert_eq!(
        quota.live(),
        0,
        "exiting with no live child keeps the count at zero"
    );
    assert!(quota.spawn().is_ok());
    quota.exit_one();
    assert_eq!(quota.live(), 0);
    quota.exit_one();
    assert_eq!(quota.live(), 0);
}

#[test]
fn exit_one_frees_a_slot_for_a_new_spawn_at_the_limit() {
    let mut quota = ProcessQuota::new();
    fill_to_child_limit(&mut quota);
    quota.exit_one();
    assert_eq!(quota.live(), CHILD_LIMIT - 1);
    assert!(quota.spawn().is_ok());
    assert_eq!(quota.live(), CHILD_LIMIT);
}

#[test]
fn concurrent_spawns_and_exits_keep_the_live_count_consistent() {
    let quota = Arc::new(Mutex::new(ProcessQuota::new()));
    let accepted = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));

    std::thread::scope(|scope| {
        for _ in 0..4 {
            let quota = Arc::clone(&quota);
            let accepted = Arc::clone(&accepted);
            let refused = Arc::clone(&refused);
            scope.spawn(move || {
                for _ in 0..8 {
                    let outcome = quota
                        .lock()
                        .expect("the quota lock is not poisoned")
                        .spawn();
                    match outcome {
                        Ok(()) => {
                            accepted.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(_) => {
                            refused.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            });
        }
    });

    assert_eq!(
        accepted.load(Ordering::SeqCst),
        CHILD_LIMIT,
        "exactly the child limit is accepted across threads"
    );
    assert_eq!(refused.load(Ordering::SeqCst), 4 * 8 - CHILD_LIMIT);
    assert_eq!(
        quota.lock().expect("the quota lock is not poisoned").live(),
        CHILD_LIMIT
    );

    std::thread::scope(|scope| {
        for _ in 0..4 {
            let quota = Arc::clone(&quota);
            scope.spawn(move || {
                for _ in 0..8 {
                    quota
                        .lock()
                        .expect("the quota lock is not poisoned")
                        .exit_one();
                }
            });
        }
    });

    assert_eq!(
        quota.lock().expect("the quota lock is not poisoned").live(),
        0,
        "32 exits for 16 live children leave zero, never a wrapped count"
    );
}

// ---------------------------------------------------------------------------------------------
// ReplyWaiter (HST-499): a reply reports `timed-out` once its deadline has passed.
// ---------------------------------------------------------------------------------------------

#[test]
fn reply_before_its_deadline_is_pending_not_timed_out() {
    let mut waiter = ReplyWaiter::new(ReplyKind::Command);
    assert_eq!(
        waiter.result(),
        None,
        "no time has passed, so the reply is pending"
    );
    waiter.advance_ms(29_999);
    assert_eq!(
        waiter.result(),
        None,
        "29.999 s into a 30 s deadline is still pending"
    );
}

#[test]
fn advance_past_the_deadline_reports_timed_out() {
    let mut waiter = ReplyWaiter::new(ReplyKind::Command);
    waiter.advance_ms(30_001);
    let error = waiter.result().expect("the deadline has passed");
    assert_eq!(error.code, ErrorCode::TimedOut);
    assert_eq!(error.retry_after_ms, None);
}

#[test]
fn advance_by_zero_is_a_no_op() {
    let mut waiter = ReplyWaiter::new(ReplyKind::JobCancelGrace);
    waiter.advance_ms(999);
    waiter.advance_ms(0);
    assert_eq!(
        waiter.result(),
        None,
        "a zero advance does not move the deadline"
    );
    waiter.advance_ms(1);
    assert_eq!(
        waiter.result().map(|error| error.code),
        Some(ErrorCode::TimedOut),
        "the next millisecond reaches the 1 s grace"
    );
}

#[test]
fn elapsed_time_accumulates_across_advances() {
    let mut waiter = ReplyWaiter::new(ReplyKind::Command);
    waiter.advance_ms(10_000);
    waiter.advance_ms(10_000);
    assert_eq!(waiter.result(), None);
    waiter.advance_ms(10_001);
    assert!(waiter.result().is_some());
}

// ---------------------------------------------------------------------------------------------
// Verdict and deadline rules (HST-189, HST-190, HST-496, HST-498, HST-499).
// ---------------------------------------------------------------------------------------------

#[test]
fn retry_after_rounds_up_to_the_next_ten_milliseconds() {
    assert_eq!(retry_after_ms(301), 310);
    assert_eq!(
        retry_after_ms(310),
        310,
        "a value already on a 10 ms step is kept"
    );
    assert_eq!(retry_after_ms(311), 320);
}

#[test]
fn retry_after_never_goes_below_one_hundred_milliseconds() {
    assert_eq!(
        retry_after_ms(0),
        100,
        "a zero or missing wait gives the 100 ms floor"
    );
    assert_eq!(
        retry_after_ms(-40),
        100,
        "a negative wait gives the 100 ms floor"
    );
    assert_eq!(retry_after_ms(i64::MIN), 100);
    assert_eq!(retry_after_ms(100), 100, "exactly the floor is kept");
    assert_eq!(
        retry_after_ms(101),
        110,
        "one above the floor rounds up to the next step"
    );
}

#[test]
fn retry_after_saturates_for_overflow_sized_input() {
    let result = retry_after_ms(i64::MAX);
    assert!(
        result >= i64::MAX - 10,
        "rounding i64::MAX up must saturate, not wrap to a small or negative value"
    );
}

#[test]
fn reply_deadlines_match_the_manual_for_every_kind() {
    assert_eq!(reply_deadline_ms(ReplyKind::Command), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::Service), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::SessionsSend), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::InferenceCompletion), 120_000);
    assert_eq!(reply_deadline_ms(ReplyKind::HttpApiRespond), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::ClientRequestDefault), 5_000);
    assert_eq!(reply_deadline_ms(ReplyKind::JobCancelGrace), 1_000);
}

#[test]
fn client_request_timeout_value_exactly_at_the_maximum_is_not_clamped() {
    assert_eq!(client_request_timeout_ms(30_000), 30_000);
}

#[test]
fn client_request_timeout_value_one_above_the_maximum_is_clamped_down() {
    assert_eq!(client_request_timeout_ms(30_001), 30_000);
}

#[test]
fn client_request_timeout_zero_and_missing_values_clamp_up_to_one_millisecond() {
    assert_eq!(client_request_timeout_ms(0), 1);
    assert_eq!(client_request_timeout_ms(1), 1);
}

#[test]
fn client_request_timeout_overflow_sized_input_saturates_at_the_maximum() {
    assert_eq!(client_request_timeout_ms(u32::MAX), 30_000);
}

#[test]
fn annotate_breach_streak_is_tolerated_below_three_and_traps_at_three() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 2, 5),
        SoftVerdict::Tolerated
    );
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 3, 5),
        SoftVerdict::TrapCounted
    );
}

#[test]
fn non_frame_classes_trap_at_a_streak_of_three() {
    let classes = [
        CallClass::Event,
        CallClass::Command,
        CallClass::Render,
        CallClass::Annotate,
        CallClass::Detector,
        CallClass::SettingsValidate,
    ];
    for class in classes {
        assert_eq!(soft_breach_verdict(class, 2, 5), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(class, 3, 5), SoftVerdict::TrapCounted);
    }
}

#[test]
fn scene_frame_breaches_trap_only_at_thirty_in_a_row() {
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 29, 1),
        SoftVerdict::Tolerated
    );
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 30, 1),
        SoftVerdict::TrapCounted
    );
}

#[test]
fn breach_inside_the_hundred_millisecond_grace_is_tolerated_below_the_streak() {
    // Annotate has a 10 ms soft deadline, so a run is inside the grace up to 110 ms.
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 109),
        SoftVerdict::Tolerated
    );
}

#[test]
fn breach_past_the_hundred_millisecond_grace_traps_at_once() {
    // A first breach that runs past the 100 ms grace is a trap without waiting for the streak.
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 111),
        SoftVerdict::TrapCounted
    );
}

#[test]
fn soft_breach_with_zero_prior_breaches_and_zero_elapsed_is_tolerated() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 0, 0),
        SoftVerdict::Tolerated
    );
}

#[test]
fn soft_breach_overflow_sized_inputs_trap_without_wrapping() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, u32::MAX, u64::MAX),
        SoftVerdict::TrapCounted
    );
}

#[test]
fn hook_handler_breaches_are_tolerated_until_the_third() {
    assert_eq!(hook_handler_breach_verdict(0), SoftVerdict::Tolerated);
    assert_eq!(hook_handler_breach_verdict(1), SoftVerdict::Tolerated);
    assert_eq!(hook_handler_breach_verdict(2), SoftVerdict::Tolerated);
}

#[test]
fn hook_handler_third_breach_disables_the_handler_without_a_failure() {
    assert_eq!(
        hook_handler_breach_verdict(3),
        SoftVerdict::DisableHandlerNoFailure
    );
    assert_eq!(
        hook_handler_breach_verdict(u32::MAX),
        SoftVerdict::DisableHandlerNoFailure,
        "overflow-sized breach counts stay disabled, not wrapped"
    );
}

#[test]
fn hook_handler_breach_never_counts_as_a_trap() {
    for breaches in 0..=10 {
        assert_ne!(
            hook_handler_breach_verdict(breaches),
            SoftVerdict::TrapCounted
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Installer (reference-install-and-update.md, reference-check-and-lint-rules.md).
// These use packages that cannot be valid, so the tests check the rules that hold for every
// package: validation happens before consent, check writes nothing, and an interrupted
// install leaves nothing instantiated.
// ---------------------------------------------------------------------------------------------

#[test]
fn install_of_an_unreadable_package_fails_before_the_grant_review() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let installer = installer_over(root.path());
    let consent = CountingConsent::new();
    let outcome = installer.install(&[], &consent);
    assert!(outcome.is_err(), "an empty package must not install");
    assert_eq!(
        consent.asked(),
        0,
        "acquire, fetch and validate run before the review (PKG-270..272)"
    );
}

#[test]
fn check_of_an_unreadable_package_reports_diagnostics_and_a_stage_number() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let installer = installer_over(root.path());
    let report = installer.check(&[], CheckMode::Check);
    assert!(
        !report.diagnostics.is_empty(),
        "an empty package has findings"
    );
    assert!(
        (1..=12).contains(&report.last_stage),
        "the last stage is one of the twelve check stages, got {}",
        report.last_stage
    );
}

#[test]
fn check_never_writes_to_the_package_store() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let installer = installer_over(root.path());
    let _report = installer.check(&[], CheckMode::Check);
    let entries = std::fs::read_dir(root.path())
        .expect("the store root exists")
        .count();
    assert_eq!(entries, 0, "check runs the stages without changing disk");
}

#[test]
fn install_interrupted_after_staging_instantiates_nothing_and_recovers() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let installer = installer_over(root.path());
    let consent = CountingConsent::new();
    let interrupted = installer.install_crashing_after(&[], &consent, InstallStep::Stage);
    assert!(interrupted.is_err(), "a simulated crash ends the install");
    assert_eq!(
        installer.instantiated_count(),
        0,
        "no component is instantiated before notify (PKG-151, PKG-272)"
    );
    installer
        .recover_at_start()
        .expect("recovery at start after an interrupted install succeeds");
    assert!(
        installer.install(&[], &consent).is_err(),
        "retrying the same failing install stays a clean failure, with no partial state"
    );
}

#[test]
fn install_with_limits_reports_not_installed_for_a_missing_package_directory() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let missing = root.path().join("no-such-package");
    for tier in [Tier::T1, Tier::T2, Tier::T3] {
        let report = install_with_limits(&missing, tier, &Limits::default());
        assert!(
            !report.installed,
            "a missing package directory must not install"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// `ilium plugin` command surface (reference-plugin-cli.md DEV-007..DEV-010, DEV-040).
// ---------------------------------------------------------------------------------------------

#[test]
fn unknown_subcommand_is_a_usage_error_with_exit_code_two() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let env = cli_env(root.path());
    let output = cli::run(&["no-such-command".to_owned()], &env);
    assert_eq!(
        output.exit_code, 2,
        "a wrong argument is a usage error (DEV-010)"
    );
}

#[test]
fn missing_required_argument_is_a_usage_error_and_writes_no_files() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let env = cli_env(root.path());
    let project = root.path().join("project");
    let args = vec!["new".to_owned(), "--json".to_owned()];
    let output = cli::run(&args, &env);
    assert_eq!(
        output.exit_code, 2,
        "`new` without --id is a usage error (DEV-040, DEV-010)"
    );
    let entries = std::fs::read_dir(&project)
        .expect("the project directory exists")
        .count();
    assert_eq!(
        entries, 0,
        "a usage error returns before any file is written (DEV-010)"
    );
}

#[test]
fn json_output_stays_one_json_object_per_line_on_a_usage_error() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let env = cli_env(root.path());
    let args = vec!["new".to_owned(), "--json".to_owned()];
    let output = cli::run(&args, &env);
    for line in output
        .stdout_raw
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("each stdout line is one complete JSON value");
        assert!(
            value.is_object(),
            "each stdout line is one JSON object: {line}"
        );
    }
}

#[test]
fn help_flag_exits_zero_with_usage_on_standard_output() {
    let root = tempfile::tempdir().expect("a temporary test directory");
    let env = cli_env(root.path());
    let args = vec!["doctor".to_owned(), "--help".to_owned()];
    let output = cli::run(&args, &env);
    assert_eq!(
        output.exit_code, 0,
        "--help exits with 0 (reference-plugin-cli.md switch table)"
    );
    assert!(
        !output.stdout_text.trim().is_empty(),
        "--help prints usage to standard output"
    );
}
