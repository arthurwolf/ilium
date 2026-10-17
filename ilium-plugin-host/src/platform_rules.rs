//! Platform rules (reference-platform-support.md): sandbox availability, path normalisation,
//! tick gaps, process termination and precompiled artifact selection.

use std::time::Duration;

use ilium_plugin_api::error::Diagnostic;
use ilium_plugin_api::platform::Platform;
use ilium_plugin_api::Tier;

/// Whether the tier's sandbox is available on the platform; `sandbox.unavailable` otherwise.
pub fn check_sandbox(platform: Platform, tier: Tier) -> Result<(), Diagnostic> {
    unimplemented!("host::platform_rules::check_sandbox")
}

/// Normalises a plugin-supplied path to the platform's form.
pub fn normalize_plugin_path(platform: Platform, path: &str) -> String {
    unimplemented!("host::platform_rules::normalize_plugin_path")
}

/// Whether a gap between scheduler ticks is treated as a suspend and not charged.
pub fn tick_gap_ignored(gap: Duration) -> bool {
    unimplemented!("host::platform_rules::tick_gap_ignored")
}

/// How child processes are terminated on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminationKind {
    /// SIGTERM, then SIGKILL after the grace.
    SigtermThenSigkill,
    /// A Windows job object.
    JobObject,
}

/// The termination plan of a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationPlan {
    /// The mechanism.
    pub kind: TerminationKind,
    /// The grace before the forced kill.
    pub grace: Duration,
    /// Children are killed with the process (job object).
    pub children_in_job: bool,
}

/// The termination plan of a platform.
pub fn termination_plan(platform: Platform) -> TerminationPlan {
    unimplemented!("host::platform_rules::termination_plan")
}

/// The target triples the host ships precompiled artifacts for.
pub fn shipped_triples() -> Vec<String> {
    unimplemented!("host::platform_rules::shipped_triples")
}

/// One artifact of a package: precompiled for a triple, or the portable module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WasmArtifact {
    /// A precompiled `.cwasm` for a target triple.
    Cwasm(String),
    /// The portable `.wasm` module.
    Wasm,
}

impl WasmArtifact {
    /// A precompiled artifact for `triple`.
    pub fn cwasm(triple: &str) -> WasmArtifact {
        unimplemented!("host::platform_rules::WasmArtifact::cwasm")
    }

    /// The portable module.
    pub fn wasm() -> WasmArtifact {
        unimplemented!("host::platform_rules::WasmArtifact::wasm")
    }
}

/// The artifact chosen for the host triple and whether a fallback was logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The artifact to load.
    pub artifact: WasmArtifact,
    /// A fallback to the portable module was logged.
    pub logged_fallback: bool,
    /// The log code of the fallback (empty when none).
    pub log_code: String,
}

/// Picks the artifact for `host_triple` from the package's artifacts.
pub fn select_precompiled(host_triple: &str, artifacts: &[WasmArtifact]) -> Selected {
    unimplemented!("host::platform_rules::select_precompiled")
}
