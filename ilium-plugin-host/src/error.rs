//! Host-level errors and their namespaced codes (reference-errors-and-limits.md `host.*`).

use std::borrow::Cow;

use ilium_plugin_api::{Code, Diagnostic};

/// A failure of the host itself, not of a plugin (reference-host-architecture.md).
#[derive(Debug, Clone, thiserror::Error)]
pub enum HostError {
    /// A host import panicked; the call is trapped (`host.import-panic`).
    #[error("a host import panicked")]
    ImportPanic,
    /// The engine reported an error (`host.engine-error`).
    #[error("engine error: {0}")]
    EngineError(String),
    /// The engine failed and cannot continue (`host.engine-failed`).
    #[error("the plugin engine failed")]
    EngineFailed,
    /// A release-embedded precompile is incompatible and has no safe fallback.
    #[error("the embedded precompiled component is incompatible")]
    EmbeddedIncompatible,
    /// The sandbox helper binary is missing (`host.helper-missing`).
    #[error("the plugin helper is missing")]
    HelperMissing,
    /// The grant store could not be read (`host.grants-unreadable`).
    #[error("the grant store is unreadable")]
    GrantsUnreadable,
    /// Shutdown exceeded its budget (`host.shutdown-timeout`).
    #[error("plugin shutdown timed out")]
    ShutdownTimeout,
    /// A stored package is corrupt (`package.corrupt`).
    #[error("a stored package is corrupt")]
    PackageCorrupt,
    /// A diagnostic from the API crate.
    #[error("{0}")]
    Diagnostic(Diagnostic),
}

impl HostError {
    /// The namespaced code of this error (reference-errors-and-limits.md).
    pub fn code(&self) -> Code {
        match self {
            HostError::ImportPanic => Code(Cow::Borrowed("host.import-panic")),
            HostError::EngineError(_) => Code(Cow::Borrowed("host.engine-error")),
            HostError::EngineFailed => Code(Cow::Borrowed("host.engine-failed")),
            HostError::EmbeddedIncompatible => Code(Cow::Borrowed("host.cwasm-incompatible")),
            HostError::HelperMissing => Code(Cow::Borrowed("host.helper-missing")),
            HostError::GrantsUnreadable => Code(Cow::Borrowed("host.grants-unreadable")),
            HostError::ShutdownTimeout => Code(Cow::Borrowed("host.shutdown-timeout")),
            HostError::PackageCorrupt => Code(Cow::Borrowed("package.corrupt")),
            HostError::Diagnostic(diagnostic) => diagnostic.code.clone(),
        }
    }
}

impl From<Diagnostic> for HostError {
    fn from(diagnostic: Diagnostic) -> HostError {
        HostError::Diagnostic(diagnostic)
    }
}
