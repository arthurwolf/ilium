//! Dependency resolution over registry sources (reference-dependency-resolution.md; PKG-804).

use crate::error::PluginError;
use crate::registry::Registry;

/// Resolves a plugin and its requirements without downloading packages.
#[derive(Debug, Clone)]
pub struct Resolver {
    /// The registries searched in order.
    pub sources: Vec<Registry>,
}

/// The plugins a resolution selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution;

impl Resolution {
    /// The ids of the selected plugins.
    pub fn plugin_ids(&self) -> Vec<String> {
        unimplemented!("api::resolve::Resolution::plugin_ids")
    }
}

impl Resolver {
    /// A resolver over `sources`.
    pub fn new(_sources: Vec<Registry>) -> Resolver {
        unimplemented!("api::resolve::Resolver::new")
    }

    /// Resolves `id` at `range`; more than 256 plugins or a chain deeper than 32 is
    /// `resolve.graph-too-large`, before any download (PKG-804).
    pub fn resolve(&self, _id: &str, _range: &str) -> Result<Resolution, PluginError> {
        unimplemented!("api::resolve::Resolver::resolve")
    }
}
