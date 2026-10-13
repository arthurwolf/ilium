//! Manifest parsing and validation entry points (reference-manifest-schema.md,
//! packages.md "Install checks").

use super::{Diagnostic, Manifest};
use crate::platform::Platform;
use crate::tier::Tier;

/// Which command runs the checks (reference-check-and-lint-rules.md DEV-200).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckMode {
    /// `ilium plugin check` (reference-check-and-lint-rules.md).
    Check,
    /// The install path (reference-manifest-schema.md).
    Install,
    /// `ilium plugin dev` (packages.md).
    Dev,
}

/// What the validator knows about the machine (reference-manifest-schema.md).
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Tier of the package under check (security.md).
    pub tier: Tier,
    /// Which command runs the checks.
    pub mode: CheckMode,
    /// Host operating system.
    pub platform: Platform,
    /// Installed plugins, for dependency and conflict rules.
    pub installed: Vec<InstalledSummary>,
    /// Core plugins, for `replaces` (reference-manifest.md PKG-028).
    pub core_plugins: Vec<CorePluginSummary>,
    /// Unstable features this host accepts (reference-abi-evolution.md, ABI-022).
    pub unstable_features: Vec<String>,
}

/// One installed plugin as the validator sees it (reference-dependency-resolution.md).
#[derive(Debug, Clone)]
pub struct InstalledSummary {
    /// Plugin id.
    pub id: String,
    /// Installed version.
    pub version: String,
    /// Services it provides: (name, version).
    pub services: Vec<(String, String)>,
    /// Settings tabs it owns.
    pub settings_owner_tabs: Vec<String>,
}

/// One core plugin as the validator sees it (reference-manifest.md PKG-028, PKG-029).
#[derive(Debug, Clone)]
pub struct CorePluginSummary {
    /// Core plugin id.
    pub id: String,
    /// Whether a third party may replace it.
    pub replaceable: bool,
    /// Its replaceable surface.
    pub surface: Vec<String>,
}

/// Stage 2: parses `manifest.json` text, stopping at the first error (reference-manifest-schema.md).
pub fn parse_manifest(json: &str) -> Result<Manifest, Vec<Diagnostic>> {
    unimplemented!("api::manifest::parse_manifest")
}

/// Stage 3 and later: every rule of the manifest pages, all errors collected
/// (reference-manifest-schema.md).
pub fn validate(manifest: &Manifest, context: &ValidationContext) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::validate")
}

/// Parse plus validate, in install order, one refusal code per package for the API codes
/// (reference-abi-evolution.md, ABI-021).
pub fn validate_json(json: &str, context: &ValidationContext) -> Vec<Diagnostic> {
    unimplemented!("api::manifest::validate_json")
}

/// The JSON Schema of reference-manifest-schema.md, embedded as text.
pub fn json_schema() -> &'static str {
    unimplemented!("api::manifest::json_schema")
}
