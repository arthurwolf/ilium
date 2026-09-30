//! GPU seam of the fBm clouds scene.
//!
//! [`build_job`] describes one frame for the `FbmClouds` kernel. The kernel
//! uniforms arrive with the shader port; until then the vector is empty, a
//! real runner reports that as an error, and the scene keeps drawing its
//! software output.
//!
//! Uniform layout (one `f32` per index, mirrored by the WGSL uniform struct):
//! none defined yet. Adding the first uniform means adding its index here and
//! to the shader in the same change.

use super::settings::FbmCloudsSettings;
use crate::gpu::{GpuJob, GpuKernel};

pub(crate) fn build_job(
    _settings: &FbmCloudsSettings,
    width: usize,
    height: usize,
    _time_seconds: f64,
) -> GpuJob {
    GpuJob {
        kernel: GpuKernel::FbmClouds,
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        uniforms: Vec::new(),
    }
}
