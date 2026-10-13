//! One side instance of a plugin: activation, calls and teardown (reference-runtime-model.md).

use ilium_plugin_api::generated::exports::hook_handler::{HookCall, HookReply};
use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::PluginError;

use crate::calls::Trap;

/// A call into a guest export other than events and render (reference-wit.md section 8).
#[derive(Debug, Clone)]
pub enum ExportCall {
    /// `hook-handler.handle`.
    Hook(HookCall),
    /// `job-runner.run`.
    Job {
        /// The job kind.
        kind: String,
        /// The job input.
        input: Vec<u8>,
    },
    /// `migrator.migrate`.
    Migrate {
        /// The data version being migrated from.
        from_version: u32,
        /// The stored bytes.
        data: Vec<u8>,
    },
}

/// The reply of an [`ExportCall`].
#[derive(Debug, Clone)]
pub enum ExportReply {
    /// Reply of a hook.
    Hook(HookReply),
    /// Reply of a job.
    Job(Result<Vec<u8>, PluginError>),
    /// Reply of a migration.
    Migrated(Result<Vec<u8>, PluginError>),
}

/// A running side instance (reference-runtime-model.md).
#[derive(Debug)]
pub struct PluginInstance {
    state: InstanceState,
}

impl PluginInstance {
    /// Calls `activate` (reference-runtime-model.md).
    pub fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        unimplemented!("host::instance::PluginInstance::activate")
    }

    /// Calls `deactivate`; the returned bytes are the saved state (reference-runtime-model.md).
    pub fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        unimplemented!("host::instance::PluginInstance::deactivate")
    }

    /// Delivers one batch of events (at most 256) and returns the submissions (ABI-008).
    pub fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        unimplemented!("host::instance::PluginInstance::on_events")
    }

    /// Renders a view (reference-wit.md section 8.3).
    pub fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        unimplemented!("host::instance::PluginInstance::render")
    }

    /// Calls any other export (reference-wit.md section 8).
    pub fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
        unimplemented!("host::instance::PluginInstance::call_export")
    }

    /// The instance state (reference-runtime-model.md).
    pub fn state(&self) -> InstanceState {
        self.state
    }
}
