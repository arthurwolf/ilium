//! Release-embedded components only. The catalogue is the `&'static` table the release
//! binary installs with `install_bundled`; it is empty until then.
//!
//! Never add filesystem or package bytes to this catalogue at runtime.

use wasmtime::component::Component;
use wasmtime::Engine;

use super::EmbeddedArtifact;
use crate::error::HostError;

/// One component compiled into the release binary. `cwasm` is empty until the release
/// precompiler produces a matching precompile; the engine then compiles `wasm` instead.
#[derive(Debug, Clone, Copy)]
pub struct BundledArtifact {
    pub name: &'static str,
    pub fingerprint: &'static str,
    pub wasm: &'static [u8],
    pub cwasm: &'static [u8],
}

// The host crate cannot depend on the crate that builds the components (that crate's build
// compiles the plugins, which depend on the SDK), so the binary that links both installs the
// catalogue once at start. Until then the catalogue is empty, which is fail-closed.
static BUNDLED: std::sync::OnceLock<&'static [BundledArtifact]> = std::sync::OnceLock::new();

/// Installs the static catalogue of the release binary. Only the first call takes effect;
/// returns false when a catalogue was already installed.
pub fn install_bundled(catalogue: &'static [BundledArtifact]) -> bool {
    BUNDLED.set(catalogue).is_ok()
}

fn bundled() -> &'static [BundledArtifact] {
    BUNDLED.get().copied().unwrap_or(&[])
}

pub(super) fn bundled_artifacts() -> Vec<EmbeddedArtifact> {
    bundled()
        .iter()
        .map(|item| EmbeddedArtifact {
            name: item.name.to_owned(),
            fingerprint: item.fingerprint.to_owned(),
            wasm: item.wasm.to_vec(),
            cwasm: item.cwasm.to_vec(),
        })
        .collect()
}

pub(super) fn is_bundled(artifact: &EmbeddedArtifact) -> bool {
    bundled().iter().any(|item| {
        item.name == artifact.name
            && item.fingerprint == artifact.fingerprint
            && item.wasm == artifact.wasm
            && item.cwasm == artifact.cwasm
    })
}

pub(super) fn deserialize_verified(
    engine: &Engine,
    artifact: &EmbeddedArtifact,
) -> Result<Component, HostError> {
    if !is_bundled(artifact) || artifact.cwasm.is_empty() {
        return Err(HostError::EmbeddedIncompatible);
    }
    // SAFETY: the bytes must match an artifact compiled into this release's
    // static catalogue. The caller has also checked the engine compatibility
    // fingerprint. Neither package nor disk-cache bytes reach this operation.
    unsafe { Component::deserialize(engine, &artifact.cwasm) }
        .map_err(|error| HostError::EngineError(error.to_string()))
}
