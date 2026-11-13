//! Release-embedded precompiled components only. The catalogue is empty until the
//! release precompiler produces real component binaries and matching `.cwasm`.
//!
//! Never add filesystem or package bytes to this catalogue at runtime.

use wasmtime::component::Component;
use wasmtime::Engine;

use super::EmbeddedArtifact;
use crate::error::HostError;

struct BundledArtifact {
    name: &'static str,
    fingerprint: &'static str,
    wasm: &'static [u8],
    cwasm: &'static [u8],
}

// Release generation must replace this with 30 actual built artifacts. An empty
// catalogue is honest and fail-closed; manufacturing placeholder cwasm is unsafe.
const BUNDLED: &[BundledArtifact] = &[];

pub(super) fn bundled_artifacts() -> Vec<EmbeddedArtifact> {
    BUNDLED
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
    BUNDLED.iter().any(|item| {
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
