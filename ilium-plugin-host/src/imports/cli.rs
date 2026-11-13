//! Host implementation of `ilium:host/cli` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCli` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::cli::{HostCli, PaneSelection};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::NodeId;

/// Host side of the `cli` interface.
#[derive(Debug)]
pub struct CliImport;

impl HostCli for CliImport {
    fn write_line(&mut self, _invocation: u64, _jsonl: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::cli::write_line")
    }

    fn write_error(&mut self, _invocation: u64, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::cli::write_error")
    }

    fn write_text(&mut self, _invocation: u64, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::cli::write_text")
    }

    fn write_stderr(&mut self, _invocation: u64, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::cli::write_stderr")
    }

    fn finish(&mut self, _invocation: u64, _exit_code: u8) {
        unimplemented!("host::imports::cli::finish")
    }

    fn read_stdin(&mut self, _invocation: u64, _max_bytes: u32) -> Result<u64, PluginError> {
        unimplemented!("host::imports::cli::read_stdin")
    }

    fn take_stdin(&mut self, _request: u64) -> Option<Vec<u8>> {
        unimplemented!("host::imports::cli::take_stdin")
    }

    fn is_tty(&mut self, _invocation: u64) -> (bool, bool) {
        unimplemented!("host::imports::cli::is_tty")
    }

    fn caller_pane(&mut self, _invocation: u64) -> Option<NodeId> {
        unimplemented!("host::imports::cli::caller_pane")
    }

    fn selected_panes(&mut self, _invocation: u64) -> Option<PaneSelection> {
        unimplemented!("host::imports::cli::selected_panes")
    }
}
