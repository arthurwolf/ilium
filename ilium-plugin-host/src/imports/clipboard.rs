//! Host implementation of `ilium:host/clipboard` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClipboard` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::clipboard::HostClipboard;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `clipboard` interface.
#[derive(Debug)]
pub struct ClipboardImport;

impl HostClipboard for ClipboardImport {
    fn read_text(&mut self) -> Option<String> {
        unimplemented!("host::imports::clipboard::read_text")
    }

    fn write_text(&mut self, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::clipboard::write_text")
    }
}
