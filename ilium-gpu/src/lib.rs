//! Optional GPU backend for the ambient scenes.
//!
//! The crate is always linked, but `wgpu` (and with it `naga` and every WGSL
//! shader) is compiled only with `--features gpu`. Without the feature,
//! [`start_probe`] just reports `NotCompiled`. With it, a single background
//! thread enumerates adapters, classifies them with the pure
//! [`diagnose::diagnose`], creates a device and publishes the result through
//! `ilium_ambient::gpu::set_gpu_availability`.

pub mod diagnose;
pub mod facts;

#[cfg(feature = "gpu")]
mod shaders;
#[cfg(feature = "gpu")]
mod wgpu_backend;

use ilium_ambient::gpu::{set_gpu_availability, GpuAvailability, GpuRunner, GpuUnavailable};
use std::sync::{Arc, Once, OnceLock};

static PROBE_STARTED: Once = Once::new();
static RUNNER: OnceLock<Arc<dyn GpuRunner>> = OnceLock::new();

/// Starts the one-time background GPU probe. Idempotent: later calls do
/// nothing. Never blocks.
pub fn start_probe() {
    PROBE_STARTED.call_once(|| {
        #[cfg(feature = "gpu")]
        {
            let spawned = std::thread::Builder::new()
                .name("ilium-gpu-probe".to_string())
                .spawn(run_probe);
            if let Err(error) = spawned {
                tracing::warn!(%error, "could not spawn the GPU probe thread");
                set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::Failed(
                    format!("could not start the GPU probe thread: {error}"),
                )));
            }
        }
        #[cfg(not(feature = "gpu"))]
        set_gpu_availability(GpuAvailability::Unavailable(GpuUnavailable::NotCompiled));
    });
}

/// The shared runner; `Some` only once the probe has found a usable GPU.
pub fn runner() -> Option<Arc<dyn GpuRunner>> {
    match ilium_ambient::gpu::gpu_availability() {
        GpuAvailability::Ready { .. } => RUNNER.get().cloned(),
        GpuAvailability::Unavailable(_) => None,
    }
}

#[cfg(feature = "gpu")]
fn run_probe() {
    let outcome = wgpu_backend::probe();
    if let Some(device_runner) = outcome.runner {
        // Only the probe thread ever sets this, once; a failed set would mean
        // a runner is already published, which is equally fine.
        let shared: Arc<dyn GpuRunner> = device_runner;
        let _ = RUNNER.set(shared);
    }
    tracing::info!(availability = ?outcome.availability, "GPU probe finished");
    set_gpu_availability(outcome.availability);
}

#[cfg(all(test, not(feature = "gpu")))]
mod tests {
    use super::*;

    #[test]
    fn default_build_reports_not_compiled_and_has_no_runner() {
        start_probe();
        start_probe();
        assert_eq!(
            ilium_ambient::gpu::gpu_availability(),
            GpuAvailability::Unavailable(GpuUnavailable::NotCompiled)
        );
        assert!(runner().is_none());
    }
}
