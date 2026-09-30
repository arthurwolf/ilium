//! Registry of WGSL sources, included only under the `gpu` feature.

use ilium_ambient::gpu::GpuKernel;

/// WGSL source for a kernel, if it has one. Kernels without a port report
/// `None` and `run` fails with a clear message.
pub(crate) fn shader_source(kernel: GpuKernel) -> Option<&'static str> {
    match kernel {
        GpuKernel::FbmClouds => Some(include_str!("shaders/fbm_clouds.wgsl")),
        GpuKernel::DitheredWaves | GpuKernel::DithrPatterns => None,
    }
}

/// Built-in gradient kernel used by the crate's tests.
#[cfg(test)]
pub(crate) const SELFTEST_SOURCE: &str = include_str!("shaders/selftest.wgsl");
