//! Pure contract types of the Ilium plugin platform (contract `ilium:plugin@0.1.0`).
//!
//! This crate holds what the host, the CLI and the tests share about the plugin contract:
//! the Rust mirror of the WIT (`generated`, written by `tools/gen_plugin_api.py`), the manifest
//! model, grants, limits, lifecycle states, error codes and the object-safe kernel facade.
//! It does no I/O and has no runtime: no Wasmtime, no V8, no tokio, no files, no network.
//!
//! Contract validation and policy functions live here so runtime adapters and tests use
//! the same rules. Host execution, persistence and capability effects belong to the host
//! crate; generated wire types remain a direct mirror of the WIT contract.
//!
//! Manual: reference-wit.md, reference-abi-evolution.md, reference-errors-and-limits.md,
//! reference-manifest.md, reference-manifest-schema.md, reference-runtime-model.md.

pub mod abi;
pub mod cli;
pub mod client;
pub mod codes;
pub mod compat;
pub mod conformance;
pub mod dedup;
pub mod deprecation;
pub mod error;
pub mod generated;
pub mod grants;
pub mod http;
pub mod ids;
pub mod kernel;
pub mod lifecycle;
pub mod limits;
pub mod lock;
pub mod manifest;
pub mod mirror;
pub mod naming;
pub mod nesting;
pub mod package;
pub mod platform;
pub mod registry;
pub mod resolve;
pub mod side;
pub mod tier;
pub mod uitree;
pub mod version;

pub use error::{Code, CodeInfo, Severity};
pub use generated::types::{ErrorCode, PluginError};
pub use manifest::{Diagnostic, Stage};
pub use platform::{Platform, ProcessKind};
pub use side::Side;
pub use tier::Tier;
pub use version::{ApiVersion, CONTRACT};
