//! The plugin host of Ilium (contract `ilium:plugin@0.1.0`).
//!
//! The host embeds the WebAssembly engine, owns the per-plugin queues and the dispatcher, the
//! package store and installer, the lifecycle manager, the host imports of every WIT interface
//! and the `ilium plugin ...` commands. It reaches the application only through the
//! object-safe `ilium_plugin_api::kernel::KernelFacade`; it has no edge to `ilium-core`,
//! `ilium-transport` or `ilium-ipc` (test plan section 1.2, reference-host-architecture.md).
//!
//! Skeleton state: data types, constants and closed enums are complete. Every function with
//! behaviour is `unimplemented!("host::<path>")` until the implementation phase.
//!
//! Manual: reference-host-architecture.md, reference-host-runtime.md, reference-runtime-model.md,
//! reference-packaging.md, reference-cli.md.

// The skeleton functions name their parameters for the implementation that replaces the
// `unimplemented!` bodies, and private fields wait for the code that reads them; until then both
// are unused by design (test plan 1.3).
#![allow(unused_variables, dead_code)]

pub mod audit;
pub mod autoupdate;
pub mod calls;
pub mod cli;
pub mod conformance_hooks;
pub mod devtools;
pub mod dispatch;
pub mod engine;
pub mod error;
pub mod extensions;
pub mod helper_client;
pub mod imports;
pub mod install;
pub mod instance;
pub mod ipc;
pub mod lifecycle;
pub mod linker;
pub mod package;
pub mod policy;
pub mod registry;
pub mod registry_client;
pub mod render;
pub mod resolve;
pub mod scheduler;
pub mod services;
pub mod settings;
pub mod approval;
pub mod consent;
pub mod limits_rules;
pub mod platform_rules;
pub mod review;
pub mod secrets;
pub mod session;
pub mod signing;
pub mod storage;
pub mod stores;
pub mod sync;
pub mod telemetry;
#[cfg(feature = "testing")]
pub mod testing;

pub use error::HostError;
