//! Host implementation of `ilium:host/process` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostProcess` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::process::{
    ChildHandle, HostProcess, ProcessExit, ProcessSpec, SpawnSpec,
};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{ChildId, RequestId};

/// Host side of the `process` interface.
#[derive(Debug)]
pub struct ProcessImport;

impl HostProcess for ProcessImport {
    fn run(&mut self, spec: ProcessSpec) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::process::run")
    }

    fn take_exit(&mut self, request: RequestId) -> Option<ProcessExit> {
        unimplemented!("host::imports::process::take_exit")
    }

    fn spawn(&mut self, spec: SpawnSpec) -> Result<ChildHandle, PluginError> {
        unimplemented!("host::imports::process::spawn")
    }

    fn child_pid_hint(&mut self, handle: ChildHandle) -> u32 {
        unimplemented!("host::imports::process::child_pid_hint")
    }

    fn child_write_stdin(
        &mut self,
        handle: ChildHandle,
        bytes: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::process::child_write_stdin")
    }

    fn child_close_stdin(&mut self, handle: ChildHandle) {
        unimplemented!("host::imports::process::child_close_stdin")
    }

    fn child_signal(&mut self, handle: ChildHandle, signal: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::process::child_signal")
    }

    fn child_id(&mut self, handle: ChildHandle) -> ChildId {
        unimplemented!("host::imports::process::child_id")
    }
}
