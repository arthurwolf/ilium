//! Checks that compare a manifest with what a built component really imports and exports
//! (reference-manifest.md PKG-042..PKG-045, reference-abi-evolution.md section 4,
//! reference-host-runtime.md). Skel-api addition to plan 1.3 for ABI-006, ABI-018, ABI-022..ABI-024.

use super::{Diagnostic, Manifest, ValidationContext};
use crate::side::Side;

/// What a component declares, read from its binary by the host or the CLI
/// (reference-abi-evolution.md "The version appears in three places").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentSummary {
    /// Imported interfaces as `package:name/interface@version`, for example
    /// `ilium:host/tree@0.1.0` or `wasi:sockets/tcp@0.2.0` (reference-host-runtime.md).
    pub imports: Vec<String>,
    /// Exported interface names without package, for example `lifecycle` or `detector`
    /// (reference-manifest.md "Components and exports").
    pub exports: Vec<String>,
    /// Payloads of every `ilium:api-version` custom section, in file order (ABI-004).
    pub api_sections: Vec<Vec<u8>>,
    /// Unstable items the component imports, as feature names (reference-abi-evolution.md section 3).
    pub unstable_imports: Vec<String>,
}

/// Compares one side's component with the manifest: export list (PKG-043), API section
/// (ABI-004), imports against grants (ABI-023), forbidden WASI (`component.wasi-forbidden`)
/// and unstable imports (ABI-022). Returns every finding in refusal order.
pub fn check_component(
    _manifest: &Manifest,
    _side: Side,
    _component: &ComponentSummary,
    _context: &ValidationContext,
) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::check_component")
}
