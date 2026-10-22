//! Limit rules that are pure functions of counts and times (reference-errors-and-limits.md):
//! retry hints, the process quota, reply deadlines and soft-deadline verdicts.

use ilium_plugin_api::{ErrorCode, PluginError};

use crate::calls::CallClass;

/// The CPU quota retry hint for its signed balance in milliseconds (HST-496).
/// Extreme deficits saturate to the largest representable whole 10 ms interval.
pub fn retry_after_ms(balance_ms: i64) -> i64 {
    let wait = ((-i128::from(balance_ms) * 2 + 9).div_euclid(10) * 10).max(100);
    wait.min(i128::from(i64::MAX / 10 * 10)) as i64
}

/// The session budget shared by run and spawn children (HST-497).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessQuota {
    live: usize,
    held: usize,
}

impl ProcessQuota {
    /// An empty quota at the default limit.
    pub fn new() -> ProcessQuota {
        Self::default()
    }

    /// Spawns a long-lived child; `quota-exceeded` at the limit.
    pub fn spawn(&mut self) -> Result<(), PluginError> {
        if self.live() >= 16 {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "Session child process quota is full",
            ));
        }
        self.live += 1;
        Ok(())
    }

    /// Runs a short child to completion; `quota-exceeded` at the limit.
    pub fn run(&mut self) -> Result<(), PluginError> {
        self.hold_run()?;
        // This synchronous model completes the short child before returning.
        self.held -= 1;
        Ok(())
    }

    /// Runs a child and keeps it counted until `exit_one`.
    pub fn hold_run(&mut self) -> Result<(), PluginError> {
        if self.live() >= 16 {
            return Err(PluginError::new(
                ErrorCode::Busy,
                "Session child process quota is full",
            ));
        }
        self.held += 1;
        Ok(())
    }

    /// One child exited.
    pub fn exit_one(&mut self) {
        // The API identifies no child; either category releases the same shared slot.
        if self.held != 0 {
            self.held -= 1;
        } else {
            self.live = self.live.saturating_sub(1);
        }
    }

    /// The live child count.
    pub fn live(&self) -> usize {
        self.live + self.held
    }
}

/// The kinds of reply a plugin owes the host, each with its deadline (reference-errors-and-limits.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplyKind {
    /// A command handler reply.
    Command,
    /// A service call reply.
    Service,
    /// An inference completion.
    InferenceCompletion,
    /// An HTTP API respond.
    HttpApiRespond,
    /// A client request with the default timeout.
    ClientRequestDefault,
    /// The grace after a job cancel.
    JobCancelGrace,
    /// A `sessions.send` reply.
    SessionsSend,
}

/// The deadline of a reply kind in milliseconds (reference-errors-and-limits.md).
pub fn reply_deadline_ms(kind: ReplyKind) -> u64 {
    match kind {
        ReplyKind::Command
        | ReplyKind::Service
        | ReplyKind::HttpApiRespond
        | ReplyKind::SessionsSend => 30_000,
        ReplyKind::InferenceCompletion => 120_000,
        ReplyKind::ClientRequestDefault => 5_000,
        ReplyKind::JobCancelGrace => 1_000,
    }
}

/// Clamps a requested client-request timeout to the allowed range (reference-errors-and-limits.md).
pub fn client_request_timeout_ms(requested_ms: u32) -> u32 {
    requested_ms.clamp(1, 30_000)
}

/// Waits for one reply and reports `timed-out` when its deadline passes (reference-errors-and-limits.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyWaiter {
    kind: ReplyKind,
    elapsed_ms: u64,
}

impl ReplyWaiter {
    /// A waiter for one reply kind.
    pub fn new(kind: ReplyKind) -> ReplyWaiter {
        Self {
            kind,
            elapsed_ms: 0,
        }
    }

    /// Time passed.
    pub fn advance_ms(&mut self, ms: u64) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(ms);
    }

    /// The error to report once the deadline passed, else `None`.
    pub fn result(&self) -> Option<PluginError> {
        (self.elapsed_ms >= reply_deadline_ms(self.kind)).then(|| {
            PluginError::new(ErrorCode::TimedOut, "Plugin reply deadline reached")
        })
    }
}

/// What a soft-deadline breach leads to (reference-errors-and-limits.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoftVerdict {
    /// The late call is tolerated.
    Tolerated,
    /// The late call counts as a trap.
    TrapCounted,
    /// The hook handler is disabled without counting a failure.
    DisableHandlerNoFailure,
}

/// The verdict for the `breaches`-th soft breach of `class`, `overshoot_ms` past its deadline
/// (reference-errors-and-limits.md).
pub fn soft_breach_verdict(class: CallClass, breaches: u32, overshoot_ms: u64) -> SoftVerdict {
    let streak = crate::calls::streak_limit(class).unwrap_or(3);
    if breaches >= streak || overshoot_ms >= crate::calls::SOFT_GRACE_MS {
        SoftVerdict::TrapCounted
    } else {
        SoftVerdict::Tolerated
    }
}

/// The verdict for the `breaches`-th breach of an extension hook handler (reference-errors-and-limits.md).
pub fn hook_handler_breach_verdict(breaches: u32) -> SoftVerdict {
    if breaches >= 3 {
        SoftVerdict::DisableHandlerNoFailure
    } else {
        SoftVerdict::Tolerated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        client_request_timeout_ms, hook_handler_breach_verdict, reply_deadline_ms, retry_after_ms,
        soft_breach_verdict, ProcessQuota, ReplyKind, ReplyWaiter, SoftVerdict,
    };
    use crate::calls::CallClass;
    use ilium_plugin_api::{ErrorCode, PluginError};

    /// The error code of a refused quota step; panics when the step was accepted.
    fn refusal_code(outcome: Result<(), PluginError>) -> ErrorCode {
        match outcome {
            Ok(()) => panic!("expected a quota refusal, but the step was accepted"),
            Err(error) => error.code,
        }
    }

    /// REQ: HST-496
    #[test]
    fn retry_after_ms_rounds_the_wait_up_to_the_next_ten_milliseconds() {
        assert_eq!(retry_after_ms(301), 310);
        assert_eq!(retry_after_ms(310), 310);
        assert_eq!(retry_after_ms(311), 320);
        assert_eq!(retry_after_ms(1_001), 1_010);
    }

    /// REQ: HST-496
    #[test]
    fn retry_after_ms_matches_documented_balance_examples() {
        // A balance of -100 ms refills to zero in 200 ms; a balance of -150 ms in 300 ms.
        assert_eq!(retry_after_ms(200), 200);
        assert_eq!(retry_after_ms(300), 300);
    }

    /// REQ: API-072
    #[test]
    fn retry_after_ms_never_goes_below_one_hundred_milliseconds() {
        assert_eq!(retry_after_ms(0), 100);
        assert_eq!(retry_after_ms(1), 100);
        assert_eq!(retry_after_ms(10), 100);
        assert_eq!(retry_after_ms(100), 100);
        assert_eq!(retry_after_ms(-40), 100);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_starts_empty_and_default_equals_new() {
        assert_eq!(ProcessQuota::new().live(), 0);
        assert_eq!(ProcessQuota::default(), ProcessQuota::new());
    }

    /// REQ: API-070
    #[test]
    fn process_quota_accepts_two_spawned_children_at_the_default_limit() {
        let mut quota = ProcessQuota::new();
        assert!(quota.spawn().is_ok());
        assert!(quota.spawn().is_ok());
        assert_eq!(quota.live(), 2);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_refuses_a_third_spawn_with_quota_exceeded() {
        let mut quota = ProcessQuota::new();
        quota.spawn().expect("first spawn is within the default limit of two");
        quota.spawn().expect("second spawn is within the default limit of two");
        assert_eq!(refusal_code(quota.spawn()), ErrorCode::QuotaExceeded);
        assert_eq!(quota.live(), 2);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_refuses_run_at_the_limit_with_quota_exceeded() {
        let mut quota = ProcessQuota::new();
        quota.spawn().expect("first spawn is within the limit");
        quota.spawn().expect("second spawn is within the limit");
        assert_eq!(refusal_code(quota.run()), ErrorCode::QuotaExceeded);
        assert_eq!(quota.live(), 2);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_short_runs_complete_without_staying_counted() {
        let mut quota = ProcessQuota::new();
        for _ in 0..5 {
            assert!(quota.run().is_ok());
        }
        assert_eq!(quota.live(), 0);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_hold_run_stays_counted_until_exit_one() {
        let mut quota = ProcessQuota::new();
        quota.hold_run().expect("first held run is within the limit");
        assert_eq!(quota.live(), 1);
        quota.hold_run().expect("second held run is within the limit");
        assert_eq!(quota.live(), 2);
        assert_eq!(refusal_code(quota.hold_run()), ErrorCode::QuotaExceeded);
        assert_eq!(quota.live(), 2);
        quota.exit_one();
        assert_eq!(quota.live(), 1);
        assert!(quota.hold_run().is_ok());
        assert_eq!(quota.live(), 2);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_held_runs_and_spawns_share_one_limit() {
        let mut quota = ProcessQuota::new();
        quota.hold_run().expect("held run is within the limit");
        quota.spawn().expect("spawn is the second child within the limit");
        assert_eq!(quota.live(), 2);
        assert_eq!(refusal_code(quota.spawn()), ErrorCode::QuotaExceeded);
    }

    /// REQ: API-070
    #[test]
    fn process_quota_exit_one_frees_a_slot_for_a_new_spawn() {
        let mut quota = ProcessQuota::new();
        quota.spawn().expect("first spawn is within the limit");
        quota.spawn().expect("second spawn is within the limit");
        quota.exit_one();
        assert_eq!(quota.live(), 1);
        assert!(quota.spawn().is_ok());
        assert_eq!(quota.live(), 2);
    }

    /// REQ: HST-499
    #[test]
    fn reply_deadline_ms_matches_the_manual_for_every_kind() {
        assert_eq!(reply_deadline_ms(ReplyKind::Command), 30_000);
        assert_eq!(reply_deadline_ms(ReplyKind::Service), 30_000);
        assert_eq!(reply_deadline_ms(ReplyKind::SessionsSend), 30_000);
        assert_eq!(reply_deadline_ms(ReplyKind::InferenceCompletion), 120_000);
        assert_eq!(reply_deadline_ms(ReplyKind::HttpApiRespond), 30_000);
        assert_eq!(reply_deadline_ms(ReplyKind::ClientRequestDefault), 5_000);
        assert_eq!(reply_deadline_ms(ReplyKind::JobCancelGrace), 1_000);
    }

    /// REQ: HST-499
    #[test]
    fn client_request_timeout_keeps_values_inside_the_allowed_range() {
        assert_eq!(client_request_timeout_ms(1), 1);
        assert_eq!(client_request_timeout_ms(5_000), 5_000);
        assert_eq!(client_request_timeout_ms(12_345), 12_345);
        assert_eq!(client_request_timeout_ms(30_000), 30_000);
    }

    /// REQ: HST-499
    #[test]
    fn client_request_timeout_clamps_zero_up_to_one_millisecond() {
        assert_eq!(client_request_timeout_ms(0), 1);
    }

    /// REQ: HST-499
    #[test]
    fn client_request_timeout_clamps_values_above_the_range_down() {
        assert_eq!(client_request_timeout_ms(30_001), 30_000);
        assert_eq!(client_request_timeout_ms(u32::MAX), 30_000);
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_reports_nothing_before_the_command_deadline() {
        let mut waiter = ReplyWaiter::new(ReplyKind::Command);
        assert_eq!(waiter.result(), None);
        waiter.advance_ms(29_999);
        assert_eq!(waiter.result(), None);
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_reports_timed_out_after_the_command_deadline() {
        let mut waiter = ReplyWaiter::new(ReplyKind::Command);
        waiter.advance_ms(30_001);
        let error = waiter.result().expect("the deadline has passed");
        assert_eq!(error.code, ErrorCode::TimedOut);
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_accumulates_elapsed_time_across_advances() {
        let mut waiter = ReplyWaiter::new(ReplyKind::Command);
        waiter.advance_ms(10_000);
        waiter.advance_ms(10_000);
        assert_eq!(waiter.result(), None);
        waiter.advance_ms(10_001);
        assert!(waiter.result().is_some());
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_zero_advance_keeps_the_reply_pending() {
        let mut waiter = ReplyWaiter::new(ReplyKind::SessionsSend);
        waiter.advance_ms(0);
        assert_eq!(waiter.result(), None);
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_client_request_default_times_out_at_five_seconds() {
        let mut waiter = ReplyWaiter::new(ReplyKind::ClientRequestDefault);
        waiter.advance_ms(4_999);
        assert_eq!(waiter.result(), None);
        waiter.advance_ms(2);
        let error = waiter.result().expect("the 5 s deadline has passed");
        assert_eq!(error.code, ErrorCode::TimedOut);
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_inference_completion_allows_two_minutes() {
        let mut waiter = ReplyWaiter::new(ReplyKind::InferenceCompletion);
        waiter.advance_ms(119_999);
        assert_eq!(waiter.result(), None);
        waiter.advance_ms(2);
        assert_eq!(waiter.result().map(|error| error.code), Some(ErrorCode::TimedOut));
    }

    /// REQ: HST-499
    #[test]
    fn reply_waiter_job_cancel_grace_is_one_second() {
        let mut waiter = ReplyWaiter::new(ReplyKind::JobCancelGrace);
        waiter.advance_ms(999);
        assert_eq!(waiter.result(), None);
        waiter.advance_ms(2);
        assert_eq!(waiter.result().map(|error| error.code), Some(ErrorCode::TimedOut));
    }

    /// REQ: HST-190
    #[test]
    fn soft_breach_is_tolerated_below_the_annotate_streak_limit() {
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 1, 5), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 2, 5), SoftVerdict::Tolerated);
    }

    /// REQ: HST-190
    #[test]
    fn soft_breach_traps_at_the_streak_limit_of_three_for_annotate_and_detector() {
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 3, 5), SoftVerdict::TrapCounted);
        assert_eq!(soft_breach_verdict(CallClass::Detector, 3, 5), SoftVerdict::TrapCounted);
        assert_eq!(soft_breach_verdict(CallClass::Detector, 2, 5), SoftVerdict::Tolerated);
    }

    /// REQ: HST-190
    #[test]
    fn soft_breach_scene_frames_trap_only_at_thirty_in_a_row() {
        assert_eq!(soft_breach_verdict(CallClass::Frame, 1, 1), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::Frame, 29, 1), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::Frame, 30, 1), SoftVerdict::TrapCounted);
    }

    /// REQ: HST-189
    #[test]
    fn soft_breach_past_the_hundred_millisecond_grace_traps_at_once() {
        // Annotate has a 10 ms soft deadline, so the grace ends at 110 ms.
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 1, 109), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 1, 111), SoftVerdict::TrapCounted);
    }

    /// REQ: HST-189
    #[test]
    fn soft_breach_grace_follows_the_class_deadline_for_settings_validate() {
        // Settings validation has a 100 ms soft deadline, so the grace ends at 200 ms.
        assert_eq!(soft_breach_verdict(CallClass::SettingsValidate, 1, 150), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::SettingsValidate, 1, 201), SoftVerdict::TrapCounted);
    }

    /// REQ: HST-190
    #[test]
    fn soft_breach_settings_validate_streak_limit_is_three() {
        assert_eq!(soft_breach_verdict(CallClass::SettingsValidate, 2, 5), SoftVerdict::Tolerated);
        assert_eq!(soft_breach_verdict(CallClass::SettingsValidate, 3, 5), SoftVerdict::TrapCounted);
    }

    /// REQ: HST-190
    #[test]
    fn soft_breach_with_no_prior_breach_and_no_elapsed_time_is_tolerated() {
        assert_eq!(soft_breach_verdict(CallClass::Annotate, 0, 0), SoftVerdict::Tolerated);
    }

    /// REQ: HST-498
    #[test]
    fn hook_handler_breaches_are_tolerated_until_the_third() {
        assert_eq!(hook_handler_breach_verdict(1), SoftVerdict::Tolerated);
        assert_eq!(hook_handler_breach_verdict(2), SoftVerdict::Tolerated);
    }

    /// REQ: HST-498
    #[test]
    fn hook_handler_third_breach_disables_the_handler_without_a_failure() {
        assert_eq!(hook_handler_breach_verdict(3), SoftVerdict::DisableHandlerNoFailure);
        assert_eq!(hook_handler_breach_verdict(4), SoftVerdict::DisableHandlerNoFailure);
    }

    /// REQ: HST-498
    #[test]
    fn hook_handler_breach_never_counts_as_a_trap() {
        for breaches in 1..=10 {
            assert_ne!(hook_handler_breach_verdict(breaches), SoftVerdict::TrapCounted);
        }
    }
}
