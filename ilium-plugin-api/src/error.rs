//! Error codes, namespaced codes and the code registry (reference-errors-and-limits.md).

use std::borrow::Cow;

pub use crate::generated::types::{ErrorCode, PluginError};

/// Severity of a diagnostic (reference-check-and-lint-rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Refuses the check or install (reference-check-and-lint-rules.md).
    Error,
    /// Advisory, written `warn` (reference-check-and-lint-rules.md).
    Warning,
    /// Informational line (reference-check-and-lint-rules.md).
    Info,
}

impl Severity {
    /// Every severity, most severe first (reference-check-and-lint-rules.md).
    pub const ALL: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Info];

    /// `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        }
    }
}

impl std::fmt::Display for Severity {
    /// `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Severity {
    type Err = UnknownName;

    /// Parses `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    fn from_str(text: &str) -> Result<Severity, UnknownName> {
        Severity::ALL
            .into_iter()
            .find(|severity| severity.as_str() == text)
            .ok_or_else(|| UnknownName::new("severity", text))
    }
}

/// A namespaced code such as "manifest.grant-side" (reference-errors-and-limits.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Code(pub Cow<'static, str>);

impl Code {
    /// A code from one of the constants of [`crate::codes`] (reference-errors-and-limits.md).
    pub const fn from_static(code: &'static str) -> Code {
        Code(Cow::Borrowed(code))
    }

    /// The code text (reference-errors-and-limits.md).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the first `.`, for example `manifest` (reference-errors-and-limits.md).
    pub fn namespace(&self) -> &str {
        unimplemented!("api::error::Code::namespace")
    }

    /// The `error-code` a plugin sees for this code (reference-errors-and-limits.md).
    pub fn base(&self) -> ErrorCode {
        unimplemented!("api::error::Code::base")
    }
}

impl std::fmt::Display for Code {
    /// The code text (reference-errors-and-limits.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Longest `plugin-error.message` the API produces, in characters (test plan 1.3, "API").
pub const MESSAGE_MAX_CHARS: usize = 256;

impl PluginError {
    /// API: message capped at 256 characters (reference-errors-and-limits.md "The plugin error record").
    pub fn new(code: ErrorCode, message: impl Into<String>) -> PluginError {
        unimplemented!("api::error::PluginError::new")
    }

    /// An error whose message starts with `<extension code>: ` (reference-errors-and-limits.md
    /// "Extension point codes").
    pub fn with_prefix(code: ErrorCode, extension_code: &Code, message: &str) -> PluginError {
        unimplemented!("api::error::PluginError::with_prefix")
    }

    /// The namespaced code at the start of the message, if any (reference-errors-and-limits.md).
    pub fn extension_code(&self) -> Option<Code> {
        unimplemented!("api::error::PluginError::extension_code")
    }
}

/// Every registered namespaced code with its base code (reference-errors-and-limits.md).
pub fn registry() -> &'static [CodeInfo] {
    unimplemented!("api::error::registry")
}

/// One entry of the code registry (reference-errors-and-limits.md).
#[derive(Debug, Clone)]
pub struct CodeInfo {
    /// The namespaced code, for example `trap.deadline`.
    pub code: &'static str,
    /// The `error-code` a plugin sees.
    pub base: ErrorCode,
    /// Whether a retry can succeed (reference-errors-and-limits.md, "Retry" column).
    pub retryable: bool,
    /// The manual page that defines the code.
    pub page: &'static str,
}

/// The closed list of refusal reason words, in table order (reference-errors-and-limits.md
/// "Refusal reasons"). For `not-permitted`, `quota-exceeded` and `network` the message starts
/// with `<reason>: `.
pub const REFUSAL_REASONS: [&str; 15] = [
    "declined",
    "revoked",
    "not-granted",
    "scope",
    "policy",
    "offline",
    "private-address",
    "reserved",
    "downgrade",
    "not-user-initiated",
    "file-type",
    "unavailable",
    "response-size",
    "stream-size",
    "tls",
];

/// The `error-code` of a refusal reason word (reference-errors-and-limits.md "Refusal reasons").
pub fn refusal_reason_code(reason: &str) -> Option<ErrorCode> {
    unimplemented!("api::error::refusal_reason_code")
}

/// A name that is not one of the closed set of a `FromStr` enum of this crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} name {name:?}")]
pub struct UnknownName {
    /// What was parsed, for example `tier`.
    pub kind: &'static str,
    /// The text that matched no name.
    pub name: String,
}

impl UnknownName {
    /// A new error for `kind` and the offending `name`.
    pub fn new(kind: &'static str, name: &str) -> UnknownName {
        UnknownName {
            kind,
            name: name.to_owned(),
        }
    }
}
