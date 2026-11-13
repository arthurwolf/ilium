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
        (self.elapsed_ms >= reply_deadline_ms(self.kind))
            .then(|| PluginError::new(ErrorCode::TimedOut, "Plugin reply deadline reached"))
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
