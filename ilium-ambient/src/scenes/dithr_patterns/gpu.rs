//! GPU seam of the dithr patterns scene.
//!
//! [`build_job`] describes one frame for the `DithrPatterns` kernel. The kernel
//! uniforms arrive with the shader port; until then the vector is empty, a
//! real runner reports that as an error, and the scene keeps drawing its
//! software output.
//!
//! Uniform layout (one `f32` per index, mirrored by the WGSL uniform struct):
//! none defined yet. Adding the first uniform means adding its index here and
//! to the shader in the same change.

use super::settings::DithrPatternsSettings;
use crate::gpu::{GpuJob, GpuKernel};

pub(crate) fn build_job(
    _settings: &DithrPatternsSettings,
    width: usize,
    height: usize,
    _time_seconds: f64,
) -> GpuJob {
    GpuJob {
        kernel: GpuKernel::DithrPatterns,
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        uniforms: Vec::new(),
    }
}
