//! Install and check diagnostics (reference-manifest-schema.md "Error catalogue",
//! reference-check-and-lint-rules.md).

use crate::error::{Code, Severity};

/// One finding: code, severity, JSON pointer, message and install stage
/// (reference-manifest.md PKG-076, reference-check-and-lint-rules.md "Key path").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Namespaced code, for example `manifest.schema` (reference-manifest-schema.md).
    pub code: Code,
    /// `error`, `warn` or `info` (reference-check-and-lint-rules.md DEV-202).
    pub severity: Severity,
    /// JSON Pointer into `manifest.json`, empty when the finding has no value (reference-manifest.md PKG-076).
    pub pointer: String,
    /// Message from the catalogue template (reference-manifest-schema.md).
    pub message: String,
    /// Install stage that reports the code, when it has one (reference-manifest-schema.md).
    pub stage: Option<Stage>,
}

impl Diagnostic {
    /// An `error` diagnostic without a stage (reference-manifest.md PKG-076).
    pub fn error(code: Code, pointer: impl Into<String>, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            severity: Severity::Error,
            pointer: pointer.into(),
            message: message.into(),
            stage: None,
        }
    }
}

impl std::fmt::Display for Diagnostic {
    /// `<code> <pointer>: <message>` (reference-check-and-lint-rules.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.pointer.is_empty() {
            write!(formatter, "{}: {}", self.code, self.message)
        } else {
            write!(
                formatter,
                "{} {}: {}",
                self.code, self.pointer, self.message
            )
        }
    }
}

/// Install stage number of the error catalogue: 1 archive, 2 parse, 3 schema, and so on
/// (reference-manifest-schema.md "Error catalogue").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stage(pub u8);
