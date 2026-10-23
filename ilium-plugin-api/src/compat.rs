//! What an older plugin sees from a newer host (reference-abi-evolution.md section 5).
//! Skel-api addition to plan 1.3 for ABI-028 and ABI-029.

use crate::generated::types::{DeactivationReason, Event, Intent, Outcome};
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
pub fn case_since(family: CaseFamily, case: &str) -> Option<ApiVersion> {
    let cases = match family {
        CaseFamily::Event => Event::CASE_NAMES,
        CaseFamily::Intent => Intent::CASE_NAMES,
        CaseFamily::Outcome => Outcome::CASE_NAMES,
        CaseFamily::DeactivationReason => DeactivationReason::CASE_NAMES,
    };
    // Every currently generated case belongs to the first frozen contract.
    // Future additions must carry their own introduction version here.
    cases.contains(&case).then_some(ApiVersion { major: 0, minor: 1, patch: 0 })
}

/// ABI-029: the outcome as a plugin of `plugin_api` receives it; a newer case maps to the
/// nearest older one (`ok-epoch` to `ok`).
pub fn map_outcome_for(outcome: &Outcome, plugin_api: ApiVersion) -> Outcome {
    if matches!(outcome, Outcome::OkEpoch(_))
        && case_since(CaseFamily::Outcome, "ok-epoch").is_some_and(|since| plugin_api < since)
    {
        Outcome::Ok
    } else {
        outcome.clone()
    }
}

/// ABI-028, ABI-029: the reason as a plugin of `plugin_api` receives it; a newer reason is
/// `shutdown`, and `crash-restart` is never delivered (`None`).
pub fn map_reason_for(
    reason: DeactivationReason,
    plugin_api: ApiVersion,
) -> Option<DeactivationReason> {
    if reason == DeactivationReason::CrashRestart { return None; }
    if case_since(CaseFamily::DeactivationReason, reason.as_str()).is_some_and(|since| plugin_api < since) {
        Some(DeactivationReason::Shutdown)
    } else { Some(reason) }
}

/// ABI-029: whether an event case reaches a plugin of `plugin_api`; a newer case is not
/// delivered and not counted in `events-dropped`.
pub fn event_deliverable(event_case: &str, plugin_api: ApiVersion) -> bool {
    case_since(CaseFamily::Event, event_case).is_some_and(|since| plugin_api >= since)
}
