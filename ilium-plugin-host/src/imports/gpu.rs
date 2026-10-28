//! Host implementation of `ilium:host/gpu` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostGpu` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::gpu::{GpuKernel, HostGpu};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `gpu` interface.
#[derive(Debug)]
pub struct GpuImport;

impl HostGpu for GpuImport {
    fn run(
        &mut self,
        _kernel: GpuKernel,
        _input: Vec<u8>,
        _width: u32,
        _height: u32,
        _params_json: String,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::gpu::run")
    }

    fn available(&mut self) -> bool {
        unimplemented!("host::imports::gpu::available")
    }
}
