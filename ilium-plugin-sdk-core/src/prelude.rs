//! `use ilium_plugin_sdk::prelude::*;` (rust-sdk.md "Prelude").

pub use crate::dedup;
pub use crate::generated::types::{
    Activation, ActionKind, ActionValue, Area, DeactivationReason, ErrorCode, Event, Intent,
    NodeId, Outcome, PluginError, PluginId, RequestRef, Submission, UiTree, ViewTarget,
};
pub use crate::plugin::{
    Annotator, CatalogProvider, Detector, Handler, HookHandler, InferenceProvider, JobRunner,
    Lifecycle, MenuProvider, Plugin, Renderer, ServiceProvider, TokenProvider, Views,
    VirtualRowsProvider, host,
};
pub use crate::task;
pub use crate::ui::build::{NodeBuilder, Tree};
pub use crate::{events, intents};
