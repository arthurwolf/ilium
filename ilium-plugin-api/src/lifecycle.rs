//! Package and instance state machines and the restart policy (reference-runtime-model.md,
//! reference-wit.md `ilium:host/plugins`). The state enums are the generated WIT enums.

pub use crate::generated::host::{HealthState, InstanceState, PackageState};
pub use crate::generated::types::DeactivationReason;

use crate::error::Code;

/// An input to the instance state machine (reference-runtime-model.md "Instance states").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceInput {
    /// The host starts activation (reference-runtime-model.md).
    Activate,
    /// `activate` returned `ok` (reference-runtime-model.md).
    ActivateOk,
    /// `activate` failed with this code (reference-runtime-model.md).
    ActivateErr(Code),
    /// A required dependency is not active (reference-dependency-resolution.md).
    DependencyMissing,
    /// The required dependency became active (reference-dependency-resolution.md).
    DependencyReady,
    /// The host starts deactivation for this reason (reference-runtime-model.md).
    Deactivate(DeactivationReason),
    /// `deactivate` returned or its deadline passed (reference-runtime-model.md).
    DeactivateDone,
    /// Deactivation finished while the package no longer permits this side to run.
    DeactivateDoneStopped,
    /// The counted failure reached the restart limit for this side.
    FailureLimitReached,
    /// The dependency wait expired without being a counted failure.
    DependencyWaitExpired,
    /// The instance trapped with this code (reference-errors-and-limits.md "Trap codes").
    Trap(Code),
    /// The restart delay elapsed (reference-runtime-model.md "12. Failures and restart").
    RestartTimerFired,
    /// The user or policy disabled the plugin (reference-runtime-model.md).
    Disable,
    /// The user enabled the plugin (reference-runtime-model.md).
    Enable,
}

/// The next instance state, or the code of an illegal transition (reference-runtime-model.md).
pub fn next_instance_state(
    current: InstanceState,
    input: &InstanceInput,
) -> Result<InstanceState, Code> {
    use InstanceInput as I;
    use InstanceState as S;
    let next = match (current, input) {
        (S::Inactive, I::Activate) | (S::RestartScheduled, I::RestartTimerFired)
            | (S::WaitingDependency, I::DependencyReady) => S::Activating,
        (S::Inactive, I::DependencyMissing) => S::WaitingDependency,
        (S::WaitingDependency, I::DependencyWaitExpired) => S::Inactive,
        (S::Activating, I::ActivateOk) => S::Active,
        (S::Activating, I::ActivateErr(_)) | (S::Activating | S::Active, I::Trap(_)) => S::RestartScheduled,
        (S::RestartScheduled, I::FailureLimitReached) => S::StoppedByFailure,
        (S::Active, I::Deactivate(reason)) if *reason != DeactivationReason::CrashRestart => S::Deactivating,
        (S::Active, I::Disable) => S::Deactivating,
        (S::Deactivating, I::DeactivateDone) => S::Inactive,
        (S::Deactivating, I::DeactivateDoneStopped) => S::Stopped,
        (S::Inactive | S::WaitingDependency | S::RestartScheduled | S::StoppedByFailure | S::Stopped, I::Disable) => S::Stopped,
        (S::Stopped | S::StoppedByFailure | S::Inactive, I::Enable) => S::Inactive,
        _ => return Err(Code::from_static("host.invalid-state")),
    };
    Ok(next)
}

/// An input to the package state machine (reference-runtime-model.md "Package states").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageInput {
    /// The package is installed (packages.md).
    Install,
    /// The user enables it (reference-runtime-model.md).
    Enable,
    /// The user disables it (reference-runtime-model.md).
    Disable,
    /// The publisher key is no longer trusted (security.md).
    TrustRemoved,
    /// The package or key is revoked (security.md).
    Revoked,
    /// Admin policy denies it (security.md).
    PolicyDeny,
    /// A load check refused it with this code (reference-manifest-schema.md).
    RefuseAtLoad(Code),
    /// An update is staged for the next restart (packages.md).
    StageUpdate,
    /// The staged update is swapped in (packages.md).
    SwapUpdate,
    /// The user asks to uninstall (packages.md).
    RequestUninstall,
    /// The last instance stopped (reference-runtime-model.md).
    InstanceStopped,
}

/// The next package state, or the code of an illegal transition (reference-runtime-model.md).
pub fn next_package_state(
    current: PackageState,
    input: &PackageInput,
) -> Result<PackageState, Code> {
    use PackageInput as I;
    use PackageState as S;
    let next = match (current, input) {
        (S::NotInstalled, I::Install) => S::InstalledEnabled,
        (S::InstalledEnabled, I::Disable) => S::InstalledDisabledByUser,
        (S::InstalledDisabledByUser | S::InstalledEnabled, I::Enable) => S::InstalledEnabled,
        (S::InstalledEnabled, I::TrustRemoved) => S::DisabledUntrusted,
        (S::InstalledEnabled, I::PolicyDeny) => S::DisabledByPolicy,
        (S::InstalledEnabled, I::StageUpdate) => S::UpdateStaged,
        (S::UpdateStaged, I::SwapUpdate) => S::InstalledEnabled,
        (S::UninstallPending, I::InstanceStopped) => S::NotInstalled,
        (state, I::Revoked) if state != S::NotInstalled => S::DisabledBlocked,
        (state, I::RefuseAtLoad(_)) if state != S::NotInstalled => S::RefusedAtLoad,
        (state, I::RequestUninstall) if !matches!(state, S::NotInstalled | S::UninstallPending) => S::UninstallPending,
        _ => return Err(Code::from_static("host.invalid-state")),
    };
    Ok(next)
}

/// Delay before the first restart after a failure, in milliseconds (reference-runtime-model.md "12. Failures and restart").
pub const FIRST_RESTART_DELAY_MS: u64 = 1_000;

/// Delay before the second restart, in milliseconds (reference-runtime-model.md "12. Failures and restart").
pub const SECOND_RESTART_DELAY_MS: u64 = 5_000;

/// Window in which a third failure disables the plugin, in milliseconds (reference-runtime-model.md "12. Failures and restart").
pub const FAILURE_WINDOW_MS: u64 = 600_000;

/// Restart rule: 1st failure restarts after 1 s, 2nd after 5 s, 3rd within 10 minutes disables
/// (reference-runtime-model.md "12. Failures and restart").
#[derive(Debug, Clone, Default)]
pub struct RestartPolicy {
    failures_ms: Vec<u64>,
}

/// What the host does after a failure (reference-runtime-model.md "12. Failures and restart").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Restart after this many milliseconds.
    RestartAfterMs(u64),
    /// Stop and disable the plugin.
    Disable,
}

impl RestartPolicy {
    /// Records a failure at `now_ms` and decides (reference-runtime-model.md "12. Failures and restart").
    pub fn on_failure(&mut self, now_ms: u64) -> RestartDecision {
        // The boundary is exclusive: a failure exactly ten minutes old expires.
        self.failures_ms.retain(|time| *time <= now_ms && now_ms - *time < FAILURE_WINDOW_MS);
        self.failures_ms.push(now_ms);
        match self.failures_ms.len() {
            1 => RestartDecision::RestartAfterMs(FIRST_RESTART_DELAY_MS),
            2 => RestartDecision::RestartAfterMs(SECOND_RESTART_DELAY_MS),
            _ => RestartDecision::Disable,
        }
    }

    /// Failures inside the window ending at `now_ms` (reference-runtime-model.md "12. Failures and restart").
    pub fn failures_in_window(&self, now_ms: u64) -> u32 {
        self.failures_ms.iter().filter(|time| **time <= now_ms && now_ms - **time < FAILURE_WINDOW_MS).count() as u32
    }

    /// Forgets every failure, after a manual enable (reference-runtime-model.md "12. Failures and restart").
    pub fn reset(&mut self) {
        self.failures_ms.clear();
    }
}
