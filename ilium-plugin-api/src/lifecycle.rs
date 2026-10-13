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
    unimplemented!("api::lifecycle::next_instance_state")
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
    unimplemented!("api::lifecycle::next_package_state")
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
    // Failure times of the current window; read by the implementation that replaces the stubs.
    #[allow(dead_code)]
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
        unimplemented!("api::lifecycle::RestartPolicy::on_failure")
    }

    /// Failures inside the window ending at `now_ms` (reference-runtime-model.md "12. Failures and restart").
    pub fn failures_in_window(&self, now_ms: u64) -> u32 {
        unimplemented!("api::lifecycle::RestartPolicy::failures_in_window")
    }

    /// Forgets every failure, after a manual enable (reference-runtime-model.md "12. Failures and restart").
    pub fn reset(&mut self) {
        unimplemented!("api::lifecycle::RestartPolicy::reset")
    }
}
