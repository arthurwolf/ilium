//! What an older plugin sees from a newer host (reference-abi-evolution.md section 5).
//! Skel-api addition to plan 1.3 for ABI-028 and ABI-029.

use crate::generated::types::{DeactivationReason, Outcome};
use crate::version::ApiVersion;

/// A case family whose cases carry an API version (reference-abi-evolution.md section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseFamily {
    /// `event` cases (ABI-029).
    Event,
    /// `outcome` cases (ABI-029).
    Outcome,
    /// `deactivation-reason` cases (ABI-028, ABI-029).
    DeactivationReason,
    /// `intent` cases (reference-abi-evolution.md section 5).
    Intent,
}

/// The API version that introduced a case, `None` for an unknown case (reference-abi-evolution.md).
pub fn case_since(_family: CaseFamily, _case: &str) -> Option<ApiVersion> {
    unimplemented!("api::compat::case_since")
}

/// ABI-029: the outcome as a plugin of `plugin_api` receives it; a newer case maps to the
/// nearest older one (`ok-epoch` to `ok`).
pub fn map_outcome_for(_outcome: &Outcome, _plugin_api: ApiVersion) -> Outcome {
    unimplemented!("api::compat::map_outcome_for")
}

/// ABI-028, ABI-029: the reason as a plugin of `plugin_api` receives it; a newer reason is
/// `shutdown`, and `crash-restart` is never delivered (`None`).
pub fn map_reason_for(
    _reason: DeactivationReason,
    _plugin_api: ApiVersion,
) -> Option<DeactivationReason> {
    unimplemented!("api::compat::map_reason_for")
}

/// ABI-029: whether an event case reaches a plugin of `plugin_api`; a newer case is not
/// delivered and not counted in `events-dropped`.
pub fn event_deliverable(_event_case: &str, _plugin_api: ApiVersion) -> bool {
    unimplemented!("api::compat::event_deliverable")
}

/// What a host minor gives a plugin built against an SDK minor (ABI-065).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedResult {
    /// The plugin loads.
    Loads,
    /// The plugin loads and a deprecation notice is shown.
    LoadsDeprecated,
    /// The plugin is refused with this code.
    Refused(String),
}

/// One cell of the host minor against SDK minor matrix (ABI-064, ABI-065).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixCell {
    /// The minor of the host.
    pub host_minor: u16,
    /// The minor of the SDK the plugin was built with.
    pub sdk_minor: u16,
    /// The result the reference states for this pair.
    pub expected: ExpectedResult,
    /// The name of the CI job that runs this cell.
    pub job_name: String,
}

/// ABI-064: every cell of the matrix.
pub fn matrix_cells() -> Vec<MatrixCell> {
    unimplemented!("api::compat::matrix_cells")
}

/// ABI-065: what a host of `host_minor` really does with a plugin built with `sdk_minor`.
pub fn observed_result(_host_minor: u16, _sdk_minor: u16) -> ExpectedResult {
    unimplemented!("api::compat::observed_result")
}

/// ABI-064: the names of the CI jobs that run the matrix.
pub fn ci_matrix_jobs() -> Vec<String> {
    unimplemented!("api::compat::ci_matrix_jobs")
}

/// ABI-066: the cells inside the window whose observed result differs from the stated one.
pub fn matrix_mismatches() -> Vec<MatrixCell> {
    unimplemented!("api::compat::matrix_mismatches")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host's own contract version, `ilium:plugin@0.1.0`. Every stable 0.1.0 item is
    /// `@since(version = 0.1.0)` (reference-abi-evolution.md section 3).
    fn host_version() -> ApiVersion {
        ApiVersion {
            major: 0,
            minor: 1,
            patch: 0,
        }
    }

    /// A plugin built before 0.1.0 (the previous minor of the host window). Every 0.1.0 case is
    /// newer than it, so ABI-029 applies.
    fn predating_version() -> ApiVersion {
        ApiVersion {
            major: 0,
            minor: 0,
            patch: 9,
        }
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_reports_0_1_0_for_stable_event_case() {
        assert_eq!(
            case_since(CaseFamily::Event, "tree-changed"),
            Some(host_version())
        );
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_reports_0_1_0_for_last_listed_event_case() {
        assert_eq!(
            case_since(CaseFamily::Event, "display-mode-changed"),
            Some(host_version())
        );
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_reports_0_1_0_for_intent_case() {
        assert_eq!(
            case_since(CaseFamily::Intent, "send-input"),
            Some(host_version())
        );
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_reports_0_1_0_for_outcome_case_ok_epoch() {
        assert_eq!(
            case_since(CaseFamily::Outcome, "ok-epoch"),
            Some(host_version())
        );
    }

    /// REQ: ABI-028
    #[test]
    fn case_since_reports_0_1_0_for_reserved_crash_restart() {
        // The reserved case exists in the enum from 0.1.0 so that numbering never changes.
        assert_eq!(
            case_since(CaseFamily::DeactivationReason, "crash-restart"),
            Some(host_version())
        );
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_returns_none_for_unknown_case_name() {
        assert_eq!(case_since(CaseFamily::Event, "no-such-event"), None);
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_returns_none_for_empty_case_name() {
        assert_eq!(case_since(CaseFamily::Intent, ""), None);
    }

    /// REQ: ABI-029
    #[test]
    fn case_since_is_none_when_name_belongs_to_another_family() {
        // `tree-changed` is an event case, not an outcome case.
        assert_eq!(case_since(CaseFamily::Outcome, "tree-changed"), None);
    }

    /// REQ: ABI-029
    #[test]
    fn map_outcome_for_keeps_ok_for_current_api() {
        assert_eq!(map_outcome_for(&Outcome::Ok, host_version()), Outcome::Ok);
    }

    /// REQ: ABI-029
    #[test]
    fn map_outcome_for_keeps_ok_epoch_for_plugin_built_for_current_api() {
        assert_eq!(
            map_outcome_for(&Outcome::OkEpoch(7), host_version()),
            Outcome::OkEpoch(7)
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_outcome_for_maps_ok_epoch_to_ok_for_older_plugin() {
        // reference-abi-evolution.md section 5: `ok-epoch(n)` reaches an earlier API as `ok`.
        assert_eq!(
            map_outcome_for(&Outcome::OkEpoch(5), predating_version()),
            Outcome::Ok
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_outcome_for_leaves_base_cancelled_unchanged_for_older_plugin() {
        assert_eq!(
            map_outcome_for(&Outcome::Cancelled, predating_version()),
            Outcome::Cancelled
        );
    }

    /// REQ: ABI-028
    #[test]
    fn map_reason_for_never_delivers_crash_restart() {
        assert_eq!(
            map_reason_for(DeactivationReason::CrashRestart, host_version()),
            None
        );
    }

    /// REQ: ABI-028
    #[test]
    fn map_reason_for_never_delivers_crash_restart_to_older_plugin() {
        assert_eq!(
            map_reason_for(DeactivationReason::CrashRestart, predating_version()),
            None
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_reason_for_keeps_shutdown_for_current_api() {
        assert_eq!(
            map_reason_for(DeactivationReason::Shutdown, host_version()),
            Some(DeactivationReason::Shutdown)
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_reason_for_keeps_revoked_for_current_api() {
        assert_eq!(
            map_reason_for(DeactivationReason::Revoked, host_version()),
            Some(DeactivationReason::Revoked)
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_reason_for_maps_newer_revoked_to_shutdown_for_older_plugin() {
        // A newer `deactivation-reason` is delivered as `shutdown` (section 5).
        assert_eq!(
            map_reason_for(DeactivationReason::Revoked, predating_version()),
            Some(DeactivationReason::Shutdown)
        );
    }

    /// REQ: ABI-029
    #[test]
    fn map_reason_for_maps_newer_granted_to_shutdown_for_older_plugin() {
        assert_eq!(
            map_reason_for(DeactivationReason::Granted, predating_version()),
            Some(DeactivationReason::Shutdown)
        );
    }

    /// REQ: ABI-029
    #[test]
    fn event_deliverable_true_for_known_event_to_current_api() {
        assert!(event_deliverable("tree-changed", host_version()));
    }

    /// REQ: ABI-029
    #[test]
    fn event_deliverable_true_for_appended_event_to_current_api() {
        // `screen-changed` is in the appended block of the event variant.
        assert!(event_deliverable("screen-changed", host_version()));
    }

    /// REQ: ABI-029
    #[test]
    fn event_deliverable_false_for_event_newer_than_older_plugin() {
        assert!(!event_deliverable("tree-changed", predating_version()));
    }

    /// REQ: ABI-029
    #[test]
    fn event_deliverable_false_for_unknown_event_case() {
        // An unknown case is not delivered to any plugin.
        assert!(!event_deliverable("no-such-event", host_version()));
    }

    /// REQ: ABI-029
    #[test]
    fn event_deliverable_false_for_empty_event_name() {
        assert!(!event_deliverable("", host_version()));
    }
}
