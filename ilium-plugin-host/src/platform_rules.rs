//! Platform rules (reference-platform-support.md): sandbox availability, path normalisation,
//! tick gaps, process termination and precompiled artifact selection.

use std::time::Duration;

use ilium_plugin_api::platform::Platform;
use ilium_plugin_api::Diagnostic;
use ilium_plugin_api::Tier;

/// Platform eligibility for the tier; `sandbox.unavailable` on unsupported systems.
/// Linux helper startup must separately validate the kernel sandbox prerequisites.
pub fn check_sandbox(platform: Platform, tier: Tier) -> Result<(), Diagnostic> {
    if tier == Tier::T1 || platform == Platform::Linux {
        return Ok(());
    }
    Err(Diagnostic::error(
        ilium_plugin_api::error::Code::from_static("sandbox.unavailable"),
        "",
        format!("Sandbox unavailable on {platform} for {tier}"),
    ))
}

/// Normalises a plugin-supplied path to the platform's form.
pub fn normalize_plugin_path(platform: Platform, path: &str) -> String {
    match platform {
        Platform::Windows => path.replace('\\', "/"),
        Platform::Linux | Platform::MacOs => path.to_owned(),
    }
}

/// Whether a gap between scheduler ticks is treated as a suspend and not charged.
pub fn tick_gap_ignored(gap: Duration) -> bool {
    gap > Duration::from_secs(1)
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
    let children_in_job = platform == Platform::Windows;
    TerminationPlan {
        kind: if children_in_job {
            TerminationKind::JobObject
        } else {
            TerminationKind::SigtermThenSigkill
        },
        grace: Duration::from_secs(2),
        children_in_job,
    }
}

/// The target triples the host ships precompiled artifacts for.
pub fn shipped_triples() -> Vec<String> {
    [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
        Self::Cwasm(triple.to_owned())
    }

    /// The portable module.
    pub fn wasm() -> WasmArtifact {
        Self::Wasm
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

/// A package cannot fall back to a portable component it does not contain.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactSelectionError {
    /// Neither a compatible compiled artifact nor a portable component exists.
    #[error("package has no compatible artifact or portable WebAssembly component")]
    MissingPortableComponent,
}

/// Picks the artifact for `host_triple` from the package's artifacts.
pub fn select_precompiled(
    host_triple: &str,
    artifacts: &[WasmArtifact],
) -> Result<Selected, ArtifactSelectionError> {
    if let Some(artifact) = artifacts.iter().find(|artifact| {
        matches!(artifact, WasmArtifact::Cwasm(triple) if triple == host_triple)
    }) {
        return Ok(Selected {
            artifact: artifact.clone(),
            logged_fallback: false,
            log_code: String::new(),
        });
    }
    if !artifacts.contains(&WasmArtifact::Wasm) {
        return Err(ArtifactSelectionError::MissingPortableComponent);
    }
    Ok(Selected {
        artifact: WasmArtifact::Wasm,
        logged_fallback: true,
        log_code: "host.cwasm-incompatible".to_owned(),
    })
}
