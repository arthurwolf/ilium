//! SDK core for Ilium plugins (manual: rust-sdk.md). Native mode runs the plugin as a plain library
//! under the testkit; component mode compiles it to wasm32. The skeleton targets native mode.

pub mod dedup;
pub mod events;
pub mod generated;
pub mod intents;
pub mod log;
pub mod plugin;
pub mod prelude;
pub mod request;
pub mod task;
pub mod ui;

pub use plugin::{
    Annotator, CatalogProvider, Detector, Handler, HookHandler, HostApi, HostCalls,
    InferenceProvider, JobRunner, Lifecycle, LifecycleState, MenuProvider, Migrator, NativeEntry,
    NativeRenderer, Plugin, Renderer, SceneRenderer, ServiceProvider, SettingsValidate,
    TokenProvider, Views, VirtualRowsProvider, host, set_host,
};

/// Types of `ilium:types`, one module per WIT interface (rust-sdk.md "Module map").
pub mod types {
    pub use crate::generated::types::*;
}
/// Host interfaces of `ilium:host` (rust-sdk.md "Module map").
pub mod host {
    pub use crate::generated::host::*;
}
/// Primitives of `ilium:primitives`.
pub mod primitives {
    pub use crate::generated::primitives::*;
}
/// Animation interfaces of `ilium:anim`.
pub mod anim {
    pub use crate::generated::anim::*;
}
/// Export traits of `ilium:exports`.
pub mod exports {
    pub use crate::generated::exports::*;
}
