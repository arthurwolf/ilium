//! Guest call classes, deadlines and traps (reference-runtime-model.md, reference-errors-and-limits.md).

use std::collections::BTreeMap;
use std::time::Duration;

use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::Code;

/// The class of a call into a guest; each has its own deadline (reference-runtime-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallClass {
    /// `on-events`.
    Event,
    /// A command handler.
    Command,
    /// `render`.
    Render,
    /// An annotator.
    Annotate,
    /// An animation frame.
    Frame,
    /// A detector `identify` or `classify`.
    Detector,
    /// `activate`.
    Activate,
    /// `deactivate`.
    Deactivate,
    /// A data migration.
    Migrate,
    /// A job run.
    Job,
}

/// The limits of one plugin after clamping to its tier (reference-manifest.md PKG-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLimits(pub BTreeMap<LimitKey, u64>);

/// A guest call that ended abnormally (reference-errors-and-limits.md `trap.*`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("trap {}: {interface}.{function}", .code.0)]
pub struct Trap {
    /// The `trap.*` code.
    pub code: Code,
    /// The interface being called.
    pub interface: String,
    /// The function being called.
    pub function: String,
}

/// The deadline of a call class under the resolved limits (reference-runtime-model.md).
pub fn deadline_for(class: CallClass, limits: &ResolvedLimits) -> Duration {
    unimplemented!("host::calls::deadline_for")
}
