//! Limit rules that are pure functions of counts and times (reference-errors-and-limits.md):
//! retry hints, the process quota, reply deadlines and soft-deadline verdicts.

use ilium_plugin_api::PluginError;

use crate::calls::CallClass;

/// The `retry-after-ms` hint for a limit that frees in `remaining_ms` (reference-errors-and-limits.md).
pub fn retry_after_ms(_remaining_ms: i64) -> i64 {
    unimplemented!("host::limits_rules::retry_after_ms")
}

/// The per-plugin budget of live child processes (reference-errors-and-limits.md `processes`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessQuota {
    live: usize,
    held: usize,
}

impl ProcessQuota {
    /// An empty quota at the default limit.
    pub fn new() -> ProcessQuota {
        unimplemented!("host::limits_rules::ProcessQuota::new")
    }

    /// Spawns a long-lived child; `quota-exceeded` at the limit.
    pub fn spawn(&mut self) -> Result<(), PluginError> {
        unimplemented!("host::limits_rules::ProcessQuota::spawn")
    }

    /// Runs a short child to completion; `quota-exceeded` at the limit.
    pub fn run(&mut self) -> Result<(), PluginError> {
        unimplemented!("host::limits_rules::ProcessQuota::run")
    }

    /// Runs a child and keeps it counted until `exit_one`.
    pub fn hold_run(&mut self) -> Result<(), PluginError> {
        unimplemented!("host::limits_rules::ProcessQuota::hold_run")
    }

    /// One child exited.
    pub fn exit_one(&mut self) {
        unimplemented!("host::limits_rules::ProcessQuota::exit_one")
    }

    /// The live child count.
    pub fn live(&self) -> usize {
        unimplemented!("host::limits_rules::ProcessQuota::live")
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
}

/// The deadline of a reply kind in milliseconds (reference-errors-and-limits.md).
pub fn reply_deadline_ms(_kind: ReplyKind) -> u64 {
    unimplemented!("host::limits_rules::reply_deadline_ms")
}

/// Clamps a requested client-request timeout to the allowed range (reference-errors-and-limits.md).
pub fn client_request_timeout_ms(_requested_ms: u32) -> u32 {
    unimplemented!("host::limits_rules::client_request_timeout_ms")
}

/// Waits for one reply and reports `timed-out` when its deadline passes (reference-errors-and-limits.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyWaiter {
    kind: ReplyKind,
    elapsed_ms: u64,
}

impl ReplyWaiter {
    /// A waiter for one reply kind.
    pub fn new(_kind: ReplyKind) -> ReplyWaiter {
        unimplemented!("host::limits_rules::ReplyWaiter::new")
    }

    /// Time passed.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("host::limits_rules::ReplyWaiter::advance_ms")
    }

    /// The error to report once the deadline passed, else `None`.
    pub fn result(&self) -> Option<PluginError> {
        unimplemented!("host::limits_rules::ReplyWaiter::result")
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

/// The verdict for the `breaches`-th soft breach of `class` that ran `elapsed_ms`
/// (reference-errors-and-limits.md).
pub fn soft_breach_verdict(_class: CallClass, _breaches: u32, _elapsed_ms: u64) -> SoftVerdict {
    unimplemented!("host::limits_rules::soft_breach_verdict")
}

/// The verdict for the `breaches`-th breach of an extension hook handler (reference-errors-and-limits.md).
pub fn hook_handler_breach_verdict(_breaches: u32) -> SoftVerdict {
    unimplemented!("host::limits_rules::hook_handler_breach_verdict")
}
