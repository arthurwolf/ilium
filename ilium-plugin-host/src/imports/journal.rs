//! Host implementation of `ilium:host/journal` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostJournal` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::journal::{HostJournal, JournalEntry};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{EpochMs, NodeId};

/// Host side of the `journal` interface.
#[derive(Debug)]
pub struct JournalImport;

impl HostJournal for JournalImport {
    fn log_event(
        &mut self,
        pane: Option<NodeId>,
        kind: String,
        payload_json: String,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::journal::log_event")
    }

    fn read(&mut self, pane: Option<NodeId>, since: EpochMs, limit: u32) -> Vec<JournalEntry> {
        unimplemented!("host::imports::journal::read")
    }
}
