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

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand for a documented code literal.
    fn code(text: &'static str) -> Code {
        Code::from_static(text)
    }

    // ----- Instance state machine (reference-plugin-lifecycle-states.md, "Instance-state transitions") -----

    /// Row I1: an inactive side with a fired activation trigger starts `activate`.
    /// REQ: LIF-005
    #[test]
    fn inactive_activate_moves_to_activating() {
        assert_eq!(
            next_instance_state(InstanceState::Inactive, &InstanceInput::Activate),
            Ok(InstanceState::Activating),
            "row I1: inactive + activation trigger -> activating"
        );
    }

    /// Row I16: an inactive side whose required provider is not active waits.
    /// REQ: LIF-284
    #[test]
    fn inactive_missing_dependency_enters_waiting_dependency() {
        assert_eq!(
            next_instance_state(InstanceState::Inactive, &InstanceInput::DependencyMissing),
            Ok(InstanceState::WaitingDependency),
            "row I16: inactive + dependency missing -> waiting-dependency"
        );
    }

    /// Row I17: when every requirement is active, the waiting side runs `activate`.
    /// REQ: LIF-284
    #[test]
    fn waiting_dependency_becomes_activating_when_ready() {
        assert_eq!(
            next_instance_state(
                InstanceState::WaitingDependency,
                &InstanceInput::DependencyReady
            ),
            Ok(InstanceState::Activating),
            "row I17: waiting-dependency + dependency ready -> activating"
        );
    }

    /// Row I3: `activate` returning `ok` within its deadline makes the instance active.
    /// REQ: LIF-005
    #[test]
    fn activating_ok_becomes_active() {
        assert_eq!(
            next_instance_state(InstanceState::Activating, &InstanceInput::ActivateOk),
            Ok(InstanceState::Active),
            "row I3: activating + activate ok -> active"
        );
    }

    /// Row I4: an `activate` error or deadline breach drops the instance into `failed`
    /// (no `deactivate` call is part of the transition).
    /// REQ: LIF-012
    #[test]
    fn activating_activate_error_fails_without_deactivate() {
        for error_code in ["trap.deadline", "trap.memory"] {
            assert_eq!(
                next_instance_state(
                    InstanceState::Activating,
                    &InstanceInput::ActivateErr(code(error_code))
                ),
                Ok(InstanceState::Failed),
                "row I4: activating + activate error {error_code} -> failed"
            );
        }
    }

    /// Row I4: a trap while activating drops the instance into `failed`.
    /// REQ: LIF-012
    #[test]
    fn activating_trap_fails() {
        assert_eq!(
            next_instance_state(
                InstanceState::Activating,
                &InstanceInput::Trap(code("trap.memory"))
            ),
            Ok(InstanceState::Failed),
            "row I4: activating + trap -> failed"
        );
    }

    /// Row I5: a trap while active drops the instance into `failed`, not `deactivating`.
    /// REQ: LIF-012
    #[test]
    fn active_trap_fails_without_deactivate() {
        assert_eq!(
            next_instance_state(
                InstanceState::Active,
                &InstanceInput::Trap(code("trap.deadline"))
            ),
            Ok(InstanceState::Failed),
            "row I5: active + trap -> failed, with no deactivate call"
        );
    }

    /// Rows I9 to I13: every deactivation reason from `active` moves the instance to `deactivating`,
    /// and a disable trigger does the same (row I9).
    /// REQ: LIF-014
    #[test]
    fn active_deactivation_reasons_move_to_deactivating() {
        let reasons = [
            DeactivationReason::Disabled,
            DeactivationReason::Shutdown,
            DeactivationReason::Upgrade,
            DeactivationReason::Revoked,
            DeactivationReason::Granted,
        ];
        for reason in reasons {
            assert_eq!(
                next_instance_state(InstanceState::Active, &InstanceInput::Deactivate(reason)),
                Ok(InstanceState::Deactivating),
                "rows I9 to I13: active + deactivate({reason:?}) -> deactivating"
            );
        }
        assert_eq!(
            next_instance_state(InstanceState::Active, &InstanceInput::Disable),
            Ok(InstanceState::Deactivating),
            "row I9: active + disable -> deactivating"
        );
    }

    /// Row I8: when the restart timer fires, the side runs `activate` again.
    /// REQ: LIF-005
    #[test]
    fn restart_timer_fired_reactivates() {
        assert_eq!(
            next_instance_state(
                InstanceState::RestartScheduled,
                &InstanceInput::RestartTimerFired
            ),
            Ok(InstanceState::Activating),
            "row I8: restart-scheduled + timer fired -> activating"
        );
    }

    /// Row I14: disabling while a restart is scheduled cancels the timer, so the restart never begins.
    /// The resulting state is not named by the manual, so the test only excludes `activating`.
    /// REQ: LIF-013
    #[test]
    fn restart_scheduled_disable_never_reactivates() {
        let result = next_instance_state(InstanceState::RestartScheduled, &InstanceInput::Disable);
        assert_ne!(
            result,
            Ok(InstanceState::Activating),
            "row I14: disable cancels the restart timer, so the side must not activate"
        );
    }

    /// Row P5 (side effect): enabling a stopped-by-failure side clears its window and sets it `inactive`.
    /// REQ: LIF-036
    #[test]
    fn stopped_by_failure_enable_sets_inactive() {
        assert_eq!(
            next_instance_state(InstanceState::StoppedByFailure, &InstanceInput::Enable),
            Ok(InstanceState::Inactive),
            "row P5: stopped-by-failure + enable -> inactive"
        );
    }

    /// Only the transitions in the tables may occur; every other input/state pair is an illegal transition.
    /// REQ: LIF-005
    #[test]
    fn transitions_outside_the_tables_are_rejected() {
        let illegal_pairs = [
            (InstanceState::Inactive, InstanceInput::ActivateOk),
            (InstanceState::Inactive, InstanceInput::Trap(code("trap.deadline"))),
            (InstanceState::Inactive, InstanceInput::RestartTimerFired),
            (InstanceState::Active, InstanceInput::Activate),
            (InstanceState::Active, InstanceInput::ActivateOk),
            (InstanceState::Active, InstanceInput::DependencyReady),
            (InstanceState::WaitingDependency, InstanceInput::Trap(code("trap.deadline"))),
            (InstanceState::Stopped, InstanceInput::Trap(code("trap.deadline"))),
            (InstanceState::Deactivating, InstanceInput::Activate),
        ];
        for (state, input) in illegal_pairs {
            assert!(
                next_instance_state(state, &input).is_err(),
                "{state:?} + {input:?} is not in the instance tables, so it must be an illegal transition"
            );
        }
    }

    // ----- Package state machine (reference-plugin-lifecycle-states.md, "Package-state transitions") -----

    /// Row P1: installing a new plugin gives it the `installed-enabled` package state.
    /// REQ: LIF-001
    #[test]
    fn install_moves_not_installed_to_installed_enabled() {
        assert_eq!(
            next_package_state(PackageState::NotInstalled, &PackageInput::Install),
            Ok(PackageState::InstalledEnabled),
            "row P1: not-installed + install -> installed-enabled"
        );
    }

    /// Rows P2 and P3: a user disable and enable move between the two user-visible states.
    /// REQ: LIF-001
    #[test]
    fn user_disable_and_enable_round_trip() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::Disable),
            Ok(PackageState::InstalledDisabledByUser),
            "row P2: installed-enabled + disable -> installed-disabled-by-user"
        );
        assert_eq!(
            next_package_state(PackageState::InstalledDisabledByUser, &PackageInput::Enable),
            Ok(PackageState::InstalledEnabled),
            "row P3: installed-disabled-by-user + enable -> installed-enabled"
        );
    }

    /// Enabling an enabled plugin, or disabling a disabled one, returns `ok` and changes nothing.
    /// REQ: LIF-019
    #[test]
    fn repeated_set_enabled_changes_nothing() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::Enable),
            Ok(PackageState::InstalledEnabled),
            "set-enabled(true) on an enabled plugin keeps installed-enabled"
        );
        assert_eq!(
            next_package_state(PackageState::InstalledDisabledByUser, &PackageInput::Disable),
            Ok(PackageState::InstalledDisabledByUser),
            "set-enabled(false) on a disabled plugin keeps installed-disabled-by-user"
        );
    }

    /// Row P13: removing the signing key with `--force` gives `disabled-untrusted`.
    /// REQ: LIF-283
    #[test]
    fn trust_removed_disables_untrusted() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::TrustRemoved),
            Ok(PackageState::DisabledUntrusted),
            "row P13: installed-enabled + trust removed -> disabled-untrusted"
        );
    }

    /// Row P14: enabling an untrusted plugin with `--accept-risk` returns it to `installed-enabled`.
    /// REQ: LIF-283
    #[test]
    fn untrusted_enable_returns_to_installed_enabled() {
        assert_eq!(
            next_package_state(PackageState::DisabledUntrusted, &PackageInput::Enable),
            Ok(PackageState::InstalledEnabled),
            "row P14: disabled-untrusted + enable -> installed-enabled"
        );
    }

    /// Row P15: a compromised key revocation blocks an installed plugin.
    /// REQ: LIF-283
    #[test]
    fn revoked_key_blocks_installed_plugin() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::Revoked),
            Ok(PackageState::DisabledBlocked),
            "row P15: installed-enabled + revoked -> disabled-blocked"
        );
    }

    /// Row P16: `enable --accept-risk` clears a blocked plugin back to `installed-enabled`.
    /// REQ: LIF-283
    #[test]
    fn blocked_enable_returns_to_installed_enabled() {
        assert_eq!(
            next_package_state(PackageState::DisabledBlocked, &PackageInput::Enable),
            Ok(PackageState::InstalledEnabled),
            "row P16: disabled-blocked + enable -> installed-enabled"
        );
    }

    /// Row P6: an administrator policy entry moves an enabled plugin to `disabled-by-policy`.
    /// REQ: LIF-004
    #[test]
    fn policy_deny_moves_to_disabled_by_policy() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::PolicyDeny),
            Ok(PackageState::DisabledByPolicy),
            "row P6: installed-enabled + policy deny -> disabled-by-policy"
        );
    }

    /// Enabling a policy-blocked or refused plugin returns `not-permitted` and changes no state.
    /// REQ: LIF-007
    #[test]
    fn enable_on_policy_or_refused_plugin_is_not_permitted() {
        for state in [PackageState::DisabledByPolicy, PackageState::RefusedAtLoad] {
            assert_eq!(
                next_package_state(state, &PackageInput::Enable),
                Err(code("not-permitted")),
                "enabling {state:?} must return not-permitted"
            );
        }
    }

    /// Row P7: each documented load-refusal code moves the package to `refused-at-load`.
    /// REQ: LIF-003
    #[test]
    fn load_refusal_codes_move_to_refused_at_load() {
        let refusal_codes = [
            "package.api-unsupported",
            "trust.key-revoked",
            "policy.tier-denied",
            "package.corrupt",
            "policy.blocked",
        ];
        for refusal_code in refusal_codes {
            assert_eq!(
                next_package_state(
                    PackageState::InstalledEnabled,
                    &PackageInput::RefuseAtLoad(code(refusal_code))
                ),
                Ok(PackageState::RefusedAtLoad),
                "row P7: installed-enabled + refusal {refusal_code} -> refused-at-load"
            );
        }
    }

    /// Rows P8 to P10: staging an update and swapping it in both leave the package `installed-enabled`;
    /// the package never rests in `update-staged` for a plain update.
    /// REQ: LIF-010
    #[test]
    fn staging_and_swapping_an_update_keep_installed_enabled() {
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::StageUpdate),
            Ok(PackageState::InstalledEnabled),
            "row P8: staging a verified update changes no visible package state"
        );
        assert_eq!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::SwapUpdate),
            Ok(PackageState::InstalledEnabled),
            "rows P9 and P10: the swap, or its rollback, ends in installed-enabled"
        );
    }

    /// Row P11: an uninstall request moves an installed plugin to `uninstall-pending`.
    /// REQ: LIF-001
    #[test]
    fn request_uninstall_moves_to_uninstall_pending() {
        for state in [
            PackageState::InstalledEnabled,
            PackageState::InstalledDisabledByUser,
        ] {
            assert_eq!(
                next_package_state(state, &PackageInput::RequestUninstall),
                Ok(PackageState::UninstallPending),
                "row P11: {state:?} + uninstall -> uninstall-pending"
            );
        }
    }

    /// While an uninstall is pending, a new enable or disable is refused with `conflict`.
    /// REQ: LIF-020
    #[test]
    fn uninstall_pending_refuses_set_enabled_with_conflict() {
        for input in [PackageInput::Enable, PackageInput::Disable] {
            assert_eq!(
                next_package_state(PackageState::UninstallPending, &input),
                Err(code("conflict")),
                "uninstall-pending + {input:?} must be refused with conflict"
            );
        }
    }

    /// Inputs that the package tables do not allow from the given state are illegal transitions.
    /// REQ: LIF-001
    #[test]
    fn package_transitions_outside_the_tables_are_rejected() {
        assert!(
            next_package_state(PackageState::NotInstalled, &PackageInput::Enable).is_err(),
            "enable of a plugin that is not installed is not in the package tables"
        );
        assert!(
            next_package_state(
                PackageState::NotInstalled,
                &PackageInput::RefuseAtLoad(code("package.corrupt"))
            )
            .is_err(),
            "P7 applies to installed states only, so refusal of a not-installed id is illegal"
        );
        assert!(
            next_package_state(PackageState::InstalledEnabled, &PackageInput::Install).is_err(),
            "an already installed plugin cannot take the install input"
        );
    }

    // ----- Restart policy (reference-runtime-model.md "12. Failures and restart", Lifecycle "Failure counting and restart") -----

    /// The window constants match the manual: 10 minutes, 1 s and 5 s restart delays.
    /// REQ: LIF-027
    #[test]
    fn restart_constants_match_the_manual() {
        assert_eq!(FAILURE_WINDOW_MS, 600_000, "window is 10 minutes");
        assert_eq!(FIRST_RESTART_DELAY_MS, 1_000, "first restart after 1 s");
        assert_eq!(SECOND_RESTART_DELAY_MS, 5_000, "second restart after 5 s");
    }

    /// The first counted failure schedules a restart after 1 s.
    /// REQ: LIF-028
    #[test]
    fn first_failure_restarts_after_one_second() {
        let mut policy = RestartPolicy::default();
        assert_eq!(
            policy.on_failure(0),
            RestartDecision::RestartAfterMs(1_000),
            "first failure restarts after 1 s"
        );
    }

    /// The second counted failure within the window schedules a restart after 5 s.
    /// REQ: LIF-029
    #[test]
    fn second_failure_restarts_after_five_seconds() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        assert_eq!(
            policy.on_failure(60_000),
            RestartDecision::RestartAfterMs(5_000),
            "second failure inside 10 minutes restarts after 5 s"
        );
    }

    /// The third counted failure within the window disables the plugin and schedules no restart.
    /// REQ: LIF-030
    #[test]
    fn third_failure_inside_window_disables() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        policy.on_failure(60_000);
        assert_eq!(
            policy.on_failure(120_000),
            RestartDecision::Disable,
            "third failure within 10 minutes disables the side"
        );
    }

    /// Failures older than 600 s drop out of the count (manual example: 0 s, 300 s, 660 s).
    /// REQ: LIF-027
    #[test]
    fn failure_at_660_seconds_sees_only_the_one_at_300_seconds() {
        let mut policy = RestartPolicy::default();
        assert_eq!(policy.on_failure(0), RestartDecision::RestartAfterMs(1_000));
        assert_eq!(policy.on_failure(300_000), RestartDecision::RestartAfterMs(5_000));
        assert_eq!(
            policy.on_failure(660_000),
            RestartDecision::RestartAfterMs(5_000),
            "the failure at 0 s has aged out, so 660 s is the second counted failure"
        );
    }

    /// A failure counts while its age is below 600 s: 599 s and 599.999 s count, 600 s does not.
    /// REQ: LIF-027
    #[test]
    fn failure_age_boundary_is_strictly_below_600_seconds() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        assert_eq!(policy.failures_in_window(599_000), 1, "age 599 s still counts");
        assert_eq!(policy.failures_in_window(599_999), 1, "age 599.999 s still counts");
        assert_eq!(policy.failures_in_window(600_000), 0, "age exactly 600 s no longer counts");
        assert_eq!(policy.failures_in_window(601_000), 0, "age 601 s does not count");
    }

    /// A failure exactly 600 s old no longer counts, so the next failure is a first failure again.
    /// REQ: LIF-027
    #[test]
    fn failure_exactly_at_window_end_is_not_counted() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        assert_eq!(
            policy.on_failure(600_000),
            RestartDecision::RestartAfterMs(1_000),
            "the failure at 0 s is outside the window at 600 s, so this is failure 1"
        );
    }

    /// A new policy (after a server restart) starts with an empty window.
    /// REQ: LIF-037
    #[test]
    fn default_policy_has_empty_window() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.failures_in_window(0), 0, "empty input: no failures recorded");
        assert_eq!(policy.failures_in_window(600_000), 0, "empty input at the window end");
    }

    /// Recording a failure makes it count inside the window.
    /// REQ: LIF-027
    #[test]
    fn recorded_failure_counts_inside_window() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        assert_eq!(policy.failures_in_window(0), 1, "one failure at time 0");
        policy.on_failure(10_000);
        assert_eq!(policy.failures_in_window(10_000), 2, "two failures inside the window");
    }

    /// Manual reset (enable, reset-failures) forgets every failure, so the next failure is a first failure.
    /// REQ: LIF-036
    #[test]
    fn reset_clears_failures_and_restarts_count() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        policy.on_failure(1_000);
        policy.reset();
        assert_eq!(policy.failures_in_window(1_000), 0, "reset empties the window");
        assert_eq!(
            policy.on_failure(2_000),
            RestartDecision::RestartAfterMs(1_000),
            "after reset the next failure restarts after 1 s"
        );
    }

    /// After a reset, the next failure restarts after the first-failure delay, not the second.
    /// REQ: LIF-009
    #[test]
    fn failure_after_enable_restarts_after_first_delay() {
        let mut policy = RestartPolicy::default();
        policy.on_failure(0);
        policy.reset();
        assert_eq!(
            policy.on_failure(500),
            RestartDecision::RestartAfterMs(1_000),
            "one failure after enable restarts after the first-failure delay"
        );
    }

    /// Each side and scope has its own window: failures on one policy do not count on another.
    /// REQ: LIF-025
    #[test]
    fn separate_policies_keep_separate_windows() {
        let mut client_policy = RestartPolicy::default();
        let mut server_policy = RestartPolicy::default();
        client_policy.on_failure(0);
        client_policy.on_failure(1_000);
        assert_eq!(
            server_policy.on_failure(1_000),
            RestartDecision::RestartAfterMs(1_000),
            "a failure on the other side is its first failure"
        );
        assert_eq!(server_policy.failures_in_window(1_000), 1, "server side has one failure");
        assert_eq!(client_policy.failures_in_window(1_000), 2, "client side has two failures");
    }
}
