//! An in-process guest that bypasses Wasmtime, for dispatcher and scheduler tests.

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::{Code, PluginError};
use std::borrow::Cow;

use crate::calls::Trap;
use crate::instance::{ExportCall, ExportReply};

/// A guest written in Rust and called directly by the harness.
pub trait NativeGuest: Send {
    /// `activate`; the default accepts.
    fn activate(&mut self, activation: Activation) -> Result<(), PluginError> {
        Ok(())
    }

    /// `deactivate`; the default saves no state.
    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }

    /// `on-events`: the submissions to return for one batch.
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap>;

    /// `render`; the default has no view export and traps with `trap.abi`.
    fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        Err(missing_export("render"))
    }

    /// Any other export; the default has none and traps with `trap.abi`.
    fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
        Err(missing_export("call-export"))
    }
}

fn missing_export(function: &str) -> Trap {
    Trap {
        code: Code(Cow::Borrowed("trap.abi")),
        interface: "native-guest".to_owned(),
        function: function.to_owned(),
    }
}
