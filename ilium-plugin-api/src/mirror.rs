//! Mirroring a registry closure into a target store (PKG-817).

use crate::error::PluginError;
use crate::registry::Registry;

/// The store a mirror writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorTarget;

impl MirrorTarget {
    /// A store that holds nothing.
    pub fn empty() -> MirrorTarget {
        unimplemented!("api::mirror::MirrorTarget::empty")
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        unimplemented!("api::mirror::MirrorTarget::is_empty")
    }
}

/// Mirrors the allow-listed ids and their dependency closure from `source`; an incomplete closure
/// is `mirror.closure-incomplete` and nothing is written (PKG-817).
pub fn mirror(
    _source: &Registry,
    _allow: &[&str],
    _target: &mut MirrorTarget,
) -> Result<(), PluginError> {
    unimplemented!("api::mirror::mirror")
}
