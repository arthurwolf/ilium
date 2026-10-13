//! Dependency and service resolution across a set of packages (reference-manifest.md PKG-650..660).

use ilium_plugin_api::manifest::{Diagnostic, InstalledSummary, Manifest};
use ilium_plugin_api::ApiVersion;

/// The order in which packages activate, with the service providers each consumer uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPlan {
    /// Plugin ids in activation order.
    pub activation_order: Vec<String>,
}

/// Resolves requirements; a cycle is `manifest.requires-cycle` (PKG-654).
pub fn resolve(
    packages: &[Manifest],
    installed: &[InstalledSummary],
    host: ApiVersion,
) -> Result<ResolutionPlan, Vec<Diagnostic>> {
    unimplemented!("host::resolve::resolve")
}
